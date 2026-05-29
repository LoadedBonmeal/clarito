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
    pub rate: String,
    pub base_amount: String,
    pub vat_amount: String,
    pub invoice_count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VatReport {
    pub date_from: String,
    pub date_to: String,
    pub company_id: Option<String>,
    pub total_base: String,
    pub total_vat: String,
    pub total_amount: String,
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
    use rust_decimal::Decimal;
    use rust_decimal::prelude::ToPrimitive;
    use std::collections::BTreeMap;
    use std::str::FromStr;

    let pool = &state.db;

    // ?1 date_from, ?2 date_to, ?3 company_id (Option<String> — None → NULL → filter skipped)
    let cid = company_id.as_deref().filter(|s| !s.is_empty());

    // Summary totals — fetch all matching rows and accumulate in Rust using Decimal
    let summary_rows = sqlx::query(
        "SELECT subtotal_amount, vat_amount, total_amount \
         FROM invoices \
         WHERE status IN ('VALIDATED','SUBMITTED','QUEUED') \
           AND issue_date >= ?1 \
           AND issue_date <= ?2 \
           AND (?3 IS NULL OR company_id = ?3)",
    )
    .bind(&date_from)
    .bind(&date_to)
    .bind(cid)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    let invoice_count = summary_rows.len() as i64;
    let (total_base_dec, total_vat_dec, total_amount_dec) = summary_rows.iter().fold(
        (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO),
        |(b, v, g), row| {
            let sub: String = row.try_get("subtotal_amount").unwrap_or_default();
            let vat: String = row.try_get("vat_amount").unwrap_or_default();
            let tot: String = row.try_get("total_amount").unwrap_or_default();
            (
                b + Decimal::from_str(&sub).unwrap_or(Decimal::ZERO),
                v + Decimal::from_str(&vat).unwrap_or(Decimal::ZERO),
                g + Decimal::from_str(&tot).unwrap_or(Decimal::ZERO),
            )
        },
    );

    // VAT groups — fetch individual line rows and group in Rust with BTreeMap
    let line_rows = sqlx::query(
        "SELECT l.vat_rate, l.subtotal_amount, l.vat_amount \
         FROM invoice_line_items l \
         JOIN invoices i ON i.id = l.invoice_id \
         WHERE i.status IN ('VALIDATED','SUBMITTED','QUEUED') \
           AND i.issue_date >= ?1 \
           AND i.issue_date <= ?2 \
           AND (?3 IS NULL OR i.company_id = ?3)",
    )
    .bind(&date_from)
    .bind(&date_to)
    .bind(cid)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    // key = rate * 100 rounded to i64 (e.g. 19% → 1900), val = (base_sum, vat_sum, line_count)
    let mut groups: BTreeMap<i64, (Decimal, Decimal, Decimal, i64)> = BTreeMap::new();
    for row in &line_rows {
        let rate_s: String = row.try_get("vat_rate").unwrap_or_default();
        let base_s: String = row.try_get("subtotal_amount").unwrap_or_default();
        let vat_s: String = row.try_get("vat_amount").unwrap_or_default();
        let rate = Decimal::from_str(&rate_s).unwrap_or(Decimal::ZERO);
        let key = (rate * Decimal::from(100)).round().to_i64().unwrap_or(0);
        let e = groups.entry(key).or_insert((rate, Decimal::ZERO, Decimal::ZERO, 0));
        e.1 += Decimal::from_str(&base_s).unwrap_or(Decimal::ZERO);
        e.2 += Decimal::from_str(&vat_s).unwrap_or(Decimal::ZERO);
        e.3 += 1;
    }

    // Build vat_groups sorted descending by rate (BTreeMap is ascending, reverse)
    let vat_groups: Vec<VatGroup> = groups
        .into_iter()
        .rev()
        .map(|(_key, (rate, base_sum, vat_sum, count))| VatGroup {
            rate: rate.round_dp(2).to_string(),
            base_amount: base_sum.round_dp(2).to_string(),
            vat_amount: vat_sum.round_dp(2).to_string(),
            invoice_count: count,
        })
        .collect();

    Ok(VatReport {
        date_from,
        date_to,
        company_id,
        total_base: total_base_dec.round_dp(2).to_string(),
        total_vat: total_vat_dec.round_dp(2).to_string(),
        total_amount: total_amount_dec.round_dp(2).to_string(),
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
                            "{}%,{},{},{}\n",
                            g.rate, g.base_amount, g.vat_amount, g.invoice_count
                        ));
                    }
                    csv.push_str(&format!(
                        "TOTAL,{},{},{}\n",
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
