//! Provizioane (class 15x) — OMFP 1802/2014 pct. 374(1)/377(1).
//!
//! Recognized ONLY when three cumulative conditions hold: (a) a present obligation (legal or
//! constructive) from a past event; (b) a probable outflow of resources; (c) a reliable estimate.
//! GL (idempotent per provision id):
//!   constituire/majorare: D 6812 / C 15x   (`source_type='PROVISION'`)
//!   reluare/utilizare:    D 15x  / C 7812   (`source_type='PROVISION_REVERSE'`)
//! NB: per Cod fiscal art. 26 most provisions are NOT profit-tax deductible (exceptions: warranty /
//! good-execution etc.) — tracked via `deductible` for the D101 computation, informational here.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

use crate::db::gl::post_register_lines;
use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Provision {
    pub id: String,
    pub company_id: String,
    pub account_15x: String,
    pub description: String,
    pub amount: String,
    pub probability: Option<String>,
    pub expected_settlement: Option<String>,
    pub deductible: bool,
    pub status: String,
    pub created_period: String,
    pub reversed_period: Option<String>,
    pub notes: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProvisionInput {
    pub company_id: String,
    pub account_15x: String,
    pub description: String,
    pub amount: String,
    pub probability: Option<String>,
    pub expected_settlement: Option<String>,
    #[serde(default)]
    pub deductible: bool,
    pub created_period: String,
    pub notes: Option<String>,
    // pct. 374(1) — the three cumulative recognition conditions must ALL be confirmed.
    #[serde(default)]
    pub obligation_present: bool,
    #[serde(default)]
    pub outflow_probable: bool,
    #[serde(default)]
    pub estimate_reliable: bool,
}

fn is_ym(s: &str) -> bool {
    s.len() == 7
        && s.as_bytes()[4] == b'-'
        && s[..4].chars().all(|c| c.is_ascii_digit())
        && s[5..].chars().all(|c| c.is_ascii_digit())
}

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

fn validate(input: &CreateProvisionInput) -> AppResult<Decimal> {
    let acct = input.account_15x.trim();
    if !(acct.len() == 4 && acct.starts_with("151")) {
        return Err(AppError::Validation(
            "Contul de provizion trebuie din clasa 151x (1511–1518).".into(),
        ));
    }
    if input.description.trim().is_empty() {
        return Err(AppError::Validation("Descrierea este obligatorie.".into()));
    }
    if !is_ym(&input.created_period) {
        return Err(AppError::Validation(
            "Perioada de constituire trebuie în format AAAA-LL.".into(),
        ));
    }
    if !(input.obligation_present && input.outflow_probable && input.estimate_reliable) {
        return Err(AppError::Validation(
            "Provizionul se recunoaște doar dacă sunt îndeplinite CUMULATIV cele trei condiții \
             (OMFP 1802/2014 pct. 374): obligație actuală, ieșire probabilă de resurse, estimare \
             credibilă."
                .into(),
        ));
    }
    let amt = Decimal::from_str(input.amount.trim())
        .map_err(|_| AppError::Validation("Suma este invalidă.".into()))?;
    if amt <= Decimal::ZERO {
        return Err(AppError::Validation("Suma trebuie să fie pozitivă.".into()));
    }
    Ok(amt)
}

pub async fn create(pool: &SqlitePool, input: CreateProvisionInput) -> AppResult<Provision> {
    let amt = validate(&input)?;
    let id = new_id();
    let now = now_unix();
    let acct = input.account_15x.trim().to_string();

    sqlx::query(
        "INSERT INTO provisions \
         (id, company_id, account_15x, description, amount, probability, expected_settlement, \
          deductible, status, created_period, notes, created_at, updated_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'active',?9,?10,?11,?11)",
    )
    .bind(&id)
    .bind(&input.company_id)
    .bind(&acct)
    .bind(input.description.trim())
    .bind(format!("{amt:.2}"))
    .bind(input.probability.as_deref().map(str::trim))
    .bind(input.expected_settlement.as_deref().map(str::trim))
    .bind(input.deductible as i64)
    .bind(&input.created_period)
    .bind(input.notes.as_deref().map(str::trim))
    .bind(now)
    .execute(pool)
    .await?;

    // Constituire: D 6812 / C 15x.
    post_register_lines(
        pool,
        &input.company_id,
        "DIVERSE",
        "PROVISION",
        &id,
        &period_end(&input.created_period),
        &format!(
            "Constituire provizion ({acct}): {}",
            input.description.trim()
        ),
        vec![("6812".to_string(), acct, amt)],
    )
    .await?;

    fetch(pool, &id, &input.company_id).await
}

pub async fn fetch(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<Provision> {
    sqlx::query_as::<_, Provision>("SELECT * FROM provisions WHERE id=?1 AND company_id=?2")
        .bind(id)
        .bind(company_id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn list(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<Provision>> {
    Ok(sqlx::query_as::<_, Provision>(
        "SELECT * FROM provisions WHERE company_id=?1 ORDER BY created_period DESC, created_at DESC",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?)
}

/// Reluare/utilizare: D 15x / C 7812, mark the provision reversed in `period` (YYYY-MM).
pub async fn reverse(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    period: &str,
) -> AppResult<Provision> {
    if !is_ym(period) {
        return Err(AppError::Validation(
            "Perioada trebuie în format AAAA-LL.".into(),
        ));
    }
    let p = fetch(pool, id, company_id).await?;
    if p.status == "reversed" {
        return Err(AppError::Validation("Provizionul este deja reluat.".into()));
    }
    let amt = Decimal::from_str(&p.amount).unwrap_or(Decimal::ZERO);

    post_register_lines(
        pool,
        company_id,
        "DIVERSE",
        "PROVISION_REVERSE",
        id,
        &period_end(period),
        &format!("Reluare provizion ({}): {}", p.account_15x, p.description),
        vec![(p.account_15x.clone(), "7812".to_string(), amt)],
    )
    .await?;

    sqlx::query(
        "UPDATE provisions SET status='reversed', reversed_period=?3, updated_at=?4 \
         WHERE id=?1 AND company_id=?2",
    )
    .bind(id)
    .bind(company_id)
    .bind(period)
    .bind(now_unix())
    .execute(pool)
    .await?;

    fetch(pool, id, company_id).await
}

pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    // Wave 4 (v0.7.4 audit) — period-lock guard (OMFP 2634/2015 immutability): deleting a
    // provision drops its PROVISION + PROVISION_REVERSE journals; if any of those months is
    // FILED (locked), declared figures would change silently. Fail-closed via `?`.
    let months: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT substr(transaction_date,1,7) FROM gl_journal \
         WHERE company_id=?1 AND source_type IN ('PROVISION','PROVISION_REVERSE') \
           AND source_id=?2",
    )
    .bind(company_id)
    .bind(id)
    .fetch_all(pool)
    .await?;
    for ym in &months {
        if crate::db::period_locks::is_period_locked(pool, company_id, ym).await? {
            return Err(AppError::Validation(format!(
                "Perioada {ym} este blocată (declarație depusă) — provizionul nu poate fi șters. \
                 Deblocați perioada pentru a înregistra o corecție."
            )));
        }
    }

    let res = sqlx::query("DELETE FROM provisions WHERE id=?1 AND company_id=?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    sqlx::query(
        "DELETE FROM gl_journal WHERE company_id=?1 \
         AND source_type IN ('PROVISION','PROVISION_REVERSE') AND source_id=?2",
    )
    .bind(company_id)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
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

    fn input() -> CreateProvisionInput {
        CreateProvisionInput {
            company_id: "co".into(),
            account_15x: "1511".into(),
            description: "Litigiu comercial X".into(),
            amount: "10000.00".into(),
            probability: Some("probabil".into()),
            expected_settlement: None,
            deductible: false,
            created_period: "2026-06".into(),
            notes: None,
            obligation_present: true,
            outflow_probable: true,
            estimate_reliable: true,
        }
    }

    async fn jt(pool: &SqlitePool, source_type: &str) -> (Decimal, Decimal) {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT e.debit, e.credit FROM gl_entry e \
             JOIN gl_journal j ON j.id=e.journal_pk WHERE j.source_type=?1",
        )
        .bind(source_type)
        .fetch_all(pool)
        .await
        .unwrap();
        (
            rows.iter().map(|r| Decimal::from_str(&r.0).unwrap()).sum(),
            rows.iter().map(|r| Decimal::from_str(&r.1).unwrap()).sum(),
        )
    }

    #[tokio::test]
    async fn create_posts_balanced_6812_to_15x() {
        let pool = pool().await;
        let p = create(&pool, input()).await.unwrap();
        assert_eq!(p.status, "active");
        let (d, c) = jt(&pool, "PROVISION").await;
        assert_eq!(d, c);
        assert_eq!(d, Decimal::from_str("10000.00").unwrap());
    }

    #[tokio::test]
    async fn three_conditions_gate_rejects_when_incomplete() {
        let pool = pool().await;
        let bad = create(
            &pool,
            CreateProvisionInput {
                estimate_reliable: false, // one condition missing
                ..input()
            },
        )
        .await;
        assert!(matches!(bad, Err(AppError::Validation(_))));
    }

    #[tokio::test]
    async fn reverse_posts_balanced_15x_to_7812() {
        let pool = pool().await;
        let p = create(&pool, input()).await.unwrap();
        let r = reverse(&pool, &p.id, "co", "2026-09").await.unwrap();
        assert_eq!(r.status, "reversed");
        let (d, c) = jt(&pool, "PROVISION_REVERSE").await;
        assert_eq!(d, c);
        assert_eq!(d, Decimal::from_str("10000.00").unwrap());
        // double reverse rejected
        assert!(reverse(&pool, &p.id, "co", "2026-10").await.is_err());
    }

    #[tokio::test]
    async fn cross_company_isolation() {
        let pool = pool().await;
        let p = create(&pool, input()).await.unwrap();
        assert!(matches!(
            delete(&pool, &p.id, "intrus").await,
            Err(AppError::NotFound)
        ));
        assert!(list(&pool, "intrus").await.unwrap().is_empty());
        assert!(delete(&pool, &p.id, "co").await.is_ok());
    }

    // ── Wave 4 audit: delete refused while any journal month is LOCKED ────────

    #[tokio::test]
    async fn delete_refused_on_locked_period() {
        let pool = pool().await;
        // input(): created_period 2026-06 → PROVISION journal dated 2026-06-30.
        let p = create(&pool, input()).await.unwrap();
        // Reverse in 2026-09 → a second journal (PROVISION_REVERSE) in another month.
        reverse(&pool, &p.id, "co", "2026-09").await.unwrap();

        // Lock ONLY the reversal month — ANY locked journal month must refuse the delete.
        crate::db::period_locks::lock_period(
            &pool,
            "co",
            "2026-09",
            "declaration:D300",
            None,
            None,
        )
        .await
        .unwrap();

        let r = delete(&pool, &p.id, "co").await;
        assert!(
            matches!(r, Err(AppError::Validation(_))),
            "delete with a locked PROVISION_REVERSE month must be a Validation error, got {r:?}"
        );
        // Entity + journals must be untouched.
        assert!(fetch(&pool, &p.id, "co").await.is_ok());
        let (d, _) = jt(&pool, "PROVISION").await;
        assert_eq!(d, Decimal::from_str("10000.00").unwrap());

        // Unlock → delete succeeds and both journals are gone.
        crate::db::period_locks::unlock_period(&pool, "co", "2026-09")
            .await
            .unwrap();
        delete(&pool, &p.id, "co").await.unwrap();
        let (d1, _) = jt(&pool, "PROVISION").await;
        let (d2, _) = jt(&pool, "PROVISION_REVERSE").await;
        assert_eq!(d1, Decimal::ZERO);
        assert_eq!(d2, Decimal::ZERO);
    }
}
