//! GDPR / data-portability commands.
//!
//! Provides two Tauri commands:
//!  - `export_all_my_data`: produces a ZIP of the entire DB + archive files
//!    at a user-chosen path.
//!  - `wipe_all_data`: irreversibly truncates all app tables, deletes ANAF +
//!    SmartBill keychain tokens per company, clears the archive directory, and
//!    removes the trial keychain marker.
//!
//! Implementation is self-contained (does NOT call into archive.rs internals)
//! to avoid coupling with the concurrently-edited archive module.

use std::io::Write;
use std::path::PathBuf;

use keyring::Entry;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::error::{AppError, AppResult};
use crate::state::AppState;

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DataExportResult {
    pub path: String,
    pub bytes: u64,
}

// ─── Keychain constants (mirror from license.rs) ──────────────────────────────

const TRIAL_KC_SERVICE: &str = "ro.lucaris.efactura.trial.v1";
const TRIAL_KC_ACCOUNT: &str = "trial_status";

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Compute the archive directory path, respecting any user override stored in settings.
async fn resolve_archive_dir(state: &AppState, app: &AppHandle) -> AppResult<PathBuf> {
    let data_dir = app.path().app_data_dir()?;
    let override_val =
        crate::db::settings::get(&state.db, crate::db::settings::keys::ARCHIVE_PATH_OVERRIDE)
            .await
            .unwrap_or(None);
    Ok(match override_val {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => data_dir.join("archive"),
    })
}

/// Recursively add all files under `dir` into the ZIP archive with paths
/// relative to `root_dir`'s parent (so the ZIP contains e.g. `archive/sent/INV.xml`).
fn zip_dir_recursive<W: Write + std::io::Seek>(
    dir: &std::path::Path,
    root_dir: &std::path::Path,
    zip: &mut zip::ZipWriter<W>,
    opts: zip::write::SimpleFileOptions,
) -> Result<(), AppError> {
    for entry in std::fs::read_dir(dir).map_err(AppError::Io)? {
        let entry = entry.map_err(AppError::Io)?;
        let path = entry.path();
        if path.is_dir() {
            zip_dir_recursive(&path, root_dir, zip, opts)?;
        } else {
            let rel = path
                .strip_prefix(root_dir.parent().unwrap_or(root_dir))
                .map_err(|e| AppError::Archive(e.to_string()))?;
            let entry_name = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            zip.start_file(&entry_name, opts)
                .map_err(|e| AppError::Archive(e.to_string()))?;
            let bytes = std::fs::read(&path).map_err(AppError::Io)?;
            zip.write_all(&bytes).map_err(AppError::Io)?;
        }
    }
    Ok(())
}

/// Recursively delete all contents of a directory (but keep the directory itself).
async fn clear_dir_contents(dir: &std::path::Path) -> Result<(), AppError> {
    if !dir.exists() {
        return Ok(());
    }
    let mut read_dir = tokio::fs::read_dir(dir).await.map_err(AppError::Io)?;
    while let Some(entry) = read_dir.next_entry().await.map_err(AppError::Io)? {
        let path = entry.path();
        if path.is_dir() {
            tokio::fs::remove_dir_all(&path).await.map_err(|e| {
                AppError::Archive(format!("Eroare ștergere director {}: {e}", path.display()))
            })?;
        } else {
            tokio::fs::remove_file(&path).await.map_err(|e| {
                AppError::Archive(format!("Eroare ștergere fișier {}: {e}", path.display()))
            })?;
        }
    }
    Ok(())
}

// ─── Commands ─────────────────────────────────────────────────────────────────

/// Export ALL user data (DB + archive files) as a ZIP at the given destination path.
///
/// Self-contained implementation: does NOT call `commands::archive::export_backup`
/// to avoid coupling with the concurrently-modified archive module (agent C1).
/// The ZIP contains:
///   - `data.db` — the SQLite database file
///   - `archive/**` — all XML + PDF invoice files
///   - `README.txt` — provenance note
#[tauri::command]
pub async fn export_all_my_data(
    app: AppHandle,
    state: State<'_, AppState>,
    dest_path: String,
) -> AppResult<DataExportResult> {
    let data_dir = app.path().app_data_dir()?;
    let db_path = data_dir.join("data.db");
    let archive_dir = resolve_archive_dir(&state, &app).await?;

    let dest = PathBuf::from(&dest_path);

    // Ensure parent exists
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(AppError::Io)?;
    }

    let dest_clone = dest.clone();
    let db_path_clone = db_path.clone();
    let archive_dir_clone = archive_dir.clone();

    let zip_bytes_written = tauri::async_runtime::spawn_blocking(move || -> Result<u64, AppError> {
        let file = std::fs::File::create(&dest_clone).map_err(AppError::Io)?;
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        // Add data.db
        if db_path_clone.exists() {
            zip.start_file("data.db", opts)
                .map_err(|e| AppError::Archive(e.to_string()))?;
            let db_bytes = std::fs::read(&db_path_clone).map_err(AppError::Io)?;
            zip.write_all(&db_bytes).map_err(AppError::Io)?;
        }

        // Add archive/** preserving relative paths
        if archive_dir_clone.exists() {
            zip_dir_recursive(&archive_dir_clone, &archive_dir_clone, &mut zip, opts)?;
        }

        // README
        zip.start_file("README.txt", opts)
            .map_err(|e| AppError::Archive(e.to_string()))?;
        let readme = format!(
            "Export GDPR — RoFactura\nData: {}\n\nConține:\n- data.db: baza de date SQLite\n- archive/: fișiere XML+PDF facturi\n\nAcest fișier conține toate datele dvs. din aplicație.\n",
            chrono::Utc::now().format("%d.%m.%Y %H:%M UTC")
        );
        zip.write_all(readme.as_bytes()).map_err(AppError::Io)?;

        let inner = zip.finish().map_err(|e| AppError::Archive(e.to_string()))?;
        let len = inner.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(len)
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))??;

    tracing::info!(path = %dest_path, bytes = zip_bytes_written, "GDPR export completed");

    Ok(DataExportResult {
        path: dest_path,
        bytes: zip_bytes_written,
    })
}

