//! Tauri commands — D100 declarație privind obligațiile de plată la bugetul de stat (OPANAF 57/2026).
//! DUK gate: lib/D100Validator.jar via `java -jar DUKIntegrator.jar -v D100 <xml> <result>`.
//! RBAC: preview_d100_xml (citire), export_d100_xml (scriere).

use tauri::State;

use crate::anaf_decl::d100_xml::{build_d100_xml, D100Header};
use crate::anaf_decl::DeclKind;
use crate::commands::declarations::{duk_gate_allows_write, OfficialExportResult};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Parametrii exportului D100.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D100ExportParams {
    /// ID-ul companiei (pentru înregistrarea filing-ului în BD).
    pub company_id: String,
    pub header: D100Header,
    /// Calea de scriere a fișierului XML.
    pub dest_path: String,
    /// `true` → scrie fișierul chiar dacă DUK raportează erori (pentru debugging / override manual).
    #[serde(default)]
    pub skip_duk_override: bool,
}

/// Construiește XML-ul D100 fără a-l scrie pe disc — pentru previzualizare în vizualizatorul
/// XML din aplicație sau pentru editare manuală înainte de export.
///
/// **ATENȚIE**: D100 obligă contabilul să confirme codurile bugetare și sumele înainte de
/// depunere — nomenclatorul obligațiilor este volatil (OPANAF 57/2026 a adus coduri noi).
/// Frontend-ul trebuie să afișeze un PreflightPanel cu mesajul de confirmare.
#[tauri::command]
pub async fn preview_d100_xml(
    _state: State<'_, AppState>,
    header: D100Header,
) -> AppResult<String> {
    build_d100_xml(&header)
}

/// Exportă D100 ca fișier XML la calea specificată, cu gate DUK (D100Validator.jar).
///
/// Validează cu `java -jar DUKIntegrator.jar -v D100 <xml> <result>` (lib/D100Validator.jar)
/// înainte de scriere. Dacă jar-ul lipsește din build, validarea DUK este omisă grațios
/// (duk_available=false) și fișierul este scris oricum.
///
/// **Guardrail**: D100 obligă contabilul să confirme codurile bugetare + sumele înainte de
/// depunere (nomenclatorul obligațiilor e volatil — 2026 a adus coduri noi). Frontend-ul
/// trebuie să afișeze PreflightPanel cu mesajul de confirmare.
#[tauri::command]
pub async fn export_d100_xml(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    params: D100ExportParams,
) -> AppResult<OfficialExportResult> {
    let dest = crate::commands::integrations::validate_export_path(&params.dest_path)?;
    let xml = build_d100_xml(&params.header)?;

    // Layer D: validare cu DUK (D100Validator.jar) înainte de scriere — grațios dacă lipsește.
    let tmp =
        std::env::temp_dir().join(format!("d100_official_check_{}.xml", uuid::Uuid::now_v7()));
    std::fs::write(&tmp, xml.as_bytes())
        .map_err(|e| AppError::Other(format!("Nu s-a putut scrie temp D100: {e}")))?;
    let provider = crate::anaf_decl::duk::BundledProvider::new(&app);
    let d100_jar = {
        use tauri::Manager;
        let root =
            crate::anaf_decl::duk::bundled_res_root(&app.path().resource_dir().unwrap_or_default());
        root.join("duk/lib/D100Validator.jar")
    };
    let duk = if d100_jar.is_file() {
        crate::anaf_decl::duk::run_duk(&provider, DeclKind::D100, &tmp)?
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
    let _ = crate::db::declaration_filings::record(
        &state.db,
        crate::db::declaration_filings::FilingInput {
            company_id: params.company_id.clone(),
            kind: "D100".into(),
            period: format!("{:04}-{:02}", params.header.an, params.header.luna),
            is_rectificative: false,
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
