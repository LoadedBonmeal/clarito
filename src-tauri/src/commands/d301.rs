//! Tauri commands — D301 decont special de TVA (OPANAF 592/2016).
//!
//! **STRUCTURA CORECTĂ PER SPECIFICAȚIE — NESUPUSĂ VALIDĂRII DUK/XSD.**
//! Namespace-ul D301 (`D301_NAMESPACE`) și versiunea schemei sunt marcate TODO-verify
//! în `anaf_decl::d301_xml`. Verificați față de XSD-ul oficial ANAF înainte de depunere.
//!
//! RBAC: `preview_d301_xml` necesită permisiune de citire (PostGl sau echivalent Read);
//! `export_d301_xml` necesită permisiune de scriere (PostGl).

use std::path::PathBuf;

use tauri::State;

use crate::anaf_decl::d301_xml::{build_d301_xml, D301Data, D301Header};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Parametrii exportului D301.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D301ExportParams {
    pub company_id: String,
    /// Luna perioadei de raportare (1-12).
    pub luna: u32,
    /// Anul perioadei de raportare.
    pub an: i32,
    /// 0 = inițială, 1 = rectificativă.
    pub d_rec: u8,
    /// Datele secțiunilor (calculate din facturile primite intracomunitar / taxare inversă).
    pub data: D301Data,
    /// Calea de scriere a fișierului XML.
    pub dest_path: String,
}

/// Construiește XML-ul D301 fără a-l scrie pe disc — pentru previzualizare în vizualizatorul
/// XML din aplicație sau pentru editare manuală înainte de export.
///
/// **STRUCTURA CORECTĂ PER SPECIFICAȚIE — NESUPUSĂ VALIDĂRII DUK/XSD.**
/// Verificați namespace-ul față de XSD-ul oficial înainte de depunerea la ANAF.
#[tauri::command]
pub async fn preview_d301_xml(
    state: State<'_, AppState>,
    company_id: String,
    luna: u32,
    an: i32,
    d_rec: u8,
    data: D301Data,
) -> AppResult<String> {
    let company = crate::db::companies::get(&state.db, &company_id).await?;
    let cui: String = company.cui.chars().filter(|c| c.is_ascii_digit()).collect();
    let header = make_header(&company, cui, luna, an, d_rec);
    build_d301_xml(&header, &data)
}

/// Exportă D301 ca fișier XML la calea specificată.
///
/// **STRUCTURA CORECTĂ PER SPECIFICAȚIE — NESUPUSĂ VALIDĂRII DUK/XSD.**
/// Verificați namespace-ul față de XSD-ul oficial și rulați DUKIntegrator
/// înainte de depunerea la ANAF prin SPV.
#[tauri::command]
pub async fn export_d301_xml(
    state: State<'_, AppState>,
    params: D301ExportParams,
) -> AppResult<String> {
    let company = crate::db::companies::get(&state.db, &params.company_id).await?;
    let cui: String = company.cui.chars().filter(|c| c.is_ascii_digit()).collect();
    let header = make_header(&company, cui, params.luna, params.an, params.d_rec);
    let xml = build_d301_xml(&header, &params.data)?;

    let path = PathBuf::from(&params.dest_path);
    std::fs::write(&path, &xml)
        .map_err(|e| AppError::Other(format!("Nu s-a putut scrie D301 XML: {e}")))?;
    Ok(params.dest_path)
}

fn make_header(
    company: &crate::db::companies::Company,
    cui: String,
    luna: u32,
    an: i32,
    d_rec: u8,
) -> D301Header {
    let adresa = format!(
        "{}, {}, {}",
        company.address.trim(),
        company.city.trim(),
        company.county.trim()
    );
    D301Header {
        cui,
        den: company.legal_name.chars().take(200).collect(),
        adresa,
        luna,
        an,
        d_rec,
        nume_declar: company.legal_name.chars().take(75).collect(),
        prenume_declar: "-".into(),
        functie_declar: "Administrator".into(),
    }
}
