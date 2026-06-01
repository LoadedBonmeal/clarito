//! Archive: export invoice ZIP + import XML invoice.

use std::io::{Read, Write};
use tauri::{AppHandle, Manager, State};

use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Export toate facturile cu XML+PDF pentru o companie într-un ZIP.
/// Returnează path-ul ZIP generat.
#[tauri::command]
pub async fn export_invoices_zip(
    state: State<'_, AppState>,
    app: AppHandle,
    company_id: String,
) -> AppResult<String> {
    let pool = &state.db;

    // Get all invoices for company
    let filter = crate::db::invoices::InvoiceFilter {
        company_id: Some(company_id.clone()),
        ..Default::default()
    };
    let result = crate::db::invoices::list(pool, filter).await?;

    // Collect all (name, bytes) pairs we need — read files concurrently then
    // build the ZIP in a spawn_blocking so we don't block the Tokio worker.
    struct FileEntry {
        name: String,
        bytes: Vec<u8>,
    }
    let mut entries: Vec<FileEntry> = Vec::new();
    for invoice in &result.items {
        if let Some(xml_path) = &invoice.xml_path {
            if let Ok(bytes) = tokio::fs::read(xml_path).await {
                entries.push(FileEntry {
                    name: format!("{}.xml", invoice.full_number.replace('/', "_")),
                    bytes,
                });
            }
        }
        if let Some(pdf_path) = &invoice.pdf_path {
            if let Ok(bytes) = tokio::fs::read(pdf_path).await {
                entries.push(FileEntry {
                    name: format!("{}.pdf", invoice.full_number.replace('/', "_")),
                    bytes,
                });
            }
        }
    }

    let zip_bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, AppError> {
        let buf = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(buf);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for entry in entries {
            zip.start_file(&entry.name, options)
                .map_err(|e| AppError::Other(e.to_string()))?;
            zip.write_all(&entry.bytes)
                .map_err(|e| AppError::Other(e.to_string()))?;
        }
        let cursor = zip.finish().map_err(|e| AppError::Other(e.to_string()))?;
        Ok(cursor.into_inner())
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))??;

    // Save to app_data_dir
    let out_dir = app.path().app_data_dir()?;
    let zip_path = out_dir.join(format!(
        "export_{}.zip",
        chrono::Utc::now().format("%Y%m%d_%H%M%S")
    ));
    tokio::fs::write(&zip_path, &zip_bytes)
        .await
        .map_err(AppError::Io)?;

    Ok(zip_path.display().to_string())
}

