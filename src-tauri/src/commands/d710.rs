//! Tauri commands — D710 declarație rectificativă (OPANAF 587/2016 + 779/2024).
//!
//! **STRUCTURA CORECTĂ PER SPECIFICAȚIE — NESUPUSĂ VALIDĂRII DUK/XSD.**
//! Namespace-ul D710 (`D710_NAMESPACE`) și versiunea schemei sunt marcate TODO-verify
//! în `anaf_decl::d710_xml`. Verificați față de XSD-ul oficial ANAF înainte de depunere.
//!
//! RBAC: `preview_d710_xml` necesită permisiune de citire; `export_d710_xml` — scriere.

use std::path::PathBuf;

use tauri::State;

use crate::anaf_decl::d710_xml::{build_d710_xml, D710Input};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Parametrii exportului D710.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D710ExportParams {
    pub input: D710Input,
    /// Calea de scriere a fișierului XML.
    pub dest_path: String,
}

/// Construiește XML-ul D710 fără a-l scrie pe disc — pentru previzualizare în vizualizatorul
/// XML din aplicație sau pentru editare manuală.
///
/// **STRUCTURA CORECTĂ PER SPECIFICAȚIE — NESUPUSĂ VALIDĂRII DUK/XSD.**
/// Verificați namespace-ul față de XSD-ul oficial (OPANAF 587/2016 + 779/2024) înainte de depunere.
#[tauri::command]
pub async fn preview_d710_xml(_state: State<'_, AppState>, input: D710Input) -> AppResult<String> {
    build_d710_xml(&input)
}

/// Exportă D710 ca fișier XML la calea specificată.
///
/// **STRUCTURA CORECTĂ PER SPECIFICAȚIE — NESUPUSĂ VALIDĂRII DUK/XSD.**
/// Verificați namespace-ul față de XSD-ul oficial și rulați DUKIntegrator
/// înainte de depunerea la ANAF prin SPV.
#[tauri::command]
pub async fn export_d710_xml(
    _state: State<'_, AppState>,
    params: D710ExportParams,
) -> AppResult<String> {
    let xml = build_d710_xml(&params.input)?;
    let path = PathBuf::from(&params.dest_path);
    std::fs::write(&path, &xml)
        .map_err(|e| AppError::Other(format!("Nu s-a putut scrie D710 XML: {e}")))?;
    Ok(params.dest_path)
}
