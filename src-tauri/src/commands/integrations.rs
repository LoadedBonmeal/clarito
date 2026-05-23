//! Integrări cu software de contabilitate extern: SmartBill, Saga, WinMentor.

use tauri::{AppHandle, Manager, State};

use crate::db::settings;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

// ─── SmartBill credentials ──────────────────────────────────────────────────

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartBillCredentials {
    pub user: String,
    pub token: String,
    pub configured: bool,
}

#[tauri::command]
pub async fn get_smartbill_credentials(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<SmartBillCredentials> {
    let user_key = format!("smartbill_user_{}", company_id);
    let token_key = format!("smartbill_token_{}", company_id);

    let user = settings::get(&state.db, &user_key)
        .await?
        .unwrap_or_default();
    let token = settings::get(&state.db, &token_key)
        .await?
        .unwrap_or_default();

    let configured = !user.is_empty() && !token.is_empty();

    Ok(SmartBillCredentials { user, token, configured })
}

// ─── SmartBill push invoice ─────────────────────────────────────────────────

#[tauri::command]
pub async fn smartbill_push_invoice(
    state: State<'_, AppState>,
    _app: AppHandle,
    company_id: String,
    invoice_id: String,
) -> AppResult<String> {
    // 1. Load credentials
    let user_key = format!("smartbill_user_{}", company_id);
    let token_key = format!("smartbill_token_{}", company_id);

    let user = settings::get(&state.db, &user_key).await?.ok_or_else(|| {
        AppError::Other("Credențialele SmartBill nu sunt configurate.".into())
    })?;
    let token = settings::get(&state.db, &token_key).await?.ok_or_else(|| {
        AppError::Other("Credențialele SmartBill nu sunt configurate.".into())
    })?;

    if user.is_empty() || token.is_empty() {
        return Err(AppError::Other(
            "Credențialele SmartBill nu sunt configurate.".into(),
        ));
    }

    // 2. Load invoice with lines
    let invoice_bundle = crate::db::invoices::get_with_lines(&state.db, &invoice_id).await?;
    let invoice = &invoice_bundle.invoice;
    let lines = &invoice_bundle.lines;

    // 3. Load company
    let company = crate::db::companies::get(&state.db, &company_id).await?;

    // 4. Load contact
    let contact = crate::db::contacts::get(&state.db, &invoice.contact_id).await?;

    // 5. Build products array
    let products: Vec<serde_json::Value> = lines
        .iter()
        .map(|line| {
            let tax_name = match line.vat_rate as u32 {
                19 => "Normala",
                9 | 5 => "Redusa",
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
    app: AppHandle,
    company_id: String,
    date_from: String,
    date_to: String,
) -> AppResult<String> {
    use crate::db::invoices::InvoiceFilter;

    // 1. Query invoices for company_id between date_from and date_to
    let filter = InvoiceFilter {
        company_id: Some(company_id.clone()),
        date_from: Some(date_from.clone()),
        date_to: Some(date_to.clone()),
        page: Some(crate::db::models::Page { offset: 0, limit: 10_000 }),
        ..Default::default()
    };
    let result = crate::db::invoices::list(&state.db, filter).await?;

    // 2. Build CSV
    let header = "\"TIP\";\"SERIE\";\"NUMAR\";\"DATA\";\"CUI\";\"DENUMIRE\";\"ADRESA\";\
        \"LOCALITATE\";\"JUDET\";\"TARA\";\"TVA\";\"SUMA_NET\";\"SUMA_TVA\";\"SUMA_TOTAL\";\
        \"MONEDA\";\"CURS\";\"SCADENTA\";\"OBSERVATII\"";

    let mut rows = vec![header.to_string()];

    for invoice in &result.items {
        let contact = crate::db::contacts::get(&state.db, &invoice.contact_id).await?;

        let number_padded = format!("{:07}", invoice.number);
        let data = iso_to_dmy_dot(&invoice.issue_date);
        let scadenta = iso_to_dmy_dot(&invoice.due_date);

        let cui = contact.cui.as_deref().unwrap_or("").to_string();
        let denumire = contact.legal_name.replace('"', "\"\"");
        let adresa = contact.address.as_deref().unwrap_or("").replace('"', "\"\"");
        let localitate = contact.city.as_deref().unwrap_or("").replace('"', "\"\"");
        let judet = contact.county.as_deref().unwrap_or("").replace('"', "\"\"");
        let observatii = invoice.notes.as_deref().unwrap_or("").replace('"', "\"\"");

        let net = format!("{:.2}", invoice.subtotal_amount);
        let tva = format!("{:.2}", invoice.vat_amount);
        let total = format!("{:.2}", invoice.total_amount);

        let row = format!(
            "\"FC\";\"{serie}\";\"{numar}\";\"{data}\";\"{cui}\";\"{denumire}\";\
            \"{adresa}\";\"{localitate}\";\"{judet}\";\"RO\";1;{net};{tva};{total};\
            \"RON\";1;\"{scadenta}\";\"{observatii}\"",
            serie = invoice.series,
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
            scadenta = scadenta,
            observatii = observatii,
        );

        rows.push(row);
    }

    let csv_content = rows.join("\r\n");

    // 3. Save file
    let out_dir = app.path().app_data_dir()?;
    let file_name = format!(
        "saga_export_{}_{}_{}.csv",
        company_id, date_from, date_to
    );
    let file_path = out_dir.join(&file_name);
    std::fs::write(&file_path, csv_content.as_bytes()).map_err(AppError::Io)?;

    Ok(file_path.display().to_string())
}

// ─── WinMentor CSV export ───────────────────────────────────────────────────

#[tauri::command]
pub async fn export_winmentor_csv(
    state: State<'_, AppState>,
    app: AppHandle,
    company_id: String,
    date_from: String,
    date_to: String,
) -> AppResult<String> {
    use crate::db::invoices::InvoiceFilter;

    // 1. Query invoices
    let filter = InvoiceFilter {
        company_id: Some(company_id.clone()),
        date_from: Some(date_from.clone()),
        date_to: Some(date_to.clone()),
        page: Some(crate::db::models::Page { offset: 0, limit: 10_000 }),
        ..Default::default()
    };
    let result = crate::db::invoices::list(&state.db, filter).await?;

    // 2. Build CSV
    let header =
        "Tip;Serie;Numar;Data;CUI_Partener;Denumire_Partener;Suma_Net;Cota_TVA;Suma_TVA;\
        Total;Moneda;Curs;Scadenta;Observatii";

    let mut rows = vec![header.to_string()];

    for invoice in &result.items {
        let contact = crate::db::contacts::get(&state.db, &invoice.contact_id).await?;

        let data = iso_to_dmy_slash(&invoice.issue_date);
        let scadenta = iso_to_dmy_slash(&invoice.due_date);

        let cui = contact.cui.as_deref().unwrap_or("").to_string();
        let denumire = contact.legal_name.replace(';', " ");
        let observatii = invoice.notes.as_deref().unwrap_or("").replace(';', " ");

        // Compute vat rate from invoice totals
        let vat_rate = if invoice.subtotal_amount > 0.0 {
            (invoice.vat_amount / invoice.subtotal_amount * 100.0).round() as i64
        } else {
            19
        };

        let row = format!(
            "FACT;{serie};{numar};{data};{cui};{denumire};\
            {net:.2};{vat_rate};{tva:.2};{total:.2};RON;1;{scadenta};{observatii}",
            serie = invoice.series,
            numar = invoice.number,
            data = data,
            cui = cui,
            denumire = denumire,
            net = invoice.subtotal_amount,
            vat_rate = vat_rate,
            tva = invoice.vat_amount,
            total = invoice.total_amount,
            scadenta = scadenta,
            observatii = observatii,
        );

        rows.push(row);
    }

    let csv_content = rows.join("\r\n");

    // 3. Save file
    let out_dir = app.path().app_data_dir()?;
    let file_name = format!(
        "winmentor_export_{}_{}_{}.csv",
        company_id, date_from, date_to
    );
    let file_path = out_dir.join(&file_name);
    std::fs::write(&file_path, csv_content.as_bytes()).map_err(AppError::Io)?;

    Ok(file_path.display().to_string())
}

// ─── XLSX export ────────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct XlsxExportFilter {
    pub company_id: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
}

#[tauri::command]
pub async fn export_invoices_xlsx(
    state: State<'_, AppState>,
    filter: XlsxExportFilter,
    output_path: String,
) -> AppResult<()> {
    use rust_xlsxwriter::*;
    use sqlx::Row;

    let pool = &state.db;
    let mut where_clauses = vec!["1=1".to_string()];
    let mut binds: Vec<String> = Vec::new();

    if let Some(cid) = &filter.company_id {
        where_clauses.push(format!("i.company_id = ?{}", binds.len() + 1));
        binds.push(cid.clone());
    }
    if let Some(from) = &filter.date_from {
        where_clauses.push(format!("i.issue_date >= ?{}", binds.len() + 1));
        binds.push(from.clone());
    }
    if let Some(to) = &filter.date_to {
        where_clauses.push(format!("i.issue_date <= ?{}", binds.len() + 1));
        binds.push(to.clone());
    }

    let sql = format!(
        "SELECT i.full_number, i.issue_date, i.due_date, \
         c.legal_name as customer_name, i.subtotal_amount, i.vat_amount, \
         i.total_amount, i.currency, i.status \
         FROM invoices i \
         LEFT JOIN contacts c ON c.id = i.contact_id \
         WHERE {} ORDER BY i.issue_date DESC",
        where_clauses.join(" AND ")
    );

    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    let rows = q.fetch_all(pool).await.map_err(AppError::Database)?;

    let mut workbook = Workbook::new();
    let ws = workbook.add_worksheet();

    // Header row with bold format
    let bold = Format::new().set_bold();
    let headers = ["Nr. Factură", "Data emiterii", "Scadență", "Client",
                   "Bază impozabilă", "TVA", "Total", "Monedă", "Status"];
    for (col, h) in headers.iter().enumerate() {
        ws.write_with_format(0, col as u16, *h, &bold)
            .map_err(|e| AppError::Other(e.to_string()))?;
    }

    for (row_idx, row) in rows.iter().enumerate() {
        let r = (row_idx + 1) as u32;
        ws.write(r, 0, row.try_get::<String, _>("full_number").unwrap_or_default())
            .map_err(|e| AppError::Other(e.to_string()))?;
        ws.write(r, 1, row.try_get::<String, _>("issue_date").unwrap_or_default())
            .map_err(|e| AppError::Other(e.to_string()))?;
        ws.write(r, 2, row.try_get::<String, _>("due_date").unwrap_or_default())
            .map_err(|e| AppError::Other(e.to_string()))?;
        ws.write(r, 3, row.try_get::<String, _>("customer_name").unwrap_or_default())
            .map_err(|e| AppError::Other(e.to_string()))?;
        ws.write(r, 4, row.try_get::<f64, _>("subtotal_amount").unwrap_or(0.0))
            .map_err(|e| AppError::Other(e.to_string()))?;
        ws.write(r, 5, row.try_get::<f64, _>("vat_amount").unwrap_or(0.0))
            .map_err(|e| AppError::Other(e.to_string()))?;
        ws.write(r, 6, row.try_get::<f64, _>("total_amount").unwrap_or(0.0))
            .map_err(|e| AppError::Other(e.to_string()))?;
        ws.write(r, 7, row.try_get::<String, _>("currency").unwrap_or_default())
            .map_err(|e| AppError::Other(e.to_string()))?;
        ws.write(r, 8, row.try_get::<String, _>("status").unwrap_or_default())
            .map_err(|e| AppError::Other(e.to_string()))?;
    }

    // Auto-fit columns
    for col in 0..9u16 {
        ws.set_column_width(col, 18.0).map_err(|e| AppError::Other(e.to_string()))?;
    }

    workbook.save(&output_path).map_err(|e| AppError::Other(e.to_string()))?;
    Ok(())
}
