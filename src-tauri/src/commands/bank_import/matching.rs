//! Auto-match bank transactions to open invoices.
//!
//! This module SUGGESTS matches only — it never auto-confirms.
//! Confirmation (recording a payment) happens via `match_bank_txn` in commands.rs
//! after explicit user action.
//!
//! Direction:
//!   incoming (+amount) → issued invoices (4111, clients)
//!   outgoing (−amount) → received invoices (401, suppliers)
//!
//! Confidence:
//!   HIGH — amount ≈ outstanding AND (invoice number appears in reference OR CUI matches)
//!   LOW  — amount ≈ outstanding only (no corroborating text signal)

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::str::FromStr;

use crate::error::AppResult;

/// Match confidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum MatchConfidence {
    High,
    Low,
}

/// A single candidate match for one bank transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchSuggestion {
    pub invoice_id: String,
    pub invoice_number: Option<String>,
    pub partner_name: Option<String>,
    /// Remaining outstanding amount as text (Decimal-as-TEXT convention).
    pub outstanding: String,
    /// "issued" | "received"
    pub direction: String,
    pub confidence: MatchConfidence,
}

// ─── Outstanding helpers ──────────────────────────────────────────────────────

/// Parse a Decimal from TEXT — silently returns ZERO on parse failure (mirrors dec_logged).
fn dec(s: &str) -> Decimal {
    Decimal::from_str(s.trim()).unwrap_or(Decimal::ZERO)
}

// ─── Main suggestion function ─────────────────────────────────────────────────

