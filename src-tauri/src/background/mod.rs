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
pub(crate) use spv::parse_received_xml;

const STATUS_POLL_SECS: u64 = 900; // 15 minutes — per plan

/// Supervised task spawner with panic recovery.
///
/// Wraps a task in a `tokio::spawn` and awaits its JoinHandle. If the task
/// panics, logs the error at error level and respawns after a 60s cooldown.
/// If the task exits normally (which shouldn't happen for infinite-loop
/// tasks), logs at warn level and respawns after the same cooldown.
///
/// The factory closure must produce a fresh future on each call — this
/// allows the task to capture cloned handles to `AppHandle` etc.
pub fn spawn_supervised<F, Fut>(name: &'static str, factory: F)
where
    F: Fn() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    drop(tokio::spawn(async move {
        let mut restart_count: u32 = 0;
        loop {
            let fut = factory();
            let handle = tokio::spawn(fut);
            match handle.await {
                Ok(()) => {
                    tracing::warn!(
                        task = name,
                        "Background task exited normally — respawning in 60s"
                    );
                }
                Err(e) if e.is_panic() => {
                    restart_count = restart_count.saturating_add(1);
                    tracing::error!(
                        task = name,
                        restart_count,
                        "Background task PANICKED — respawning in 60s. Panic: {:?}",
                        e
                    );
                }
                Err(e) => {
                    // Cancelled/aborted — supervisor exits
                    tracing::error!(
                        task = name,
                        error = ?e,
                        "Background task supervisor: task cancelled, exiting"
                    );
                    return;
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        }
    }));
}

/// Spawns all background tasks under panic-supervision.
///
/// Each task is wrapped in `spawn_supervised` which auto-restarts the task
/// after a 60s cooldown if it panics. This prevents a single buggy iteration
/// from permanently killing background functionality (ANAF polling, SPV sync,
/// etc.) until the app is restarted.
pub fn spawn_background_tasks(app: AppHandle) {
    let app_for_poll = app.clone();
    let app_for_spv = app.clone();
    let app_for_cert = app.clone();
    let app_for_cleanup = app.clone();
    let app_for_archive = app.clone();
    let app_for_token = app.clone();
    let app_for_recurring = app.clone();

    // Task 1: Poll status of SUBMITTED invoices every 15 min
    spawn_supervised("poll_submitted_invoices", move || {
        let app_inner = app_for_poll.clone();
        async move {
            loop {
                tokio::time::sleep(Duration::from_secs(STATUS_POLL_SECS)).await;
                if let Err(e) = poll::poll_submitted_invoices(&app_inner).await {
                    tracing::warn!("Status poll error: {:?}", e);
                } else if let Some(state) = app_inner.try_state::<AppState>() {
                    let pool = state.db.clone();
                    if let Err(e) = sqlx::query(
                        "INSERT INTO audit_log (id, action, entity_type, entity_id, metadata, created_at) VALUES (?1, ?2, ?3, ?4, ?5, unixepoch())"
                    )
                    .bind(crate::db::models::new_id())
                    .bind("background_task_run")
                    .bind("background")
                    .bind("poll_status")
                    .bind("{\"result\":\"ok\"}")
                    .execute(&pool)
                    .await
                    {
                        tracing::debug!(error = ?e, "Failed to write audit_log entry (non-fatal)");
                    }
                }
            }
        }
    });

    // Task 2: Sync SPV messages — daily at 04:00 local time
    spawn_supervised("sync_spv_messages", move || {
        let app_inner = app_for_spv.clone();
        async move {
            loop {
                sleep_until_local_time(4, 0).await;
                if let Err(e) = spv::sync_spv(&app_inner).await {
                    tracing::warn!("SPV sync error: {:?}", e);
                } else if let Some(state) = app_inner.try_state::<AppState>() {
                    let pool = state.db.clone();
                    if let Err(e) = sqlx::query(
                        "INSERT INTO audit_log (id, action, entity_type, entity_id, metadata, created_at) VALUES (?1, ?2, ?3, ?4, ?5, unixepoch())"
                    )
                    .bind(crate::db::models::new_id())
                    .bind("background_task_run")
                    .bind("background")
                    .bind("sync_spv_messages")
                    .bind("{\"result\":\"ok\"}")
                    .execute(&pool)
                    .await
                    {
                        tracing::debug!(error = ?e, "Failed to write audit_log entry (non-fatal)");
                    }

                    // Update last_sync_at in settings for StatusBar
                    let _ = sqlx::query(
                        "INSERT INTO settings(key,value) VALUES('last_sync_at',?1) \
                         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    )
                    .bind(chrono::Utc::now().timestamp().to_string())
                    .execute(&pool)
                    .await;

                    // Update tray tooltip with pending invoice count
                    if let Some(tray) = app_inner.tray_by_id("main") {
                        let pending_count: i64 = sqlx::query_scalar(
                            "SELECT COUNT(*) FROM invoices WHERE status = 'SUBMITTED'",
                        )
                        .fetch_one(&pool)
                        .await
                        .unwrap_or(0);
                        let _ = tray.set_tooltip(Some(&format!(
                            "Clarito — {} în așteptare",
                            pending_count
                        )));
                    }
                }
            }
        }
    });

    // Task 3: Certificate expiry checker — daily at 09:00 local time
    spawn_supervised("check_certificate_expiry", move || {
        let app_inner = app_for_cert.clone();
        async move {
            loop {
                sleep_until_local_time(9, 0).await;
                if let Some(state) = app_inner.try_state::<AppState>() {
                    recovery::check_certificate_expiry(&state.db, &app_inner).await;
                }
            }
        }
    });

    // Task 4: Cleanup audit log (every 7 days)
    spawn_supervised("cleanup_audit_log", move || {
        let app_inner = app_for_cleanup.clone();
        async move {
            let mut interval = tokio::time::interval(Duration::from_secs(7 * 24 * 3600));
            loop {
                interval.tick().await;
                if let Some(state) = app_inner.try_state::<AppState>() {
                    recovery::cleanup_audit_log(state.db.clone()).await;
                }
            }
        }
    });

    // Task 5: Archive check (every 30 days)
    spawn_supervised("archive_check", move || {
        let app_inner = app_for_archive.clone();
        async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30 * 24 * 3600));
            loop {
                interval.tick().await;
                if let Some(state) = app_inner.try_state::<AppState>() {
                    recovery::archive_check(state.db.clone(), app_inner.clone()).await;
                }
            }
        }
    });

    // Task 6: Refresh expiring OAuth tokens — daily at 03:00 local time
    spawn_supervised("refresh_expiring_tokens", move || {
        let app_inner = app_for_token.clone();
        async move {
            loop {
                sleep_until_local_time(3, 0).await;
                if let Some(state) = app_inner.try_state::<AppState>() {
                    recovery::refresh_expiring_certificates(&state.db, &app_inner).await;
                }
            }
        }
    });

    // Task 7: Generate recurring invoices — daily at 08:00 local time
    spawn_supervised("generate_recurring", move || {
        let app_inner = app_for_recurring.clone();
        async move {
            loop {
                sleep_until_local_time(8, 0).await;
                if let Err(e) = recurring::generate_recurring(&app_inner).await {
                    tracing::warn!("Recurring invoice processing error: {:?}", e);
                }
            }
        }
    });

    // Task 8: One-shot crash recovery — reset QUEUED invoices with no upload_id
    // Not supervised: runs once at startup, not an infinite loop.
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
        .earliest()
        .unwrap_or(now);
    if target <= now {
        target += chrono::Duration::days(1);
    }
    let duration = (target - now).to_std().unwrap_or(Duration::from_secs(3600));
    tokio::time::sleep(duration).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn spawn_supervised_compiles_and_runs() {
        spawn_supervised("test_task", || async {
            // no-op; just verify the API compiles and runs once
        });
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}
