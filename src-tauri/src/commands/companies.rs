use tauri::State;

use crate::db::companies::{
    self, Company, CreateCompanyInput, TaxRegimeStatus, UpdateCompanyInput,
};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[tauri::command]
pub async fn list_companies(state: State<'_, AppState>) -> AppResult<Vec<Company>> {
    companies::list(&state.db).await
}

#[tauri::command]
pub async fn get_company(state: State<'_, AppState>, id: String) -> AppResult<Company> {
    companies::get(&state.db, &id).await
}

/// Micro-ceiling status for a company: its year-to-date sales turnover (RON) vs the 100.000 EUR
/// micro ceiling (`eur_ron` = the EUR→RON rate the FE supplies, e.g. the year-end BNR rate), with
/// an advisory when approaching/exceeding it. Turnover proxy = validated, non-storno sales net.
#[tauri::command]
pub async fn tax_regime_status(
    state: State<'_, AppState>,
    company_id: String,
    year: i32,
    eur_ron: f64,
) -> AppResult<TaxRegimeStatus> {
    use rust_decimal::Decimal;
    let pool = &state.db;
    let company = companies::get(pool, &company_id).await?;
    let from = format!("{year}-01-01");
    let to = format!("{year}-12-31");
    let turnover_f: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(CAST(subtotal_amount AS REAL) * COALESCE(exchange_rate, 1.0)), 0.0) \
         FROM invoices \
         WHERE company_id = ?1 AND status = 'VALIDATED' AND storno_of_invoice_id IS NULL \
           AND issue_date >= ?2 AND issue_date <= ?3",
    )
    .bind(&company_id)
    .bind(&from)
    .bind(&to)
    .fetch_one(pool)
    .await?;
    let turnover = Decimal::try_from(turnover_f.max(0.0))
        .unwrap_or(Decimal::ZERO)
        .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
    let eur = Decimal::try_from(eur_ron.max(0.0)).unwrap_or(Decimal::ZERO);
    Ok(companies::micro_ceiling_status(
        &company.tax_regime,
        company.cash_vat,
        turnover,
        eur,
    ))
}

/// Starea plafonului de scutire TVA (art. 310 Cod fiscal, Legea 141/2025): 395.000 lei cifră de
/// afaceri anuală de la 01.09.2025. Relevant DOAR pentru neplătitorii de TVA — depășirea obligă la
/// înregistrarea în scopuri de TVA. `level`: none / approaching (≥80%) / exceeded.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VatRegistrationStatus {
    pub applicable: bool,
    pub level: String,
    pub ytd_turnover_ron: String,
    pub plafon_ron: String,
    pub pct: i64,
}

#[tauri::command]
pub async fn vat_registration_status(
    state: State<'_, AppState>,
    company_id: String,
    year: i32,
) -> AppResult<VatRegistrationStatus> {
    use rust_decimal::Decimal;
    const PLAFON_LEI: i64 = 395_000;
    let pool = &state.db;
    let company = companies::get(pool, &company_id).await?;
    if company.vat_payer {
        return Ok(VatRegistrationStatus {
            applicable: false,
            level: "none".into(),
            ytd_turnover_ron: "0".into(),
            plafon_ron: PLAFON_LEI.to_string(),
            pct: 0,
        });
    }
    let from = format!("{year}-01-01");
    let to = format!("{year}-12-31");
    let turnover_f: f64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(CAST(subtotal_amount AS REAL) * COALESCE(exchange_rate, 1.0)), 0.0) \
         FROM invoices \
         WHERE company_id = ?1 AND status IN ('VALIDATED','STORNED') \
           AND issue_date >= ?2 AND issue_date <= ?3",
    )
    .bind(&company_id)
    .bind(&from)
    .bind(&to)
    .fetch_one(pool)
    .await?;
    let turnover = crate::db::invoices::round2(
        Decimal::try_from(turnover_f.max(0.0)).unwrap_or(Decimal::ZERO),
    );
    let plafon = Decimal::from(PLAFON_LEI);
    let pct: i64 = ((turnover / plafon) * Decimal::from(100))
        .round_dp_with_strategy(0, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
        .try_into()
        .unwrap_or(0);
    let level = if turnover >= plafon {
        "exceeded"
    } else if pct >= 80 {
        "approaching"
    } else {
        "none"
    };
    Ok(VatRegistrationStatus {
        applicable: true,
        level: level.into(),
        ytd_turnover_ron: turnover.to_string(),
        plafon_ron: PLAFON_LEI.to_string(),
        pct,
    })
}

