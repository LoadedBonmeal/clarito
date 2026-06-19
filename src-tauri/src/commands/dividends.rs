//! Tauri commands — dividende repartizate + impozit pe dividende (Legea 141/2025). Înregistrarea unui
//! dividend calculează cota (16% de la 2026 / 10% tranzitoriu) și postează nota 117/457/446 în GL.
//! Exportul D205 (informativă anuală, pe beneficiar) agregă dividendele rezidenților pe CNP și emite
//! XML-ul validat cu DUK (`D205Validator.jar`).

use tauri::State;

use crate::anaf_decl::d205_xml::{build_d205_xml, D205Header};
use crate::anaf_decl::d207_xml::{build_d207_xml, D207Header};
use crate::anaf_decl::DeclKind;
use crate::commands::declarations::{duk_gate_allows_write, OfficialExportResult};
use crate::db::dividends::{self, Dividend, DividendBeneficiaryUpdate, DividendInput};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[tauri::command]
pub async fn list_dividends(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<Dividend>> {
    dividends::list(&state.db, &company_id).await
}

#[tauri::command]
pub async fn create_dividend(
    state: State<'_, AppState>,
    input: DividendInput,
) -> AppResult<Dividend> {
    dividends::create(&state.db, input).await
}

/// DIV-01: editează in-place identitatea beneficiarului (CNP, nume, rezidență, tip) + data plății/nota.
/// NU schimbă sumele (brut/impozit rămân imuabile — postează GL); pentru a corecta un CNP lipsă fără
/// a șterge înregistrarea (altfel exportul D205 al anului rămâne blocat).
#[tauri::command]
pub async fn update_dividend_beneficiary(
    state: State<'_, AppState>,
    update: DividendBeneficiaryUpdate,
) -> AppResult<Dividend> {
    dividends::update_beneficiary(&state.db, update).await
}

#[tauri::command]
pub async fn delete_dividend(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    dividends::delete(&state.db, &id, &company_id).await
}

/// Parametrii exportului D205 (grupați ca struct, ca să păstrăm comanda sub limita clippy).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D205ExportParams {
    pub company_id: String,
    pub year: i32,
    pub dest_path: String,
    /// `true` → declarație rectificativă (d_rec=1); `false` (implicit) → declarație inițială.
    pub is_rectificative: bool,
}

/// Construiește XML-ul D205 (`:v3`) pentru firma + anul de venit date — agregare pe CNP a
/// dividendelor REZIDENTE (un `<benef>` per CNP); nerezidenții merg în D207 (excluși). Sursa comună
/// pentru `export_d205_official` (scrie + validează) și `preview_d205_xml` (doar întoarce șirul, pentru
/// vizualizatorul XML din aplicație). Blochează (eroare) dacă vreun dividend rezident nu are CNP.
async fn build_d205_xml_for(
    db: &sqlx::SqlitePool,
    company_id: &str,
    year: i32,
    is_rectificative: bool,
) -> AppResult<String> {
    let company = crate::db::companies::get(db, company_id).await?;
    let beneficiaries =
        dividends::d205_beneficiaries_for_year(&dividends::list(db, company_id).await?, year)?;

    // Declarantul: din datele firmei (ca D112 — nume_declar = denumirea, funcția „Administrator").
    // `cui` = CUI fără „RO"; `adresa` = adresa + localitate + județ (ambele obligatorii în D205).
    let cui: String = company.cui.chars().filter(|c| c.is_ascii_digit()).collect();
    let adresa = format!(
        "{}, {}, {}",
        company.address.trim(),
        company.city.trim(),
        company.county.trim()
    );
    let header = D205Header {
        cui,
        adresa,
        den: company.legal_name.chars().take(75).collect(),
        an: year,
        d_rec: u8::from(is_rectificative), // 0 = inițială, 1 = rectificativă
        nume_declar: company.legal_name.chars().take(75).collect(),
        prenume_declar: "-".into(),
        functie_declar: "Administrator".into(),
    };

    // Eroare clară dacă nu există beneficiari (nimic de raportat pentru anul selectat).
    build_d205_xml(&header, &beneficiaries)
}

