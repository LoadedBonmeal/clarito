//! Recurring invoice templates — auto-generate invoices on a schedule.

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::db::models::new_id;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RecurringInvoice {
    pub id: String,
    pub company_id: String,
    pub template_name: String,
    pub client_id: String,
    pub frequency: String,
    pub next_issue_date: String,
    pub day_of_month: i64,
    pub auto_submit_anaf: bool,
    pub active: bool,
    pub series: String,
    pub lines_json: String,
    pub notes: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRecurringInput {
    pub company_id: String,
    pub template_name: String,
    pub client_id: String,
    pub frequency: String,
    pub next_issue_date: String,
    pub day_of_month: i64,
    pub auto_submit_anaf: bool,
    pub series: String,
    pub lines_json: String,
    pub notes: Option<String>,
}

pub async fn create(pool: &SqlitePool, input: CreateRecurringInput) -> AppResult<RecurringInvoice> {
    let valid_frequencies = ["monthly", "quarterly", "annual"];
    if !valid_frequencies.contains(&input.frequency.as_str()) {
        return Err(AppError::Validation(
            "Frecvență invalidă. Valori acceptate: monthly, quarterly, annual".into(),
        ));
    }
    if !(1..=28).contains(&input.day_of_month) {
        return Err(AppError::Validation(
            "Ziua lunii trebuie să fie între 1 și 28".into(),
        ));
    }

    let id = new_id();
    let auto = if input.auto_submit_anaf { 1i64 } else { 0i64 };

    sqlx::query(
        "INSERT INTO recurring_invoices \
         (id, company_id, template_name, client_id, frequency, next_issue_date, \
          day_of_month, auto_submit_anaf, series, lines_json, notes) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
    )
    .bind(&id)
    .bind(&input.company_id)
    .bind(&input.template_name)
    .bind(&input.client_id)
    .bind(&input.frequency)
    .bind(&input.next_issue_date)
    .bind(input.day_of_month)
    .bind(auto)
    .bind(&input.series)
    .bind(&input.lines_json)
    .bind(&input.notes)
    .execute(pool)
    .await?;

    get_by_id(pool, &id).await
}

pub async fn get_by_id(pool: &SqlitePool, id: &str) -> AppResult<RecurringInvoice> {
    Ok(sqlx::query_as::<_, RecurringInvoice>(
        "SELECT id, company_id, template_name, client_id, frequency, next_issue_date, \
         day_of_month, auto_submit_anaf, active, series, lines_json, notes, created_at, updated_at \
         FROM recurring_invoices WHERE id = ?1",
    )
    .bind(id)
    .fetch_one(pool)
    .await?)
}

pub async fn list(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<RecurringInvoice>> {
    Ok(sqlx::query_as::<_, RecurringInvoice>(
        "SELECT id, company_id, template_name, client_id, frequency, next_issue_date, \
         day_of_month, auto_submit_anaf, active, series, lines_json, notes, created_at, updated_at \
         FROM recurring_invoices WHERE company_id = ?1 ORDER BY next_issue_date ASC",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?)
}

pub async fn list_due(pool: &SqlitePool) -> AppResult<Vec<RecurringInvoice>> {
    Ok(sqlx::query_as::<_, RecurringInvoice>(
        "SELECT id, company_id, template_name, client_id, frequency, next_issue_date, \
         day_of_month, auto_submit_anaf, active, series, lines_json, notes, created_at, updated_at \
         FROM recurring_invoices \
         WHERE active = 1 AND next_issue_date <= date('now') \
         ORDER BY next_issue_date ASC",
    )
    .fetch_all(pool)
    .await?)
}

pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    let rows = sqlx::query("DELETE FROM recurring_invoices WHERE id = ?1 AND company_id = ?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?
        .rows_affected();

    if rows == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRecurringInput {
    pub template_name: String,
    pub frequency: String,
    pub next_issue_date: String,
    pub day_of_month: i64,
    pub auto_submit_anaf: bool,
    pub active: bool,
    pub series: String,
    pub lines_json: String,
    pub notes: Option<String>,
}

pub async fn update(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    input: UpdateRecurringInput,
) -> AppResult<()> {
    let valid_frequencies = ["monthly", "quarterly", "annual"];
    if !valid_frequencies.contains(&input.frequency.as_str()) {
        return Err(AppError::Validation(
            "Frecvență invalidă. Valori acceptate: monthly, quarterly, annual".into(),
        ));
    }
    if !(1..=28).contains(&input.day_of_month) {
        return Err(AppError::Validation(
            "Ziua lunii trebuie să fie între 1 și 28".into(),
        ));
    }

    let rows = sqlx::query(
        "UPDATE recurring_invoices SET \
            template_name = ?1, frequency = ?2, next_issue_date = ?3, \
            day_of_month = ?4, auto_submit_anaf = ?5, active = ?6, \
            series = ?7, lines_json = ?8, notes = ?9, \
            updated_at = unixepoch() \
         WHERE id = ?10 AND company_id = ?11",
    )
    .bind(&input.template_name)
    .bind(&input.frequency)
    .bind(&input.next_issue_date)
    .bind(input.day_of_month)
    .bind(if input.auto_submit_anaf { 1_i64 } else { 0 })
    .bind(if input.active { 1_i64 } else { 0 })
    .bind(&input.series)
    .bind(&input.lines_json)
    .bind(&input.notes)
    .bind(id)
    .bind(company_id)
    .execute(pool)
    .await?
    .rows_affected();

    if rows == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn set_active(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    active: bool,
) -> AppResult<()> {
    let rows = sqlx::query(
        "UPDATE recurring_invoices SET active = ?1, updated_at = unixepoch() \
         WHERE id = ?2 AND company_id = ?3",
    )
    .bind(if active { 1_i64 } else { 0 })
    .bind(id)
    .bind(company_id)
    .execute(pool)
    .await?
    .rows_affected();

    if rows == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Advance next_issue_date by one frequency period.
pub fn advance_date(current: &str, frequency: &str, day_of_month: u32) -> String {
    use chrono::{Datelike, NaiveDate};
    let day = day_of_month.clamp(1, 28);
    let date = NaiveDate::parse_from_str(current, "%Y-%m-%d")
        .unwrap_or_else(|_| chrono::Local::now().date_naive());

    let next = match frequency {
        "monthly" => {
            let (y, m) = if date.month() == 12 {
                (date.year() + 1, 1)
            } else {
                (date.year(), date.month() + 1)
            };
            NaiveDate::from_ymd_opt(y, m, day).unwrap_or_else(|| {
                NaiveDate::from_ymd_opt(y, m, 28)
                    .expect("day 28 is always valid in any month — constant infallible")
            })
        }
        "quarterly" => {
            let months = date.month() + 3;
            let (y, m) = if months > 12 {
                (date.year() + 1, months - 12)
            } else {
                (date.year(), months)
            };
            NaiveDate::from_ymd_opt(y, m, day).unwrap_or_else(|| {
                NaiveDate::from_ymd_opt(y, m, 28)
                    .expect("day 28 is always valid in any month — constant infallible")
            })
        }
        "annual" => NaiveDate::from_ymd_opt(date.year() + 1, date.month(), day)
            .unwrap_or_else(|| date.with_year(date.year() + 1).unwrap_or(date)),
        _ => date,
    };

    next.format("%Y-%m-%d").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn setup_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .unwrap();
        // Schema mirrors migration 0003 (without the FK on contacts so tests stay
        // self-contained — we never join through this table in these tests).
        sqlx::query(
            "CREATE TABLE recurring_invoices (
                id               TEXT PRIMARY KEY NOT NULL,
                company_id       TEXT NOT NULL,
                template_name    TEXT NOT NULL,
                client_id        TEXT NOT NULL,
                frequency        TEXT NOT NULL,
                next_issue_date  TEXT NOT NULL,
                day_of_month     INTEGER NOT NULL DEFAULT 1,
                auto_submit_anaf INTEGER NOT NULL DEFAULT 0,
                active           INTEGER NOT NULL DEFAULT 1,
                series           TEXT NOT NULL,
                lines_json       TEXT NOT NULL,
                notes            TEXT,
                created_at       INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at       INTEGER NOT NULL DEFAULT (unixepoch())
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    async fn create_sample(pool: &SqlitePool) -> RecurringInvoice {
        create(
            pool,
            CreateRecurringInput {
                company_id: "comp-1".into(),
                template_name: "Hosting lunar".into(),
                client_id: "client-1".into(),
                frequency: "monthly".into(),
                next_issue_date: "2026-06-01".into(),
                day_of_month: 1,
                auto_submit_anaf: false,
                series: "FCT".into(),
                lines_json: "[]".into(),
                notes: None,
            },
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn update_changes_template_name() {
        let pool = setup_pool().await;
        let created = create_sample(&pool).await;

        update(
            &pool,
            &created.id,
            "comp-1",
            UpdateRecurringInput {
                template_name: "Abonament SaaS".into(),
                frequency: "quarterly".into(),
                next_issue_date: "2026-07-15".into(),
                day_of_month: 15,
                auto_submit_anaf: true,
                active: true,
                series: "ABO".into(),
                lines_json: "[]".into(),
                notes: Some("note".into()),
            },
        )
        .await
        .unwrap();

        let refreshed = get_by_id(&pool, &created.id).await.unwrap();
        assert_eq!(refreshed.template_name, "Abonament SaaS");
        assert_eq!(refreshed.frequency, "quarterly");
        assert_eq!(refreshed.day_of_month, 15);
        assert_eq!(refreshed.series, "ABO");
        assert!(refreshed.auto_submit_anaf);
        assert_eq!(refreshed.notes.as_deref(), Some("note"));
    }

    #[tokio::test]
    async fn set_active_toggles_flag() {
        let pool = setup_pool().await;
        let created = create_sample(&pool).await;
        assert!(created.active, "template should start active by default");

        set_active(&pool, &created.id, "comp-1", false)
            .await
            .unwrap();
        let paused = get_by_id(&pool, &created.id).await.unwrap();
        assert!(!paused.active);

        set_active(&pool, &created.id, "comp-1", true)
            .await
            .unwrap();
        let resumed = get_by_id(&pool, &created.id).await.unwrap();
        assert!(resumed.active);
    }

    #[tokio::test]
    async fn update_wrong_company_returns_not_found() {
        let pool = setup_pool().await;
        let created = create_sample(&pool).await;

        let result = update(
            &pool,
            &created.id,
            "wrong-company",
            UpdateRecurringInput {
                template_name: "Should not change".into(),
                frequency: "monthly".into(),
                next_issue_date: "2026-06-01".into(),
                day_of_month: 1,
                auto_submit_anaf: false,
                active: true,
                series: "FCT".into(),
                lines_json: "[]".into(),
                notes: None,
            },
        )
        .await;
        assert!(
            matches!(result, Err(crate::error::AppError::NotFound)),
            "update with wrong company_id should return NotFound"
        );
    }

    #[tokio::test]
    async fn set_active_wrong_company_returns_not_found() {
        let pool = setup_pool().await;
        let created = create_sample(&pool).await;

        let result = set_active(&pool, &created.id, "wrong-company", false).await;
        assert!(
            matches!(result, Err(crate::error::AppError::NotFound)),
            "set_active with wrong company_id should return NotFound"
        );
    }
}
