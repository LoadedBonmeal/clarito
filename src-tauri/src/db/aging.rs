//! Balanță cu vechime sold (AR/AP aging report).
//!
//! Raport de tip "aged receivables / aged payables" — pentru fiecare partener
//! listează soldul restant (outstanding = total − plăți) defalcat pe tranșe de
//! vechime față de data scadentă:
//!   current  — scadent în viitor (days_overdue ≤ 0)
//!   d1_30    — 1–30 zile întârziere
//!   d31_60   — 31–60 zile
//!   d61_90   — 61–90 zile
//!   over_90  — peste 90 zile
//!
//! Direcție:
//!   Receivable — cont 4111, facturile emise (issued invoices) — clienți
//!   Payable    — cont 401, facturile primite (received invoices) — furnizori
//!
//! Numai facturile cu outstanding > 0.01 (prag Decimal) sunt incluse.
//! Facturile DRAFT / STORNED / REJECTED emise sunt excluse.
//! Received invoices fără plăți = outstanding = total.
//!
//! Data scadentă (due_date):
//!   Receivable — câmpul `due_date` al facturii emise.
//!   Payable    — `issue_date + contact.payment_term_days` dacă există un contact
//!                cu CUI-ul emitentului (comparație canonică, fără prefix "RO");
//!                altfel `issue_date` (payment_term_days implicit 0).

use serde::{Deserialize, Serialize};
use sqlx::Row;
use sqlx::SqlitePool;

use crate::db::models::dec_logged;
use crate::error::{AppError, AppResult};

// ─── Types ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum AgingDirection {
    Receivable,
    Payable,
}

impl AgingDirection {
    /// Parse a direction string ("RECEIVABLE" | "PAYABLE") without implementing
    /// `std::str::FromStr` (which requires an associated error type and a
    /// `Result` return — overkill here; `.ok_or_else` at the call site is cleaner).
    pub fn parse_direction(s: &str) -> Option<Self> {
        match s {
            "RECEIVABLE" => Some(Self::Receivable),
            "PAYABLE" => Some(Self::Payable),
            _ => None,
        }
    }
}

/// O linie per partener (sau linia de totaluri dacă `partner_cui` e empty string).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgingRow {
    pub partner_cui: String,
    pub partner_name: String,
    /// Sold total restant (sumă toate tranșele).
    pub total_outstanding: String,
    /// Facturile neajunse la scadență (days_overdue ≤ 0).
    pub current: String,
    /// 1–30 zile întârziere.
    pub d1_30: String,
    /// 31–60 zile întârziere.
    pub d31_60: String,
    /// 61–90 zile întârziere.
    pub d61_90: String,
    /// Peste 90 zile întârziere.
    pub over_90: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgingReport {
    pub as_of: String,
    pub direction: AgingDirection,
    pub rows: Vec<AgingRow>,
    pub totals: AgingRow,
}

// ─── Helpers ───────────────────────────────────────────────────────────────

use rust_decimal::Decimal;

/// Normalizează CUI-ul: elimină prefixul "RO" (case-insensitive) și spațiile.
/// Comparăm CUI-ul canonic pentru a găsi contactul corespunzător unui emitent
/// din facturile primite, indiferent dacă e stocat "RO12345" sau "12345".
fn canonical_cui(cui: &str) -> String {
    let s = cui.trim().to_uppercase();
    if let Some(rest) = s.strip_prefix("RO") {
        rest.trim().to_string()
    } else {
        s
    }
}

/// Calculează numărul de zile de întârziere față de data scadentă.
/// `as_of` și `due_date` în format "YYYY-MM-DD".
/// Returnează: negative = nu a ajuns la scadență (current), positive = zile depășite.
fn days_overdue(as_of: &str, due_date: &str) -> i64 {
    // Parsed as NaiveDate for subtraction — no time zone needed.
    use chrono::NaiveDate;
    let parse = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok();
    match (parse(as_of), parse(due_date)) {
        (Some(a), Some(d)) => (a - d).num_days(),
        _ => 0, // dacă parse eșuează, tratăm ca "current"
    }
}