#[tauri::command]
pub async fn create_company(
    state: State<'_, AppState>,
    input: CreateCompanyInput,
) -> AppResult<Company> {
    // Verificăm limita de companii conform planului de licență:
    // TRIAL → 3, SOLO → 1, ACCOUNTANT → 15, FIRM → nelimitat
    if let Some(lic) = crate::db::license::get(&state.db).await? {
        let current_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM companies WHERE is_active = 1")
                .fetch_one(&state.db)
                .await
                .unwrap_or(0);
        let limit: Option<i64> = match lic.tier.as_str() {
            "TRIAL" => Some(3),
            "SOLO" => Some(1),
            "ACCOUNTANT" => Some(15),
            "FIRM" => None, // nelimitat
            _ => Some(3),   // tier necunoscut → fallback la TRIAL
        };
        if let Some(max) = limit {
            if current_count >= max {
                return Err(AppError::Validation(format!(
                    "Planul {} permite maxim {} companie(i). \
                     Actualizați licența pentru a adăuga mai multe.",
                    lic.tier, max
                )));
            }
        }
    }
    let new_company = companies::create(&state.db, input).await?;
    let _ = crate::db::audit::log_user_action(
        &state.db,
        "company_created",
        "company",
        &new_company.id,
        Some(&new_company.id),
        Some(&new_company.cui),
    )
    .await;
    Ok(new_company)
}

#[tauri::command]
pub async fn update_company(
    state: State<'_, AppState>,
    id: String,
    input: UpdateCompanyInput,
) -> AppResult<Company> {
    let updated = companies::update(&state.db, &id, input).await?;
    let _ = crate::db::audit::log_user_action(
        &state.db,
        "company_updated",
        "company",
        &id,
        Some(&id),
        None,
    )
    .await;
    Ok(updated)
}

#[tauri::command]
pub async fn delete_company(state: State<'_, AppState>, id: String) -> AppResult<()> {
    companies::soft_delete(&state.db, &id).await
}

