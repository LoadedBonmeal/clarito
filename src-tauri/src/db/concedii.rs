//! Concedii medicale (OUG 158/2005) — registrul certificatelor de concediu medical, sursa blocului
//! D112 `asiguratD` (per certificat) + rollup-urile `asiguratB3` (B3_12 = Σ indemnizație angajator,
//! B3_13 = Σ indemnizație FNUASS) și totalul de recuperat din FNUASS (angajatorC2).
//!
//! Câmpurile derivate determinist (total zile = D_14+D_15; media zilnică = baza/zile_baza) se
//! calculează aici; sumele indemnizațiilor (D_20/D_21) sunt introduse de utilizator (calculul lor
//! din media veniturilor pe 6 luni e o extensie ulterioară). Validarea finală se face în
//! DUKIntegrator înainte de depunere.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MedicalLeave {
    pub id: String,
    pub company_id: String,
    pub employee_id: String,
    pub period_ym: String,
    pub serie: String,
    pub numar: String,
    pub cod_indemnizatie: String,
    pub data_acordare: String,
    pub data_inceput: String,
    pub data_sfarsit: String,
    pub zile_angajator: i64,
    pub zile_fnuass: i64,
    pub baza_calcul: String,
    pub zile_baza: i64,
    pub suma_angajator: String,
    pub suma_fnuass: String,
    pub procent: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MedicalLeaveInput {
    pub company_id: String,
    pub employee_id: String,
    pub period_ym: String,
    #[serde(default)]
    pub serie: Option<String>,
    #[serde(default)]
    pub numar: Option<String>,
    #[serde(default)]
    pub cod_indemnizatie: Option<String>,
    #[serde(default)]
    pub data_acordare: Option<String>,
    #[serde(default)]
    pub data_inceput: Option<String>,
    #[serde(default)]
    pub data_sfarsit: Option<String>,
    #[serde(default)]
    pub zile_angajator: Option<i64>,
    #[serde(default)]
    pub zile_fnuass: Option<i64>,
    #[serde(default)]
    pub baza_calcul: Option<String>,
    #[serde(default)]
    pub zile_baza: Option<i64>,
    #[serde(default)]
    pub suma_angajator: Option<String>,
    #[serde(default)]
    pub suma_fnuass: Option<String>,
    #[serde(default)]
    pub procent: Option<i64>,
}

const COLS: &str = "id, company_id, employee_id, period_ym, serie, numar, cod_indemnizatie, \
                    data_acordare, data_inceput, data_sfarsit, zile_angajator, zile_fnuass, \
                    baza_calcul, zile_baza, suma_angajator, suma_fnuass, procent, created_at";

fn money(label: &str, s: &str) -> AppResult<String> {
    let d = Decimal::from_str(s.trim()).map_err(|_| {
        AppError::Validation(format!("{label} invalid — folosiți formatul 123.45."))
    })?;
    if d.is_sign_negative() {
        return Err(AppError::Validation(format!(
            "{label} nu poate fi negativ."
        )));
    }
    Ok(d.to_string())
}

/// All medical-leave certificates for a company in a reporting month ('YYYY-MM').
pub async fn list(
    pool: &SqlitePool,
    company_id: &str,
    period_ym: &str,
) -> AppResult<Vec<MedicalLeave>> {
    let q = format!(
        "SELECT {COLS} FROM medical_leaves WHERE company_id=?1 AND period_ym=?2 \
         ORDER BY employee_id, data_inceput"
    );
    Ok(sqlx::query_as::<_, MedicalLeave>(&q)
        .bind(company_id)
        .bind(period_ym)
        .fetch_all(pool)
        .await?)
}

pub async fn create(pool: &SqlitePool, input: MedicalLeaveInput) -> AppResult<MedicalLeave> {
    let baza = money(
        "Baza de calcul",
        input.baza_calcul.as_deref().unwrap_or("0"),
    )?;
    let s_ang = money(
        "Indemnizația angajator",
        input.suma_angajator.as_deref().unwrap_or("0"),
    )?;
    let s_fnuass = money(
        "Indemnizația FNUASS",
        input.suma_fnuass.as_deref().unwrap_or("0"),
    )?;
    let id = new_id();
    sqlx::query(&format!(
        "INSERT INTO medical_leaves ({COLS}) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)"
    ))
    .bind(&id)
    .bind(&input.company_id)
    .bind(&input.employee_id)
    .bind(&input.period_ym)
    .bind(input.serie.as_deref().unwrap_or("").trim())
    .bind(input.numar.as_deref().unwrap_or("").trim())
    .bind(input.cod_indemnizatie.as_deref().unwrap_or("01"))
    .bind(input.data_acordare.as_deref().unwrap_or(""))
    .bind(input.data_inceput.as_deref().unwrap_or(""))
    .bind(input.data_sfarsit.as_deref().unwrap_or(""))
    .bind(input.zile_angajator.unwrap_or(0).max(0))
    .bind(input.zile_fnuass.unwrap_or(0).max(0))
    .bind(&baza)
    .bind(input.zile_baza.unwrap_or(0).max(0))
    .bind(&s_ang)
    .bind(&s_fnuass)
    .bind(input.procent.unwrap_or(75))
    .bind(now_unix())
    .execute(pool)
    .await?;
    list(pool, &input.company_id, &input.period_ym)
        .await?
        .into_iter()
        .find(|m| m.id == id)
        .ok_or(AppError::NotFound)
}

pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    sqlx::query("DELETE FROM medical_leaves WHERE id=?1 AND company_id=?2")
        .bind(id)
        .bind(company_id)
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
        sqlx::query(
            "INSERT INTO employees (id, company_id, cnp, full_name, gross_salary, personal_deduction) \
             VALUES ('e1','co','1900101410011','Ion','5000','0')",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn create_list_delete_roundtrip() {
        let pool = pool().await;
        let m = create(
            &pool,
            MedicalLeaveInput {
                company_id: "co".into(),
                employee_id: "e1".into(),
                period_ym: "2026-06".into(),
                serie: Some("AB".into()),
                numar: Some("123".into()),
                cod_indemnizatie: Some("01".into()),
                data_acordare: Some("2026-06-01".into()),
                data_inceput: Some("2026-06-02".into()),
                data_sfarsit: Some("2026-06-06".into()),
                zile_angajator: Some(5),
                zile_fnuass: Some(0),
                baza_calcul: Some("6000".into()),
                zile_baza: Some(21),
                suma_angajator: Some("1071.43".into()),
                suma_fnuass: Some("0".into()),
                procent: Some(75),
            },
        )
        .await
        .unwrap();
        assert_eq!(m.serie, "AB");
        let all = list(&pool, "co", "2026-06").await.unwrap();
        assert_eq!(all.len(), 1);
        delete(&pool, &m.id, "co").await.unwrap();
        assert!(list(&pool, "co", "2026-06").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn rejects_negative_amount() {
        let pool = pool().await;
        let r = create(
            &pool,
            MedicalLeaveInput {
                company_id: "co".into(),
                employee_id: "e1".into(),
                period_ym: "2026-06".into(),
                suma_angajator: Some("-5".into()),
                serie: None,
                numar: None,
                cod_indemnizatie: None,
                data_acordare: None,
                data_inceput: None,
                data_sfarsit: None,
                zile_angajator: None,
                zile_fnuass: None,
                baza_calcul: None,
                zile_baza: None,
                suma_fnuass: None,
                procent: None,
            },
        )
        .await;
        assert!(r.is_err());
    }
}
