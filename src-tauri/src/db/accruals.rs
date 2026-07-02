//! Accruals — cheltuieli/venituri înregistrate în avans (471/472). OMFP 1802/2014 pct. 351:
//! amounts paid/received in the current period that relate to future periods are deferred and
//! recognized over their schedule.
//!
//! Monografie contabilă (self-contained per accrual; idempotent GL):
//!   prepaid  (471): constituire  D 471 / C 6xx(counter)   — `source_type='ACCRUAL_DEFER'`/accrual id
//!                   recunoaștere D 6xx(counter) / C 471    — `source_type='ACCRUAL'`/period (monthly)
//!   deferred (472): constituire  D 7xx(counter) / C 472
//!                   recunoaștere D 472 / C 7xx(counter)
//! The monthly slice = total/months, with the last month carrying the rounding remainder so the
//! slices sum to the total exactly. `run_accruals` aggregates every active accrual into one
//! balanced journal per period (re-runnable: idempotent per (company,'ACCRUAL',period)).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

use crate::db::gl::{post_register_lines, RegisterPostResult};
use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Accrual {
    pub id: String,
    pub company_id: String,
    pub kind: String, // 'prepaid' | 'deferred'
    pub description: String,
    pub counter_acct: String,
    pub total_amount: String,
    pub start_period: String, // YYYY-MM
    pub months: i64,
    pub notes: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAccrualInput {
    pub company_id: String,
    pub kind: String,
    pub description: String,
    pub counter_acct: String,
    pub total_amount: String,
    pub start_period: String,
    pub months: i64,
    pub notes: Option<String>,
}

fn is_ym(s: &str) -> bool {
    s.len() == 7
        && s.as_bytes()[4] == b'-'
        && s[..4].chars().all(|c| c.is_ascii_digit())
        && s[5..].chars().all(|c| c.is_ascii_digit())
}

/// 0-based index of `period` relative to `start` (negative = before start).
fn month_index(start: &str, period: &str) -> i64 {
    let sy: i64 = start[..4].parse().unwrap_or(0);
    let sm: i64 = start[5..].parse().unwrap_or(1);
    let py: i64 = period[..4].parse().unwrap_or(0);
    let pm: i64 = period[5..].parse().unwrap_or(1);
    (py * 12 + (pm - 1)) - (sy * 12 + (sm - 1))
}

/// Last calendar day of a YYYY-MM period, as YYYY-MM-DD.
fn period_end(ym: &str) -> String {
    use chrono::NaiveDate;
    let y: i32 = ym[..4].parse().unwrap_or(2026);
    let m: u32 = ym[5..].parse().unwrap_or(1);
    let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    NaiveDate::from_ymd_opt(ny, nm, 1)
        .and_then(|d| d.pred_opt())
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| format!("{ym}-28"))
}

fn validate(input: &CreateAccrualInput) -> AppResult<Decimal> {
    if !matches!(input.kind.as_str(), "prepaid" | "deferred") {
        return Err(AppError::Validation(
            "Tip invalid (prepaid = cheltuieli în avans / deferred = venituri în avans).".into(),
        ));
    }
    if input.description.trim().is_empty() {
        return Err(AppError::Validation("Descrierea este obligatorie.".into()));
    }
    if input.counter_acct.trim().is_empty() {
        return Err(AppError::Validation(
            "Contul de cheltuială/venit (6xx/7xx) este obligatoriu.".into(),
        ));
    }
    if !is_ym(&input.start_period) {
        return Err(AppError::Validation(
            "Perioada de start trebuie în format AAAA-LL.".into(),
        ));
    }
    if input.months < 1 {
        return Err(AppError::Validation("Numărul de luni trebuie ≥ 1.".into()));
    }
    let amt = Decimal::from_str(input.total_amount.trim())
        .map_err(|_| AppError::Validation("Suma este invalidă.".into()))?;
    if amt <= Decimal::ZERO {
        return Err(AppError::Validation("Suma trebuie să fie pozitivă.".into()));
    }
    Ok(amt)
}