/// Plasează un sold în tranșa corectă și actualizează `AgingRow` în loc.
fn bucket_add(row: &mut BucketAcc, amount: Decimal, days: i64) {
    if days <= 0 {
        row.current += amount;
    } else if days <= 30 {
        row.d1_30 += amount;
    } else if days <= 60 {
        row.d31_60 += amount;
    } else if days <= 90 {
        row.d61_90 += amount;
    } else {
        row.over_90 += amount;
    }
    row.total += amount;
}

/// Sumator intern pe tranșe — convertit în `AgingRow` la final.
#[derive(Default)]
struct BucketAcc {
    total: Decimal,
    current: Decimal,
    d1_30: Decimal,
    d31_60: Decimal,
    d61_90: Decimal,
    over_90: Decimal,
}

impl BucketAcc {
    fn into_aging_row(self, partner_cui: String, partner_name: String) -> AgingRow {
        // Always emit exactly 2 decimal places (e.g. "1000.00" not "1000").
        // rust_decimal's to_string() drops trailing zeros from the stored representation,
        // so we use format!("{:.2}") which routes through std::fmt::Display with precision.
        let r2 = |d: Decimal| {
            format!(
                "{:.2}",
                d.round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
            )
        };
        AgingRow {
            partner_cui,
            partner_name,
            total_outstanding: r2(self.total),
            current: r2(self.current),
            d1_30: r2(self.d1_30),
            d31_60: r2(self.d31_60),
            d61_90: r2(self.d61_90),
            over_90: r2(self.over_90),
        }
    }
}

// ─── Main function ─────────────────────────────────────────────────────────

/// Generează raportul de balanță cu vechime sold (aging report).
pub async fn aging_report(
    pool: &SqlitePool,
    company_id: &str,
    direction: AgingDirection,
    as_of: &str,
) -> AppResult<AgingReport> {
    match direction {
        AgingDirection::Receivable => aging_receivable(pool, company_id, as_of).await,
        AgingDirection::Payable => aging_payable(pool, company_id, as_of).await,
    }
}

// ─── Receivable (clienți, cont 4111) ──────────────────────────────────────

