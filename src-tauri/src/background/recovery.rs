//! Startup recovery and periodic maintenance tasks.

use tauri::{AppHandle, Emitter, Manager};

/// Crash recovery: on startup, find invoices stuck in QUEUED with no anaf_upload_id
/// (meaning the app crashed after the ANAF upload succeeded but before mark_submitted ran).
/// Any such invoice older than 10 minutes is reset to DRAFT so it can be retried.
pub(crate) async fn recover_stale_queued(app: &AppHandle) {
    let db = match app.try_state::<crate::state::AppState>() {
        Some(s) => s.db.clone(),
        None => return,
    };

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

/// Verifică certificatele care expiră și trimite notificări la pragurile 30/14/7/1 zile.
pub(crate) async fn check_certificate_expiry(pool: &sqlx::SqlitePool, app: &AppHandle) {
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
pub(crate) async fn refresh_expiring_certificates(pool: &sqlx::SqlitePool, app: &AppHandle) {
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

        tracing::info!(
            company_id = company.id.as_str(),
            "Reîmprospătăm token OAuth2"
        );
        let config = crate::commands::anaf::build_oauth_config(pool).await;
        match oauth::refresh_token_bundle_with_client_id(
            &bundle.refresh_token,
            &config.client_id,
            &config.token_url,
        )
        .await
        {
            Ok(refreshed) => {
                let new_bundle = TokenBundle {
                    access_token: refreshed.access_token,
                    refresh_token: refreshed.refresh_token,
                    expires_at: refreshed.expires_at,
                };
                if let Err(e) = new_bundle.save(&company.id) {
                    tracing::warn!(
                        "Nu s-a putut salva token reîmprospătat pentru {}: {e}",
                        company.id
                    );
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
                    company.legal_name,
                    company.id,
                    e
                );
                // Notify user — they need to re-authorize manually
                let title = format!("Re-autorizare necesară: {}", company.legal_name);
                let body = "Token-ul ANAF a expirat și nu a putut fi reîmprospătat automat. \
                            Mergeți la Setări → Certificate."
                    .to_string();
                crate::notifications::notify(app, &title, &body).await;
            }
        }
    }
}

pub(crate) async fn cleanup_audit_log(pool: sqlx::SqlitePool) {
    let two_years_ago = chrono::Utc::now().timestamp() - (2 * 365 * 24 * 3600);
    let _ = sqlx::query("DELETE FROM audit_log WHERE created_at < ?1")
        .bind(two_years_ago)
        .execute(&pool)
        .await;
    tracing::info!("Audit log cleanup done");
}

pub(crate) async fn archive_check(pool: sqlx::SqlitePool, app: AppHandle) {
    use sqlx::Row;

    let rows = match sqlx::query("SELECT xml_path FROM invoices WHERE xml_path IS NOT NULL")
        .fetch_all(&pool)
        .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = ?e, "archive_check: failed to query invoices, skipping run");
            return;
        }
    };

    let missing: Vec<String> = rows
        .iter()
        .filter_map(|r| r.try_get::<String, _>("xml_path").ok())
        .filter(|p| !std::path::Path::new(p).exists())
        .collect();

    if !missing.is_empty() {
        let body = format!("{} fișiere XML lipsesc din arhivă.", missing.len());
        crate::notifications::notify(&app, "Verificare arhivă", &body).await;
    }
}
