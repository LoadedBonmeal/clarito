//! Comenzi sistem: info app, sync manual, path debug.

use serde::Serialize;
use sqlx::Row;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::db::pool::resolve_db_path;
use crate::error::AppResult;
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub db_path: String,
    pub app_data_dir: String,
}

#[tauri::command]
pub fn get_app_info(app: AppHandle) -> AppResult<AppInfo> {
    let db_path = resolve_db_path(&app)?;
    let app_data_dir = app.path().app_data_dir()?;

    Ok(AppInfo {
        name: "RoFactura".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        db_path: db_path.display().to_string(),
        app_data_dir: app_data_dir.display().to_string(),
    })
}

#[tauri::command]
pub fn get_db_path(app: AppHandle) -> AppResult<String> {
    resolve_db_path(&app).map(|p| p.display().to_string())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    pub status_polls: u32,
    pub new_received: u32,
    pub updated_at: i64,
}

#[tauri::command]
pub async fn manual_sync(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> AppResult<SyncResult> {
    use crate::background::{do_sync_spv, poll_submitted_for_company};

    let pool = &state.db;
    let all_companies = crate::db::companies::list(pool).await?;
    let mut status_polls: u32 = 0;
    let mut new_received: u32 = 0;

    for company in &all_companies {
        // Skip companies without an ANAF token
        if crate::anaf::keychain::TokenBundle::load(&company.id).is_none() {
            continue;
        }

        // Poll SUBMITTED invoices for this company
        status_polls += poll_submitted_for_company(pool, &company.id, None)
            .await
            .unwrap_or(0);

        // Sync SPV messages for this company (respect USE_ANAF_TEST_ENV setting)
        let test_mode = crate::db::settings::get_bool(
            pool,
            crate::db::settings::keys::USE_ANAF_TEST_ENV,
            false,
        )
        .await
        .unwrap_or(false);
        new_received += do_sync_spv(pool, &company.id, &app, test_mode)
            .await
            .unwrap_or(0) as u32;
    }

    // Persist last sync timestamp for StatusBar display
    let now = chrono::Utc::now().timestamp();
    let _ = sqlx::query(
        "INSERT INTO settings(key, value) VALUES('last_sync_at', ?1) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(now.to_string())
    .execute(pool)
    .await;

    let result = SyncResult {
        status_polls,
        new_received,
        updated_at: now,
    };

    // Emit sync_completed event for frontend reactive updates
    let _ = app.emit(
        "sync_completed",
        serde_json::json!({
            "statusPolls": result.status_polls,
            "newReceived": result.new_received,
            "updatedAt": result.updated_at
        }),
    );

    Ok(result)
}

/// Seed pentru development. Idempotent — nu face nimic dacă DB-ul are date.
/// Disponibil numai în build-urile de debug (dev). Exclus din release.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn dev_seed(state: State<'_, AppState>) -> AppResult<()> {
    crate::db::seed::run_if_empty(&state.db).await
}

/// Returnează ultimele 50 de înregistrări din audit_log cu action = 'background_task_run'.
#[tauri::command]
pub async fn get_activity_log(state: State<'_, AppState>) -> AppResult<Vec<serde_json::Value>> {
    let rows = sqlx::query(
        "SELECT id, entity_id, metadata, created_at FROM audit_log \
         WHERE action IN ( \
             'background_task_run', \
             'invoice_created', 'invoice_updated', 'invoice_deleted', \
             'invoice_stornoed', 'invoice_duplicated', 'invoice_submitted_anaf', \
             'company_created', 'company_updated', \
             'recurring_updated' \
         ) \
         ORDER BY created_at DESC LIMIT 50",
    )
    .fetch_all(&state.db)
    .await
    .map_err(crate::error::AppError::Database)?;

    let result = rows
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.try_get::<String, _>("id").unwrap_or_default(),
                "entityId": r.try_get::<String, _>("entity_id").unwrap_or_default(),
                "metadata": r.try_get::<String, _>("metadata").unwrap_or_default(),
                "createdAt": r.try_get::<i64, _>("created_at").unwrap_or_default(),
            })
        })
        .collect();
    Ok(result)
}

/// Exportă jurnalul de activitate ca CSV și returnează conținutul.
#[tauri::command]
pub async fn export_activity_log_csv(state: State<'_, AppState>) -> AppResult<String> {
    let rows = sqlx::query(
        "SELECT id, entity_id, metadata, created_at FROM audit_log \
         WHERE action IN ( \
             'background_task_run', \
             'invoice_created', 'invoice_updated', 'invoice_deleted', \
             'invoice_stornoed', 'invoice_duplicated', 'invoice_submitted_anaf', \
             'company_created', 'company_updated', \
             'recurring_updated' \
         ) \
         ORDER BY created_at DESC LIMIT 500",
    )
    .fetch_all(&state.db)
    .await
    .map_err(crate::error::AppError::Database)?;

    let mut csv = String::from("ID,Task,Rezultat,Timp\n");
    for r in &rows {
        let id = r.try_get::<String, _>("id").unwrap_or_default();
        let entity_id = r.try_get::<String, _>("entity_id").unwrap_or_default();
        let metadata = r.try_get::<String, _>("metadata").unwrap_or_default();
        let created_at = r.try_get::<i64, _>("created_at").unwrap_or_default();
        let ts = chrono::DateTime::from_timestamp(created_at, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| created_at.to_string());
        // Proper CSV escaping: wrap fields with commas/quotes in double-quotes
        let meta_safe = metadata.replace('"', "\"\"");
        csv.push_str(&format!("{},{},\"{}\",{}\n", id, entity_id, meta_safe, ts));
    }
    Ok(csv)
}

/// Activează / dezactivează pornirea automată la login (LaunchAgent pe macOS, Registry pe Windows).
#[tauri::command]
pub async fn set_autostart(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> AppResult<()> {
    use tauri_plugin_autostart::ManagerExt;

    // 1. OS-level autostart
    if enabled {
        app.autolaunch()
            .enable()
            .map_err(|e| crate::error::AppError::Other(e.to_string()))?;
    } else {
        app.autolaunch()
            .disable()
            .map_err(|e| crate::error::AppError::Other(e.to_string()))?;
    }

    // 2. Mirror in DB so the toggle reads back correctly without an OS call
    let value = if enabled { "1" } else { "0" };
    sqlx::query(
        "INSERT INTO settings(key, value) VALUES('autostart', ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(value)
    .execute(&state.db)
    .await
    .map_err(crate::error::AppError::Database)?;

    Ok(())
}

#[tauri::command]
pub async fn get_autostart(app: AppHandle) -> AppResult<bool> {
    use tauri_plugin_autostart::ManagerExt;
    let enabled = app
        .autolaunch()
        .is_enabled()
        .map_err(|e| crate::error::AppError::Other(e.to_string()))?;
    Ok(enabled)
}

/// Deschide folderul de arhivă în file manager.
#[tauri::command]
pub async fn open_archive_folder(app: AppHandle) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;

    let data_dir = app.path().app_data_dir()?;
    let archive_dir = data_dir.join("archive");
    std::fs::create_dir_all(&archive_dir).map_err(crate::error::AppError::Io)?;
    app.opener()
        .open_path(archive_dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| crate::error::AppError::Other(e.to_string()))?;
    Ok(())
}
