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
    /// D112 asiguratA fields: A_1 tip asigurat (Nomenclator 5, "1"=salariat), A_2 pensionar (0/1),
    /// A_3 tip contract (Nomenclator 12, "N"=normă întreagă, "P1".."P7"=parțial), A_4 ore normă (6/7/8).
    pub tip_asigurat: String,
    pub pensionar: bool,
    pub tip_contract: String,
    pub ore_norma: i64,
    /// art. 146 (5^7) excepție de la baza minimă CAS/CASS part-time: ''/'elev_student'/'ucenic'/
    /// 'dizabilitate'/'contracte_multiple' (pensionarii via `pensionar`).
    pub exceptie_cas_min: String,
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
    #[serde(default)]
    pub tip_asigurat: Option<String>,
    #[serde(default)]
    pub pensionar: Option<bool>,
    #[serde(default)]
    pub tip_contract: Option<String>,
    #[serde(default)]
    pub ore_norma: Option<i64>,
    #[serde(default)]
    pub exceptie_cas_min: Option<String>,
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
    pub tip_asigurat: Option<String>,
    pub pensionar: Option<bool>,
    pub tip_contract: Option<String>,
    pub ore_norma: Option<i64>,
    pub exceptie_cas_min: Option<String>,
}

const COLS: &str = "id, company_id, cnp, full_name, gross_salary, personal_deduction, \
                    employment_date, active, tip_asigurat, pensionar, tip_contract, ore_norma, \
                    exceptie_cas_min, created_at, updated_at";

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

/// Parse a money amount (non-negative), returning its canonical Decimal string. Rejects garbage so a
/// salary never silently becomes 0 and corrupts the payroll totals / GL postings.
fn parse_money(label: &str, s: &str) -> AppResult<String> {
    let d = Decimal::from_str(s.trim()).map_err(|_| {
        AppError::Validation(format!("{label} invalid — folosiți formatul 1234.56."))
    })?;
    if d.is_sign_negative() {
        return Err(AppError::Validation(format!(
            "{label} nu poate fi negativ."
        )));
    }
    Ok(d.to_string())
}

pub async fn create(pool: &SqlitePool, input: CreateEmployeeInput) -> AppResult<Employee> {
    if input.full_name.trim().is_empty() {
        return Err(AppError::Validation(
            "Numele angajatului e obligatoriu.".into(),
        ));
    }
    let gross = parse_money("Salariul brut", &input.gross_salary)?;
    let ded = parse_money(
        "Deducerea personală",
        input.personal_deduction.as_deref().unwrap_or("0"),
    )?;
    let id = new_id();
    let now = now_unix();
    sqlx::query(
        "INSERT INTO employees (id, company_id, cnp, full_name, gross_salary, personal_deduction, \
         employment_date, active, tip_asigurat, pensionar, tip_contract, ore_norma, \
         exceptie_cas_min, created_at, updated_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,1,?8,?9,?10,?11,?12,?13,?13)",
    )
    .bind(&id)
    .bind(&input.company_id)
    .bind(input.cnp.trim())
    .bind(input.full_name.trim())
    .bind(&gross)
    .bind(&ded)
    .bind(&input.employment_date)
    .bind(input.tip_asigurat.as_deref().unwrap_or("1"))
    .bind(input.pensionar.unwrap_or(false))
    .bind(input.tip_contract.as_deref().unwrap_or("N"))
    .bind(input.ore_norma.unwrap_or(8))
    .bind(input.exceptie_cas_min.as_deref().unwrap_or(""))
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
    // Validate any supplied money fields (partial update) so garbage never silently becomes 0.
    let gross = match input.gross_salary.as_deref() {
        Some(s) => parse_money("Salariul brut", s)?,
        None => cur.gross_salary.clone(),
    };
    let ded = match input.personal_deduction.as_deref() {
        Some(s) => parse_money("Deducerea personală", s)?,
        None => cur.personal_deduction.clone(),
    };
    sqlx::query(
        "UPDATE employees SET cnp=?3, full_name=?4, gross_salary=?5, personal_deduction=?6, \
         employment_date=?7, active=?8, tip_asigurat=?9, pensionar=?10, tip_contract=?11, \
         ore_norma=?12, exceptie_cas_min=?13, updated_at=?14 WHERE id=?1 AND company_id=?2",
    )
    .bind(id)
    .bind(company_id)
    .bind(input.cnp.as_deref().unwrap_or(&cur.cnp))
    .bind(input.full_name.as_deref().unwrap_or(&cur.full_name))
    .bind(&gross)
    .bind(&ded)
    .bind(input.employment_date.or(cur.employment_date))
    .bind(input.active.unwrap_or(cur.active))
    .bind(input.tip_asigurat.as_deref().unwrap_or(&cur.tip_asigurat))
    .bind(input.pensionar.unwrap_or(cur.pensionar))
    .bind(input.tip_contract.as_deref().unwrap_or(&cur.tip_contract))
    .bind(input.ore_norma.unwrap_or(cur.ore_norma))
    .bind(
        input
            .exceptie_cas_min
            .as_deref()
            .unwrap_or(&cur.exceptie_cas_min),
    )
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
    let month: u32 = period_from
        .get(5..7)
        .and_then(|m| m.parse().ok())
        .unwrap_or(1);
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
    // Employer-borne part-time minimum-base CAS/CASS difference (art. 146 (5^6)).
    let (mut t_cas_diff, mut t_cass_diff) = (Decimal::ZERO, Decimal::ZERO);
    for e in employees.iter().filter(|e| e.active) {
        let gross = dec(&e.gross_salary);
        let r = compute_payroll(&PayrollInput {
            gross,
            personal_deduction: dec(&e.personal_deduction),
        });
        let exempt =
            crate::anaf_decl::d112::exempt_part_time_min_base(e.pensionar, &e.exceptie_cas_min);
        if let Some((_, cas_diff, cass_diff)) =
            crate::anaf_decl::d112::part_time_min_base(gross, &e.tip_contract, exempt, month)
        {
            t_cas_diff += cas_diff;
            t_cass_diff += cass_diff;
        }
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
        t_cas_diff,
        t_cass_diff,
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
                    tip_asigurat: None,
                    pensionar: None,
                    tip_contract: None,
                    ore_norma: None,
                    exceptie_cas_min: None,
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
