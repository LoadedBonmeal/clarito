//! Tauri commands pentru autentificarea ANAF OAuth2 și API-ul e-Factura.
//!
//! Toate comenzile sunt `async` — OAuth2 și HTTP calls sunt blocking I/O.

use crate::anaf::{client::AnafClient, keychain::TokenBundle, oauth};
use tauri::Manager;

use crate::commands::invoices::resolve_storno_ref;
use crate::db::models::new_id;
use crate::db::{companies, contacts, invoices as db_invoices};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::ubl::{
    generator::{generate_ubl, GeneratorInput},
    validator::validate_invoice_data,
};

// ─── Helper: sanitizare componentă de cale filesystem ─────────────────────

/// Permite doar caractere safe pentru name-uri de directoare/fișiere.
/// Elimină `..`, `/`, `\` și orice caracter în afară de [a-zA-Z0-9._-].
/// Limitează la 64 de caractere pentru a preveni path-uri excesiv de lungi.
fn sanitize_path_component(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .take(64)
        .collect()
}

// ─── Helper: obține token valid, încearcă refresh dacă e expirat ──────────

/// Obține un access_token valid pentru `company_id`.
///
/// Dacă token-ul e expirat, îl reîmprospătează folosind `client_id`-ul configurat
/// (citit din DB via `build_oauth_config`) pentru a evita mismatch-ul cu ANAF
/// la utilizatorii cu client_id custom.
///
/// Refresh-ul este serializat prin `lock` (app-wide async mutex) cu un double-check:
/// după achiziționarea lock-ului re-citim token-ul din keychain și re-testăm
/// `is_expired()` — dacă alt task a reîmprospătat între timp, sărim refresh-ul
/// și returnăm token-ul proaspăt. Astfel evităm `invalid_grant` de la ANAF când
/// două task-uri concurrent văd token-ul expirat simultan.
pub(crate) async fn get_valid_token(
    company_id: &str,
    pool: &sqlx::SqlitePool,
    lock: &tokio::sync::Mutex<()>,
) -> AppResult<String> {
    // Fast path: token is still valid — no lock needed.
    let bundle = TokenBundle::load(company_id)
        .ok_or_else(|| AppError::Other("Autentificați-vă la ANAF mai întâi.".into()))?;

    if !bundle.is_expired() {
        return Ok(bundle.access_token);
    }

    // Slow path: token expired — acquire the single-flight lock.
    let _guard = lock.lock().await;

    // Double-check: re-load from keychain; another task may have refreshed while
    // we were waiting for the lock.
    let bundle = TokenBundle::load(company_id)
        .ok_or_else(|| AppError::Other("Autentificați-vă la ANAF mai întâi.".into()))?;

    if !bundle.is_expired() {
        // Another task already refreshed — use the fresh token.
        return Ok(bundle.access_token);
    }

    // Still expired under the lock — we are responsible for refreshing.
    let config = build_oauth_config(pool).await;
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

    Ok(result.access_token)
}

// ─── Commands ──────────────────────────────────────────────────────────────

