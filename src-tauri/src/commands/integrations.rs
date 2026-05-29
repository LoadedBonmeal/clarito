//! Integrări cu software de contabilitate extern: SmartBill, Saga, WinMentor.

use tauri::{AppHandle, State};

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
            use rust_decimal::Decimal;
            use rust_decimal::prelude::ToPrimitive;
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

    if let Some(path) = output_path {
        std::fs::write(&path, csv_content.as_bytes())
            .map_err(|e| AppError::Io(e))?;
        Ok(path)
    } else {
        Ok(csv_content)
    }
}

// ─── WinMentor CSV export ───────────────────────────────────────────────────

#[tauri::command]
pub async fn export_winmentor_csv(
    state: State<'_, AppState>,
    company_id: String,
    date_from: String,
    date_to: String,
    output_path: Option<String>,
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

        // Fetch line items to group by VAT rate — avoids blended rate on mixed-VAT invoices.
        use rust_decimal::Decimal;
        use rust_decimal::prelude::ToPrimitive;
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
            let rate_s: String = lr.try_get("vat_rate").unwrap_or_else(|_| "0.00".to_string());
            let net_s: String  = lr.try_get("subtotal_amount").unwrap_or_else(|_| "0.00".to_string());
            let tva_s: String  = lr.try_get("vat_amount").unwrap_or_else(|_| "0.00".to_string());
            let tot_s: String  = lr.try_get("total_amount").unwrap_or_else(|_| "0.00".to_string());
            let rate_dec = Decimal::from_str(&rate_s).unwrap_or(Decimal::ZERO);
            let net = Decimal::from_str(&net_s).unwrap_or(Decimal::ZERO);
            let tva = Decimal::from_str(&tva_s).unwrap_or(Decimal::ZERO);
            let tot = Decimal::from_str(&tot_s).unwrap_or(Decimal::ZERO);
            let rate_key = rate_dec.round().to_i64().unwrap_or(0);
            let entry = groups.entry(rate_key).or_insert((Decimal::ZERO, Decimal::ZERO, Decimal::ZERO));
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

        for (vat_rate, (net_dec, tva_dec, tot_dec)) in &groups {
            let net = net_dec.to_f64().unwrap_or(0.0);
            let tva = tva_dec.to_f64().unwrap_or(0.0);
            let total = tot_dec.to_f64().unwrap_or(0.0);

            let row = format!(
                "FACT;{serie};{numar};{data};{cui};{denumire};\
                {net:.2};{vat_rate};{tva:.2};{total:.2};RON;1;{scadenta};{observatii}",
                serie = invoice.series,
                numar = invoice.number,
                data = data,
                cui = cui,
                denumire = denumire,
                net = net,
                vat_rate = vat_rate,
                tva = tva,
                total = total,
                scadenta = scadenta,
                observatii = observatii,
            );
            rows.push(row);
        }
    }

    let csv_content = rows.join("\r\n");

    if let Some(path) = output_path {
        std::fs::write(&path, csv_content.as_bytes())
            .map_err(|e| AppError::Io(e))?;
        Ok(path)
    } else {
        Ok(csv_content)
    }
}

// ─── XLSX export ────────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct XlsxExportFilter {
    pub company_id: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
}

#[derive(Clone)]
struct XlsxRowData {
    full_number: String,
    issue_date: String,
    due_date: String,
    customer_name: String,
    customer_cui: String,
    customer_city: String,
    currency: String,
    status: String,
    net: f64,
    vat: f64,
    total: f64,
}

