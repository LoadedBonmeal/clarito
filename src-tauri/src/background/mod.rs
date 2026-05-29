//! Background tasks: auto-poll submitted invoices + sync SPV messages.
//! Launched once at startup, runs in separate tokio tasks.

use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

const STATUS_POLL_SECS: u64 = 900;   // 15 minutes — per plan

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
            if let Some(state) = app1.try_state::<AppState>() {
                if let Err(e) = poll_submitted_invoices(&app1, &state).await {
                    tracing::warn!("Status poll error: {:?}", e);
                } else {
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
        }
    });

    // Task 2: Sync SPV messages — daily at 04:00 local time
    tauri::async_runtime::spawn(async move {
        loop {
            sleep_until_local_time(4, 0).await;
            if let Some(state) = app2.try_state::<AppState>() {
                if let Err(e) = sync_spv_messages(&app2, &state).await {
                    tracing::warn!("SPV sync error: {:?}", e);
                } else {
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
                            "SELECT COUNT(*) FROM invoices WHERE status = 'SUBMITTED'"
                        ).fetch_one(&pool).await.unwrap_or(0);
                        let _ = tray.set_tooltip(Some(&format!("RoFactura — {} în așteptare", pending_count)));
                    }
                }
            }
        }
    });

    // Task 3: Certificate expiry checker — daily at 09:00 local time
    tauri::async_runtime::spawn(async move {
        loop {
            sleep_until_local_time(9, 0).await;
            if let Some(state) = app3.try_state::<AppState>() {
                check_certificate_expiry(&state.db, &app3).await;
            }
        }
    });

    // Task 4: Cleanup audit log (every 7 days)
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(7 * 24 * 3600));
        loop {
            interval.tick().await;
            if let Some(state) = app4.try_state::<AppState>() {
                cleanup_audit_log(state.db.clone()).await;
            }
        }
    });

    // Task 5: Archive check (every 30 days)
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30 * 24 * 3600));
        loop {
            interval.tick().await;
            if let Some(state) = app5.try_state::<AppState>() {
                archive_check(state.db.clone(), app5.clone()).await;
            }
        }
    });

    // Task 6: Refresh expiring OAuth tokens — daily at 03:00 local time
    tauri::async_runtime::spawn(async move {
        loop {
            sleep_until_local_time(3, 0).await;
            if let Some(state) = app6.try_state::<AppState>() {
                refresh_expiring_certificates(&state.db, &app6).await;
            }
        }
    });

    // Task 7: Generate recurring invoices — daily at 08:00 local time
    tauri::async_runtime::spawn(async move {
        loop {
            sleep_until_local_time(8, 0).await;
            if let Some(state) = app7.try_state::<AppState>() {
                if let Err(e) = process_recurring_invoices(&state.db, &app7).await {
                    tracing::warn!("Recurring invoice processing error: {:?}", e);
                }
            }
        }
    });

    // Task 8: One-shot crash recovery — reset QUEUED invoices with no upload_id
    let app8 = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(state) = app8.try_state::<AppState>() {
            recover_stuck_queued_invoices(app8.clone(), state.db.clone()).await;
        }
    });
}

/// Dorme până la ora locală specificată (HH:MM) din ziua curentă sau mâine.
async fn sleep_until_local_time(hour: u32, minute: u32) {
    use chrono::{Local, NaiveTime};
    let now = Local::now();
    let target_time = NaiveTime::from_hms_opt(hour, minute, 0)
        .unwrap_or_else(|| NaiveTime::from_hms_opt(4, 0, 0).expect("04:00 is a valid time — constant infallible"));
    let mut target = now.date_naive().and_time(target_time)
        .and_local_timezone(Local)
        .single()
        .unwrap_or(now);
    if target <= now {
        target = target + chrono::Duration::days(1);
    }
    let duration = (target - now)
        .to_std()
        .unwrap_or(Duration::from_secs(3600));
    tokio::time::sleep(duration).await;
}

async fn poll_submitted_invoices(
    app: &AppHandle,
    state: &AppState,
) -> crate::error::AppResult<()> {
    let pool = &state.db;
    let companies = crate::db::companies::list(pool).await?;

    for company in companies {
        // Only proceed if a token exists for this company
        if crate::anaf::keychain::TokenBundle::load(&company.id).is_none() {
            continue;
        }

        if let Err(e) = poll_submitted_for_company(pool, &company.id, Some(app)).await {
            tracing::warn!("poll_submitted_for_company error for {}: {:?}", company.id, e);
        }
    }

    Ok(())
}

