//! Archive: export invoice ZIP + import XML invoice.

use std::io::{Read, Write};
use tauri::{AppHandle, Manager, State};

use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Sanitize a string for safe use as a ZIP entry filename (defense-in-depth
/// against zip-slip via a crafted invoice series). Allows only `[A-Za-z0-9._-]`;
/// every other character (path separators, `..` etc.) becomes `_`.
fn sanitize_zip_name(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.trim_matches('.').is_empty() {
        "file".to_string()
    } else {
        cleaned
    }
}

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
                    name: format!("{}.xml", sanitize_zip_name(&invoice.full_number)),
                    bytes,
                });
            }
        }
        if let Some(pdf_path) = &invoice.pdf_path {
            if let Ok(bytes) = tokio::fs::read(pdf_path).await {
                entries.push(FileEntry {
                    name: format!("{}.pdf", sanitize_zip_name(&invoice.full_number)),
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
///
/// Backup consistency: instead of reading `data.db` directly via `std::fs::read`
/// (which may capture a torn WAL), we use SQLite's `VACUUM INTO` command to produce
/// a clean, WAL-consolidated single-file snapshot at a temporary path, then include
/// that snapshot in the ZIP. The temp file is removed after the ZIP is written.
#[tauri::command]
pub async fn export_backup(
    state: State<'_, AppState>,
    app: AppHandle,
    dest_path: Option<String>,
) -> AppResult<String> {
    let data_dir = app.path().app_data_dir()?;

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

    // Step 1: produce a consistent DB snapshot via VACUUM INTO.
    // VACUUM INTO writes a single-file SQLite copy with WAL fully applied,
    // safe to read without the pool's WAL being active.
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let snapshot_path = data_dir.join(format!("efactura_snapshot_{timestamp}.db"));
    let snapshot_path_str = snapshot_path.to_string_lossy().to_string();

    sqlx::query(&format!(
        "VACUUM INTO '{}'",
        snapshot_path_str.replace('\'', "''")
    ))
    .execute(&state.db)
    .await
    .map_err(|e| AppError::Other(format!("VACUUM INTO failed: {e}")))?;

    // Use caller-supplied path when provided (validated: absolute, no '..', no UNC, .zip only —
    // the IPC endpoint is callable with an arbitrary string, so never trust it raw); otherwise
    // fall back to app_data_dir.
    let out_path = if let Some(ref p) = dest_path {
        crate::commands::integrations::validate_export_path(p)?
    } else {
        data_dir.join(format!("efactura_backup_{timestamp}.zip"))
    };
    let out_path_clone = out_path.clone();
    let snapshot_path_clone = snapshot_path.clone();

    let result = tauri::async_runtime::spawn_blocking(move || -> Result<String, AppError> {
        let file = std::fs::File::create(&out_path_clone).map_err(AppError::Io)?;
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        // Add data.db from the consistent VACUUM INTO snapshot.
        if snapshot_path_clone.exists() {
            zip.start_file("data.db", opts)
                .map_err(|e| AppError::Other(e.to_string()))?;
            let db_bytes = std::fs::read(&snapshot_path_clone).map_err(AppError::Io)?;
            zip.write_all(&db_bytes).map_err(AppError::Io)?;
        }

        // Add archive/**  (all XML + PDF files under the archive directory),
        // preserving relative paths so restore can recreate the same layout.
        if archive_dir.exists() {
            zip_dir_recursive(&archive_dir, &archive_dir, &mut zip, opts)?;
        }

        // Add invoices/**  (manually-generated XML+PDF under app_data/invoices/).
        // ubl/paths.rs writes to {app_data}/invoices/{company_id}/{invoice_id}.{xml,pdf}.
        let invoices_dir = data_dir.join("invoices");
        if invoices_dir.exists() {
            zip_dir_recursive(&invoices_dir, &invoices_dir, &mut zip, opts)?;
        }

        // Add receipts/**  (receipt PDFs under app_data/receipts/).
        // commands/receipts.rs writes to {app_data}/receipts/{company_id}/{receipt_id}.pdf.
        let receipts_dir = data_dir.join("receipts");
        if receipts_dir.exists() {
            zip_dir_recursive(&receipts_dir, &receipts_dir, &mut zip, opts)?;
        }

        // Add README
        zip.start_file("README.txt", opts)
            .map_err(|e| AppError::Other(e.to_string()))?;
        let readme = format!(
            "Backup eFactura Desktop\r\nData: {}\r\n\r\nConține:\r\n- data.db: baza de date SQLite\r\n- archive/: fișiere XML+PDF facturi (recepționate ANAF)\r\n- invoices/: fișiere XML+PDF facturi emise manual\r\n- receipts/: chitanțe PDF\r\n\r\nRestaurare: folosiți funcția Import Backup din aplicație.\r\n",
            chrono::Utc::now().format("%d.%m.%Y %H:%M UTC")
        );
        zip.write_all(readme.as_bytes()).map_err(AppError::Io)?;

        zip.finish().map_err(|e| AppError::Other(e.to_string()))?;

        Ok(out_path_clone.to_string_lossy().to_string())
    })
    .await;

    // Clean up the temporary VACUUM INTO snapshot UNCONDITIONALLY (best-effort),
    // even on the error path, so timestamped temp snapshots don't accumulate.
    let _ = tokio::fs::remove_file(&snapshot_path).await;

    let out_path = result.map_err(|e| AppError::Other(e.to_string()))??;
    Ok(out_path)
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
        // Use SqliteConnectOptions::new().filename() so the path is never
        // parsed as a URL — prevents query-param injection via a crafted path
        // containing '?' (e.g. `?mode=...`).
        let check_opts = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&temp_check_path)
            .read_only(true);
        let check_pool = sqlx::SqlitePool::connect_with(check_opts)
            .await
            .map_err(|_| {
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
            // Use SqliteConnectOptions to avoid URL injection via path characters.
            let db_opts = sqlx::sqlite::SqliteConnectOptions::new()
                .filename(&db_path)
                .read_only(true);
            if let Ok(pool) = sqlx::SqlitePool::connect_with(db_opts).await {
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
    // app_data_dir for restoring invoices/ and receipts/ entries.
    let data_dir_for_restore = app.path().app_data_dir()?;
    let data_dir_for_rewrite = data_dir_for_restore.clone();
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

            // Determine which top-level prefix this entry belongs to.
            // We handle: archive/, invoices/, receipts/
            // All other entries (data.db, README.txt, …) are skipped.
            enum EntryKind<'a> {
                Archive(&'a std::path::Path), // target root = archive_dir
                AppData(&'a std::path::Path), // target root = data_dir_for_restore
            }
            let kind = if raw_name.starts_with("archive/") {
                EntryKind::Archive(&archive_dir)
            } else if raw_name.starts_with("invoices/") || raw_name.starts_with("receipts/") {
                EntryKind::AppData(&data_dir_for_restore)
            } else {
                continue;
            };

            let (guard_root, suffix) = match kind {
                EntryKind::Archive(root) => (root, &raw_name["archive/".len()..]),
                EntryKind::AppData(root) => {
                    // For invoices/ and receipts/ the ZIP prefix IS the subdir name,
                    // so we strip nothing — the full raw_name is the relative path
                    // under data_dir (e.g. "invoices/comp-id/INV.xml").
                    (root, raw_name.as_str())
                }
            };

            // SEC-ZIP-01: zip-slip guard — build absolute target path and verify it
            // stays inside the expected root after normalisation.
            //
            let target = guard_root.join(suffix);
            if !target_is_inside_root(guard_root, &target) {
                return Err(AppError::Other(format!(
                    "Backup invalid: intrarea '{raw_name}' iese din directorul destinație (zip-slip)."
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
    //    point to the current machine's paths rather than the source machine's.
    //
    //    Three classes of stored paths, each identified by a marker segment:
    //
    //    a) Archive files (ANAF received): …/<archive_dir>/sent/…  or  …/received/…
    //       Marker: "/sent/" or "/received/".  New root = local archive_dir.
    //
    //    b) Manually-generated invoices: …/invoices/<company_id>/<id>.{xml,pdf}
    //       Marker: "/invoices/".  New root = local app_data_dir (data_dir_for_rewrite).
    //       (ubl/paths.rs writes {app_data}/invoices/{company_id}/{invoice_id}.ext)
    //
    //    c) Receipt PDFs: …/receipts/<company_id>/<id>.pdf
    //       Marker: "/receipts/".  New root = local app_data_dir.
    //       (commands/receipts.rs writes {app_data}/receipts/{company_id}/{receipt_id}.pdf)
    //
    //    Rows whose stored path matches none of the markers are left untouched
    //    (defensive: skip rather than corrupt).
    //
    //    All rewrites are separator-agnostic (replace('\','/')+instr) — same
    //    XW-1 pattern used for /sent/ and /received/.
    {
        let archive_dir_str = archive_dir_for_rewrite.to_string_lossy().to_string();
        let data_dir_str = data_dir_for_rewrite.to_string_lossy().to_string();
        // Open the restored DB directly (AppState still holds the old pool — the
        // app is about to restart, so we open a short-lived pool here).
        // Use SqliteConnectOptions to avoid URL injection via path characters.
        let db_opts = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(false);
        if let Ok(pool) = sqlx::SqlitePool::connect_with(db_opts).await {
            for table in &["invoices", "received_invoices"] {
                for col in &["xml_path", "pdf_path"] {
                    // (a) Rewrite paths that contain "/sent/" — archive files (ANAF received).
                    // Separator-agnostic: normalise the stored column to forward
                    // slashes before locating the marker so that paths stored with
                    // Windows backslashes (e.g. C:\...\sent\INV.xml) are matched
                    // correctly.  The rewritten path uses forward slashes
                    // throughout — these are valid path separators on Windows for
                    // Rust/Tauri file I/O (P0 Windows fix).
                    let sql_sent = format!(
                        "UPDATE \"{table}\" \
                         SET \"{col}\" = ?1 || substr(replace(\"{col}\", '\\', '/'), \
                                                       instr(replace(\"{col}\", '\\', '/'), '/sent/')) \
                         WHERE \"{col}\" IS NOT NULL \
                           AND instr(replace(\"{col}\", '\\', '/'), '/sent/') > 0"
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

                    // (a) Rewrite paths that contain "/received/" analogously.
                    let sql_received = format!(
                        "UPDATE \"{table}\" \
                         SET \"{col}\" = ?1 || substr(replace(\"{col}\", '\\', '/'), \
                                                       instr(replace(\"{col}\", '\\', '/'), '/received/')) \
                         WHERE \"{col}\" IS NOT NULL \
                           AND instr(replace(\"{col}\", '\\', '/'), '/received/') > 0"
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

                    // (b) Rewrite paths that contain "/invoices/" — manually-generated
                    // invoice XML/PDF (ubl/paths.rs → {app_data}/invoices/…).
                    // New root = data_dir (app_data on this machine).
                    let sql_invoices = format!(
                        "UPDATE \"{table}\" \
                         SET \"{col}\" = ?1 || substr(replace(\"{col}\", '\\', '/'), \
                                                       instr(replace(\"{col}\", '\\', '/'), '/invoices/')) \
                         WHERE \"{col}\" IS NOT NULL \
                           AND instr(replace(\"{col}\", '\\', '/'), '/invoices/') > 0"
                    );
                    if let Err(e) = sqlx::query(&sql_invoices)
                        .bind(&data_dir_str)
                        .execute(&pool)
                        .await
                    {
                        tracing::warn!(
                            table, col, error = ?e,
                            "import_backup: path rewrite (/invoices/) failed, continuing"
                        );
                    }

                    // (c) Rewrite paths that contain "/receipts/" — receipt PDFs
                    // (commands/receipts.rs → {app_data}/receipts/…).
                    // New root = data_dir (app_data on this machine).
                    let sql_receipts = format!(
                        "UPDATE \"{table}\" \
                         SET \"{col}\" = ?1 || substr(replace(\"{col}\", '\\', '/'), \
                                                       instr(replace(\"{col}\", '\\', '/'), '/receipts/')) \
                         WHERE \"{col}\" IS NOT NULL \
                           AND instr(replace(\"{col}\", '\\', '/'), '/receipts/') > 0"
                    );
                    if let Err(e) = sqlx::query(&sql_receipts)
                        .bind(&data_dir_str)
                        .execute(&pool)
                        .await
                    {
                        tracing::warn!(
                            table, col, error = ?e,
                            "import_backup: path rewrite (/receipts/) failed, continuing"
                        );
                    }
                }
            }
            pool.close().await;
            tracing::info!("import_backup: xml_path/pdf_path rewritten to local roots (archive+invoices+receipts)");
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

/// Zip-slip guard: true iff `target` (a path built by joining an untrusted ZIP-entry suffix onto
/// `root`) stays inside `root` after resolving `.`/`..`.
///
/// XPLAT-01: normalises BOTH `root` and `target` with [`normalise_path`] and compares — it must NEVER
/// `canonicalize()` the root, because on Windows `canonicalize` returns a verbatim `\\?\C:\…` path
/// (`Prefix::VerbatimDisk`) while `target` keeps a plain `Prefix::Disk`, and `Path::starts_with` treats
/// those prefixes as unequal — so every legitimate entry was wrongly rejected as zip-slip on Windows
/// (after the live DB had already been overwritten, leaving a half-restored state). `root` is already
/// absolute, so symmetric normalisation keeps both sides prefix-comparable while still rejecting any
/// `..`-escaping entry (`normalise_path` pops `..`, so an out-of-root target fails `starts_with`).
fn target_is_inside_root(root: &std::path::Path, target: &std::path::Path) -> bool {
    normalise_path(target).starts_with(normalise_path(root))
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
    /// Câte dintre fișierele LIPSĂ aparțin unor documente aflate încă în termenul legal de
    /// păstrare de 5 ani (L82/1991) — încălcări de arhivare, nu doar curățenie.
    pub missing_under_retention: usize,
}

/// Verifică că toate fișierele XML referențiate în DB există pe disc.
/// Verifică atât facturile emise (`invoices`) cât și cele primite
/// (`received_invoices`) pentru o acoperire completă.
/// Returnează un raport structurat; apelat din frontend ca `verify_archive_integrity`.
#[tauri::command]
pub async fn verify_archive_integrity(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<ArchiveIntegrityReport> {
    use sqlx::Row;

    // Pragul termenului legal de păstrare (5 ani, L82/1991): un fișier lipsă al unui document
    // emis după acest prag e o încălcare de arhivare.
    let retention_cutoff = (chrono::Utc::now() - chrono::Duration::days((5.0 * 365.25) as i64))
        .format("%Y-%m-%d")
        .to_string();

    // Check sent invoices (xml_path may be NULL for drafts not yet submitted). Scoped per company —
    // the report must not mix tenants' archives.
    let sent_rows = sqlx::query(
        "SELECT xml_path, issue_date FROM invoices          WHERE xml_path IS NOT NULL AND company_id = ?1",
    )
    .bind(&company_id)
    .fetch_all(&state.db)
    .await?;

    // Check received invoices (xml_path is always populated for received invoices).
    let received_rows = sqlx::query(
        "SELECT xml_path, issue_date FROM received_invoices          WHERE xml_path IS NOT NULL AND company_id = ?1",
    )
    .bind(&company_id)
    .fetch_all(&state.db)
    .await?;

    let mut checked: usize = 0;
    let mut missing: Vec<String> = Vec::new();
    let mut missing_under_retention: usize = 0;

    for row in sent_rows.iter().chain(received_rows.iter()) {
        let xml_path: String = row.try_get("xml_path").map_err(AppError::Database)?;
        let issue_date: String = row.try_get("issue_date").unwrap_or_default();
        checked += 1;
        if !std::path::Path::new(&xml_path).exists() {
            if issue_date >= retention_cutoff {
                missing_under_retention += 1;
            }
            missing.push(xml_path);
        }
    }

    let ok = missing.is_empty();
    Ok(ArchiveIntegrityReport {
        checked,
        missing,
        ok,
        missing_under_retention,
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

    // 4. Părintele trebuie să existe (accesibil). Nu mai restrângem la $HOME:
    //    la fel ca `validate_export_path` (XW-3), permitem orice unitate locală
    //    (ex. D:\ pe Windows) atâta timp cât calea e absolută, fără UNC și fără
    //    componente `..`. Astfel mutarea arhivei pe alt disc funcționează pe Windows.
    let parent = new_path_buf
        .parent()
        .ok_or_else(|| AppError::Validation("Cale invalidă.".into()))?;
    parent
        .canonicalize()
        .map_err(|_| AppError::Validation("Calea părinte nu există.".into()))?;

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

    // Update xml_path / pdf_path in both invoices and received_invoices so
    // existing rows point to the new archive location rather than the old one.
    //
    // Strategy: the same separator-based rewrite used in import_backup — find
    // the last occurrence of "/sent/" or "/received/" in each stored path and
    // replace the prefix up to (but not including) that separator with new_path.
    // Rows whose path does not contain either separator are left untouched.
    // Separator-agnostic rewrites: normalise the stored column to forward
    // slashes before locating the marker so that paths stored with Windows
    // backslashes (e.g. C:\...\sent\INV.xml) are matched correctly.
    // The rewritten path uses forward slashes — valid on Windows for Rust/Tauri
    // file I/O (P0 Windows fix, mirrors the same logic in import_backup).
    let new_path_ref = &new_path;
    for table in &["invoices", "received_invoices"] {
        for col in &["xml_path", "pdf_path"] {
            let sql_sent = format!(
                "UPDATE \"{table}\" \
                 SET \"{col}\" = ?1 || substr(replace(\"{col}\", '\\', '/'), \
                                               instr(replace(\"{col}\", '\\', '/'), '/sent/')) \
                 WHERE \"{col}\" IS NOT NULL \
                   AND instr(replace(\"{col}\", '\\', '/'), '/sent/') > 0"
            );
            if let Err(e) = sqlx::query(&sql_sent)
                .bind(new_path_ref)
                .execute(&state.db)
                .await
            {
                tracing::warn!(
                    table, col, error = ?e,
                    "change_archive_location: path rewrite (/sent/) failed, continuing"
                );
            }

            let sql_received = format!(
                "UPDATE \"{table}\" \
                 SET \"{col}\" = ?1 || substr(replace(\"{col}\", '\\', '/'), \
                                               instr(replace(\"{col}\", '\\', '/'), '/received/')) \
                 WHERE \"{col}\" IS NOT NULL \
                   AND instr(replace(\"{col}\", '\\', '/'), '/received/') > 0"
            );
            if let Err(e) = sqlx::query(&sql_received)
                .bind(new_path_ref)
                .execute(&state.db)
                .await
            {
                tracing::warn!(
                    table, col, error = ?e,
                    "change_archive_location: path rewrite (/received/) failed, continuing"
                );
            }
        }
    }
    tracing::info!(
        new_path,
        "change_archive_location: xml_path/pdf_path rewritten to new archive root"
    );

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

    // ── target_is_inside_root (XPLAT-01) ─────────────────────────────────────
    /// A legitimate ZIP entry must stay inside the root, while a `..`-escape is rejected.
    #[test]
    fn zip_slip_guard_accepts_legit_and_rejects_escape() {
        let root = std::path::Path::new("/app/data/archive");
        assert!(
            target_is_inside_root(root, &root.join("sent/2026/INV-0001.xml")),
            "a legitimate entry under the root must be accepted"
        );
        assert!(
            !target_is_inside_root(root, &root.join("../../../etc/passwd")),
            "a ..-escaping entry must be rejected"
        );
    }

    /// XPLAT-01 Windows regression: with a real `C:\…` root (plain `Prefix::Disk`), a legitimate entry
    /// must pass. Before the fix the root was `canonicalize()`d to a verbatim `\\?\C:\…` prefix that
    /// `starts_with` never matched, breaking EVERY restore on Windows. Runs only on Windows.
    #[cfg(windows)]
    #[test]
    fn zip_slip_guard_windows_disk_prefix_is_prefix_comparable() {
        let root = std::path::Path::new(r"C:\app\data\archive");
        assert!(
            target_is_inside_root(root, &root.join(r"invoices\comp-1\INV.xml")),
            "Windows disk-prefixed legit entry must be accepted (XPLAT-01)"
        );
        assert!(
            !target_is_inside_root(root, &root.join(r"..\..\Windows\System32\x")),
            "Windows ..-escape must still be rejected"
        );
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

    // ── backslash-stored paths are rewritten correctly ───────────────────────
    /// Prove that a Windows-style path stored with backslashes (e.g.
    /// `C:\Users\...\sent\INV.xml`) is correctly rewritten to a forward-slash
    /// path under the new root.
    ///
    /// This mirrors the separator-agnostic SQL logic:
    ///   normalize = replace(col, '\', '/')
    ///   pos       = instr(normalize, '/sent/')
    ///   result    = new_root || substr(normalize, pos)
    #[test]
    fn xml_path_backslash_rewrite_sent() {
        // Path as stored on Windows
        let stored =
            r"C:\Users\alice\AppData\Roaming\com.lucaris.efactura\archive\sent\INV-0001.xml";
        // Normalize backslashes → forward slashes (what the SQL replace does)
        let normalized = stored.replace('\\', "/");
        let local_archive = "/Users/bob/Library/Application Support/ro.lucaris.efactura/archive";

        let separator = "/sent/";
        let pos = normalized
            .find(separator)
            .expect("should contain /sent/ after normalization");
        let suffix = &normalized[pos..]; // includes "/sent/"
        let rewritten = format!("{local_archive}{suffix}");

        assert_eq!(
            rewritten,
            "/Users/bob/Library/Application Support/ro.lucaris.efactura/archive/sent/INV-0001.xml"
        );
    }

    #[test]
    fn xml_path_backslash_rewrite_received() {
        // Path as stored on Windows for a received invoice
        let stored = r"C:\Users\alice\AppData\Roaming\com.lucaris.efactura\archive\received\RO123\msg-abc\invoice.xml";
        let normalized = stored.replace('\\', "/");
        let local_archive = "/new/archive";

        let separator = "/received/";
        let pos = normalized
            .find(separator)
            .expect("should contain /received/ after normalization");
        let suffix = &normalized[pos..];
        let rewritten = format!("{local_archive}{suffix}");

        assert_eq!(rewritten, "/new/archive/received/RO123/msg-abc/invoice.xml");
    }

    #[test]
    fn xml_path_backslash_no_match_skipped() {
        // A backslash path with neither /sent/ nor /received/ must NOT be
        // rewritten (the WHERE clause would exclude it).
        let stored = r"C:\Users\alice\Documents\invoice.xml";
        let normalized = stored.replace('\\', "/");
        let has_sent = normalized.contains("/sent/");
        let has_received = normalized.contains("/received/");
        assert!(
            !has_sent && !has_received,
            "path should not match any rewrite separator after normalization"
        );
    }

    // ── /invoices/ and /receipts/ path rewrites ──────────────────────────────

    /// Simulate the SQL rewrite for /invoices/ marker — manually-generated
    /// invoice XML/PDF stored under {app_data}/invoices/{company_id}/{id}.xml.
    #[test]
    fn xml_path_rewrite_invoices_marker() {
        let stored = "/Users/alice/Library/Application Support/ro.lucaris.efactura/invoices/comp-1/INV-001.xml";
        let local_data_dir = "/Users/bob/Library/Application Support/ro.lucaris.efactura";

        let separator = "/invoices/";
        let pos = stored.find(separator).expect("should contain /invoices/");
        let suffix = &stored[pos..]; // includes "/invoices/"
        let rewritten = format!("{local_data_dir}{suffix}");

        assert_eq!(
            rewritten,
            "/Users/bob/Library/Application Support/ro.lucaris.efactura/invoices/comp-1/INV-001.xml"
        );
    }

    /// Simulate the SQL rewrite for /receipts/ marker — receipt PDFs stored
    /// under {app_data}/receipts/{company_id}/{receipt_id}.pdf.
    #[test]
    fn xml_path_rewrite_receipts_marker() {
        let stored = "/Users/alice/Library/Application Support/ro.lucaris.efactura/receipts/comp-2/REC-007.pdf";
        let local_data_dir = "/Users/bob/Library/Application Support/ro.lucaris.efactura";

        let separator = "/receipts/";
        let pos = stored.find(separator).expect("should contain /receipts/");
        let suffix = &stored[pos..];
        let rewritten = format!("{local_data_dir}{suffix}");

        assert_eq!(
            rewritten,
            "/Users/bob/Library/Application Support/ro.lucaris.efactura/receipts/comp-2/REC-007.pdf"
        );
    }

    /// Backslash-stored /invoices/ path is normalized then rewritten correctly.
    #[test]
    fn xml_path_backslash_rewrite_invoices() {
        let stored =
            r"C:\Users\alice\AppData\Roaming\com.lucaris.efactura\invoices\comp-1\INV-001.xml";
        let normalized = stored.replace('\\', "/");
        let local_data_dir = "/Users/bob/Library/Application Support/ro.lucaris.efactura";

        let separator = "/invoices/";
        let pos = normalized
            .find(separator)
            .expect("should contain /invoices/ after normalization");
        let rewritten = format!("{local_data_dir}{}", &normalized[pos..]);

        assert_eq!(
            rewritten,
            "/Users/bob/Library/Application Support/ro.lucaris.efactura/invoices/comp-1/INV-001.xml"
        );
    }

    /// backup_includes_invoices_and_receipts: verifies that zip_dir_recursive
    /// correctly packages invoices/ and receipts/ entries with the right prefix.
    #[test]
    fn backup_includes_invoices_and_receipts() {
        let tmp = tempfile::tempdir().unwrap();

        // Simulate app_data/invoices/comp-1/INV-001.xml
        let inv_dir = tmp.path().join("invoices").join("comp-1");
        std::fs::create_dir_all(&inv_dir).unwrap();
        std::fs::write(inv_dir.join("INV-001.xml"), b"<Invoice/>").unwrap();

        // Simulate app_data/receipts/comp-1/REC-001.pdf
        let rec_dir = tmp.path().join("receipts").join("comp-1");
        std::fs::create_dir_all(&rec_dir).unwrap();
        std::fs::write(rec_dir.join("REC-001.pdf"), b"%PDF").unwrap();

        let buf = std::io::Cursor::new(Vec::new());
        let mut zw = zip::ZipWriter::new(buf);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        // Add both dirs using zip_dir_recursive (same as export_backup does).
        let invoices_dir = tmp.path().join("invoices");
        zip_dir_recursive(&invoices_dir, &invoices_dir, &mut zw, opts).unwrap();
        let receipts_dir = tmp.path().join("receipts");
        zip_dir_recursive(&receipts_dir, &receipts_dir, &mut zw, opts).unwrap();

        let inner = zw.finish().unwrap().into_inner();
        let cursor = std::io::Cursor::new(inner);
        let mut za = zip::ZipArchive::new(cursor).unwrap();
        let names: Vec<_> = (0..za.len())
            .map(|i| za.by_index(i).unwrap().name().to_string())
            .collect();

        assert!(
            names.iter().any(|n| n == "invoices/comp-1/INV-001.xml"),
            "expected 'invoices/comp-1/INV-001.xml', got: {names:?}"
        );
        assert!(
            names.iter().any(|n| n == "receipts/comp-1/REC-001.pdf"),
            "expected 'receipts/comp-1/REC-001.pdf', got: {names:?}"
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
