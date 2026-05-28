//! Rapoarte TVA și export date contabile.

use serde::{Deserialize, Serialize};
use sqlx::Row;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

// ── VatReport ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VatGroup {
    pub rate: f64,
    pub base_amount: f64,
    pub vat_amount: f64,
    pub invoice_count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VatReport {
    pub date_from: String,
    pub date_to: String,
    pub company_id: Option<String>,
    pub total_base: f64,
    pub total_vat: f64,
    pub total_amount: f64,
    pub invoice_count: i64,
    pub vat_groups: Vec<VatGroup>,
    pub generated_at: i64,
}

/// Generează raportul de TVA pentru perioada specificată.
#[tauri::command]
pub async fn generate_vat_report(
    state: State<'_, AppState>,
    date_from: String,
    date_to: String,
    company_id: Option<String>,
) -> AppResult<VatReport> {
    let pool = &state.db;

    // ?1 date_from, ?2 date_to, ?3 company_id (Option<String> — None → NULL → filter skipped)
    let cid = company_id.as_deref().filter(|s| !s.is_empty());

    // Summary totals — static SQL with nullable company_id bind
    let summary_row = sqlx::query(
        "SELECT COUNT(*) as cnt, \
         COALESCE(SUM(subtotal_amount),0) as base_total, \
         COALESCE(SUM(vat_amount),0) as vat_total, \
         COALESCE(SUM(total_amount),0) as grand_total \
         FROM invoices \
         WHERE status IN ('VALIDATED','SUBMITTED','QUEUED','SENT','ACCEPTED') \
           AND issue_date >= ?1 \
           AND issue_date <= ?2 \
           AND (?3 IS NULL OR company_id = ?3)",
    )
    .bind(&date_from)
    .bind(&date_to)
    .bind(cid)
    .fetch_one(pool)
    .await
    .map_err(AppError::Database)?;

    let invoice_count: i64 = summary_row.try_get("cnt").unwrap_or(0);
    let total_base: f64 = summary_row.try_get("base_total").unwrap_or(0.0);
    let total_vat: f64 = summary_row.try_get("vat_total").unwrap_or(0.0);
    let total_amount: f64 = summary_row.try_get("grand_total").unwrap_or(0.0);

    // VAT groups — join with line items to get breakdown per rate
    let group_rows = sqlx::query(
        "SELECT l.vat_rate, \
         COALESCE(SUM(l.subtotal_amount),0) as base_sum, \
         COALESCE(SUM(l.vat_amount),0) as vat_sum, \
         COUNT(DISTINCT i.id) as inv_count \
         FROM invoice_line_items l \
         JOIN invoices i ON i.id = l.invoice_id \
         WHERE i.status IN ('VALIDATED','SUBMITTED','QUEUED','SENT','ACCEPTED') \
           AND i.issue_date >= ?1 \
           AND i.issue_date <= ?2 \
           AND (?3 IS NULL OR i.company_id = ?3) \
         GROUP BY l.vat_rate ORDER BY l.vat_rate DESC",
    )
    .bind(&date_from)
    .bind(&date_to)
    .bind(cid)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    let vat_groups: Vec<VatGroup> = group_rows
        .iter()
        .map(|r| VatGroup {
            rate: r.try_get::<f64, _>("vat_rate").unwrap_or(0.0),
            base_amount: r.try_get::<f64, _>("base_sum").unwrap_or(0.0),
            vat_amount: r.try_get::<f64, _>("vat_sum").unwrap_or(0.0),
            invoice_count: r.try_get::<i64, _>("inv_count").unwrap_or(0),
        })
        .collect();

    Ok(VatReport {
        date_from,
        date_to,
        company_id,
        total_base,
        total_vat,
        total_amount,
        invoice_count,
        vat_groups,
        generated_at: chrono::Utc::now().timestamp(),
    })
}

// ── export_report ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportReportParams {
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub company_id: Option<String>,
}

/// Exportă raportul ca CSV sau JSON la calea specificată.
/// `format`: "csv" | "json"
#[tauri::command]
pub async fn export_report(
    state: State<'_, AppState>,
    report_type: String,
    params: ExportReportParams,
    format: String,
    output_path: String,
) -> AppResult<String> {
    let date_from = params.date_from.unwrap_or_else(|| "2000-01-01".to_string());
    let date_to = params.date_to.unwrap_or_else(|| "2099-12-31".to_string());

    match report_type.as_str() {
        "vat" => {
            let report = generate_vat_report(
                state,
                date_from,
                date_to,
                params.company_id,
            )
            .await?;

            let content = match format.as_str() {
                "json" => serde_json::to_string_pretty(&report)
                    .map_err(|e| AppError::Other(e.to_string()))?,
                _ => {
                    // CSV format
                    let mut csv =
                        String::from("Cotă TVA,Bază impozabilă,TVA,Nr. Facturi\n");
                    for g in &report.vat_groups {
                        csv.push_str(&format!(
                            "{}%,{:.2},{:.2},{}\n",
                            g.rate, g.base_amount, g.vat_amount, g.invoice_count
                        ));
                    }
                    csv.push_str(&format!(
                        "TOTAL,{:.2},{:.2},{}\n",
                        report.total_base, report.total_vat, report.invoice_count
                    ));
                    csv
                }
            };

            std::fs::write(&output_path, content.as_bytes())
                .map_err(|e| AppError::Other(e.to_string()))?;
            Ok(output_path)
        }
        _ => Err(AppError::Other(format!(
            "Tip raport necunoscut: {}",
            report_type
        ))),
    }
}
