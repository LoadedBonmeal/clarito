//! Catalog editabil de cote TVA — tabel GLOBAL (fără company_id).
//!
//! Cotele TVA din România sunt reglementate la nivel național și se aplică
//! uniform tuturor companiilor. Din acest motiv `vat_rates` este un tabel
//! GLOBAL, fără coloana `company_id`. Aceasta este excepția deliberată de la
//! regula de company-scoping din restul aplicației. Nu adăuga `company_id`
//! aici — auditurile viitoare trebuie să trateze absența sa ca intenționată.
//!
//! Coloana `rate` este TEXT (ex. "19", "9") pentru a respecta convenția
//! Decimal-as-TEXT a aplicației.

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

// ─── Model ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct VatRate {
    pub id: String,
    pub rate: String,
    pub label: String,
    pub active: bool,
    pub sort_order: i64,
    pub created_at: i64,
}

// ─── Inputs ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VatRateInput {
    pub rate: String,
    pub label: String,
    pub active: Option<bool>,
    pub sort_order: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateVatRateInput {
    pub rate: Option<String>,
    pub label: Option<String>,
    pub active: Option<bool>,
    pub sort_order: Option<i64>,
}

// ─── Queries ───────────────────────────────────────────────────────────────

/// List all VAT rates, ordered by sort_order then rate.
/// When `active_only` is true, only rows with `active = 1` are returned.
///
/// NOTE: This table is global (no company_id). All companies share the same
/// rate catalog — see module-level doc for the rationale.
pub async fn list(pool: &SqlitePool, active_only: bool) -> AppResult<Vec<VatRate>> {
    let items = if active_only {
        sqlx::query_as::<_, VatRate>(
            "SELECT id, rate, label, active, sort_order, created_at \
             FROM vat_rates \
             WHERE active = 1 \
             ORDER BY sort_order, CAST(rate AS REAL)",
        )
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query_as::<_, VatRate>(
            "SELECT id, rate, label, active, sort_order, created_at \
             FROM vat_rates \
             ORDER BY sort_order, CAST(rate AS REAL)",
        )
        .fetch_all(pool)
        .await?
    };
    Ok(items)
}