pub async fn create(pool: &SqlitePool, input: CreateAccrualInput) -> AppResult<Accrual> {
    let amt = validate(&input)?;
    let id = new_id();
    let now = now_unix();
    let total_s = format!("{amt:.2}");
    let counter = input.counter_acct.trim().to_string();

    sqlx::query(
        "INSERT INTO accruals \
         (id, company_id, kind, description, counter_acct, total_amount, start_period, months, notes, created_at, updated_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10)",
    )
    .bind(&id)
    .bind(&input.company_id)
    .bind(&input.kind)
    .bind(input.description.trim())
    .bind(&counter)
    .bind(&total_s)
    .bind(&input.start_period)
    .bind(input.months)
    .bind(input.notes.as_deref().map(str::trim))
    .bind(now)
    .execute(pool)
    .await?;

    // Constituire (deferral): move the full amount into 471/472, once.
    let defer_line = if input.kind == "prepaid" {
        ("471".to_string(), counter.clone(), amt) // D 471 / C 6xx
    } else {
        (counter.clone(), "472".to_string(), amt) // D 7xx / C 472
    };
    let label = if input.kind == "prepaid" {
        "cheltuieli"
    } else {
        "venituri"
    };
    post_register_lines(
        pool,
        &input.company_id,
        "DIVERSE",
        "ACCRUAL_DEFER",
        &id,
        &format!("{}-01", &input.start_period),
        &format!("Constituire {label} în avans: {}", input.description.trim()),
        vec![defer_line],
    )
    .await?;

    fetch(pool, &id, &input.company_id).await
}

