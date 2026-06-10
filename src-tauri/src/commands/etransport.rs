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
    let resp = result.map_err(AppError::Other)?;

    // Evidența UIT (audit r3 W6): codul UIT e valabil 5 zile (transport național, cod 30) sau
    // 15 zile (operațiuni intracomunitare/import-export) de la transmitere — îl persistăm cu
    // termenul, ca UI-ul să poată avertiza înainte de expirare. Best-effort: un eșec de inserare
    // nu invalidează transmiterea (deja acceptată de ANAF).
    let validity_days: i64 = if declaration.cod_tip_operatiune == "30" {
        5
    } else {
        15
    };
    let now = crate::db::models::now_unix();
    if let Err(e) = sqlx::query(
        "INSERT INTO etransport_declarations \
         (id, company_id, uit, index_incarcare, cod_tip_operatiune, partner_name, vehicle, \
          test_mode, submitted_at, expires_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
    )
    .bind(crate::db::models::new_id())
    .bind(&company_id)
    .bind(&resp.uit)
    .bind(&resp.index_incarcare)
    .bind(&declaration.cod_tip_operatiune)
    .bind(&declaration.partner.denumire)
    .bind(&declaration.transport.nr_vehicul)
    .bind(test_mode)
    .bind(now)
    .bind(now + validity_days * 86_400)
    .execute(pool)
    .await
    {
        tracing::warn!(error = ?e, "e-Transport: nu s-a putut salva evidența UIT (non-fatal)");
    }

    Ok(resp)
}

/// O declarație e-Transport transmisă, cu termenul de valabilitate al UIT-ului.
#[derive(Debug, serde::Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct EtransportDeclRecord {
    pub id: String,
    pub company_id: String,
    pub uit: Option<String>,
    pub index_incarcare: String,
    pub cod_tip_operatiune: String,
    pub partner_name: String,
    pub vehicle: String,
    pub test_mode: bool,
    pub submitted_at: i64,
    pub expires_at: i64,
}

/// Lista declarațiilor e-Transport transmise (cele mai recente primele).
#[tauri::command]
pub async fn list_etransport_declarations(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<EtransportDeclRecord>> {
    Ok(sqlx::query_as::<_, EtransportDeclRecord>(
        "SELECT id, company_id, uit, index_incarcare, cod_tip_operatiune, partner_name, vehicle, \
                test_mode, submitted_at, expires_at \
         FROM etransport_declarations WHERE company_id = ?1 \
         ORDER BY submitted_at DESC LIMIT 200",
    )
    .bind(&company_id)
    .fetch_all(&state.db)
    .await?)
}
