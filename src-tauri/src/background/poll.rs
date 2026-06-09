//! Background polling: check ANAF status for all SUBMITTED invoices.

use tauri::{AppHandle, Emitter, Manager};

/// Reîmprospătează token-ul OAuth2 pentru o companie și îl salvează în keychain.
/// Returnează noul access_token. Citește config-ul OAuth din DB pentru a folosi
/// client_id-ul configurat de utilizator (evită mismatch la ANAF cu client_id custom).
///
/// Serializat prin `lock` cu double-check: dacă alt task a reîmprospătat token-ul
/// cât timp așteptam lock-ul, returnăm token-ul proaspăt fără a apela ANAF din nou.
/// Refresh after a 401: `failed_token` is the access token ANAF just rejected. ANAF can 401 a
/// token that is still locally non-expired (revocation, cert change, clock skew), so we must NOT
/// short-circuit on is_expired here — instead, only reuse the keychain token if it CHANGED since
/// the failed attempt (another task already refreshed), otherwise force a real refresh.
pub(crate) async fn refresh_token_after_401(
    company_id: &str,
    pool: &sqlx::SqlitePool,
    lock: &tokio::sync::Mutex<()>,
    failed_token: &str,
) -> Result<String, String> {
    refresh_token_for_impl(company_id, pool, lock, Some(failed_token)).await
}

