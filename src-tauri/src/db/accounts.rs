//! Plan de conturi (chart of accounts) — company-scoped catalog.
//!
//! Fiecare cont aparține unei companii (company_id). Toate operațiunile
//! sunt scoped pe company_id — cross-company access returnează NotFound.
//!
//! `seed_standard` inserează un subset din planul de conturi românesc
//! standard dacă firma nu are niciun cont înregistrat (idempotent).

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

// ─── Model ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub company_id: String,
    pub account_code: String,
    pub account_name: String,
    pub account_class: Option<i64>,
    pub parent_code: Option<String>,
    pub active: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

// ─── Inputs ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInput {
    pub account_code: String,
    pub account_name: String,
    pub account_class: Option<i64>,
    pub parent_code: Option<String>,
    pub active: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAccountInput {
    pub account_code: Option<String>,
    pub account_name: Option<String>,
    pub account_class: Option<i64>,
    pub parent_code: Option<String>,
    pub active: Option<bool>,
}

// ─── Queries ───────────────────────────────────────────────────────────────

/// List all accounts for a company, ordered by account_code.
/// Always company-scoped: every row is filtered by `company_id = ?`.
pub async fn list(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<Account>> {
    let items = sqlx::query_as::<_, Account>(
        "SELECT id, company_id, account_code, account_name, account_class, \
         parent_code, active, created_at, updated_at \
         FROM chart_of_accounts \
         WHERE company_id = ?1 \
         ORDER BY account_code",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(items)
}

/// Fetch a single account by id; verify ownership.
pub async fn get(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<Account> {
    let account = sqlx::query_as::<_, Account>(
        "SELECT id, company_id, account_code, account_name, account_class, \
         parent_code, active, created_at, updated_at \
         FROM chart_of_accounts WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;

    // R14/R15 company isolation: cross-company access returns NotFound.
    if account.company_id != company_id {
        return Err(AppError::NotFound);
    }
    Ok(account)
}

/// Create a new account for the given company.
/// Returns `Conflict` if `(company_id, account_code)` already exists.
pub async fn create(
    pool: &SqlitePool,
    company_id: &str,
    input: AccountInput,
) -> AppResult<Account> {
    let id = new_id();
    let now = now_unix();

    sqlx::query(
        "INSERT INTO chart_of_accounts (
            id, company_id, account_code, account_name, account_class,
            parent_code, active, created_at, updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8
        )",
    )
    .bind(&id)
    .bind(company_id)
    .bind(&input.account_code)
    .bind(&input.account_name)
    .bind(input.account_class)
    .bind(&input.parent_code)
    .bind(input.active.unwrap_or(true))
    .bind(now)
    .execute(pool)
    .await
    .map_err(|e| {
        // UNIQUE constraint on (company_id, account_code) → Conflict.
        if e.to_string().contains("UNIQUE") {
            AppError::Conflict(format!(
                "Contul cu codul '{}' există deja pentru această companie.",
                input.account_code
            ))
        } else {
            AppError::Database(e)
        }
    })?;

    get(pool, &id, company_id).await
}

/// Update an account. Verifies ownership via `get` first.
/// The UPDATE SQL is also scoped with `AND company_id = ?` as defence-in-depth.
pub async fn update(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    input: UpdateAccountInput,
) -> AppResult<Account> {
    let current = get(pool, id, company_id).await?;
    let now = now_unix();

    let new_code = input
        .account_code
        .as_deref()
        .unwrap_or(&current.account_code);
    let new_name = input
        .account_name
        .as_deref()
        .unwrap_or(&current.account_name);
    let new_class = if input.account_class.is_some() {
        input.account_class
    } else {
        current.account_class
    };
    let new_parent = if input.parent_code.is_some() {
        input.parent_code.clone()
    } else {
        current.parent_code.clone()
    };
    let new_active = input.active.unwrap_or(current.active);

    sqlx::query(
        "UPDATE chart_of_accounts SET
            account_code  = ?2,
            account_name  = ?3,
            account_class = ?4,
            parent_code   = ?5,
            active        = ?6,
            updated_at    = ?7
        WHERE id = ?1 AND company_id = ?8",
    )
    .bind(id)
    .bind(new_code)
    .bind(new_name)
    .bind(new_class)
    .bind(new_parent)
    .bind(new_active)
    .bind(now)
    .bind(company_id)
    .execute(pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            AppError::Conflict(format!(
                "Contul cu codul '{}' există deja pentru această companie.",
                new_code
            ))
        } else {
            AppError::Database(e)
        }
    })?;

    get(pool, id, company_id).await
}

/// Delete an account. Verifies ownership first; cross-company returns NotFound.
pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    // Verify ownership first for a clear NotFound on cross-company attempts.
    let account = get(pool, id, company_id).await?;
    if account.company_id != company_id {
        return Err(AppError::NotFound);
    }
    let res = sqlx::query("DELETE FROM chart_of_accounts WHERE id = ?1 AND company_id = ?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

// ─── Standard RO chart seed ────────────────────────────────────────────────

/// Seed entry used internally by `seed_standard`.
struct SeedEntry {
    code: &'static str,
    name: &'static str,
    class: i64,
    parent: Option<&'static str>,
}

/// A representative subset of the standard Romanian chart of accounts (PCG).
/// Covers the most-used accounts across classes 1–7.
/// account_class = first digit of the account code.
fn standard_accounts() -> Vec<SeedEntry> {
    vec![
        // ── Clasa 1 — Conturi de capitaluri ────────────────────────────────
        SeedEntry {
            code: "101",
            name: "Capital",
            class: 1,
            parent: None,
        },
        SeedEntry {
            code: "1012",
            name: "Capital subscris vărsat",
            class: 1,
            parent: Some("101"),
        },
        SeedEntry {
            code: "104",
            name: "Prime de capital",
            class: 1,
            parent: None,
        },
        SeedEntry {
            code: "106",
            name: "Rezerve",
            class: 1,
            parent: None,
        },
        SeedEntry {
            code: "121",
            name: "Profit sau pierdere",
            class: 1,
            parent: None,
        },
        SeedEntry {
            code: "129",
            name: "Repartizarea profitului",
            class: 1,
            parent: None,
        },
        SeedEntry {
            code: "161",
            name: "Împrumuturi din emisiuni de obligațiuni",
            class: 1,
            parent: None,
        },
        SeedEntry {
            code: "162",
            name: "Credite bancare pe termen lung",
            class: 1,
            parent: None,
        },
        // ── Clasa 2 — Conturi de imobilizări ───────────────────────────────
        SeedEntry {
            code: "201",
            name: "Cheltuieli de constituire",
            class: 2,
            parent: None,
        },
        SeedEntry {
            code: "205",
            name: "Concesiuni, brevete și alte drepturi",
            class: 2,
            parent: None,
        },
        SeedEntry {
            code: "212",
            name: "Construcții",
            class: 2,
            parent: None,
        },
        SeedEntry {
            code: "213",
            name: "Instalații tehnice, mijloace de transport",
            class: 2,
            parent: None,
        },
        SeedEntry {
            code: "214",
            name: "Mobilier, aparatură birotică",
            class: 2,
            parent: None,
        },
        SeedEntry {
            code: "231",
            name: "Imobilizări corporale în curs de execuție",
            class: 2,
            parent: None,
        },
        SeedEntry {
            code: "281",
            name: "Amortizări privind imobilizările necorporale",
            class: 2,
            parent: None,
        },
        SeedEntry {
            code: "2813",
            name: "Amortizarea instalațiilor tehnice",
            class: 2,
            parent: Some("281"),
        },
        // ── Clasa 3 — Conturi de stocuri ───────────────────────────────────
        SeedEntry {
            code: "301",
            name: "Materii prime",
            class: 3,
            parent: None,
        },
        SeedEntry {
            code: "302",
            name: "Materiale consumabile",
            class: 3,
            parent: None,
        },
        SeedEntry {
            code: "371",
            name: "Mărfuri",
            class: 3,
            parent: None,
        },
        // ── Clasa 4 — Conturi de terți ─────────────────────────────────────
        SeedEntry {
            code: "401",
            name: "Furnizori",
            class: 4,
            parent: None,
        },
        SeedEntry {
            code: "404",
            name: "Furnizori de imobilizări",
            class: 4,
            parent: None,
        },
        SeedEntry {
            code: "411",
            name: "Clienți",
            class: 4,
            parent: None,
        },
        SeedEntry {
            code: "4111",
            name: "Clienți",
            class: 4,
            parent: Some("411"),
        },
        SeedEntry {
            code: "419",
            name: "Clienți — creditori",
            class: 4,
            parent: None,
        },
        SeedEntry {
            code: "421",
            name: "Personal — salarii datorate",
            class: 4,
            parent: None,
        },
        SeedEntry {
            code: "423",
            name: "Personal — ajutoare materiale datorate",
            class: 4,
            parent: None,
        },
        SeedEntry {
            code: "431",
            name: "Asigurări sociale",
            class: 4,
            parent: None,
        },
        SeedEntry {
            code: "441",
            name: "Impozitul pe profit/venit",
            class: 4,
            parent: None,
        },
        SeedEntry {
            code: "4423",
            name: "TVA de plată",
            class: 4,
            parent: None,
        },
        SeedEntry {
            code: "4424",
            name: "TVA de recuperat",
            class: 4,
            parent: None,
        },
        SeedEntry {
            code: "4426",
            name: "TVA deductibilă",
            class: 4,
            parent: None,
        },
        SeedEntry {
            code: "4427",
            name: "TVA colectată",
            class: 4,
            parent: None,
        },
        SeedEntry {
            code: "4428",
            name: "TVA neexigibilă",
            class: 4,
            parent: None,
        },
        SeedEntry {
            code: "461",
            name: "Debitori diverși",
            class: 4,
            parent: None,
        },
        SeedEntry {
            code: "462",
            name: "Creditori diverși",
            class: 4,
            parent: None,
        },
        // ── Clasa 5 — Conturi de trezorerie ────────────────────────────────
        SeedEntry {
            code: "5121",
            name: "Conturi la bănci în lei",
            class: 5,
            parent: None,
        },
        SeedEntry {
            code: "5124",
            name: "Conturi la bănci în valută",
            class: 5,
            parent: None,
        },
        SeedEntry {
            code: "531",
            name: "Casa",
            class: 5,
            parent: None,
        },
        SeedEntry {
            code: "5311",
            name: "Casa în lei",
            class: 5,
            parent: Some("531"),
        },
        SeedEntry {
            code: "5314",
            name: "Casa în valută",
            class: 5,
            parent: Some("531"),
        },
        // ── Clasa 6 — Conturi de cheltuieli ────────────────────────────────
        SeedEntry {
            code: "601",
            name: "Cheltuieli cu materiile prime",
            class: 6,
            parent: None,
        },
        SeedEntry {
            code: "602",
            name: "Cheltuieli cu materialele consumabile",
            class: 6,
            parent: None,
        },
        SeedEntry {
            code: "604",
            name: "Cheltuieli privind materialele nestocate",
            class: 6,
            parent: None,
        },
        SeedEntry {
            code: "607",
            name: "Cheltuieli privind mărfurile",
            class: 6,
            parent: None,
        },
        SeedEntry {
            code: "611",
            name: "Cheltuieli de întreținere și reparații",
            class: 6,
            parent: None,
        },
        SeedEntry {
            code: "612",
            name: "Cheltuieli cu redevențele, locațiile de gestiune și chiriile",
            class: 6,
            parent: None,
        },
        SeedEntry {
            code: "613",
            name: "Cheltuieli cu primele de asigurare",
            class: 6,
            parent: None,
        },
        SeedEntry {
            code: "621",
            name: "Cheltuieli cu colaboratorii",
            class: 6,
            parent: None,
        },
        SeedEntry {
            code: "622",
            name: "Cheltuieli privind comisioanele și onorariile",
            class: 6,
            parent: None,
        },
        SeedEntry {
            code: "623",
            name: "Cheltuieli de protocol, reclamă și publicitate",
            class: 6,
            parent: None,
        },
        SeedEntry {
            code: "624",
            name: "Cheltuieli cu transportul de bunuri și personal",
            class: 6,
            parent: None,
        },
        SeedEntry {
            code: "625",
            name: "Cheltuieli cu deplasări, detașări și transferări",
            class: 6,
            parent: None,
        },
        SeedEntry {
            code: "626",
            name: "Cheltuieli poștale și taxe de telecomunicații",
            class: 6,
            parent: None,
        },
        SeedEntry {
            code: "627",
            name: "Cheltuieli cu serviciile bancare și asimilate",
            class: 6,
            parent: None,
        },
        SeedEntry {
            code: "628",
            name: "Alte cheltuieli cu serviciile executate de terți",
            class: 6,
            parent: None,
        },
        SeedEntry {
            code: "635",
            name: "Cheltuieli cu alte impozite, taxe și vărsăminte",
            class: 6,
            parent: None,
        },
        SeedEntry {
            code: "641",
            name: "Cheltuieli cu salariile personalului",
            class: 6,
            parent: None,
        },
        SeedEntry {
            code: "645",
            name: "Cheltuieli privind asigurările și protecția socială",
            class: 6,
            parent: None,
        },
        SeedEntry {
            code: "658",
            name: "Alte cheltuieli de exploatare",
            class: 6,
            parent: None,
        },
        SeedEntry {
            code: "665",
            name: "Cheltuieli din diferențe de curs valutar",
            class: 6,
            parent: None,
        },
        SeedEntry {
            code: "666",
            name: "Cheltuieli privind dobânzile",
            class: 6,
            parent: None,
        },
        SeedEntry {
            code: "681",
            name: "Cheltuieli de exploatare privind amortizările",
            class: 6,
            parent: None,
        },
        // ── Clasa 7 — Conturi de venituri ──────────────────────────────────
        SeedEntry {
            code: "701",
            name: "Venituri din vânzarea produselor finite",
            class: 7,
            parent: None,
        },
        SeedEntry {
            code: "702",
            name: "Venituri din vânzarea semifabricatelor",
            class: 7,
            parent: None,
        },
        SeedEntry {
            code: "703",
            name: "Venituri din vânzarea produselor reziduale",
            class: 7,
            parent: None,
        },
        SeedEntry {
            code: "704",
            name: "Venituri din servicii prestate",
            class: 7,
            parent: None,
        },
        SeedEntry {
            code: "705",
            name: "Venituri din studii și cercetări",
            class: 7,
            parent: None,
        },
        SeedEntry {
            code: "706",
            name: "Venituri din redevențe, locații de gestiune și chirii",
            class: 7,
            parent: None,
        },
        SeedEntry {
            code: "707",
            name: "Venituri din vânzarea mărfurilor",
            class: 7,
            parent: None,
        },
        SeedEntry {
            code: "708",
            name: "Venituri din activități diverse",
            class: 7,
            parent: None,
        },
        SeedEntry {
            code: "709",
            name: "Reduceri comerciale acordate",
            class: 7,
            parent: None,
        },
        SeedEntry {
            code: "758",
            name: "Alte venituri din exploatare",
            class: 7,
            parent: None,
        },
        SeedEntry {
            code: "765",
            name: "Venituri din diferențe de curs valutar",
            class: 7,
            parent: None,
        },
        SeedEntry {
            code: "766",
            name: "Venituri din dobânzi",
            class: 7,
            parent: None,
        },
    ]
}

/// Insert a subset of the standard Romanian chart of accounts for a company,
/// only when the company has no accounts yet (idempotent — safe to call repeatedly).
pub async fn seed_standard(pool: &SqlitePool, company_id: &str) -> AppResult<usize> {
    // Check how many accounts already exist for this company.
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM chart_of_accounts WHERE company_id = ?1")
            .bind(company_id)
            .fetch_one(pool)
            .await?;

    if count > 0 {
        // Idempotent: already seeded — do nothing.
        return Ok(0);
    }

    let entries = standard_accounts();
    let mut inserted = 0usize;
    let now = now_unix();

    for entry in &entries {
        let id = new_id();
        sqlx::query(
            "INSERT OR IGNORE INTO chart_of_accounts (
                id, company_id, account_code, account_name, account_class,
                parent_code, active, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?7)",
        )
        .bind(&id)
        .bind(company_id)
        .bind(entry.code)
        .bind(entry.name)
        .bind(entry.class)
        .bind(entry.parent)
        .bind(now)
        .execute(pool)
        .await?;
        inserted += 1;
    }

    Ok(inserted)
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    /// Minimal in-memory schema for accounts Wave 4 tests.
    async fn setup_accounts_pool() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .unwrap();

        sqlx::query(
            "CREATE TABLE chart_of_accounts (
                id           TEXT    PRIMARY KEY NOT NULL,
                company_id   TEXT    NOT NULL,
                account_code TEXT    NOT NULL,
                account_name TEXT    NOT NULL,
                account_class INTEGER,
                parent_code  TEXT,
                active       INTEGER NOT NULL DEFAULT 1,
                created_at   INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at   INTEGER NOT NULL DEFAULT (unixepoch()),
                UNIQUE (company_id, account_code)
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Seed: two companies, one account each.
        sqlx::query(
            "INSERT INTO chart_of_accounts \
             (id, company_id, account_code, account_name, account_class) \
             VALUES \
             ('a1', 'comp-1', '4111', 'Clienți', 4), \
             ('a2', 'comp-2', '401', 'Furnizori', 4)",
        )
        .execute(&pool)
        .await
        .unwrap();

        pool
    }

    // ── get: wrong company → NotFound ───────────────────────────────────────

    #[tokio::test]
    async fn wave4_account_get_wrong_company_returns_not_found() {
        let pool = setup_accounts_pool().await;
        let result = get(&pool, "a1", "comp-2").await;
        assert!(
            matches!(result, Err(AppError::NotFound)),
            "get with wrong company_id must return NotFound"
        );
    }

    #[tokio::test]
    async fn wave4_account_get_correct_company_succeeds() {
        let pool = setup_accounts_pool().await;
        let result = get(&pool, "a1", "comp-1").await;
        assert!(result.is_ok(), "get with correct company_id must succeed");
        assert_eq!(result.unwrap().account_code, "4111");
    }

    // ── update: wrong company → NotFound ────────────────────────────────────

    #[tokio::test]
    async fn wave4_account_update_wrong_company_returns_not_found() {
        let pool = setup_accounts_pool().await;
        let input = UpdateAccountInput {
            account_name: Some("Renamed".to_string()),
            ..Default::default()
        };
        let result = update(&pool, "a1", "comp-2", input).await;
        assert!(
            matches!(result, Err(AppError::NotFound)),
            "update with wrong company_id must return NotFound"
        );
        // Name must be unchanged.
        let account = get(&pool, "a1", "comp-1").await.unwrap();
        assert_eq!(account.account_name, "Clienți", "name must not change");
    }

    #[tokio::test]
    async fn wave4_account_update_correct_company_succeeds() {
        let pool = setup_accounts_pool().await;
        let input = UpdateAccountInput {
            account_name: Some("Clienți interni".to_string()),
            ..Default::default()
        };
        let result = update(&pool, "a1", "comp-1", input).await;
        assert!(
            result.is_ok(),
            "update with correct company_id must succeed"
        );
        let account = get(&pool, "a1", "comp-1").await.unwrap();
        assert_eq!(account.account_name, "Clienți interni");
    }

    // ── delete: wrong company → NotFound ────────────────────────────────────

    #[tokio::test]
    async fn wave4_account_delete_wrong_company_returns_not_found() {
        let pool = setup_accounts_pool().await;
        let result = delete(&pool, "a1", "comp-2").await;
        assert!(
            matches!(result, Err(AppError::NotFound)),
            "delete with wrong company_id must return NotFound"
        );
        // Account must still exist.
        let still_there = get(&pool, "a1", "comp-1").await;
        assert!(still_there.is_ok(), "account must not have been deleted");
    }

    #[tokio::test]
    async fn wave4_account_delete_correct_company_succeeds() {
        let pool = setup_accounts_pool().await;
        let result = delete(&pool, "a1", "comp-1").await;
        assert!(
            result.is_ok(),
            "delete with correct company_id must succeed"
        );
        let gone = get(&pool, "a1", "comp-1").await;
        assert!(
            matches!(gone, Err(AppError::NotFound)),
            "account must be gone after correct-company delete"
        );
    }

    // ── create + list round-trip ─────────────────────────────────────────────

    #[tokio::test]
    async fn wave4_account_create_list_round_trip() {
        let pool = setup_accounts_pool().await;
        let input = AccountInput {
            account_code: "5121".to_string(),
            account_name: "Conturi la bănci în lei".to_string(),
            account_class: Some(5),
            parent_code: None,
            active: Some(true),
        };
        let created = create(&pool, "comp-1", input).await.unwrap();
        assert_eq!(created.company_id, "comp-1");
        assert_eq!(created.account_code, "5121");
        assert_eq!(created.account_class, Some(5));

        let list_result = list(&pool, "comp-1").await.unwrap();
        // comp-1 now has a1 + newly created
        assert_eq!(list_result.len(), 2);
        assert!(list_result.iter().any(|a| a.id == created.id));

        // comp-2 must not see comp-1's accounts.
        let list_comp2 = list(&pool, "comp-2").await.unwrap();
        assert_eq!(list_comp2.len(), 1);
        assert_eq!(list_comp2[0].id, "a2");
    }

    // ── cross-company isolation: list ────────────────────────────────────────

    #[tokio::test]
    async fn wave4_account_list_is_company_scoped() {
        let pool = setup_accounts_pool().await;
        let list1 = list(&pool, "comp-1").await.unwrap();
        let list2 = list(&pool, "comp-2").await.unwrap();
        assert!(list1.iter().all(|a| a.company_id == "comp-1"));
        assert!(list2.iter().all(|a| a.company_id == "comp-2"));
        assert!(list1.iter().all(|a| a.id != "a2"));
        assert!(list2.iter().all(|a| a.id != "a1"));
    }

    // ── seed_standard: populates when empty + idempotent ─────────────────────

    #[tokio::test]
    async fn wave4_seed_standard_populates_when_empty() {
        let pool = setup_accounts_pool().await;
        // comp-99 has no accounts.
        let inserted = seed_standard(&pool, "comp-99").await.unwrap();
        assert!(
            inserted > 0,
            "seed_standard must insert entries for empty company"
        );
        let accounts = list(&pool, "comp-99").await.unwrap();
        assert!(!accounts.is_empty(), "accounts must be present after seed");
        // All seeded accounts belong to comp-99.
        assert!(accounts.iter().all(|a| a.company_id == "comp-99"));
        // Verify some well-known entries are present.
        assert!(accounts.iter().any(|a| a.account_code == "4111"));
        assert!(accounts.iter().any(|a| a.account_code == "4427"));
        assert!(accounts.iter().any(|a| a.account_code == "5121"));
        assert!(accounts.iter().any(|a| a.account_code == "707"));
    }

    #[tokio::test]
    async fn wave4_seed_standard_is_idempotent() {
        let pool = setup_accounts_pool().await;
        // First call seeds.
        let first = seed_standard(&pool, "comp-99").await.unwrap();
        assert!(first > 0);
        // Second call must return 0 (no duplicates inserted).
        let second = seed_standard(&pool, "comp-99").await.unwrap();
        assert_eq!(second, 0, "second seed_standard call must insert nothing");
        // Count must be stable.
        let accounts_first_count = list(&pool, "comp-99").await.unwrap().len();
        let accounts_second_count = list(&pool, "comp-99").await.unwrap().len();
        assert_eq!(
            accounts_first_count, accounts_second_count,
            "account count must not change after second seed"
        );
    }

    // ── seed_standard does NOT affect companies that already have accounts ────

    #[tokio::test]
    async fn wave4_seed_standard_skips_non_empty_company() {
        let pool = setup_accounts_pool().await;
        // comp-1 already has 1 account (a1).
        let inserted = seed_standard(&pool, "comp-1").await.unwrap();
        assert_eq!(
            inserted, 0,
            "seed_standard must not insert when company already has accounts"
        );
        let accounts = list(&pool, "comp-1").await.unwrap();
        assert_eq!(accounts.len(), 1, "account count must remain 1");
    }
}