/// Construiește `OAuthConfig` citind setările opționale din DB.
/// Fallback la valorile implicite (prod ANAF) pentru orice cheie lipsă.
///
/// Chei suprascriibile:
/// - `anaf_oauth_client_id`      — client_id OAuth (implicit: "efactura-desktop")
/// - `anaf_oauth_redirect_uri`   — redirect URI   (implicit: "http://localhost:8787/callback")
/// - `anaf_oauth_callback_port`  — port TCP        (implicit: 8787)
/// - `anaf_oauth_authorize_url`  — URL autorizare  (implicit: prod ANAF)
/// - `anaf_oauth_token_url`      — URL token       (implicit: prod ANAF)
pub(crate) async fn build_oauth_config(pool: &sqlx::SqlitePool) -> oauth::OAuthConfig {
    use crate::db::settings;

    let mut cfg = oauth::OAuthConfig::default_prod();

    if let Ok(Some(v)) = settings::get(pool, "anaf_oauth_client_id").await {
        if !v.trim().is_empty() {
            cfg.client_id = v.trim().to_string();
        }
    }
    if let Ok(Some(v)) = settings::get(pool, "anaf_oauth_redirect_uri").await {
        let candidate = v.trim().to_string();
        if !candidate.is_empty() {
            if oauth::is_allowed_redirect_uri(&candidate) {
                cfg.redirect_uri = candidate;
            } else {
                tracing::warn!(
                    uri = %candidate,
                    "anaf_oauth_redirect_uri din setări nu este un loopback localhost valid \
                     (http(s)://localhost|127.0.0.1|[::1]) — se folosește URI-ul implicit"
                );
            }
        }
    }
    if let Ok(Some(v)) = settings::get(pool, "anaf_oauth_callback_port").await {
        if let Ok(port) = v.trim().parse::<u16>() {
            if port > 1024 {
                cfg.callback_port = port;
                // Actualizăm redirect_uri să reflecte portul dacă nu a fost suprascris explicit
                if cfg.redirect_uri == oauth::OAuthConfig::default_prod().redirect_uri {
                    cfg.redirect_uri = format!("http://localhost:{port}/callback");
                }
            }
        }
    }
    if let Ok(Some(v)) = settings::get(pool, "anaf_oauth_authorize_url").await {
        let candidate = v.trim().to_string();
        if !candidate.is_empty() {
            if oauth::is_allowed_anaf_url(&candidate) {
                cfg.authorize_url = candidate;
            } else {
                tracing::warn!(
                    url = %candidate,
                    "anaf_oauth_authorize_url din setări nu respectă allowlist-ul \
                     (https + *.anaf.ro) — se folosește URL-ul prod implicit"
                );
            }
        }
    }
    if let Ok(Some(v)) = settings::get(pool, "anaf_oauth_token_url").await {
        let candidate = v.trim().to_string();
        if !candidate.is_empty() {
            if oauth::is_allowed_anaf_url(&candidate) {
                cfg.token_url = candidate;
            } else {
                tracing::warn!(
                    url = %candidate,
                    "anaf_oauth_token_url din setări nu respectă allowlist-ul \
                     (https + *.anaf.ro) — se folosește URL-ul prod implicit"
                );
            }
        }
    }

    // client_secret-ul OAuth (client confidențial ANAF) — citit din OS keychain,
    // nu din tabela settings (este o credențială sensibilă).
    if let Ok(Some(secret)) = crate::anaf::keychain::get_oauth_client_secret() {
        cfg.client_secret = secret;
    }

    cfg
}

/// Pornește fluxul OAuth2 PKCE — deschide browser-ul, așteaptă callback.
/// Returnează `true` dacă autentificarea a reușit.
/// Emite evenimentul `oauth_completed` { companyId, success } pentru frontend.
#[tauri::command]
pub async fn anaf_authorize(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    company_id: String,
) -> AppResult<bool> {
    use tauri::Emitter;

    let config = build_oauth_config(&state.db).await;
    let result = oauth::authorize(&company_id, &config)
        .await
        .map_err(AppError::Validation)?;

    let bundle = TokenBundle {
        access_token: result.access_token,
        refresh_token: result.refresh_token,
        expires_at: result.expires_at,
    };
    bundle
        .save(&company_id)
        .map_err(|e| AppError::Other(e.to_string()))?;

    // Emit oauth_completed so frontend can react (refresh auth state)
    let _ = app.emit(
        "oauth_completed",
        serde_json::json!({
            "companyId": company_id,
            "success": true
        }),
    );

    Ok(true)
}

/// Verifică dacă există un token valid (ne-expirat) pentru această companie.
#[tauri::command]
pub async fn anaf_is_authenticated(company_id: String) -> AppResult<bool> {
    match TokenBundle::load(&company_id) {
        Some(bundle) => Ok(!bundle.is_expired()),
        None => Ok(false),
    }
}

