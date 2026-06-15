//! Tauri commands — dividende repartizate + impozit pe dividende (Legea 141/2025). Înregistrarea unui
//! dividend calculează cota (16% de la 2026 / 10% tranzitoriu) și postează nota 117/457/446 în GL.
//! Exportul D205 (informativă anuală, pe beneficiar) agregă dividendele rezidenților pe CNP și emite
//! XML-ul validat cu DUK (`D205Validator.jar`).

use std::collections::BTreeMap;
use std::str::FromStr;

use rust_decimal::Decimal;
use tauri::State;

use crate::anaf_decl::d205_xml::{build_d205_xml, D205Beneficiary, D205Header};
use crate::anaf_decl::DeclKind;
use crate::commands::declarations::{duk_gate_allows_write, OfficialExportResult};
use crate::db::dividends::{self, Dividend, DividendInput};
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
    } = params;
    let company = crate::db::companies::get(&state.db, &company_id).await?;
    let dest = crate::commands::integrations::validate_export_path(&dest_path)?;

    // Dividendele cu data distribuirii în anul de venit.
    let year_str = format!("{year:04}");
    let in_year: Vec<Dividend> = dividends::list(&state.db, &company_id)
        .await?
        .into_iter()
        .filter(|d| d.distribution_date.starts_with(&year_str))
        .collect();

    // Nerezidenții → D207 (excluși). Rezidenții TREBUIE să aibă CNP — altfel D205 ar fi incompletă.
    let residents: Vec<&Dividend> = in_year.iter().filter(|d| d.beneficiary_resident).collect();
    let missing = residents
        .iter()
        .filter(|d| {
            d.beneficiary_cnp
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
        })
        .count();
    if missing > 0 {
        return Err(AppError::Validation(format!(
            "D205 {year}: {missing} dividende rezidente fără CNP beneficiar — completați CNP-ul \
             înainte de export (nerezidenții se raportează separat în D207)."
        )));
    }

    // Agregare pe CNP — un <benef> per beneficiar. `divid_P` = brutul plătit (cu dată de plată).
    let mut by_cnp: BTreeMap<String, D205Beneficiary> = BTreeMap::new();
    for d in &residents {
        let cnp = d
            .beneficiary_cnp
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_string();
        let gross = Decimal::from_str(d.gross_amount.trim()).unwrap_or_default();
        let tax = Decimal::from_str(d.tax_amount.trim()).unwrap_or_default();
        let paid = if d
            .payment_date
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_some()
        {
            gross
        } else {
            Decimal::ZERO
        };
        let entry = by_cnp
            .entry(cnp.clone())
            .or_insert_with(|| D205Beneficiary {
                cnp: cnp.clone(),
                name: String::new(),
                baza: Decimal::ZERO,
                impozit: Decimal::ZERO,
                distribuit: Decimal::ZERO,
                platit: Decimal::ZERO,
                resident: true,
            });
        entry.baza += gross;
        entry.impozit += tax;
        entry.distribuit += gross;
        entry.platit += paid;
        if entry.name.is_empty() {
            if let Some(n) = d.shareholder.as_deref() {
                entry.name = n.trim().to_string();
            }
        }
    }
    let beneficiaries: Vec<D205Beneficiary> = by_cnp.into_values().collect();

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
        d_rec: 0,
        nume_declar: company.legal_name.chars().take(75).collect(),
        prenume_declar: "-".into(),
        functie_declar: "Administrator".into(),
    };

    // Eroare clară dacă nu există beneficiari (nimic de raportat pentru anul selectat).
    let xml = build_d205_xml(&header, &beneficiaries)?;

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
        app.path()
            .resource_dir()
            .unwrap_or_default()
            .join("duk/lib/D205Validator.jar")
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
    Ok(OfficialExportResult {
        path: dest.to_string_lossy().to_string(),
        written: true,
        duk_available,
        duk_passed,
        issues,
    })
}
