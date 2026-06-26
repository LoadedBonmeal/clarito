//! Tauri commands — D301 decont special de TVA (OPANAF 592/2016).
//!
//! **XSD-VALIDAT** — Namespace-ul D301 (`D301_NAMESPACE`) este `mfp:anaf:dgti:d301:declaratie:v1`
//! (versiunea v1 corectă per d301.xsd oficial ANAF). Structura este validată via
//! `xmllint --schema tools/anaf/d301.xsd`. Validarea regulilor de business necesită
//! `D301Validator.jar` din pachetul `D301_20201022.zip` (declaratii.anaf.ro / DUKIntegrator).
//!
//! RBAC: `preview_d301_xml` și `aggregate_d301_rows` necesită permisiune de citire;
//! `export_d301_xml` necesită permisiune de scriere.

use tauri::State;

use crate::anaf_decl::d301_xml::{
    aggregate_d301, build_d301_xml, D301Data, D301Header, D301Sectiune,
};
use crate::anaf_decl::DeclKind;
use crate::commands::declarations::{duk_gate_allows_write, OfficialExportResult};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

// ── Parametrii de export ──────────────────────────────────────────────────────

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
    /// Datele secțiunilor (rânduri <sectiune> pre-calculate sau venite din aggregate_d301_rows).
    pub data: D301Data,
    /// Calea de scriere a fișierului XML.
    pub dest_path: String,
    /// `true` → scrie fișierul chiar dacă DUK raportează erori (pentru debugging / override manual).
    #[serde(default)]
    pub skip_duk_override: bool,
}

// ── Comenzi Tauri ─────────────────────────────────────────────────────────────

/// Construiește XML-ul D301 fără a-l scrie pe disc — pentru previzualizare în
/// vizualizatorul XML din aplicație sau pentru editare manuală înainte de export.
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
    let cif: String = company.cui.chars().filter(|c| c.is_ascii_digit()).collect();
    let header = make_header(&company, cif, luna, an, d_rec);
    build_d301_xml(&header, &data)
}