#[tauri::command]
pub async fn get_next_invoice_number(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<i64> {
    companies::next_invoice_number(&state.db, &company_id).await
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnafCompanyData {
    pub cui: String,
    pub legal_name: String,
    pub address: String,
    pub city: String,
    pub county: String,
    pub postal_code: Option<String>,
    pub registry_number: Option<String>,
    pub phone: Option<String>,
    pub vat_payer: bool,
    /// TVA la încasare (cash VAT) — `inregistrare_RTVAI.statusTvaIncasare`. The buyer's VAT
    /// deduction is deferred to payment (art. 297 Cod fiscal) when the supplier is on cash VAT.
    pub cash_vat: bool,
    /// Registered in "Registrul RO e-Factura" — `date_generale.statusRO_e_Factura`.
    pub efactura_registered: bool,
    /// False when the company is an inactive contributor (`stare_inactiv.statusInactivi`) — its
    /// invoices carry restricted deductibility for the buyer (art. 11 Cod fiscal).
    pub active: bool,
}

#[tauri::command]
pub async fn fetch_anaf_company_data(cui: String) -> AppResult<AnafCompanyData> {
    // Clean CUI: strip "RO" prefix (case-insensitive), strip spaces, parse as u64
    let cleaned = cui.trim().to_uppercase();
    let cleaned = cleaned.strip_prefix("RO").unwrap_or(&cleaned);
    let cleaned = cleaned.replace(' ', "");
    let cui_number: u64 = cleaned
        .parse()
        .map_err(|_| AppError::Validation(format!("CUI invalid: {cui}")))?;

    // Today's date as "YYYY-MM-DD" — local, so the VAT-status lookup doesn't query the prior
    // day near local midnight (RO is UTC+2/+3).
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    // POST to ANAF API
    let body = serde_json::json!([{"cui": cui_number, "data": today}]);
    let client = reqwest::Client::new();
    let response = client
        .post("https://webservicesp.anaf.ro/api/PlatitorTvaRest/v9/tva")
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;

    // Check if not found
    let not_found = json.get("notFound").and_then(|v| v.as_array());
    let found = json.get("found").and_then(|v| v.as_array());

    let found_list = match found {
        Some(list) if !list.is_empty() => list,
        _ => {
            return Err(AppError::NotFound);
        }
    };

    // Also check notFound array contains our cui
    if let Some(nf) = not_found {
        if nf.iter().any(|v| {
            v.as_u64() == Some(cui_number)
                || v.as_str().map(|s| s == cleaned.as_str()).unwrap_or(false)
        }) {
            return Err(AppError::NotFound);
        }
    }

    let entry = &found_list[0];
    map_anaf_entry(entry, cleaned).ok_or(AppError::NotFound)
}

/// Map one `found[i]` entry from the ANAF v9 PlatitorTvaRest reply to `AnafCompanyData`. Pure +
/// unit-tested so the (subtle) field keys stay correct: `inregistrare_scop_Tva` (capital T),
/// the structured `adresa_sediu_social` for city/county, cash-VAT, e-Factura, and inactive status.
/// Returns `None` if `date_generale` is missing.
fn map_anaf_entry(entry: &serde_json::Value, cleaned: String) -> Option<AnafCompanyData> {
    let date_gen = entry.get("date_generale")?;
    // NB: the v9 key is `inregistrare_scop_Tva` (capital T) — the lowercase spelling never matches.
    let inreg_tva = entry.get("inregistrare_scop_Tva");
    // Structured registered-office address (date_generale has no localitate/judet — only a flat
    // `adresa` string); use the `s`-prefixed fields for a clean city + 2-letter county code.
    let sediu = entry.get("adresa_sediu_social");
    let sediu_field = |field: &str| -> String {
        sediu
            .and_then(|s| s.get(field))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let str_field = |field: &str| -> String {
        date_gen
            .get(field)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let opt_str_field = |field: &str| -> Option<String> {
        let s = str_field(field);
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    };

    let legal_name = str_field("denumire");
    let address = str_field("adresa");
    // City from the structured office (e.g. "Mun. Bacău"), stripping the admin prefix; county as
    // the 2-letter auto code ("BC") the app uses. Fall back to nothing if the office is absent.
    let raw_city = sediu_field("sdenumire_Localitate");
    let city = if raw_city.contains("ucure") {
        // The capital arrives as "Sector N Mun. Bucureşti" (note the ş) — normalise to "București".
        "București".to_string()
    } else {
        raw_city
            .strip_prefix("Mun. ")
            .or_else(|| raw_city.strip_prefix("Municipiul "))
            .or_else(|| raw_city.strip_prefix("Oraș "))
            .or_else(|| raw_city.strip_prefix("Com. "))
            .or_else(|| raw_city.strip_prefix("Sat "))
            .unwrap_or(&raw_city)
            .trim()
            .to_string()
    };
    let county = sediu_field("scod_JudetAuto").to_uppercase();
    let postal_code = opt_str_field("codPostal");
    let registry_number = opt_str_field("nrRegCom");
    let phone = opt_str_field("telefon");

    // VAT scope, cash VAT, e-Factura registration, inactive status.
    let bool_at = |obj: Option<&serde_json::Value>, field: &str| -> bool {
        obj.and_then(|v| v.get(field))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    };
    let vat_payer = bool_at(inreg_tva, "scpTVA");
    let cash_vat = bool_at(entry.get("inregistrare_RTVAI"), "statusTvaIncasare");
    let efactura_registered = date_gen
        .get("statusRO_e_Factura")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let active = !bool_at(entry.get("stare_inactiv"), "statusInactivi");

    Some(AnafCompanyData {
        cui: cleaned,
        legal_name,
        address,
        city,
        county,
        postal_code,
        registry_number,
        phone,
        vat_payer,
        cash_vat,
        efactura_registered,
        active,
    })
}

// ─── VIES — validare cod TVA UE ──────────────────────────────────────────────

/// Known VIES `userError` values that indicate a transient/service fault (retry-able).
/// Everything else is treated as a hard validation error.
/// Transient VIES error codes (the lookup may succeed on retry). Real codes from the VIES REST API
/// `errorWrappers[].error` envelope; anything not listed here (e.g. INVALID_INPUT, INVALID_REQUESTER_INFO)
/// is treated as a hard validation error.
const VIES_TRANSIENT_ERRORS: &[&str] = &[
    "MS_UNAVAILABLE",
    "MS_MAX_CONCURRENT_REQ",
    "GLOBAL_MAX_CONCURRENT_REQ",
    "TIMEOUT",
    "SERVICE_UNAVAILABLE",
    "SERVER_BUSY",
];

/// Result of a VIES check-vat-number REST call.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViesResult {
    pub valid: bool,
    pub name: Option<String>,
    pub address: Option<String>,
    pub country_code: String,
    pub vat_number: String,
}

/// Normalise the inputs.
///   - `country_code`: uppercase, must be exactly 2 ASCII letters.
///   - `vat_number`: strip all non-alphanumeric characters, then strip a
///     leading country-code prefix if the user pasted e.g. "DE123456789" into
///     the number field.
///
/// Returns `(cc, num)` or an `AppError::Validation` if `country_code` is
/// not 2 letters.
fn normalise_vies_inputs(
    country_code: &str,
    vat_number: &str,
) -> crate::error::AppResult<(String, String)> {
    let cc = country_code.trim().to_uppercase();
    if cc.len() != 2 || !cc.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(AppError::Validation(format!(
            "Cod de țară invalid: '{}' — trebuie să fie exact 2 litere (ex: DE, FR, IT).",
            country_code
        )));
    }
    // VIES uses the fiscal prefix EL for Greece, while the app's country list (and ISO 3166) uses GR.
    // Remap before building the request + stripping the pasted prefix, else VIES returns INVALID_INPUT.
    let cc = if cc == "GR" { "EL".to_string() } else { cc };
    // Strip non-alphanumeric characters.
    let stripped: String = vat_number
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_uppercase();
    // If the user pasted the full VAT id (e.g. "DE123456789") into the number
    // field, remove the country-code prefix.
    let num = if stripped.starts_with(cc.as_str()) {
        stripped[2..].to_string()
    } else {
        stripped
    };
    Ok((cc, num))
}

/// Map a VIES REST response JSON to `ViesResult`.
///
/// The real VIES REST API returns a success envelope `{"countryCode","vatNumber","valid":bool,
/// "name","address",…}` and a failure envelope `{"actionSucceed":false,"errorWrappers":[{"error":
/// "INVALID_INPUT"}…]}`. A `valid:false` is NOT an error — it means "not a registered VAT id".
/// Treats "---", empty strings, and null as None for name/address (masked by member states).
fn map_vies_response(
    json: &serde_json::Value,
    cc: String,
    num: String,
) -> crate::error::AppResult<ViesResult> {
    // Hard / transient errors arrive in the `errorWrappers` array (not the success envelope).
    if let Some(wrappers) = json.get("errorWrappers").and_then(|v| v.as_array()) {
        let codes: Vec<&str> = wrappers
            .iter()
            .filter_map(|w| w.get("error").and_then(|e| e.as_str()))
            .filter(|c| !c.is_empty())
            .collect();
        if !codes.is_empty() {
            let msg = format!("VIES: {}", codes.join(", "));
            // Only transient if EVERY reported code is transient; any hard code → validation error.
            if codes.iter().all(|c| VIES_TRANSIENT_ERRORS.contains(c)) {
                return Err(AppError::Other(format!(
                    "{msg} — serviciul VIES este temporar indisponibil. Încercați din nou."
                )));
            }
            return Err(AppError::Validation(msg));
        }
    }
    let valid = json.get("valid").and_then(|v| v.as_bool()).unwrap_or(false);

    let clean_opt = |v: Option<&serde_json::Value>| -> Option<String> {
        let s = v.and_then(|x| x.as_str()).unwrap_or("").trim().to_string();
        if s.is_empty() || s == "---" {
            None
        } else {
            Some(s)
        }
    };
    Ok(ViesResult {
        valid,
        name: clean_opt(json.get("name")),
        address: clean_opt(json.get("address")),
        country_code: cc,
        vat_number: num,
    })
}

/// Validare cod TVA UE prin VIES REST API (backend native — fără CORS/CSP).
///
/// Apelează `https://ec.europa.eu/taxation_customs/vies/rest-api/check-vat-number`
/// cu JSON body `{"countryCode": "<CC>", "vatNumber": "<NUM>"}`.
/// Returnează `ViesResult` cu `valid`, `name`, `address` (mascate de unele
/// state membre — pot fi `null`), `countryCode`, `vatNumber`.
#[tauri::command]
pub async fn validate_vies(country_code: String, vat_number: String) -> AppResult<ViesResult> {
    let (cc, num) = normalise_vies_inputs(&country_code, &vat_number)?;

    let body = serde_json::json!({
        "countryCode": cc,
        "vatNumber":   num,
    });
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| AppError::Other(e.to_string()))?;
    let response = client
        .post("https://ec.europa.eu/taxation_customs/vies/rest-api/check-vat-number")
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Other(format!("VIES request failed: {e}")))?;

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| AppError::Other(format!("VIES response parse error: {e}")))?;

    map_vies_response(&json, cc, num)
}

