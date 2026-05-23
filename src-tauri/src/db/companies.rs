//! Companii (entitățile pe care utilizatorul le administrează).
//!
//! Suport multi-tenant: un user poate avea N companii. Tier-ul licenței
//! limitează numărul (verificarea se face în layer-ul de comenzi).

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

// ─── Model ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Company {
    pub id: String,
    pub cui: String,
    pub legal_name: String,
    pub trade_name: Option<String>,
    pub registry_number: Option<String>,
    pub vat_payer: bool,

    pub address: String,
    pub city: String,
    pub county: String,
    pub postal_code: Option<String>,
    pub country: String,

    pub email: Option<String>,
    pub phone: Option<String>,
    pub iban: Option<String>,
    pub bank_name: Option<String>,

    pub is_active: bool,
    pub spv_enabled: bool,

    pub invoice_series: String,
    pub last_invoice_number: i64,

    pub logo_path: Option<String>,

    pub created_at: i64,
    pub updated_at: i64,
}

// ─── Inputs ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCompanyInput {
    pub cui: String,
    pub legal_name: String,
    pub trade_name: Option<String>,
    pub registry_number: Option<String>,
    pub vat_payer: Option<bool>,

    pub address: String,
    pub city: String,
    pub county: String,
    pub postal_code: Option<String>,
    pub country: Option<String>,

    pub email: Option<String>,
    pub phone: Option<String>,
    pub iban: Option<String>,
    pub bank_name: Option<String>,

    pub invoice_series: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCompanyInput {
    pub legal_name: Option<String>,
    pub trade_name: Option<String>,
    pub registry_number: Option<String>,
    pub vat_payer: Option<bool>,

    pub address: Option<String>,
    pub city: Option<String>,
    pub county: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,

    pub email: Option<String>,
    pub phone: Option<String>,
    pub iban: Option<String>,
    pub bank_name: Option<String>,

    pub is_active: Option<bool>,
    pub spv_enabled: Option<bool>,

    pub invoice_series: Option<String>,
    pub logo_path: Option<String>,
}

// ─── Queries ───────────────────────────────────────────────────────────────

const SELECT_COLUMNS: &str = "id, cui, legal_name, trade_name, registry_number, vat_payer, \
    address, city, county, postal_code, country, email, phone, iban, bank_name, \
    is_active, spv_enabled, invoice_series, last_invoice_number, logo_path, \
    created_at, updated_at";

pub async fn list(pool: &SqlitePool) -> AppResult<Vec<Company>> {
    let sql = format!(
        "SELECT {SELECT_COLUMNS} FROM companies WHERE is_active = 1 ORDER BY legal_name",
    );
    let rows = sqlx::query_as::<_, Company>(&sql).fetch_all(pool).await?;
    Ok(rows)
}