/// Revocă token-ul la ANAF și îl șterge din keychain (logout).
#[tauri::command]
pub async fn anaf_logout(state: tauri::State<'_, AppState>, company_id: String) -> AppResult<()> {
    // Revocă token-ul la ANAF (best-effort) înainte de ștergerea din keychain.
    // Folosim client_id-ul configurat de utilizator pentru a evita mismatch-ul
    // la serverul de revocare ANAF.
    if let Some(bundle) = TokenBundle::load(&company_id) {
        let config = build_oauth_config(&state.db).await;
        oauth::revoke_token(
            &bundle.access_token,
            &config.client_id,
            &config.client_secret,
        )
        .await;
    }
    TokenBundle::delete(&company_id);
    Ok(())
}

/// Salvează (sau șterge, dacă e gol) client_secret-ul OAuth ANAF în OS keychain.
/// Secret-ul NU este niciodată citit înapoi în frontend — doar setat/șters.
#[tauri::command]
pub async fn anaf_set_oauth_client_secret(secret: String) -> AppResult<()> {
    let trimmed = secret.trim();
    if trimmed.is_empty() {
        crate::anaf::keychain::delete_oauth_client_secret()
    } else {
        crate::anaf::keychain::store_oauth_client_secret(trimmed)
    }
}

/// `true` dacă există un client_secret OAuth salvat în keychain (pentru a afișa
/// starea „configurat" în Setări, fără a expune valoarea).
#[tauri::command]
pub async fn anaf_has_oauth_client_secret() -> AppResult<bool> {
    Ok(crate::anaf::keychain::get_oauth_client_secret()?.is_some())
}

/// Trimite XML-ul facturii la ANAF. Returnează `index_incarcare` (upload ID).
#[tauri::command]
pub async fn anaf_submit_invoice(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    company_id: String,
    invoice_id: String,
    test_mode: bool,
) -> AppResult<String> {
    submit_invoice_inner(&app, &state.db, &company_id, &invoice_id, test_mode).await
}

