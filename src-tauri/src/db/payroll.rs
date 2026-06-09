//! Salarizare — angajați + statul de salarii lunar (nucleul D112).
//!
//! Un angajat are salariu brut + deducere personală. Rularea lunară calculează stările individuale
//! (via [`crate::anaf_decl::d112::compute_payroll`], ratele 2026) și postează nota contabilă
//! agregată în GL (via [`crate::db::gl::post_payroll`]).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

use crate::anaf_decl::d112::{compute_payroll, PayrollInput};
use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Employee {
    pub id: String,
    pub company_id: String,
    pub cnp: String,
    pub full_name: String,
    pub gross_salary: String,
    pub personal_deduction: String,
    pub employment_date: Option<String>,
    pub active: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateEmployeeInput {
    pub company_id: String,
    pub cnp: String,
    pub full_name: String,
    pub gross_salary: String,
    #[serde(default)]
    pub personal_deduction: Option<String>,
    pub employment_date: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEmployeeInput {
    pub cnp: Option<String>,
    pub full_name: Option<String>,
    pub gross_salary: Option<String>,
    pub personal_deduction: Option<String>,
    pub employment_date: Option<String>,
    pub active: Option<bool>,
}

const COLS: &str = "id, company_id, cnp, full_name, gross_salary, personal_deduction, \
                    employment_date, active, created_at, updated_at";

pub async fn list(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<Employee>> {
    let q = format!(
        "SELECT {COLS} FROM employees WHERE company_id = ?1 ORDER BY active DESC, full_name"
    );
    Ok(sqlx::query_as::<_, Employee>(&q)
        .bind(company_id)
        .fetch_all(pool)
        .await?)
}

pub async fn get(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<Employee> {
    let q = format!("SELECT {COLS} FROM employees WHERE id = ?1 AND company_id = ?2");
    sqlx::query_as::<_, Employee>(&q)
        .bind(id)
        .bind(company_id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn create(pool: &SqlitePool, input: CreateEmployeeInput) -> AppResult<Employee> {
    if input.full_name.trim().is_empty() {
        return Err(AppError::Validation(
            "Numele angajatului e obligatoriu.".into(),
        ));
    }
    let id = new_id();
    let now = now_unix();
    sqlx::query(
        "INSERT INTO employees (id, company_id, cnp, full_name, gross_salary, personal_deduction, \
         employment_date, active, created_at, updated_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,1,?8,?8)",
    )
    .bind(&id)
    .bind(&input.company_id)
    .bind(input.cnp.trim())
    .bind(input.full_name.trim())
    .bind(&input.gross_salary)
    .bind(input.personal_deduction.as_deref().unwrap_or("0"))
    .bind(&input.employment_date)
    .bind(now)
    .execute(pool)
    .await?;
    get(pool, &id, &input.company_id).await
}

pub async fn update(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    input: UpdateEmployeeInput,
) -> AppResult<Employee> {
    let cur = get(pool, id, company_id).await?;
    sqlx::query(
        "UPDATE employees SET cnp=?3, full_name=?4, gross_salary=?5, personal_deduction=?6, \
         employment_date=?7, active=?8, updated_at=?9 WHERE id=?1 AND company_id=?2",
    )
    .bind(id)
    .bind(company_id)
    .bind(input.cnp.as_deref().unwrap_or(&cur.cnp))
    .bind(input.full_name.as_deref().unwrap_or(&cur.full_name))
    .bind(input.gross_salary.as_deref().unwrap_or(&cur.gross_salary))
    .bind(
        input
            .personal_deduction
            .as_deref()
            .unwrap_or(&cur.personal_deduction),
    )
    .bind(input.employment_date.or(cur.employment_date))
    .bind(input.active.unwrap_or(cur.active))
    .bind(now_unix())
    .execute(pool)
    .await?;
    get(pool, id, company_id).await
}

pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    get(pool, id, company_id).await?;
    sqlx::query("DELETE FROM employees WHERE id=?1 AND company_id=?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// One employee's computed salary state for the month.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmployeeState {
    pub employee_id: String,
    pub full_name: String,
    pub gross: String,
    pub cas: String,
    pub cass: String,
    pub income_tax: String,
    pub net: String,
    pub cam: String,
}

/// The monthly payroll register + the GL post result.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PayrollRun {
    pub states: Vec<EmployeeState>,
    pub total_gross: String,
    pub total_cas: String,
    pub total_cass: String,
    pub total_income_tax: String,
    pub total_net: String,
    pub total_cam: String,
    pub posted: bool,
    pub entry_date: String,
}

/// Compute the monthly salary states for all active employees and post the aggregate to the GL.
pub async fn run_payroll(
    pool: &SqlitePool,
    company_id: &str,
    period_from: &str,
    period_to: &str,
) -> AppResult<PayrollRun> {
    let dec = |s: &str| Decimal::from_str(s).unwrap_or(Decimal::ZERO);
    let employees = list(pool, company_id).await?;
    let mut states = Vec::new();
    let (mut t_gross, mut t_cas, mut t_cass, mut t_tax, mut t_net, mut t_cam) = (
        Decimal::ZERO,
        Decimal::ZERO,
        Decimal::ZERO,
        Decimal::ZERO,
        Decimal::ZERO,
        Decimal::ZERO,
    );
    for e in employees.iter().filter(|e| e.active) {
        let r = compute_payroll(&PayrollInput {
            gross: dec(&e.gross_salary),
            personal_deduction: dec(&e.personal_deduction),
        });
        t_gross += dec(&r.gross);
        t_cas += dec(&r.cas);
        t_cass += dec(&r.cass);
        t_tax += dec(&r.income_tax);
        t_net += dec(&r.net);
        t_cam += dec(&r.cam);
        states.push(EmployeeState {
            employee_id: e.id.clone(),
            full_name: e.full_name.clone(),
            gross: r.gross,
            cas: r.cas,
            cass: r.cass,
            income_tax: r.income_tax,
            net: r.net,
            cam: r.cam,
        });
    }

    let post = crate::db::gl::post_payroll(
        pool,
        company_id,
        period_from,
        period_to,
        t_gross,
        t_cas,
        t_cass,
        t_tax,
        t_cam,
    )
    .await?;

    let f = |d: Decimal| format!("{:.2}", d.round_dp(2));
    Ok(PayrollRun {
        states,
        total_gross: f(t_gross),
        total_cas: f(t_cas),
        total_cass: f(t_cass),
        total_income_tax: f(t_tax),
        total_net: f(t_net),
        total_cam: f(t_cam),
        posted: post.posted,
        entry_date: post.entry_date,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    async fn setup() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co1','12345678','Test SRL','Str 1','Cluj','CJ','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn run_payroll_aggregates_and_posts_gl() {
        let pool = setup().await;
        for (cnp, name) in [("1", "A"), ("2", "B")] {
            create(
                &pool,
                CreateEmployeeInput {
                    company_id: "co1".into(),
                    cnp: cnp.into(),
                    full_name: name.into(),
                    gross_salary: "5000".into(),
                    personal_deduction: Some("0".into()),
                    employment_date: None,
                },
            )
            .await
            .unwrap();
        }
        let run = run_payroll(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        // 2 × (gross 5000, CAS 1250, CASS 500, impozit 325, net 2925, CAM 113).
        assert_eq!(run.total_gross, "10000.00");
        assert_eq!(run.total_cas, "2500.00");
        assert_eq!(run.total_cass, "1000.00");
        assert_eq!(run.total_income_tax, "650.00");
        assert_eq!(run.total_net, "5850.00");
        assert_eq!(run.total_cam, "226.00");
        assert_eq!(run.states.len(), 2);
        assert!(run.posted);

        // GL: 641 debit 10.000 (cheltuieli), 421 credit = net 5.850, 4315 credit 2.500, 646 = CAM.
        let tb = crate::db::gl::trial_balance(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        let bal = |code: &str| {
            tb.rows
                .iter()
                .find(|r| r.account_code == code)
                .map(|r| (r.closing_debit.clone(), r.closing_credit.clone()))
        };
        assert_eq!(bal("641"), Some(("10000.00".into(), "0.00".into())));
        assert_eq!(bal("421"), Some(("0.00".into(), "5850.00".into())));
        assert_eq!(bal("4315"), Some(("0.00".into(), "2500.00".into())));
        assert_eq!(bal("646"), Some(("226.00".into(), "0.00".into())));
        assert!(tb.balanced, "payroll journal balances");
    }
}
