//! Comenzi sistem: info app, sync manual, path debug.

use serde::Serialize;
use sqlx::Row;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::db::pool::resolve_db_path;
use crate::error::AppResult;
use crate::state::AppState;

/// SEC-07/08: the activity-log filter — the surfaced action set, scoped to one company,
/// plus the global `background_task_run` maintenance carve-out (visible to every company).
/// `?1` binds the company_id. Single source of truth for the two read commands + the
/// isolation test so the scoping predicate can never drift between them.
const ACTIVITY_LOG_WHERE: &str = "action IN ( \
         'background_task_run', \
         'invoice_created', 'invoice_updated', 'invoice_deleted', \
         'invoice_stornoed', 'invoice_duplicated', 'invoice_submitted_anaf', \
         'company_created', 'company_updated', \
         'recurring_updated' \
     ) \
     AND (company_id = ?1 OR action = 'background_task_run')";

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
        name: "Clarito".into(),
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
        status_polls +=
            poll_submitted_for_company(pool, &company.id, None, &state.token_refresh_lock)
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

/// Returnează ultimele 50 de înregistrări din audit_log pentru compania curentă.
/// SEC-07: scoped la `company_id` — un eveniment al altei companii nu mai apare; doar
/// evenimentele globale de mentenanță (`background_task_run`) sunt vizibile tuturor.
#[tauri::command]
pub async fn get_activity_log(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<serde_json::Value>> {
    let rows = sqlx::query(&format!(
        "SELECT id, entity_id, metadata, created_at FROM audit_log \
         WHERE {ACTIVITY_LOG_WHERE} ORDER BY created_at DESC LIMIT 50"
    ))
    .bind(&company_id)
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
/// SEC-08: scoped la `company_id` (vezi `get_activity_log`).
#[tauri::command]
pub async fn export_activity_log_csv(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<String> {
    let rows = sqlx::query(&format!(
        "SELECT id, entity_id, metadata, created_at FROM audit_log \
         WHERE {ACTIVITY_LOG_WHERE} ORDER BY created_at DESC LIMIT 500"
    ))
    .bind(&company_id)
    .fetch_all(&state.db)
    .await
    .map_err(crate::error::AppError::Database)?;

    // UTF-8 BOM so Excel opens Romanian diacritics correctly
    let mut csv = String::from("\u{FEFF}ID,Task,Rezultat,Timp\r\n");
    for r in &rows {
        let id = r.try_get::<String, _>("id").unwrap_or_default();
        let entity_id = r.try_get::<String, _>("entity_id").unwrap_or_default();
        let metadata = r.try_get::<String, _>("metadata").unwrap_or_default();
        let created_at = r.try_get::<i64, _>("created_at").unwrap_or_default();
        let ts = chrono::DateTime::from_timestamp(created_at, 0)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| created_at.to_string());
        // Neutralize CSV/DDE formula injection on the user-derived fields, then quote-escape.
        let entity_id = sanitize_csv_field(&entity_id);
        let meta_safe = sanitize_csv_field(&metadata).replace('"', "\"\"");
        csv.push_str(&format!("{id},{entity_id},\"{meta_safe}\",{ts}\r\n"));
    }
    Ok(csv)
}

/// Neutralize CSV/DDE formula injection: a field beginning with `= + - @` (or TAB/CR) is prefixed
/// with a single quote so a spreadsheet treats it as text, not a formula. Excel/LibreOffice execute
/// `=cmd()`, `+`, `-`, `@DDE` on open, so any user-derived CSV field must pass through this.
pub(crate) fn sanitize_csv_field(s: &str) -> String {
    match s.chars().next() {
        Some('=' | '+' | '-' | '@' | '\t' | '\r') => format!("'{s}"),
        _ => s.to_string(),
    }
}

#[cfg(test)]
mod csv_injection_tests {
    use super::sanitize_csv_field;
    #[test]
    fn prefixes_formula_leads_only() {
        for bad in ["=1+1", "+1", "-1", "@SUM", "\tx", "\rx"] {
            assert_eq!(sanitize_csv_field(bad), format!("'{bad}"));
        }
        for ok in ["FAC-2026-001", "Pop Ana", "123", "a=b"] {
            assert_eq!(sanitize_csv_field(ok), ok);
        }
    }
}

/// Verifică dacă versiunile formularelor ANAF grupate sunt la zi față de manifesto-ul CDN.
/// Erorile de rețea sunt non-fatale — returnează vector gol (fără banner).
#[tauri::command]
pub async fn check_form_versions() -> AppResult<Vec<crate::anaf_decl::form_versions::FormStaleness>>
{
    Ok(crate::anaf_decl::form_versions::check().await)
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
pub async fn open_archive_folder(state: State<'_, AppState>, app: AppHandle) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;

    let data_dir = app.path().app_data_dir()?;
    // Honor the user-configured archive location (ARCHIVE_PATH_OVERRIDE), falling
    // back to the default <app_data>/archive. Same resolution as gdpr::resolve_archive_dir.
    let override_val =
        crate::db::settings::get(&state.db, crate::db::settings::keys::ARCHIVE_PATH_OVERRIDE)
            .await
            .unwrap_or(None);
    let archive_dir = match override_val {
        Some(p) if !p.is_empty() => std::path::PathBuf::from(p),
        _ => data_dir.join("archive"),
    };
    tokio::fs::create_dir_all(&archive_dir)
        .await
        .map_err(crate::error::AppError::Io)?;
    app.opener()
        .open_path(archive_dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| crate::error::AppError::Other(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ACTIVITY_LOG_WHERE;
    use sqlx::{Row, SqlitePool};

    /// SEC-07/08: the shared activity-log filter must return ONLY the querying company's
    /// events plus the global background_task_run carve-out — never another company's.
    #[tokio::test]
    async fn activity_log_is_company_scoped() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE audit_log (id TEXT PRIMARY KEY, action TEXT NOT NULL, \
             entity_type TEXT NOT NULL, entity_id TEXT NOT NULL, company_id TEXT, \
             metadata TEXT, created_at INTEGER NOT NULL DEFAULT (unixepoch()))",
        )
        .execute(&pool)
        .await
        .unwrap();
        for (id, action, company_id) in [
            ("a", "invoice_created", Some("comp-1")),
            ("b", "invoice_submitted_anaf", Some("comp-2")), // foreign — must be hidden
            ("c", "background_task_run", None),              // global — must be visible
            ("d", "invoice_deleted", Some("comp-1")),
        ] {
            sqlx::query(
                "INSERT INTO audit_log (id, action, entity_type, entity_id, company_id) \
                 VALUES (?1, ?2, 'x', 'e', ?3)",
            )
            .bind(id)
            .bind(action)
            .bind(company_id)
            .execute(&pool)
            .await
            .unwrap();
        }

        let ids: Vec<String> = sqlx::query(&format!(
            "SELECT id FROM audit_log WHERE {ACTIVITY_LOG_WHERE} ORDER BY id"
        ))
        .bind("comp-1")
        .fetch_all(&pool)
        .await
        .unwrap()
        .iter()
        .map(|r| r.get::<String, _>("id"))
        .collect();

        // comp-1's two rows + the global background_task_run; comp-2's row excluded.
        assert_eq!(ids, vec!["a", "c", "d"], "comp-2 (id=b) must not leak");
    }
}