/// Exportă un backup complet (DB + archive/**) într-un fișier ZIP.
/// Returnează path-ul fișierului ZIP generat.
#[tauri::command]
pub async fn export_backup(state: State<'_, AppState>, app: AppHandle) -> AppResult<String> {
    let data_dir = app.path().app_data_dir()?;
    let db_path = data_dir.join("data.db");

    // Resolve archive directory: prefer user-configured override, fall back to
    // <app_data>/archive.
    let archive_dir = {
        let override_val =
            crate::db::settings::get(&state.db, crate::db::settings::keys::ARCHIVE_PATH_OVERRIDE)
                .await
                .unwrap_or(None);
        match override_val {
            Some(p) if !p.is_empty() => std::path::PathBuf::from(p),
            _ => data_dir.join("archive"),
        }
    };

    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let out_path = data_dir.join(format!("efactura_backup_{timestamp}.zip"));
    let out_path_clone = out_path.clone();

    let result = tauri::async_runtime::spawn_blocking(move || -> Result<String, AppError> {
        let file = std::fs::File::create(&out_path_clone).map_err(AppError::Io)?;
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        // Add data.db
        if db_path.exists() {
            zip.start_file("data.db", opts)
                .map_err(|e| AppError::Other(e.to_string()))?;
            let db_bytes = std::fs::read(&db_path).map_err(AppError::Io)?;
            zip.write_all(&db_bytes).map_err(AppError::Io)?;
        }

        // Add archive/**  (all XML + PDF files under the archive directory),
        // preserving relative paths so restore can recreate the same layout.
        if archive_dir.exists() {
            zip_dir_recursive(&archive_dir, &archive_dir, &mut zip, opts)?;
        }

        // Add README
        zip.start_file("README.txt", opts)
            .map_err(|e| AppError::Other(e.to_string()))?;
        let readme = format!(
            "Backup eFactura Desktop\r\nData: {}\r\n\r\nConține:\r\n- data.db: baza de date SQLite\r\n- archive/: fișiere XML+PDF facturi\r\n\r\nRestaurare: folosiți funcția Import Backup din aplicație.\r\n",
            chrono::Utc::now().format("%d.%m.%Y %H:%M UTC")
        );
        zip.write_all(readme.as_bytes()).map_err(AppError::Io)?;

        zip.finish().map_err(|e| AppError::Other(e.to_string()))?;

        Ok(out_path_clone.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))??;

    Ok(result)
}

/// Recursively add all files under `dir` into the ZIP archive, using paths
/// relative to `root_dir` (so the ZIP entry is e.g. `archive/sent/2026/INV.xml`).
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
            // Build a relative path anchored at root_dir's *parent* so the
            // archive prefix is preserved (e.g. root_dir = …/archive ⇒
            // entry = "archive/sent/INV.xml").
            let rel = path
                .strip_prefix(root_dir.parent().unwrap_or(root_dir))
                .map_err(|e| AppError::Other(e.to_string()))?;
            // Normalise to forward-slash for cross-platform ZIP entries.
            let entry_name = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            zip.start_file(&entry_name, opts)
                .map_err(|e| AppError::Other(e.to_string()))?;
            let bytes = std::fs::read(&path).map_err(AppError::Io)?;
            zip.write_all(&bytes).map_err(AppError::Io)?;
        }
    }
    Ok(())
}

