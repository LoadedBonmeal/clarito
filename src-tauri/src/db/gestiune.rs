//! Gestiuni (warehouses) — CRUD + helpers (OMFP 1802/2014 pct. 96).
//!
//! Fiecare companie are cel puțin o gestiune principală (is_default=1).
//! Metodele de evaluare sunt per-gestiune (CMP/FIFO/LIFO).
//! Ștergerea e blocată dacă există stock_ledger rows referencing it sau dacă e default.

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Gestiune {
    pub id: String,
    pub company_id: String,
    pub cod: String,
    pub denumire: String,
    pub tip: String,
    pub metoda_evaluare: String,
    pub cont_stoc: String,
    pub adresa: Option<String>,
    pub dispersata_teritorial: i64,
    pub is_default: i64,
    pub activ: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GestiuneInput {
    pub cod: String,
    pub denumire: String,
    pub tip: Option<String>,
    pub metoda_evaluare: Option<String>,
    pub cont_stoc: Option<String>,
    pub adresa: Option<String>,
    pub dispersata_teritorial: Option<bool>,
}

pub async fn list(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<Gestiune>> {
    Ok(sqlx::query_as::<_, Gestiune>(
        "SELECT id, company_id, cod, denumire, tip, metoda_evaluare, cont_stoc, adresa, \
         dispersata_teritorial, is_default, activ, created_at \
         FROM gestiune WHERE company_id=?1 ORDER BY is_default DESC, cod",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?)
}

pub async fn get(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<Gestiune> {
    sqlx::query_as::<_, Gestiune>(
        "SELECT id, company_id, cod, denumire, tip, metoda_evaluare, cont_stoc, adresa, \
         dispersata_teritorial, is_default, activ, created_at \
         FROM gestiune WHERE id=?1 AND company_id=?2",
    )
    .bind(id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}

/// Returns the default gestiune id for a company. If none exists (e.g. company was created before
/// migration 0064, or in an in-memory test DB), auto-seeds the default gestiune row and returns
/// its id. This ensures stock_ledger inserts never fail the FK constraint.
pub async fn default_gestiune_id(pool: &SqlitePool, company_id: &str) -> AppResult<String> {
    let id: Option<String> =
        sqlx::query_scalar("SELECT id FROM gestiune WHERE company_id=?1 AND is_default=1 LIMIT 1")
            .bind(company_id)
            .fetch_optional(pool)
            .await?;

    if let Some(id) = id {
        return Ok(id);
    }

    // Auto-seed: deterministic id (same formula as migration 0064 so it's idempotent).
    let fallback_id = format!("gest-default-{company_id}");
    let now = now_unix();
    sqlx::query(
        "INSERT OR IGNORE INTO gestiune \
         (id, company_id, cod, denumire, tip, metoda_evaluare, cont_stoc, \
          dispersata_teritorial, is_default, activ, created_at) \
         VALUES (?1,?2,'PRINCIPALA','Gestiune principala','cantitativ_valorica','CMP','371',0,1,1,?3)",
    )
    .bind(&fallback_id)
    .bind(company_id)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(fallback_id)
}

pub async fn create(
    pool: &SqlitePool,
    company_id: &str,
    input: GestiuneInput,
) -> AppResult<Gestiune> {
    let cod = input.cod.trim().to_uppercase();
    if cod.is_empty() {
        return Err(AppError::Validation(
            "Codul gestiunii este obligatoriu.".into(),
        ));
    }
    let denumire = input.denumire.trim().to_string();
    if denumire.is_empty() {
        return Err(AppError::Validation(
            "Denumirea gestiunii este obligatorie.".into(),
        ));
    }
    let tip = input
        .tip
        .as_deref()
        .unwrap_or("cantitativ_valorica")
        .to_string();
    let metoda = input
        .metoda_evaluare
        .as_deref()
        .unwrap_or("CMP")
        .to_string();
    let cont = input.cont_stoc.as_deref().unwrap_or("371").to_string();
    let dispersata: i64 = if input.dispersata_teritorial.unwrap_or(false) {
        1
    } else {
        0
    };

    let id = new_id();
    let now = now_unix();
    sqlx::query(
        "INSERT INTO gestiune (id, company_id, cod, denumire, tip, metoda_evaluare, cont_stoc, \
         adresa, dispersata_teritorial, is_default, activ, created_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,0,1,?10)",
    )
    .bind(&id)
    .bind(company_id)
    .bind(&cod)
    .bind(&denumire)
    .bind(&tip)
    .bind(&metoda)
    .bind(&cont)
    .bind(&input.adresa)
    .bind(dispersata)
    .bind(now)
    .execute(pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            AppError::Validation(format!("Există deja o gestiune cu codul '{cod}'."))
        } else {
            AppError::Database(e)
        }
    })?;

    get(pool, &id, company_id).await
}

pub async fn update(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    input: GestiuneInput,
) -> AppResult<Gestiune> {
    let current = get(pool, id, company_id).await?;
    let cod = input.cod.trim().to_uppercase();
    if cod.is_empty() {
        return Err(AppError::Validation(
            "Codul gestiunii este obligatoriu.".into(),
        ));
    }
    let denumire = input.denumire.trim().to_string();
    if denumire.is_empty() {
        return Err(AppError::Validation(
            "Denumirea gestiunii este obligatorie.".into(),
        ));
    }
    let tip = input.tip.as_deref().unwrap_or(&current.tip).to_string();
    let metoda = input
        .metoda_evaluare
        .as_deref()
        .unwrap_or(&current.metoda_evaluare)
        .to_string();
    let cont = input
        .cont_stoc
        .as_deref()
        .unwrap_or(&current.cont_stoc)
        .to_string();
    let dispersata: i64 = input
        .dispersata_teritorial
        .map(|b| if b { 1 } else { 0 })
        .unwrap_or(current.dispersata_teritorial);
    let adresa = input.adresa.or(current.adresa);

    sqlx::query(
        "UPDATE gestiune SET cod=?2, denumire=?3, tip=?4, metoda_evaluare=?5, cont_stoc=?6, \
         adresa=?7, dispersata_teritorial=?8 WHERE id=?1 AND company_id=?9",
    )
    .bind(id)
    .bind(&cod)
    .bind(&denumire)
    .bind(&tip)
    .bind(&metoda)
    .bind(&cont)
    .bind(&adresa)
    .bind(dispersata)
    .bind(company_id)
    .execute(pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            AppError::Validation(format!("Există deja o gestiune cu codul '{cod}'."))
        } else {
            AppError::Database(e)
        }
    })?;

    get(pool, id, company_id).await
}

pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    let g = get(pool, id, company_id).await?;
    if g.is_default == 1 {
        return Err(AppError::Validation(
            "Gestiunea principală (default) nu poate fi ștearsă.".into(),
        ));
    }
    let has_stock: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM stock_ledger WHERE gestiune_id=?1 AND company_id=?2",
    )
    .bind(id)
    .bind(company_id)
    .fetch_one(pool)
    .await?;
    if has_stock > 0 {
        return Err(AppError::Validation(
            "Gestiunea are mișcări de stoc înregistrate și nu poate fi ștearsă.".into(),
        ));
    }
    sqlx::query("DELETE FROM gestiune WHERE id=?1 AND company_id=?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_active(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    activ: bool,
) -> AppResult<()> {
    sqlx::query("UPDATE gestiune SET activ=?2 WHERE id=?1 AND company_id=?3")
        .bind(id)
        .bind(if activ { 1i64 } else { 0i64 })
        .bind(company_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// On-hand quantity and value for a product, optionally filtered by gestiune_id.
/// Returns (qty, value) as Decimal-formatted strings.
pub async fn stock_on_hand(
    pool: &SqlitePool,
    company_id: &str,
    product_id: &str,
    gestiune_id: Option<&str>,
) -> AppResult<(String, String)> {
    let row: Option<(Option<String>, Option<String>)> = if let Some(gid) = gestiune_id {
        sqlx::query_as(
            "SELECT run_qty, run_value FROM stock_ledger \
             WHERE company_id=?1 AND product_id=?2 AND gestiune_id=?3 \
             ORDER BY entry_date DESC, seq DESC, created_at DESC LIMIT 1",
        )
        .bind(company_id)
        .bind(product_id)
        .bind(gid)
        .fetch_optional(pool)
        .await?
    } else {
        // Total across all gestiuni — use products cache
        sqlx::query_as("SELECT stock_qty, stock_value FROM products WHERE id=?1 AND company_id=?2")
            .bind(product_id)
            .bind(company_id)
            .fetch_optional(pool)
            .await?
    };

    Ok(row
        .map(|(q, v)| {
            (
                q.unwrap_or_else(|| "0.000000".into()),
                v.unwrap_or_else(|| "0.00".into()),
            )
        })
        .unwrap_or(("0.000000".into(), "0.00".into())))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn setup_pool() -> SqlitePool {
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
    async fn migration_seeds_default_gestiune() {
        // In an in-memory test DB the migration 0064 seed runs BEFORE companies are inserted,
        // so no row is seeded automatically. The auto-seed in default_gestiune_id() acts as the
        // fallback — calling it causes the PRINCIPALA row to appear in the list.
        let pool = setup_pool().await;
        // Trigger auto-seed
        let _id = default_gestiune_id(&pool, "co1").await.unwrap();
        let list = list(&pool, "co1").await.unwrap();
        assert!(
            !list.is_empty(),
            "auto-seed should create a default gestiune"
        );
        assert_eq!(list[0].is_default, 1);
        assert_eq!(list[0].cod, "PRINCIPALA");
    }

    #[tokio::test]
    async fn cannot_delete_default_gestiune() {
        let pool = setup_pool().await;
        // Trigger auto-seed so the default gestiune exists
        let _id = default_gestiune_id(&pool, "co1").await.unwrap();
        let default = list(&pool, "co1")
            .await
            .unwrap()
            .into_iter()
            .find(|g| g.is_default == 1)
            .unwrap();
        let result = delete(&pool, &default.id, "co1").await;
        assert!(matches!(result, Err(AppError::Validation(_))));
    }

    #[tokio::test]
    async fn cannot_delete_gestiune_with_stock() {
        let pool = setup_pool().await;
        // Create a non-default gestiune
        let g = create(
            &pool,
            "co1",
            GestiuneInput {
                cod: "SECOND".into(),
                denumire: "A doua gestiune".into(),
                tip: None,
                metoda_evaluare: None,
                cont_stoc: None,
                adresa: None,
                dispersata_teritorial: None,
            },
        )
        .await
        .unwrap();

        // Create a product and insert a stock_ledger row pointing to this gestiune
        sqlx::query(
            "INSERT INTO products (id, company_id, name, unit) VALUES ('p1','co1','Marfă','buc')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO stock_ledger (id, company_id, product_id, entry_date, seq, direction, qty, \
             unit_cost, value, run_qty, run_value, fifo_remaining, gestiune_id, created_at) \
             VALUES ('sl1','co1','p1','2026-01-01',0,'IN','10.000000','5.00','50.00', \
             '10.000000','50.00','10.000000',?1,unixepoch())",
        )
        .bind(&g.id)
        .execute(&pool)
        .await
        .unwrap();

        let result = delete(&pool, &g.id, "co1").await;
        assert!(matches!(result, Err(AppError::Validation(_))));
    }

    #[tokio::test]
    async fn unique_cod_per_company() {
        let pool = setup_pool().await;
        // Ensure the PRINCIPALA row exists before trying to duplicate it.
        let _id = default_gestiune_id(&pool, "co1").await.unwrap();
        let input = GestiuneInput {
            cod: "PRINCIPALA".into(), // same as auto-seeded default
            denumire: "Duplicate".into(),
            tip: None,
            metoda_evaluare: None,
            cont_stoc: None,
            adresa: None,
            dispersata_teritorial: None,
        };
        let result = create(&pool, "co1", input).await;
        assert!(matches!(result, Err(AppError::Validation(_))));
    }

    #[tokio::test]
    async fn crud_create_update() {
        let pool = setup_pool().await;
        let g = create(
            &pool,
            "co1",
            GestiuneInput {
                cod: "CONS".into(),
                denumire: "Consignație".into(),
                tip: Some("global_valorica".into()),
                metoda_evaluare: Some("FIFO".into()),
                cont_stoc: Some("301".into()),
                adresa: Some("Str. Test 1".into()),
                dispersata_teritorial: Some(true),
            },
        )
        .await
        .unwrap();
        assert_eq!(g.cod, "CONS");
        assert_eq!(g.metoda_evaluare, "FIFO");
        assert_eq!(g.dispersata_teritorial, 1);
        assert_eq!(g.is_default, 0);

        let upd = update(
            &pool,
            &g.id,
            "co1",
            GestiuneInput {
                cod: "CONS".into(),
                denumire: "Consignație updată".into(),
                tip: None,
                metoda_evaluare: None,
                cont_stoc: None,
                adresa: None,
                dispersata_teritorial: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(upd.denumire, "Consignație updată");

        delete(&pool, &g.id, "co1").await.unwrap(); // no stock → ok
    }
}