#[cfg(test)]
mod vies_tests {
    use super::{map_vies_response, normalise_vies_inputs};

    // ── normalise_vies_inputs ──────────────────────────────────────────────

    #[test]
    fn normalise_strips_non_alphanumeric_and_uppercases() {
        let (cc, num) = normalise_vies_inputs("de", "123 456.789").unwrap();
        assert_eq!(cc, "DE");
        assert_eq!(num, "123456789");
    }

    #[test]
    fn normalise_strips_country_prefix_from_vat_number() {
        // User pasted "DE123456789" into the vat_number field.
        let (cc, num) = normalise_vies_inputs("DE", "DE123456789").unwrap();
        assert_eq!(cc, "DE");
        assert_eq!(num, "123456789");
    }

    #[test]
    fn normalise_does_not_strip_different_prefix() {
        // Country code FR but number starts with DE → keep as-is.
        let (_, num) = normalise_vies_inputs("FR", "DE123456789").unwrap();
        assert_eq!(num, "DE123456789");
    }

    #[test]
    fn normalise_rejects_invalid_country_code() {
        assert!(normalise_vies_inputs("DEX", "123").is_err());
        assert!(normalise_vies_inputs("1E", "123").is_err());
        assert!(normalise_vies_inputs("", "123").is_err());
    }