async fn sync_spv_messages(
    app: &AppHandle,
    state: &AppState,
) -> crate::error::AppResult<()> {
    let pool = &state.db;
    let test_mode = crate::db::settings::get_bool(
        pool,
        crate::db::settings::keys::USE_ANAF_TEST_ENV,
        false,
    ).await.unwrap_or(false);
    let companies = crate::db::companies::list(pool).await?;

    for company in companies {
        // Only proceed if a token exists for this company
        if crate::anaf::keychain::TokenBundle::load(&company.id).is_none() {
            continue;
        }

        // Re-use the same sync logic as the anaf_sync_spv command
        match do_sync_spv(pool, &company.id, app, test_mode).await {
            Ok(new_count) => {
                if new_count > 0 {
                    if let Err(e) = app.emit("spv://new-messages", new_count) {
                        tracing::warn!("Failed to emit spv://new-messages: {:?}", e);
                    }
                    crate::notifications::notify_new_received(app, new_count as u32).await;
                }
            }
            Err(e) => {
                tracing::warn!("SPV sync failed for company {}: {:?}", company.id, e);
            }
        }
    }

    Ok(())
}

/// Reîmprospătează token-ul OAuth2 pentru o companie și îl salvează în keychain.
/// Returnează noul access_token.
async fn refresh_token_for(company_id: &str) -> Result<String, String> {
    use crate::anaf::{keychain::TokenBundle, oauth};
    let bundle = TokenBundle::load(company_id)
        .ok_or_else(|| format!("Nu există token pentru compania {}", company_id))?;
    let result = oauth::refresh_token_bundle(&bundle.refresh_token)
        .await
        .map_err(|e| e.to_string())?;
    let new_bundle = TokenBundle {
        access_token: result.access_token.clone(),
        refresh_token: result.refresh_token,
        expires_at: result.expires_at,
    };
    new_bundle
        .save(company_id)
        .map_err(|e| format!("Keychain save eșuat: {e}"))?;
    Ok(result.access_token)
}

/// Polls ANAF status for all SUBMITTED invoices of a single company.
/// Returns the number of invoices whose status was checked.
/// Pass `app` to fire native OS notifications on status changes.
pub(crate) async fn poll_submitted_for_company(
    pool: &sqlx::SqlitePool,
    company_id: &str,
    app: Option<&AppHandle>,
) -> crate::error::AppResult<u32> {
    use crate::anaf::{client::{AnafClient, ERR_UNAUTHORIZED}, keychain::TokenBundle, oauth};
    use crate::db::invoices as db_inv;
    use crate::error::AppError;

    let bundle = match TokenBundle::load(company_id) {
        Some(b) => b,
        None => return Ok(0),
    };

    let mut access_token = if !bundle.is_expired() {
        bundle.access_token.clone()
    } else {
        let result = oauth::refresh_token_bundle(&bundle.refresh_token)
            .await
            .map_err(AppError::Other)?;
        let new_bundle = TokenBundle {
            access_token: result.access_token.clone(),
            refresh_token: result.refresh_token,
            expires_at: result.expires_at,
        };
        new_bundle.save(company_id).map_err(|e| AppError::Other(e.to_string()))?;
        result.access_token
    };

    // Read test_mode from settings so background poll respects the same environment
    let test_mode = crate::db::settings::get_bool(
        pool,
        crate::db::settings::keys::USE_ANAF_TEST_ENV,
        false,
    ).await.unwrap_or(false);
    let client = AnafClient::new(test_mode);
    let submitted = db_inv::list_submitted(pool, company_id).await.unwrap_or_default();
    let mut count = 0u32;

    for invoice in &submitted {
        if let Some(upload_id) = &invoice.anaf_upload_id {
            count += 1;

            // Call check_status; if 401, refresh token once and retry.
            let mut result = client.check_status(&access_token, upload_id).await;
            if let Err(ref e) = result {
                if e == ERR_UNAUTHORIZED {
                    tracing::info!(company_id, "ANAF 401 — reîmprospătăm token și reîncercăm");
                    if let Ok(new_tok) = refresh_token_for(company_id).await {
                        access_token = new_tok;
                        result = client.check_status(&access_token, upload_id).await;
                    }
                }
            }

            if let Ok(status_resp) = result {
                let stare = status_resp.stare.as_str();
                if stare == "ok" {
                    let _ = db_inv::mark_validated(pool, &invoice.id, status_resp.index_incarcare).await;
                    if let Some(app) = app {
                        crate::notifications::notify_invoice_validated(app, &invoice.full_number).await;
                        // Emit reactive event for frontend
                        let _ = app.emit("invoice_status_changed", serde_json::json!({
                            "invoiceId": &invoice.id,
                            "newStatus": "VALIDATED"
                        }));
                    }
                } else if stare == "nok" || stare.contains("erori") {
                    let raw_reason = status_resp.descriere.or(status_resp.erori);
                    let friendly_reason: Option<String> = raw_reason.as_deref().map(|r| {
                        crate::anaf::errors::friendly_message_from_body(r)
                    });
                    if let Some(app) = app {
                        let reason_str = friendly_reason.as_deref().unwrap_or("Verificați detaliile");
                        crate::notifications::notify_invoice_rejected(app, &invoice.full_number, reason_str).await;
                        // Emit reactive event for frontend
                        let _ = app.emit("invoice_status_changed", serde_json::json!({
                            "invoiceId": &invoice.id,
                            "newStatus": "REJECTED"
                        }));
                    }
                    let _ = db_inv::mark_rejected(pool, &invoice.id, friendly_reason, None).await;
                }
            }
        }
    }

    Ok(count)
}

