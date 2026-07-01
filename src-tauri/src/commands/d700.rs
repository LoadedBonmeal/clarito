//! Tauri commands — D700 declarație de înregistrare/mențiuni/radiere (OPANAF 15/2026, ed. 0126).
//!
//! **STRUCTURA CORECTĂ PER STRUCTURA_XML_D700_0126 — FĂRĂ XSD PUBLIC.**
//! D700 nu are XSD public descărcabil — validați cu `D700Validator.jar` din pachetul
//! `D700_20260423.zip` pe declaratii.anaf.ro, prin DUKIntegrator, înainte de depunere.
//! Structura XML include `felD`, `dec_inreg`, `totalPlata_A`, `Bifa_A..Bifa_D` gates.
//!
//! RBAC: `preview_d700_xml` necesită permisiune de citire; `export_d700_xml` — scriere.

use tauri::State;

use crate::anaf_decl::d700_xml::{build_d700_xml, D700Input};
use crate::anaf_decl::DeclKind;
use crate::commands::declarations::{duk_gate_allows_write, OfficialExportResult};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Parametrii exportului D700.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D700ExportParams {
    pub input: D700Input,
    /// Calea de scriere a fișierului XML.
    pub dest_path: String,
    /// `true` → scrie fișierul chiar dacă DUK raportează erori (pentru debugging / override manual).
    #[serde(default)]
    pub skip_duk_override: bool,
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

/// Exportă D700 ca fișier XML la calea specificată, cu gate DUK (D700Validator.jar).
///
/// Validează cu `java -jar DUKIntegrator.jar -v D700 <xml> <result>` (lib/D700Validator.jar)
/// înainte de scriere — aceeași formă gated ca D205/D112. Dacă jar-ul lipsește din build,
/// validarea DUK este omisă grațios (duk_available=false) și fișierul este scris oricum.
#[tauri::command]
pub async fn export_d700_xml(
    app: tauri::AppHandle,
    _state: State<'_, AppState>,
    params: D700ExportParams,
) -> AppResult<OfficialExportResult> {
    let dest = crate::commands::integrations::validate_export_path(&params.dest_path)?;
    let xml = build_d700_xml(&params.input)?;

    // Layer D: validare cu DUK (D700Validator.jar) înainte de scriere — grațios dacă lipsește
    // (require_jar: jar-ul per-declarație poate lipsi din build).
    let gate = crate::anaf_decl::duk::gate_xml_with_duk(&app, DeclKind::D700, &xml, true)?;
    let (duk_available, duk_passed, issues) = (gate.available, gate.passed, gate.issues);
    if !duk_gate_allows_write(duk_available, duk_passed, params.skip_duk_override) {
        return Ok(OfficialExportResult {
            path: String::new(),
            written: false,
            duk_available,
            duk_passed,
            issues,
        });
    }

    std::fs::write(&dest, xml.as_bytes()).map_err(|e| AppError::Other(e.to_string()))?;
    Ok(OfficialExportResult {
        path: dest.to_string_lossy().to_string(),
        written: true,
        duk_available,
        duk_passed,
        issues,
    })
}