    // ── map_vies_response ─────────────────────────────────────────────────

    // NOTE: these payloads mirror the REAL VIES REST API (verified against live calls): the success
    // envelope carries `valid` (NOT `isValid`) and errors come in an `errorWrappers` array (NOT a
    // top-level `userError`). A `valid:false` is "not registered", not an error.

    #[test]
    fn maps_valid_response_with_name_and_address() {
        let json = serde_json::json!({
            "countryCode": "DE",
            "vatNumber": "123456789",
            "valid": true,
            "name": "ACME GmbH",
            "address": "Hauptstraße 1, Berlin"
        });
        let r = map_vies_response(&json, "DE".into(), "123456789".into()).unwrap();
        assert!(r.valid);
        assert_eq!(r.name.as_deref(), Some("ACME GmbH"));
        assert_eq!(r.address.as_deref(), Some("Hauptstraße 1, Berlin"));
        assert_eq!(r.country_code, "DE");
        assert_eq!(r.vat_number, "123456789");
    }

    #[test]
    fn maps_invalid_response() {
        // Not a registered VAT id — valid:false, NOT an error.
        let json = serde_json::json!({
            "countryCode": "RO",
            "vatNumber": "99999999",
            "valid": false,
            "name": null,
            "address": null
        });
        let r = map_vies_response(&json, "RO".into(), "99999999".into()).unwrap();
        assert!(!r.valid);
        assert!(r.name.is_none());
        assert!(r.address.is_none());
    }

    #[test]
    fn masked_name_address_treated_as_none() {
        // Several member states (incl. DE/RO) mask data with "---".
        let json = serde_json::json!({
            "countryCode": "RO",
            "vatNumber": "12345678",
            "valid": true,
            "name": "---",
            "address": "---"
        });
        let r = map_vies_response(&json, "RO".into(), "12345678".into()).unwrap();
        assert!(r.valid);
        assert!(r.name.is_none(), "masked '---' must become None");
        assert!(r.address.is_none(), "masked '---' must become None");
    }

    #[test]
    fn missing_valid_key_defaults_to_false_not_error() {
        // Defensive: an unexpected envelope without `valid` and without errors → not valid (no panic).
        let json = serde_json::json!({ "countryCode": "DE", "vatNumber": "1" });
        let r = map_vies_response(&json, "DE".into(), "1".into()).unwrap();
        assert!(!r.valid);
    }

    #[test]
    fn transient_error_returns_app_error_other() {
        let json = serde_json::json!({
            "actionSucceed": false,
            "errorWrappers": [{ "error": "MS_UNAVAILABLE" }]
        });
        let err = map_vies_response(&json, "DE".into(), "123".into()).unwrap_err();
        match err {
            crate::error::AppError::Other(_) => {}
            other => panic!("expected Other, got {other:?}"),
        }
    }