/// Fetch a single VAT rate by id.
pub async fn get(pool: &SqlitePool, id: &str) -> AppResult<VatRate> {
    sqlx::query_as::<_, VatRate>(
        "SELECT id, rate, label, active, sort_order, created_at \
         FROM vat_rates WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}

/// Create a new VAT rate entry.
pub async fn create(pool: &SqlitePool, input: VatRateInput) -> AppResult<VatRate> {
    let id = new_id();
    let now = now_unix();

    sqlx::query(
        "INSERT INTO vat_rates (id, rate, label, active, sort_order, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )
    .bind(&id)
    .bind(&input.rate)
    .bind(&input.label)
    .bind(input.active.unwrap_or(true))
    .bind(input.sort_order.unwrap_or(0))
    .bind(now)
    .execute(pool)
    .await?;

    get(pool, &id).await
}

/// Update an existing VAT rate entry. Fields not supplied keep their current values.
pub async fn update(pool: &SqlitePool, id: &str, input: UpdateVatRateInput) -> AppResult<VatRate> {
    let current = get(pool, id).await?;

    sqlx::query(
        "UPDATE vat_rates SET \
            rate       = ?2, \
            label      = ?3, \
            active     = ?4, \
            sort_order = ?5 \
         WHERE id = ?1",
    )
    .bind(id)
    .bind(input.rate.as_deref().unwrap_or(&current.rate))
    .bind(input.label.as_deref().unwrap_or(&current.label))
    .bind(input.active.unwrap_or(current.active))
    .bind(input.sort_order.unwrap_or(current.sort_order))
    .execute(pool)
    .await?;

    get(pool, id).await
}

/// Delete a VAT rate entry by id.
pub async fn delete(pool: &SqlitePool, id: &str) -> AppResult<()> {
    // Verify existence first so we return a clean NotFound instead of 0 rows affected.
    get(pool, id).await?;
    let res = sqlx::query("DELETE FROM vat_rates WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Toggle the `active` flag for a VAT rate entry.
pub async fn set_active(pool: &SqlitePool, id: &str, active: bool) -> AppResult<VatRate> {
    get(pool, id).await?;
    sqlx::query("UPDATE vat_rates SET active = ?2 WHERE id = ?1")
        .bind(id)
        .bind(active)
        .execute(pool)
        .await?;
    get(pool, id).await
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    /// Build an in-memory SQLite pool with the vat_rates schema and seed rows.
    async fn setup_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .unwrap();

        sqlx::query(
            "CREATE TABLE vat_rates (
                id         TEXT    PRIMARY KEY,
                rate       TEXT    NOT NULL,
                label      TEXT    NOT NULL,
                active     INTEGER NOT NULL DEFAULT 1,
                sort_order INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Mirror the seed rows from migration 0014.
        sqlx::query(
            "INSERT INTO vat_rates (id, rate, label, active, sort_order) VALUES \
             ('vat-19', '19', 'Standard 19%', 1, 0), \
             ('vat-21', '21', 'Standard 21%', 1, 1), \
             ('vat-9',  '9',  'Redus 9%',     1, 2), \
             ('vat-11', '11', 'Redus 11%',    1, 3), \
             ('vat-5',  '5',  'Redus 5%',     1, 4), \
             ('vat-0',  '0',  'Cotă zero 0%', 1, 5)",
        )
        .execute(&pool)
        .await
        .unwrap();

        pool
    }

    // ── Seed presence ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn wave2_vat_rates_seed_all_present() {
        let pool = setup_pool().await;
        let rates = list(&pool, false).await.unwrap();
        assert_eq!(rates.len(), 6, "migration seed must produce exactly 6 rows");
        let ids: Vec<&str> = rates.iter().map(|r| r.id.as_str()).collect();
        for expected in &["vat-0", "vat-5", "vat-9", "vat-11", "vat-19", "vat-21"] {
            assert!(ids.contains(expected), "seed row {expected} missing");
        }
    }

    // ── Create / list round-trip ─────────────────────────────────────────────

    #[tokio::test]
    async fn wave2_vat_rates_create_list_round_trip() {
        let pool = setup_pool().await;
        let input = VatRateInput {
            rate: "8".to_string(),
            label: "Redus special 8%".to_string(),
            active: Some(true),
            sort_order: Some(10),
        };
        let created = create(&pool, input).await.unwrap();
        assert_eq!(created.rate, "8");
        assert_eq!(created.label, "Redus special 8%");
        assert!(created.active);

        let all = list(&pool, false).await.unwrap();
        assert_eq!(all.len(), 7, "create must add one row");
        assert!(all.iter().any(|r| r.id == created.id));
    }

    // ── active_only filter ────────────────────────────────────────────────────

    #[tokio::test]
    async fn wave2_vat_rates_active_only_filter() {
        let pool = setup_pool().await;
        // Deactivate one of the seeded rows.
        set_active(&pool, "vat-21", false).await.unwrap();

        let active = list(&pool, true).await.unwrap();
        assert_eq!(
            active.len(),
            5,
            "active_only must return 5 after deactivating one"
        );
        assert!(
            active.iter().all(|r| r.active),
            "active_only must not include inactive rows"
        );

        let all = list(&pool, false).await.unwrap();
        assert_eq!(all.len(), 6, "list all must still return 6");
    }

    // ── update label ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn wave2_vat_rates_update_label() {
        let pool = setup_pool().await;
        let input = UpdateVatRateInput {
            label: Some("TVA Standard 19% (România)".to_string()),
            ..Default::default()
        };
        let updated = update(&pool, "vat-19", input).await.unwrap();
        assert_eq!(updated.label, "TVA Standard 19% (România)");
        assert_eq!(updated.rate, "19", "rate must be unchanged");
    }

    // ── toggle active ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn wave2_vat_rates_toggle_active() {
        let pool = setup_pool().await;
        // Deactivate.
        let after_deactivate = set_active(&pool, "vat-21", false).await.unwrap();
        assert!(
            !after_deactivate.active,
            "must be inactive after set_active(false)"
        );

        // Re-activate.
        let after_activate = set_active(&pool, "vat-21", true).await.unwrap();
        assert!(
            after_activate.active,
            "must be active after set_active(true)"
        );
    }

    // ── get unknown id → NotFound ────────────────────────────────────────────

    #[tokio::test]
    async fn wave2_vat_rates_get_unknown_returns_not_found() {
        let pool = setup_pool().await;
        let result = get(&pool, "nonexistent-id").await;
        assert!(
            matches!(result, Err(AppError::NotFound)),
            "get with unknown id must return NotFound"
        );
    }
}
