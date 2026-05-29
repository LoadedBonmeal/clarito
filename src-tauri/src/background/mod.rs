//! Background tasks: auto-poll submitted invoices + sync SPV messages.
//! Launched once at startup, runs in separate tokio tasks.

use std::time::Duration;
use tauri::{AppHandle, Manager};

use crate::state::AppState;

mod poll;
mod recovery;
mod recurring;
mod spv;

// Re-export functions used by other modules
pub(crate) use poll::poll_submitted_for_company;
pub(crate) use spv::do_sync_spv;

const STATUS_POLL_SECS: u64 = 900; // 15 minutes — per plan

pub fn spawn_background_tasks(app: AppHandle) {
    let app1 = app.clone();
    let app2 = app.clone();
    let app3 = app.clone();
    let app4 = app.clone();
    let app5 = app.clone();
    let app6 = app.clone();
    let app7 = app.clone();

    // Task 1: Poll status of SUBMITTED invoices every 15 min
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(STATUS_POLL_SECS)).await;
            if let Err(e) = poll::poll_submitted_invoices(&app1).await {
                tracing::warn!("Status poll error: {:?}", e);
            } else if let Some(state) = app1.try_state::<AppState>() {
                let pool = state.db.clone();
                let _ = sqlx::query(
                    "INSERT INTO audit_log (id, action, entity_type, entity_id, metadata, created_at) VALUES (?1, ?2, ?3, ?4, ?5, unixepoch())"
                )
                .bind(crate::db::models::new_id())
                .bind("background_task_run")
                .bind("background")
                .bind("poll_status")
                .bind("{\"result\":\"ok\"}")
                .execute(&pool)
                .await;
            }
        }
    });

    // Task 2: Sync SPV messages — daily at 04:00 local time
    tauri::async_runtime::spawn(async move {
        loop {
            sleep_until_local_time(4, 0).await;
            if let Err(e) = spv::sync_spv(&app2).await {
                tracing::warn!("SPV sync error: {:?}", e);
            } else if let Some(state) = app2.try_state::<AppState>() {
                let pool = state.db.clone();
                let _ = sqlx::query(
                    "INSERT INTO audit_log (id, action, entity_type, entity_id, metadata, created_at) VALUES (?1, ?2, ?3, ?4, ?5, unixepoch())"
                )
                .bind(crate::db::models::new_id())
                .bind("background_task_run")
                .bind("background")
                .bind("sync_spv_messages")
                .bind("{\"result\":\"ok\"}")
                .execute(&pool)
                .await;

                // Update last_sync_at in settings for StatusBar
                let _ = sqlx::query(
                    "INSERT INTO settings(key,value) VALUES('last_sync_at',?1) \
                     ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                )
                .bind(chrono::Utc::now().timestamp().to_string())
                .execute(&pool)
                .await;

                // Update tray tooltip with pending invoice count
                if let Some(tray) = app2.tray_by_id("main") {
                    let pending_count: i64 = sqlx::query_scalar(
                        "SELECT COUNT(*) FROM invoices WHERE status = 'SUBMITTED'",
                    )
                    .fetch_one(&pool)
                    .await
                    .unwrap_or(0);
                    let _ = tray
                        .set_tooltip(Some(&format!("RoFactura — {} în așteptare", pending_count)));
                }
            }
        }
    });

    // Task 3: Certificate expiry checker — daily at 09:00 local time
    tauri::async_runtime::spawn(async move {
        loop {
            sleep_until_local_time(9, 0).await;
            if let Some(state) = app3.try_state::<AppState>() {
                recovery::check_certificate_expiry(&state.db, &app3).await;
            }
        }
    });

    // Task 4: Cleanup audit log (every 7 days)
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(7 * 24 * 3600));
        loop {
            interval.tick().await;
            if let Some(state) = app4.try_state::<AppState>() {
                recovery::cleanup_audit_log(state.db.clone()).await;
            }
        }
    });

    // Task 5: Archive check (every 30 days)
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30 * 24 * 3600));
        loop {
            interval.tick().await;
            if let Some(state) = app5.try_state::<AppState>() {
                recovery::archive_check(state.db.clone(), app5.clone()).await;
            }
        }
    });

    // Task 6: Refresh expiring OAuth tokens — daily at 03:00 local time
    tauri::async_runtime::spawn(async move {
        loop {
            sleep_until_local_time(3, 0).await;
            if let Some(state) = app6.try_state::<AppState>() {
                recovery::refresh_expiring_certificates(&state.db, &app6).await;
            }
        }
    });

    // Task 7: Generate recurring invoices — daily at 08:00 local time
    tauri::async_runtime::spawn(async move {
        loop {
            sleep_until_local_time(8, 0).await;
            if let Err(e) = recurring::generate_recurring(&app7).await {
                tracing::warn!("Recurring invoice processing error: {:?}", e);
            }
        }
    });

    // Task 8: One-shot crash recovery — reset QUEUED invoices with no upload_id
    let app8 = app.clone();
    tauri::async_runtime::spawn(async move {
        recovery::recover_stale_queued(&app8).await;
    });
}

/// Dorme până la ora locală specificată (HH:MM) din ziua curentă sau mâine.
async fn sleep_until_local_time(hour: u32, minute: u32) {
    use chrono::{Local, NaiveTime};
    let now = Local::now();
    let target_time = NaiveTime::from_hms_opt(hour, minute, 0).unwrap_or_else(|| {
        NaiveTime::from_hms_opt(4, 0, 0).expect("04:00 is a valid time — constant infallible")
    });
    let mut target = now
        .date_naive()
        .and_time(target_time)
        .and_local_timezone(Local)
        .single()
        .unwrap_or(now);
    if target <= now {
        target += chrono::Duration::days(1);
    }
    let duration = (target - now).to_std().unwrap_or(Duration::from_secs(3600));
    tokio::time::sleep(duration).await;
}