    #[test]
    fn hard_error_returns_app_error_validation() {
        let json = serde_json::json!({
            "actionSucceed": false,
            "errorWrappers": [{ "error": "INVALID_INPUT" }]
        });
        let err = map_vies_response(&json, "DE".into(), "123".into()).unwrap_err();
        match err {
            crate::error::AppError::Validation(_) => {}
            other => panic!("expected Validation, got {other:?}"),
        }
    }

    #[test]
    fn mixed_errors_with_any_hard_code_is_validation() {
        // A transient + a hard code together → hard wins (validation error).
        let json = serde_json::json!({
            "actionSucceed": false,
            "errorWrappers": [{ "error": "MS_UNAVAILABLE" }, { "error": "INVALID_INPUT" }]
        });
        match map_vies_response(&json, "DE".into(), "1".into()).unwrap_err() {
            crate::error::AppError::Validation(_) => {}
            other => panic!("expected Validation, got {other:?}"),
        }
    }

    #[test]
    fn greece_country_code_remapped_to_el() {
        // The app stores ISO `GR`; VIES needs the fiscal prefix `EL`.
        let (cc, _) = normalise_vies_inputs("GR", "123456789").unwrap();
        assert_eq!(cc, "EL");
        // And a pasted full id "EL123…" gets its prefix stripped against the remapped cc.
        let (cc2, num2) = normalise_vies_inputs("GR", "EL123456789").unwrap();
        assert_eq!(cc2, "EL");
        assert_eq!(num2, "123456789");
    }
}

#[cfg(test)]
mod anaf_tests {
    use super::map_anaf_entry;

    #[test]
    fn maps_v9_entry_with_correct_keys() {
        // Shape mirrors the real ANAF v9 reply (verified against a live lookup).
        let entry = serde_json::json!({
            "date_generale": {
                "denumire": "DEDEMAN SRL",
                "adresa": "JUD. BACĂU, MUN. BACĂU, STR. ALEXEI TOLSTOI, NR.8",
                "nrRegCom": "J04/123/1993",
                "telefon": "0234513330",
                "codPostal": "600093",
                "statusRO_e_Factura": true
            },
            "inregistrare_scop_Tva": { "scpTVA": true },
            "inregistrare_RTVAI": { "statusTvaIncasare": false },
            "stare_inactiv": { "statusInactivi": false },
            "adresa_sediu_social": {
                "sdenumire_Localitate": "Mun. Bacău",
                "sdenumire_Judet": "BACĂU",
                "scod_JudetAuto": "BC",
                "scod_Postal": "600093"
            }
        });
        let d = map_anaf_entry(&entry, "2816464".into()).expect("maps");
        assert_eq!(d.legal_name, "DEDEMAN SRL");
        assert!(d.vat_payer, "scpTVA under the capital-T key must be read");
        assert!(!d.cash_vat);
        assert!(d.efactura_registered);
        assert!(d.active, "statusInactivi=false → active");
        assert_eq!(d.city, "Bacău", "admin prefix stripped");
        assert_eq!(d.county, "BC", "2-letter auto code");
    }

    #[test]
    fn inactive_and_cash_vat_are_flagged() {
        let entry = serde_json::json!({
            "date_generale": { "denumire": "X SRL", "adresa": "" },
            "inregistrare_scop_Tva": { "scpTVA": true },
            "inregistrare_RTVAI": { "statusTvaIncasare": true },
            "stare_inactiv": { "statusInactivi": true }
        });
        let d = map_anaf_entry(&entry, "123".into()).expect("maps");
        assert!(d.cash_vat, "statusTvaIncasare=true");
        assert!(!d.active, "statusInactivi=true → inactive");
    }

    #[test]
    fn bucharest_locality_normalises_to_city_and_sector_county() {
        // The capital arrives as "Sector N Mun. Bucureşti" with county auto-code "B".
        let entry = serde_json::json!({
            "date_generale": { "denumire": "Y SRL", "adresa": "" },
            "adresa_sediu_social": {
                "sdenumire_Localitate": "Sector 6 Mun. Bucureşti",
                "scod_JudetAuto": "B"
            }
        });
        let d = map_anaf_entry(&entry, "1".into()).expect("maps");
        assert_eq!(d.city, "București");
        assert_eq!(d.county, "B");
    }

    #[test]
    fn missing_date_generale_returns_none() {
        assert!(map_anaf_entry(&serde_json::json!({}), "1".into()).is_none());
    }
}