/// Verifică certificatele care expiră și trimite notificări la pragurile 30/14/7/1 zile.
async fn check_certificate_expiry(pool: &sqlx::SqlitePool, app: &AppHandle) {
    let now = chrono::Utc::now().timestamp();

    let certs = match crate::db::certificates::list_expiring(pool, 30).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("check_certificate_expiry: DB error: {:?}", e);
            return;
        }
    };

    for cert in certs {
        let days_left = (cert.expires_at - now) / 86_400;
        if days_left < 0 {
            continue; // already expired
        }

        // Notifică la praguri: 30, 14, 7, 1 zile — cu fereastră ±1 zi pentru a
        // prinde certificatul chiar dacă app-ul nu rula exact în ziua-prag.
        let in_threshold = [30i64, 14, 7, 1]
            .iter()
            .any(|&t| days_left >= t - 1 && days_left <= t + 1);
        if !in_threshold {
            continue;
        }

        let company_name = match crate::db::companies::get(pool, &cert.company_id).await {
            Ok(c) => c.legal_name,
            Err(_) => cert.company_id.clone(),
        };

        crate::notifications::notify_certificate_expiring(app, &company_name, days_left).await;
    }
}

/// Încearcă să reîmprospăteze silent token-urile OAuth2 care sunt expirate sau aproape
/// de expirare. Dacă refresh-ul eșuează (ex. certificat expirat), nu face nimic —
/// utilizatorul va fi notificat de `check_certificate_expiry`.
async fn refresh_expiring_certificates(pool: &sqlx::SqlitePool, app: &AppHandle) {
    use crate::anaf::{keychain::TokenBundle, oauth};

    let companies = match crate::db::companies::list(pool).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("refresh_expiring_certificates: DB error: {:?}", e);
            return;
        }
    };

    for company in &companies {
        let bundle = match TokenBundle::load(&company.id) {
            Some(b) => b,
            None => continue,
        };

        if !bundle.is_expired() {
            continue; // token is still valid
        }

        tracing::info!(company_id = company.id.as_str(), "Reîmprospătăm token OAuth2");
        match oauth::refresh_token_bundle(&bundle.refresh_token).await {
            Ok(refreshed) => {
                let new_bundle = TokenBundle {
                    access_token: refreshed.access_token,
                    refresh_token: refreshed.refresh_token,
                    expires_at: refreshed.expires_at,
                };
                if let Err(e) = new_bundle.save(&company.id) {
                    tracing::warn!("Nu s-a putut salva token reîmprospătat pentru {}: {e}", company.id);
                } else {
                    tracing::info!("Token reîmprospătat pentru compania {}", company.legal_name);
                    // Log to audit
                    let _ = sqlx::query(
                        "INSERT INTO audit_log (id, action, entity_type, entity_id, metadata, created_at) \
                         VALUES (?1, ?2, ?3, ?4, ?5, unixepoch())"
                    )
                    .bind(crate::db::models::new_id())
                    .bind("token_refreshed")
                    .bind("company")
                    .bind(&company.id)
                    .bind("{\"source\":\"background\"}")
                    .execute(pool)
                    .await;
                }
            }
            Err(e) => {
                tracing::warn!(
                    "Refresh token eșuat pentru compania {} ({}): {}",
                    company.legal_name, company.id, e
                );
                // Notify user — they need to re-authorize manually
                let title = format!("Re-autorizare necesară: {}", company.legal_name);
                let body = "Token-ul ANAF a expirat și nu a putut fi reîmprospătat automat. \
                            Mergeți la Setări → Certificate.".to_string();
                crate::notifications::notify(app, &title, &body).await;
            }
        }
    }
}

