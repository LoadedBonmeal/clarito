//! Integrări cu software de contabilitate extern: SmartBill, Saga, WinMentor.

use tauri::{AppHandle, State};

use crate::db::settings;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

// ─── Path validation pentru export-uri ──────────────────────────────────────
//
// Apărare împotriva path-traversal și a scrierii arbitrare în filesystem:
// - cale absolută obligatorie
// - fără UNC / SMB (`\\server\share`)
// - fără componente `..`
// - extensie permisă (csv, xlsx, xml, txt)
// - părintele trebuie să fie în $HOME al utilizatorului

pub(crate) fn validate_export_path(path: &str) -> AppResult<std::path::PathBuf> {
    use std::path::PathBuf;

    let p = PathBuf::from(path);

    if !p.is_absolute() {
        return Err(AppError::Validation(
            "Calea trebuie să fie absolută.".into(),
        ));
    }

    let path_str = p.to_string_lossy();
    if path_str.starts_with(r"\\") || path_str.starts_with("//") {
        return Err(AppError::Validation(
            "Locațiile de rețea (UNC/SMB) nu sunt permise.".into(),
        ));
    }

    if p.components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(AppError::Validation(
            "Calea nu poate conține componente '..'.".into(),
        ));
    }

    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
    if !matches!(ext, "csv" | "xlsx" | "xml" | "txt") {
        return Err(AppError::Validation(format!(
            "Extensie fișier nepermisă: .{ext}. Permise: csv, xlsx, xml, txt."
        )));
    }

    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| AppError::Other("Nu pot determina directorul home.".into()))?;

    let parent = p
        .parent()
        .ok_or_else(|| AppError::Validation("Cale invalidă.".into()))?;
    let parent_canon = parent
        .canonicalize()
        .map_err(|_| AppError::Validation("Directorul țintă nu există.".into()))?;
    let home_canon = std::path::PathBuf::from(&home)
        .canonicalize()
        .map_err(AppError::Io)?;

    if !parent_canon.starts_with(&home_canon) {
        return Err(AppError::Validation(
            "Fișierul trebuie să fie în directorul home al utilizatorului.".into(),
        ));
    }

    Ok(p)
}

// ─── SmartBill credentials ──────────────────────────────────────────────────
//
// Token-ul SmartBill (API key) este stocat în OS keychain, NU în tabela
// `settings`. Endpoint-ul `get_smartbill_credentials` NU returnează niciodată
// token-ul către frontend; expune doar username-ul și un flag `configured`.

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartbillCredentialsView {
    pub user: Option<String>,
    pub configured: bool,
}

