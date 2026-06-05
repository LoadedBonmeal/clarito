//! Contacte (clienți și furnizori).
//!
//! Un contact poate fi CUSTOMER, SUPPLIER sau BOTH. Aparține unei companii
//! (parent). Pentru contactele cu CUI românesc se permite și fără prefix
//! "RO" (persoane juridice neînregistrate ca plătitori de TVA).

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::db::companies;
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
    /// True for an individual/consumer (persoană fizică) — B2C. No CUI required;
    /// the UBL generator emits the ANAF placeholder "0000000000000".
    pub is_individual: bool,

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
    pub is_individual: Option<bool>,

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
    pub is_individual: Option<bool>,

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
        "SELECT id, company_id, contact_type, cui, legal_name, vat_payer, is_individual, \
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

/// S1: Fetch a contact by id, scoped to the given company.
/// Returns NotFound if the id doesn't exist OR belongs to a different company.
pub async fn get(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<Contact> {
    sqlx::query_as::<_, Contact>(
        "SELECT id, company_id, contact_type, cui, legal_name, vat_payer, is_individual, \
         address, city, county, country, email, phone, currency, created_at, updated_at \
         FROM contacts WHERE id = ?1 AND company_id = ?2",
    )
    .bind(id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)

    // Note: this is the only `get` fn; callers that already verified ownership
    // (update, delete) also get the scoping for free via the company_id they pass.
}

pub async fn create(pool: &SqlitePool, input: CreateContactInput) -> AppResult<Contact> {
    // Task 6: reject self-invoicing — contact CUI must not match the company's own CUI.
    if let Some(ref contact_cui) = input.cui {
        let contact_cui_norm = contact_cui.trim().to_uppercase();
        let contact_digits = contact_cui_norm
            .strip_prefix("RO")
            .unwrap_or(&contact_cui_norm)
            .trim();
        if !contact_digits.is_empty() {
            let company = companies::get(pool, &input.company_id).await?;
            let company_cui_norm = company.cui.trim().to_uppercase();
            let company_digits = company_cui_norm
                .strip_prefix("RO")
                .unwrap_or(&company_cui_norm)
                .trim();
            if contact_digits == company_digits {
                return Err(AppError::Validation(
                    "Nu puteți adăuga propria companie ca și contact.".to_string(),
                ));
            }
        }
    }

    // Task 2: prevent duplicate (company_id, cui) per company.
    // Normalize the same way the self-CUI check does (trim + uppercase + strip RO prefix)
    // so "RO111" and "111" are treated as the same CUI (consistent de-duplication).
    let normalized_cui: Option<String> = input.cui.as_ref().and_then(|c| {
        let norm = c.trim().to_uppercase();
        let digits = norm.strip_prefix("RO").unwrap_or(&norm).trim().to_string();
        if digits.is_empty() {
            None
        } else {
            Some(digits)
        }
    });

    if let Some(ref norm_cui) = normalized_cui {
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT id FROM contacts WHERE company_id = ?1 AND \
             (cui = ?2 OR cui = 'RO' || ?2) LIMIT 1",
        )
        .bind(&input.company_id)
        .bind(norm_cui)
        .fetch_optional(pool)
        .await?;
        if existing.is_some() {
            return Err(AppError::Validation(
                "Există deja un contact cu acest CUI pentru această companie.".to_string(),
            ));
        }
    }

    let id = new_id();
    let now = now_unix();
    let contact_type = serde_json::to_value(input.contact_type)
        .map(|v| v.as_str().unwrap_or("CUSTOMER").to_string())
        .unwrap_or_else(|_| "CUSTOMER".to_string());

    // Bind the trimmed CUI (consistent with the dup check above).
    let cui_to_store: Option<String> = input
        .cui
        .as_ref()
        .map(|c| c.trim().to_string())
        .filter(|s| !s.is_empty());

    sqlx::query(
        "INSERT INTO contacts (
            id, company_id, contact_type, cui, legal_name, vat_payer,
            address, city, county, country, email, phone, currency,
            is_individual, created_at, updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6,
            ?7, ?8, ?9, ?10, ?11, ?12, ?13,
            ?14, ?15, ?15
        )",
    )
    .bind(&id)
    .bind(&input.company_id)
    .bind(&contact_type)
    .bind(&cui_to_store)
    .bind(&input.legal_name)
    .bind(input.vat_payer.unwrap_or(false))
    .bind(&input.address)
    .bind(&input.city)
    .bind(&input.county)
    .bind(input.country.as_deref().unwrap_or("RO"))
    .bind(&input.email)
    .bind(&input.phone)
    .bind(&input.currency)
    .bind(input.is_individual.unwrap_or(false))
    .bind(now)
    .execute(pool)
    .await?;

    // S1: pass the company_id so the scoped get works correctly.
    get(pool, &id, &input.company_id).await
}