/// Construiește XML-ul D205 fără a-l scrie pe disc — pentru previzualizare/editare în vizualizatorul
/// XML din aplicație. Re-validarea cu DUK se face separat prin `validate_declaration_xml`.
#[tauri::command]
pub async fn preview_d205_xml(
    state: State<'_, AppState>,
    company_id: String,
    year: i32,
    is_rectificative: bool,
) -> AppResult<String> {
    build_d205_xml_for(&state.db, &company_id, year, is_rectificative).await
}

/// Exportă D205 (declarația informativă anuală, pe beneficiar) pentru anul de venit dat — capitolul
/// dividende. Agregă dividendele REZIDENTE cu CNP pe beneficiar (un `<benef>` per CNP), construiește
/// XML-ul `:v3` și îl validează cu validatorul OFICIAL ANAF `D205Validator.jar` (`-v D205`, prin
/// `run_duk`) înainte de scriere — aceeași formă gated ca `export_saft_official` / `export_d112_xml`.
///
/// Nerezidenții se raportează SEPARAT în D207 (excluși aici). Dacă există dividende rezidente fără CNP
/// beneficiar, exportul e BLOCAT (o D205 incompletă = declarație greșită) până la completarea CNP-ului.
#[tauri::command]
pub async fn export_d205_official(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    params: D205ExportParams,
    skip_duk_override: bool,
) -> AppResult<OfficialExportResult> {
    let D205ExportParams {
        company_id,
        year,
        dest_path,
        is_rectificative,
    } = params;
    let dest = crate::commands::integrations::validate_export_path(&dest_path)?;

    // Agregare pe CNP (rezidenți) + construirea XML-ului — sursă comună cu `preview_d205_xml`.
    // Blochează dacă vreun dividend rezident nu are CNP; nerezidenții → D207 (excluși).
    let xml = build_d205_xml_for(&state.db, &company_id, year, is_rectificative).await?;

    // Layer D: validare cu DUK (D205Validator.jar) înainte de scriere — grațios dacă runtime lipsește.
    let tmp =
        std::env::temp_dir().join(format!("d205_official_check_{}.xml", uuid::Uuid::now_v7()));
    std::fs::write(&tmp, xml.as_bytes())
        .map_err(|e| AppError::Other(format!("Nu s-a putut scrie temp D205: {e}")))?;
    let provider = crate::anaf_decl::duk::BundledProvider::new(&app);
    // `D205Validator.jar` poate lipsi din build (spre deosebire de D300/D406/D112) — dacă lipsește,
    // SĂRIM DUK grațios (scriere directă), ca să nu blocăm exportul cu o eroare „validator neinstalat".
    // Gate-ul DUK se activează automat când jar-ul e prezent în `resources/duk/lib/`.
    let d205_jar = {
        use tauri::Manager;
        let root =
            crate::anaf_decl::duk::bundled_res_root(&app.path().resource_dir().unwrap_or_default());
        root.join("duk/lib/D205Validator.jar")
    };
    let duk = if d205_jar.is_file() {
        crate::anaf_decl::duk::run_duk(&provider, DeclKind::D205, &tmp)?
    } else {
        None
    };
    let _ = std::fs::remove_file(&tmp);
    let (duk_available, duk_passed, issues) = match &duk {
        Some(o) => (true, o.passed, o.errors.clone()),
        None => (false, false, Vec::new()),
    };
    if !duk_gate_allows_write(duk_available, duk_passed, skip_duk_override) {
        return Ok(OfficialExportResult {
            path: String::new(),
            written: false,
            duk_available,
            duk_passed,
            issues,
        });
    }

    std::fs::write(&dest, xml.as_bytes()).map_err(|e| AppError::Other(e.to_string()))?;
    // Înregistrează depunerea în istoric (best-effort — erorile sunt înghițite).
    let _ = crate::db::declaration_filings::record(
        &state.db,
        crate::db::declaration_filings::FilingInput {
            company_id: company_id.clone(),
            kind: "D205".into(),
            period: format!("{year:04}"),
            is_rectificative,
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

// ─── D207 (dividende către NEREZIDENȚI) ──────────────────────────────────────────────────────────

/// Construiește XML-ul D207 (`:v2`) pentru firma + anul de venit — agregare a dividendelor NEREZIDENTE
/// pe (țară, identitate). Sursă comună pentru `export_d207_official` și `preview_d207_xml`. Blochează
/// (eroare) dacă un nerezident nu are nume/țară. Validare = XSD (D207 nu are validator DUK dedicat).
async fn build_d207_xml_for(
    db: &sqlx::SqlitePool,
    company_id: &str,
    year: i32,
    is_rectificative: bool,
) -> AppResult<String> {
    let company = crate::db::companies::get(db, company_id).await?;
    let benefs =
        dividends::d207_beneficiaries_for_year(&dividends::list(db, company_id).await?, year)?;
    let cui: String = company.cui.chars().filter(|c| c.is_ascii_digit()).collect();
    let adresa = format!(
        "{}, {}, {}",
        company.address.trim(),
        company.city.trim(),
        company.county.trim()
    );
    let header = D207Header {
        cui,
        den: company.legal_name.chars().take(200).collect(),
        adresa,
        an: year,
        d_rec: u8::from(is_rectificative), // 0 = inițială, 1 = rectificativă
        nume_declar: company.legal_name.chars().take(75).collect(),
        prenume_declar: "-".into(),
        functie_declar: "Administrator".into(),
    };
    build_d207_xml(&header, &benefs)
}

/// Previzualizează XML-ul D207 (fără scriere) — pentru vizualizatorul XML din aplicație.
#[tauri::command]
pub async fn preview_d207_xml(
    state: State<'_, AppState>,
    company_id: String,
    year: i32,
    is_rectificative: bool,
) -> AppResult<String> {
    build_d207_xml_for(&state.db, &company_id, year, is_rectificative).await
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D207ExportParams {
    pub company_id: String,
    pub year: i32,
    pub dest_path: String,
    /// `true` → declarație rectificativă (d_rec=1); `false` (implicit) → declarație inițială.
    pub is_rectificative: bool,
}

/// Exportă D207 (informativă anuală, beneficiari NEREZIDENȚI) pentru anul de venit dat. D207 NU are un
/// `D207Validator.jar` — validarea oficială e prin XSD (`tools/anaf/d207.xsd`, testată în
/// `tests/d207_xsd.rs`). Scriem direct XML-ul valid (cu `duk_available = false`, ca la D205 fără jar).
#[tauri::command]
pub async fn export_d207_official(
    state: State<'_, AppState>,
    params: D207ExportParams,
) -> AppResult<OfficialExportResult> {
    let D207ExportParams {
        company_id,
        year,
        dest_path,
        is_rectificative,
    } = params;
    let dest = crate::commands::integrations::validate_export_path(&dest_path)?;
    let xml = build_d207_xml_for(&state.db, &company_id, year, is_rectificative).await?;
    std::fs::write(&dest, xml.as_bytes()).map_err(|e| AppError::Other(e.to_string()))?;
    // Înregistrează depunerea în istoric (best-effort — erorile sunt înghițite).
    let _ = crate::db::declaration_filings::record(
        &state.db,
        crate::db::declaration_filings::FilingInput {
            company_id: company_id.clone(),
            kind: "D207".into(),
            period: format!("{year:04}"),
            is_rectificative,
            file_path: Some(dest.to_string_lossy().to_string()),
        },
    )
    .await;
    Ok(OfficialExportResult {
        path: dest.to_string_lossy().to_string(),
        written: true,
        duk_available: false, // D207 nu are validator DUK; conformitatea e garantată de XSD
        duk_passed: false,
        issues: Vec::new(),
    })
}