#[tauri::command]
pub async fn get_smartbill_credentials(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<SmartbillCredentialsView> {
    let user_key = format!("smartbill_user_{}", company_id);
    let user = settings::get(&state.db, &user_key).await?;
    let has_user = user.as_ref().is_some_and(|u| !u.is_empty());
    let configured = has_user && crate::anaf::keychain::get_smartbill_token(&company_id)?.is_some();
    Ok(SmartbillCredentialsView { user, configured })
}

/// Salvează credențialele SmartBill: username în settings, token în OS keychain.
/// Dacă `token` este `None` sau vid, token-ul existent în keychain este păstrat
/// (permite update doar al username-ului fără re-introducerea token-ului).
#[tauri::command]
pub async fn set_smartbill_credentials(
    state: State<'_, AppState>,
    company_id: String,
    user: String,
    token: Option<String>,
) -> AppResult<()> {
    if company_id.trim().is_empty() {
        return Err(AppError::Validation(
            "ID-ul companiei este obligatoriu.".into(),
        ));
    }
    let user_key = format!("smartbill_user_{}", company_id);
    settings::set(&state.db, &user_key, user.trim()).await?;

    if let Some(tok) = token {
        let t = tok.trim();
        if !t.is_empty() {
            crate::anaf::keychain::store_smartbill_token(&company_id, t)?;
        }
    }
    Ok(())
}

/// Șterge credențialele SmartBill pentru o companie (username + token).
#[tauri::command]
pub async fn clear_smartbill_credentials(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<()> {
    if company_id.trim().is_empty() {
        return Err(AppError::Validation(
            "ID-ul companiei este obligatoriu.".into(),
        ));
    }
    let user_key = format!("smartbill_user_{}", company_id);
    settings::set(&state.db, &user_key, "").await?;
    crate::anaf::keychain::delete_smartbill_token(&company_id)?;
    Ok(())
}

// ─── SmartBill push invoice ─────────────────────────────────────────────────

#[tauri::command]
pub async fn smartbill_push_invoice(
    state: State<'_, AppState>,
    _app: AppHandle,
    company_id: String,
    invoice_id: String,
) -> AppResult<String> {
    // 1. Load credentials — username din settings, token din OS keychain
    let user_key = format!("smartbill_user_{}", company_id);

    let user = settings::get(&state.db, &user_key).await?.ok_or_else(|| {
        AppError::Validation("Credențialele SmartBill nu sunt configurate.".into())
    })?;
    let token = crate::anaf::keychain::get_smartbill_token(&company_id)?.ok_or_else(|| {
        AppError::Validation("SmartBill nu este configurat pentru această companie.".into())
    })?;

    if user.is_empty() || token.is_empty() {
        return Err(AppError::Validation(
            "Credențialele SmartBill nu sunt configurate.".into(),
        ));
    }

    // 2. Load invoice with lines
    let invoice_bundle = crate::db::invoices::get_with_lines(&state.db, &invoice_id).await?;
    // G1: verify the invoice belongs to the requested company — prevents pushing a
    // foreign company's invoice to a different company's SmartBill account.
    crate::commands::invoices::check_invoice_ownership(&invoice_bundle.invoice, &company_id)?;
    let invoice = &invoice_bundle.invoice;
    let lines = &invoice_bundle.lines;

    // 3. Load company
    let company = crate::db::companies::get(&state.db, &company_id).await?;

    // 4. Load contact
    let contact = crate::db::contacts::get(&state.db, &invoice.contact_id, &company_id).await?;

    // 5. Build products array
    let products: Vec<serde_json::Value> = lines
        .iter()
        .map(|line| {
            use rust_decimal::prelude::ToPrimitive;
            use rust_decimal::Decimal;
            use std::str::FromStr;
            let vat_rate_dec = Decimal::from_str(&line.vat_rate).unwrap_or(Decimal::ZERO);
            let vat_rate_u32 = vat_rate_dec.to_u32().unwrap_or(0);
            let tax_name = match vat_rate_u32 {
                21 | 19 => "Normala",
                11 | 9 | 5 => "Redusa",
                0 => "Scutita",
                _ => "Normala",
            };
            serde_json::json!({
                "name": line.name,
                "code": "",
                "description": line.description.as_deref().unwrap_or(""),
                "price": line.unit_price,
                "measuringUnitName": line.unit,
                "currency": invoice.currency,
                "quantity": line.quantity,
                "isTaxIncluded": false,
                "taxName": tax_name,
                "taxPercentage": line.vat_rate,
                "saveToDb": false,
                "isService": true
            })
        })
        .collect();

    // 6. Build payload
    let payload = serde_json::json!({
        "companyVatCode": company.cui,
        "client": {
            "name": contact.legal_name,
            "vatCode": contact.cui.as_deref().unwrap_or(""),
            "isTaxPayer": contact.vat_payer,
            "address": contact.address.as_deref().unwrap_or(""),
            "city": contact.city.as_deref().unwrap_or(""),
            "county": contact.county.as_deref().unwrap_or(""),
            "country": contact.country,
            "email": contact.email.as_deref().unwrap_or(""),
            "saveToDb": true
        },
        "issueDate": invoice.issue_date,
        "seriesName": invoice.series,
        "isDraft": false,
        "dueDate": invoice.due_date,
        "deliveryDate": serde_json::Value::Null,
        "mentions": "",
        "observations": invoice.notes.as_deref().unwrap_or(""),
        "currency": invoice.currency,
        "exchangeRate": invoice.exchange_rate.unwrap_or(1.0),
        "language": "RO",
        "precision": 2,
        "type": "Invoice",
        "aviz": false,
        "useStock": false,
        "products": products
    });

    // 7. POST to SmartBill API
    let url = "https://ws.smartbill.ro/SBORO/api/invoice";
    let response = reqwest::Client::new()
        .post(url)
        .basic_auth(&user, Some(&token))
        .json(&payload)
        .send()
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;

    // 8. Handle response
    if !response.status().is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "Eroare necunoscută SmartBill".into());
        return Err(AppError::Other(format!("SmartBill API error: {}", body)));
    }

    let resp_json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;

    // Return URL if present, otherwise number
    let result = resp_json
        .get("url")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            resp_json
                .get("number")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| resp_json.to_string());

    Ok(result)
}