/// R14 Wave A: `company_id` is required. After fetching the contact, we verify
/// ownership and return `NotFound` for any mismatch. The UPDATE SQL is also
/// scoped with `AND company_id = ?` as a defence-in-depth layer.
/// S1: `get` is now company-scoped so the ownership check is implicit.
pub async fn update(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    input: UpdateContactInput,
) -> AppResult<Contact> {
    // S1: scoped get returns NotFound if id belongs to a different company.
    let current = get(pool, id, company_id).await?;

    // Validation parity with create(): the new effective CUI must not match the
    // company's own CUI (self-invoicing) and must not duplicate another contact
    // in the same company.
    let effective_cui: Option<String> = input.cui.clone().or_else(|| current.cui.clone());
    if let Some(ref contact_cui) = effective_cui {
        let contact_cui_norm = contact_cui.trim().to_uppercase();
        let contact_digits = contact_cui_norm
            .strip_prefix("RO")
            .unwrap_or(&contact_cui_norm)
            .trim();
        if !contact_digits.is_empty() {
            let company = companies::get(pool, company_id).await?;
            let company_cui_norm = company.cui.trim().to_uppercase();
            let company_digits = company_cui_norm
                .strip_prefix("RO")
                .unwrap_or(&company_cui_norm)
                .trim();
            if contact_digits == company_digits {
                return Err(AppError::Validation(
                    "Nu puteți adăuga propria companie ca și contact.".to_string(),
                ));
            }
            let existing: Option<String> = sqlx::query_scalar(
                "SELECT id FROM contacts WHERE company_id = ?1 AND \
                 (cui = ?2 OR cui = 'RO' || ?2) AND id != ?3 LIMIT 1",
            )
            .bind(company_id)
            .bind(contact_digits)
            .bind(id)
            .fetch_optional(pool)
            .await?;
            if existing.is_some() {
                return Err(AppError::Validation(
                    "Există deja un contact cu acest CUI pentru această companie.".to_string(),
                ));
            }
        }
    }

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
            is_individual = ?13,
            updated_at   = ?14
        WHERE id = ?1 AND company_id = ?15",
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
    .bind(input.is_individual.unwrap_or(current.is_individual))
    .bind(now)
    .bind(company_id)
    .execute(pool)
    .await?;

    get(pool, id, company_id).await
}

