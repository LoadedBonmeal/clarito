//! Export registru facturi în format XLSX (Microsoft Excel).

use tauri::State;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

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

    // SEC-03: validate path before writing (no UNC, no traversal, must be in $HOME)
    let validated_output = crate::commands::integrations::validate_export_path(&output_path)?;
    let output_path = validated_output.to_string_lossy().to_string();

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
        cq.map(|r| {
            (
                r.try_get::<String, _>("legal_name").unwrap_or_default(),
                r.try_get::<String, _>("cui").unwrap_or_default(),
                r.try_get::<String, _>("city").unwrap_or_default(),
            )
        })
    } else {
        None
    };

    // Extract owned data from sqlx rows before crossing the spawn_blocking boundary
    let row_data: Vec<XlsxRowData> = raw_rows
        .iter()
        .map(|row| XlsxRowData {
            full_number: row.try_get::<String, _>("full_number").unwrap_or_default(),
            issue_date: row.try_get::<String, _>("issue_date").unwrap_or_default(),
            due_date: row.try_get::<String, _>("due_date").unwrap_or_default(),
            customer_name: row
                .try_get::<String, _>("customer_name")
                .unwrap_or_default(),
            customer_cui: row.try_get::<String, _>("customer_cui").unwrap_or_default(),
            customer_city: row
                .try_get::<String, _>("customer_city")
                .unwrap_or_default(),
            currency: row
                .try_get::<String, _>("currency")
                .unwrap_or_else(|_| "RON".to_string()),
            status: row.try_get::<String, _>("status").unwrap_or_default(),
            net: {
                // M5: parse via Decimal for 2dp precision, then convert to f64 for the cell.
                // Avoids raw f64 noise (e.g. 1000.1000000000001) from TEXT storage.
                use rust_decimal::prelude::ToPrimitive as XlsxParseToPrimitive;
                use rust_decimal::Decimal as XlsxParseDecimal;
                use std::str::FromStr as XlsxParseFromStr;
                let s = row
                    .try_get::<String, _>("subtotal_amount")
                    .unwrap_or_default();
                XlsxParseDecimal::from_str(&s)
                    .unwrap_or(XlsxParseDecimal::ZERO)
                    .round_dp(2)
                    .to_f64()
                    .unwrap_or(0.0)
            },
            vat: {
                use rust_decimal::prelude::ToPrimitive as XlsxParseToPrimitive;
                use rust_decimal::Decimal as XlsxParseDecimal;
                use std::str::FromStr as XlsxParseFromStr;
                let s = row.try_get::<String, _>("vat_amount").unwrap_or_default();
                XlsxParseDecimal::from_str(&s)
                    .unwrap_or(XlsxParseDecimal::ZERO)
                    .round_dp(2)
                    .to_f64()
                    .unwrap_or(0.0)
            },
            total: {
                use rust_decimal::prelude::ToPrimitive as XlsxParseToPrimitive;
                use rust_decimal::Decimal as XlsxParseDecimal;
                use std::str::FromStr as XlsxParseFromStr;
                let s = row.try_get::<String, _>("total_amount").unwrap_or_default();
                XlsxParseDecimal::from_str(&s)
                    .unwrap_or(XlsxParseDecimal::ZERO)
                    .round_dp(2)
                    .to_f64()
                    .unwrap_or(0.0)
            },
        })
        .collect();

    let date_from = filter.date_from.clone();
    let date_to = filter.date_to.clone();

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
        let subtitle = format!("Generat la {} · Clarito v1 · RO_CIUS 1.0.1", generated);
        ws.write_with_format(1, 0, &subtitle, &fmt_subtitle)?;
        ws.merge_range(1, 0, 1, 10, &subtitle, &fmt_subtitle)?;

        // Filter info
        let filter_info = match (&date_from, &date_to) {
            (Some(from), Some(to)) => {
                format!("Perioadă: {} — {} · {} facturi", from, to, row_data.len())
            }
            (Some(from), None) => format!("De la: {} · {} facturi", from, row_data.len()),
            (None, Some(to)) => format!("Până la: {} · {} facturi", to, row_data.len()),
            (None, None) => format!("{} facturi", row_data.len()),
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

        let num_headers = [
            (6u16, "Net (RON)", 14.0),
            (7, "TVA (RON)", 14.0),
            (8, "Total (RON)", 16.0),
        ];
        for (col, label, width) in &num_headers {
            ws.write_with_format(header_row, *col, *label, &fmt_header_num)?;
            ws.set_column_width(*col, *width)?;
        }

        // ── Data rows ─────────────────────────────────────────────────────
        use rust_decimal::prelude::ToPrimitive as XlsxToPrimitive;
        use rust_decimal::Decimal as XlsxDecimal;
        let mut total_net_dec = XlsxDecimal::ZERO;
        let mut total_vat_dec = XlsxDecimal::ZERO;
        let mut total_amount_dec = XlsxDecimal::ZERO;

        for (i, row) in row_data.iter().enumerate() {
            let data_row = header_row + 1 + i as u32;
            let is_odd = i % 2 == 0;
            ws.set_row_height(data_row, 18)?;

            let (fmt_text, fmt_num, fmt_mono) = if is_odd {
                (&fmt_row_odd, &fmt_row_num_odd, &fmt_mono_odd)
            } else {
                (&fmt_row_even, &fmt_row_num_even, &fmt_mono_even)
            };

            // Accumulate as Decimal to avoid f64 rounding drift on money sums.
            total_net_dec += XlsxDecimal::try_from(row.net).unwrap_or_default();
            total_vat_dec += XlsxDecimal::try_from(row.vat).unwrap_or_default();
            total_amount_dec += XlsxDecimal::try_from(row.total).unwrap_or_default();

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
                "REJECTED" => &fmt_status_rejected,
                _ => &fmt_status_default,
            };
            let status_label = match row.status.as_str() {
                "VALIDATED" => "✓ Validat",
                "REJECTED" => "✗ Respins",
                "SUBMITTED" => "→ Trimis",
                "DRAFT" => "Schiță",
                "STORNED" => "Stornat",
                other => other,
            };
            ws.write_with_format(data_row, 10, status_label, status_fmt)?;
        }

        // ── Totals row ─────────────────────────────────────────────────────
        let total_row = header_row + 1 + row_data.len() as u32 + 1;
        ws.set_row_height(total_row, 22)?;
        ws.write_with_format(total_row, 0, "TOTAL", &fmt_total_label)?;
        ws.merge_range(total_row, 0, total_row, 5, "TOTAL", &fmt_total_label)?;
        ws.write_with_format(
            total_row,
            6,
            total_net_dec.to_f64().unwrap_or(0.0),
            &fmt_total_num,
        )?;
        ws.write_with_format(
            total_row,
            7,
            total_vat_dec.to_f64().unwrap_or(0.0),
            &fmt_total_num,
        )?;
        ws.write_with_format(
            total_row,
            8,
            total_amount_dec.to_f64().unwrap_or(0.0),
            &fmt_total_num,
        )?;

        // Freeze header rows
        ws.set_freeze_panes(header_row + 1, 0)?;

        workbook
            .save(&output_path)
            .map_err(|e| AppError::Other(e.to_string()))?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))??;

    Ok(())
}