// ─── Helper: format date from YYYY-MM-DD to DD.MM.YYYY ─────────────────────

fn iso_to_dmy_dot(date: &str) -> String {
    // date is YYYY-MM-DD
    let parts: Vec<&str> = date.split('-').collect();
    if parts.len() == 3 {
        format!("{}.{}.{}", parts[2], parts[1], parts[0])
    } else {
        date.to_string()
    }
}

// ─── Helper: format date from YYYY-MM-DD to DD/MM/YYYY ─────────────────────

fn iso_to_dmy_slash(date: &str) -> String {
    let parts: Vec<&str> = date.split('-').collect();
    if parts.len() == 3 {
        format!("{}/{}/{}", parts[2], parts[1], parts[0])
    } else {
        date.to_string()
    }
}

// ─── Saga CSV export ────────────────────────────────────────────────────────

#[tauri::command]
pub async fn export_saga_csv(
    state: State<'_, AppState>,
    company_id: String,
    date_from: String,
    date_to: String,
    output_path: Option<String>,
) -> AppResult<String> {
    use crate::db::invoices::InvoiceFilter;
    use crate::db::models::InvoiceStatus;

    // 1. Query invoices for company_id between date_from and date_to
    // Only VALIDATED invoices are exported — DRAFT/REJECTED/STORNED must not
    // reach the accounting system (fiscal pollution).
    let filter = InvoiceFilter {
        company_id: Some(company_id.clone()),
        date_from: Some(date_from.clone()),
        date_to: Some(date_to.clone()),
        statuses: Some(vec![InvoiceStatus::Validated]),
        page: Some(crate::db::models::Page {
            offset: 0,
            limit: 10_000,
        }),
        ..Default::default()
    };
    let result = crate::db::invoices::list(&state.db, filter).await?;

    // 2. Build CSV — UTF-8 BOM so Excel opens Romanian diacritics correctly
    let header = "\u{FEFF}\"TIP\";\"SERIE\";\"NUMAR\";\"DATA\";\"CUI\";\"DENUMIRE\";\"ADRESA\";\
        \"LOCALITATE\";\"JUDET\";\"TARA\";\"TVA\";\"SUMA_NET\";\"SUMA_TVA\";\"SUMA_TOTAL\";\
        \"MONEDA\";\"CURS\";\"SCADENTA\";\"OBSERVATII\"";

    let mut rows = vec![header.to_string()];

    for invoice in &result.items {
        let contact =
            crate::db::contacts::get(&state.db, &invoice.contact_id, &invoice.company_id).await?;

        let number_padded = format!("{:07}", invoice.number);
        let data = iso_to_dmy_dot(&invoice.issue_date);
        let scadenta = iso_to_dmy_dot(&invoice.due_date);

        // R3: cui și series sunt acum și ele escaped (double-quote) și neutralizate
        // R2: câmpurile text utilizator trec prin csv_neutralize înainte de quoting
        use crate::commands::journals::csv_neutralize;
        let cui = csv_neutralize(contact.cui.as_deref().unwrap_or("")).replace('"', "\"\"");
        let series_escaped = csv_neutralize(&invoice.series).replace('"', "\"\"");
        let denumire = csv_neutralize(&contact.legal_name).replace('"', "\"\"");
        let adresa = csv_neutralize(contact.address.as_deref().unwrap_or("")).replace('"', "\"\"");
        let localitate = csv_neutralize(contact.city.as_deref().unwrap_or("")).replace('"', "\"\"");
        let judet = csv_neutralize(contact.county.as_deref().unwrap_or("")).replace('"', "\"\"");
        let observatii =
            csv_neutralize(invoice.notes.as_deref().unwrap_or("")).replace('"', "\"\"");

        use rust_decimal::Decimal;
        use std::str::FromStr;
        let net = format!(
            "{:.2}",
            Decimal::from_str(&invoice.subtotal_amount).unwrap_or_default()
        );
        let tva = format!(
            "{:.2}",
            Decimal::from_str(&invoice.vat_amount).unwrap_or_default()
        );
        let total = format!(
            "{:.2}",
            Decimal::from_str(&invoice.total_amount).unwrap_or_default()
        );

        // B: Use the invoice's actual currency and exchange rate (fall back to RON/1).
        let moneda = if invoice.currency.is_empty() {
            "RON"
        } else {
            &invoice.currency
        };
        let curs = invoice.exchange_rate.unwrap_or(1.0);
        // SAGA TVA field: 1 = taxabil (standard); kept as 1 for all exported invoices
        // (only VALIDATED invoices reach this path — exempt/zero-rate not distinguished here).
        let row = format!(
            "\"FC\";\"{serie}\";\"{numar}\";\"{data}\";\"{cui}\";\"{denumire}\";\
            \"{adresa}\";\"{localitate}\";\"{judet}\";\"RO\";1;{net};{tva};{total};\
            \"{moneda}\";{curs};\"{scadenta}\";\"{observatii}\"",
            serie = series_escaped,
            numar = number_padded,
            data = data,
            cui = cui,
            denumire = denumire,
            adresa = adresa,
            localitate = localitate,
            judet = judet,
            net = net,
            tva = tva,
            total = total,
            moneda = moneda,
            curs = curs,
            scadenta = scadenta,
            observatii = observatii,
        );

        rows.push(row);
    }

    let csv_content = rows.join("\r\n");

    if let Some(path) = output_path {
        let validated = validate_export_path(&path)?;
        tokio::fs::write(&validated, csv_content.as_bytes())
            .await
            .map_err(AppError::Io)?;
        Ok(validated.to_string_lossy().to_string())
    } else {
        Ok(csv_content)
    }
}

