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

async fn get_valid_token(company_id: &str) -> AppResult<String> {
    let bundle = TokenBundle::load(company_id)
        .ok_or_else(|| AppError::Other("Autentificați-vă la ANAF mai întâi.".into()))?;

    if !bundle.is_expired() {
        return Ok(bundle.access_token);
    }

    // Încearcă refresh
    let result = oauth::refresh_token_bundle(&bundle.refresh_token)
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
async fn build_oauth_config(pool: &sqlx::SqlitePool) -> oauth::OAuthConfig {
    use crate::db::settings;

    let mut cfg = oauth::OAuthConfig::default_prod();

    if let Ok(Some(v)) = settings::get(pool, "anaf_oauth_client_id").await {
        if !v.trim().is_empty() {
            cfg.client_id = v.trim().to_string();
        }
    }
    if let Ok(Some(v)) = settings::get(pool, "anaf_oauth_redirect_uri").await {
        if !v.trim().is_empty() {
            cfg.redirect_uri = v.trim().to_string();
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
        if !v.trim().is_empty() {
            cfg.authorize_url = v.trim().to_string();
        }
    }
    if let Ok(Some(v)) = settings::get(pool, "anaf_oauth_token_url").await {
        if !v.trim().is_empty() {
            cfg.token_url = v.trim().to_string();
        }
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
pub async fn anaf_logout(company_id: String) -> AppResult<()> {
    // Revocă token-ul la ANAF (best-effort) înainte de ștergerea din keychain.
    if let Some(bundle) = TokenBundle::load(&company_id) {
        oauth::revoke_token(&bundle.access_token).await;
    }
    TokenBundle::delete(&company_id);
    Ok(())
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
    let claim = sqlx::query(
        "UPDATE invoices SET status = 'QUEUED', updated_at = unixepoch() WHERE id = ?1 AND status = 'DRAFT'",
    )
    .bind(invoice_id)
    .execute(pool)
    .await
    .map_err(AppError::Database)?;
    if claim.rows_affected() != 1 {
        return Err(AppError::Validation(
            "Factura nu mai este în stadiu de ciornă (este posibil să fi fost deja trimisă)."
                .into(),
        ));
    }

    // 2. Încarcă factura + liniile din DB (acum cu status QUEUED)
    let invoice_with_lines = db_invoices::get_with_lines(pool, invoice_id).await?;
    let invoice = invoice_with_lines.invoice;
    let lines = invoice_with_lines.lines;

    // Guard: company_id trebuie să corespundă facturii
    if invoice.company_id.as_str() != company_id {
        return Err(AppError::Validation(
            "Factura nu aparține companiei selectate.".into(),
        ));
    }

    // 2. Încarcă compania + contactul din DB
    let company = companies::get(pool, company_id).await?;
    let buyer = contacts::get(pool, &invoice.contact_id).await?;

    // 3. Detectează dacă e factură storno și extrage referința originală.
    //    BIZ-13: sursa autoritativă este `invoices.storno_of_invoice_id`;
    //    parserul `STORNO_OF:{full_number}|{motiv}` din notes rămâne fallback
    //    pentru rândurile create înainte de migrația 0008.
    let storno_ref = resolve_storno_ref(pool, &invoice).await?;

    // 3. Generează XML UBL
    let xml_string = generate_ubl(&GeneratorInput {
        invoice: invoice.clone(),
        lines: lines.clone(),
        seller: company.clone(),
        buyer: buyer.clone(),
        storno_ref: storno_ref.clone(),
    })?;

    // 4. Validează — dacă sunt erori blocante, oprește
    let (data_errors, _data_warnings) =
        validate_invoice_data(&invoice, &lines, &company, &buyer, storno_ref.as_deref());
    if !data_errors.is_empty() {
        let msg = data_errors.join("; ");
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
    std::fs::create_dir_all(&archive_dir).map_err(AppError::Io)?;
    let xml_path = archive_dir.join("invoice.xml");
    std::fs::write(&xml_path, xml_string.as_bytes()).map_err(AppError::Io)?;
    // Actualizează xml_path în DB
    sqlx::query("UPDATE invoices SET xml_path = ?1, updated_at = unixepoch() WHERE id = ?2")
        .bind(xml_path.to_string_lossy().as_ref())
        .bind(invoice_id)
        .execute(pool)
        .await
        .map_err(AppError::Database)?;

    // Helper: revert invoice to DRAFT so the user can retry after any error.
    // Only reverts if still QUEUED — avoids accidentally overwriting a later status.
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

    // 6. Obține token valid din keychain (înainte de a schimba statusul)
    let mut token = match get_valid_token(company_id).await {
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
            if let Some(bundle) = TokenBundle::load(company_id) {
                if let Ok(refreshed) = oauth::refresh_token_bundle(&bundle.refresh_token).await {
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

    let token = get_valid_token(&company_id).await?;

    let client = AnafClient::new(test_mode);
    let status_resp = client
        .check_status(&token, &upload_id)
        .await
        .map_err(AppError::Other)?;

    let stare = status_resp.stare.clone();

    if stare == "ok" {
        db_invoices::mark_validated(pool, &invoice_id, status_resp.index_incarcare).await?;
    } else if stare == "nok" || stare.contains("erori") {
        let raw_reason = status_resp.descriere.or(status_resp.erori);
        let friendly_reason = raw_reason
            .as_deref()
            .map(crate::anaf::errors::friendly_message_from_body);
        db_invoices::mark_rejected(pool, &invoice_id, friendly_reason, None).await?;
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