pub async fn fetch(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<Accrual> {
    sqlx::query_as::<_, Accrual>("SELECT * FROM accruals WHERE id=?1 AND company_id=?2")
        .bind(id)
        .bind(company_id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn list(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<Accrual>> {
    Ok(sqlx::query_as::<_, Accrual>(
        "SELECT * FROM accruals WHERE company_id=?1 ORDER BY start_period DESC, created_at DESC",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?)
}

pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    // Wave 4 (v0.7.4 audit) — period-lock guard (OMFP 2634/2015 immutability): deleting an accrual
    // drops its ACCRUAL_DEFER journal; if that journal's month is FILED (locked), declared figures
    // would change silently. Refuse when ANY affected month is locked (fail-closed via `?`).
    let months: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT substr(transaction_date,1,7) FROM gl_journal \
         WHERE company_id=?1 AND source_type='ACCRUAL_DEFER' AND source_id=?2",
    )
    .bind(company_id)
    .bind(id)
    .fetch_all(pool)
    .await?;
    for ym in &months {
        if crate::db::period_locks::is_period_locked(pool, company_id, ym).await? {
            return Err(AppError::Validation(format!(
                "Perioada {ym} este blocată (declarație depusă) — înregistrarea în avans nu poate \
                 fi ștearsă. Deblocați perioada pentru a înregistra o corecție."
            )));
        }
    }

    let res = sqlx::query("DELETE FROM accruals WHERE id=?1 AND company_id=?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    // Drop its deferral journal; period recognition aggregates self-correct on the next run.
    sqlx::query(
        "DELETE FROM gl_journal WHERE company_id=?1 AND source_type='ACCRUAL_DEFER' AND source_id=?2",
    )
    .bind(company_id)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Recognize every active accrual's slice for `period` (YYYY-MM) into one balanced journal.
pub async fn run_accruals(
    pool: &SqlitePool,
    company_id: &str,
    period: &str,
) -> AppResult<RegisterPostResult> {
    if !is_ym(period) {
        return Err(AppError::Validation(
            "Perioada trebuie în format AAAA-LL.".into(),
        ));
    }
    let rows = list(pool, company_id).await?;
    let mut lines: Vec<(String, String, Decimal)> = Vec::new();
    for a in &rows {
        let idx = month_index(&a.start_period, period);
        if idx < 0 || idx >= a.months {
            continue; // not active in this period
        }
        let total = Decimal::from_str(&a.total_amount).unwrap_or(Decimal::ZERO);
        let base = (total / Decimal::from(a.months))
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
        // Last month carries the remainder so the slices sum to the total exactly.
        let slice = if idx == a.months - 1 {
            total - base * Decimal::from(a.months - 1)
        } else {
            base
        };
        if slice <= Decimal::ZERO {
            continue;
        }
        if a.kind == "prepaid" {
            lines.push((a.counter_acct.clone(), "471".to_string(), slice)); // D 6xx / C 471
        } else {
            lines.push(("472".to_string(), a.counter_acct.clone(), slice)); // D 472 / C 7xx
        }
    }
    post_register_lines(
        pool,
        company_id,
        "DIVERSE",
        "ACCRUAL",
        period,
        &period_end(period),
        "Recunoaștere cheltuieli/venituri în avans (471/472)",
        lines,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co','RO1','T','S','C','CJ','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    fn input() -> CreateAccrualInput {
        CreateAccrualInput {
            company_id: "co".into(),
            kind: "prepaid".into(),
            description: "Asigurare RCA 12 luni".into(),
            counter_acct: "613".into(),
            total_amount: "1200.00".into(),
            start_period: "2026-01".into(),
            months: 12,
            notes: None,
        }
    }

    async fn journal_total(pool: &SqlitePool, source_type: &str) -> (Decimal, Decimal) {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT e.debit, e.credit FROM gl_entry e \
             JOIN gl_journal j ON j.id=e.journal_pk WHERE j.source_type=?1",
        )
        .bind(source_type)
        .fetch_all(pool)
        .await
        .unwrap();
        let d: Decimal = rows.iter().map(|r| Decimal::from_str(&r.0).unwrap()).sum();
        let c: Decimal = rows.iter().map(|r| Decimal::from_str(&r.1).unwrap()).sum();
        (d, c)
    }

    #[tokio::test]
    async fn create_posts_balanced_deferral() {
        let pool = pool().await;
        let a = create(&pool, input()).await.unwrap();
        assert_eq!(a.total_amount, "1200.00");
        // Deferral: D 471 / C 613, balanced, full amount.
        let (d, c) = journal_total(&pool, "ACCRUAL_DEFER").await;
        assert_eq!(d, c);
        assert_eq!(d, Decimal::from_str("1200.00").unwrap());
    }

    #[tokio::test]
    async fn run_recognizes_balanced_slice_and_sums_to_total() {
        let pool = pool().await;
        create(&pool, input()).await.unwrap();
        // Run all 12 months; each slice balanced; total recognized == 1200 exactly.
        for m in 1..=12 {
            let r = run_accruals(&pool, "co", &format!("2026-{m:02}"))
                .await
                .unwrap();
            assert!(r.posted, "month {m} should post a recognition slice");
        }
        let (d, c) = journal_total(&pool, "ACCRUAL").await;
        assert_eq!(d, c, "recognition must be balanced");
        assert_eq!(
            d,
            Decimal::from_str("1200.00").unwrap(),
            "12 monthly slices must sum to the full 1200 (last month carries the remainder)"
        );
    }

    #[tokio::test]
    async fn run_is_idempotent_per_period() {
        let pool = pool().await;
        create(&pool, input()).await.unwrap();
        run_accruals(&pool, "co", "2026-03").await.unwrap();
        run_accruals(&pool, "co", "2026-03").await.unwrap(); // re-run same month
        let (d, _) = journal_total(&pool, "ACCRUAL").await;
        // Only one month's slice present (100), not doubled.
        assert_eq!(d, Decimal::from_str("100.00").unwrap());
    }

    #[tokio::test]
    async fn cross_company_isolation() {
        let pool = pool().await;
        let a = create(&pool, input()).await.unwrap();
        assert!(matches!(
            delete(&pool, &a.id, "intrus").await,
            Err(AppError::NotFound)
        ));
        assert!(list(&pool, "intrus").await.unwrap().is_empty());
        assert!(delete(&pool, &a.id, "co").await.is_ok());
    }

    // ── Wave 4 audit: delete refused while the deferral journal's month is LOCKED ──

    #[tokio::test]
    async fn delete_refused_on_locked_period() {
        let pool = pool().await;
        // input(): start_period 2026-01 → ACCRUAL_DEFER journal dated 2026-01-01.
        let a = create(&pool, input()).await.unwrap();
        crate::db::period_locks::lock_period(
            &pool,
            "co",
            "2026-01",
            "declaration:D300",
            None,
            None,
        )
        .await
        .unwrap();

        let r = delete(&pool, &a.id, "co").await;
        assert!(
            matches!(r, Err(AppError::Validation(_))),
            "delete with a locked ACCRUAL_DEFER month must be a Validation error, got {r:?}"
        );
        // Entity + journal must be untouched.
        assert!(fetch(&pool, &a.id, "co").await.is_ok());
        let (d, _) = journal_total(&pool, "ACCRUAL_DEFER").await;
        assert_eq!(d, Decimal::from_str("1200.00").unwrap());

        // Unlock → delete succeeds and the journal is gone.
        crate::db::period_locks::unlock_period(&pool, "co", "2026-01")
            .await
            .unwrap();
        delete(&pool, &a.id, "co").await.unwrap();
        let (d_after, _) = journal_total(&pool, "ACCRUAL_DEFER").await;
        assert_eq!(d_after, Decimal::ZERO);
    }
}
