//! SmartBill REST API adapter — Wave C W3.
//!
//! # Architecture: sync parse vs async fetch
//!
//! The `ImportAdapter` trait is SYNC. Network I/O is separated into an async
//! helper (`fetch_smartbill`) that the W4 Tauri command calls BEFORE handing
//! the raw JSON bytes to `SmartBillRestAdapter::parse()`.
//!
//! Flow in W4:
//!   1. W4 command reads credentials (username from `settings`, token from keychain).
//!   2. Calls `fetch_smartbill(company_id, creds, kind).await` → `Vec<u8>` (raw JSON).
//!   3. Passes `ImportInput::Bytes(bytes)` to `SmartBillRestAdapter.parse()`.
//!   4. `parse()` calls `map_stocks_json()` and returns `StagedData`. No network.
//!
//! # SmartBill REST hard limitation
//!
//! The SmartBill REST API has **no confirmed bulk GET endpoint for partners or
//! issued invoices** (2025 state; only `POST /invoice` create, `GET /stocks`,
//! `GET /series`, `GET /tax` are documented as read-accessible). Therefore
//! REST import covers **products/stock + reference data (series, tax) ONLY**.
//! Partners and invoices must come from the "Export pentru Saga" XML (W2).
//!
//! Additional gotcha: SmartBill REST returns country as a **NAME** (e.g.
//! "Romania") while the Saga XML uses a country **CODE** (e.g. "RO").
//! W4/normalisation handles the divergence.
//!
//! # Stocks JSON response shape (MEDIUM confidence)
//!
//! Confirmed via @JsonAlias in smartbillclient4j Product.java:
//! - `productName`   → `StagedProduct.name`
//! - `productCode`   → `StagedProduct.code`
//! - `measuringUnit` → `StagedProduct.unit`
//!   (read uses `"measuringUnit"`; write uses `"measuringUnitName"` — intentionally different)
//! - `quantity`      → `StagedProduct.stock_qty`
//! - `isService`     → `StagedProduct.is_service`
//!   (LOW confidence — not in the Java client; present in some SDK docs; tolerated here)
//!
//! The response shape is an object with a top-level "list" key (UNVERIFIED)
//! wrapping an array of product objects, OR the array itself. The mapper
//! tries both shapes defensively.
//!
//! # Rate limit
//!
//! SmartBill enforces 3 requests/second. Exceeding it blocks the IP for 10
//! minutes. The fetcher inserts a small sleep between successive calls.

use serde_json::Value;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

use super::adapter::ImportAdapter;
use super::{canonical_cui, ImportInput, ParseCtx, SourceKind, StagedData, StagedProduct};

const SOURCE: &str = "SMARTBILL_REST";
const SMARTBILL_BASE: &str = "https://ws.smartbill.ro/SBORO/api";

// ─── Resource kinds the fetcher can retrieve ─────────────────────────────────

/// Which SmartBill read-endpoint to call.
#[derive(Debug, Clone, Copy)]
pub enum SmartBillResource {
    /// GET /stocks — product stock levels. The primary import resource.
    Stocks,
    /// GET /series?cif=&type= — document series (reference data).
    Series,
    /// GET /tax?cif= — VAT rates (reference data).
    Tax,
}

// ─── Credentials bundle ───────────────────────────────────────────────────────

/// Resolved SmartBill credentials, supplied by the W4 command.
///
/// The W4 command is responsible for reading `user` from the settings table
/// (`smartbill_user_{company_id}`) and `token` from the OS keychain
/// (`crate::anaf::keychain::get_smartbill_token`). Neither is persisted in
/// staging tables, logs, or this struct beyond the duration of the request.
pub struct SmartBillCreds {
    /// SmartBill account e-mail (HTTP Basic username).
    pub user: String,
    /// SmartBill API token (HTTP Basic password). Never logged.
    pub token: String,
}

// ─── Adapter ─────────────────────────────────────────────────────────────────

/// Parse-only adapter for pre-fetched SmartBill JSON bytes.
///
/// This adapter does NOT make network calls. It only maps a previously-fetched
/// `/stocks` (or `/series`/`/tax`) JSON response into `StagedData`.
pub struct SmartBillRestAdapter;