/// Importă un backup ZIP, înlocuind DB-ul curent și restaurând fișierele archive.
/// Aplicația se repornește automat după import.
#[tauri::command]
pub async fn import_backup(app: AppHandle, path: String) -> AppResult<()> {
    // 1+2. Deschide ZIP-ul și extrage data.db în memorie (zip crate este sync).
    let path_clone = path.clone();
    let buf = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, AppError> {
        let file = std::fs::File::open(&path_clone).map_err(AppError::Io)?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| AppError::Other(e.to_string()))?;

        // Verifică că ZIP-ul conține data.db
        let has_db = (0..archive.len()).any(|i| {
            archive
                .by_index(i)
                .map(|f| f.name() == "data.db")
                .unwrap_or(false)
        });
        if !has_db {
            return Err(AppError::Other("ZIP invalid: lipsește data.db".to_string()));
        }

        // Extrage conținutul data.db din ZIP în memorie (necesar și pentru verificare)
        let mut db_entry = archive
            .by_name("data.db")
            .map_err(|e| AppError::Other(e.to_string()))?;
        let mut b = Vec::new();
        db_entry.read_to_end(&mut b).map_err(AppError::Io)?;
        Ok(b)
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))??;

    // 2b. Validează integritatea SQLite a backup-ului înainte de a suprascrie DB-ul curent
    let temp_check_path = {
        let data_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| AppError::Other(e.to_string()))?;
        data_dir.join("data_restore_check.db")
    };
    tokio::fs::write(&temp_check_path, &buf)
        .await
        .map_err(AppError::Io)?;

    {
        let check_url = format!("sqlite:{}?mode=ro", temp_check_path.to_string_lossy());
        let check_pool = sqlx::SqlitePool::connect(&check_url).await.map_err(|_| {
            AppError::Other("Backup invalid: DB-ul nu poate fi deschis.".to_string())
        })?;

        let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_one(&check_pool)
            .await
            .unwrap_or_else(|_| "error".to_string());

        if integrity != "ok" {
            check_pool.close().await;
            let _ = tokio::fs::remove_file(&temp_check_path).await;
            return Err(AppError::Other(format!(
                "Backup corupt: PRAGMA integrity_check a returnat: {integrity}"
            )));
        }

        // SEC-06: schema validation — verifică tabelele și coloanele cheie pentru
        // a respinge DB-uri SQLite valide dar cu altă schemă (potențial crafted).
        let required_tables = [
            "companies",
            "invoices",
            "invoice_line_items",
            "contacts",
            "settings",
            "received_invoices",
        ];

        for table in required_tables.iter() {
            let exists: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            )
            .bind(table)
            .fetch_one(&check_pool)
            .await
            .unwrap_or(0);

            if exists == 0 {
                check_pool.close().await;
                let _ = tokio::fs::remove_file(&temp_check_path).await;
                return Err(AppError::Other(format!(
                    "Backup invalid: tabelul '{table}' lipsește. Acest fișier nu pare să fie un backup Clarito valid."
                )));
            }
        }

        // Validează că `invoices` are coloanele cheie pe care le folosește aplicația.
        let invoice_cols: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info('invoices')")
                .fetch_all(&check_pool)
                .await
                .unwrap_or_default();

        let required_cols = [
            "id",
            "company_id",
            "series",
            "number",
            "status",
            "issue_date",
        ];
        for col in required_cols.iter() {
            if !invoice_cols.iter().any(|c| c == col) {
                check_pool.close().await;
                let _ = tokio::fs::remove_file(&temp_check_path).await;
                return Err(AppError::Other(format!(
                    "Backup invalid: coloana '{col}' lipsește din tabelul invoices."
                )));
            }
        }

        check_pool.close().await;
    }
    let _ = tokio::fs::remove_file(&temp_check_path).await;

    // 3. Determină calea curentă a DB
    let db_path = crate::db::pool::resolve_db_path(&app)?;

    // 4. Backup DB curent
    let db_bak = db_path.with_extension("db.bak");
    tokio::fs::copy(&db_path, &db_bak)
        .await
        .map_err(AppError::Io)?;

    // 5. Scrie data.db extrasă din ZIP la calea DB-ului
    tokio::fs::write(&db_path, &buf)
        .await
        .map_err(AppError::Io)?;

    // 6. Restaurează fișierele archive/* din ZIP (dacă există), cu protecție
    //    zip-slip: orice intrare al cărei path normalizat iese din archive_dir
    //    este respinsă (consistent cu SEC-06 de mai sus).
    //    Respectăm ARCHIVE_PATH_OVERRIDE (aceeași logică ca în export_backup),
    //    astfel restaurarea aterizează în același director ca exportul.
    let archive_dir = {
        let data_dir = app.path().app_data_dir()?;
        // Read ARCHIVE_PATH_OVERRIDE from the backup DB (already written to
        // db_path at step 5) so restore targets the same directory the user had
        // configured when the backup was created.  Falls back to
        // <app_data>/archive if the setting is absent or the pool fails.
        let override_val: Option<String> = {
            let db_url = format!("sqlite:{}?mode=ro", db_path.to_string_lossy());
            if let Ok(pool) = sqlx::SqlitePool::connect(&db_url).await {
                let val = sqlx::query_scalar::<_, String>(
                    "SELECT value FROM settings \
                     WHERE key = 'archive_path_override' LIMIT 1",
                )
                .fetch_optional(&pool)
                .await
                .ok()
                .flatten();
                pool.close().await;
                val
            } else {
                None
            }
        };

        match override_val {
            Some(p) if !p.is_empty() => std::path::PathBuf::from(p),
            _ => data_dir.join("archive"),
        }
    };
    // Clone archive_dir before moving it into spawn_blocking; the original is
    // needed again after the closure for the xml_path rewrite step (fix 3).
    let archive_dir_for_rewrite = archive_dir.clone();
    // Re-open and extract archive entries in spawn_blocking (zip crate is sync).
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let file2 = std::fs::File::open(&path).map_err(AppError::Io)?;
        let mut archive2 =
            zip::ZipArchive::new(file2).map_err(|e| AppError::Other(e.to_string()))?;

        for i in 0..archive2.len() {
            let mut entry = archive2
                .by_index(i)
                .map_err(|e| AppError::Other(e.to_string()))?;

            let raw_name = entry.name().to_string();

            // Only process entries that start with "archive/" (skip data.db / README).
            if !raw_name.starts_with("archive/") {
                continue;
            }

            // SEC-ZIP-01: zip-slip guard — build absolute target path and verify it
            // stays inside archive_dir after normalisation.
            let target = archive_dir.join(&raw_name["archive/".len()..]);
            let canonical_archive = archive_dir
                .canonicalize()
                .unwrap_or_else(|_| archive_dir.clone());
            // Use clean path normalisation without requiring the file to exist yet.
            let normalised = normalise_path(&target);
            if !normalised.starts_with(&canonical_archive) {
                return Err(AppError::Other(format!(
                    "Backup invalid: intrarea '{raw_name}' iese din directorul arhivă (zip-slip)."
                )));
            }

            if entry.is_dir() {
                std::fs::create_dir_all(&target).map_err(AppError::Io)?;
            } else {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent).map_err(AppError::Io)?;
                }
                let mut out = std::fs::File::create(&target).map_err(AppError::Io)?;
                std::io::copy(&mut entry, &mut out).map_err(AppError::Io)?;
            }
        }
        Ok(())
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))??;

    // 7. Rewrite absolute xml_path / pdf_path stored in the restored DB so they
    //    point to the current machine's archive_dir rather than the source machine's.
    //
    //    Archive files live under <archive_dir>/sent/... and <archive_dir>/received/...
    //    The stored paths on the source machine had a different <archive_dir> prefix.
    //    Strategy: for each non-null path, find the last occurrence of "/sent/" or
    //    "/received/" in the stored value — everything from that separator onward is
    //    machine-independent (relative within the archive).  Prepend the local
    //    archive_dir to obtain the correct absolute path on this machine.
    //
    //    Rows whose stored path does NOT contain "/sent/" or "/received/" are left
    //    untouched (defensive: skip rather than corrupt).
    {
        let archive_dir_str = archive_dir_for_rewrite.to_string_lossy().to_string();
        // Open the restored DB directly (AppState still holds the old pool — the
        // app is about to restart, so we open a short-lived pool here).
        let db_url = format!("sqlite:{}", db_path.to_string_lossy());
        if let Ok(pool) = sqlx::SqlitePool::connect(&db_url).await {
            for table in &["invoices", "received_invoices"] {
                for col in &["xml_path", "pdf_path"] {
                    // Rewrite paths that contain "/sent/" — update only the prefix
                    // up to that separator, keeping the rest intact.
                    let sql_sent = format!(
                        "UPDATE \"{table}\" \
                         SET \"{col}\" = ?1 || substr(\"{col}\", instr(\"{col}\", '/sent/')) \
                         WHERE \"{col}\" IS NOT NULL \
                           AND instr(\"{col}\", '/sent/') > 0"
                    );
                    if let Err(e) = sqlx::query(&sql_sent)
                        .bind(&archive_dir_str)
                        .execute(&pool)
                        .await
                    {
                        tracing::warn!(
                            table, col, error = ?e,
                            "import_backup: path rewrite (/sent/) failed, continuing"
                        );
                    }

                    // Rewrite paths that contain "/received/" analogously.
                    let sql_received = format!(
                        "UPDATE \"{table}\" \
                         SET \"{col}\" = ?1 || substr(\"{col}\", instr(\"{col}\", '/received/')) \
                         WHERE \"{col}\" IS NOT NULL \
                           AND instr(\"{col}\", '/received/') > 0"
                    );
                    if let Err(e) = sqlx::query(&sql_received)
                        .bind(&archive_dir_str)
                        .execute(&pool)
                        .await
                    {
                        tracing::warn!(
                            table, col, error = ?e,
                            "import_backup: path rewrite (/received/) failed, continuing"
                        );
                    }
                }
            }
            pool.close().await;
            tracing::info!("import_backup: xml_path/pdf_path rewritten to local archive root");
        } else {
            tracing::warn!("import_backup: could not open restored DB for path rewrite — skipping");
        }
    }

    // 8. Repornește aplicația
    app.request_restart();
    #[allow(unreachable_code)]
    Ok(())
}

