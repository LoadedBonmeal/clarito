//! Inițializare pool SQLite + rulare migrații.
//!
//! Path-ul DB-ului e rezolvat din `app_data_dir()` al Tauri-ului, deci diferă
//! per OS:
//! - macOS: `~/Library/Application Support/com.lucaris.efactura/data.db`
//! - Windows: `%APPDATA%\com.lucaris.efactura\data.db`
//!
//! Migrațiile sunt embeddate la compile time prin `sqlx::migrate!`.

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::str::FromStr;
use tauri::{AppHandle, Manager};

use crate::error::AppResult;

pub const MAX_CONNECTIONS: u32 = 5;

/// Construiește path-ul absolut către fișierul SQLite, creând directorul
/// părinte dacă nu există.
pub fn resolve_db_path(app: &AppHandle) -> AppResult<PathBuf> {
    let dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("data.db"))
}

/// Inițializează pool-ul și rulează migrațiile pending.
pub async fn init(app: &AppHandle) -> AppResult<SqlitePool> {
    let db_path = resolve_db_path(app)?;
    let db_url = format!("sqlite://{}", db_path.display());

    let options = SqliteConnectOptions::from_str(&db_url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);

    let pool = SqlitePoolOptions::new()
        .max_connections(MAX_CONNECTIONS)
        .connect_with(options)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    tracing::info!(path = %db_path.display(), "SQLite pool ready");
    Ok(pool)
}
