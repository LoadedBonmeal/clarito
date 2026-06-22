//! Tauri commands — D700 declarație de înregistrare/mențiuni/radiere (OPANAF 15/2026, ed. 0126).
//!
//! **STRUCTURA CORECTĂ PER SPECIFICAȚIE — NESUPUSĂ VALIDĂRII DUK/XSD.**
//! Namespace-ul D700 (`D700_NAMESPACE`) și versiunea schemei sunt marcate TODO-verify
//! în `anaf_decl::d700_xml`. Verificați față de XSD-ul oficial ANAF înainte de depunere.
//!
//! RBAC: `preview_d700_xml` necesită permisiune de citire; `export_d700_xml` — scriere.

use std::path::PathBuf;

use tauri::State;

use crate::anaf_decl::d700_xml::{build_d700_xml, D700Input};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Parametrii exportului D700.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D700ExportParams {
    pub input: D700Input,
    /// Calea de scriere a fișierului XML.
    pub dest_path: String,
}

/// Construiește XML-ul D700 fără a-l scrie pe disc — pentru previzualizare în vizualizatorul
/// XML din aplicație sau pentru editare manuală.
///
/// **STRUCTURA CORECTĂ PER SPECIFICAȚIE — NESUPUSĂ VALIDĂRII DUK/XSD.**
/// Verificați namespace-ul față de XSD-ul oficial (OPANAF 15/2026) înainte de depunere.
#[tauri::command]
pub async fn preview_d700_xml(_state: State<'_, AppState>, input: D700Input) -> AppResult<String> {
    build_d700_xml(&input)
}

/// Exportă D700 ca fișier XML la calea specificată.
///
/// **STRUCTURA CORECTĂ PER SPECIFICAȚIE — NESUPUSĂ VALIDĂRII DUK/XSD.**
/// Verificați namespace-ul față de XSD-ul oficial și rulați DUKIntegrator
/// înainte de depunerea la ANAF prin SPV.
#[tauri::command]
pub async fn export_d700_xml(
    _state: State<'_, AppState>,
    params: D700ExportParams,
) -> AppResult<String> {
    let xml = build_d700_xml(&params.input)?;
    let path = PathBuf::from(&params.dest_path);
    std::fs::write(&path, &xml)
        .map_err(|e| AppError::Other(format!("Nu s-a putut scrie D700 XML: {e}")))?;
    Ok(params.dest_path)
}