impl ImportAdapter for SmartBillRestAdapter {
    fn source(&self) -> SourceKind {
        SourceKind::SmartbillRest
    }

    /// Parse pre-fetched SmartBill JSON (`ImportInput::Bytes`) into `StagedData`.
    ///
    /// Only `Bytes` is supported in `parse()`. For `RestCreds` the W4 command
    /// must call `fetch_smartbill()` first and then pass the resulting bytes.
    fn parse(&self, input: &ImportInput, ctx: &ParseCtx) -> AppResult<StagedData> {
        let bytes = match input {
            ImportInput::Bytes(b) => b,
            ImportInput::RestCreds { .. } => {
                return Err(AppError::Validation(
                    "SmartBillRestAdapter::parse() nu acceptă RestCreds. \
                     Apelați fetch_smartbill() async în prealabil și transmiteți \
                     bytes-urile JSON ca ImportInput::Bytes."
                        .into(),
                ));
            }
            ImportInput::Files(_) => {
                return Err(AppError::Validation(
                    "SmartBillRestAdapter::parse() nu acceptă Files. \
                     Transmiteți bytes-urile JSON ca ImportInput::Bytes."
                        .into(),
                ));
            }
        };

        let json: Value = serde_json::from_slice(bytes)
            .map_err(|e| AppError::Validation(format!("SmartBill JSON invalid: {e}")))?;

        let mut out = StagedData::empty();
        out.products = map_stocks_json(&json, ctx, &mut out.warnings);
        Ok(out)
    }
}

// ─── Pure mapping function (unit-tested, no network) ─────────────────────────

/// Map a SmartBill `/stocks` JSON response to a `Vec<StagedProduct>`.
///
/// The response is either:
///   - An array at the root: `[{productName, ...}, ...]`
///   - An object with a `"list"` key: `{"list": [{...}], ...}` (UNVERIFIED shape)
///   - An object with a `"products"` key (alternative UNVERIFIED shape)
///
/// Unknown/missing fields produce a warning; they never cause a panic or `Err`.
/// Money/qty stored as String (Decimal-as-TEXT convention matching W1 types).
pub fn map_stocks_json(
    json: &Value,
    ctx: &ParseCtx,
    warnings: &mut Vec<String>,
) -> Vec<StagedProduct> {
    // Resolve the product array from the JSON structure.
    let array: &Vec<Value> = {
        if let Some(arr) = json.as_array() {
            arr
        } else if let Some(list) = json.get("list").and_then(|v| v.as_array()) {
            list
        } else if let Some(products) = json.get("products").and_then(|v| v.as_array()) {
            products
        } else if let Some(data) = json.get("data").and_then(|v| v.as_array()) {
            // Additional candidate key (UNVERIFIED)
            data
        } else {
            warnings.push(
                "SmartBill REST: răspunsul JSON nu conține un array de produse \
                 (așteptat la rădăcină sau în câmpul \"list\"/\"products\")."
                    .into(),
            );
            return vec![];
        }
    };

    if array.is_empty() {
        return vec![];
    }

    let mut products = Vec::with_capacity(array.len());

    for (i, item) in array.iter().enumerate() {
        let obj = match item.as_object() {
            Some(o) => o,
            None => {
                warnings.push(format!(
                    "SmartBill REST stocks[{i}]: elementul nu este un obiect JSON, ignorat."
                ));
                continue;
            }
        };

        // Helper: extract a string field, emitting a warning if absent.
        let get_str = |key: &str| -> Option<String> {
            obj.get(key)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty())
        };

        // productName → StagedProduct.name  (MEDIUM confidence field name)
        let name = get_str("productName");
        if name.is_none() {
            warnings.push(format!(
                "SmartBill REST stocks[{i}]: câmpul \"productName\" lipsește sau este gol."
            ));
        }

        // productCode → StagedProduct.code  (MEDIUM confidence field name)
        let code = get_str("productCode");

        // measuringUnit → StagedProduct.unit  (MEDIUM confidence; read uses "measuringUnit",
        // write uses "measuringUnitName" — intentionally different keys)
        let unit = get_str("measuringUnit");

        // quantity → StagedProduct.stock_qty  (MEDIUM confidence)
        let stock_qty = obj.get("quantity").map(|v| v.to_string());

        // isService → StagedProduct.is_service  (LOW confidence — not all SDK docs list it)
        let is_service = obj.get("isService").and_then(|v| v.as_bool());

        // Dedup key: product code (cross-system identity via SmartBill code)
        let dedup_key = code.clone();

        // Build dedup_key for contact link via company CUI
        let _ = ctx.company_cui_canonical; // used in W4 for issuer-side checks
        let _ = canonical_cui; // imported to keep the dep visible for W4

        // Serialise the raw object for audit.
        let raw_json = serde_json::to_string(item).unwrap_or_else(|_| "{}".to_string());

        products.push(StagedProduct {
            id: Uuid::now_v7().to_string(),
            source: SOURCE.to_string(),
            raw_json,
            source_code: code.clone(),
            name,
            unit,
            unit_price: None, // /stocks does not return price
            vat_rate: None,
            vat_category: None,
            code,
            barcode: None,
            stock_qty,
            is_service,
            dedup_key,
        });
    }

    products
}

