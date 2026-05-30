//! KV store pentru setări runtime.
//!
//! Cheile cunoscute (constante) sunt definite în `keys`. Valoarea e mereu
//! TEXT — serializăm la callsite ce vrem (bool, JSON, etc.).

use sqlx::SqlitePool;

use crate::db::models::now_unix;
use crate::error::AppResult;

#[allow(dead_code)]
pub mod keys {
    //! Chei cunoscute pentru tabela settings. Unele sunt folosite în fazele
    //! ulterioare (background tasks, archive, tray); le păstrăm aici ca
    //! sursă unică de adevăr.
    pub const FIRST_RUN_COMPLETED: &str = "first_run_completed";
    pub const USE_ANAF_TEST_ENV: &str = "use_anaf_test_env";
    pub const POLLING_ENABLED: &str = "polling_enabled";
    pub const NOTIFICATIONS_QUIET_HOURS: &str = "quiet_hours";
    pub const NOTIFICATIONS_SOUND: &str = "notifications_sound";
    pub const RUN_ON_STARTUP: &str = "run_on_startup";
    pub const ARCHIVE_PATH_OVERRIDE: &str = "archive_path_override";
    pub const LAST_SYNC_AT: &str = "last_sync_at";
}

pub async fn get(pool: &SqlitePool, key: &str) -> AppResult<Option<String>> {
    Ok(
        sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?1")
            .bind(key)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn set(pool: &SqlitePool, key: &str, value: &str) -> AppResult<()> {
    let now = now_unix();
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(value)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_all(pool: &SqlitePool) -> AppResult<Vec<(String, String)>> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT key, value FROM settings ORDER BY key")
            .fetch_all(pool)
            .await?;
    Ok(rows)
}

pub async fn get_bool(pool: &SqlitePool, key: &str, default: bool) -> AppResult<bool> {
    Ok(get(pool, key)
        .await?
        .map(|v| v == "true" || v == "1")
        .unwrap_or(default))
}
