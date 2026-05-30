//! Contacte (clienți și furnizori).
//!
//! Un contact poate fi CUSTOMER, SUPPLIER sau BOTH. Aparține unei companii
//! (parent). Pentru contactele cu CUI românesc se permite și fără prefix
//! "RO" (persoane juridice neînregistrate ca plătitori de TVA).

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::db::models::{new_id, now_unix, ContactType};
use crate::error::{AppError, AppResult};

// ─── Model ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    pub id: String,
    pub company_id: String,

    pub contact_type: String,
    pub cui: Option<String>,
    pub legal_name: String,
    pub vat_payer: bool,

    pub address: Option<String>,
    pub city: Option<String>,
    pub county: Option<String>,
    pub country: String,

    pub email: Option<String>,
    pub phone: Option<String>,

    pub currency: Option<String>,

    pub created_at: i64,
    pub updated_at: i64,
}

// ─── Inputs ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateContactInput {
    pub company_id: String,
    pub contact_type: ContactType,
    pub cui: Option<String>,
    pub legal_name: String,
    pub vat_payer: Option<bool>,

    pub address: Option<String>,
    pub city: Option<String>,
    pub county: Option<String>,
    pub country: Option<String>,

    pub email: Option<String>,
    pub phone: Option<String>,

    pub currency: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateContactInput {
    pub contact_type: Option<ContactType>,
    pub cui: Option<String>,
    pub legal_name: Option<String>,
    pub vat_payer: Option<bool>,

    pub address: Option<String>,
    pub city: Option<String>,
    pub county: Option<String>,
    pub country: Option<String>,

    pub email: Option<String>,
    pub phone: Option<String>,

    pub currency: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactFilter {
    pub company_id: Option<String>,
    pub query: Option<String>,
}

// ─── Queries ───────────────────────────────────────────────────────────────

pub async fn list(pool: &SqlitePool, filter: ContactFilter) -> AppResult<Vec<Contact>> {
    let company_id = filter.company_id.as_ref().filter(|s| !s.is_empty());
    let query_term = filter.query.as_ref().filter(|s| !s.is_empty());

    // ?1 company_id (Option<&str>), ?2 query_term (Option<&str>)
    let items = sqlx::query_as::<_, Contact>(
        "SELECT id, company_id, contact_type, cui, legal_name, vat_payer, \
         address, city, county, country, email, phone, currency, created_at, updated_at \
         FROM contacts \
         WHERE (?1 IS NULL OR company_id = ?1) \
           AND (?2 IS NULL OR legal_name LIKE '%' || ?2 || '%' OR cui LIKE '%' || ?2 || '%') \
         ORDER BY legal_name",
    )
    .bind(company_id)
    .bind(query_term)
    .fetch_all(pool)
    .await?;
    Ok(items)
}

pub async fn get(pool: &SqlitePool, id: &str) -> AppResult<Contact> {
    sqlx::query_as::<_, Contact>(
        "SELECT id, company_id, contact_type, cui, legal_name, vat_payer, \
         address, city, county, country, email, phone, currency, created_at, updated_at \
         FROM contacts WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}

pub async fn create(pool: &SqlitePool, input: CreateContactInput) -> AppResult<Contact> {
    let id = new_id();
    let now = now_unix();
    let contact_type = serde_json::to_value(input.contact_type)
        .map(|v| v.as_str().unwrap_or("CUSTOMER").to_string())
        .unwrap_or_else(|_| "CUSTOMER".to_string());

    sqlx::query(
        "INSERT INTO contacts (
            id, company_id, contact_type, cui, legal_name, vat_payer,
            address, city, county, country, email, phone, currency,
            created_at, updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6,
            ?7, ?8, ?9, ?10, ?11, ?12, ?13,
            ?14, ?14
        )",
    )
    .bind(&id)
    .bind(&input.company_id)
    .bind(&contact_type)
    .bind(&input.cui)
    .bind(&input.legal_name)
    .bind(input.vat_payer.unwrap_or(false))
    .bind(&input.address)
    .bind(&input.city)
    .bind(&input.county)
    .bind(input.country.as_deref().unwrap_or("RO"))
    .bind(&input.email)
    .bind(&input.phone)
    .bind(&input.currency)
    .bind(now)
    .execute(pool)
    .await?;

    get(pool, &id).await
}

pub async fn update(pool: &SqlitePool, id: &str, input: UpdateContactInput) -> AppResult<Contact> {
    let current = get(pool, id).await?;
    let now = now_unix();

    let contact_type = match input.contact_type {
        Some(t) => serde_json::to_value(t)
            .map(|v| v.as_str().unwrap_or("CUSTOMER").to_string())
            .unwrap_or(current.contact_type),
        None => current.contact_type,
    };

    sqlx::query(
        "UPDATE contacts SET
            contact_type = ?2,
            cui          = ?3,
            legal_name   = ?4,
            vat_payer    = ?5,
            address      = ?6,
            city         = ?7,
            county       = ?8,
            country      = ?9,
            email        = ?10,
            phone        = ?11,
            currency     = ?12,
            updated_at   = ?13
        WHERE id = ?1",
    )
    .bind(id)
    .bind(&contact_type)
    .bind(input.cui.or(current.cui))
    .bind(input.legal_name.unwrap_or(current.legal_name))
    .bind(input.vat_payer.unwrap_or(current.vat_payer))
    .bind(input.address.or(current.address))
    .bind(input.city.or(current.city))
    .bind(input.county.or(current.county))
    .bind(input.country.unwrap_or(current.country))
    .bind(input.email.or(current.email))
    .bind(input.phone.or(current.phone))
    .bind(input.currency.or(current.currency))
    .bind(now)
    .execute(pool)
    .await?;

    get(pool, id).await
}

pub async fn delete(pool: &SqlitePool, id: &str) -> AppResult<()> {
    let res = sqlx::query("DELETE FROM contacts WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}