pub(crate) async fn do_sync_spv(
    pool: &sqlx::SqlitePool,
    company_id: &str,
    app: &AppHandle,
    test_mode: bool,
) -> crate::error::AppResult<i32> {
    use crate::anaf::{client::{AnafClient, ERR_UNAUTHORIZED}, keychain::TokenBundle, oauth};
    use crate::db::notifications;
    use crate::error::AppError;

    let bundle = TokenBundle::load(company_id)
        .ok_or_else(|| AppError::Other("No token".into()))?;

    let mut access_token = if !bundle.is_expired() {
        bundle.access_token.clone()
    } else {
        let result = oauth::refresh_token_bundle(&bundle.refresh_token)
            .await
            .map_err(AppError::Other)?;
        let new_bundle = TokenBundle {
            access_token: result.access_token.clone(),
            refresh_token: result.refresh_token,
            expires_at: result.expires_at,
        };
        new_bundle.save(company_id).map_err(|e| AppError::Other(e.to_string()))?;
        result.access_token
    };

    let company = crate::db::companies::get(pool, company_id).await?;
    let client = AnafClient::new(test_mode);

    // list_messages: handle 401 with one refresh+retry
    let mut messages_result = client.list_messages(&access_token, &company.cui, 60).await;
    if let Err(ref e) = messages_result {
        if e == ERR_UNAUTHORIZED {
            tracing::info!(company_id, "ANAF 401 on list_messages — reîmprospătăm token");
            if let Ok(new_tok) = refresh_token_for(company_id).await {
                access_token = new_tok;
                messages_result = client.list_messages(&access_token, &company.cui, 60).await;
            }
        }
    }
    let messages = messages_result.map_err(AppError::Other)?;

    // Resolve app data dir once for archive paths
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Other(e.to_string()))?;

    let mut new_count = 0i32;
    for msg in messages {
        let data_key = format!("spv_msg_{}", msg.id);
        let exists: bool = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM notifications WHERE data = ?1",
        )
        .bind(&data_key)
        .fetch_one(pool)
        .await
        .unwrap_or(0)
            > 0;

        if !exists {
            // ── Download the ZIP from ANAF (with 401 refresh-once) ───────
            let mut dl_result = client.download_message(&access_token, &msg.id).await;
            if let Err(ref e) = dl_result {
                if e == ERR_UNAUTHORIZED {
                    tracing::info!(msg_id = msg.id.as_str(), "ANAF 401 on download — reîmprospătăm token");
                    if let Ok(new_tok) = refresh_token_for(company_id).await {
                        access_token = new_tok;
                        dl_result = client.download_message(&access_token, &msg.id).await;
                    }
                }
            }
            let zip_bytes = match dl_result {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!("download_message {} failed: {}", msg.id, e);
                    // Still create notification so we know about the message
                    let title = format!("Mesaj SPV nou: {}", msg.tip);
                    let body = msg
                        .detalii
                        .clone()
                        .unwrap_or_else(|| format!("Mesaj primit la {}", msg.data_creare));
                    let _ = notifications::create(
                        pool,
                        notifications::CreateNotificationInput {
                            notification_type: "SPV_MESSAGE".into(),
                            title,
                            body,
                            data: Some(data_key),
                        },
                    )
                    .await;
                    let _ = app.emit("new_notification", serde_json::json!({}));
                    new_count += 1;
                    continue;
                }
            };

            // ── Extract XML from ZIP ──────────────────────────────────────
            let xml_content = match extract_xml_from_zip(&zip_bytes) {
                Some(x) => x,
                None => {
                    tracing::warn!("No XML found in ZIP for message {}", msg.id);
                    let title = format!("Mesaj SPV nou: {}", msg.tip);
                    let body = msg
                        .detalii
                        .clone()
                        .unwrap_or_else(|| format!("Mesaj primit la {}", msg.data_creare));
                    let _ = notifications::create(
                        pool,
                        notifications::CreateNotificationInput {
                            notification_type: "SPV_MESSAGE".into(),
                            title,
                            body,
                            data: Some(data_key),
                        },
                    )
                    .await;
                    let _ = app.emit("new_notification", serde_json::json!({}));
                    new_count += 1;
                    continue;
                }
            };

            // ── Save to archive ───────────────────────────────────────────
            let year = if msg.data_creare.len() >= 4 {
                &msg.data_creare[..4]
            } else {
                "0000"
            };
            // Sanitize msg.id (received from ANAF) to prevent path traversal.
            let safe_msg_id: String = msg.id
                .chars()
                .filter(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_'))
                .take(64)
                .collect();
            let safe_cui: String = company.cui
                .chars()
                .filter(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_'))
                .take(64)
                .collect();
            let safe_year: String = year
                .chars()
                .filter(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_'))
                .take(64)
                .collect();
            let archive_path = app_data_dir
                .join("archive")
                .join("received")
                .join(&safe_cui)
                .join(&safe_year)
                .join(&safe_msg_id);
            // Belt-and-suspenders: verify the resolved path stays under app_data_dir.
            if !archive_path.starts_with(&app_data_dir) {
                tracing::error!("Archive path escape attempt for msg {}: {:?}", msg.id, archive_path);
                continue;
            }
            if let Err(e) = std::fs::create_dir_all(&archive_path) {
                tracing::error!("Failed to create archive dir for msg {}: {}", msg.id, e);
                continue;
            }
            let xml_path = archive_path.join("invoice.xml");
            let zip_path = archive_path.join("original.zip");
            if let Err(e) = std::fs::write(&xml_path, &xml_content) {
                tracing::error!("Failed to write XML archive for msg {}: {}", msg.id, e);
                continue;
            }
            if let Err(e) = std::fs::write(&zip_path, &zip_bytes) {
                tracing::warn!("Failed to write ZIP archive for msg {}: {}", msg.id, e);
                // ZIP is supplementary; proceed even if it fails.
            }

            // ── Parse XML for basic info ──────────────────────────────────
            let (issuer_cui, issuer_name, total_amount, issue_date) =
                parse_received_xml(&xml_content);

            // ── Insert received_invoice row ───────────────────────────────
            let recv_id = crate::db::models::new_id();
            let now = chrono::Utc::now().timestamp();
            if let Err(e) = sqlx::query(
                "INSERT OR IGNORE INTO received_invoices \
                 (id, company_id, anaf_download_id, anaf_index, issuer_cui, issuer_name, \
                  series, number, total_amount, currency, issue_date, xml_path, status, \
                  downloaded_at, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'RON', ?10, ?11, 'NEW', ?12, ?12)",
            )
            .bind(&recv_id)
            .bind(company_id)
            .bind(&msg.id)
            .bind(&msg.id_solicitare)
            .bind(&issuer_cui)
            .bind(&issuer_name)
            .bind(Option::<String>::None) // series — NULL until extracted from XML
            .bind(Option::<String>::None) // number — NULL until extracted from XML
            .bind(&total_amount)
            .bind(&issue_date)
            .bind(xml_path.to_string_lossy().as_ref())
            .bind(now)
            .execute(pool)
            .await
            {
                tracing::error!(
                    error = ?e,
                    anaf_download_id = %msg.id,
                    issuer_cui = %issuer_cui,
                    "Failed to insert received_invoice — invoice will be lost unless re-downloaded from SPV"
                );
            }

            // ── Create SPV notification ───────────────────────────────────
            let title = format!("Factură primită de la {}", issuer_name);
            let body = format!(
                "Sumă: {} RON — {}",
                total_amount, issue_date
            );

            notifications::create(
                pool,
                notifications::CreateNotificationInput {
                    notification_type: "SPV_MESSAGE".into(),
                    title,
                    body,
                    data: Some(data_key),
                },
            )
            .await?;

            // Emit reactive event so frontend notification badge updates immediately
            let _ = app.emit("new_notification", serde_json::json!({}));

            new_count += 1;
        }
    }

    Ok(new_count)
}