pub(crate) async fn submit_invoice_inner(
    app: &tauri::AppHandle,
    pool: &sqlx::SqlitePool,
    company_id: &str,
    invoice_id: &str,
    test_mode: bool,
) -> AppResult<String> {
    // 1. Claim atomic DRAFT → QUEUED. Dacă alt apel concurrent a revendicat deja
    //    factura (sau statusul e altul decât DRAFT), respingem imediat — fără
    //    window de dublă trimitere.
    //    G2: company_id is bound as ?2 so a foreign-company DRAFT is never flipped
    //    to QUEUED — rows_affected == 0 for any wrong-company call, and the existing
    //    "not draft" error path covers the wrong-company case without any status mutation.
    let claim = sqlx::query(
        "UPDATE invoices SET status = 'QUEUED', updated_at = unixepoch() \
         WHERE id = ?1 AND status = 'DRAFT' AND company_id = ?2",
    )
    .bind(invoice_id)
    .bind(company_id)
    .execute(pool)
    .await
    .map_err(AppError::Database)?;
    if claim.rows_affected() != 1 {
        return Err(AppError::Validation(
            "Factura nu mai este în stadiu de ciornă (este posibil să fi fost deja trimisă)."
                .into(),
        ));
    }

    // Helper: revert invoice QUEUED → DRAFT so the user can retry after ANY
    // pre-submission error (validation, UBL generation, archive, token, upload).
    // Only reverts if still QUEUED — never overwrites a later status.
    let revert_to_draft = |pool: &sqlx::SqlitePool, invoice_id: &str| {
        let pool = pool.clone();
        let invoice_id = invoice_id.to_string();
        async move {
            let _ = sqlx::query(
                "UPDATE invoices SET status = 'DRAFT', updated_at = unixepoch() \
                 WHERE id = ?1 AND status = 'QUEUED'",
            )
            .bind(&invoice_id)
            .execute(&pool)
            .await
            .map_err(|e| {
                tracing::error!(error = ?e, %invoice_id, "Failed to revert invoice to DRAFT status after ANAF error");
            });
        }
    };

    // 2. Încarcă factura + liniile din DB (acum cu status QUEUED) — SEC-06: scoped la
    // company_id (filtru în query). Even if the claim SQL above has a bug, a foreign
    // invoice can't be fetched, let alone submitted.
    let invoice_with_lines =
        db_invoices::get_with_lines_scoped(pool, invoice_id, company_id).await?;
    let invoice = invoice_with_lines.invoice;
    let lines = invoice_with_lines.lines;

    // 2. Încarcă compania + contactul din DB
    let company = companies::get(pool, company_id).await?;
    let buyer = contacts::get(pool, &invoice.contact_id, &invoice.company_id).await?;

    // 3. Detectează dacă e factură storno și extrage referința originală.
    //    BIZ-13: sursa autoritativă este `invoices.storno_of_invoice_id`;
    //    parserul `STORNO_OF:{full_number}|{motiv}` din notes rămâne fallback
    //    pentru rândurile create înainte de migrația 0008.
    let storno_ref = resolve_storno_ref(pool, &invoice).await?;

    // 3. Generează XML UBL
    let xml_string = match generate_ubl(&GeneratorInput {
        invoice: invoice.clone(),
        lines: lines.clone(),
        seller: company.clone(),
        buyer: buyer.clone(),
        storno_ref: storno_ref.clone(),
    }) {
        Ok(xml) => xml,
        Err(e) => {
            revert_to_draft(pool, invoice_id).await;
            return Err(e);
        }
    };

    // 4. Validează — dacă sunt erori blocante, oprește
    let (data_errors, _data_warnings) =
        validate_invoice_data(&invoice, &lines, &company, &buyer, storno_ref.as_deref());
    if !data_errors.is_empty() {
        let msg = data_errors.join("; ");
        revert_to_draft(pool, invoice_id).await;
        return Err(AppError::Validation(msg));
    }

    // 5. Salvează XML în arhivă
    let year = if invoice.issue_date.len() >= 4 {
        &invoice.issue_date[..4]
    } else {
        "0000"
    };
    // Sanitizăm componentele de cale: permitem doar [a-zA-Z0-9._-], max 64 chars.
    // CUI-ul e deja în format RO + cifre; full_number e validat în BR-RO-025
    // (doar [a-zA-Z0-9-_]), dar aplicăm sanitizare defensivă oricum.
    let safe_cui = sanitize_path_component(&company.cui);
    let safe_year = sanitize_path_component(year);
    let safe_full_number = sanitize_path_component(&invoice.full_number);
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Other(e.to_string()))?;
    let archive_dir = base
        .join("archive")
        .join("sent")
        .join(&safe_cui)
        .join(&safe_year)
        .join(&safe_full_number);
    // Belt-and-suspenders: verificăm că path-ul rămâne sub base
    // (nu putem canonicaliza înainte de create_dir_all, dar verificăm prefix-ul direct)
    if !archive_dir.starts_with(&base) {
        return Err(AppError::Validation("Cale de arhivă invalidă".into()));
    }
    tokio::fs::create_dir_all(&archive_dir)
        .await
        .map_err(AppError::Io)?;
    let xml_path = archive_dir.join("invoice.xml");
    tokio::fs::write(&xml_path, xml_string.as_bytes())
        .await
        .map_err(AppError::Io)?;
    // Actualizează xml_path în DB
    sqlx::query("UPDATE invoices SET xml_path = ?1, updated_at = unixepoch() WHERE id = ?2")
        .bind(xml_path.to_string_lossy().as_ref())
        .bind(invoice_id)
        .execute(pool)
        .await
        .map_err(AppError::Database)?;

    // 6. Obține token valid din keychain (înainte de a schimba statusul)
    let refresh_lock = app
        .try_state::<AppState>()
        .map(|s| s.token_refresh_lock.clone())
        .unwrap_or_else(|| std::sync::Arc::new(tokio::sync::Mutex::new(())));
    let mut token = match get_valid_token(company_id, pool, &refresh_lock).await {
        Ok(t) => t,
        Err(e) => {
            revert_to_draft(pool, invoice_id).await;
            return Err(e);
        }
    };

    // 7. Upload la ANAF (cu un retry la 401)
    let client = AnafClient::new(test_mode);
    let xml_bytes = xml_string.into_bytes();
    let mut upload_result = client
        .upload_invoice(&token, &company.cui, xml_bytes.clone())
        .await;
    if let Err(ref e) = upload_result {
        use crate::anaf::client::ERR_UNAUTHORIZED;
        if e == ERR_UNAUTHORIZED {
            tracing::info!(company_id, "ANAF 401 on upload — reîmprospătăm token");
            use crate::anaf::{keychain::TokenBundle, oauth};
            // Acquire the same app-wide lock so we don't race with other 401-retry
            // or proactive-refresh paths.
            let _guard = refresh_lock.lock().await;
            // Double-check: re-load from keychain — another task may have refreshed.
            if let Some(bundle) = TokenBundle::load(company_id) {
                // Try the fresh bundle first.
                if !bundle.is_expired() {
                    token = bundle.access_token;
                    upload_result = client
                        .upload_invoice(&token, &company.cui, xml_bytes.clone())
                        .await;
                } else {
                    // Folosim client_id configurat (nu DEFAULT_CLIENT_ID) pentru refresh.
                    let cfg = build_oauth_config(pool).await;
                    if let Ok(refreshed) = oauth::refresh_token_bundle_with_client_id(
                        &bundle.refresh_token,
                        &cfg.client_id,
                        &cfg.client_secret,
                        &cfg.token_url,
                    )
                    .await
                    {
                        let new_bundle = TokenBundle {
                            access_token: refreshed.access_token.clone(),
                            refresh_token: refreshed.refresh_token,
                            expires_at: refreshed.expires_at,
                        };
                        if let Err(e) = new_bundle.save(company_id) {
                            tracing::error!(error = ?e, %company_id, "Failed to persist refreshed ANAF token bundle — user will be forced to re-authenticate");
                        }
                        token = refreshed.access_token;
                        upload_result = client
                            .upload_invoice(&token, &company.cui, xml_bytes.clone())
                            .await;
                    }
                }
            }
        }
    }
    let upload_resp = match upload_result {
        Ok(r) => r,
        Err(e) => {
            revert_to_draft(pool, invoice_id).await;
            return Err(AppError::Other(e));
        }
    };

    // Upload succeeded — status is already QUEUED (set atomically at the start).
    // Proceed to mark SUBMITTED via mark_submitted below.
    let upload_id = upload_resp.index_incarcare;

    // 9. Actualizează DB cu status SUBMITTED + anaf_upload_id
    db_invoices::mark_submitted(pool, invoice_id, &upload_id).await?;
    {
        use tauri::Emitter;
        let _ = app.emit(
            "invoice_status_changed",
            serde_json::json!({"invoiceId": invoice_id, "newStatus": "SUBMITTED"}),
        );
    }

    // 10. Inserează eveniment invoice_event
    let event_id = new_id();
    if let Err(e) = sqlx::query(
        "INSERT INTO invoice_events (id, invoice_id, event_type, message, created_at) \
         VALUES (?1, ?2, 'SUBMITTED_TO_ANAF', ?3, unixepoch())",
    )
    .bind(&event_id)
    .bind(invoice_id)
    .bind(format!("Factură trimisă la ANAF. Upload ID: {}", upload_id))
    .execute(pool)
    .await
    {
        tracing::warn!(error = ?e, %invoice_id, "Failed to insert invoice event for SUBMITTED_TO_ANAF");
    }

    // 11. Notificare
    crate::notifications::notify(
        app,
        "✓ Factură trimisă",
        &format!(
            "Factura {} a fost trimisă la ANAF. ID: {}",
            invoice.full_number, upload_id
        ),
    )
    .await;

    let _ = crate::db::audit::log_user_action(
        pool,
        "invoice_submitted_anaf",
        "invoice",
        invoice_id,
        Some(&invoice.company_id),
        Some(&upload_id),
    )
    .await;

    Ok(upload_id)
}