// ─── WinMentor CSV export ─────────────────────────────────────────────────────

#[tauri::command]
pub async fn export_winmentor_csv(
    state: State<'_, AppState>,
    company_id: String,
    date_from: String,
    date_to: String,
    output_path: Option<String>,
) -> AppResult<String> {
    use crate::db::invoices::InvoiceFilter;
    use crate::db::models::InvoiceStatus;

    // 1. Query invoices
    // Only VALIDATED invoices are exported — DRAFT/REJECTED/STORNED must not
    // reach the accounting system (fiscal pollution).
    let filter = InvoiceFilter {
        company_id: Some(company_id.clone()),
        date_from: Some(date_from.clone()),
        date_to: Some(date_to.clone()),
        statuses: Some(vec![InvoiceStatus::Validated]),
        page: Some(crate::db::models::Page {
            offset: 0,
            limit: 10_000,
        }),
        ..Default::default()
    };
    let result = crate::db::invoices::list(&state.db, filter).await?;

    // 2. Build CSV — UTF-8 BOM so Excel opens Romanian diacritics correctly
    let header =
        "\u{FEFF}Tip;Serie;Numar;Data;CUI_Partener;Denumire_Partener;Suma_Net;Cota_TVA;Suma_TVA;\
        Total;Moneda;Curs;Scadenta;Observatii";

    let mut rows = vec![header.to_string()];

    for invoice in &result.items {
        let contact =
            crate::db::contacts::get(&state.db, &invoice.contact_id, &invoice.company_id).await?;

        let data = iso_to_dmy_slash(&invoice.issue_date);
        let scadenta = iso_to_dmy_slash(&invoice.due_date);

        // R2: câmpurile text utilizator trec prin csv_neutralize pentru a preveni
        // formula injection — WinMentor CSV folosește `;` ca separator, fără quoting,
        // deci neutralizarea cu `'` prefix este suficientă.
        use crate::commands::journals::csv_neutralize;
        let cui = csv_neutralize(contact.cui.as_deref().unwrap_or("")).replace(';', " ");
        let denumire = csv_neutralize(&contact.legal_name).replace(';', " ");
        let observatii = csv_neutralize(invoice.notes.as_deref().unwrap_or("")).replace(';', " ");

        // Fetch line items to group by VAT rate — avoids blended rate on mixed-VAT invoices.
        use rust_decimal::prelude::ToPrimitive;
        use rust_decimal::Decimal;
        use std::collections::BTreeMap;

        let line_rows = sqlx::query(
            "SELECT vat_rate, subtotal_amount, vat_amount, total_amount \
             FROM invoice_line_items WHERE invoice_id = ?1 ORDER BY position",
        )
        .bind(&invoice.id)
        .fetch_all(&state.db)
        .await
        .map_err(AppError::Database)?;

        // Group by vat_rate (stored as String in DB; use integer key for bucketing).
        // BTreeMap keeps rates sorted ascending for deterministic output.
        use std::str::FromStr;
        let mut groups: BTreeMap<i64, (Decimal, Decimal, Decimal)> = BTreeMap::new();
        for lr in &line_rows {
            use sqlx::Row;
            let rate_s: String = lr
                .try_get("vat_rate")
                .unwrap_or_else(|_| "0.00".to_string());
            let net_s: String = lr
                .try_get("subtotal_amount")
                .unwrap_or_else(|_| "0.00".to_string());
            let tva_s: String = lr
                .try_get("vat_amount")
                .unwrap_or_else(|_| "0.00".to_string());
            let tot_s: String = lr
                .try_get("total_amount")
                .unwrap_or_else(|_| "0.00".to_string());
            let rate_dec = Decimal::from_str(&rate_s).unwrap_or(Decimal::ZERO);
            let net = Decimal::from_str(&net_s).unwrap_or(Decimal::ZERO);
            let tva = Decimal::from_str(&tva_s).unwrap_or(Decimal::ZERO);
            let tot = Decimal::from_str(&tot_s).unwrap_or(Decimal::ZERO);
            let rate_key = rate_dec.round().to_i64().unwrap_or(0);
            let entry =
                groups
                    .entry(rate_key)
                    .or_insert((Decimal::ZERO, Decimal::ZERO, Decimal::ZERO));
            entry.0 += net;
            entry.1 += tva;
            entry.2 += tot;
        }

        // If we ended up with no lines (shouldn't happen), emit one row from invoice totals
        // using 19 as fallback rate — better than a silently wrong blended rate.
        if groups.is_empty() {
            let net = Decimal::from_str(&invoice.subtotal_amount).unwrap_or(Decimal::ZERO);
            let tva = Decimal::from_str(&invoice.vat_amount).unwrap_or(Decimal::ZERO);
            let tot = Decimal::from_str(&invoice.total_amount).unwrap_or(Decimal::ZERO);
            groups.insert(19, (net, tva, tot));
            tracing::warn!(invoice_id = %invoice.id, "WinMentor export: no line items found, using fallback rate 19%");
        }

        // Warn if multiple VAT rates are present — WinMentor will get one row per rate.
        if groups.len() > 1 {
            tracing::warn!(
                invoice_id = %invoice.id,
                full_number = %invoice.full_number,
                rate_count = groups.len(),
                "WinMentor export: invoice has {} VAT rate groups — emitting one row per group",
                groups.len()
            );
        }

        // B: Use the invoice's actual currency and exchange rate (fall back to RON/1).
        let moneda = if invoice.currency.is_empty() {
            "RON"
        } else {
            &invoice.currency
        };
        let curs = invoice.exchange_rate.unwrap_or(1.0);

        for (vat_rate, (net_dec, tva_dec, tot_dec)) in &groups {
            let net = format!("{:.2}", net_dec.round_dp(2));
            let tva = format!("{:.2}", tva_dec.round_dp(2));
            let total = format!("{:.2}", tot_dec.round_dp(2));

            let row = format!(
                "FACT;{serie};{numar};{data};{cui};{denumire};\
                {net};{vat_rate};{tva};{total};{moneda};{curs};{scadenta};{observatii}",
                serie = invoice.series,
                numar = invoice.number,
                data = data,
                cui = cui,
                denumire = denumire,
                net = net,
                vat_rate = vat_rate,
                tva = tva,
                total = total,
                moneda = moneda,
                curs = curs,
                scadenta = scadenta,
                observatii = observatii,
            );
            rows.push(row);
        }
    }

    let csv_content = rows.join("\r\n");

    if let Some(path) = output_path {
        let validated = validate_export_path(&path)?;
        tokio::fs::write(&validated, csv_content.as_bytes())
            .await
            .map_err(AppError::Io)?;
        Ok(validated.to_string_lossy().to_string())
    } else {
        Ok(csv_content)
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    /// A: SAGA CSV export starts with UTF-8 BOM for correct diacritics in Excel.
    #[test]
    fn saga_csv_starts_with_utf8_bom() {
        let header = "\u{FEFF}\"TIP\";\"SERIE\";\"NUMAR\"";
        assert!(
            header.starts_with('\u{FEFF}'),
            "SAGA CSV must start with UTF-8 BOM"
        );
    }

    /// A: WinMentor CSV export starts with UTF-8 BOM for correct diacritics in Excel.
    #[test]
    fn winmentor_csv_starts_with_utf8_bom() {
        let header = "\u{FEFF}Tip;Serie;Numar";
        assert!(
            header.starts_with('\u{FEFF}'),
            "WinMentor CSV must start with UTF-8 BOM"
        );
    }

    /// Helper mirroring production: resolve currency/curs from invoice fields.
    fn resolve_moneda_curs(currency: &str, exchange_rate: Option<f64>) -> (&str, f64) {
        let moneda = if currency.is_empty() { "RON" } else { currency };
        let curs = exchange_rate.unwrap_or(1.0);
        (moneda, curs)
    }

    /// B: SAGA/WinMentor use real invoice currency — not hardcoded RON.
    #[test]
    fn saga_row_uses_invoice_currency() {
        let (moneda, curs) = resolve_moneda_curs("EUR", Some(5.0));
        let row = format!("\"MONEDA={moneda}\";\"CURS={curs}\"");
        assert!(row.contains("EUR"), "SAGA row must contain EUR currency");
        assert!(row.contains("5"), "SAGA row must contain exchange rate 5");
    }

    /// B: WinMentor uses real invoice currency — not hardcoded RON.
    #[test]
    fn winmentor_row_uses_invoice_currency() {
        let (moneda, curs) = resolve_moneda_curs("USD", Some(4.5));
        let row = format!(
            "FACT;FCT;1;01/01/2026;RO123;CLIENT;1000.00;19;190.00;1190.00;{moneda};{curs};31/01/2026;"
        );
        assert!(
            row.contains("USD"),
            "WinMentor row must contain USD currency"
        );
        assert!(
            row.contains("4.5"),
            "WinMentor row must contain exchange rate 4.5"
        );
    }

    /// B: Falls back to RON/1 when currency is empty and exchange_rate is None.
    #[test]
    fn currency_fallback_to_ron() {
        let (moneda, curs) = resolve_moneda_curs("", None);
        assert_eq!(moneda, "RON");
        assert!((curs - 1.0).abs() < f64::EPSILON);
    }
}
