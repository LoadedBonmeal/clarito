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

    // Build ZIP in memory
    let buf = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(buf);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    for invoice in &result.items {
        // Add XML if exists
        if let Some(xml_path) = &invoice.xml_path {
            if let Ok(bytes) = std::fs::read(xml_path) {
                let name = format!("{}.xml", invoice.full_number.replace('/', "_"));
                zip.start_file(&name, options)
                    .map_err(|e| AppError::Other(e.to_string()))?;
                zip.write_all(&bytes)
                    .map_err(|e| AppError::Other(e.to_string()))?;
            }
        }
        // Add PDF if exists
        if let Some(pdf_path) = &invoice.pdf_path {
            if let Ok(bytes) = std::fs::read(pdf_path) {
                let name = format!("{}.pdf", invoice.full_number.replace('/', "_"));
                zip.start_file(&name, options)
                    .map_err(|e| AppError::Other(e.to_string()))?;
                zip.write_all(&bytes)
                    .map_err(|e| AppError::Other(e.to_string()))?;
            }
        }
    }

    let cursor = zip.finish().map_err(|e| AppError::Other(e.to_string()))?;

    // Save to app_data_dir
    let out_dir = app.path().app_data_dir()?;
    let zip_path = out_dir.join(format!(
        "export_{}.zip",
        chrono::Utc::now().format("%Y%m%d_%H%M%S")
    ));
    std::fs::write(&zip_path, cursor.into_inner()).map_err(AppError::Io)?;

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
            "Backup eFactura Desktop\nData: {}\n\nConține:\n- data.db: baza de date SQLite\n- archive/: fișiere XML+PDF facturi\n\nRestaurare: folosiți funcția Import Backup din aplicație.\n",
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
    // 1. Deschide ZIP-ul
    let file = std::fs::File::open(&path).map_err(AppError::Io)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| AppError::Other(e.to_string()))?;

    // 2. Verifică că ZIP-ul conține data.db
    let has_db = (0..archive.len()).any(|i| {
        archive
            .by_index(i)
            .map(|f| f.name() == "data.db")
            .unwrap_or(false)
    });
    if !has_db {
        return Err(AppError::Other("ZIP invalid: lipsește data.db".to_string()));
    }

    // 2a. Extrage conținutul data.db din ZIP în memorie (necesar și pentru verificare)
    let buf = {
        let mut db_entry = archive
            .by_name("data.db")
            .map_err(|e| AppError::Other(e.to_string()))?;
        let mut b = Vec::new();
        db_entry.read_to_end(&mut b).map_err(AppError::Io)?;
        b
    };

    // 2b. Validează integritatea SQLite a backup-ului înainte de a suprascrie DB-ul curent
    let temp_check_path = {
        let data_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| AppError::Other(e.to_string()))?;
        data_dir.join("data_restore_check.db")
    };
    std::fs::write(&temp_check_path, &buf).map_err(AppError::Io)?;

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
            let _ = std::fs::remove_file(&temp_check_path);
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
                let _ = std::fs::remove_file(&temp_check_path);
                return Err(AppError::Other(format!(
                    "Backup invalid: tabelul '{table}' lipsește. Acest fișier nu pare să fie un backup RoFactura valid."
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
                let _ = std::fs::remove_file(&temp_check_path);
                return Err(AppError::Other(format!(
                    "Backup invalid: coloana '{col}' lipsește din tabelul invoices."
                )));
            }
        }

        check_pool.close().await;
    }
    std::fs::remove_file(&temp_check_path).ok();

    // 3. Determină calea curentă a DB
    let db_path = crate::db::pool::resolve_db_path(&app)?;

    // 4. Backup DB curent
    std::fs::copy(&db_path, db_path.with_extension("db.bak")).map_err(AppError::Io)?;

    // 5. Scrie data.db extrasă din ZIP la calea DB-ului
    std::fs::write(&db_path, &buf).map_err(AppError::Io)?;

    // 6. Restaurează fișierele archive/* din ZIP (dacă există), cu protecție
    //    zip-slip: orice intrare al cărei path normalizat iese din archive_dir
    //    este respinsă (consistent cu SEC-06 de mai sus).
    let archive_dir = app.path().app_data_dir()?.join("archive");
    // Re-open the archive for a second pass (first was consumed above).
    let file2 = std::fs::File::open(&path).map_err(AppError::Io)?;
    let mut archive2 = zip::ZipArchive::new(file2).map_err(|e| AppError::Other(e.to_string()))?;

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

    // 7. Repornește aplicația
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
/// Returnează un raport structurat; apelat din frontend ca `verify_archive_integrity`.
#[tauri::command]
pub async fn verify_archive_integrity(
    state: State<'_, AppState>,
) -> AppResult<ArchiveIntegrityReport> {
    use sqlx::Row;

    let rows = sqlx::query("SELECT xml_path FROM invoices WHERE xml_path IS NOT NULL")
        .fetch_all(&state.db)
        .await?;

    let mut checked: usize = 0;
    let mut missing: Vec<String> = Vec::new();

    for row in &rows {
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
#[tauri::command]
pub async fn get_archive_size(app: AppHandle) -> AppResult<u64> {
    let archive_dir = app.path().app_data_dir()?.join("archive");
    if !archive_dir.exists() {
        return Ok(0);
    }
    Ok(dir_size(&archive_dir))
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

    let new_dir = new_path_buf.as_path();
    std::fs::create_dir_all(new_dir).map_err(|e| AppError::Other(e.to_string()))?;

    // Get current archive path
    let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Other(e.to_string()))?;
    let current_archive = app_data.join("archive");

    // Copy existing files if directory exists
    if current_archive.exists() {
        copy_dir_recursive(&current_archive, new_dir)
            .map_err(|e| AppError::Other(format!("Copiere arhivă eșuată: {}", e)))?;
    }

    // Save new path in settings
    sqlx::query(
        "INSERT INTO settings(key, value) VALUES('archive_path', ?1) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(&new_path)
    .execute(&state.db)
    .await
    .map_err(AppError::Database)?;

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
