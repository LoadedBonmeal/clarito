//! Inițializare pool SQLite + rulare migrații.
//!
//! Path-ul DB-ului e rezolvat din `app_data_dir()` al Tauri-ului, deci diferă
//! per OS:
//! - macOS: `~/Library/Application Support/com.lucaris.efactura/data.db`
//! - Windows: `%APPDATA%\com.lucaris.efactura\data.db`
//!
//! Migrațiile sunt embeddate la compile time prin `sqlx::migrate!`.

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

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

    // Build options directly from the PathBuf instead of formatting a
    // "sqlite://<path>" URL string.  On Windows the path contains backslashes
    // (e.g. C:\Users\...\data.db) which are not valid inside a sqlite:// URL
    // and caused the DB to never open (P0 Windows fix).
    let options = SqliteConnectOptions::new()
        .filename(&db_path)
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal);

    let pool = SqlitePoolOptions::new()
        .max_connections(MAX_CONNECTIONS)
        .connect_with(options)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    // Restrict DB file permissions to owner read/write only (0o600) to protect
    // PII stored in plaintext.  Also applied to WAL/SHM sidecars if present.
    // Best-effort: log on failure but never crash.
    #[cfg(unix)]
    {
        let mode_600 = std::fs::Permissions::from_mode(0o600);
        if let Err(e) = std::fs::set_permissions(&db_path, mode_600) {
            tracing::warn!(path = %db_path.display(), error = %e, "Failed to set 0o600 on DB file");
        }
        for suffix in &["-wal", "-shm"] {
            // SQLite WAL sidecars are named <db_path>-wal and <db_path>-shm,
            // i.e. the suffix is appended to the full filename (including .db).
            let sidecar_path = {
                let mut p = db_path.as_os_str().to_owned();
                p.push(suffix);
                PathBuf::from(p)
            };
            let sidecar = suffix; // keep binding for the warning message
            if sidecar_path.exists() {
                if let Err(e) =
                    std::fs::set_permissions(&sidecar_path, std::fs::Permissions::from_mode(0o600))
                {
                    tracing::warn!(path = %sidecar_path.display(), sidecar, error = %e, "Failed to set 0o600 on DB sidecar");
                }
            }
        }
    }

    tracing::info!(path = %db_path.display(), "SQLite pool ready");
    Ok(pool)
}