/// Normalise a path (resolve `.` and `..` components) without hitting the
/// filesystem.  Returns an absolute path; if the input is relative it is
/// kept relative (this helper is only called with absolute paths in practice).
fn normalise_path(path: &std::path::Path) -> std::path::PathBuf {
    use std::path::Component;
    let mut out = std::path::PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

// ─── Integrity check ───────────────────────────────────────────────────────

/// Report returned by [`verify_archive_integrity`].
///
/// - `checked`: total number of invoice XML paths examined.
/// - `missing`: list of paths that were recorded in the DB but absent on disk.
/// - `ok`: `true` iff `missing` is empty.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveIntegrityReport {
    pub checked: usize,
    pub missing: Vec<String>,
    pub ok: bool,
}

/// Verifică că toate fișierele XML referențiate în DB există pe disc.
/// Verifică atât facturile emise (`invoices`) cât și cele primite
/// (`received_invoices`) pentru o acoperire completă.
/// Returnează un raport structurat; apelat din frontend ca `verify_archive_integrity`.
#[tauri::command]
pub async fn verify_archive_integrity(
    state: State<'_, AppState>,
) -> AppResult<ArchiveIntegrityReport> {
    use sqlx::Row;

    // Check sent invoices (xml_path may be NULL for drafts not yet submitted).
    let sent_rows = sqlx::query("SELECT xml_path FROM invoices WHERE xml_path IS NOT NULL")
        .fetch_all(&state.db)
        .await?;

    // Check received invoices (xml_path is always populated for received invoices).
    let received_rows =
        sqlx::query("SELECT xml_path FROM received_invoices WHERE xml_path IS NOT NULL")
            .fetch_all(&state.db)
            .await?;

    let mut checked: usize = 0;
    let mut missing: Vec<String> = Vec::new();

    for row in sent_rows.iter().chain(received_rows.iter()) {
        let xml_path: String = row.try_get("xml_path").map_err(AppError::Database)?;
        checked += 1;
        if !std::path::Path::new(&xml_path).exists() {
            missing.push(xml_path);
        }
    }

    let ok = missing.is_empty();
    Ok(ArchiveIntegrityReport {
        checked,
        missing,
        ok,
    })
}