/// Build match suggestions for one bank transaction (no DB writes).
///
/// `txn_amount` is the signed transaction amount (positive = incoming, negative = outgoing).
/// `txn_currency` is the transaction's currency; empty/blank is treated as RON (the same
/// COALESCE(NULLIF(TRIM(...)),'RON') default the payment code applies to invoice currency).
/// Wave 4 audit: candidates must be in the SAME currency — a EUR 500 bank transaction must
/// never suggest a RON 500 invoice just because the numeric amounts collide.
pub async fn suggest_matches(
    pool: &SqlitePool,
    company_id: &str,
    txn_amount: &Decimal,
    txn_currency: &str,
    txn_reference: Option<&str>,
    counterparty_cui: Option<&str>,
) -> AppResult<Vec<MatchSuggestion>> {
    let abs_amount = txn_amount.abs();
    let threshold = Decimal::new(1, 2); // 0.01 amount match tolerance
    let txn_cur = {
        let t = txn_currency.trim();
        if t.is_empty() {
            "RON"
        } else {
            t
        }
    }
    .to_uppercase();

    let mut suggestions: Vec<MatchSuggestion> = Vec::new();

    if *txn_amount >= Decimal::ZERO {
        // ── Incoming: match against OPEN issued invoices (4111) ──────────────
        let inv_rows = sqlx::query(
            "SELECT i.id, i.full_number, i.total_amount, \
                    COALESCE(NULLIF(TRIM(i.currency),''),'RON') AS currency, \
                    COALESCE(c.legal_name,'') AS partner_name, \
                    COALESCE(c.cui,'')        AS partner_cui \
             FROM invoices i \
             LEFT JOIN contacts c ON c.id = i.contact_id \
             WHERE i.company_id = ?1 \
               AND i.status IN ('VALIDATED','SUBMITTED','QUEUED') \
             ORDER BY i.issue_date DESC \
             LIMIT 500",
        )
        .bind(company_id)
        .fetch_all(pool)
        .await?;

        // Batch paid amounts for all matching invoices in one query
        let paid_rows = sqlx::query(
            "SELECT p.invoice_id, COALESCE(SUM(CAST(p.amount AS REAL)), 0.0) AS paid \
             FROM payments p \
             INNER JOIN invoices i ON i.id = p.invoice_id \
             WHERE i.company_id = ?1 \
               AND i.status IN ('VALIDATED','SUBMITTED','QUEUED') \
             GROUP BY p.invoice_id",
        )
        .bind(company_id)
        .fetch_all(pool)
        .await?;

        let mut paid_map: std::collections::HashMap<String, Decimal> =
            std::collections::HashMap::new();
        for pr in &paid_rows {
            let inv_id: String = pr.try_get("invoice_id").unwrap_or_default();
            let paid_f: f64 = pr.try_get("paid").unwrap_or(0.0);
            // Convert f64 via string to avoid Decimal::from(f64) precision issues
            paid_map.insert(inv_id, dec(&format!("{paid_f:.2}")));
        }

        for row in &inv_rows {
            let inv_id: String = row.try_get("id").unwrap_or_default();
            let full_number: Option<String> = row.try_get("full_number").ok();
            let total_str: String = row
                .try_get("total_amount")
                .unwrap_or_else(|_| "0".to_string());
            let partner_name: String = row.try_get("partner_name").unwrap_or_default();
            let partner_cui: String = row.try_get("partner_cui").unwrap_or_default();

            // Wave 4 audit: currency must match — amounts in different currencies never compare.
            let inv_cur: String = row
                .try_get("currency")
                .unwrap_or_else(|_| "RON".to_string());
            if inv_cur.trim().to_uppercase() != txn_cur {
                continue;
            }

            let total = dec(&total_str);
            let paid = paid_map.get(&inv_id).copied().unwrap_or(Decimal::ZERO);
            let outstanding = total - paid;
            if outstanding <= threshold {
                continue; // fully paid or negligible remainder
            }

            // Amount must approximately match
            if (outstanding - abs_amount).abs() > threshold {
                continue;
            }

            // Text signals for HIGH confidence
            let ref_match = txn_reference
                .map(|r| {
                    let r_up = r.to_uppercase();
                    full_number
                        .as_ref()
                        // Guard an empty number: contains("") is always true and would
                        // fabricate HIGH confidence.
                        .filter(|n| !n.trim().is_empty())
                        .map(|n| r_up.contains(&n.to_uppercase()))
                        .unwrap_or(false)
                })
                .unwrap_or(false);
            let cui_match = counterparty_cui
                .map(|cui| {
                    !partner_cui.is_empty()
                        && partner_cui.trim_start_matches('0') == cui.trim().trim_start_matches('0')
                })
                .unwrap_or(false);

            let confidence = if ref_match || cui_match {
                MatchConfidence::High
            } else {
                MatchConfidence::Low
            };

            suggestions.push(MatchSuggestion {
                invoice_id: inv_id,
                invoice_number: full_number,
                partner_name: if partner_name.is_empty() {
                    None
                } else {
                    Some(partner_name)
                },
                outstanding: outstanding.round_dp(2).to_string(),
                direction: "issued".to_string(),
                confidence,
            });
        }
    } else {
        // ── Outgoing: match against OPEN received invoices (401) ─────────────
        // NOTE: received_invoices has SERIES + NUMBER columns (no `invoice_number`) — the old
        // `ri.invoice_number` select failed at runtime ("no such column") and the caller's
        // unwrap_or_default() swallowed it, so OUTGOING suggestions never worked at all
        // (found via the Wave-4 QA follow-up). Compose the display number like gl.rs does.
        let inv_rows = sqlx::query(
            "SELECT ri.id, \
                    TRIM(COALESCE(ri.series,'') || ' ' || COALESCE(ri.number,'')) AS full_number, \
                    ri.total_amount, \
                    COALESCE(NULLIF(TRIM(ri.currency),''),'RON') AS currency, \
                    COALESCE(ri.issuer_name,'') AS partner_name, \
                    COALESCE(ri.issuer_cui,'')  AS partner_cui \
             FROM received_invoices ri \
             WHERE ri.company_id = ?1 \
             ORDER BY ri.issue_date DESC \
             LIMIT 500",
        )
        .bind(company_id)
        .fetch_all(pool)
        .await?;

        let paid_rows = sqlx::query(
            "SELECT rp.received_invoice_id, \
                    COALESCE(SUM(CAST(rp.amount AS REAL)), 0.0) AS paid \
             FROM received_invoice_payments rp \
             INNER JOIN received_invoices ri ON ri.id = rp.received_invoice_id \
             WHERE ri.company_id = ?1 \
             GROUP BY rp.received_invoice_id",
        )
        .bind(company_id)
        .fetch_all(pool)
        .await?;

        let mut paid_map: std::collections::HashMap<String, Decimal> =
            std::collections::HashMap::new();
        for pr in &paid_rows {
            let inv_id: String = pr.try_get("received_invoice_id").unwrap_or_default();
            let paid_f: f64 = pr.try_get("paid").unwrap_or(0.0);
            paid_map.insert(inv_id, dec(&format!("{paid_f:.2}")));
        }

        for row in &inv_rows {
            let inv_id: String = row.try_get("id").unwrap_or_default();
            let full_number: Option<String> = row.try_get("full_number").ok();
            let total_str: String = row
                .try_get("total_amount")
                .unwrap_or_else(|_| "0".to_string());
            let partner_name: String = row.try_get("partner_name").unwrap_or_default();
            let partner_cui: String = row.try_get("partner_cui").unwrap_or_default();

            // Wave 4 audit (QA follow-up): the currency filter must cover the OUTGOING
            // direction too — an EUR −500 bank line must not suggest a RON 500 supplier
            // invoice just because the amounts collide.
            let inv_cur: String = row
                .try_get("currency")
                .unwrap_or_else(|_| "RON".to_string());
            if inv_cur.trim().to_uppercase() != txn_cur {
                continue;
            }

            let total = dec(&total_str);
            let paid = paid_map.get(&inv_id).copied().unwrap_or(Decimal::ZERO);
            let outstanding = total - paid;
            if outstanding <= threshold {
                continue;
            }

            if (outstanding - abs_amount).abs() > threshold {
                continue;
            }

            let ref_match = txn_reference
                .map(|r| {
                    let r_up = r.to_uppercase();
                    full_number
                        .as_ref()
                        // Guard an empty number: contains("") is always true and would
                        // fabricate HIGH confidence.
                        .filter(|n| !n.trim().is_empty())
                        .map(|n| r_up.contains(&n.to_uppercase()))
                        .unwrap_or(false)
                })
                .unwrap_or(false);
            let cui_match = counterparty_cui
                .map(|cui| {
                    !partner_cui.is_empty()
                        && partner_cui.trim_start_matches('0') == cui.trim().trim_start_matches('0')
                })
                .unwrap_or(false);

            let confidence = if ref_match || cui_match {
                MatchConfidence::High
            } else {
                MatchConfidence::Low
            };

            suggestions.push(MatchSuggestion {
                invoice_id: inv_id,
                invoice_number: full_number,
                partner_name: if partner_name.is_empty() {
                    None
                } else {
                    Some(partner_name)
                },
                outstanding: outstanding.round_dp(2).to_string(),
                direction: "received".to_string(),
                confidence,
            });
        }
    }

    // Sort: HIGH first, then LOW
    suggestions.sort_by(|a, b| match (&a.confidence, &b.confidence) {
        (MatchConfidence::High, MatchConfidence::Low) => std::cmp::Ordering::Less,
        (MatchConfidence::Low, MatchConfidence::High) => std::cmp::Ordering::Greater,
        _ => std::cmp::Ordering::Equal,
    });

    Ok(suggestions)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co','RO1','Test SRL','Str 1','Cluj','CJ','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO contacts (id, company_id, contact_type, legal_name, cui) \
             VALUES ('ct','co','CUSTOMER','CLIENT SRL','12345678')",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    async fn seed_issued(pool: &SqlitePool, id: &str, total: &str, full_number: &str) {
        sqlx::query(
            "INSERT INTO invoices \
             (id, company_id, contact_id, series, number, full_number, \
              issue_date, due_date, currency, subtotal_amount, vat_amount, total_amount, \
              status, payment_means_code, created_at, updated_at) \
             VALUES (?1,'co','ct','F',1,?2,'2026-01-01','2026-02-01','RON','0','0',?3,'VALIDATED','30',1,1)",
        )
        .bind(id)
        .bind(full_number)
        .bind(total)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn seed_received(pool: &SqlitePool, id: &str, total: &str, currency: &str) {
        sqlx::query(
            "INSERT INTO received_invoices \
             (id, company_id, anaf_download_id, anaf_index, issuer_cui, issuer_name, \
              series, number, total_amount, net_amount, vat_amount, currency, exchange_rate, \
              issue_date, xml_path, pdf_path, status, is_advance, downloaded_at, created_at) \
             VALUES (?1,'co','DL-1',NULL,'RO11223342','FURNIZOR SRL', \
                     'FF','77',?2,?2,'0',?3,NULL,'2026-01-05','','','APPROVED',0,0,0)",
        )
        .bind(id)
        .bind(total)
        .bind(currency)
        .execute(pool)
        .await
        .unwrap();
    }

    /// The outgoing (supplier) direction must actually WORK — the old SQL selected a
    /// non-existent `ri.invoice_number` column, so every outgoing suggestion silently
    /// failed at runtime (unwrap_or_default at the caller). Regression guard.
    #[tokio::test]
    async fn suggest_outgoing_matches_received_invoice() {
        let pool = pool().await;
        seed_received(&pool, "ri1", "800.00", "RON").await;

        let amount = Decimal::from_str("-800.00").unwrap();
        let sugg = suggest_matches(&pool, "co", &amount, "RON", Some("Plata FF 77"), None)
            .await
            .unwrap();
        assert!(
            sugg.iter().any(|s| s.invoice_id == "ri1"),
            "outgoing txn must suggest the open received invoice, got: {:?}",
            sugg
        );
        let hit = sugg.iter().find(|s| s.invoice_id == "ri1").unwrap();
        assert_eq!(hit.direction, "received");
        assert_eq!(
            hit.confidence,
            MatchConfidence::High,
            "series+number in the reference must give HIGH confidence"
        );
    }

    /// Wave 4 QA follow-up: the currency filter must cover the OUTGOING direction too —
    /// a EUR −800 bank line must not suggest a RON 800 supplier invoice.
    #[tokio::test]
    async fn suggest_outgoing_no_cross_currency_amount_match() {
        let pool = pool().await;
        seed_received(&pool, "ri2", "800.00", "RON").await;

        let amount = Decimal::from_str("-800.00").unwrap();
        let sugg = suggest_matches(&pool, "co", &amount, "EUR", None, None)
            .await
            .unwrap();
        assert!(
            sugg.iter().all(|s| s.invoice_id != "ri2"),
            "a EUR outgoing txn must not match a RON received invoice on amount alone, got: {:?}",
            sugg
        );
    }

    #[tokio::test]
    async fn suggest_high_confidence_ref_match() {
        let pool = pool().await;
        seed_issued(&pool, "inv1", "1500.00", "F2026-001").await;

        let amount = Decimal::from_str("1500.00").unwrap();
        let sugg = suggest_matches(
            &pool,
            "co",
            &amount,
            "RON",
            Some("Incasare factura F2026-001 CLIENT SRL"),
            Some("12345678"),
        )
        .await
        .unwrap();

        assert!(!sugg.is_empty(), "should find a suggestion");
        assert_eq!(
            sugg[0].confidence,
            MatchConfidence::High,
            "ref match should give HIGH confidence"
        );
    }

    #[tokio::test]
    async fn suggest_low_confidence_amount_only() {
        let pool = pool().await;
        seed_issued(&pool, "inv2", "2000.00", "F2026-002").await;

        let amount = Decimal::from_str("2000.00").unwrap();
        let sugg = suggest_matches(&pool, "co", &amount, "RON", None, None)
            .await
            .unwrap();

        assert!(!sugg.is_empty());
        assert_eq!(sugg[0].confidence, MatchConfidence::Low);
    }

    #[tokio::test]
    async fn suggest_no_match_when_fully_paid() {
        let pool = pool().await;
        seed_issued(&pool, "inv3", "500.00", "F2026-003").await;

        // Record a full payment
        sqlx::query(
            "INSERT INTO payments \
             (id, invoice_id, company_id, amount, currency, paid_at, method) \
             VALUES ('p1','inv3','co','500.00','RON','2026-01-15','transfer')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let amount = Decimal::from_str("500.00").unwrap();
        let sugg = suggest_matches(&pool, "co", &amount, "RON", None, None)
            .await
            .unwrap();

        assert!(
            sugg.iter().all(|s| s.invoice_id != "inv3"),
            "fully paid invoice should not be suggested"
        );
    }

    #[tokio::test]
    async fn suggest_high_ranked_before_low() {
        let pool = pool().await;
        // Two invoices with same amount — one ref-matchable, one not
        seed_issued(&pool, "inv4", "777.00", "F2026-004").await;
        sqlx::query(
            "INSERT INTO invoices \
             (id, company_id, contact_id, series, number, full_number, \
              issue_date, due_date, currency, subtotal_amount, vat_amount, total_amount, \
              status, payment_means_code, created_at, updated_at) \
             VALUES ('inv5','co','ct','G',5,'G2026-005','2026-01-01','2026-02-01','RON','0','0','777.00','VALIDATED','30',1,1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let amount = Decimal::from_str("777.00").unwrap();
        let sugg = suggest_matches(
            &pool,
            "co",
            &amount,
            "RON",
            Some("Plata factura F2026-004"),
            None,
        )
        .await
        .unwrap();

        assert!(sugg.len() >= 2);
        // HIGH confidence suggestion must come first
        assert_eq!(sugg[0].confidence, MatchConfidence::High);
    }

    /// Wave 4 audit: matching must be currency-aware — a EUR bank transaction must never
    /// suggest a RON invoice just because the numeric amounts coincide (a match would
    /// create a mis-denominated payment).
    #[tokio::test]
    async fn suggest_no_cross_currency_amount_match() {
        let pool = pool().await;
        seed_issued(&pool, "inv6", "500.00", "F2026-006").await; // RON invoice

        let amount = Decimal::from_str("500.00").unwrap();
        let sugg = suggest_matches(&pool, "co", &amount, "EUR", None, None)
            .await
            .unwrap();
        assert!(
            sugg.iter().all(|s| s.invoice_id != "inv6"),
            "a EUR transaction must not match a RON invoice on amount alone, got: {:?}",
            sugg
        );

        // Same transaction in RON matches as before.
        let sugg_ron = suggest_matches(&pool, "co", &amount, "RON", None, None)
            .await
            .unwrap();
        assert!(
            sugg_ron.iter().any(|s| s.invoice_id == "inv6"),
            "the RON control case must still match"
        );
    }
}