/// Verifică statusul ANAF al unei facturi trimise. Actualizează statusul DB.
/// Returnează `stare`-ul curent (ex. "ok", "in prelucrare", "nok").
#[tauri::command]
pub async fn anaf_check_invoice_status(
    state: tauri::State<'_, AppState>,
    company_id: String,
    invoice_id: String,
    test_mode: bool,
) -> AppResult<String> {
    let pool = &state.db;

    let invoice = db_invoices::get(pool, &invoice_id).await?;

    if invoice.company_id != company_id {
        return Err(AppError::Validation(
            "Factura nu aparține companiei selectate.".into(),
        ));
    }

    let upload_id = invoice
        .anaf_upload_id
        .ok_or_else(|| AppError::Validation("Factura nu are un upload ID ANAF.".into()))?;

    let token = get_valid_token(&company_id, pool, &state.token_refresh_lock).await?;

    let client = AnafClient::new(test_mode);

    // Mirror the retry pattern from poll.rs: on ERR_UNAUTHORIZED refresh token once
    // and retry the status check to avoid leaving invoices stuck as SUBMITTED.
    let mut check_result = client.check_status(&token, &upload_id).await;
    if let Err(ref e) = check_result {
        use crate::anaf::client::ERR_UNAUTHORIZED;
        if e == ERR_UNAUTHORIZED {
            tracing::info!(company_id, "ANAF 401 on check_status — reîmprospătăm token");
            if let Ok(new_tok) = crate::background::refresh_token_after_401(
                &company_id,
                pool,
                &state.token_refresh_lock,
                &token,
            )
            .await
            {
                check_result = client.check_status(&new_tok, &upload_id).await;
            }
        }
    }
    let status_resp = check_result.map_err(AppError::Other)?;

    let stare = status_resp.stare.clone();

    if stare == "ok" {
        db_invoices::mark_validated(pool, &invoice_id, &company_id, status_resp.index_incarcare)
            .await?;
    } else if stare == "nok" || stare.contains("erori") {
        let raw_reason = status_resp.descriere.or(status_resp.erori);
        let friendly_reason = raw_reason
            .as_deref()
            .map(crate::anaf::errors::friendly_message_from_body);
        db_invoices::mark_rejected(pool, &invoice_id, &company_id, friendly_reason, None).await?;
    }
    // "in prelucrare" — nu facem nimic

    Ok(stare)
}

