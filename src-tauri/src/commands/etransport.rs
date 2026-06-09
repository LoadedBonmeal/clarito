//! RO e-Transport commands — validate + generate the UIT XML (pure, testable) and submit it via
//! the ANAF OAuth API (live). Unlike D300/D394, e-Transport has a real submission API.

use tauri::State;

use crate::anaf::client::{AnafClient, EtransportUploadResponse};
use crate::anaf_decl::etransport::{
    generate_etransport_xml, validate_etransport, EtransportDeclaration,
};
use crate::db::companies;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Validate an e-Transport declaration; returns the list of problems (empty = valid).
#[tauri::command]
pub async fn etransport_validate(declaration: EtransportDeclaration) -> AppResult<Vec<String>> {
    Ok(validate_etransport(&declaration))
}

/// Validate + build the e-Transport XML (schema v2). Errors if the declaration is invalid.
#[tauri::command]
pub async fn etransport_generate_xml(declaration: EtransportDeclaration) -> AppResult<String> {
    let errs = validate_etransport(&declaration);
    if !errs.is_empty() {
        return Err(AppError::Validation(format!(
            "Declarație e-Transport invalidă: {}",
            errs.join("; ")
        )));
    }
    generate_etransport_xml(&declaration)
}

/// Submit an e-Transport declaration to ANAF (live). Returns the upload index + Cod UIT.
/// Requires a connected ANAF account; not reachable without credentials.
#[tauri::command]
pub async fn etransport_submit(
    state: State<'_, AppState>,
    company_id: String,
    declaration: EtransportDeclaration,
    test_mode: bool,
) -> AppResult<EtransportUploadResponse> {
    use crate::anaf::client::ERR_UNAUTHORIZED;
    let pool = &state.db;

    let errs = validate_etransport(&declaration);
    if !errs.is_empty() {
        return Err(AppError::Validation(format!(
            "Declarație e-Transport invalidă: {}",
            errs.join("; ")
        )));
    }
    let company = companies::get(pool, &company_id).await?;
    let xml = generate_etransport_xml(&declaration)?;
    let token =
        crate::commands::anaf::get_valid_token(&company_id, pool, &state.token_refresh_lock)
            .await?;
    let client = AnafClient::new(test_mode);

    let mut result = client
        .upload_etransport(&token, &company.cui, xml.clone().into_bytes())
        .await;
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
                result = client
                    .upload_etransport(&new_tok, &company.cui, xml.into_bytes())
                    .await;
            }
        }
    }
    result.map_err(AppError::Other)
}
