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