async fn aging_receivable(
    pool: &SqlitePool,
    company_id: &str,
    as_of: &str,
) -> AppResult<AgingReport> {
    use std::collections::HashMap;

    // Fetch all VALIDATED invoices (receivable ones — exclude DRAFT/STORNED/REJECTED).
    // We include SUBMITTED/QUEUED because they are outstanding receivables even though
    // not yet confirmed by ANAF; exclude STORNED (original reversed) and REJECTED.
    let inv_rows = sqlx::query(
        "SELECT i.id, i.contact_id, i.due_date, i.total_amount, \
                COALESCE(c.legal_name, '') AS partner_name, \
                COALESCE(c.cui, '') AS partner_cui \
         FROM invoices i \
         LEFT JOIN contacts c ON c.id = i.contact_id \
         WHERE i.company_id = ?1 \
           AND i.status IN ('VALIDATED', 'SUBMITTED', 'QUEUED') \
           AND i.issue_date <= ?2 \
         ORDER BY i.contact_id, i.due_date",
    )
    .bind(company_id)
    .bind(as_of)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    if inv_rows.is_empty() {
        return Ok(AgingReport {
            as_of: as_of.to_string(),
            direction: AgingDirection::Receivable,
            rows: vec![],
            totals: empty_totals(),
        });
    }

    // Batch fetch all payments for these invoices in one query (Decimal-safe TEXT).
    // SQLite doesn't support array binds — use a subquery with company_id.
    let payment_rows = sqlx::query(
        "SELECT p.invoice_id, p.amount \
         FROM payments p \
         INNER JOIN invoices i ON i.id = p.invoice_id \
         WHERE i.company_id = ?1 \
           AND i.status IN ('VALIDATED', 'SUBMITTED', 'QUEUED') \
           AND i.issue_date <= ?2",
    )
    .bind(company_id)
    .bind(as_of)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    let mut paid_map: HashMap<String, Decimal> = HashMap::new();
    for row in &payment_rows {
        let inv_id: String = row.try_get("invoice_id").unwrap_or_default();
        let amt_str: String = row.try_get("amount").unwrap_or_else(|_| "0".to_string());
        let amt = dec_logged("aging.receivable.payment", &amt_str);
        *paid_map.entry(inv_id).or_insert(Decimal::ZERO) += amt;
    }

    // Group by contact_id — keyed on (partner_cui, partner_name) for display.
    // Use contact_id as the aggregation key; name/CUI are display only.
    let mut by_contact: HashMap<String, (BucketAcc, String, String)> = HashMap::new();

    let threshold = Decimal::new(1, 2); // 0.01
    for row in &inv_rows {
        let inv_id: String = row.try_get("id").unwrap_or_default();
        let total_str: String = row
            .try_get("total_amount")
            .unwrap_or_else(|_| "0".to_string());
        let due_date: String = row
            .try_get("due_date")
            .unwrap_or_else(|_| as_of.to_string());
        let contact_id: String = row.try_get("contact_id").unwrap_or_default();
        let partner_name: String = row.try_get("partner_name").unwrap_or_default();
        let partner_cui: String = row.try_get("partner_cui").unwrap_or_default();

        let total = crate::db::invoices::round2(dec_logged("aging.receivable.total", &total_str));
        let paid = paid_map
            .get(&inv_id)
            .copied()
            .unwrap_or(Decimal::ZERO)
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
        let outstanding = total - paid;

        if outstanding <= threshold {
            continue; // fully paid or negligible rounding remnant
        }

        let days = days_overdue(as_of, &due_date);
        let (bucket, _, _) = by_contact
            .entry(contact_id)
            .or_insert_with(|| (BucketAcc::default(), partner_cui, partner_name));
        bucket_add(bucket, outstanding, days);
    }

    // Build sorted rows (alphabetically by partner name).
    let mut rows: Vec<AgingRow> = by_contact
        .into_iter()
        .map(|(_contact_id, (acc, cui, name))| acc.into_aging_row(cui, name))
        .collect();
    rows.sort_by(|a, b| a.partner_name.cmp(&b.partner_name));

    let totals = compute_totals(&rows);

    Ok(AgingReport {
        as_of: as_of.to_string(),
        direction: AgingDirection::Receivable,
        rows,
        totals,
    })
}

// ─── Payable (furnizori, cont 401) ────────────────────────────────────────

