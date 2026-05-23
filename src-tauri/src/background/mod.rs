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
                    .bind(uuid::Uuid::now_v7().to_string())
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
                    .bind(uuid::Uuid::now_v7().to_string())
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
}

/// Dorme până la ora locală specificată (HH:MM) din ziua curentă sau mâine.
async fn sleep_until_local_time(hour: u32, minute: u32) {
    use chrono::{Local, NaiveTime};
    let now = Local::now();
    let target_time = NaiveTime::from_hms_opt(hour, minute, 0)
        .unwrap_or_else(|| NaiveTime::from_hms_opt(4, 0, 0).unwrap());
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
    let companies = crate::db::companies::list(pool).await?;

    for company in companies {
        // Only proceed if a token exists for this company
        if crate::anaf::keychain::TokenBundle::load(&company.id).is_none() {
            continue;
        }

        // Re-use the same sync logic as the anaf_sync_spv command
        match do_sync_spv(pool, &company.id, app).await {
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

    let client = AnafClient::new(false);
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

        // Notify only at tier thresholds to avoid daily spam
        if ![30i64, 14, 7, 1].contains(&days_left) {
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
                    .bind(uuid::Uuid::now_v7().to_string())
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
    let client = AnafClient::new(false);

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
            let archive_path = app_data_dir
                .join("archive")
                .join("received")
                .join(&company.cui)
                .join(year)
                .join(&msg.id);
            std::fs::create_dir_all(&archive_path).ok();
            let xml_path = archive_path.join("invoice.xml");
            let zip_path = archive_path.join("original.zip");
            std::fs::write(&xml_path, &xml_content).ok();
            std::fs::write(&zip_path, &zip_bytes).ok();

            // ── Parse XML for basic info ──────────────────────────────────
            let (issuer_cui, issuer_name, total_amount, issue_date) =
                parse_received_xml(&xml_content);

            // ── Insert received_invoice row ───────────────────────────────
            let recv_id = uuid::Uuid::now_v7().to_string();
            let now = chrono::Utc::now().timestamp();
            let _ = sqlx::query(
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
            .bind("")      // series — parse from XML if available
            .bind(&msg.id) // number fallback to message id
            .bind(total_amount)
            .bind(&issue_date)
            .bind(xml_path.to_string_lossy().as_ref())
            .bind(now)
            .execute(pool)
            .await;

            // ── Create SPV notification ───────────────────────────────────
            let title = format!("Factură primită de la {}", issuer_name);
            let body = format!(
                "Sumă: {:.2} RON — {}",
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

fn parse_received_xml(xml_bytes: &[u8]) -> (String, String, f64, String) {
    let xml = String::from_utf8_lossy(xml_bytes);

    let issuer_cui = extract_xml_value(&xml, "CompanyID")
        .or_else(|| extract_xml_value(&xml, "cbc:CompanyID"))
        .unwrap_or_else(|| "NECUNOSCUT".to_string());

    let issuer_name = extract_xml_value(&xml, "RegistrationName")
        .or_else(|| extract_xml_value(&xml, "cbc:RegistrationName"))
        .unwrap_or_else(|| "Necunoscut".to_string());

    let total_str = extract_xml_value(&xml, "PayableAmount")
        .or_else(|| extract_xml_value(&xml, "cbc:PayableAmount"))
        .unwrap_or_else(|| "0".to_string());
    let total_amount: f64 = total_str.trim().parse().unwrap_or(0.0);

    let issue_date = extract_xml_value(&xml, "IssueDate")
        .or_else(|| extract_xml_value(&xml, "cbc:IssueDate"))
        .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string());

    (issuer_cui, issuer_name, total_amount, issue_date)
}

fn extract_xml_value(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(xml[start..end].trim().to_string())
}

async fn cleanup_audit_log(pool: sqlx::SqlitePool) {
    let two_years_ago = chrono::Utc::now().timestamp() - (2 * 365 * 24 * 3600);
    let _ = sqlx::query("DELETE FROM audit_log WHERE created_at < ?1")
        .bind(two_years_ago)
        .execute(&pool)
        .await;
    tracing::info!("Audit log cleanup done");
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
