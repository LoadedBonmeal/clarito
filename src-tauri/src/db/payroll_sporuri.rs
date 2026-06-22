//! Sporuri salariale (Codul muncii) — adaosuri taxabile la brut, per angajat per lună.
//!
//! Modelul: un row per (company, employee, period, kind) cu suma adaosului.
//! Engine-ul de salarizare (`run_payroll`) însumează toate sporurile lunii unui angajat,
//! le adaugă la `gross_salary` și calculează CAS/CASS/impozit/CAM pe baza combinată.
//! Aceasta menține invariantul GL≡D112 (o singură rotunjire pe baza combinată).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

/// Spor salarial per angajat per lună.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Spor {
    pub id: String,
    pub company_id: String,
    pub employee_id: String,
    pub period: String,
    pub amount: String,
    /// Tip spor: 'vechime' | 'noapte' | 'suplimentare' | 'conditii_deosebite' | 'alte'
    pub kind: String,
    pub description: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSporInput {
    pub company_id: String,
    pub employee_id: String,
    pub period: String,
    pub amount: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSporInput {
    pub amount: Option<String>,
    pub kind: Option<String>,
    pub description: Option<String>,
}

const COLS: &str =
    "id, company_id, employee_id, period, amount, kind, description, created_at, updated_at";

/// Validare sumă spor: obligatoriu pozitivă, fără notație științifică.
fn parse_amount(s: &str) -> AppResult<String> {
    let t = s.trim();
    if t.contains('e') || t.contains('E') {
        return Err(AppError::Validation(
            "Suma spor invalidă — folosiți formatul 1234.56 (fără notație științifică).".into(),
        ));
    }
    let d = Decimal::from_str(t).map_err(|_| {
        AppError::Validation("Suma spor invalidă — folosiți formatul 1234.56.".into())
    })?;
    if d.is_sign_negative() {
        return Err(AppError::Validation(
            "Suma spor nu poate fi negativă.".into(),
        ));
    }
    Ok(d.to_string())
}

/// Lista sporurilor per companie + perioadă (YYYY-MM).
pub async fn list(pool: &SqlitePool, company_id: &str, period: &str) -> AppResult<Vec<Spor>> {
    let q = format!(
        "SELECT {COLS} FROM payroll_sporuri \
         WHERE company_id=?1 AND period=?2 ORDER BY employee_id, kind"
    );
    Ok(sqlx::query_as::<_, Spor>(&q)
        .bind(company_id)
        .bind(period)
        .fetch_all(pool)
        .await?)
}

pub async fn get(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<Spor> {
    let q = format!("SELECT {COLS} FROM payroll_sporuri WHERE id=?1 AND company_id=?2");
    sqlx::query_as::<_, Spor>(&q)
        .bind(id)
        .bind(company_id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn create(pool: &SqlitePool, input: CreateSporInput) -> AppResult<Spor> {
    if input.period.len() != 7 {
        return Err(AppError::Validation(
            "Perioada spor trebuie să fie în format YYYY-MM.".into(),
        ));
    }
    let amount = parse_amount(&input.amount)?;
    let kind = input.kind.as_deref().unwrap_or("alte").trim().to_string();
    let description = input
        .description
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    let id = new_id();
    let now = now_unix();
    sqlx::query(
        "INSERT INTO payroll_sporuri \
         (id, company_id, employee_id, period, amount, kind, description, created_at, updated_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8)",
    )
    .bind(&id)
    .bind(&input.company_id)
    .bind(&input.employee_id)
    .bind(&input.period)
    .bind(&amount)
    .bind(&kind)
    .bind(&description)
    .bind(now)
    .execute(pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            AppError::Validation(format!(
                "Există deja un spor de tip „{kind}\" pentru acest angajat în perioada {}.",
                input.period
            ))
        } else {
            AppError::from(e)
        }
    })?;
    get(pool, &id, &input.company_id).await
}

pub async fn update(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    input: UpdateSporInput,
) -> AppResult<Spor> {
    let cur = get(pool, id, company_id).await?;
    let amount = match input.amount.as_deref() {
        Some(s) => parse_amount(s)?,
        None => cur.amount.clone(),
    };
    let kind = input
        .kind
        .as_deref()
        .unwrap_or(&cur.kind)
        .trim()
        .to_string();
    let description = input
        .description
        .as_deref()
        .unwrap_or(&cur.description)
        .trim()
        .to_string();
    sqlx::query(
        "UPDATE payroll_sporuri SET amount=?3, kind=?4, description=?5, updated_at=?6 \
         WHERE id=?1 AND company_id=?2",
    )
    .bind(id)
    .bind(company_id)
    .bind(&amount)
    .bind(&kind)
    .bind(&description)
    .bind(now_unix())
    .execute(pool)
    .await?;
    get(pool, id, company_id).await
}

pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    get(pool, id, company_id).await?;
    sqlx::query("DELETE FROM payroll_sporuri WHERE id=?1 AND company_id=?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Suma tuturor sporurilor unui angajat pentru o perioadă — folosită de `run_payroll`.
/// Returnează zero dacă nu există sporuri.
pub async fn total_for_employee(
    pool: &SqlitePool,
    company_id: &str,
    employee_id: &str,
    period: &str,
) -> AppResult<Decimal> {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT amount FROM payroll_sporuri \
         WHERE company_id=?1 AND employee_id=?2 AND period=?3",
    )
    .bind(company_id)
    .bind(employee_id)
    .bind(period)
    .fetch_all(pool)
    .await?;
    let total = rows
        .iter()
        .map(|s| Decimal::from_str(s).unwrap_or(Decimal::ZERO))
        .fold(Decimal::ZERO, |acc, x| acc + x);
    Ok(total)
}

/// Sumele sporurilor per angajat pentru o perioadă, ca HashMap employee_id → Decimal.
/// Angajații fără sporuri nu apar în map (caller tratează lipsă ca zero).
pub async fn sporuri_by_employee(
    pool: &SqlitePool,
    company_id: &str,
    period: &str,
) -> AppResult<std::collections::HashMap<String, Decimal>> {
    #[derive(sqlx::FromRow)]
    struct Row {
        employee_id: String,
        amount: String,
    }
    let rows: Vec<Row> = sqlx::query_as::<_, Row>(
        "SELECT employee_id, amount FROM payroll_sporuri \
         WHERE company_id=?1 AND period=?2",
    )
    .bind(company_id)
    .bind(period)
    .fetch_all(pool)
    .await?;
    let mut map: std::collections::HashMap<String, Decimal> = std::collections::HashMap::new();
    for r in rows {
        let d = Decimal::from_str(&r.amount).unwrap_or(Decimal::ZERO);
        *map.entry(r.employee_id).or_insert(Decimal::ZERO) += d;
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        sqlx::query(
            "INSERT INTO employees \
             (id,company_id,cnp,full_name,gross_salary,personal_deduction,\
             active,tip_asigurat,pensionar,tip_contract,ore_norma,\
             exceptie_cas_min,sediu_cif,beneficiar_suma_netaxabila,created_at,updated_at) \
             VALUES ('emp1','co1','1900101410011','Ion',\
             '4000','0',1,'1',0,'N',8,'','',0,1,1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn spor_crud_and_total() {
        let pool = setup().await;

        // Create two sporuri for emp1
        let s1 = create(
            &pool,
            CreateSporInput {
                company_id: "co1".into(),
                employee_id: "emp1".into(),
                period: "2026-06".into(),
                amount: "500".into(),
                kind: Some("vechime".into()),
                description: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(s1.amount, "500");

        let s2 = create(
            &pool,
            CreateSporInput {
                company_id: "co1".into(),
                employee_id: "emp1".into(),
                period: "2026-06".into(),
                amount: "300".into(),
                kind: Some("noapte".into()),
                description: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(s2.amount, "300");

        // Total = 800
        let total = total_for_employee(&pool, "co1", "emp1", "2026-06")
            .await
            .unwrap();
        assert_eq!(total, Decimal::from(800));

        // Map
        let map = sporuri_by_employee(&pool, "co1", "2026-06").await.unwrap();
        assert_eq!(map["emp1"], Decimal::from(800));

        // Update
        update(
            &pool,
            &s1.id,
            "co1",
            UpdateSporInput {
                amount: Some("600".into()),
                kind: None,
                description: None,
            },
        )
        .await
        .unwrap();
        let total2 = total_for_employee(&pool, "co1", "emp1", "2026-06")
            .await
            .unwrap();
        assert_eq!(total2, Decimal::from(900));

        // Delete
        delete(&pool, &s2.id, "co1").await.unwrap();
        let total3 = total_for_employee(&pool, "co1", "emp1", "2026-06")
            .await
            .unwrap();
        assert_eq!(total3, Decimal::from(600));

        // Duplicate kind rejected
        let dup = create(
            &pool,
            CreateSporInput {
                company_id: "co1".into(),
                employee_id: "emp1".into(),
                period: "2026-06".into(),
                amount: "100".into(),
                kind: Some("vechime".into()),
                description: None,
            },
        )
        .await;
        assert!(dup.is_err(), "duplicate kind should be rejected");
    }

    #[tokio::test]
    async fn spor_amount_validation() {
        let pool = setup().await;
        let bad = create(
            &pool,
            CreateSporInput {
                company_id: "co1".into(),
                employee_id: "emp1".into(),
                period: "2026-06".into(),
                amount: "1e5".into(),
                kind: None,
                description: None,
            },
        )
        .await;
        assert!(bad.is_err(), "scientific notation rejected");

        let neg = create(
            &pool,
            CreateSporInput {
                company_id: "co1".into(),
                employee_id: "emp1".into(),
                period: "2026-06".into(),
                amount: "-100".into(),
                kind: None,
                description: None,
            },
        )
        .await;
        assert!(neg.is_err(), "negative amount rejected");
    }
}