/// Re-pornește fluxul OAuth2 pentru o companie (re-autorizare certificat).
///
/// NOTE: Aceasta lansează un flux complet browser OAuth în loc să apeleze
/// `refresh_token_bundle`. Motivul: certificatele ANAF expirate necesită
/// re-autentificare interactivă — token refresh nu e suficient după expirarea
/// certificatului digital. Token-urile curente sunt totuși refreshate automat
/// de `get_valid_token` înaintea fiecărei operații API.
#[tauri::command]
pub async fn anaf_refresh_certificate(
    state: tauri::State<'_, AppState>,
    company_id: String,
) -> AppResult<bool> {
    let config = build_oauth_config(&state.db).await;
    let result = oauth::authorize(&company_id, &config)
        .await
        .map_err(AppError::Validation)?;

    let bundle = TokenBundle {
        access_token: result.access_token,
        refresh_token: result.refresh_token,
        expires_at: result.expires_at,
    };
    bundle
        .save(&company_id)
        .map_err(|e| AppError::Other(e.to_string()))?;

    Ok(true)
}

/// Revocă certificatul SPV al unei companii — șterge token-ul și dezactivează certificatele.
#[tauri::command]
pub async fn anaf_revoke_certificate(
    state: tauri::State<'_, AppState>,
    company_id: String,
) -> AppResult<()> {
    let pool = &state.db;

    // Verify the company exists
    let _company = companies::get(pool, &company_id).await?;

    // Delete token from keychain
    TokenBundle::delete(&company_id);

    // Mark certificates inactive
    sqlx::query(
        "UPDATE certificates SET is_active = 0, updated_at = unixepoch() WHERE company_id = ?1",
    )
    .bind(&company_id)
    .execute(pool)
    .await
    .map_err(AppError::Database)?;

    // Update company spv_enabled = false
    sqlx::query("UPDATE companies SET spv_enabled = 0, updated_at = unixepoch() WHERE id = ?1")
        .bind(&company_id)
        .execute(pool)
        .await
        .map_err(AppError::Database)?;

    Ok(())
}