// ─── Async HTTP fetcher (NOT unit-tested — no live calls in tests) ────────────

/// Fetch a SmartBill read-endpoint and return raw JSON bytes.
///
/// # Credentials
///
/// `creds` must be supplied by the W4 Tauri command, which reads:
/// - `creds.user`  — from `settings` table key `smartbill_user_{company_id}`
/// - `creds.token` — from OS keychain via `crate::anaf::keychain::get_smartbill_token`
///
/// The token is used as the HTTP Basic password and is **never** logged,
/// persisted in staging tables, or returned to the caller.
///
/// # Rate limit
///
/// SmartBill enforces 3 requests/second. A 400ms delay is inserted after each
/// successful fetch to stay safely under the limit. If a caller makes multiple
/// sequential calls, the aggregate rate stays well below 3/sec.
///
/// # Errors
///
/// Returns `AppError::Validation` for missing/empty credentials, and
/// `AppError::Other` for network failures or non-2xx HTTP responses.
pub async fn fetch_smartbill(
    company_id: &str,
    creds: &SmartBillCreds,
    kind: SmartBillResource,
) -> AppResult<Vec<u8>> {
    if creds.user.is_empty() || creds.token.is_empty() {
        return Err(AppError::Validation(
            "Credențialele SmartBill nu sunt configurate (user/token lipsesc).".into(),
        ));
    }

    let url = build_url(company_id, kind);

    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .basic_auth(&creds.user, Some(&creds.token))
        .send()
        .await
        .map_err(|e| AppError::Other(format!("SmartBill fetch error: {e}")))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| String::from("(corp de răspuns necitibil)"));
        return Err(AppError::Other(format!(
            "SmartBill API eroare {status}: {body}"
        )));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| AppError::Other(format!("SmartBill: citire răspuns eșuată: {e}")))?;

    // Honor the 3 req/sec rate limit: sleep 400ms after each successful fetch.
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;

    Ok(bytes.to_vec())
}