pub async fn get(pool: &SqlitePool, id: &str) -> AppResult<Company> {
    let sql = format!("SELECT {SELECT_COLUMNS} FROM companies WHERE id = ?1");
    let row = sqlx::query_as::<_, Company>(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(row)
}

pub async fn get_by_cui(pool: &SqlitePool, cui: &str) -> AppResult<Option<Company>> {
    let sql = format!("SELECT {SELECT_COLUMNS} FROM companies WHERE cui = ?1");
    let row = sqlx::query_as::<_, Company>(&sql)
        .bind(cui)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn create(pool: &SqlitePool, input: CreateCompanyInput) -> AppResult<Company> {
    validate_cui(&input.cui)?;
    if get_by_cui(pool, &input.cui).await?.is_some() {
        return Err(AppError::Conflict(format!(
            "Există deja o companie cu CUI {}",
            input.cui
        )));
    }

    let id = new_id();
    let now = now_unix();

    sqlx::query(
        "INSERT INTO companies (
            id, cui, legal_name, trade_name, registry_number, vat_payer,
            address, city, county, postal_code, country,
            email, phone, iban, bank_name,
            invoice_series, created_at, updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6,
            ?7, ?8, ?9, ?10, ?11,
            ?12, ?13, ?14, ?15,
            ?16, ?17, ?17
        )",
    )
    .bind(&id)
    .bind(&input.cui)
    .bind(&input.legal_name)
    .bind(&input.trade_name)
    .bind(&input.registry_number)
    .bind(input.vat_payer.unwrap_or(true))
    .bind(&input.address)
    .bind(&input.city)
    .bind(&input.county)
    .bind(&input.postal_code)
    .bind(input.country.as_deref().unwrap_or("RO"))
    .bind(&input.email)
    .bind(&input.phone)
    .bind(&input.iban)
    .bind(&input.bank_name)
    .bind(input.invoice_series.as_deref().unwrap_or("FACT"))
    .bind(now)
    .execute(pool)
    .await?;

    get(pool, &id).await
}

pub async fn update(
    pool: &SqlitePool,
    id: &str,
    input: UpdateCompanyInput,
) -> AppResult<Company> {
    // Asigură existența + colectează vechile valori.
    let current = get(pool, id).await?;
    let now = now_unix();

    sqlx::query(
        "UPDATE companies SET
            legal_name      = ?2,
            trade_name      = ?3,
            registry_number = ?4,
            vat_payer       = ?5,
            address         = ?6,
            city            = ?7,
            county          = ?8,
            postal_code     = ?9,
            country         = ?10,
            email           = ?11,
            phone           = ?12,
            iban            = ?13,
            bank_name       = ?14,
            is_active       = ?15,
            spv_enabled     = ?16,
            invoice_series  = ?17,
            logo_path       = ?18,
            updated_at      = ?19
        WHERE id = ?1",
    )
    .bind(id)
    .bind(input.legal_name.unwrap_or(current.legal_name))
    .bind(input.trade_name.or(current.trade_name))
    .bind(input.registry_number.or(current.registry_number))
    .bind(input.vat_payer.unwrap_or(current.vat_payer))
    .bind(input.address.unwrap_or(current.address))
    .bind(input.city.unwrap_or(current.city))
    .bind(input.county.unwrap_or(current.county))
    .bind(input.postal_code.or(current.postal_code))
    .bind(input.country.unwrap_or(current.country))
    .bind(input.email.or(current.email))
    .bind(input.phone.or(current.phone))
    .bind(input.iban.or(current.iban))
    .bind(input.bank_name.or(current.bank_name))
    .bind(input.is_active.unwrap_or(current.is_active))
    .bind(input.spv_enabled.unwrap_or(current.spv_enabled))
    .bind(input.invoice_series.unwrap_or(current.invoice_series))
    .bind(input.logo_path.or(current.logo_path))
    .bind(now)
    .execute(pool)
    .await?;

    get(pool, id).await
}

/// Soft-delete (is_active = 0). Hard delete necesită confirmare separată
/// pentru a păstra integritatea referențială cu facturile istorice.
pub async fn soft_delete(pool: &SqlitePool, id: &str) -> AppResult<()> {
    let now = now_unix();
    let res = sqlx::query(
        "UPDATE companies SET is_active = 0, updated_at = ?2 WHERE id = ?1",
    )
    .bind(id)
    .bind(now)
    .execute(pool)
    .await?;

    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Incrementează contorul de facturi și returnează noul număr.
/// Folosit atomic pentru a evita coliziuni de numerotare.
pub async fn next_invoice_number(
    pool: &SqlitePool,
    company_id: &str,
) -> AppResult<i64> {
    let mut tx = pool.begin().await?;

    sqlx::query("UPDATE companies SET last_invoice_number = last_invoice_number + 1 WHERE id = ?1")
        .bind(company_id)
        .execute(&mut *tx)
        .await?;

    let new_number: i64 = sqlx::query_scalar(
        "SELECT last_invoice_number FROM companies WHERE id = ?1",
    )
    .bind(company_id)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(new_number)
}

// ─── Validation ────────────────────────────────────────────────────────────

/// CIF românesc: opțional prefix "RO" + 2-10 cifre.
fn validate_cui(cui: &str) -> AppResult<()> {
    let trimmed = cui.trim().to_uppercase();
    let digits = trimmed.strip_prefix("RO").unwrap_or(&trimmed);
    if digits.len() < 2 || digits.len() > 10 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err(AppError::Validation(format!(
            "CUI invalid: '{cui}'. Format așteptat: 2-10 cifre, cu sau fără prefix RO."
        )));
    }
    Ok(())
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_cui_with_ro_prefix() {
        assert!(validate_cui("RO12345678").is_ok());
        assert!(validate_cui("ro12345678").is_ok());
        assert!(validate_cui(" RO12345678 ").is_ok());
    }

    #[test]
    fn validates_cui_without_prefix() {
        assert!(validate_cui("12345678").is_ok());
        assert!(validate_cui("99").is_ok());
    }

    #[test]
    fn rejects_invalid_cui() {
        assert!(validate_cui("").is_err());
        assert!(validate_cui("RO").is_err());
        assert!(validate_cui("RO123456789012").is_err());
        assert!(validate_cui("RO123abc").is_err());
    }
}