/// Returnează lista de certificate pentru o companie.
#[tauri::command]
pub async fn anaf_get_certificates(
    state: tauri::State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<crate::db::certificates::Certificate>> {
    crate::db::certificates::list_for_company(&state.db, &company_id).await
}

/// Sincronizează mesajele SPV pentru o companie. Returnează numărul de mesaje noi.
/// Descarcă automat facturile primite și le stochează în arhivă + DB.
#[tauri::command]
pub async fn anaf_sync_spv(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    company_id: String,
    test_mode: bool,
) -> AppResult<i32> {
    crate::background::do_sync_spv(&state.db, &company_id, &app, test_mode).await
}

/// One SPV-inbox item: the raw SPV message + its category bucket.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpvInboxItem {
    pub id: String,
    pub tip: String,
    pub data_creare: String,
    pub cif: String,
    pub id_solicitare: Option<String>,
    pub detalii: Option<String>,
    /// Inbox bucket: recipisa | notificare | somatie | decizie | factura | altele.
    pub category: &'static str,
}

/// Read the GENERAL SPV inbox (SPVWS2) — declaration recipise, notificări, somații, decizii —
/// distinct from the e-Factura sync. Read-only; ANAF provides no declaration-submission API
/// (D300/D394/D406 are uploaded manually in the SPV portal), so this surfaces the responses.
/// Live-only: requires a connected ANAF account; not reachable without credentials.
#[tauri::command]
pub async fn anaf_list_spv_inbox(
    state: tauri::State<'_, AppState>,
    company_id: String,
    days: u32,
    test_mode: bool,
) -> AppResult<Vec<SpvInboxItem>> {
    use crate::anaf::client::{classify_spv_tip, ERR_UNAUTHORIZED};
    let pool = &state.db;
    let company = companies::get(pool, &company_id).await?;
    let token = get_valid_token(&company_id, pool, &state.token_refresh_lock).await?;
    let client = AnafClient::new(test_mode);

    let days = days.clamp(1, 60);
    let mut result = client.list_spv_messages(&token, &company.cui, days).await;
    if let Err(ref e) = result {
        if e == ERR_UNAUTHORIZED {
            if let Ok(new_tok) = crate::background::refresh_token_after_401(
                &company_id,
                pool,
                &state.token_refresh_lock,
                &token,
            )
            .await
            {
                result = client.list_spv_messages(&new_tok, &company.cui, days).await;
            }
        }
    }
    let messages = result.map_err(AppError::Other)?;
    Ok(messages
        .into_iter()
        .map(|m| SpvInboxItem {
            category: classify_spv_tip(&m.tip),
            id: m.id,
            tip: m.tip,
            data_creare: m.data_creare,
            cif: m.cif,
            id_solicitare: m.id_solicitare,
            detalii: m.detalii,
        })
        .collect())
}