// ─── ZIP + XML helpers ─────────────────────────────────────────────────────

fn extract_xml_from_zip(zip_bytes: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read;
    let cursor = std::io::Cursor::new(zip_bytes);
    let mut archive = zip::ZipArchive::new(cursor).ok()?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).ok()?;
        if file.name().ends_with(".xml") && !file.name().contains("semnatura") {
            let mut contents = Vec::new();
            file.read_to_end(&mut contents).ok()?;
            return Some(contents);
        }
    }
    None
}

/// Parsează XML-ul UBL primit de la ANAF folosind quick-xml (namespace-aware).
/// Extrage: CUI emitent, denumire emitent, valoare totală, dată emisie.
fn parse_received_xml(xml_bytes: &[u8]) -> (String, String, String, String) {
    use quick_xml::events::Event;
    use quick_xml::Reader;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    // Strip UTF-8 BOM dacă există
    let xml_str = String::from_utf8_lossy(xml_bytes);
    let xml_str = xml_str.trim_start_matches('\u{FEFF}');

    let mut reader = Reader::from_str(xml_str);
    reader.config_mut().trim_text(true);

    let mut issuer_cui = String::new();
    let mut issuer_name = String::new();
    let mut total_amount_str = "0.00".to_string();
    let mut issue_date = chrono::Utc::now().format("%Y-%m-%d").to_string();

    // State machine pentru navigarea structurii UBL
    let mut depth_supplier = 0i32;      // >0 când suntem în AccountingSupplierParty
    let mut depth_party_tax = 0i32;     // >0 când suntem în PartyTaxScheme (al supplier)
    let mut depth_party_legal = 0i32;   // >0 când suntem în PartyLegalEntity (al supplier)
    let mut current_local = String::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let local = std::str::from_utf8(e.local_name().into_inner())
                    .unwrap_or("")
                    .to_string();
                match local.as_str() {
                    "AccountingSupplierParty" => depth_supplier += 1,
                    "PartyTaxScheme" if depth_supplier > 0 => depth_party_tax += 1,
                    "PartyLegalEntity" if depth_supplier > 0 => depth_party_legal += 1,
                    _ => {}
                }
                current_local = local;
            }
            Ok(Event::End(ref e)) => {
                let local = std::str::from_utf8(e.local_name().into_inner()).unwrap_or("");
                match local {
                    "AccountingSupplierParty" => { depth_supplier -= 1; }
                    "PartyTaxScheme" if depth_supplier > 0 => { depth_party_tax -= 1; }
                    "PartyLegalEntity" if depth_supplier > 0 => { depth_party_legal -= 1; }
                    _ => {}
                }
                current_local.clear();
            }
            Ok(Event::Text(ref e)) => {
                let text = match e.unescape() {
                    Ok(t) => t.trim().to_string(),
                    Err(_) => continue,
                };
                if text.is_empty() { continue; }
                match current_local.as_str() {
                    // CompanyID în PartyTaxScheme al furnizorului = CUI fiscal
                    "CompanyID" if depth_supplier > 0 && depth_party_tax > 0 => {
                        if issuer_cui.is_empty() { issuer_cui = text; }
                    }
                    // RegistrationName în PartyLegalEntity al furnizorului = denumire
                    "RegistrationName" if depth_supplier > 0 && depth_party_legal > 0 => {
                        if issuer_name.is_empty() { issuer_name = text; }
                    }
                    // PayableAmount = valoarea totală de plată
                    "PayableAmount" => {
                        if let Ok(d) = Decimal::from_str(text.trim()) {
                            total_amount_str = d.round_dp(2).to_string();
                        }
                    }
                    // IssueDate = data emiterii
                    "IssueDate" => {
                        issue_date = text;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }

    (
        if issuer_cui.is_empty() { "NECUNOSCUT".to_string() } else { issuer_cui },
        if issuer_name.is_empty() { "Necunoscut".to_string() } else { issuer_name },
        total_amount_str,
        issue_date,
    )
}

/// Generate invoices for all active recurring templates whose next_issue_date is today or earlier.
/// One invoice is created per template per run (even if multiple periods were missed).
/// next_issue_date is advanced through ALL missed periods so it lands in the future.
async fn process_recurring_invoices(
    pool: &sqlx::SqlitePool,
    app: &AppHandle,
) -> crate::error::AppResult<()> {
    use crate::db::recurring;
    use crate::db::models::new_id;
    use chrono::Local;
    use rust_decimal::Decimal;
    use rust_decimal::prelude::ToPrimitive;

    let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
    let hundred = Decimal::from(100u32);

    let due = recurring::list_due(pool).await?;

    for template in due {
        // Parse lines_json
        let lines: Vec<serde_json::Value> = match serde_json::from_str(&template.lines_json) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(
                    error = ?e,
                    template_id = %template.id,
                    "Failed to parse lines_json for recurring template — skipping"
                );
                continue;
            }
        };

        if lines.is_empty() {
            tracing::warn!(template_id = %template.id, "Recurring template has no lines — skipping");
            continue;
        }

        let mut tx = match pool.begin().await {
            Ok(t) => t,
            Err(e) => {
                tracing::error!(error = ?e, template_id = %template.id, "Failed to begin transaction — skipping");
                continue;
            }
        };

        // Allocate invoice number atomically by bumping companies.last_invoice_number
        if let Err(e) = sqlx::query(
            "UPDATE companies SET last_invoice_number = last_invoice_number + 1 WHERE id = ?1",
        )
        .bind(&template.company_id)
        .execute(&mut *tx)
        .await
        {
            tracing::error!(error = ?e, template_id = %template.id, "Failed to allocate invoice number — skipping");
            continue;
        }

        let allocated_number: i64 = match sqlx::query_scalar(
            "SELECT last_invoice_number FROM companies WHERE id = ?1",
        )
        .bind(&template.company_id)
        .fetch_one(&mut *tx)
        .await
        {
            Ok(n) => n,
            Err(e) => {
                tracing::error!(error = ?e, template_id = %template.id, "Failed to read allocated number — skipping");
                continue;
            }
        };

        let invoice_id = new_id();
        let full_number = format!("{}-{:04}", template.series, allocated_number);
        let issue_date = today.clone();
        let due_date = (Local::now() + chrono::Duration::days(30))
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();

        // Calculate totals from lines using Decimal
        let mut subtotal_dec = Decimal::ZERO;
        let mut vat_total_dec = Decimal::ZERO;

        struct LineCalc {
            name: String,
            description: Option<String>,
            quantity: String,
            unit: String,
            unit_price: String,
            vat_rate: String,
            vat_category: String,
            subtotal: String,
            vat_amount: String,
            total_amount: String,
        }

        let mut line_calcs: Vec<LineCalc> = Vec::with_capacity(lines.len());

        for line in &lines {
            let name = line["name"].as_str()
                .or_else(|| line["description"].as_str())
                .unwrap_or("Servicii")
                .to_string();
            let description = line["description"].as_str().map(|s| s.to_string());
            let unit = line["unit"].as_str().unwrap_or("BUC").to_string();
            let vat_category = line["vatCategory"].as_str().unwrap_or("S").to_string();

            let qty = line["quantity"].as_f64()
                .and_then(|v| Decimal::try_from(v).ok())
                .unwrap_or(Decimal::ONE);
            let price = line["unitPrice"].as_str()
                .and_then(|s| s.parse::<Decimal>().ok())
                .or_else(|| line["unitPrice"].as_f64().and_then(|v| Decimal::try_from(v).ok()))
                .unwrap_or(Decimal::ZERO);
            let vat_rate = if let Some(n) = line["vatRate"].as_i64() {
                if !crate::db::models::VALID_VAT_RATES.contains(&n) {
                    tracing::warn!(
                        template_id = %template.id,
                        vat_rate = n,
                        "Recurring invoice: invalid VAT rate in template, skipping line"
                    );
                    continue;
                }
                Decimal::from(n)
            } else if let Some(s) = line["vatRate"].as_str() {
                match s.parse::<Decimal>() {
                    Ok(d) => {
                        let rounded = d.round_dp(0).to_i64().unwrap_or(-1);
                        if !crate::db::models::VALID_VAT_RATES.contains(&rounded) {
                            tracing::warn!(
                                template_id = %template.id,
                                vat_rate = s,
                                "Recurring invoice: invalid VAT rate in template, skipping line"
                            );
                            continue;
                        }
                        d
                    }
                    Err(_) => {
                        tracing::warn!(
                            template_id = %template.id,
                            "Recurring invoice: unparseable VAT rate in template, skipping line"
                        );
                        continue;
                    }
                }
            } else {
                tracing::warn!(
                    template_id = %template.id,
                    "Recurring invoice: missing vatRate in template line, skipping"
                );
                continue;
            };

            let ls = (qty * price).round_dp(2);
            let lv = (ls * vat_rate / hundred).round_dp(2);
            let lt = ls + lv;
            subtotal_dec += ls;
            vat_total_dec += lv;

            line_calcs.push(LineCalc {
                name,
                description,
                quantity: qty.round_dp(2).to_string(),
                unit,
                unit_price: price.round_dp(2).to_string(),
                vat_rate: vat_rate.round_dp(2).to_string(),
                vat_category,
                subtotal: ls.round_dp(2).to_string(),
                vat_amount: lv.round_dp(2).to_string(),
                total_amount: lt.round_dp(2).to_string(),
            });
        }

        let subtotal = subtotal_dec.round_dp(2).to_string();
        let vat_total = vat_total_dec.round_dp(2).to_string();
        let total = (subtotal_dec + vat_total_dec).round_dp(2).to_string();

        let now_unix = chrono::Utc::now().timestamp();

        // Insert invoice header
        let insert_result = sqlx::query(
            "INSERT INTO invoices (
                id, company_id, contact_id, series, number, full_number,
                issue_date, due_date, currency, exchange_rate,
                subtotal_amount, vat_amount, total_amount, status, notes,
                payment_means_code, created_at, updated_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                ?7, ?8, 'RON', NULL,
                ?9, ?10, ?11, 'DRAFT', ?12,
                '30', ?13, ?13
            )",
        )
        .bind(&invoice_id)
        .bind(&template.company_id)
        .bind(&template.client_id)
        .bind(&template.series)
        .bind(allocated_number)
        .bind(&full_number)
        .bind(&issue_date)
        .bind(&due_date)
        .bind(&subtotal)
        .bind(&vat_total)
        .bind(&total)
        .bind(template.notes.as_deref().unwrap_or(""))
        .bind(now_unix)
        .execute(&mut *tx)
        .await;

        if let Err(e) = insert_result {
            tracing::error!(error = ?e, template_id = %template.id, "Failed to insert recurring invoice header — skipping");
            continue; // tx drops, auto-rolls-back
        }

        // Insert line items
        let mut lines_ok = true;
        for (i, lc) in line_calcs.iter().enumerate() {
            let line_id = new_id();
            if let Err(e) = sqlx::query(
                "INSERT INTO invoice_line_items (
                    id, invoice_id, position, name, description,
                    quantity, unit, unit_price, vat_rate, vat_category,
                    subtotal_amount, vat_amount, total_amount, cpv_code
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5,
                    ?6, ?7, ?8, ?9, ?10,
                    ?11, ?12, ?13, NULL
                )",
            )
            .bind(&line_id)
            .bind(&invoice_id)
            .bind((i as i64) + 1)
            .bind(&lc.name)
            .bind(&lc.description)
            .bind(&lc.quantity)
            .bind(&lc.unit)
            .bind(&lc.unit_price)
            .bind(&lc.vat_rate)
            .bind(&lc.vat_category)
            .bind(&lc.subtotal)
            .bind(&lc.vat_amount)
            .bind(&lc.total_amount)
            .execute(&mut *tx)
            .await
            {
                tracing::error!(error = ?e, template_id = %template.id, "Failed to insert line item — aborting template");
                lines_ok = false;
                break;
            }
        }

        if !lines_ok {
            continue; // tx drops, auto-rolls-back
        }

        // Insert CREATED event
        let _ = sqlx::query(
            "INSERT INTO invoice_events (id, invoice_id, event_type, message, created_at)
             VALUES (?1, ?2, 'CREATED', 'Factură creată automat din șablon recurent', ?3)",
        )
        .bind(new_id())
        .bind(&invoice_id)
        .bind(now_unix)
        .execute(&mut *tx)
        .await;

        // Advance next_issue_date through all missed periods until it's in the future
        let mut current_date = template.next_issue_date.clone();
        loop {
            let next = recurring::advance_date(
                &current_date,
                &template.frequency,
                template.day_of_month as u32,
            );
            if next > today {
                // This is the correct next future date — write it
                if let Err(e) = sqlx::query(
                    "UPDATE recurring_invoices SET next_issue_date = ?1, updated_at = unixepoch() WHERE id = ?2",
                )
                .bind(&next)
                .bind(&template.id)
                .execute(&mut *tx)
                .await
                {
                    tracing::error!(error = ?e, template_id = %template.id, "Failed to advance next_issue_date — aborting template");
                    lines_ok = false; // reuse flag to skip commit
                }
                break;
            }
            current_date = next;
        }

        if !lines_ok {
            continue; // tx drops, auto-rolls-back
        }

        // Commit
        if let Err(e) = tx.commit().await {
            tracing::error!(error = ?e, template_id = %template.id, "Failed to commit recurring invoice transaction");
            continue;
        }

        tracing::info!(
            invoice_id = %invoice_id,
            full_number = %full_number,
            template_id = %template.id,
            template_name = %template.template_name,
            "Generated recurring invoice"
        );

        // Notify frontend
        let _ = app.emit(
            "recurring_invoice_generated",
            serde_json::json!({
                "invoiceId": invoice_id,
                "templateId": template.id,
                "templateName": template.template_name,
                "fullNumber": full_number,
            }),
        );
    }

    Ok(())
}