/// Exportă D301 ca fișier XML la calea specificată, cu gate DUK (D301Validator.jar).
///
/// Validează cu `java -jar DUKIntegrator.jar -v D301 <xml> <result>` (lib/D301Validator.jar)
/// înainte de scriere — aceeași formă gated ca D205/D112. Dacă jar-ul lipsește din build,
/// validarea DUK este omisă grațios (duk_available=false) și fișierul este scris oricum.
#[tauri::command]
pub async fn export_d301_xml(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    params: D301ExportParams,
) -> AppResult<OfficialExportResult> {
    let dest = crate::commands::integrations::validate_export_path(&params.dest_path)?;
    let company = crate::db::companies::get(&state.db, &params.company_id).await?;
    let cif: String = company.cui.chars().filter(|c| c.is_ascii_digit()).collect();
    let header = make_header(&company, cif, params.luna, params.an, params.d_rec);
    let xml = build_d301_xml(&header, &params.data)?;

    // Layer D: validare cu DUK (D301Validator.jar) înainte de scriere — grațios dacă lipsește.
    let tmp =
        std::env::temp_dir().join(format!("d301_official_check_{}.xml", uuid::Uuid::now_v7()));
    std::fs::write(&tmp, xml.as_bytes())
        .map_err(|e| AppError::Other(format!("Nu s-a putut scrie temp D301: {e}")))?;
    let provider = crate::anaf_decl::duk::BundledProvider::new(&app);
    let d301_jar = {
        use tauri::Manager;
        let root =
            crate::anaf_decl::duk::bundled_res_root(&app.path().resource_dir().unwrap_or_default());
        root.join("duk/lib/D301Validator.jar")
    };
    let duk = if d301_jar.is_file() {
        crate::anaf_decl::duk::run_duk(&provider, DeclKind::D301, &tmp)?
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
    crate::db::declaration_filings::record_or_warn(
        &state.db,
        crate::db::declaration_filings::FilingInput {
            company_id: params.company_id.clone(),
            kind: "D301".into(),
            period: format!("{:04}-{:02}", params.an, params.luna),
            is_rectificative: params.d_rec != 0,
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

/// Auto-agregă rândurile D301 din `received_invoice_vat_lines` pentru compania și
/// perioada dată, returnând lista de `D301Sectiune` gata de inserat în `D301Data`.
///
/// Clasificare (conformă cu d301.xsd enum {1,2,3,4,5}):
/// - `vat_category='K'` + `intra_eu_kind='goods'` → tip_operatie 1 (AIC bunuri)
/// - `vat_category='K'` + `intra_eu_kind='services'` → tip_operatie 4 (servicii intracomunitare,
///   beneficiar obligat la TVA, art.150/307(2))
/// - `vat_category='AE'` → tip_operatie 5 (alte operațiuni taxare inversă)
///
/// **Limitare**: tipurile 2 (mijloace transport noi) și 3 (produse accizabile) necesită
/// flag explicit absent din modelul curent — rândurile respective se adaugă manual.
///
/// Returnează eroare dacă compania este înregistrată art.316 (depune D300, nu D301).
#[tauri::command]
pub async fn aggregate_d301_rows(
    state: State<'_, AppState>,
    company_id: String,
    // Luna perioadei (1-12) — folosită pentru a construi intervalul YYYY-MM-01..YYYY-MM-DD.
    luna: u32,
    an: i32,
) -> AppResult<Vec<D301Sectiune>> {
    let company = crate::db::companies::get(&state.db, &company_id).await?;

    // Construim intervalul perioadei: de la prima la ultima zi a lunii.
    let period_from = format!("{an}-{luna:02}-01");
    // Ultima zi a lunii (simplu: prima zi a lunii următoare − 1 zi).
    let last_day = last_day_of_month(an, luna);
    let period_to = format!("{an}-{luna:02}-{last_day:02}");

    aggregate_d301(
        &state.db,
        &company_id,
        &period_from,
        &period_to,
        company.vat_payer,
    )
    .await
}

// ── Helpers privați ───────────────────────────────────────────────────────────

fn make_header(
    company: &crate::db::companies::Company,
    cif: String,
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
    // pers_inreg: 1 = neînregistrat art.316 (tipic D301); 2 = art.317 only.
    // Firmele art.316 depun D300 — dar permit totuși emiterea D301 ca document manual
    // (utilizatorul poate forța). Setăm 1 pentru non-art.316, 2 pentru rest.
    let pers_inreg: u8 = if company.vat_payer { 2 } else { 1 };

    D301Header {
        cif,
        denumire: company.legal_name.chars().take(200).collect(),
        adresa,
        telefon: company.phone.clone().unwrap_or_default(),
        fax: String::new(),
        email: company.email.clone().unwrap_or_default(),
        banca: company.bank_name.clone().unwrap_or_default(),
        cont: company.iban.clone().unwrap_or_default(),
        pers_inreg,
        nr_evid: 0, // IntStr23SType — utilizatorul poate actualiza dacă are număr ROI
        luna,
        an,
        d_rec,
        temei: 1, // 1 = declarație normală; utilizatorul setează 2 pentru corectivă
        // Declarant: legal_name ca nume, "-" prenume, "Administrator" funcție.
        // Utilizatorul poate modifica prin câmpul manual din UI înainte de export.
        nume_declarant: company.legal_name.chars().take(75).collect(),
        prenume_declarant: "-".into(),
        functia_declarant: "Administrator".into(),
    }
}

/// Returnează ultima zi a lunii `luna` din anul `an` (fără chrono — calcul simplu).
fn last_day_of_month(an: i32, luna: u32) -> u32 {
    match luna {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            // An bisect: divizibil cu 4, dar nu cu 100, sau divizibil cu 400.
            if (an % 4 == 0 && an % 100 != 0) || an % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 30, // fallback
    }
}