#[tauri::command]
pub async fn export_invoices_xlsx(
    state: State<'_, AppState>,
    filter: XlsxExportFilter,
    output_path: String,
) -> AppResult<()> {
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
        "SELECT i.full_number, i.series, i.number, i.issue_date, i.due_date, i.currency, i.status, \
         i.subtotal_amount, i.vat_amount, i.total_amount, i.anaf_index, i.notes, \
         c.legal_name as customer_name, c.cui as customer_cui, c.address as customer_address, \
         c.city as customer_city, c.county as customer_county \
         FROM invoices i \
         LEFT JOIN contacts c ON c.id = i.contact_id \
         WHERE {} ORDER BY i.issue_date DESC",
        where_clauses.join(" AND ")
    );

    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    let raw_rows = q.fetch_all(pool).await.map_err(AppError::Database)?;

    // Get company info if company_id filter is set
    let company_info: Option<(String, String, String)> = if let Some(cid) = &filter.company_id {
        let cq = sqlx::query("SELECT legal_name, cui, city FROM companies WHERE id = ?1")
            .bind(cid)
            .fetch_optional(pool)
            .await
            .map_err(AppError::Database)?;
        cq.map(|r| (
            r.try_get::<String, _>("legal_name").unwrap_or_default(),
            r.try_get::<String, _>("cui").unwrap_or_default(),
            r.try_get::<String, _>("city").unwrap_or_default(),
        ))
    } else {
        None
    };

    // Extract owned data from sqlx rows before crossing the spawn_blocking boundary
    let row_data: Vec<XlsxRowData> = raw_rows.iter().map(|row| XlsxRowData {
        full_number:   row.try_get::<String, _>("full_number").unwrap_or_default(),
        issue_date:    row.try_get::<String, _>("issue_date").unwrap_or_default(),
        due_date:      row.try_get::<String, _>("due_date").unwrap_or_default(),
        customer_name: row.try_get::<String, _>("customer_name").unwrap_or_default(),
        customer_cui:  row.try_get::<String, _>("customer_cui").unwrap_or_default(),
        customer_city: row.try_get::<String, _>("customer_city").unwrap_or_default(),
        currency:      row.try_get::<String, _>("currency").unwrap_or_else(|_| "RON".to_string()),
        status:        row.try_get::<String, _>("status").unwrap_or_default(),
        net:           {
            let s = row.try_get::<String, _>("subtotal_amount").unwrap_or_default();
            s.parse::<f64>().unwrap_or(0.0)
        },
        vat:           {
            let s = row.try_get::<String, _>("vat_amount").unwrap_or_default();
            s.parse::<f64>().unwrap_or(0.0)
        },
        total:         {
            let s = row.try_get::<String, _>("total_amount").unwrap_or_default();
            s.parse::<f64>().unwrap_or(0.0)
        },
    }).collect();

    let date_from = filter.date_from.clone();
    let date_to   = filter.date_to.clone();

    // CPU-bound workbook creation runs on the blocking thread pool
    tauri::async_runtime::spawn_blocking(move || -> AppResult<()> {
        use rust_xlsxwriter::*;

        let mut workbook = Workbook::new();
        let ws = workbook.add_worksheet();
        ws.set_name("Registru facturi")?;

        // ── Format definitions ─────────────────────────────────────────────
        let fmt_title = Format::new()
            .set_bold()
            .set_font_size(14)
            .set_font_color(Color::RGB(0x1E3A5F));

        let fmt_subtitle = Format::new()
            .set_font_size(10)
            .set_font_color(Color::RGB(0x6B7280));

        let fmt_header = Format::new()
            .set_bold()
            .set_font_size(10)
            .set_font_color(Color::White)
            .set_background_color(Color::RGB(0x1E3A5F))
            .set_align(FormatAlign::Center)
            .set_border(FormatBorder::Thin)
            .set_border_color(Color::RGB(0x1E3A5F));

        let fmt_header_num = Format::new()
            .set_bold()
            .set_font_size(10)
            .set_font_color(Color::White)
            .set_background_color(Color::RGB(0x1E3A5F))
            .set_align(FormatAlign::Right)
            .set_border(FormatBorder::Thin)
            .set_border_color(Color::RGB(0x1E3A5F));

        let fmt_row_odd = Format::new()
            .set_font_size(10)
            .set_background_color(Color::RGB(0xF9FAFB))
            .set_border(FormatBorder::Hair)
            .set_border_color(Color::RGB(0xE5E7EB));

        let fmt_row_even = Format::new()
            .set_font_size(10)
            .set_background_color(Color::White)
            .set_border(FormatBorder::Hair)
            .set_border_color(Color::RGB(0xE5E7EB));

        let fmt_row_num_odd = Format::new()
            .set_font_size(10)
            .set_background_color(Color::RGB(0xF9FAFB))
            .set_num_format("#,##0.00")
            .set_align(FormatAlign::Right)
            .set_border(FormatBorder::Hair)
            .set_border_color(Color::RGB(0xE5E7EB));

        let fmt_row_num_even = Format::new()
            .set_font_size(10)
            .set_background_color(Color::White)
            .set_num_format("#,##0.00")
            .set_align(FormatAlign::Right)
            .set_border(FormatBorder::Hair)
            .set_border_color(Color::RGB(0xE5E7EB));

        let fmt_mono_odd = Format::new()
            .set_font_size(10)
            .set_font_name("Courier New")
            .set_background_color(Color::RGB(0xF9FAFB))
            .set_border(FormatBorder::Hair)
            .set_border_color(Color::RGB(0xE5E7EB));

        let fmt_mono_even = Format::new()
            .set_font_size(10)
            .set_font_name("Courier New")
            .set_background_color(Color::White)
            .set_border(FormatBorder::Hair)
            .set_border_color(Color::RGB(0xE5E7EB));

        let fmt_total_label = Format::new()
            .set_bold()
            .set_font_size(10)
            .set_background_color(Color::RGB(0xF3F4F6))
            .set_border(FormatBorder::Thin)
            .set_border_color(Color::RGB(0xD1D5DB));

        let fmt_total_num = Format::new()
            .set_bold()
            .set_font_size(11)
            .set_font_color(Color::RGB(0x1E3A5F))
            .set_background_color(Color::RGB(0xEFF6FF))
            .set_num_format("#,##0.00")
            .set_align(FormatAlign::Right)
            .set_border(FormatBorder::Medium)
            .set_border_color(Color::RGB(0x1E3A5F));

        let fmt_status_validated = Format::new()
            .set_font_size(10)
            .set_font_color(Color::RGB(0x166534))
            .set_background_color(Color::RGB(0xDCFCE7))
            .set_align(FormatAlign::Center)
            .set_border(FormatBorder::Hair);

        let fmt_status_rejected = Format::new()
            .set_font_size(10)
            .set_font_color(Color::RGB(0x991B1B))
            .set_background_color(Color::RGB(0xFEE2E2))
            .set_align(FormatAlign::Center)
            .set_border(FormatBorder::Hair);

        let fmt_status_default = Format::new()
            .set_font_size(10)
            .set_align(FormatAlign::Center)
            .set_border(FormatBorder::Hair);

        // ── Header section ─────────────────────────────────────────────────
        ws.set_row_height(0, 28)?;
        ws.set_row_height(1, 16)?;
        ws.set_row_height(2, 20)?;

        let title = if let Some((name, cui, city)) = &company_info {
            format!("Registru Facturi — {} ({}) · {}", name, cui, city)
        } else {
            "Registru Facturi e-Factura".to_string()
        };
        ws.write_with_format(0, 0, &title, &fmt_title)?;
        ws.merge_range(0, 0, 0, 10, &title, &fmt_title)?;

        let generated = chrono::Utc::now().format("%d.%m.%Y %H:%M").to_string();
        let subtitle = format!("Generat la {} · RoFactura v1 · RO_CIUS 1.0.1", generated);
        ws.write_with_format(1, 0, &subtitle, &fmt_subtitle)?;
        ws.merge_range(1, 0, 1, 10, &subtitle, &fmt_subtitle)?;

        // Filter info
        let filter_info = match (&date_from, &date_to) {
            (Some(from), Some(to)) => format!("Perioadă: {} — {} · {} facturi", from, to, row_data.len()),
            (Some(from), None)     => format!("De la: {} · {} facturi", from, row_data.len()),
            (None, Some(to))       => format!("Până la: {} · {} facturi", to, row_data.len()),
            (None, None)           => format!("{} facturi", row_data.len()),
        };
        ws.write_with_format(2, 0, &filter_info, &fmt_subtitle)?;
        ws.merge_range(2, 0, 2, 10, &filter_info, &fmt_subtitle)?;

        // ── Column headers (row 4) ─────────────────────────────────────────
        let header_row: u32 = 4;
        ws.set_row_height(header_row, 22)?;

        let text_headers = [
            (0u16, "Nr. Factură", 16.0),
            (1, "Data Emiterii", 13.0),
            (2, "Scadență", 13.0),
            (3, "Client", 30.0),
            (4, "CUI Client", 14.0),
            (5, "Localitate", 16.0),
            (9, "Monedă", 8.0),
            (10, "Status ANAF", 14.0),
        ];
        for (col, label, width) in &text_headers {
            ws.write_with_format(header_row, *col, *label, &fmt_header)?;
            ws.set_column_width(*col, *width)?;
        }

        let num_headers = [(6u16, "Net (RON)", 14.0), (7, "TVA (RON)", 14.0), (8, "Total (RON)", 16.0)];
        for (col, label, width) in &num_headers {
            ws.write_with_format(header_row, *col, *label, &fmt_header_num)?;
            ws.set_column_width(*col, *width)?;
        }

        // ── Data rows ─────────────────────────────────────────────────────
        let mut total_net: f64 = 0.0;
        let mut total_vat: f64 = 0.0;
        let mut total_amount: f64 = 0.0;

        for (i, row) in row_data.iter().enumerate() {
            let data_row = header_row + 1 + i as u32;
            let is_odd = i % 2 == 0;
            ws.set_row_height(data_row, 18)?;

            let (fmt_text, fmt_num, fmt_mono) = if is_odd {
                (&fmt_row_odd, &fmt_row_num_odd, &fmt_mono_odd)
            } else {
                (&fmt_row_even, &fmt_row_num_even, &fmt_mono_even)
            };

            total_net    += row.net;
            total_vat    += row.vat;
            total_amount += row.total;

            ws.write_with_format(data_row, 0, &row.full_number, fmt_mono)?;
            ws.write_with_format(data_row, 1, &row.issue_date, fmt_text)?;
            ws.write_with_format(data_row, 2, &row.due_date, fmt_text)?;
            ws.write_with_format(data_row, 3, &row.customer_name, fmt_text)?;
            ws.write_with_format(data_row, 4, &row.customer_cui, fmt_mono)?;
            ws.write_with_format(data_row, 5, &row.customer_city, fmt_text)?;
            ws.write_with_format(data_row, 6, row.net, fmt_num)?;
            ws.write_with_format(data_row, 7, row.vat, fmt_num)?;
            ws.write_with_format(data_row, 8, row.total, fmt_num)?;
            ws.write_with_format(data_row, 9, &row.currency, fmt_text)?;

            let status_fmt = match row.status.as_str() {
                "VALIDATED" => &fmt_status_validated,
                "REJECTED"  => &fmt_status_rejected,
                _           => &fmt_status_default,
            };
            let status_label = match row.status.as_str() {
                "VALIDATED" => "✓ Validat",
                "REJECTED"  => "✗ Respins",
                "SUBMITTED" => "→ Trimis",
                "DRAFT"     => "Schiță",
                "STORNED"   => "Stornat",
                other       => other,
            };
            ws.write_with_format(data_row, 10, status_label, status_fmt)?;
        }

        // ── Totals row ─────────────────────────────────────────────────────
        let total_row = header_row + 1 + row_data.len() as u32 + 1;
        ws.set_row_height(total_row, 22)?;
        ws.write_with_format(total_row, 0, "TOTAL", &fmt_total_label)?;
        ws.merge_range(total_row, 0, total_row, 5, "TOTAL", &fmt_total_label)?;
        ws.write_with_format(total_row, 6, total_net, &fmt_total_num)?;
        ws.write_with_format(total_row, 7, total_vat, &fmt_total_num)?;
        ws.write_with_format(total_row, 8, total_amount, &fmt_total_num)?;

        // Freeze header rows
        ws.set_freeze_panes(header_row + 1, 0)?;

        workbook.save(&output_path).map_err(|e| AppError::Other(e.to_string()))?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))??;

    Ok(())
}
