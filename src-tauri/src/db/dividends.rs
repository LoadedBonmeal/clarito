//! Dividende repartizate + impozit pe dividende (Legea 141/2025): **16%** pentru dividende DISTRIBUITE
//! de la 01.01.2026; **10%** tranzitoriu pentru distribuiri anterioare SAU pentru dividende din
//! situații financiare interimare întocmite în 2025 (chiar dacă plata e în 2026). Fiecare înregistrare
//! postează nota contabilă **117 / 457 / 446** (idempotent) și expune obligația pentru Declarația 100,
//! scadentă pe 25 a lunii următoare PLĂȚII (ori 25 ianuarie pentru dividende distribuite, neplătite).

use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::str::FromStr;

use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

/// Cota de impozit pe dividende pentru o distribuire (Legea 141/2025 art. II pct.1 + art. VII):
/// 16% pentru dividende DISTRIBUITE de la 01.01.2026; 10% pentru distribuiri anterioare sau pentru
/// dividende din situații interimare 2025 (`interim_2025`). `distribution_date` = ISO `YYYY-MM-DD`
/// (compararea lexicografică a datelor ISO = cronologică, vezi `db::concedii`).
pub fn dividend_tax_rate(distribution_date: &str, interim_2025: bool) -> i64 {
    if interim_2025 {
        return 10;
    }
    if distribution_date >= "2026-01-01" {
        16
    } else {
        10
    }
}

/// Termenul de plată/declarare a impozitului pe dividende (Cod fiscal art. 43(2)/97(7)/224(4)): 25 a
/// lunii următoare celei în care s-a făcut PLATA; pentru dividende distribuite dar NEPLĂTITE până la
/// finalul anului, 25 ianuarie a anului următor anului distribuirii. Întoarce ISO `YYYY-MM-DD`.
pub fn dividend_tax_deadline(distribution_date: &str, payment_date: Option<&str>) -> String {
    use chrono::Datelike;
    if let Some(pd) = payment_date.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(pd, "%Y-%m-%d") {
            let (y, m) = if d.month() == 12 {
                (d.year() + 1, 1)
            } else {
                (d.year(), d.month() + 1)
            };
            return format!("{y:04}-{m:02}-25");
        }
    }
    let year = distribution_date
        .get(0..4)
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0);
    format!("{:04}-01-25", year + 1)
}