async fn cleanup_audit_log(pool: sqlx::SqlitePool) {
    let two_years_ago = chrono::Utc::now().timestamp() - (2 * 365 * 24 * 3600);
    let _ = sqlx::query("DELETE FROM audit_log WHERE created_at < ?1")
        .bind(two_years_ago)
        .execute(&pool)
        .await;
    tracing::info!("Audit log cleanup done");
}

/// Crash recovery: on startup, find invoices stuck in QUEUED with no anaf_upload_id
/// (meaning the app crashed after the ANAF upload succeeded but before mark_submitted ran).
/// Any such invoice older than 10 minutes is reset to DRAFT so it can be retried.
async fn recover_stuck_queued_invoices(app: tauri::AppHandle, db: sqlx::SqlitePool) {
    // Brief delay so the DB pool is fully warmed up before we query it.
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    let sql = "SELECT id
               FROM invoices
               WHERE status = 'QUEUED'
                 AND (anaf_upload_id IS NULL OR anaf_upload_id = '')
                 AND updated_at < (unixepoch() - 600)";

    let rows = match sqlx::query(sql).fetch_all(&db).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Recuperare facturi blocate: query eșuat: {e}");
            return;
        }
    };

    if rows.is_empty() {
        return;
    }

    use sqlx::Row;
    for row in rows {
        let invoice_id: String = match row.try_get("id") {
            Ok(v) => v,
            Err(_) => continue,
        };

        let update_sql =
            "UPDATE invoices SET status = 'DRAFT', updated_at = unixepoch() WHERE id = ?1";
        if let Err(e) = sqlx::query(update_sql).bind(&invoice_id).execute(&db).await {
            tracing::error!("Recuperare factura {invoice_id}: update eșuat: {e}");
            continue;
        }

        let event_id = crate::db::models::new_id();
        let event_sql =
            "INSERT INTO invoice_events (id, invoice_id, event_type, message, metadata, created_at)
             VALUES (?1, ?2, ?3, ?4, NULL, unixepoch())";
        let _ = sqlx::query(event_sql)
            .bind(&event_id)
            .bind(&invoice_id)
            .bind("RECOVERED_FROM_QUEUED")
            .bind("Factura resetata la DRAFT dupa esec de incarcare ANAF (crash recovery)")
            .execute(&db)
            .await;

        tracing::warn!("Factura {invoice_id} recuperata: QUEUED → DRAFT (crash recovery)");

        let _ = app.emit(
            "invoice_status_changed",
            serde_json::json!({
                "invoice_id": invoice_id,
                "new_status": "DRAFT",
                "reason": "recovery",
            }),
        );
    }
}

async fn archive_check(pool: sqlx::SqlitePool, app: AppHandle) {
    use sqlx::Row;

    let rows = sqlx::query("SELECT xml_path FROM invoices WHERE xml_path IS NOT NULL")
        .fetch_all(&pool)
        .await
        .unwrap_or_default();

    let missing: Vec<String> = rows.iter()
        .filter_map(|r| r.try_get::<String, _>("xml_path").ok())
        .filter(|p| !std::path::Path::new(p).exists())
        .collect();

    if !missing.is_empty() {
        let body = format!("{} fișiere XML lipsesc din arhivă.", missing.len());
        crate::notifications::notify(&app, "Verificare arhivă", &body).await;
    }
}
