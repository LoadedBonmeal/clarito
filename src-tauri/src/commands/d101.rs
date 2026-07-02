//! Tauri commands — D101 declarație privind impozitul pe profit (OPANAF 206/2025).
//! DUK gate: lib/D101Validator.jar via `java -jar DUKIntegrator.jar -v D101 <xml> <result>`.
//! RBAC: preview_d101_xml (citire), export_d101_xml (scriere).

use tauri::State;

use crate::anaf_decl::d101_xml::{build_d101_xml, D101Header};
use crate::anaf_decl::DeclKind;
use crate::commands::declarations::{duk_gate_allows_write, OfficialExportResult};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Parametrii exportului D101.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D101ExportParams {
    /// ID-ul companiei (pentru înregistrarea filing-ului în BD).
    pub company_id: String,
    pub header: D101Header,
    /// Calea de scriere a fișierului XML.
    pub dest_path: String,
    /// `true` → scrie fișierul chiar dacă DUK raportează erori (pentru debugging / override manual).
    #[serde(default)]
    pub skip_duk_override: bool,
    /// W8-3: evidența „originală vs. rectificativă" pentru filing-ul înregistrat local. Explicită,
    /// pentru că pentru an ≥ 2024 atributul XML `d_rec` este o constantă structurală a dicționarului
    /// v10 (mereu 2, verificat cu DUK-ul real — vezi `anaf_decl::d101_xml`) și NU mai poate semnala
    /// rectificativa; derivarea veche `header.d_rec != 0` ar fi marcat GREȘIT orice depunere
    /// originală ≥2024 drept rectificativă. Implicit `false` (declarație originală).
    #[serde(default)]
    pub is_rectificative: bool,
}

/// Construiește XML-ul D101 fără a-l scrie pe disc — pentru previzualizare în vizualizatorul
/// XML din aplicație sau pentru editare manuală înainte de export.
///
/// **ATENȚIE**: D101 se depune anual pentru impozitul pe profit definitiv. Contabilul trebuie
/// să verifice câmpurile P1-P15 (rezultat fiscal, deduceri, credite) față de bilanț înainte de
/// depunere. Frontend-ul trebuie să afișeze un PreflightPanel cu mesajul de confirmare.
#[tauri::command]
pub async fn preview_d101_xml(
    _state: State<'_, AppState>,
    header: D101Header,
) -> AppResult<String> {
    build_d101_xml(&header)
}

/// Exportă D101 ca fișier XML la calea specificată, cu gate DUK (D101Validator.jar).
///
/// Validează cu `java -jar DUKIntegrator.jar -v D101 <xml> <result>` (lib/D101Validator.jar)
/// înainte de scriere. Dacă jar-ul lipsește din build, validarea DUK este omisă grațios
/// (duk_available=false) și fișierul este scris oricum.
#[tauri::command]
pub async fn export_d101_xml(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    params: D101ExportParams,
) -> AppResult<OfficialExportResult> {
    let dest = crate::commands::integrations::validate_export_path(&params.dest_path)?;
    let xml = build_d101_xml(&params.header)?;

    // Layer D: validare cu DUK (D101Validator.jar) înainte de scriere — grațios dacă lipsește
    // (require_jar: jar-ul per-declarație poate lipsi din build).
    let gate = crate::anaf_decl::duk::gate_xml_with_duk(&app, DeclKind::D101, &xml, true)?;
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
    crate::db::declaration_filings::record_or_warn(
        &state.db,
        crate::db::declaration_filings::FilingInput {
            company_id: params.company_id.clone(),
            kind: "D101".into(),
            period: format!("{:04}-{:02}", params.header.an, params.header.luna),
            // W8-3: flag explicit, NU derivat din header.d_rec — pentru ≥2024 XML-ul poartă
            // obligatoriu d_rec=2 (constantă v10), deci `d_rec != 0` ar marca originalele
            // drept rectificative.
            is_rectificative: params.is_rectificative,
            file_path: Some(dest.to_string_lossy().to_string()),
        },
    )
    .await;
    Ok(OfficialExportResult {
        path: dest.to_string_lossy().to_string(),
        written: true,
        duk_available,
        duk_passed,
        issues,
    })
}
