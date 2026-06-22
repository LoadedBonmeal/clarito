//! Tauri commands — D710 declarație rectificativă (OPANAF 587/2016 + 779/2024).
//!
//! Exportul este gated cu validatorul STANDALONE `D710Validator.jar` (din pachetul
//! `D710_20052026.zip`). D710 NU trece prin DUKIntegrator overlay — validatorul este
//! apelat direct: `java -jar D710Validator.jar <xml>`. Dacă jar-ul lipsește din build,
//! validarea DUK este omisă grațios (duk_available=false) și fișierul este scris oricum.
//!
//! RBAC: `preview_d710_xml` necesită permisiune de citire; `export_d710_xml` — scriere.

use tauri::State;

use crate::anaf_decl::d710_xml::{build_d710_xml, D710Input};
use crate::anaf_decl::DeclKind;
use crate::commands::declarations::{duk_gate_allows_write, OfficialExportResult};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Parametrii exportului D710.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D710ExportParams {
    pub input: D710Input,
    /// Calea de scriere a fișierului XML.
    pub dest_path: String,
    /// `true` → scrie fișierul chiar dacă DUK raportează erori (pentru debugging / override manual).
    #[serde(default)]
    pub skip_duk_override: bool,
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

/// Exportă D710 ca fișier XML la calea specificată, cu gate DUK STANDALONE (D710Validator.jar).
///
/// D710 folosește un validator STANDALONE (NU prin DUKIntegrator overlay):
/// `java -jar D710Validator.jar <xml>` — `run_duk` rutează intern la `run_standalone_validator`.
/// Dacă `lib/D710Validator.jar` lipsește, exportul continuă grațios (duk_available=false).
#[tauri::command]
pub async fn export_d710_xml(
    app: tauri::AppHandle,
    _state: State<'_, AppState>,
    params: D710ExportParams,
) -> AppResult<OfficialExportResult> {
    let dest = crate::commands::integrations::validate_export_path(&params.dest_path)?;
    let xml = build_d710_xml(&params.input)?;

    // Layer D: validare cu D710Validator.jar STANDALONE înainte de scriere — grațios dacă lipsește.
    // run_duk pentru DeclKind::D710 rutează automat la run_standalone_validator (is_standalone=true).
    let tmp =
        std::env::temp_dir().join(format!("d710_official_check_{}.xml", uuid::Uuid::now_v7()));
    std::fs::write(&tmp, xml.as_bytes())
        .map_err(|e| AppError::Other(format!("Nu s-a putut scrie temp D710: {e}")))?;
    let provider = crate::anaf_decl::duk::BundledProvider::new(&app);
    let d710_jar = {
        use tauri::Manager;
        let root =
            crate::anaf_decl::duk::bundled_res_root(&app.path().resource_dir().unwrap_or_default());
        root.join("duk/lib/D710Validator.jar")
    };
    let duk = if d710_jar.is_file() {
        crate::anaf_decl::duk::run_duk(&provider, DeclKind::D710, &tmp)?
    } else {
        None
    };
    let _ = std::fs::remove_file(&tmp);
    let (duk_available, duk_passed, issues) = match &duk {
        Some(o) => (true, o.passed, o.errors.clone()),
        None => (false, false, Vec::new()),
    };
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