// ─── Archive size ──────────────────────────────────────────────────────────

fn dir_size(path: &std::path::Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries.flatten().fold(0u64, |acc, e| {
        let p = e.path();
        if p.is_dir() {
            acc + dir_size(&p)
        } else {
            acc + e.metadata().map(|m| m.len()).unwrap_or(0)
        }
    })
}

/// Returnează dimensiunea totală a directorului archive în bytes.
/// Respectă setarea ARCHIVE_PATH_OVERRIDE dacă a fost configurată.
#[tauri::command]
pub async fn get_archive_size(state: State<'_, AppState>, app: AppHandle) -> AppResult<u64> {
    let data_dir = app.path().app_data_dir()?;
    let archive_dir = {
        let override_val =
            crate::db::settings::get(&state.db, crate::db::settings::keys::ARCHIVE_PATH_OVERRIDE)
                .await
                .unwrap_or(None);
        match override_val {
            Some(p) if !p.is_empty() => std::path::PathBuf::from(p),
            _ => data_dir.join("archive"),
        }
    };
    if !archive_dir.exists() {
        return Ok(0);
    }
    tokio::task::spawn_blocking(move || dir_size(&archive_dir))
        .await
        .map_err(|e| AppError::Other(e.to_string()))
}

/// Schimbă locația arhivei: copiază fișierele existente în noul path și salvează setarea.
#[tauri::command]
pub async fn change_archive_location(
    state: State<'_, AppState>,
    new_path: String,
    app: AppHandle,
) -> AppResult<()> {
    use std::path::PathBuf;

    let new_path_buf = PathBuf::from(&new_path);

    // SEC-02: validează calea înainte de a o folosi.
    // 1. Trebuie să fie absolută.
    if !new_path_buf.is_absolute() {
        return Err(AppError::Validation(
            "Calea trebuie să fie absolută.".into(),
        ));
    }

    // 2. Refuză UNC paths / network shares.
    let path_str = new_path_buf.to_string_lossy();
    if path_str.starts_with(r"\\") || path_str.starts_with("//") {
        return Err(AppError::Validation(
            "Locațiile de rețea (UNC/SMB) nu sunt permise.".into(),
        ));
    }

    // 3. Refuză path-uri ce conțin componente `..`.
    if new_path_buf
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(AppError::Validation(
            "Calea nu poate conține componente '..'.".into(),
        ));
    }

    // 4. Părintele trebuie să existe și să fie în $HOME.
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| AppError::Other("Nu pot determina directorul home.".into()))?;
    let home_canon = PathBuf::from(&home).canonicalize().map_err(AppError::Io)?;

    let parent = new_path_buf
        .parent()
        .ok_or_else(|| AppError::Validation("Cale invalidă.".into()))?;
    let parent_canon = parent
        .canonicalize()
        .map_err(|_| AppError::Validation("Calea părinte nu există.".into()))?;

    if !parent_canon.starts_with(&home_canon) {
        return Err(AppError::Validation(
            "Calea trebuie să fie în directorul home al utilizatorului.".into(),
        ));
    }

    tokio::fs::create_dir_all(&new_path_buf)
        .await
        .map_err(|e| AppError::Other(e.to_string()))?;

    // Get current archive path
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Other(e.to_string()))?;
    let current_archive = app_data.join("archive");

    // Copy existing files if directory exists (sync recursive walk — spawn_blocking).
    if current_archive.exists() {
        let new_path_buf_clone = new_path_buf.clone();
        tokio::task::spawn_blocking(move || {
            copy_dir_recursive(&current_archive, &new_path_buf_clone)
        })
        .await
        .map_err(|e| AppError::Other(e.to_string()))?
        .map_err(|e| AppError::Other(format!("Copiere arhivă eșuată: {}", e)))?;
    }

    // Save new path in settings using the canonical key that all readers use.
    crate::db::settings::set(
        &state.db,
        crate::db::settings::keys::ARCHIVE_PATH_OVERRIDE,
        &new_path,
    )
    .await?;

    Ok(())
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── normalise_path ───────────────────────────────────────────────────────
    #[test]
    fn normalise_path_removes_dot_dot() {
        let p = std::path::Path::new("/tmp/archive/../../../etc/passwd");
        let norm = normalise_path(p);
        assert_eq!(norm, std::path::Path::new("/etc/passwd"));
    }

    #[test]
    fn normalise_path_keeps_valid_path() {
        let p = std::path::Path::new("/tmp/archive/sent/2026/INV-0001.xml");
        let norm = normalise_path(p);
        assert_eq!(norm, p);
    }

    // ── backup_includes_archive_files ────────────────────────────────────────
    /// Create a temp archive directory with a fake XML file, run
    /// zip_dir_recursive, and assert the ZIP contains the entry.
    #[test]
    fn backup_includes_archive_files() {
        let tmp = tempfile::tempdir().unwrap();
        // Simulate <app_data>/archive/sent/2026/INV-0001.xml
        let sent_dir = tmp.path().join("archive").join("sent").join("2026");
        std::fs::create_dir_all(&sent_dir).unwrap();
        let xml_path = sent_dir.join("INV-0001.xml");
        std::fs::write(&xml_path, b"<Invoice/>").unwrap();

        let zip_file_path = tmp.path().join("backup.zip");
        let file = std::fs::File::create(&zip_file_path).unwrap();
        let mut zw = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        let archive_dir = tmp.path().join("archive");
        zip_dir_recursive(&archive_dir, &archive_dir, &mut zw, opts).unwrap();
        zw.finish().unwrap();

        // Re-open and check entry names
        let zip_file = std::fs::File::open(&zip_file_path).unwrap();
        let mut za = zip::ZipArchive::new(zip_file).unwrap();
        let names: Vec<String> = (0..za.len())
            .map(|i| za.by_index(i).unwrap().name().to_string())
            .collect();

        assert!(
            names
                .iter()
                .any(|n| n.ends_with("INV-0001.xml") && n.contains("archive")),
            "Expected archive/sent/2026/INV-0001.xml in ZIP, got: {names:?}"
        );
    }

    // ── restore_rejects_zip_slip ─────────────────────────────────────────────
    /// A ZIP entry whose path traverses `..` must be rejected by normalise_path
    /// before extraction so it can't escape the archive directory.
    #[test]
    fn restore_rejects_zip_slip() {
        // Craft a malicious entry name.
        let malicious = "archive/../../../etc/passwd";
        let target_base = std::path::Path::new("/tmp/safe_archive_dir");

        // Simulate the zip-slip check performed in import_backup.
        let raw_suffix = &malicious["archive/".len()..]; // "../../../etc/passwd"
        let target = target_base.join(raw_suffix);
        let normalised = normalise_path(&target);

        // Normalised path must NOT start with target_base → we would reject it.
        assert!(
            !normalised.starts_with(target_base),
            "zip-slip guard should have rejected this path, normalised = {normalised:?}"
        );
    }

    // ── archive_path_override key is consistent ──────────────────────────────
    /// Verify that ARCHIVE_PATH_OVERRIDE constant value matches the string
    /// hardcoded in import_backup (the inline override-read in that function
    /// must use the same literal key).
    #[test]
    fn archive_path_override_key_is_canonical() {
        assert_eq!(
            crate::db::settings::keys::ARCHIVE_PATH_OVERRIDE,
            "archive_path_override",
            "ARCHIVE_PATH_OVERRIDE constant must equal the literal used in import_backup"
        );
    }

    // ── xml_path prefix rewrite produces expected new path ───────────────────
    /// Simulate the SQL rewrite logic: given a stored path from machine A and
    /// the local archive root from machine B, compute the expected rewritten path.
    #[test]
    fn xml_path_prefix_rewrite_sent() {
        // Source machine path
        let stored = "/Users/alice/Library/Application Support/ro.lucaris.efactura/archive/sent/2026/INV-0001.xml";
        // Local archive dir on target machine
        let local_archive = "/Users/bob/Library/Application Support/ro.lucaris.efactura/archive";

        // Replicate the SQL logic: find "/sent/" and prepend the local root.
        let separator = "/sent/";
        let pos = stored.find(separator).expect("should contain /sent/");
        let suffix = &stored[pos..]; // includes "/sent/"
        let rewritten = format!("{local_archive}{suffix}");

        assert_eq!(
            rewritten,
            "/Users/bob/Library/Application Support/ro.lucaris.efactura/archive/sent/2026/INV-0001.xml"
        );
    }

    #[test]
    fn xml_path_prefix_rewrite_received() {
        let stored = "/old/archive/received/RO123/2026/msg-abc/invoice.xml";
        let local_archive = "/new/archive";

        let separator = "/received/";
        let pos = stored.find(separator).expect("should contain /received/");
        let suffix = &stored[pos..];
        let rewritten = format!("{local_archive}{suffix}");

        assert_eq!(
            rewritten,
            "/new/archive/received/RO123/2026/msg-abc/invoice.xml"
        );
    }

    #[test]
    fn xml_path_rewrite_skips_unrecognised_paths() {
        // A path that has neither /sent/ nor /received/ should not be modified
        // (the SQL WHERE clause guards against this).
        let stored = "/some/custom/path/invoice.xml";
        let has_sent = stored.contains("/sent/");
        let has_received = stored.contains("/received/");
        assert!(
            !has_sent && !has_received,
            "path should not match any rewrite separator"
        );
    }

    // ── zip_dir_recursive skips root correctly ───────────────────────────────
    #[test]
    fn zip_dir_recursive_produces_archive_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let archive_dir = tmp.path().join("archive");
        let sub = archive_dir.join("sent");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("test.xml"), b"<x/>").unwrap();

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
            names.iter().any(|n| n == "archive/sent/test.xml"),
            "expected 'archive/sent/test.xml', got: {names:?}"
        );
    }
}