async fn aging_payable(pool: &SqlitePool, company_id: &str, as_of: &str) -> AppResult<AgingReport> {
    // Fetch all received invoices for the company with issue_date ≤ as_of.
    let inv_rows = sqlx::query(
        "SELECT ri.id, ri.issuer_cui, ri.issuer_name, ri.total_amount, ri.issue_date \
         FROM received_invoices ri \
         WHERE ri.company_id = ?1 \
           AND ri.issue_date <= ?2 \
         ORDER BY ri.issuer_cui, ri.issue_date",
    )
    .bind(company_id)
    .bind(as_of)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    if inv_rows.is_empty() {
        return Ok(AgingReport {
            as_of: as_of.to_string(),
            direction: AgingDirection::Payable,
            rows: vec![],
            totals: empty_totals(),
        });
    }

    // Batch fetch all received payments for this company up to as_of.
    let payment_rows = sqlx::query(
        "SELECT rip.received_invoice_id, rip.amount \
         FROM received_invoice_payments rip \
         INNER JOIN received_invoices ri ON ri.id = rip.received_invoice_id \
         WHERE ri.company_id = ?1 \
           AND ri.issue_date <= ?2",
    )
    .bind(company_id)
    .bind(as_of)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    use std::collections::HashMap;
    let mut paid_map: HashMap<String, Decimal> = HashMap::new();
    for row in &payment_rows {
        let ri_id: String = row.try_get("received_invoice_id").unwrap_or_default();
        let amt_str: String = row.try_get("amount").unwrap_or_else(|_| "0".to_string());
        let amt = dec_logged("aging.payable.payment", &amt_str);
        *paid_map.entry(ri_id).or_insert(Decimal::ZERO) += amt;
    }

    // Fetch contacts for CUI→payment_term_days lookup (canonical CUI match).
    // Bring ALL contacts for the company (or with NULL company_id for system contacts).
    let contact_rows = sqlx::query(
        "SELECT COALESCE(cui, '') AS cui, COALESCE(payment_term_days, 0) AS payment_term_days \
         FROM contacts \
         WHERE company_id = ?1 AND cui IS NOT NULL",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    // Build map: canonical_cui → payment_term_days
    let mut term_map: HashMap<String, i64> = HashMap::new();
    for row in &contact_rows {
        let cui: String = row.try_get("cui").unwrap_or_default();
        let term: i64 = row.try_get("payment_term_days").unwrap_or(0);
        if !cui.is_empty() {
            term_map.insert(canonical_cui(&cui), term);
        }
    }

    // Group by issuer_cui.
    let mut by_issuer: HashMap<String, (BucketAcc, String)> = HashMap::new();

    let threshold = Decimal::new(1, 2); // 0.01
    for row in &inv_rows {
        let ri_id: String = row.try_get("id").unwrap_or_default();
        let issuer_cui: String = row.try_get("issuer_cui").unwrap_or_default();
        let issuer_name: String = row.try_get("issuer_name").unwrap_or_default();
        let total_str: String = row
            .try_get("total_amount")
            .unwrap_or_else(|_| "0".to_string());
        let issue_date: String = row
            .try_get("issue_date")
            .unwrap_or_else(|_| as_of.to_string());

        // Derive due_date from contact's payment_term_days.
        let canon = canonical_cui(&issuer_cui);
        let term_days = term_map.get(&canon).copied().unwrap_or(0);
        let due_date = add_days_to_date(&issue_date, term_days);

        let total = crate::db::invoices::round2(dec_logged("aging.payable.total", &total_str));
        let paid = paid_map
            .get(&ri_id)
            .copied()
            .unwrap_or(Decimal::ZERO)
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
        let outstanding = total - paid;

        if outstanding <= threshold {
            continue;
        }

        let days = days_overdue(as_of, &due_date);
        let (bucket, _) = by_issuer
            .entry(issuer_cui.clone())
            .or_insert_with(|| (BucketAcc::default(), issuer_name));
        bucket_add(bucket, outstanding, days);
    }

    let mut rows: Vec<AgingRow> = by_issuer
        .into_iter()
        .map(|(cui, (acc, name))| acc.into_aging_row(cui, name))
        .collect();
    rows.sort_by(|a, b| a.partner_name.cmp(&b.partner_name));

    let totals = compute_totals(&rows);

    Ok(AgingReport {
        as_of: as_of.to_string(),
        direction: AgingDirection::Payable,
        rows,
        totals,
    })
}

// ─── Utility ───────────────────────────────────────────────────────────────

fn add_days_to_date(date: &str, days: i64) -> String {
    use chrono::NaiveDate;
    match NaiveDate::parse_from_str(date, "%Y-%m-%d") {
        Ok(d) => {
            let result = d + chrono::Duration::days(days);
            result.format("%Y-%m-%d").to_string()
        }
        Err(_) => date.to_string(),
    }
}

fn empty_totals() -> AgingRow {
    AgingRow {
        partner_cui: String::new(),
        partner_name: String::new(),
        total_outstanding: "0.00".to_string(),
        current: "0.00".to_string(),
        d1_30: "0.00".to_string(),
        d31_60: "0.00".to_string(),
        d61_90: "0.00".to_string(),
        over_90: "0.00".to_string(),
    }
}

fn compute_totals(rows: &[AgingRow]) -> AgingRow {
    let mut acc = BucketAcc::default();
    for r in rows {
        acc.total += dec_logged("aging.totals.total", &r.total_outstanding);
        acc.current += dec_logged("aging.totals.current", &r.current);
        acc.d1_30 += dec_logged("aging.totals.d1_30", &r.d1_30);
        acc.d31_60 += dec_logged("aging.totals.d31_60", &r.d31_60);
        acc.d61_90 += dec_logged("aging.totals.d61_90", &r.d61_90);
        acc.over_90 += dec_logged("aging.totals.over_90", &r.over_90);
    }
    acc.into_aging_row(String::new(), String::new())
}

// ─── CSV export helper ─────────────────────────────────────────────────────

/// Generează CSV (UTF-8 BOM) din raportul de aging.
pub fn aging_to_csv(report: &AgingReport) -> String {
    use crate::commands::journals::{csv_neutralize, csv_num};

    let direction_label = match report.direction {
        AgingDirection::Receivable => "Clienți (Creanțe)",
        AgingDirection::Payable => "Furnizori (Datorii)",
    };

    let mut out = String::from("\u{FEFF}"); // UTF-8 BOM
    out.push_str(&format!(
        "Balanță cu vechime sold — {} — la {}\r\n",
        direction_label, report.as_of
    ));
    out.push_str(
        "CUI Partener,Denumire Partener,Total restant,Curent (neajuns),1-30 zile,31-60 zile,61-90 zile,Peste 90 zile\r\n"
    );

    for row in &report.rows {
        let fields = [
            csv_neutralize(&row.partner_cui),
            csv_neutralize(&row.partner_name),
            csv_num(&row.total_outstanding).to_string(),
            csv_num(&row.current).to_string(),
            csv_num(&row.d1_30).to_string(),
            csv_num(&row.d31_60).to_string(),
            csv_num(&row.d61_90).to_string(),
            csv_num(&row.over_90).to_string(),
        ];
        out.push_str(&fields.join(","));
        out.push_str("\r\n");
    }

    // Totals row
    let t = &report.totals;
    out.push_str(&format!(
        "TOTAL,,{},{},{},{},{},{}\r\n",
        csv_num(&t.total_outstanding),
        csv_num(&t.current),
        csv_num(&t.d1_30),
        csv_num(&t.d31_60),
        csv_num(&t.d61_90),
        csv_num(&t.over_90),
    ));

    out
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        // Company
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co','RO1','Test SRL','Str 1','Cluj','CJ','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    async fn seed_contact(pool: &SqlitePool, id: &str, cui: &str, name: &str, term_days: i64) {
        sqlx::query(
            "INSERT INTO contacts (id, company_id, contact_type, legal_name, cui, \
              vat_payer, is_individual, cash_vat, country, payment_term_days) \
             VALUES (?1,'co','CUSTOMER',?2,?3,0,0,0,'RO',?4)",
        )
        .bind(id)
        .bind(name)
        .bind(cui)
        .bind(term_days)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn seed_invoice(
        pool: &SqlitePool,
        id: &str,
        contact_id: &str,
        total: &str,
        due_date: &str,
        status: &str,
        number: i64,
    ) {
        sqlx::query(
            "INSERT INTO invoices \
             (id, company_id, contact_id, series, number, full_number, \
              issue_date, due_date, currency, subtotal_amount, vat_amount, total_amount, \
              status, payment_means_code, created_at, updated_at) \
             VALUES (?1,'co',?2,'F',?3,'F/'||?3,'2026-01-01',?4,'RON','0','0',?5,?6,'30',1,1)",
        )
        .bind(id)
        .bind(contact_id)
        .bind(number)
        .bind(due_date)
        .bind(total)
        .bind(status)
        .execute(pool)
        .await
        .unwrap();
    }

    async fn seed_payment(pool: &SqlitePool, invoice_id: &str, amount: &str) {
        let id = format!("pmt-{}-{}", invoice_id, amount.replace('.', "_"));
        sqlx::query(
            "INSERT INTO payments \
             (id, invoice_id, company_id, amount, currency, paid_at, method, created_at) \
             VALUES (?1,?2,'co',?3,'RON','2026-01-10','transfer',1)",
        )
        .bind(&id)
        .bind(invoice_id)
        .bind(amount)
        .execute(pool)
        .await
        .unwrap();
    }

    // ─── Test 1: receivable — buckets + totals + partial payment ─────────────

    /// 3 issued invoices for 2 partners, spanning all buckets.
    /// as_of = 2026-06-20
    ///   inv1 (partner A): due 2026-07-01 → current (not yet due, days = -11)
    ///   inv2 (partner A): due 2026-05-05 → d31_60 (46 days overdue), partially paid
    ///   inv3 (partner B): due 2026-02-20 → over_90 (120 days overdue)
    #[tokio::test]
    async fn receivable_buckets_and_partial_payment() {
        let pool = pool().await;
        seed_contact(&pool, "ct_a", "RO100", "Client A SRL", 30).await;
        seed_contact(&pool, "ct_b", "RO200", "Client B SRL", 30).await;

        // inv1 — not yet due (current)
        seed_invoice(
            &pool,
            "inv1",
            "ct_a",
            "500.00",
            "2026-07-01",
            "VALIDATED",
            1,
        )
        .await;
        // inv2 — 46 days overdue (d31_60), partially paid 100
        seed_invoice(
            &pool,
            "inv2",
            "ct_a",
            "300.00",
            "2026-05-05",
            "VALIDATED",
            2,
        )
        .await;
        seed_payment(&pool, "inv2", "100.00").await;
        // inv3 — 120 days overdue (over_90)
        seed_invoice(
            &pool,
            "inv3",
            "ct_b",
            "800.00",
            "2026-02-20",
            "VALIDATED",
            3,
        )
        .await;

        let report = aging_report(&pool, "co", AgingDirection::Receivable, "2026-06-20")
            .await
            .unwrap();

        assert_eq!(report.rows.len(), 2, "2 partners with outstanding");

        let a = report
            .rows
            .iter()
            .find(|r| r.partner_name == "Client A SRL")
            .unwrap();
        // inv1 = 500 current, inv2 = 200 (300-100) d31_60
        assert_eq!(a.current, "500.00", "inv1 not yet due → current");
        assert_eq!(a.d31_60, "200.00", "inv2 partially paid → d31_60");
        assert_eq!(a.total_outstanding, "700.00", "partner A total");

        let b = report
            .rows
            .iter()
            .find(|r| r.partner_name == "Client B SRL")
            .unwrap();
        assert_eq!(b.over_90, "800.00", "inv3 → over_90");

        // Totals row
        assert_eq!(report.totals.total_outstanding, "1500.00");
        assert_eq!(report.totals.current, "500.00");
        assert_eq!(report.totals.d31_60, "200.00");
        assert_eq!(report.totals.over_90, "800.00");
    }

    // ─── Test 2: fully-paid invoice is excluded ───────────────────────────────

    #[tokio::test]
    async fn fully_paid_invoice_excluded() {
        let pool = pool().await;
        seed_contact(&pool, "ct_a", "RO100", "Client A SRL", 30).await;
        seed_invoice(
            &pool,
            "inv1",
            "ct_a",
            "200.00",
            "2026-04-01",
            "VALIDATED",
            1,
        )
        .await;
        // Pay the full amount
        seed_payment(&pool, "inv1", "200.00").await;

        let report = aging_report(&pool, "co", AgingDirection::Receivable, "2026-06-20")
            .await
            .unwrap();

        assert!(
            report.rows.is_empty(),
            "fully paid invoice must not appear in aging"
        );
        assert_eq!(report.totals.total_outstanding, "0.00");
    }

    // ─── Test 3: STORNED / DRAFT invoices excluded ────────────────────────────

    #[tokio::test]
    async fn draft_and_storned_excluded() {
        let pool = pool().await;
        seed_contact(&pool, "ct_a", "RO100", "Client A SRL", 30).await;
        // DRAFT — should be excluded
        seed_invoice(&pool, "inv1", "ct_a", "300.00", "2026-04-01", "DRAFT", 1).await;
        // STORNED — should be excluded
        seed_invoice(&pool, "inv2", "ct_a", "400.00", "2026-04-01", "STORNED", 2).await;
        // VALIDATED — should appear
        seed_invoice(
            &pool,
            "inv3",
            "ct_a",
            "150.00",
            "2026-05-01",
            "VALIDATED",
            3,
        )
        .await;

        let report = aging_report(&pool, "co", AgingDirection::Receivable, "2026-06-20")
            .await
            .unwrap();

        assert_eq!(report.rows.len(), 1, "only VALIDATED inv3 should appear");
        assert_eq!(report.totals.total_outstanding, "150.00");
    }

    // ─── Test 4: payable — contact match + term_days derived due_date ─────────

    /// Received invoices from two issuers:
    ///   issuer "RO300" matched to a contact with payment_term_days=30
    ///     issue_date 2026-05-05 → due 2026-06-04 → 16 days overdue (d1_30)
    ///   issuer "RO999" with NO matching contact → due = issue_date = 2026-03-01 → over_90
    #[tokio::test]
    async fn payable_term_days_and_no_contact() {
        let pool = pool().await;

        // Supplier contact with term_days=30 — CUI "RO300" canonical = "300"
        seed_contact(&pool, "ct_sup", "RO300", "Furnizor Cunoscut SRL", 30).await;

        // Received invoice for known supplier
        sqlx::query(
            "INSERT INTO received_invoices \
             (id, company_id, anaf_download_id, issuer_cui, issuer_name, total_amount, \
              currency, issue_date, xml_path, status, intra_eu_kind) \
             VALUES ('ri1','co','dl1','RO300','Furnizor Cunoscut SRL','1000','RON','2026-05-05','/x.xml','NEW','goods')",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Received invoice for unknown supplier (no contact match)
        sqlx::query(
            "INSERT INTO received_invoices \
             (id, company_id, anaf_download_id, issuer_cui, issuer_name, total_amount, \
              currency, issue_date, xml_path, status, intra_eu_kind) \
             VALUES ('ri2','co','dl2','RO999','Furnizor Necunoscut SRL','500','RON','2026-03-01','/y.xml','NEW','goods')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let report = aging_report(&pool, "co", AgingDirection::Payable, "2026-06-20")
            .await
            .unwrap();

        assert_eq!(report.rows.len(), 2, "2 issuers");

        let known = report
            .rows
            .iter()
            .find(|r| r.partner_cui == "RO300")
            .unwrap();
        // issue 2026-05-05 + 30 days = 2026-06-04; as_of 2026-06-20 → 16 days overdue → d1_30
        assert_eq!(
            known.d1_30, "1000.00",
            "known supplier with term 30d → d1_30 bucket"
        );

        let unknown = report
            .rows
            .iter()
            .find(|r| r.partner_cui == "RO999")
            .unwrap();
        // issue 2026-03-01 + 0 days = 2026-03-01; days overdue = 111 → over_90
        assert_eq!(
            unknown.over_90, "500.00",
            "unknown supplier → due = issue_date → over_90"
        );

        assert_eq!(report.totals.total_outstanding, "1500.00");
    }

    // ─── Test 5: payable — CUI canonical match "300" == "RO300" ─────────────

    #[tokio::test]
    async fn payable_canonical_cui_match() {
        let pool = pool().await;

        // Contact stored WITHOUT "RO" prefix
        seed_contact(&pool, "ct_sup2", "300", "Furnizor Direct SRL", 60).await;

        // Received invoice with "RO300" prefix
        sqlx::query(
            "INSERT INTO received_invoices \
             (id, company_id, anaf_download_id, issuer_cui, issuer_name, total_amount, \
              currency, issue_date, xml_path, status, intra_eu_kind) \
             VALUES ('ri3','co','dl3','RO300','Furnizor Direct SRL','600','RON','2026-05-01','/z.xml','NEW','goods')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let report = aging_report(&pool, "co", AgingDirection::Payable, "2026-06-20")
            .await
            .unwrap();

        // issue 2026-05-01 + 60 = 2026-06-30 → still in the future → current
        let row = &report.rows[0];
        assert_eq!(
            row.current, "600.00",
            "60-day term → still current on 2026-06-20"
        );
    }

    // ─── Test 6: payable — fully paid received invoice excluded ──────────────

    #[tokio::test]
    async fn payable_fully_paid_excluded() {
        let pool = pool().await;

        sqlx::query(
            "INSERT INTO received_invoices \
             (id, company_id, anaf_download_id, issuer_cui, issuer_name, total_amount, \
              currency, issue_date, xml_path, status, intra_eu_kind) \
             VALUES ('ri4','co','dl4','RO400','Furnizor Achitat SRL','400','RON','2026-04-01','/a.xml','NEW','goods')",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Pay fully
        sqlx::query(
            "INSERT INTO received_invoice_payments \
             (id, received_invoice_id, company_id, amount, currency, paid_at, method, created_at) \
             VALUES ('rpmt1','ri4','co','400','RON','2026-04-10','transfer',1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        let report = aging_report(&pool, "co", AgingDirection::Payable, "2026-06-20")
            .await
            .unwrap();

        assert!(
            report.rows.is_empty(),
            "fully paid received invoice must not appear"
        );
    }

    // ─── Test 7: canonical_cui helper ────────────────────────────────────────

    #[test]
    fn canonical_cui_strips_ro_prefix() {
        assert_eq!(canonical_cui("RO12345"), "12345");
        assert_eq!(canonical_cui("ro12345"), "12345");
        assert_eq!(canonical_cui("12345"), "12345");
        assert_eq!(canonical_cui(" RO 456 "), "456");
    }

    // ─── Test 8: days_overdue helper ─────────────────────────────────────────

    #[test]
    fn days_overdue_correct() {
        // 10 days past due
        assert_eq!(days_overdue("2026-06-20", "2026-06-10"), 10);
        // not yet due
        assert_eq!(days_overdue("2026-06-20", "2026-07-01"), -11);
        // exactly on due date
        assert_eq!(days_overdue("2026-06-20", "2026-06-20"), 0);
    }

    #[test]
    fn bucket_boundaries_are_exact() {
        // Each boundary day lands in exactly one bucket (no off-by-one):
        // ≤0 current · 1-30 · 31-60 · 61-90 · >90.
        let cases = [
            (-5_i64, "current"),
            (0, "current"),
            (1, "d1_30"),
            (30, "d1_30"),
            (31, "d31_60"),
            (60, "d31_60"),
            (61, "d61_90"),
            (90, "d61_90"),
            (91, "over_90"),
        ];
        for (days, want) in cases {
            let mut r = BucketAcc::default();
            bucket_add(&mut r, Decimal::ONE, days);
            let got = if r.current == Decimal::ONE {
                "current"
            } else if r.d1_30 == Decimal::ONE {
                "d1_30"
            } else if r.d31_60 == Decimal::ONE {
                "d31_60"
            } else if r.d61_90 == Decimal::ONE {
                "d61_90"
            } else {
                "over_90"
            };
            assert_eq!(got, want, "day {days} must land in {want}, got {got}");
            // The amount is counted exactly once (in total + its single bucket).
            assert_eq!(r.total, Decimal::ONE, "day {days} total must be 1");
        }
    }
}