/// R14 Wave A: `company_id` is required. Deletion is scoped to the owning
/// company; cross-company attempts receive `NotFound`.
/// S1: `get` is now company-scoped so the ownership check is done in the SQL.
pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    // S1: scoped get — returns NotFound if contact doesn't exist OR belongs
    // to a different company.
    let _ = get(pool, id, company_id).await?;

    // Block deletion when the contact is referenced by invoices — the FK would otherwise
    // reject it with an opaque DB error. Give a clear, actionable message instead.
    let ref_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM invoices WHERE contact_id = ?1 AND company_id = ?2",
    )
    .bind(id)
    .bind(company_id)
    .fetch_one(pool)
    .await
    .map_err(AppError::Database)?;
    if ref_count > 0 {
        return Err(AppError::Validation(format!(
            "Partenerul este referențiat de {ref_count} factură(i) — ștergeți sau reasignați-le mai întâi."
        )));
    }

    let res = sqlx::query("DELETE FROM contacts WHERE id = ?1 AND company_id = ?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    /// Minimal in-memory schema for contacts Wave A tests.
    async fn setup_contacts_pool() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .unwrap();

        // companies table is needed because contacts::create now looks up the company.
        sqlx::query(
            "CREATE TABLE companies (
                id TEXT PRIMARY KEY NOT NULL,
                cui TEXT NOT NULL,
                legal_name TEXT NOT NULL DEFAULT '',
                trade_name TEXT,
                registry_number TEXT,
                vat_payer INTEGER NOT NULL DEFAULT 1,
                address TEXT NOT NULL DEFAULT '',
                city TEXT NOT NULL DEFAULT '',
                county TEXT NOT NULL DEFAULT '',
                postal_code TEXT,
                country TEXT NOT NULL DEFAULT 'RO',
                email TEXT,
                phone TEXT,
                iban TEXT,
                bank_name TEXT,
                is_active INTEGER NOT NULL DEFAULT 1,
                spv_enabled INTEGER NOT NULL DEFAULT 0,
                invoice_series TEXT NOT NULL DEFAULT 'FACT',
                last_invoice_number INTEGER NOT NULL DEFAULT 0,
                logo_path TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE contacts (
                id TEXT PRIMARY KEY NOT NULL,
                company_id TEXT NOT NULL,
                contact_type TEXT NOT NULL DEFAULT 'CUSTOMER',
                cui TEXT,
                legal_name TEXT NOT NULL DEFAULT '',
                vat_payer INTEGER NOT NULL DEFAULT 0,
                is_individual INTEGER NOT NULL DEFAULT 0,
                address TEXT,
                city TEXT,
                county TEXT,
                country TEXT NOT NULL DEFAULT 'RO',
                email TEXT,
                phone TEXT,
                currency TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Minimal invoices table so delete()'s referential-integrity check
        // (SELECT COUNT(*) FROM invoices ...) can run in these unit tests.
        sqlx::query(
            "CREATE TABLE invoices (
                id TEXT PRIMARY KEY NOT NULL,
                company_id TEXT NOT NULL,
                contact_id TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Seed: two companies.
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name) \
             VALUES ('comp-1', 'RO12345674', 'Firma Unu SRL'), \
                    ('comp-2', 'RO98765438', 'Firma Doi SRL')",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Seed: two contacts — one per company.
        sqlx::query(
            "INSERT INTO contacts (id, company_id, legal_name)
             VALUES ('c1', 'comp-1', 'Client Comp1'),
                    ('c2', 'comp-2', 'Client Comp2')",
        )
        .execute(&pool)
        .await
        .unwrap();

        pool
    }

    // ── update: wrong company → NotFound ────────────────────────────────────

    #[tokio::test]
    async fn wave_a_contact_update_wrong_company_returns_not_found() {
        let pool = setup_contacts_pool().await;
        let input = UpdateContactInput {
            legal_name: Some("Renamed".to_string()),
            ..Default::default()
        };
        // comp-2 tries to update comp-1's contact.
        let result = update(&pool, "c1", "comp-2", input).await;
        assert!(
            matches!(result, Err(AppError::NotFound)),
            "update with wrong company_id must return NotFound"
        );
        // Name must be unchanged — fetch with correct company.
        let contact = get(&pool, "c1", "comp-1").await.unwrap();
        assert_eq!(contact.legal_name, "Client Comp1", "name must not change");
    }

    #[tokio::test]
    async fn wave_a_contact_update_correct_company_succeeds() {
        let pool = setup_contacts_pool().await;
        let input = UpdateContactInput {
            legal_name: Some("Renamed OK".to_string()),
            ..Default::default()
        };
        let result = update(&pool, "c1", "comp-1", input).await;
        assert!(
            result.is_ok(),
            "update with correct company_id must succeed"
        );
        let contact = get(&pool, "c1", "comp-1").await.unwrap();
        assert_eq!(contact.legal_name, "Renamed OK", "name must be updated");
    }

    // ── delete: wrong company → NotFound ────────────────────────────────────

    #[tokio::test]
    async fn wave_a_contact_delete_wrong_company_returns_not_found() {
        let pool = setup_contacts_pool().await;
        // comp-2 tries to delete comp-1's contact.
        let result = delete(&pool, "c1", "comp-2").await;
        assert!(
            matches!(result, Err(AppError::NotFound)),
            "delete with wrong company_id must return NotFound"
        );
        // Contact must still exist — fetch with correct company.
        let still_there = get(&pool, "c1", "comp-1").await;
        assert!(still_there.is_ok(), "contact must not have been deleted");
    }

    #[tokio::test]
    async fn wave_a_contact_delete_correct_company_succeeds() {
        let pool = setup_contacts_pool().await;
        let result = delete(&pool, "c1", "comp-1").await;
        assert!(
            result.is_ok(),
            "delete with correct company_id must succeed"
        );
        let gone = get(&pool, "c1", "comp-1").await;
        assert!(
            matches!(gone, Err(AppError::NotFound)),
            "contact must be gone after correct-company delete"
        );
    }

    // ── S1: get is now company-scoped ────────────────────────────────────────

    /// S1: get with correct company_id returns the contact.
    #[tokio::test]
    async fn s1_get_contact_correct_company_returns_contact() {
        let pool = setup_contacts_pool().await;
        let result = get(&pool, "c1", "comp-1").await;
        assert!(result.is_ok(), "get with correct company must succeed");
        let c = result.unwrap();
        assert_eq!(c.id, "c1");
        assert_eq!(c.company_id, "comp-1");
    }

    /// S1: get with wrong company_id returns NotFound (isolation gap closed).
    #[tokio::test]
    async fn s1_get_contact_wrong_company_returns_not_found() {
        let pool = setup_contacts_pool().await;
        // comp-2 tries to fetch comp-1's contact.
        let result = get(&pool, "c1", "comp-2").await;
        assert!(
            matches!(result, Err(AppError::NotFound)),
            "get with wrong company_id must return NotFound (S1)"
        );
    }

    /// S1: get with unknown id returns NotFound regardless of company.
    #[tokio::test]
    async fn s1_get_contact_unknown_id_returns_not_found() {
        let pool = setup_contacts_pool().await;
        let result = get(&pool, "nonexistent", "comp-1").await;
        assert!(
            matches!(result, Err(AppError::NotFound)),
            "get with unknown id must return NotFound"
        );
    }

    // ── Task 2: duplicate (company_id, cui) rejected ─────────────────────────

    #[tokio::test]
    async fn task2_duplicate_contact_cui_per_company_rejected() {
        let pool = setup_contacts_pool().await;

        // First contact with CUI RO11111110 for comp-1 — should succeed.
        let input1 = CreateContactInput {
            company_id: "comp-1".to_string(),
            contact_type: ContactType::Customer,
            cui: Some("RO11111110".to_string()),
            legal_name: "Primul Client SRL".to_string(),
            vat_payer: Some(false),
            is_individual: None,
            address: None,
            city: None,
            county: None,
            country: None,
            email: None,
            phone: None,
            currency: None,
        };
        let r1 = create(&pool, input1).await;
        assert!(r1.is_ok(), "first contact with this CUI must succeed");

        // Second contact with same CUI for same company — must fail.
        let input2 = CreateContactInput {
            company_id: "comp-1".to_string(),
            contact_type: ContactType::Customer,
            cui: Some("RO11111110".to_string()),
            legal_name: "Al Doilea Client SRL".to_string(),
            vat_payer: Some(false),
            is_individual: None,
            address: None,
            city: None,
            county: None,
            country: None,
            email: None,
            phone: None,
            currency: None,
        };
        let r2 = create(&pool, input2).await;
        assert!(
            matches!(r2, Err(AppError::Validation(_))),
            "duplicate CUI for same company must return Validation error"
        );

        // Same CUI for a different company — must succeed.
        let input3 = CreateContactInput {
            company_id: "comp-2".to_string(),
            contact_type: ContactType::Customer,
            cui: Some("RO11111110".to_string()),
            legal_name: "Client Comp2 SRL".to_string(),
            vat_payer: Some(false),
            is_individual: None,
            address: None,
            city: None,
            county: None,
            country: None,
            email: None,
            phone: None,
            currency: None,
        };
        let r3 = create(&pool, input3).await;
        assert!(r3.is_ok(), "same CUI for a different company must succeed");
    }

    // ── Task 6: self-invoicing rejected (contact CUI == company CUI) ─────────

    #[tokio::test]
    async fn task6_self_cui_contact_rejected() {
        let pool = setup_contacts_pool().await;

        // comp-1 has CUI RO12345674 — trying to add a contact with the same CUI must fail.
        let input = CreateContactInput {
            company_id: "comp-1".to_string(),
            contact_type: ContactType::Customer,
            cui: Some("RO12345674".to_string()),
            legal_name: "Propria Firma SRL".to_string(),
            vat_payer: Some(true),
            is_individual: None,
            address: None,
            city: None,
            county: None,
            country: None,
            email: None,
            phone: None,
            currency: None,
        };
        let result = create(&pool, input).await;
        assert!(
            matches!(result, Err(AppError::Validation(_))),
            "contact with company's own CUI must return Validation error"
        );
    }

    #[tokio::test]
    async fn task6_different_cui_contact_allowed() {
        let pool = setup_contacts_pool().await;

        // Different CUI — should be fine.
        let input = CreateContactInput {
            company_id: "comp-1".to_string(),
            contact_type: ContactType::Customer,
            cui: Some("RO22222229".to_string()),
            legal_name: "Alt Client SRL".to_string(),
            vat_payer: Some(false),
            is_individual: None,
            address: None,
            city: None,
            county: None,
            country: None,
            email: None,
            phone: None,
            currency: None,
        };
        let result = create(&pool, input).await;
        assert!(result.is_ok(), "contact with different CUI must be allowed");
    }
}