fn round2(d: Decimal) -> Decimal {
    d.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dividend {
    pub id: String,
    pub company_id: String,
    pub distribution_date: String,
    pub payment_date: Option<String>,
    pub gross_amount: String,
    pub tax_rate: i64,
    pub tax_amount: String,
    pub net_amount: String,
    pub interim_2025: bool,
    pub shareholder: Option<String>,
    pub note: Option<String>,
    /// Termenul de plată al impozitului (derivat, nu stocat).
    pub tax_deadline: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DividendInput {
    pub company_id: String,
    pub distribution_date: String,
    pub payment_date: Option<String>,
    pub gross_amount: String,
    #[serde(default)]
    pub interim_2025: bool,
    pub shareholder: Option<String>,
    pub note: Option<String>,
}

fn row_to_dividend(r: &sqlx::sqlite::SqliteRow) -> Dividend {
    let distribution_date: String = r.get("distribution_date");
    let payment_date: Option<String> = r.get("payment_date");
    let tax_deadline = dividend_tax_deadline(&distribution_date, payment_date.as_deref());
    Dividend {
        id: r.get("id"),
        company_id: r.get("company_id"),
        distribution_date,
        payment_date,
        gross_amount: r.get("gross_amount"),
        tax_rate: r.get("tax_rate"),
        tax_amount: r.get("tax_amount"),
        net_amount: r.get("net_amount"),
        interim_2025: r.get::<i64, _>("interim_2025") != 0,
        shareholder: r.get("shareholder"),
        note: r.get("note"),
        tax_deadline,
    }
}

const SELECT: &str =
    "SELECT id, company_id, distribution_date, payment_date, gross_amount, tax_rate, \
     tax_amount, net_amount, interim_2025, shareholder, note FROM dividends";

pub async fn list(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<Dividend>> {
    let rows = sqlx::query(&format!(
        "{SELECT} WHERE company_id = ?1 ORDER BY distribution_date DESC, created_at DESC"
    ))
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(row_to_dividend).collect())
}

pub async fn get(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<Dividend> {
    let row = sqlx::query(&format!("{SELECT} WHERE id = ?1 AND company_id = ?2"))
        .bind(id)
        .bind(company_id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(row_to_dividend(&row))
}

pub async fn create(pool: &SqlitePool, input: DividendInput) -> AppResult<Dividend> {
    let date = input.distribution_date.trim();
    if date.len() != 10 || chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
        return Err(AppError::Validation(
            "Data distribuirii trebuie să fie o dată calendaristică validă (AAAA-LL-ZZ).".into(),
        ));
    }
    let gross = round2(
        Decimal::from_str(input.gross_amount.trim())
            .map_err(|_| AppError::Validation("Sumă brută dividende invalidă.".into()))?,
    );
    if gross <= Decimal::ZERO {
        return Err(AppError::Validation(
            "Suma brută a dividendelor trebuie să fie > 0.".into(),
        ));
    }
    let rate = dividend_tax_rate(date, input.interim_2025);
    let tax = round2(gross * Decimal::new(rate, 2));
    let net = gross - tax; // ambele 2dp → diferența e exactă, deci nota e echilibrată

    let id = new_id();
    sqlx::query(
        "INSERT INTO dividends (id, company_id, distribution_date, payment_date, gross_amount, \
         tax_rate, tax_amount, net_amount, interim_2025, shareholder, note, created_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
    )
    .bind(&id)
    .bind(&input.company_id)
    .bind(date)
    .bind(
        input
            .payment_date
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty()),
    )
    .bind(gross.to_string())
    .bind(rate)
    .bind(tax.to_string())
    .bind(net.to_string())
    .bind(input.interim_2025 as i64)
    .bind(input.shareholder.as_deref())
    .bind(input.note.as_deref())
    .bind(now_unix())
    .execute(pool)
    .await?;

    // Nota contabilă: D 117 (rezultat reportat) brut; C 457 (dividende de plată) net; C 446 (impozit
    // pe dividende) impozit. Σdebit (brut) = Σcredit (net + impozit). Idempotent per dividend id.
    let desc = format!("Repartizare dividende {date} (impozit {rate}%)");
    crate::db::gl::post_manual_journal(
        pool,
        &crate::db::gl::ManualJournal {
            company_id: &input.company_id,
            journal_id: "DIVERSE",
            journal_type: "DIVIDEND",
            source_type: "DIVIDEND",
            source_id: &id,
            date,
            description: &desc,
        },
        &[
            ("117", gross, Decimal::ZERO),
            ("457", Decimal::ZERO, net),
            ("446", Decimal::ZERO, tax),
        ],
    )
    .await?;

    get(pool, &id, &input.company_id).await
}

pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    let res = sqlx::query("DELETE FROM dividends WHERE id = ?1 AND company_id = ?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound); // cross-company / id inexistent
    }
    // Șterge și nota contabilă asociată.
    sqlx::query(
        "DELETE FROM gl_journal WHERE company_id = ?1 AND source_type = 'DIVIDEND' AND source_id = ?2",
    )
    .bind(company_id)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Total impozit pe dividende cu termenul de declarare/plată într-o perioadă (lună), pentru obligația
/// din Declarația 100. `period_ym` = `YYYY-MM`.
pub async fn dividend_tax_due_in_period(
    pool: &SqlitePool,
    company_id: &str,
    period_ym: &str,
) -> AppResult<Decimal> {
    let mut total = Decimal::ZERO;
    for d in list(pool, company_id).await? {
        if d.tax_deadline.starts_with(period_ym) {
            total += Decimal::from_str(d.tax_amount.trim()).unwrap_or(Decimal::ZERO);
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_16_from_2026_else_10() {
        assert_eq!(dividend_tax_rate("2026-01-01", false), 16);
        assert_eq!(dividend_tax_rate("2026-06-15", false), 16);
        assert_eq!(dividend_tax_rate("2025-12-31", false), 10); // distribuit în 2025 → 10%
                                                                // Tranzitoriu: situații interimare 2025, chiar dacă distribuirea/plata e în 2026 → 10%.
        assert_eq!(dividend_tax_rate("2026-03-10", true), 10);
    }

    #[test]
    fn tax_amounts_round_and_balance() {
        let gross = round2(Decimal::from_str("10000").unwrap());
        let tax = round2(gross * Decimal::new(16, 2));
        let net = gross - tax;
        assert_eq!(tax, Decimal::from_str("1600.00").unwrap());
        assert_eq!(net, Decimal::from_str("8400.00").unwrap());
        assert_eq!(net + tax, gross); // nota 117/457/446 e echilibrată
    }

    #[test]
    fn deadline_25th_of_month_after_payment_or_jan() {
        // Plătit în iunie 2026 → 25 iulie 2026.
        assert_eq!(
            dividend_tax_deadline("2026-06-01", Some("2026-06-20")),
            "2026-07-25"
        );
        // Plătit în decembrie → 25 ianuarie anul următor.
        assert_eq!(
            dividend_tax_deadline("2026-12-05", Some("2026-12-30")),
            "2027-01-25"
        );
        // Distribuit dar neplătit → 25 ianuarie a anului următor anului distribuirii.
        assert_eq!(dividend_tax_deadline("2026-09-10", None), "2027-01-25");
        assert_eq!(dividend_tax_deadline("2026-09-10", Some("")), "2027-01-25");
    }

    #[tokio::test]
    async fn create_posts_balanced_117_457_446_and_delete_clears_it() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        // gl_journal.company_id are FK către companies → seed o firmă.
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co-1','12345678','Test SRL','Str 1','Bucuresti','B','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let d = create(
            &pool,
            DividendInput {
                company_id: "co-1".into(),
                distribution_date: "2026-03-15".into(),
                payment_date: Some("2026-03-20".into()),
                gross_amount: "10000".into(),
                interim_2025: false,
                shareholder: Some("Asociat A".into()),
                note: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(d.tax_rate, 16);
        assert_eq!(d.tax_amount, "1600.00");
        assert_eq!(d.net_amount, "8400.00");
        assert_eq!(d.tax_deadline, "2026-04-25"); // plătit în martie → 25 aprilie

        // Nota GL: 3 linii (117/457/446), echilibrată, total debit = brutul.
        let row = sqlx::query(
            "SELECT COALESCE(SUM(CAST(e.debit AS REAL)),0) AS d, \
                    COALESCE(SUM(CAST(e.credit AS REAL)),0) AS c, COUNT(*) AS n \
             FROM gl_entry e JOIN gl_journal j ON e.journal_pk = j.id \
             WHERE j.company_id='co-1' AND j.source_type='DIVIDEND' AND j.source_id=?1",
        )
        .bind(&d.id)
        .fetch_one(&pool)
        .await
        .unwrap();
        let (sd, sc, n): (f64, f64, i64) = (row.get("d"), row.get("c"), row.get("n"));
        assert_eq!(n, 3, "nota are 3 linii (117/457/446)");
        assert!((sd - sc).abs() < 0.005, "nota GL trebuie echilibrată");
        assert!((sd - 10000.0).abs() < 0.005, "debit total = brutul");

        // Ștergerea înlătură și nota contabilă.
        delete(&pool, &d.id, "co-1").await.unwrap();
        let after: i64 =
            sqlx::query("SELECT COUNT(*) AS n FROM gl_journal WHERE source_type='DIVIDEND'")
                .fetch_one(&pool)
                .await
                .unwrap()
                .get("n");
        assert_eq!(after, 0, "ștergerea dividendului curăță nota GL");
    }
}
