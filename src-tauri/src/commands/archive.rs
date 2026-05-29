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

/// Exportă un backup complet (DB + README) într-un fișier ZIP.
/// Returnează path-ul fișierului ZIP generat.
#[tauri::command]
pub async fn export_backup(_state: State<'_, AppState>, app: AppHandle) -> AppResult<String> {
    let data_dir = app.path().app_data_dir()?;
    let db_path = data_dir.join("data.db");

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

        // Add README
        zip.start_file("README.txt", opts)
            .map_err(|e| AppError::Other(e.to_string()))?;
        let readme = format!(
            "Backup eFactura Desktop\nData: {}\n\nConține:\n- data.db: baza de date SQLite\n\nRestaurare: copiați data.db în folderul de date al aplicației.\n",
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

/// Importă un backup ZIP, înlocuind DB-ul curent.
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

    // 3. Determină calea curentă a DB
    let db_path = crate::db::pool::resolve_db_path(&app)?;

    // 4. Backup DB curent
    std::fs::copy(&db_path, db_path.with_extension("db.bak")).map_err(AppError::Io)?;

    // 5. Extrage data.db din ZIP și scrie la calea DB-ului
    let mut db_entry = archive
        .by_name("data.db")
        .map_err(|e| AppError::Other(e.to_string()))?;
    let mut buf = Vec::new();
    db_entry.read_to_end(&mut buf).map_err(AppError::Io)?;
    std::fs::write(&db_path, &buf).map_err(AppError::Io)?;

    // 6. Repornește aplicația
    app.request_restart();
    #[allow(unreachable_code)]
    Ok(())
}

// ─── Integrity check ───────────────────────────────────────────────────────

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrityResult {
    pub total_checked: u32,
    pub missing_files: Vec<String>,
    pub ok: bool,
}

/// Verifică că toate fișierele XML referențiate în DB există pe disc.
#[tauri::command]
pub async fn verify_archive_integrity(state: State<'_, AppState>) -> AppResult<IntegrityResult> {
    use sqlx::Row;

    let rows = sqlx::query("SELECT xml_path FROM invoices WHERE xml_path IS NOT NULL")
        .fetch_all(&state.db)
        .await?;

    let mut total_checked: u32 = 0;
    let mut missing_files: Vec<String> = Vec::new();

    for row in &rows {
        let xml_path: String = row.try_get("xml_path").map_err(AppError::Database)?;
        total_checked += 1;
        if !std::path::Path::new(&xml_path).exists() {
            missing_files.push(xml_path);
        }
    }

    let ok = missing_files.is_empty();
    Ok(IntegrityResult {
        total_checked,
        missing_files,
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
    let new_dir = std::path::Path::new(&new_path);
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
