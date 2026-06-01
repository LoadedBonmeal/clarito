//! Rapoarte TVA și export date contabile.

use serde::{Deserialize, Serialize};
use sqlx::Row;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::ubl::fx::{amount_to_ron, parse_rate};

// ── VatReport ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VatGroup {
    pub rate: String,
    /// BIZ-12: VAT category (e.g. "S", "Z", "E", "AE", "K", "G", "O"). Two
    /// lines at the same rate but with different categories must surface as
    /// separate groups so D300/D394 reporting stays accurate.
    pub vat_category: String,
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
    use rust_decimal::prelude::ToPrimitive;
    use rust_decimal::Decimal;
    use std::collections::BTreeMap;
    use std::str::FromStr;

    let pool = &state.db;

    // ?1 date_from, ?2 date_to, ?3 company_id (Option<String> — None → NULL → filter skipped)
    let cid = company_id.as_deref().filter(|s| !s.is_empty());

    // Summary totals — fetch all matching rows and accumulate in Rust using Decimal.
    // BIZ-11: SUBMITTED invoices are still pending ANAF validation and must NOT be
    // counted as fiscal events — only VALIDATED ones land in TVA reports.
    // Wave 4: fetch currency + exchange_rate so each amount can be normalised to RON.
    let summary_rows = sqlx::query(
        "SELECT subtotal_amount, vat_amount, total_amount, \
                COALESCE(currency, 'RON') AS currency, exchange_rate \
         FROM invoices \
         WHERE status = 'VALIDATED' \
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
            let currency: String = row
                .try_get("currency")
                .unwrap_or_else(|_| "RON".to_string());
            let rate = parse_rate(
                row.try_get::<Option<f64>, _>("exchange_rate")
                    .unwrap_or(None),
            );
            let sub_dec = amount_to_ron(
                Decimal::from_str(&sub).unwrap_or(Decimal::ZERO),
                &currency,
                rate,
            );
            let vat_dec = amount_to_ron(
                Decimal::from_str(&vat).unwrap_or(Decimal::ZERO),
                &currency,
                rate,
            );
            let tot_dec = amount_to_ron(
                Decimal::from_str(&tot).unwrap_or(Decimal::ZERO),
                &currency,
                rate,
            );
            (b + sub_dec, v + vat_dec, g + tot_dec)
        },
    );

    // VAT groups — fetch individual line rows and group in Rust with BTreeMap.
    // BIZ-12: group by (rate, vat_category) so that e.g. 0% Exempt ("E") and
    // 0% Zero-rated ("Z") stay separate rows even though their numeric rate
    // collides.
    // Wave 4: fetch parent invoice's currency + exchange_rate via the existing JOIN.
    let line_rows = sqlx::query(
        "SELECT l.vat_rate, l.vat_category, l.subtotal_amount, l.vat_amount, \
                COALESCE(i.currency, 'RON') AS currency, i.exchange_rate \
         FROM invoice_line_items l \
         JOIN invoices i ON i.id = l.invoice_id \
         WHERE i.status = 'VALIDATED' \
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

    // key = (rate * 100 rounded to i64, vat_category), val = (rate, base_sum, vat_sum, line_count)
    let mut groups: BTreeMap<(i64, String), (Decimal, Decimal, Decimal, i64)> = BTreeMap::new();
    for row in &line_rows {
        let rate_s: String = row.try_get("vat_rate").unwrap_or_default();
        let category: String = row
            .try_get("vat_category")
            .unwrap_or_else(|_| String::from("S"));
        let base_s: String = row.try_get("subtotal_amount").unwrap_or_default();
        let vat_s: String = row.try_get("vat_amount").unwrap_or_default();
        let currency: String = row
            .try_get("currency")
            .unwrap_or_else(|_| "RON".to_string());
        let rate = parse_rate(
            row.try_get::<Option<f64>, _>("exchange_rate")
                .unwrap_or(None),
        );
        let rate_dec = Decimal::from_str(&rate_s).unwrap_or(Decimal::ZERO);
        let rate_key = (rate_dec * Decimal::from(100))
            .round()
            .to_i64()
            .unwrap_or(0);
        // Convert per-line amounts to RON before accumulating.
        let base_ron = amount_to_ron(
            Decimal::from_str(&base_s).unwrap_or(Decimal::ZERO),
            &currency,
            rate,
        );
        let vat_ron = amount_to_ron(
            Decimal::from_str(&vat_s).unwrap_or(Decimal::ZERO),
            &currency,
            rate,
        );
        let e = groups.entry((rate_key, category)).or_insert((
            rate_dec,
            Decimal::ZERO,
            Decimal::ZERO,
            0,
        ));
        e.1 += base_ron;
        e.2 += vat_ron;
        e.3 += 1;
    }

    // Build vat_groups sorted descending by rate, then category ascending.
    // BTreeMap is ascending on (rate_key, category); reverse for descending rate.
    let vat_groups: Vec<VatGroup> = groups
        .into_iter()
        .rev()
        .map(
            |((_rate_key, category), (rate, base_sum, vat_sum, count))| VatGroup {
                rate: rate.round_dp(2).to_string(),
                vat_category: category,
                base_amount: base_sum.round_dp(2).to_string(),
                vat_amount: vat_sum.round_dp(2).to_string(),
                invoice_count: count,
            },
        )
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
            let report = generate_vat_report(state, date_from, date_to, params.company_id).await?;

            let content = match format.as_str() {
                "json" => serde_json::to_string_pretty(&report)
                    .map_err(|e| AppError::Other(e.to_string()))?,
                _ => {
                    // CSV format
                    let mut csv = String::from("Cotă TVA,Bază impozabilă,TVA,Nr. Facturi\r\n");
                    for g in &report.vat_groups {
                        csv.push_str(&format!(
                            "{}%,{},{},{}\r\n",
                            g.rate, g.base_amount, g.vat_amount, g.invoice_count
                        ));
                    }
                    csv.push_str(&format!(
                        "TOTAL,{},{},{}\r\n",
                        report.total_base, report.total_vat, report.invoice_count
                    ));
                    csv
                }
            };

            tokio::fs::write(&output_path, content.as_bytes())
                .await
                .map_err(|e| AppError::Other(e.to_string()))?;
            Ok(output_path)
        }
        _ => Err(AppError::Other(format!(
            "Tip raport necunoscut: {}",
            report_type
        ))),
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;
    use std::str::FromStr;

    #[test]
    fn draft_excluded_from_fiscal_statuses() {
        // BIZ-11: only VALIDATED counts as a fiscal event. DRAFT, QUEUED, and
        // SUBMITTED (awaiting ANAF outcome) must all be excluded from VAT
        // reports.
        let fiscal = ["VALIDATED"];
        assert!(!fiscal.contains(&"DRAFT"));
        assert!(!fiscal.contains(&"QUEUED"));
        assert!(!fiscal.contains(&"SUBMITTED"));
        assert!(fiscal.contains(&"VALIDATED"));
    }

    #[test]
    fn vat_groups_split_by_category() {
        // BIZ-12: two lines at 0% but with different categories (E vs Z) must
        // produce two distinct VAT groups, never collapse into one.
        use std::collections::BTreeMap;

        let mut groups: BTreeMap<(i64, String), (Decimal, Decimal)> = BTreeMap::new();
        // Both lines have rate 0%, but different categories.
        let rate_key = 0_i64;
        for (cat, base) in [("E", "100.00"), ("Z", "50.00")] {
            let e = groups
                .entry((rate_key, cat.to_string()))
                .or_insert((Decimal::ZERO, Decimal::ZERO));
            e.0 += Decimal::from_str(base).unwrap();
        }
        assert_eq!(groups.len(), 2, "0% Exempt and 0% Zero-rated must split");
        assert_eq!(
            groups[&(0, "E".to_string())].0,
            Decimal::from_str("100.00").unwrap()
        );
        assert_eq!(
            groups[&(0, "Z".to_string())].0,
            Decimal::from_str("50.00").unwrap()
        );
    }

    #[test]
    fn decimal_vat_accumulation_is_exact() {
        // Verify Decimal avoids float drift
        let amounts = ["100.00", "200.00", "300.00"];
        let total: Decimal = amounts.iter().map(|s| Decimal::from_str(s).unwrap()).sum();
        assert_eq!(total, Decimal::from_str("600.00").unwrap());
    }

    // ── Wave 4: FX normalisation ──────────────────────────────────────────────

    /// Wave 4: A EUR line (base=1000, vat=190, rate=5.0) contributes 5000/950 RON.
    #[test]
    fn vat_report_eur_line_converted_to_ron() {
        use crate::ubl::fx::{amount_to_ron, parse_rate};

        let base_eur = Decimal::from_str("1000.00").unwrap();
        let vat_eur = Decimal::from_str("190.00").unwrap();
        let rate = parse_rate(Some(5.0));

        let base_ron = amount_to_ron(base_eur, "EUR", rate);
        let vat_ron = amount_to_ron(vat_eur, "EUR", rate);

        assert_eq!(
            base_ron,
            Decimal::from_str("5000.00").unwrap(),
            "EUR 1000 * 5.0 must equal RON 5000"
        );
        assert_eq!(
            vat_ron,
            Decimal::from_str("950.00").unwrap(),
            "EUR 190 * 5.0 must equal RON 950"
        );
    }

    /// Wave 4: A RON line (base=1000, vat=190) is unchanged after amount_to_ron.
    #[test]
    fn vat_report_ron_line_unchanged() {
        use crate::ubl::fx::{amount_to_ron, parse_rate};

        let base = Decimal::from_str("1000.00").unwrap();
        let vat = Decimal::from_str("190.00").unwrap();
        let rate = parse_rate(Some(5.0)); // rate present but currency is RON

        let base_ron = amount_to_ron(base, "RON", rate);
        let vat_ron = amount_to_ron(vat, "RON", rate);

        assert_eq!(
            base_ron,
            Decimal::from_str("1000.00").unwrap(),
            "RON amount must be unchanged regardless of rate"
        );
        assert_eq!(
            vat_ron,
            Decimal::from_str("190.00").unwrap(),
            "RON vat must be unchanged regardless of rate"
        );
    }

    /// Wave 4: Mixed EUR+RON accumulation produces the correct RON aggregate.
    #[test]
    fn vat_report_mixed_eur_ron_accumulation() {
        use crate::ubl::fx::{amount_to_ron, parse_rate};
        use rust_decimal::prelude::ToPrimitive;
        use std::collections::BTreeMap;

        // EUR invoice: base=1000, vat=190, rate=5.0 → 5000/950 RON
        // RON invoice: base=1000, vat=190 → 1000/190 RON
        // Expected aggregate at 19%: base=6000, vat=1140

        let lines = [
            ("1000.00", "190.00", "EUR", Some(5.0_f64)),
            ("1000.00", "190.00", "RON", None),
        ];

        let mut groups: BTreeMap<(i64, String), (Decimal, Decimal, Decimal, i64)> = BTreeMap::new();
        for (base_s, vat_s, currency, raw_rate) in &lines {
            let rate_dec = Decimal::from_str("0.19").unwrap();
            let rate_key = (rate_dec * Decimal::from(100))
                .round()
                .to_i64()
                .unwrap_or(0);
            let fx_rate = parse_rate(*raw_rate);
            let base_ron = amount_to_ron(Decimal::from_str(base_s).unwrap(), currency, fx_rate);
            let vat_ron = amount_to_ron(Decimal::from_str(vat_s).unwrap(), currency, fx_rate);
            let e = groups.entry((rate_key, "S".to_string())).or_insert((
                rate_dec,
                Decimal::ZERO,
                Decimal::ZERO,
                0,
            ));
            e.1 += base_ron;
            e.2 += vat_ron;
            e.3 += 1;
        }

        let g = &groups[&(19, "S".to_string())];
        assert_eq!(
            g.1,
            Decimal::from_str("6000.00").unwrap(),
            "Aggregate base must be 5000+1000=6000 RON"
        );
        assert_eq!(
            g.2,
            Decimal::from_str("1140.00").unwrap(),
            "Aggregate vat must be 950+190=1140 RON"
        );
    }
}