/// Irreversibly wipe ALL local data.
///
/// Steps:
/// 0. Enumerate company ids and delete ANAF + SmartBill keychain tokens per company
///    (GDPR erasure — credentials must not survive a wipe even if DB is cleared).
/// 1. Truncate every application table (in a transaction) except `_sqlx_migrations`.
/// 2. Clear the archive directory contents (XML/PDF files).
/// 3. Remove the trial keychain marker so a fresh trial can be started.
///
/// The frontend MUST double-confirm before calling this command.
#[tauri::command]
pub async fn wipe_all_data(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let pool = &state.db;

    // Step 0: enumerate company ids BEFORE truncating the companies table, then
    // delete ANAF OAuth tokens and SmartBill tokens from the OS keychain.
    // Per-entry errors are ignored (best-effort: the credential may not exist).
    let company_ids: Vec<String> = sqlx::query_scalar("SELECT id FROM companies")
        .fetch_all(pool)
        .await
        .unwrap_or_default();

    for company_id in &company_ids {
        // ANAF OAuth token (TokenBundle stored under the "efactura" service).
        crate::anaf::keychain::TokenBundle::delete(company_id);

        // SmartBill API token.
        let _ = crate::anaf::keychain::delete_smartbill_token(company_id);

        tracing::debug!(company_id = %company_id, "GDPR wipe: keychain tokens deleted");
    }

    tracing::info!(
        count = company_ids.len(),
        "GDPR wipe: keychain tokens deleted for all companies"
    );

    // Step 1: discover all application tables (exclude SQLx migration tracking).
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE '_sqlx%' ORDER BY name",
    )
    .fetch_all(pool)
    .await?;

    // Step 2: truncate all tables inside a single transaction.
    // We disable foreign keys for the duration to avoid constraint-order issues.
    let mut tx = pool.begin().await?;

    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *tx)
        .await?;

    for table in &tables {
        // Table names come from sqlite_master — safe to interpolate (no user input).
        let sql = format!("DELETE FROM \"{table}\"");
        sqlx::query(&sql).execute(&mut *tx).await?;
    }

    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    tracing::info!(tables = ?tables, "GDPR wipe: all app tables cleared");

    // Step 3: clear archive directory contents.
    let archive_dir = resolve_archive_dir(&state, &app).await?;
    if archive_dir.exists() {
        if let Err(e) = clear_dir_contents(&archive_dir).await {
            tracing::warn!(error = ?e, "GDPR wipe: archive clear partially failed");
            return Err(e);
        }
    }

    tracing::info!("GDPR wipe: archive directory cleared");

    // Step 4: remove the trial keychain marker so a new trial can be started.
    match Entry::new(TRIAL_KC_SERVICE, TRIAL_KC_ACCOUNT) {
        Ok(entry) => {
            let _ = entry.delete_credential();
            tracing::info!("GDPR wipe: trial keychain marker removed");
        }
        Err(e) => {
            tracing::warn!(error = ?e, "GDPR wipe: could not open keychain entry to delete");
        }
    }

    Ok(())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn clear_dir_contents_removes_files_and_subdirs() {
        let tmp = tempfile::tempdir().unwrap();
        let subdir = tmp.path().join("subdir");
        std::fs::create_dir(&subdir).unwrap();
        std::fs::write(tmp.path().join("file.xml"), b"<x/>").unwrap();
        std::fs::write(subdir.join("nested.pdf"), b"%PDF").unwrap();

        clear_dir_contents(tmp.path()).await.unwrap();

        // Directory itself should still exist but be empty
        assert!(tmp.path().exists());
        let entries: Vec<_> = std::fs::read_dir(tmp.path()).unwrap().collect();
        assert!(
            entries.is_empty(),
            "Expected empty dir after wipe, found: {entries:?}"
        );
    }

    #[tokio::test]
    async fn clear_dir_contents_noop_if_missing() {
        let missing = std::path::Path::new("/tmp/nonexistent_gdpr_test_dir_xyz");
        // Should not error if the directory doesn't exist
        assert!(clear_dir_contents(missing).await.is_ok());
    }

    #[test]
    fn zip_dir_recursive_produces_archive_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let archive_dir = tmp.path().join("archive");
        let sent = archive_dir.join("sent");
        std::fs::create_dir_all(&sent).unwrap();
        std::fs::write(sent.join("INV-001.xml"), b"<Invoice/>").unwrap();

        let buf = std::io::Cursor::new(Vec::new());
        let mut zw = zip::ZipWriter::new(buf);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        zip_dir_recursive(&archive_dir, &archive_dir, &mut zw, opts).unwrap();
        let inner = zw.finish().unwrap().into_inner();

        let cursor = std::io::Cursor::new(inner);
        let mut za = zip::ZipArchive::new(cursor).unwrap();
        let names: Vec<_> = (0..za.len())
            .map(|i| za.by_index(i).unwrap().name().to_string())
            .collect();

        assert!(
            names.iter().any(|n| n == "archive/sent/INV-001.xml"),
            "Expected 'archive/sent/INV-001.xml', got: {names:?}"
        );
    }
}