fn build_url(company_id: &str, kind: SmartBillResource) -> String {
    match kind {
        SmartBillResource::Stocks => {
            format!("{SMARTBILL_BASE}/stocks?cif={company_id}")
        }
        SmartBillResource::Series => {
            format!("{SMARTBILL_BASE}/series?cif={company_id}")
        }
        SmartBillResource::Tax => {
            format!("{SMARTBILL_BASE}/tax?cif={company_id}")
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx() -> ParseCtx<'static> {
        ParseCtx {
            company_cui_canonical: "12345678",
            column_map: None,
        }
    }

    // ── happy path: full /stocks object with root array ───────────────────────

    #[test]
    fn map_stocks_root_array_maps_all_fields() {
        let json = json!([
            {
                "productName": "Laptop ProBook",
                "productCode": "LP-001",
                "measuringUnit": "BUC",
                "quantity": 5,
                "isService": false
            }
        ]);

        let mut warnings = vec![];
        let products = map_stocks_json(&json, &ctx(), &mut warnings);

        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
        assert_eq!(products.len(), 1);
        let p = &products[0];
        assert_eq!(p.name.as_deref(), Some("Laptop ProBook"));
        assert_eq!(p.code.as_deref(), Some("LP-001"));
        assert_eq!(p.unit.as_deref(), Some("BUC"));
        assert_eq!(p.stock_qty.as_deref(), Some("5"));
        assert_eq!(p.is_service, Some(false));
        assert_eq!(p.source, SOURCE);
        assert_eq!(p.dedup_key.as_deref(), Some("LP-001"));
    }

    // ── "list" wrapper shape ──────────────────────────────────────────────────

    #[test]
    fn map_stocks_list_key_wrapper() {
        let json = json!({
            "list": [
                {
                    "productName": "Serviciu consultanta",
                    "productCode": "SVC-002",
                    "measuringUnit": "ORA",
                    "quantity": 10.5,
                    "isService": true
                }
            ],
            "status": "success"
        });

        let mut warnings = vec![];
        let products = map_stocks_json(&json, &ctx(), &mut warnings);
        assert_eq!(products.len(), 1);
        assert_eq!(products[0].is_service, Some(true));
        assert_eq!(products[0].stock_qty.as_deref(), Some("10.5"));
    }

    // ── missing productName → warning, not panic ──────────────────────────────

    #[test]
    fn map_stocks_missing_product_name_emits_warning() {
        let json = json!([
            {
                "productCode": "X-999",
                "measuringUnit": "KG",
                "quantity": 3
            }
        ]);

        let mut warnings = vec![];
        let products = map_stocks_json(&json, &ctx(), &mut warnings);

        assert_eq!(products.len(), 1);
        assert!(products[0].name.is_none());
        assert!(
            warnings.iter().any(|w| w.contains("productName")),
            "expected a warning about productName, got: {warnings:?}"
        );
    }

    // ── empty array → empty Vec ───────────────────────────────────────────────

    #[test]
    fn map_stocks_empty_array_returns_empty() {
        let json = json!([]);
        let mut warnings = vec![];
        let products = map_stocks_json(&json, &ctx(), &mut warnings);
        assert!(products.is_empty());
        assert!(warnings.is_empty());
    }

    // ── unrecognised JSON shape → warning + empty Vec ─────────────────────────

    #[test]
    fn map_stocks_unrecognised_shape_emits_warning() {
        let json = json!({ "unknown_key": "data" });
        let mut warnings = vec![];
        let products = map_stocks_json(&json, &ctx(), &mut warnings);
        assert!(products.is_empty());
        assert!(
            !warnings.is_empty(),
            "expected at least one warning for unrecognised shape"
        );
    }

    // ── multiple products ─────────────────────────────────────────────────────

    #[test]
    fn map_stocks_multiple_products() {
        let json = json!([
            { "productName": "Produs A", "productCode": "A1", "measuringUnit": "BUC", "quantity": 1 },
            { "productName": "Produs B", "productCode": "B2", "measuringUnit": "KG",  "quantity": 2 }
        ]);
        let mut warnings = vec![];
        let products = map_stocks_json(&json, &ctx(), &mut warnings);
        assert_eq!(products.len(), 2);
        assert_eq!(products[0].code.as_deref(), Some("A1"));
        assert_eq!(products[1].code.as_deref(), Some("B2"));
    }

    // ── parse() rejects RestCreds ─────────────────────────────────────────────

    #[test]
    fn parse_rest_creds_returns_error() {
        let adapter = SmartBillRestAdapter;
        let input = ImportInput::RestCreds {
            company_id: "12345678".to_string(),
        };
        let ctx = ctx();
        let result = adapter.parse(&input, &ctx);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("fetch_smartbill") || msg.contains("RestCreds"),
            "unexpected error message: {msg}"
        );
    }

    // ── parse() with valid Bytes ──────────────────────────────────────────────

    #[test]
    fn parse_bytes_round_trips_through_adapter() {
        let json = json!([
            { "productName": "Test", "productCode": "T1", "measuringUnit": "BUC", "quantity": 7 }
        ]);
        let bytes = serde_json::to_vec(&json).unwrap();
        let adapter = SmartBillRestAdapter;
        let input = ImportInput::Bytes(bytes);
        let ctx = ctx();
        let data = adapter.parse(&input, &ctx).unwrap();
        assert_eq!(data.products.len(), 1);
        assert_eq!(data.products[0].name.as_deref(), Some("Test"));
    }
}