async fn refresh_token_for_impl(
    company_id: &str,
    pool: &sqlx::SqlitePool,
    lock: &tokio::sync::Mutex<()>,
    failed_token: Option<&str>,
) -> Result<String, String> {
    use crate::anaf::{keychain::TokenBundle, oauth};

    // Acquire the app-wide refresh lock to serialize concurrent callers.
    let _guard = lock.lock().await;

    // Double-check: re-load from keychain — another task may have refreshed while we waited.
    let bundle = TokenBundle::load(company_id)
        .ok_or_else(|| format!("Nu există token pentru compania {}", company_id))?;
    match failed_token {
        // 401 path: reuse only if another task already swapped the token; else force a refresh.
        Some(ft) if bundle.access_token != ft => return Ok(bundle.access_token),
        Some(_) => {}
        // Proactive path: skip the refresh if the token is still valid.
        None if !bundle.is_expired() => return Ok(bundle.access_token),
        None => {}
    }

    let config = crate::commands::anaf::build_oauth_config(pool).await;
    let result = oauth::refresh_token_bundle_with_client_id(
        &bundle.refresh_token,
        &config.client_id,
        &config.client_secret,
        &config.token_url,
    )
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

pub(crate) async fn poll_submitted_invoices(app: &AppHandle) -> crate::error::AppResult<()> {
    let state = app
        .try_state::<crate::state::AppState>()
        .ok_or_else(|| crate::error::AppError::Other("AppState not available".into()))?;
    let pool = &state.db;
    let lock = state.token_refresh_lock.clone();
    let companies = crate::db::companies::list(pool).await?;

    for company in companies {
        // Only proceed if a token exists for this company
        if crate::anaf::keychain::TokenBundle::load(&company.id).is_none() {
            continue;
        }

        if let Err(e) = poll_submitted_for_company(pool, &company.id, Some(app), &lock).await {
            tracing::warn!(
                "poll_submitted_for_company error for {}: {:?}",
                company.id,
                e
            );
        }
    }

    Ok(())
}

/// Polls ANAF status for all SUBMITTED invoices of a single company.
/// Returns the number of invoices whose status was checked.
/// Pass `app` to fire native OS notifications on status changes.
/// `lock` is the app-wide async mutex that serializes token refreshes.
pub(crate) async fn poll_submitted_for_company(
    pool: &sqlx::SqlitePool,
    company_id: &str,
    app: Option<&AppHandle>,
    lock: &tokio::sync::Mutex<()>,
) -> crate::error::AppResult<u32> {
    use crate::anaf::{
        client::{AnafClient, ERR_UNAUTHORIZED},
        keychain::TokenBundle,
        oauth,
    };
    use crate::db::invoices as db_inv;
    use crate::error::AppError;

    let bundle = match TokenBundle::load(company_id) {
        Some(b) => b,
        None => return Ok(0),
    };

    // Proactive-expiry path: refresh token if needed, with single-flight lock.
    let mut access_token = if !bundle.is_expired() {
        bundle.access_token.clone()
    } else {
        // Acquire lock; double-check after acquiring.
        let _guard = lock.lock().await;
        let bundle = match TokenBundle::load(company_id) {
            Some(b) => b,
            None => return Ok(0),
        };
        if !bundle.is_expired() {
            // Another task already refreshed while we waited.
            bundle.access_token.clone()
        } else {
            let config = crate::commands::anaf::build_oauth_config(pool).await;
            let result = oauth::refresh_token_bundle_with_client_id(
                &bundle.refresh_token,
                &config.client_id,
                &config.client_secret,
                &config.token_url,
            )
            .await
            .map_err(AppError::Other)?;
            let new_bundle = TokenBundle {
                access_token: result.access_token.clone(),
                refresh_token: result.refresh_token,
                expires_at: result.expires_at,
            };
            new_bundle
                .save(company_id)
                .map_err(|e| AppError::Other(e.to_string()))?;
            result.access_token
        }
    };

    // Read test_mode from settings so background poll respects the same environment
    let test_mode =
        crate::db::settings::get_bool(pool, crate::db::settings::keys::USE_ANAF_TEST_ENV, false)
            .await
            .unwrap_or(false);
    let client = AnafClient::new(test_mode);
    let submitted = match db_inv::list_submitted(pool, company_id).await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(
                company_id = %company_id,
                error = ?e,
                "Failed to list submitted invoices"
            );
            return Err(e);
        }
    };
    let mut count = 0u32;

    for invoice in &submitted {
        if let Some(upload_id) = &invoice.anaf_upload_id {
            count += 1;

            // Call check_status; if 401, refresh token once and retry.
            let mut result = client.check_status(&access_token, upload_id).await;
            if let Err(ref e) = result {
                if e == ERR_UNAUTHORIZED {
                    tracing::info!(company_id, "ANAF 401 — reîmprospătăm token și reîncercăm");
                    if let Ok(new_tok) =
                        refresh_token_after_401(company_id, pool, lock, &access_token).await
                    {
                        access_token = new_tok;
                        result = client.check_status(&access_token, upload_id).await;
                    }
                }
            }

            if let Ok(status_resp) = result {
                let stare = status_resp.stare.as_str();
                if stare == "ok" {
                    if let Err(e) =
                        db_inv::mark_validated(pool, &invoice.id, status_resp.index_incarcare).await
                    {
                        tracing::error!(
                            invoice_id = %invoice.id,
                            error = ?e,
                            "Failed to persist VALIDATED status after ANAF confirmation"
                        );
                    }
                    if let Some(app) = app {
                        crate::notifications::notify_invoice_validated(app, &invoice.full_number)
                            .await;
                        // Emit reactive event for frontend
                        let _ = app.emit(
                            "invoice_status_changed",
                            serde_json::json!({
                                "invoiceId": &invoice.id,
                                "newStatus": "VALIDATED"
                            }),
                        );
                    }
                } else if stare == "nok" || stare.contains("erori") {
                    let raw_reason = status_resp.descriere.or(status_resp.erori);
                    let friendly_reason: Option<String> = raw_reason
                        .as_deref()
                        .map(crate::anaf::errors::friendly_message_from_body);
                    if let Some(app) = app {
                        let reason_str =
                            friendly_reason.as_deref().unwrap_or("Verificați detaliile");
                        crate::notifications::notify_invoice_rejected(
                            app,
                            &invoice.full_number,
                            reason_str,
                        )
                        .await;
                        // Emit reactive event for frontend
                        let _ = app.emit(
                            "invoice_status_changed",
                            serde_json::json!({
                                "invoiceId": &invoice.id,
                                "newStatus": "REJECTED"
                            }),
                        );
                    }
                    if let Err(e) =
                        db_inv::mark_rejected(pool, &invoice.id, friendly_reason, None).await
                    {
                        tracing::error!(
                            invoice_id = %invoice.id,
                            error = ?e,
                            "Failed to persist REJECTED status after ANAF rejection"
                        );
                    }
                }
            }
        }
    }

    Ok(count)
}
