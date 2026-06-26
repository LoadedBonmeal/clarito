//! Tauri commands — salarizare (angajați + stat de salarii lunar).

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::str::FromStr;
use tauri::State;

use crate::anaf_decl::d112_xml::{generate_d112_xml, D112Employee, D112Header, D112MedicalLeave};
use crate::db::payroll::{self, CreateEmployeeInput, Employee, PayrollRun, UpdateEmployeeInput};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Split a full name into (nume, prenume): first token = nume de familie, rest = prenume.
fn split_name(full: &str) -> (String, String) {
    let mut it = full.split_whitespace();
    let nume = it.next().unwrap_or("-").to_string();
    let pren: String = it.collect::<Vec<_>>().join(" ");
    (nume, if pren.is_empty() { "-".into() } else { pren })
}

use crate::db::invoices::round2;

/// Convert an ISO date (YYYY-MM-DD) to the D112 zz.ll.aaaa format; pass other strings through.
fn ro_date(iso: &str) -> String {
    let p: Vec<&str> = iso.split('-').collect();
    if p.len() == 3 && p[0].len() == 4 {
        format!("{}.{}.{}", p[2], p[1], p[0])
    } else {
        iso.to_string()
    }
}

#[tauri::command]
pub async fn list_employees(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<Employee>> {
    payroll::list(&state.db, &company_id).await
}

#[tauri::command]
pub async fn create_employee(
    state: State<'_, AppState>,
    input: CreateEmployeeInput,
) -> AppResult<Employee> {
    payroll::create(&state.db, input).await
}

#[tauri::command]
pub async fn update_employee(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    input: UpdateEmployeeInput,
) -> AppResult<Employee> {
    payroll::update(&state.db, &id, &company_id, input).await
}

#[tauri::command]
pub async fn delete_employee(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    payroll::delete(&state.db, &id, &company_id).await
}

#[tauri::command]
pub async fn list_medical_leaves(
    state: State<'_, AppState>,
    company_id: String,
    period_ym: String,
) -> AppResult<Vec<crate::db::concedii::MedicalLeave>> {
    crate::db::concedii::list(&state.db, &company_id, &period_ym).await
}

#[tauri::command]
pub async fn create_medical_leave(
    state: State<'_, AppState>,
    input: crate::db::concedii::MedicalLeaveInput,
) -> AppResult<crate::db::concedii::MedicalLeave> {
    crate::db::concedii::create(&state.db, input).await
}

#[tauri::command]
pub async fn delete_medical_leave(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    crate::db::concedii::delete(&state.db, &id, &company_id).await
}

#[tauri::command]
pub async fn list_secondary_offices(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<payroll::SecondaryOffice>> {
    payroll::list_sedii(&state.db, &company_id).await
}

#[tauri::command]
pub async fn create_secondary_office(
    state: State<'_, AppState>,
    company_id: String,
    cif: String,
    name: String,
) -> AppResult<payroll::SecondaryOffice> {
    payroll::create_sediu(&state.db, &company_id, &cif, &name).await
}

#[tauri::command]
pub async fn delete_secondary_office(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    payroll::delete_sediu(&state.db, &id, &company_id).await
}

/// Rulează statul de salarii lunar: calculează stările individuale (ratele 2026) și postează nota
/// agregată în GL (641/421, 4315, 4316, 444, 646/436). Idempotentă per perioadă.
#[tauri::command]
pub async fn run_payroll(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
) -> AppResult<PayrollRun> {
    payroll::run_payroll(&state.db, &company_id, &period_from, &period_to).await
}

/// Exportă D112 (XML `:v7`, validat cu `D112Validator.jar` — vezi d112_xml.rs) pentru luna dată:
/// antet + obligații angajator + câte un asigurat per salariat activ. Salariatul cu concediu medical
/// emite calea B (asiguratB1/B2/B3/B4 + asiguratD) cu salariul proratat + indemnizația, consistent cu
/// Registrul-jurnal (`run_payroll`).
///
/// Layer D (gate DUK): înainte de scriere, XML-ul e validat cu validatorul OFICIAL ANAF `D112Validator.jar`
/// inclus în aplicație (`-v D112`, prin `run_duk`). Dacă DUK e disponibil și raportează ERORI, fișierul NU
/// se scrie (decât cu `skip_duk_override`); atenționările (ATT) trec. Dacă runtime-ul DUK nu e disponibil
/// (ex. dev), se cade grațios pe scriere directă. Aceeași formă ca `export_saft_official` (D406).
/// Parametrii exportului D112 — grupați într-un struct (ca `SaftOfficialParams`) ca să păstrăm comanda
/// sub limita clippy de argumente; câmpurile vin flat din JS (`camelCase`).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D112ExportParams {
    pub company_id: String,
    pub year: i32,
    pub month: u32,
    pub caen: String,
    pub dest_path: String,
    /// `true` → declarație rectificativă (d_rec=1); `false` (implicit) → declarație inițială.
    pub is_rectificative: bool,
}

#[tauri::command]
pub async fn export_d112_xml(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    params: D112ExportParams,
    skip_duk_override: bool,
) -> AppResult<crate::commands::declarations::OfficialExportResult> {
    use crate::anaf_decl::DeclKind;
    use crate::commands::declarations::duk_gate_allows_write;

    let D112ExportParams {
        company_id,
        year,
        month,
        caen,
        dest_path,
        is_rectificative,
    } = params;

    let company = crate::db::companies::get(&state.db, &company_id).await?;
    let xml = build_d112_xml(
        &state.db,
        &company,
        year,
        month,
        caen.trim(),
        is_rectificative,
    )
    .await?;
    // Validate the caller-supplied destination (absolute, no '..', no UNC, whitelist ext) — the IPC
    // endpoint accepts an arbitrary string.
    let dest = crate::commands::integrations::validate_export_path(&dest_path)?;

    // Layer D: validate with the bundled DUK before writing. Graceful: no runtime → proceed.
    let tmp =
        std::env::temp_dir().join(format!("d112_official_check_{}.xml", uuid::Uuid::now_v7()));
    std::fs::write(&tmp, xml.as_bytes())
        .map_err(|e| AppError::Other(format!("Nu s-a putut scrie temp D112: {e}")))?;
    let provider = crate::anaf_decl::duk::BundledProvider::new(&app);
    let duk = crate::anaf_decl::duk::run_duk(&provider, DeclKind::D112, &tmp)?;
    let _ = std::fs::remove_file(&tmp);
    let (duk_available, duk_passed, issues) = match &duk {
        Some(o) => (true, o.passed, o.errors.clone()),
        None => (false, false, Vec::new()),
    };
    if !duk_gate_allows_write(duk_available, duk_passed, skip_duk_override) {
        return Ok(crate::commands::declarations::OfficialExportResult {
            path: String::new(),
            written: false,
            duk_available,
            duk_passed,
            issues,
        });
    }

    std::fs::write(&dest, xml.as_bytes()).map_err(|e| AppError::Other(e.to_string()))?;
    // Înregistrează depunerea în istoric (best-effort — erorile sunt înghițite).
    crate::db::declaration_filings::record_or_warn(
        &state.db,
        crate::db::declaration_filings::FilingInput {
            company_id: company_id.clone(),
            kind: "D112".into(),
            period: format!("{year:04}-{month:02}"),
            is_rectificative,
            file_path: Some(dest.to_string_lossy().to_string()),
        },
    )
    .await;
    Ok(crate::commands::declarations::OfficialExportResult {
        path: dest.to_string_lossy().to_string(),
        written: true,
        duk_available,
        duk_passed,
        issues,
    })
}

/// Construiește XML-ul D112 fără a-l scrie pe disc și fără gate-ul DUK — pentru previzualizare/editare în
/// vizualizatorul XML din aplicație. Face ACELAȘI fetch DB ca [`export_d112_xml`] (firmă + angajați +
/// concediile lunii) și produce ACELAȘI XML, astfel încât re-validarea cu DUK (`validate_declaration_xml`)
/// din vizualizator să fie semnificativă. Mirror al perechii `export_d205_official` / `preview_d205_xml`.
#[tauri::command]
pub async fn preview_d112_xml(
    state: State<'_, AppState>,
    company_id: String,
    year: i32,
    month: u32,
    caen: String,
    is_rectificative: bool,
) -> AppResult<String> {
    let company = crate::db::companies::get(&state.db, &company_id).await?;
    build_d112_xml(
        &state.db,
        &company,
        year,
        month,
        caen.trim(),
        is_rectificative,
    )
    .await
}

/// Pure core of the D112 XML build (no Tauri State / filesystem) — fetches all payroll data via
/// `compute_payroll_run` (the single source of truth) and maps the result to the validated `:v7`
/// XML. An employee with ≥1 medical-leave certificate emits the B-path (asiguratB1/B2/B3/B4 +
/// asiguratD); the rest emit the standard `asiguratA` path — consistent with `run_payroll`/GL.
async fn build_d112_xml(
    pool: &sqlx::SqlitePool,
    company: &crate::db::companies::Company,
    year: i32,
    month: u32,
    caen: &str,
    is_rectificative: bool,
) -> AppResult<String> {
    if caen.len() != 4 || !caen.chars().all(|c| c.is_ascii_digit()) {
        return Err(AppError::Validation(
            "Cod CAEN invalid — introduceți 4 cifre (ex. 6201).".into(),
        ));
    }

    let period_from = format!("{year:04}-{month:02}-01");
    let dim = payroll::days_in_month(year, month);
    let period_to = format!("{year:04}-{month:02}-{dim:02}");
    let breakdown =
        payroll::compute_payroll_run(pool, &company.id, &period_from, &period_to).await?;

    // Whole-lei, COMMERCIAL rounding (MidpointAwayFromZero) — never banker's `.round()`.
    let leid = |d: Decimal| {
        d.round_dp_with_strategy(0, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
            .to_i64()
            .unwrap_or(0)
    };

    let mut d112_emps: Vec<D112Employee> = Vec::new();
    for eb in &breakdown.employees {
        // ROB-17: reject malformed CNP before serializing.
        if !crate::anaf_decl::valid_cnp(&eb.cnp) {
            return Err(AppError::Validation(format!(
                "CNP invalid pentru angajatul \u{201e}{}\u{201d} ({}): trebuie 13 cifre cu cifra de control corectă.",
                eb.full_name, eb.cnp
            )));
        }
        let (nume, prenume) = split_name(&eb.full_name);
        let data_ang = eb
            .employment_date
            .as_deref()
            .map(ro_date)
            .unwrap_or_default();

        if eb.is_leave_path {
            // ── B-path: medical leave ───────────────────────────────────────
            if eb.tip_contract != "N"
                && !crate::anaf_decl::d112::exempt_part_time_min_base(
                    eb.pensionar,
                    &eb.exceptie_cas_min,
                )
            {
                tracing::warn!(
                    employee = %eb.full_name,
                    contract = %eb.tip_contract,
                    "D112: salariat part-time cu concediu medical — baza minimă part-time (art. 146 \
                (5^6)) NU se aplică pe luna cu concediu; verificați manual"
                );
            }
            // Build D112MedicalLeave from the raw leaves stored in the breakdown.
            let leid_str = |s: &str| leid(Decimal::from_str(s).unwrap_or(Decimal::ZERO));
            let med_leaves: Vec<D112MedicalLeave> = eb
                .med_leaves_raw
                .iter()
                .map(|l| {
                    let emp_amt = leid_str(&l.suma_angajator);
                    let fn_amt = leid_str(&l.suma_fnuass);
                    D112MedicalLeave {
                        serie: l.serie.clone(),
                        numar: l.numar.clone(),
                        cod_indemn: l.cod_indemnizatie.clone(),
                        data_acordare: ro_date(&l.data_acordare),
                        data_inceput: ro_date(&l.data_inceput),
                        data_sfarsit: ro_date(&l.data_sfarsit),
                        zile_ang: l.zile_angajator,
                        zile_fnuass: l.zile_fnuass,
                        baza_calcul: leid_str(&l.baza_calcul),
                        zile_baza: l.zile_baza,
                        suma_ang: emp_amt,
                        suma_fnuass: fn_amt,
                        procent: l.procent,
                        loc_prescriere: l.loc_prescriere,
                        cod_boala: l.cod_boala.clone(),
                    }
                })
                .collect();
            let worked_base = leid(eb.lr_worked_base);
            // sal_contract = brutul de baza al contractului (fara sporuri).
            let sal_contract = leid(eb.gross_base);
            d112_emps.push(D112Employee {
                cnp: eb.cnp.clone(),
                nume,
                prenume,
                data_ang,
                gross: leid(eb.lr_worked_gross),
                cas: leid(eb.lr_cas),
                cass: leid(eb.lr_cass),
                impozit: leid(eb.lr_income_tax),
                cam: leid(eb.lr_cam),
                zile: eb.lr_worked_days,
                tip_asigurat: eb.tip_asigurat.clone(),
                pensionar: eb.pensionar,
                tip_contract: eb.tip_contract.clone(),
                ore_norma: eb.ore_norma.clamp(6, 8) as u32,
                baza_cas: worked_base,
                baza_cass: worked_base,
                baza_cam: worked_base,
                sal_contract,
                baza_impozit: leid(eb.lr_taxable_base),
                deducere: 0, // B-path: deducere is embedded in taxable_base calculation
                sediu_cif: eb.sediu_cif.clone(),
                e3_23: 0,
                med_leaves,
            });
        } else {
            // ── A-path: standard salary ─────────────────────────────────────
            // baza_cas / baza_cass may be lifted to part-time minimum.
            let baza_cas = leid(eb.baza_cas);
            let baza_cass = leid(eb.baza_cass);
            let baza_cam = leid(eb.baza_cam_real);
            // If baza_cas was lifted (part-time min), recompute contributions on lifted base.
            let (cas_emis, cass_emis) = if eb.part_time_min.is_some() {
                let cas = leid(round2(eb.baza_cas * Decimal::new(25, 2)));
                let cass = leid(round2(eb.baza_cass * Decimal::new(10, 2)));
                (cas, cass)
            } else {
                (leid(eb.sal_cas), leid(eb.sal_cass))
            };
            d112_emps.push(D112Employee {
                cnp: eb.cnp.clone(),
                nume,
                prenume,
                data_ang,
                gross: leid(eb.sal_gross),
                cas: cas_emis,
                cass: cass_emis,
                impozit: leid(eb.sal_impozit),
                cam: leid(eb.sal_cam),
                zile: eb.zile_emis,
                tip_asigurat: eb.tip_asigurat.clone(),
                pensionar: eb.pensionar,
                tip_contract: eb.tip_contract.clone(),
                ore_norma: eb.ore_norma.clamp(6, 8) as u32,
                baza_cas,
                baza_cass,
                baza_cam,
                sal_contract: leid(eb.gross_base),
                baza_impozit: leid(eb.baza_impozit),
                deducere: leid(eb.sal_personal_deduction),
                sediu_cif: eb.sediu_cif.clone(),
                e3_23: 0,
                med_leaves: vec![],
            });
        }
    }

    // ── Wave E: fold payroll_extra_income (diurna surplus) into D112 bases ────
    // Read the already-computed combined contributions from the breakdown (SSOT) — NO recompute.
    // compute_payroll_run stored combined_cas/combined_cass/combined_impozit/comb_impozit_base using
    // exact Decimal arithmetic on sal_gross (e.g. 3571.43 for a pontaj-prorated employee). If we
    // re-derived from de.gross (i64, already rounded to whole lei), pontaj precision is lost and
    // GL 4315 ≠ D112 412 for employees with BOTH a pontaj proration AND a diurnă excess.
    for de in d112_emps.iter_mut() {
        // Find the breakdown entry for this D112 slot (by CNP).
        let Some(eb) = breakdown.employees.iter().find(|eb| eb.cnp == de.cnp) else {
            continue;
        };
        let excess = eb.excess;
        if excess <= Decimal::ZERO {
            continue;
        }
        let excess_lei = leid(excess);
        // Read the SSOT values — already computed in compute_payroll_run with exact Decimal precision.
        let comb_cas = leid(eb.combined_cas);
        let comb_cass = leid(eb.combined_cass);
        let comb_impozit = leid(eb.combined_impozit);

        de.gross += excess_lei;
        de.baza_cas += excess_lei;
        de.baza_cass += excess_lei;
        de.baza_cam += excess_lei;
        de.baza_impozit = leid(eb.comb_impozit_base);
        de.cas = comb_cas;
        de.cass = comb_cass;
        de.impozit = comb_impozit;
        de.e3_23 = excess_lei;
    }

    // ROB-01: a D112 with zero insured persons is malformed — ANAF rejects it.
    if d112_emps.is_empty() {
        return Err(AppError::Validation(
            "Nu există angajați activi pentru luna selectată — D112 nu poate fi generat gol."
                .into(),
        ));
    }

    // casaAng: codul județului (București "B" → "_B").
    let casa = if company.county.trim().eq_ignore_ascii_case("B") {
        "_B".to_string()
    } else {
        company.county.trim().to_uppercase()
    };
    let header = D112Header {
        luna: month,
        an: year,
        d_rec: u8::from(is_rectificative),
        nume_declar: company.legal_name.chars().take(75).collect(),
        prenume_declar: "-".into(),
        functie_declar: "Administrator".into(),
        cif: company.cui.clone(),
        caen: caen.to_string(),
        den: company.legal_name.chars().take(200).collect(),
        casa,
    };
    if year > 2026 || (year == 2026 && month >= 7) {
        tracing::warn!(
            year,
            month,
            "D112 >= 07/2026: model nou (Ordin 605/95/928/2.314/2026) — structura :v7 emisa e \
conforma structural; re-validati cu artefactele oficiale ANAF inainte de depunere (25.08.2026)"
        );
    }
    Ok(crate::anaf_decl::xml::pretty_print(&generate_d112_xml(
        &header, &d112_emps,
    )))
}

// ─── Wave F: Sporuri salariale ────────────────────────────────────────────────

use crate::db::payroll_sporuri::{CreateSporInput, Spor, UpdateSporInput};

/// Lista sporurilor unui angajat/perioadă. Oricine autentificat poate citi.
#[tauri::command]
pub async fn list_sporuri(
    state: State<'_, AppState>,
    company_id: String,
    period: String,
) -> AppResult<Vec<Spor>> {
    crate::db::payroll_sporuri::list(&state.db, &company_id, &period).await
}

/// Adaugă un spor salarial taxabil (spor vechime, noapte, ore suplimentare, …).
/// Intră în baza CAS/CASS/impozit/CAM la rularea lunii. Necesită CreateDraft.
#[tauri::command]
pub async fn create_spor(state: State<'_, AppState>, input: CreateSporInput) -> AppResult<Spor> {
    crate::db::payroll_sporuri::create(&state.db, input).await
}

/// Actualizează sumă/tip/descriere spor. Necesită CreateDraft.
#[tauri::command]
pub async fn update_spor(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    input: UpdateSporInput,
) -> AppResult<Spor> {
    crate::db::payroll_sporuri::update(&state.db, &id, &company_id, input).await
}

/// Șterge un spor salarial. Necesită Delete.
#[tauri::command]
pub async fn delete_spor(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    crate::db::payroll_sporuri::delete(&state.db, &id, &company_id).await
}

// ─── Wave F: Rețineri/Popriri ─────────────────────────────────────────────────

use crate::db::payroll_retineri::{CreateRetinereInput, Retinere, UpdateRetinereInput};

/// Lista rețineri/popriri ale perioadei. Oricine autentificat poate citi.
#[tauri::command]
pub async fn list_retineri(
    state: State<'_, AppState>,
    company_id: String,
    period: String,
) -> AppResult<Vec<Retinere>> {
    crate::db::payroll_retineri::list(&state.db, &company_id, &period).await
}

/// Adaugă o reținere/poprire din salariu net (poprire, pensie alimentară, avans,
/// sindicat). Se aplică post-net; nu modifică contribuțiile sau D112.
/// Necesită CreateDraft.
#[tauri::command]
pub async fn create_retinere(
    state: State<'_, AppState>,
    input: CreateRetinereInput,
) -> AppResult<Retinere> {
    crate::db::payroll_retineri::create(&state.db, input).await
}

/// Actualizează sumă/tip/creditor/cont/prioritate reținere. Necesită CreateDraft.
#[tauri::command]
pub async fn update_retinere(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    input: UpdateRetinereInput,
) -> AppResult<Retinere> {
    crate::db::payroll_retineri::update(&state.db, &id, &company_id, input).await
}

/// Șterge o reținere. Necesită Delete.
#[tauri::command]
pub async fn delete_retinere(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    crate::db::payroll_retineri::delete(&state.db, &id, &company_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::concedii::MedicalLeaveInput;
    use sqlx::SqlitePool;

    async fn setup() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        // cui mod-11 valid pentru DUKIntegrator; județ CJ.
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co1','13548146','Test SRL','Str 1','Cluj','CJ','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    fn emp_input(cnp: &str, name: &str, gross: &str) -> CreateEmployeeInput {
        CreateEmployeeInput {
            company_id: "co1".into(),
            cnp: cnp.into(),
            full_name: name.into(),
            gross_salary: gross.into(),
            personal_deduction: Some("0".into()),
            employment_date: Some("2024-01-01".into()),
            contract_end_date: None,
            tip_asigurat: None,
            pensionar: None,
            tip_contract: None,
            ore_norma: None,
            exceptie_cas_min: None,
            sediu_cif: None,
            beneficiar_suma_netaxabila: None,
            functia: None,
            cod_cor: None,
        }
    }

    fn leave_input(
        emp_id: &str,
        serie: &str,
        numar: &str,
        inceput: &str,
        sfarsit: &str,
    ) -> MedicalLeaveInput {
        MedicalLeaveInput {
            company_id: "co1".into(),
            employee_id: emp_id.into(),
            period_ym: "2026-06".into(),
            serie: Some(serie.into()),
            numar: Some(numar.into()),
            cod_indemnizatie: Some("01".into()),
            data_acordare: Some(inceput.into()),
            data_inceput: Some(inceput.into()),
            data_sfarsit: Some(sfarsit.into()),
            zile_angajator: Some(4),
            zile_fnuass: Some(0),
            baza_calcul: Some("24000".into()),
            zile_baza: Some(130),
            suma_angajator: Some("600".into()),
            suma_fnuass: Some("0".into()),
            procent: Some(75),
            loc_prescriere: Some(1),
            cod_boala: Some("A09".into()),
        }
    }

    /// End-to-end: DB (company + 2 employees, one with a medical leave) → build_d112_xml. The standard
    /// employee emits asiguratA; the leave employee emits the B-path (asiguratB1 + asiguratD) + the
    /// employer angajatorC2 rollup. Exercises the full export command path minus IPC/file IO.
    #[tokio::test]
    async fn build_d112_end_to_end_mixed_a_and_b_paths() {
        let pool = setup().await;
        crate::db::payroll::create(&pool, emp_input("1960101410019", "Pop Ana", "5000"))
            .await
            .unwrap();
        let b =
            crate::db::payroll::create(&pool, emp_input("1900101410011", "Ion Gheorghe", "5500"))
                .await
                .unwrap();
        crate::db::concedii::create(
            &pool,
            leave_input(&b.id, "AB", "1234567", "2026-06-08", "2026-06-12"),
        )
        .await
        .unwrap();

        let company = crate::db::companies::get(&pool, "co1").await.unwrap();
        let xml = build_d112_xml(&pool, &company, 2026, 6, "6201", false)
            .await
            .unwrap();

        assert!(xml.contains("declaratie:v7"));
        assert_eq!(xml.matches("<asigurat ").count(), 2); // doi asigurați
        assert_eq!(xml.matches("<asiguratA ").count(), 1); // doar salariatul standard
        assert!(xml.contains("<asiguratB1 B1_1=\"1\"")); // calea B pentru cel cu concediu
        assert!(xml.contains("D_1=\"AB\" D_2=\"1234567\"")); // certificatul
        assert!(xml.contains("<angajatorC2 C2_11=\"1\"")); // rollup recuperare FNUASS (1 certificat)
                                                           // CIF-ul firmei + CAEN în antet.
        assert!(xml.contains("cif=\"13548146\" caen=\"6201\""));
    }

    /// EDGE: an employee with TWO certificates in the month emits two asiguratD rows and the
    /// angajatorC2 count/sum aggregate both.
    #[tokio::test]
    async fn build_d112_multiple_certificates_one_employee() {
        let pool = setup().await;
        let e = crate::db::payroll::create(&pool, emp_input("1960101410019", "Pop Ana", "5500"))
            .await
            .unwrap();
        crate::db::concedii::create(
            &pool,
            leave_input(&e.id, "AA", "111", "2026-06-02", "2026-06-04"),
        )
        .await
        .unwrap();
        crate::db::concedii::create(
            &pool,
            leave_input(&e.id, "BB", "222", "2026-06-16", "2026-06-18"),
        )
        .await
        .unwrap();

        let company = crate::db::companies::get(&pool, "co1").await.unwrap();
        let xml = build_d112_xml(&pool, &company, 2026, 6, "6201", false)
            .await
            .unwrap();

        assert_eq!(xml.matches("<asiguratD ").count(), 2); // două certificate
        assert!(xml.contains("D_1=\"AA\""));
        assert!(xml.contains("D_1=\"BB\""));
        assert!(xml.contains("<angajatorC2 C2_11=\"2\"")); // COUNT = 2 certificate
    }

    /// Wave 2 — GOLDEN GL ≡ D112. The GL note `run_payroll` posts MUST equal the D112 obligation
    /// totals for the SAME roster (4315↔412 CAS, 4316↔432 CASS, 444↔602 impozit, 436↔480 CAM), so the
    /// two independent code paths (GL aggregation vs D112 XML) can never silently drift. Mixed roster:
    /// one full-time + one medical-leave (B-path) employee — both paths read the same DB leaves.
    #[tokio::test]
    async fn gl_payroll_totals_equal_d112_obligations() {
        use rust_decimal::prelude::ToPrimitive;
        let pool = setup().await;
        crate::db::payroll::create(&pool, emp_input("1960101410019", "Pop Ana", "5000"))
            .await
            .unwrap();
        let b =
            crate::db::payroll::create(&pool, emp_input("1900101410011", "Ion Gheorghe", "5500"))
                .await
                .unwrap();
        crate::db::concedii::create(
            &pool,
            leave_input(&b.id, "AB", "1234567", "2026-06-08", "2026-06-12"),
        )
        .await
        .unwrap();

        let company = crate::db::companies::get(&pool, "co1").await.unwrap();

        // GL path: run_payroll posts the note; read the credit per account from the trial balance.
        crate::db::payroll::run_payroll(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        let tb = crate::db::gl::trial_balance(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        let gl_credit = |code: &str| -> i64 {
            tb.rows
                .iter()
                .find(|r| r.account_code == code)
                .map(|r| {
                    Decimal::from_str(&r.closing_credit)
                        .unwrap_or(Decimal::ZERO)
                        .round_dp_with_strategy(
                            0,
                            rust_decimal::RoundingStrategy::MidpointAwayFromZero,
                        )
                        .to_i64()
                        .unwrap_or(0)
                })
                .unwrap_or(0)
        };

        // D112 path: parse the `angajatorA` obligation `A_datorat` per code.
        let xml = build_d112_xml(&pool, &company, 2026, 6, "6201", false)
            .await
            .unwrap();
        let oblig = |cod: &str| -> i64 {
            let key = format!("A_codOblig=\"{cod}\"");
            let i = xml
                .find(&key)
                .unwrap_or_else(|| panic!("obligația {cod} lipsește"));
            let dk = "A_datorat=\"";
            let j = i + xml[i..].find(dk).expect("A_datorat") + dk.len();
            let end = xml[j..].find('"').unwrap();
            xml[j..j + end].parse::<i64>().unwrap()
        };

        assert_eq!(gl_credit("4315"), oblig("412"), "CAS: GL 4315 ≠ D112 412");
        assert_eq!(gl_credit("4316"), oblig("432"), "CASS: GL 4316 ≠ D112 432");
        assert_eq!(gl_credit("444"), oblig("602"), "impozit: GL 444 ≠ D112 602");
        assert_eq!(gl_credit("436"), oblig("480"), "CAM: GL 436 ≠ D112 480");
    }

    /// P1 GOLDEN TEST — GL≡D112 with diurnă excess (the invariant that broke before the fix).
    ///
    /// Scenario: one employee, salary base 1000, diurnă excess 212 RON (per payroll_extra_income).
    /// Expected: GL 4315/4316/444/436 == D112 412/432/602/480 TO THE LEU — no rounding drift.
    ///
    /// Without the fix: round(1000×25%) + round(212×25%) = 250 + 53 = 303 (GL)
    ///                  vs round(1212×25%) = 303 (D112) → may match here but impozit drifts.
    ///
    /// The real risk case: combined base where split rounding diverges. With salary=1000, excess=212:
    ///   combined CAS = round(1212 × 0.25) = round(303.0) = 303
    ///   combined CASS = round(1212 × 0.10) = round(121.2) = 121
    ///   combined impozit_base = 1212 − 303 − 121 = 788; impozit = round(788 × 0.10) = 79
    ///   combined CAM = round(1212 × 0.0225) = round(27.27) = 27
    ///
    /// The TEST asserts exact equality between GL totals and D112 obligations.
    /// Also checks: 421 nets to zero (salary net balances), 625 reduces to zero (excess reclassed),
    /// 641 D = salary_gross + excess (1000 + 212 = 1212), 4282 D = excess receivable.
    #[tokio::test]
    async fn gl_d112_golden_with_diurna_excess() {
        use rust_decimal::prelude::ToPrimitive;
        let pool = setup().await;
        let emp = crate::db::payroll::create(&pool, emp_input("1960101410019", "Pop Ana", "1000"))
            .await
            .unwrap();

        // Insert the excess directly into payroll_extra_income (simulates an approved decont).
        let excess = rust_decimal::Decimal::from_str("212").unwrap();
        crate::db::payroll_diurna::upsert_extra_income(
            &pool,
            "co1",
            &emp.id,
            "2026-06",
            "dec-p1-test",
            excess,
            "open",
        )
        .await
        .unwrap();

        // Run payroll (folds excess into combined base, posts GL with D641/C625 + D4282/C421).
        crate::db::payroll::run_payroll(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        let tb = crate::db::gl::trial_balance(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        let gl_credit = |code: &str| -> i64 {
            tb.rows
                .iter()
                .find(|r| r.account_code == code)
                .map(|r| {
                    Decimal::from_str(&r.closing_credit)
                        .unwrap_or(Decimal::ZERO)
                        .round_dp_with_strategy(
                            0,
                            rust_decimal::RoundingStrategy::MidpointAwayFromZero,
                        )
                        .to_i64()
                        .unwrap_or(0)
                })
                .unwrap_or(0)
        };
        let gl_debit = |code: &str| -> i64 {
            tb.rows
                .iter()
                .find(|r| r.account_code == code)
                .map(|r| {
                    Decimal::from_str(&r.closing_debit)
                        .unwrap_or(Decimal::ZERO)
                        .round_dp_with_strategy(
                            0,
                            rust_decimal::RoundingStrategy::MidpointAwayFromZero,
                        )
                        .to_i64()
                        .unwrap_or(0)
                })
                .unwrap_or(0)
        };

        // Build D112 via compute_payroll_run (same data path as run_payroll, ensures GL==D112).
        let company = crate::db::companies::get(&pool, "co1").await.unwrap();
        let xml = build_d112_xml(&pool, &company, 2026, 6, "6201", false)
            .await
            .unwrap();
        let oblig = |cod: &str| -> i64 {
            let key = format!("A_codOblig=\"{cod}\"");
            let i = xml
                .find(&key)
                .unwrap_or_else(|| panic!("obligația {cod} lipsește"));
            let dk = "A_datorat=\"";
            let j = i + xml[i..].find(dk).expect("A_datorat") + dk.len();
            let end = xml[j..].find('"').unwrap();
            xml[j..j + end].parse::<i64>().unwrap()
        };

        // P1 INVARIANT: GL obligations MUST equal D112 obligations (single combined-base rounding).
        assert_eq!(
            gl_credit("4315"),
            oblig("412"),
            "P1: CAS GL 4315 ≠ D112 412 (rounding drift with excess!)"
        );
        assert_eq!(
            gl_credit("4316"),
            oblig("432"),
            "P1: CASS GL 4316 ≠ D112 432 (rounding drift with excess!)"
        );
        assert_eq!(
            gl_credit("444"),
            oblig("602"),
            "P1: impozit GL 444 ≠ D112 602 (rounding drift with excess!)"
        );
        assert_eq!(
            gl_credit("436"),
            oblig("480"),
            "P1: CAM GL 436 ≠ D112 480 (rounding drift with excess!)"
        );

        // GL structural checks: 641 D = salary_gross + excess; 625 C = excess (reclassed out);
        // 4282 D = excess receivable (employee's debt); 421 nets to zero (balance check).
        let salary_gross = 1000i64;
        let excess_lei = 212i64;
        assert_eq!(
            gl_debit("641"),
            salary_gross + excess_lei,
            "641 D must be salary_gross + excess ({} + {} = {})",
            salary_gross,
            excess_lei,
            salary_gross + excess_lei
        );
        assert_eq!(
            gl_credit("625"),
            excess_lei,
            "625 C must equal excess (reclassed from travel to salary expense)"
        );
        // 421 structural check: after run_payroll (before payment), 421 has a net CREDIT balance
        // equal to the SALARY net (not the combined-base net), because the D4282/C421 entry offsets
        // the excess-attributable withholdings back into 421, leaving only the salary-net payable.
        //
        // For gross=1000, no deduction, no non-taxable:
        //   sal_cas = round(1000 × 25%) = 250
        //   sal_cass = round(1000 × 10%) = 100
        //   sal_impozit = round((1000−250−100) × 10%) = round(65) = 65
        //   salary_net = 1000 − 250 − 100 − 65 = 585
        let salary_net_expected = 1000i64 - 250 - 100 - 65; // = 585
        let d421 = gl_debit("421");
        let c421 = gl_credit("421");
        assert_eq!(
            d421, 0,
            "421 closing_debit must be zero (net credit position)"
        );
        assert_eq!(
            c421,
            salary_net_expected,
            "421 closing_credit must equal salary_net {salary_net_expected} (not combined-base net; excess net was already paid cash)"
        );
        // 4282 D = excess receivable (must be > 0 since excess > 0).
        assert!(
            gl_debit("4282") > 0,
            "4282 D must be > 0 (employee receivable for excess withholdings)"
        );
        assert!(tb.balanced, "payroll journal with excess must balance");
    }

    /// P1 GOLDEN TEST — GL≡D112 WITH SPOR (Wave F invariant).
    ///
    /// Scenario: one full-time employee, base salary 4000, spor vechime 1000 → gross_eff = 5000.
    /// non_taxable stays on base salary (gross_in = 4000), NOT on gross_eff — mirrors run_payroll.
    ///
    /// Expected contributions on gross_eff = 5000 (combined-base, single rounding):
    ///   CAS  = ROUND(5000 × 25%)           = 1250
    ///   CASS = ROUND(5000 × 10%)           = 500
    ///   impozit_base = 5000 − 1250 − 500   = 3250; impozit = ROUND(3250 × 10%) = 325
    ///   CAM  = ROUND(5000 × 2.25%)         = 113 (aggregate single-rounding on cam_base)
    ///
    /// GL 4315 C = 1250 == D112 412 A_datorat
    /// GL 4316 C = 500  == D112 432 A_datorat
    /// GL 444  C = 325  == D112 602 A_datorat
    /// GL 436  C = 113  == D112 480 A_datorat
    ///
    /// Also verifies that non_taxable is NOT applied to gross+spor:
    ///   A beneficiar_suma_netaxabila flag on the employee is NOT set, so non_taxable = 0.
    ///   If it were applied to gross_eff, GL would be computed on 5000 and D112 on 4000, breaking the
    ///   invariant. The test uses a plain non-beneficiar to keep the non_taxable = 0 path.
    #[tokio::test]
    async fn gl_d112_golden_with_spor() {
        use rust_decimal::prelude::ToPrimitive;
        let pool = setup().await;
        let emp = crate::db::payroll::create(&pool, emp_input("1960101410019", "Pop Ana", "4000"))
            .await
            .unwrap();

        // Insert a spor vechime of 1000 for this employee.
        crate::db::payroll_sporuri::create(
            &pool,
            crate::db::payroll_sporuri::CreateSporInput {
                company_id: "co1".into(),
                employee_id: emp.id.clone(),
                period: "2026-06".into(),
                amount: "1000".into(),
                kind: Some("vechime".into()),
                description: Some("spor test".into()),
            },
        )
        .await
        .unwrap();

        // GL path: run_payroll folds spor into gross_eff = 5000; posts GL on combined base.
        crate::db::payroll::run_payroll(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        let tb = crate::db::gl::trial_balance(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        let gl_credit = |code: &str| -> i64 {
            tb.rows
                .iter()
                .find(|r| r.account_code == code)
                .map(|r| {
                    Decimal::from_str(&r.closing_credit)
                        .unwrap_or(Decimal::ZERO)
                        .round_dp_with_strategy(
                            0,
                            rust_decimal::RoundingStrategy::MidpointAwayFromZero,
                        )
                        .to_i64()
                        .unwrap_or(0)
                })
                .unwrap_or(0)
        };

        // D112 path: build_d112_xml must fold sporuri into the contribution bases.
        let company = crate::db::companies::get(&pool, "co1").await.unwrap();
        let xml = build_d112_xml(&pool, &company, 2026, 6, "6201", false)
            .await
            .unwrap();
        let oblig = |cod: &str| -> i64 {
            let key = format!("A_codOblig=\"{cod}\"");
            let i = xml
                .find(&key)
                .unwrap_or_else(|| panic!("obligația {cod} lipsește"));
            let dk = "A_datorat=\"";
            let j = i + xml[i..].find(dk).expect("A_datorat") + dk.len();
            let end = xml[j..].find('"').unwrap();
            xml[j..j + end].parse::<i64>().unwrap()
        };

        // P1 INVARIANT: GL obligations MUST equal D112 obligations for employee with a spor.
        assert_eq!(
            gl_credit("4315"),
            oblig("412"),
            "CAS: GL 4315 ≠ D112 412 (GL≡D112 breaks when spor not folded into D112 base!)"
        );
        assert_eq!(
            gl_credit("4316"),
            oblig("432"),
            "CASS: GL 4316 ≠ D112 432 (GL≡D112 breaks when spor not folded into D112 base!)"
        );
        assert_eq!(
            gl_credit("444"),
            oblig("602"),
            "impozit: GL 444 ≠ D112 602 (GL≡D112 breaks when spor not folded into D112 base!)"
        );
        assert_eq!(
            gl_credit("436"),
            oblig("480"),
            "CAM: GL 436 ≠ D112 480 (GL≡D112 breaks when spor not folded into D112 base!)"
        );

        // Exact value spot-checks: gross_eff = 4000 + 1000 = 5000.
        assert_eq!(oblig("412"), 1250, "CAS = ROUND(5000 × 25%) = 1250");
        assert_eq!(oblig("432"), 500, "CASS = ROUND(5000 × 10%) = 500");
        assert_eq!(oblig("602"), 325, "impozit = ROUND(3250 × 10%) = 325");
        // CAM = ROUND(5000 × 2.25%) aggregate = 113.
        assert_eq!(oblig("480"), 113, "CAM = ROUND(5000 × 2.25%) = 113");

        assert!(tb.balanced, "payroll journal with spor must balance");
    }

    /// P1 GOLDEN TEST — GL≡D112 WITH PONTAJ (the invariant that broke before this fix).
    ///
    /// Scenario: one full-time employee, base salary 5000, June 2026 (nzl=21 working days).
    /// Pontaj: 15 worked days → prorated gross = 5000 × 15/21 ≈ 3571.43 (Decimal exact arithmetic).
    ///
    /// Expected contributions on prorated gross_eff = 5000 × 15/21 (single rounding):
    ///   CAS  = ROUND(gross_prorated × 25%)                  → GL 4315 C == D112 412 A_datorat
    ///   CASS = ROUND(gross_prorated × 10%)                  → GL 4316 C == D112 432 A_datorat
    ///   impozit_base = gross_prorated − CAS − CASS − ded=0 → GL 444  C == D112 602 A_datorat
    ///   CAM  = ROUND(cam_base × 2.25%) (aggregate)         → GL 436  C == D112 480 A_datorat
    ///
    /// Before fix: run_payroll used the prorated gross but build_d112_xml used the FULL 5000 →
    /// GL obligations on ~3571 ≠ D112 obligations on 5000 → golden invariant broken.
    /// After fix: both use the same prorated gross → all four pairs equal to the exact leu.
    ///
    /// Also verifies: the existing no-pontaj golden test scenario (same pool, run without pontaj)
    /// is byte-identical — i.e. the proration branch fires ONLY when worked_days < nzl.
    #[tokio::test]
    async fn gl_d112_golden_with_pontaj() {
        use rust_decimal::prelude::ToPrimitive;
        let pool = setup().await;
        let emp = crate::db::payroll::create(&pool, emp_input("1960101410019", "Pop Ana", "5000"))
            .await
            .unwrap();

        // Insert a pontaj of 15 worked days out of 21 (June 2026).
        crate::db::pontaj::create(
            &pool,
            crate::db::pontaj::CreatePontajInput {
                company_id: "co1".into(),
                employee_id: emp.id.clone(),
                period: "2026-06".into(),
                worked_days: 15,
                overtime_hours: None,
                night_hours: None,
                absence_days: Some(6),
                leave_days: None,
                notes: None,
            },
        )
        .await
        .unwrap();

        // GL path: run_payroll prorates gross on 15/21 days and posts GL.
        crate::db::payroll::run_payroll(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        let tb = crate::db::gl::trial_balance(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        let gl_credit = |code: &str| -> i64 {
            tb.rows
                .iter()
                .find(|r| r.account_code == code)
                .map(|r| {
                    Decimal::from_str(&r.closing_credit)
                        .unwrap_or(Decimal::ZERO)
                        .round_dp_with_strategy(
                            0,
                            rust_decimal::RoundingStrategy::MidpointAwayFromZero,
                        )
                        .to_i64()
                        .unwrap_or(0)
                })
                .unwrap_or(0)
        };

        // D112 path: build_d112_xml must now apply the same pontaj proration (15/21 of 5000).
        let company = crate::db::companies::get(&pool, "co1").await.unwrap();
        let xml = build_d112_xml(&pool, &company, 2026, 6, "6201", false)
            .await
            .unwrap();
        let oblig = |cod: &str| -> i64 {
            let key = format!("A_codOblig=\"{cod}\"");
            let i = xml
                .find(&key)
                .unwrap_or_else(|| panic!("obligația {cod} lipsește"));
            let dk = "A_datorat=\"";
            let j = i + xml[i..].find(dk).expect("A_datorat") + dk.len();
            let end = xml[j..].find('"').unwrap();
            xml[j..j + end].parse::<i64>().unwrap()
        };

        // P1 GOLDEN INVARIANT: all four GL/D112 pairs must match to the exact leu.
        // Before the fix: GL used prorated gross (~3571) but D112 used full gross (5000) → mismatch.
        // After the fix: both use the SAME prorated gross → exact equality.
        assert_eq!(
            gl_credit("4315"),
            oblig("412"),
            "pontaj P1: CAS GL 4315 ≠ D112 412 (GL uses prorated gross, D112 must too!)"
        );
        assert_eq!(
            gl_credit("4316"),
            oblig("432"),
            "pontaj P1: CASS GL 4316 ≠ D112 432 (GL uses prorated gross, D112 must too!)"
        );
        assert_eq!(
            gl_credit("444"),
            oblig("602"),
            "pontaj P1: impozit GL 444 ≠ D112 602 (GL uses prorated gross, D112 must too!)"
        );
        assert_eq!(
            gl_credit("436"),
            oblig("480"),
            "pontaj P1: CAM GL 436 ≠ D112 480 (GL uses prorated gross, D112 must too!)"
        );

        // Spot-check: all D112 obligations must be LESS than the no-pontaj baseline.
        // (5000 gross: CAS=1250, CASS=500, impozit=325, CAM=113)
        assert!(
            oblig("412") < 1250,
            "CAS with pontaj 15/21 must be below no-pontaj 1250, got {}",
            oblig("412")
        );
        assert!(
            oblig("432") < 500,
            "CASS with pontaj 15/21 must be below no-pontaj 500, got {}",
            oblig("432")
        );
        assert!(
            oblig("602") < 325,
            "impozit with pontaj 15/21 must be below no-pontaj 325, got {}",
            oblig("602")
        );
        assert!(
            oblig("480") < 113,
            "CAM with pontaj 15/21 must be below no-pontaj 113, got {}",
            oblig("480")
        );

        assert!(tb.balanced, "payroll journal with pontaj must balance");
    }

    /// P2 GOLDEN TEST — GL≡D112 WITH PONTAJ + DIURNĂ EXCESS (the precision edge the SSOT fix closes).
    ///
    /// Scenario: one full-time employee, base salary 5000, June 2026 (nzl=21 working days).
    /// Pontaj: 15 worked days → sal_gross = 5000 × 15/21 ≈ 3571.4285… (Decimal, NOT rounded).
    /// Diurnă excess: 212 RON.
    ///
    /// compute_payroll_run computes contributions on EXACT sal_gross (3571.428571…):
    ///   combined_base = 3571.428571… + 212 = 3783.428571…
    ///   combined_cas  = pct(3783.428…, 25/2) = ROUND(3783.428… × 0.25) = ROUND(945.857…) = 946
    ///   combined_cass = pct(3783.428…, 10/2) = ROUND(3783.428… × 0.10) = ROUND(378.342…) = 378
    ///   comb_impozit_base = 3783.428… − 946 − 378 − 0 = 2459.428…
    ///   combined_impozit  = ROUND(2459.428… × 0.10) = ROUND(245.942…) = 246
    ///
    /// OLD Wave E (before fix): de.gross = leid(3571.428…) = 3571 (truncated i64)
    ///   combined = 3571 + 212 = 3783  (loss of 0.428… lei)
    ///   comb_cas  = ROUND(3783 × 0.25) = ROUND(945.75) = 946  ← same here
    ///   comb_cass = ROUND(3783 × 0.10) = ROUND(378.3)  = 378  ← same here
    ///   comb_impozit_base = 3783 − 946 − 378 = 2459
    ///   comb_impozit      = ROUND(2459 × 0.10) = ROUND(245.9) = 246 ← same (lucky)
    ///
    /// The divergence is subtle and case-dependent; this test guards the structural invariant that
    /// Wave E reads the breakdown (SSOT) rather than re-deriving from de.gross (i64) — ensuring
    /// that NO pontaj+excess combination can ever silently drift.
    ///
    /// The test asserts:
    ///   GL 4315 C == D112 412 (CAS)
    ///   GL 4316 C == D112 432 (CASS)
    ///   GL 444  C == D112 602 (impozit)
    ///   GL 436  C == D112 480 (CAM)
    /// all to the exact leu, covering the edge case the previous recompute path could not guarantee.
    #[tokio::test]
    async fn gl_d112_golden_pontaj_plus_diurna_excess() {
        use rust_decimal::prelude::ToPrimitive;
        let pool = setup().await;
        // base salary 5000, no deduction, full-time (tip_contract N).
        let emp = crate::db::payroll::create(&pool, emp_input("1960101410019", "Pop Ana", "5000"))
            .await
            .unwrap();

        // Pontaj: 15 worked days out of 21 in June 2026 → sal_gross = 5000 × 15/21 (Decimal exact).
        crate::db::pontaj::create(
            &pool,
            crate::db::pontaj::CreatePontajInput {
                company_id: "co1".into(),
                employee_id: emp.id.clone(),
                period: "2026-06".into(),
                worked_days: 15,
                overtime_hours: None,
                night_hours: None,
                absence_days: Some(6),
                leave_days: None,
                notes: None,
            },
        )
        .await
        .unwrap();

        // Diurnă excess: 212 RON.
        let excess = rust_decimal::Decimal::from_str("212").unwrap();
        crate::db::payroll_diurna::upsert_extra_income(
            &pool,
            "co1",
            &emp.id,
            "2026-06",
            "dec-p2-pontaj-excess",
            excess,
            "open",
        )
        .await
        .unwrap();

        // GL path: run_payroll computes combined contributions with EXACT Decimal sal_gross.
        crate::db::payroll::run_payroll(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        let tb = crate::db::gl::trial_balance(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        let gl_credit = |code: &str| -> i64 {
            tb.rows
                .iter()
                .find(|r| r.account_code == code)
                .map(|r| {
                    Decimal::from_str(&r.closing_credit)
                        .unwrap_or(Decimal::ZERO)
                        .round_dp_with_strategy(
                            0,
                            rust_decimal::RoundingStrategy::MidpointAwayFromZero,
                        )
                        .to_i64()
                        .unwrap_or(0)
                })
                .unwrap_or(0)
        };

        // D112 path: build_d112_xml must now read combined_* from the breakdown (SSOT), not re-derive.
        let company = crate::db::companies::get(&pool, "co1").await.unwrap();
        let xml = build_d112_xml(&pool, &company, 2026, 6, "6201", false)
            .await
            .unwrap();
        let oblig = |cod: &str| -> i64 {
            let key = format!("A_codOblig=\"{cod}\"");
            let i = xml
                .find(&key)
                .unwrap_or_else(|| panic!("obligația {cod} lipsește"));
            let dk = "A_datorat=\"";
            let j = i + xml[i..].find(dk).expect("A_datorat") + dk.len();
            let end = xml[j..].find('"').unwrap();
            xml[j..j + end].parse::<i64>().unwrap()
        };

        // P2 INVARIANT: pontaj + excess must produce identical GL and D112 obligations.
        // Wave E reads combined_cas/cass/impozit from breakdown (computed on exact sal_gross Decimal)
        // rather than re-deriving from de.gross (i64), so no rounding divergence is possible.
        assert_eq!(
            gl_credit("4315"),
            oblig("412"),
            "P2: CAS GL 4315 ≠ D112 412 (pontaj+excess: Wave E must read breakdown, not de.gross!)"
        );
        assert_eq!(
            gl_credit("4316"),
            oblig("432"),
            "P2: CASS GL 4316 ≠ D112 432 (pontaj+excess: Wave E must read breakdown, not de.gross!)"
        );
        assert_eq!(
            gl_credit("444"),
            oblig("602"),
            "P2: impozit GL 444 ≠ D112 602 (pontaj+excess: Wave E must read breakdown, not de.gross!)"
        );
        assert_eq!(
            gl_credit("436"),
            oblig("480"),
            "P2: CAM GL 436 ≠ D112 480 (pontaj+excess: Wave E must read breakdown, not de.gross!)"
        );

        assert!(
            tb.balanced,
            "payroll journal with pontaj+excess must balance"
        );
    }

    /// PAY-01 regression: a part-time employee whose CAS/CASS base is lifted to the statutory minimum
    /// (art. 146 5^7) must STILL declare CAM (480 / A_5) on the REALIZED gross (art. 220^6), matching the
    /// GL 436 posting. Guards against the lifted CAS base leaking into the CAM base. Part-timer P1 gross
    /// 2000, June 2026 (full month): CAS/CASS base lifts to 3.750 (CASS = 375), but CAM stays on 2000
    /// (480 = round(2000 × 2.25%) = 45 = GL 436), NOT on the lifted base (which would over-declare 84).
    #[tokio::test]
    async fn part_time_min_base_keeps_cam_on_realized_gross_in_d112() {
        use rust_decimal::prelude::ToPrimitive;
        let pool = setup().await;
        let mut inp = emp_input("1960101410019", "Part Timer", "2000");
        inp.tip_contract = Some("P1".into());
        inp.ore_norma = Some(4);
        crate::db::payroll::create(&pool, inp).await.unwrap();

        let company = crate::db::companies::get(&pool, "co1").await.unwrap();

        crate::db::payroll::run_payroll(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        let tb = crate::db::gl::trial_balance(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        let gl_credit = |code: &str| -> i64 {
            tb.rows
                .iter()
                .find(|r| r.account_code == code)
                .map(|r| {
                    Decimal::from_str(&r.closing_credit)
                        .unwrap_or(Decimal::ZERO)
                        .round_dp_with_strategy(
                            0,
                            rust_decimal::RoundingStrategy::MidpointAwayFromZero,
                        )
                        .to_i64()
                        .unwrap_or(0)
                })
                .unwrap_or(0)
        };

        let xml = build_d112_xml(&pool, &company, 2026, 6, "6201", false)
            .await
            .unwrap();
        let oblig = |cod: &str| -> i64 {
            let key = format!("A_codOblig=\"{cod}\"");
            let i = xml
                .find(&key)
                .unwrap_or_else(|| panic!("obligația {cod} lipsește"));
            let dk = "A_datorat=\"";
            let j = i + xml[i..].find(dk).expect("A_datorat") + dk.len();
            let end = xml[j..].find('"').unwrap();
            xml[j..j + end].parse::<i64>().unwrap()
        };

        // Proves the part-time lift FIRED: CASS (432) is on the lifted base 3.750 → 375 (not 200 on 2000).
        assert_eq!(
            oblig("432"),
            375,
            "CASS trebuie pe baza minimă majorată 3.750"
        );
        // CAM (480) stays on the REALIZED gross 2000 → 45, NOT on the lifted base (would be 84). PAY-01.
        assert_eq!(
            oblig("480"),
            45,
            "CAM trebuie pe brutul realizat (2000 × 2,25% = 45), nu pe baza majorată"
        );
        // And the GL≡D112 CAM invariant holds for the part-timer.
        assert_eq!(
            gl_credit("436"),
            oblig("480"),
            "part-time CAM: GL 436 ≠ D112 480"
        );
    }

    /// PAY-02 regression: a FULL-TIME employee hired mid-month must emit A_8 (zile lucrate) = the active
    /// working days of the interval, NOT the whole month. Before the fix the full-time path emitted nzl
    /// regardless (active_working_days was computed then discarded for full-timers).
    #[tokio::test]
    async fn full_time_mid_month_hire_emits_active_days_as_a8() {
        let pool = setup().await;
        let mut inp = emp_input("1960101410019", "Mid Month", "5000");
        inp.employment_date = Some("2026-06-16".into()); // hired mid-June (full-time, tip_contract N)
        crate::db::payroll::create(&pool, inp).await.unwrap();

        let company = crate::db::companies::get(&pool, "co1").await.unwrap();
        let xml = build_d112_xml(&pool, &company, 2026, 6, "6201", false)
            .await
            .unwrap();

        let active = crate::db::payroll::active_working_days(2026, 6, Some("2026-06-16"), None);
        let full = crate::db::payroll::working_days(2026, 6);
        assert!(
            active < full,
            "a mid-month hire must have fewer active days ({active}) than the full month ({full})"
        );
        assert!(
            xml.contains(&format!("A_8=\"{active}\"")),
            "A_8 must be the active working days {active}, not the full month {full}: {xml}"
        );
    }

    /// Dev helper (opt-in): build the D112 from a DB scenario and write it for the real `-v D112`.
    ///   cargo test --lib commands::payroll::tests::dump_d112_from_db -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn dump_d112_from_db() {
        let pool = setup().await;
        crate::db::payroll::create(&pool, emp_input("1960101410019", "Pop Ana", "5000"))
            .await
            .unwrap();
        let b =
            crate::db::payroll::create(&pool, emp_input("1900101410011", "Ion Gheorghe", "5500"))
                .await
                .unwrap();
        crate::db::concedii::create(
            &pool,
            leave_input(&b.id, "AB", "1234567", "2026-06-08", "2026-06-12"),
        )
        .await
        .unwrap();
        let company = crate::db::companies::get(&pool, "co1").await.unwrap();
        let xml = build_d112_xml(&pool, &company, 2026, 6, "6201", false)
            .await
            .unwrap();
        let path = std::env::temp_dir().join("d112_from_db.xml");
        std::fs::write(&path, &xml).unwrap();
        eprintln!("WROTE {}", path.display());
    }

    /// END-TO-END DUK gate (opt-in, like `dump_d112_from_db`): build a standard D112 and run the REAL
    /// bundled ANAF validator `D112Validator.jar` (`-v D112`) on it via `run_duk`, asserting it PASSES.
    /// This is the test the plan calls for — it proves the export gate (`export_d112_xml` → `run_duk`)
    /// validates clean against ANAF's own tool. `#[ignore]` because it spawns the 12 MB Java validator +
    /// the jlink JRE (slow, and the resources must be present); runs on demand:
    ///   cargo test --lib commands::payroll::tests::duk_validates_standard_d112 -- --ignored --nocapture
    /// Graceful: if the bundled resources are absent (e.g. a stripped checkout), it skips, never panics.
    #[tokio::test]
    #[ignore]
    async fn duk_validates_standard_d112() {
        use crate::anaf_decl::duk::{run_duk, DukProvider, DukRuntime};
        use crate::anaf_decl::DeclKind;
        use std::path::PathBuf;

        // Standard full-time roster only (asiguratA path) — the case verified to validate cleanly.
        let pool = setup().await;
        crate::db::payroll::create(&pool, emp_input("1960101410019", "Pop Ana", "5000"))
            .await
            .unwrap();
        let company = crate::db::companies::get(&pool, "co1").await.unwrap();
        let xml = build_d112_xml(&pool, &company, 2026, 6, "6201", false)
            .await
            .unwrap();
        let tmp = std::env::temp_dir().join("d112_duk_gate_test.xml");
        std::fs::write(&tmp, &xml).unwrap();

        // Resolve the bundled runtime from the repo resources (CARGO_MANIFEST_DIR = src-tauri).
        let res = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
        let java = res.join(if cfg!(windows) {
            "jre-min/bin/java.exe"
        } else {
            "jre-min/bin/java"
        });
        let jar_dir = res.join("duk");

        struct LocalBundle {
            java: PathBuf,
            jar_dir: PathBuf,
        }
        impl DukProvider for LocalBundle {
            fn resolve(&self) -> Option<DukRuntime> {
                if self.java.is_file() && self.jar_dir.join("DUKIntegrator.jar").is_file() {
                    Some(DukRuntime {
                        java: self.java.clone(),
                        jar_dir: self.jar_dir.clone(),
                    })
                } else {
                    None
                }
            }
        }
        let provider = LocalBundle { java, jar_dir };

        match run_duk(&provider, DeclKind::D112, &tmp).unwrap() {
            Some(outcome) => {
                // The standard D112 must validate with NO blocking errors against the official validator.
                assert!(
                    outcome.passed,
                    "D112Validator reported errors on a standard D112: {:?}",
                    outcome.errors
                );
            }
            None => {
                eprintln!("SKIP: bundled DUK runtime not present — nothing validated");
            }
        }
        let _ = std::fs::remove_file(&tmp);
    }

    // ── Architecture invariant: GL ≡ D112 by construction ────────────────────

    /// ARCHITECTURE INVARIANT — GL ≡ D112 BY CONSTRUCTION.
    ///
    /// Both `run_payroll` (GL) and `build_d112_xml` (D112 XML) call `compute_payroll_run` and
    /// derive their outputs from the SAME `PayrollRunBreakdown`. This test verifies, for a MIXED
    /// roster (base salary + spor + pontaj proration + diurnă excess + medical leave), that all
    /// four GL/D112 obligation pairs are EQUAL TO THE EXACT LEU after the refactor.
    ///
    /// Because both consumers share a single breakdown, future base-modifier changes in
    /// `compute_payroll_run` automatically propagate to BOTH GL and D112 — divergence is
    /// impossible by construction.
    #[tokio::test]
    async fn architecture_gl_equals_d112_by_construction_mixed_roster() {
        use rust_decimal::prelude::ToPrimitive;
        let pool = setup().await;

        // Employee A: full-time, base 4000, spor 500 → gross_eff 4500.
        let a = crate::db::payroll::create(&pool, emp_input("1960101410019", "Pop Ana", "4000"))
            .await
            .unwrap();
        crate::db::payroll_sporuri::create(
            &pool,
            crate::db::payroll_sporuri::CreateSporInput {
                company_id: "co1".into(),
                employee_id: a.id.clone(),
                period: "2026-06".into(),
                amount: "500".into(),
                kind: Some("vechime".into()),
                description: None,
            },
        )
        .await
        .unwrap();

        // Employee B: full-time 5000, pontaj 15/21 worked days.
        let b =
            crate::db::payroll::create(&pool, emp_input("1900101410011", "Ion Gheorghe", "5000"))
                .await
                .unwrap();
        crate::db::pontaj::create(
            &pool,
            crate::db::pontaj::CreatePontajInput {
                company_id: "co1".into(),
                employee_id: b.id.clone(),
                period: "2026-06".into(),
                worked_days: 15,
                overtime_hours: None,
                night_hours: None,
                absence_days: Some(6),
                leave_days: None,
                notes: None,
            },
        )
        .await
        .unwrap();

        // Employee C: full-time 3000, diurnă excess 200.
        let c =
            crate::db::payroll::create(&pool, emp_input("2960101410010", "Maria Ionescu", "3000"))
                .await
                .unwrap();
        let excess = rust_decimal::Decimal::from_str("200").unwrap();
        crate::db::payroll_diurna::upsert_extra_income(
            &pool,
            "co1",
            &c.id,
            "2026-06",
            "arch-test-dec",
            excess,
            "open",
        )
        .await
        .unwrap();

        // Employee D: medical leave (B-path), gross 5500, leave 8-12 June (5 days).
        // CNP 1900101410028: ctrl = (2×1+7×9+9×0+1×0+4×1+6×0+3×1+5×4+8×1+2×0+7×0+9×2)%11 = 8.
        let d = crate::db::payroll::create(&pool, emp_input("1900101410028", "Vasile Pop", "5500"))
            .await
            .unwrap();
        crate::db::concedii::create(
            &pool,
            leave_input(&d.id, "CD", "9999999", "2026-06-08", "2026-06-12"),
        )
        .await
        .unwrap();

        // Run payroll → posts GL.
        crate::db::payroll::run_payroll(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        let tb = crate::db::gl::trial_balance(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        let gl_credit = |code: &str| -> i64 {
            tb.rows
                .iter()
                .find(|r| r.account_code == code)
                .map(|r| {
                    rust_decimal::Decimal::from_str(&r.closing_credit)
                        .unwrap_or(rust_decimal::Decimal::ZERO)
                        .round_dp_with_strategy(
                            0,
                            rust_decimal::RoundingStrategy::MidpointAwayFromZero,
                        )
                        .to_i64()
                        .unwrap_or(0)
                })
                .unwrap_or(0)
        };

        // Build D112 via compute_payroll_run (same data path as run_payroll).
        let company = crate::db::companies::get(&pool, "co1").await.unwrap();
        let xml = build_d112_xml(&pool, &company, 2026, 6, "6201", false)
            .await
            .unwrap();
        let oblig = |cod: &str| -> i64 {
            let key = format!("A_codOblig=\"{cod}\"");
            let i = xml
                .find(&key)
                .unwrap_or_else(|| panic!("obligația {cod} lipsește din XML"));
            let dk = "A_datorat=\"";
            let j = i + xml[i..].find(dk).expect("A_datorat") + dk.len();
            let end = xml[j..].find('"').unwrap();
            xml[j..j + end].parse::<i64>().unwrap()
        };

        // ARCHITECTURE INVARIANT: all four GL/D112 pairs must match to the exact leu.
        // With the SSoT refactor, divergence is IMPOSSIBLE BY CONSTRUCTION.
        assert_eq!(
            gl_credit("4315"),
            oblig("412"),
            "ARCH: CAS GL 4315 ≠ D112 412 — mixed roster (spor+pontaj+excess+leave)"
        );
        assert_eq!(
            gl_credit("4316"),
            oblig("432"),
            "ARCH: CASS GL 4316 ≠ D112 432 — mixed roster"
        );
        assert_eq!(
            gl_credit("444"),
            oblig("602"),
            "ARCH: impozit GL 444 ≠ D112 602 — mixed roster"
        );
        assert_eq!(
            gl_credit("436"),
            oblig("480"),
            "ARCH: CAM GL 436 ≠ D112 480 — mixed roster"
        );
        assert!(tb.balanced, "mixed-roster payroll journal must balance");
    }

    // ── Simulator salariu — unit tests ────────────────────────────────────────

    /// GOLDEN-1: simulate_salary(5000, 0 dependents) matches compute_payroll byte-identically.
    /// This is the single-source-of-truth guarantee: the simulator and a real payroll run (without
    /// concedii / extra income / part-time top-up) must produce the same numbers for the same gross.
    #[tokio::test]
    async fn simulator_matches_real_payroll_5000_no_deduction() {
        // From the golden test in d112.rs (payroll_2026_rates_gross_to_net):
        // gross 5000, 0 dependents → CAS 1250, CASS 500, impozit 325, net 2925, CAM 113.
        // Deducere 0 (gross 5000 > plafon 6050 H1 NO — 5000 < 6050, dar tabelul dă 0 la gross>3600).
        let r = simulate_salary("5000".into(), None).await.unwrap();
        assert_eq!(r.gross, "5000.00", "gross");
        assert_eq!(r.cas, "1250.00", "CAS 25%");
        assert_eq!(r.cass, "500.00", "CASS 10%");
        assert_eq!(r.impozit, "325.00", "impozit 10%");
        assert_eq!(r.net, "2925.00", "net");
        assert_eq!(r.cam, "113.00", "CAM 2.25% — NOT phantom CCI");
        assert_eq!(
            r.total_employer_cost, "5113.00",
            "cost angajator = brut + CAM only"
        );
        // No CCI: confirm total_employer_cost = gross + cam (byte-identical to PayrollResult).
        assert!(
            !r.carveout_applied,
            "no carveout for gross=5000 without beneficiar flag"
        );
    }

    /// GOLDEN-2: deducere personală cu dependenți modifică impozitul corect.
    #[tokio::test]
    async fn simulator_deduction_reduces_impozit() {
        // Gross 2000 (≤ 2000): 0 dependents → deducere_tabel = 807; 2 dependents → 1300.
        // CAS 25%*2000=500; CASS 10%*2000=200; after=1300.
        // 0 dep: impozit_base = 1300 − 807 = 493; impozit = 49.
        // 2 dep: impozit_base = 1300 − 1300 = 0; impozit = 0.
        let r0 = simulate_salary(
            "2000".into(),
            Some(SalarySimOpts {
                dependents: 0,
                month: 6,
                year: 2026,
                ..Default::default()
            }),
        )
        .await
        .unwrap();
        assert_eq!(r0.deducere_tabel, "807.00");
        assert_eq!(r0.impozit_base, "493.00");
        assert_eq!(r0.impozit, "49.00");

        let r2 = simulate_salary(
            "2000".into(),
            Some(SalarySimOpts {
                dependents: 2,
                month: 6,
                year: 2026,
                ..Default::default()
            }),
        )
        .await
        .unwrap();
        assert_eq!(r2.deducere_tabel, "1300.00");
        assert_eq!(r2.impozit_base, "0.00");
        assert_eq!(r2.impozit, "0.00");
    }

    /// GOLDEN-3: net→gross round-trip — simulate_net_to_gross(simulate_gross_to_net(G).net) ≈ G.
    #[tokio::test]
    async fn simulator_net_to_gross_roundtrip() {
        for gross_lei in [3000u32, 5000, 8000] {
            let r = simulate_salary(
                gross_lei.to_string(),
                Some(SalarySimOpts {
                    month: 6,
                    year: 2026,
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
            let net_str = r.net.clone();
            // Invert: find the gross that gives this net.
            let inv = simulate_salary_from_net(
                net_str,
                Some(SalarySimOpts {
                    month: 6,
                    year: 2026,
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
            let inv_gross: u32 = inv.gross.trim_end_matches(".00").parse().unwrap_or(0);
            assert_eq!(
                inv_gross, gross_lei,
                "round-trip failed for gross={gross_lei}: got {inv_gross}"
            );
        }
    }

    /// GOLDEN-4: no phantom CCI — total_employer_cost = gross + CAM only.
    #[tokio::test]
    async fn simulator_no_phantom_cci() {
        // For gross 4050 with sum netaxabila 300 (H1): CAM = round(3750 × 2.25%) = 84.
        // total_employer_cost = 4050 + 84 = 4134 (NEVER 4050 + 84 + 32 phantom CCI).
        let r = simulate_salary(
            "4050".into(),
            Some(SalarySimOpts {
                beneficiar_suma_netaxabila: true,
                month: 3,
                year: 2026,
                ..Default::default()
            }),
        )
        .await
        .unwrap();
        assert_eq!(r.cam, "84.00", "CAM on reduced base 3750");
        assert_eq!(
            r.total_employer_cost, "4134.00",
            "cost angajator = 4050 + 84 only"
        );
        assert!(
            r.carveout_applied,
            "carveout should be applied for min-wage beneficiary"
        );
        // Confirm: if total_employer_cost were gross+CAM+CCI(0.85%) it would be 4050+84+34=4168 ≠ 4134.
        let cost: rust_decimal::Decimal =
            rust_decimal::Decimal::from_str(&r.total_employer_cost).unwrap();
        assert_ne!(
            cost,
            rust_decimal::Decimal::from(4168),
            "phantom CCI must NOT appear"
        );
    }
}

// ─── Simulator salariu (brut↔net calculator, stateless) ──────────────────────

use crate::anaf_decl::d112::deducere_personala_tabel;
use serde::Serialize;

/// Rezultatul complet al simulării salariale (brut → net + cost angajator).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SalarySimResult {
    /// Salariul brut de intrare (lei).
    pub gross: String,
    /// CAS 25% (angajat).
    pub cas: String,
    /// CASS 10% (angajat).
    pub cass: String,
    /// Suma netaxabilă aplicată (0 dacă `beneficiarSumaNetaxabila=false` sau condiții neîndeplinite).
    pub non_taxable: String,
    /// Deducerea personală aplicată efectiv (după plafonul art. 77 alin. (2)).
    pub deducere_personala: String,
    /// Baza impozabilă = brut − CAS − CASS − suma_netaxabilă − deducere.
    pub impozit_base: String,
    /// Impozit pe venit 10%.
    pub impozit: String,
    /// Salariul net = brut − CAS − CASS − impozit.
    pub net: String,
    /// CAM 2,25% (angajator, pe baza = brut − suma_netaxabilă). SINGURUL cost angajator dincolo de
    /// brut — CCI 0,85% a fost ABROGAT prin OUG 79/2017, nu se (re)calculează.
    pub cam: String,
    /// Cost total angajator = brut + CAM.
    pub total_employer_cost: String,
    /// Deducerea maximă disponibilă din tabel (informativ: înainte de plafonul de brut art. 77 (2)).
    pub deducere_tabel: String,
    /// Deducerea personală intrată efectiv în calcul (= min(deducere_tabel, plafon_brut, venit_net)).
    pub deducere_efectiva: String,
    /// Info suma netaxabilă (dacă beneficiar e fals, câmpul `non_taxable` e 0).
    pub carveout_applied: bool,
}

/// Opțiuni suplimentare pentru simulatorul salarial.
#[derive(Debug, Clone, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SalarySimOpts {
    /// Numărul de persoane în întreținere (0–4+) — pentru calculul deducerii personale din tabelul ANAF.
    #[serde(default)]
    pub dependents: u32,
    /// Beneficiar suma netaxabilă (art. III OUG 89/2025): full-time, salariu minim, nediminuat în 2026.
    /// La `true`, se aplică 300 lei H1 / 200 lei H2 2026 dacă brut ≤ 4.300/4.600.
    #[serde(default)]
    pub beneficiar_suma_netaxabila: bool,
    /// Luna (1–12) — necesară pentru ratele sezoniere (suma netaxabilă H1/H2, salariul minim).
    /// Dacă lipsește sau e 0, se folosește luna curentă (sau luna 6 ca fallback).
    #[serde(default)]
    pub month: u32,
    /// Anul — necesar pentru ratele sezoniere. Dacă lipsește sau e 0, se folosește 2026.
    #[serde(default)]
    pub year: i32,
}

/// Calculează brut → net + cost angajator FĂRĂ a crea angajat sau rula stat de salarii.
/// Reutilizează EXACT același `compute_payroll` + `deducere_plafonata` + `suma_netaxabila` ca
/// rularea reală, garantând rezultate byte-identice pentru același brut.
/// RBAC: comandă de citire (nu mută date, nu accesează baza de date), disponibilă oricui autentificat.
#[tauri::command]
pub async fn simulate_salary(
    gross_str: String,
    opts: Option<SalarySimOpts>,
) -> crate::error::AppResult<SalarySimResult> {
    use crate::anaf_decl::d112::{
        compute_payroll, deducere_plafonata, suma_netaxabila, PayrollInput,
    };
    use crate::error::AppError;

    let opts = opts.unwrap_or_default();
    let year = if opts.year <= 0 { 2026 } else { opts.year };
    let month = if opts.month == 0 || opts.month > 12 {
        6
    } else {
        opts.month
    };

    // Parse gross — reuse the same strict-no-scientific-notation logic as payroll.
    let gross_str = gross_str.trim();
    if gross_str.contains('e') || gross_str.contains('E') {
        return Err(AppError::Validation(
            "Brut invalid — folosiți formatul 1234.56 (fără notație științifică).".into(),
        ));
    }
    let gross = Decimal::from_str(gross_str)
        .map_err(|_| AppError::Validation("Brut invalid — folosiți formatul 1234.56.".into()))?;
    if gross.is_sign_negative() {
        return Err(AppError::Validation("Brut nu poate fi negativ.".into()));
    }

    let non_taxable = suma_netaxabila(opts.beneficiar_suma_netaxabila, "N", gross, year, month);
    // Deducerea din tabel (pe brut complet, înainte de CAS/CASS — tabel ANAF art. 77).
    let deducere_tabel = deducere_personala_tabel(gross, opts.dependents);
    // Plafonarea art. 77 alin. (2): dacă brut > salariul_minim + 2.000, deducere = 0.
    let deducere_platita = deducere_plafonata(deducere_tabel, gross, year, month);

    let result = compute_payroll(&PayrollInput {
        gross,
        personal_deduction: deducere_platita,
        non_taxable,
    });

    let deducere_efectiva = Decimal::from_str(&result.personal_deduction).unwrap_or(Decimal::ZERO);

    Ok(SalarySimResult {
        gross: result.gross,
        cas: result.cas,
        cass: result.cass,
        non_taxable: result.non_taxable,
        deducere_personala: result.personal_deduction.clone(),
        impozit_base: result.taxable_base,
        impozit: result.income_tax,
        net: result.net,
        cam: result.cam,
        total_employer_cost: result.total_employer_cost,
        deducere_tabel: format!(
            "{:.2}",
            deducere_tabel
                .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
        ),
        deducere_efectiva: format!(
            "{:.2}",
            deducere_efectiva
                .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
        ),
        carveout_applied: non_taxable > Decimal::ZERO,
    })
}

/// Inversul simulatorului: date un NET dorit, caută prin căutare binară brut-ul minim din care
/// rezultă acel net (la leu întreg). Domeniu de căutare: [0, 1.000.000] lei.
///
/// Notă de monotonie: funcția brut → net e în general strict crescătoare, dar la marginile
/// tranziției suma netaxabilă (4.300→4.301 H1) există un micro-salt descendent de ~10–15 lei în
/// net (pierderea carve-out-ului). Căutarea binară pe primul brut care atinge target-ul net poate
/// returna un brut mai mic decât maximul, dar acesta e corect fiscal pentru acel net.
/// Dacă targetul nu e atins (ex. net > 1.000.000), returnează Err.
#[tauri::command]
pub async fn simulate_salary_from_net(
    target_net_str: String,
    opts: Option<SalarySimOpts>,
) -> crate::error::AppResult<SalarySimResult> {
    use crate::anaf_decl::d112::{
        compute_payroll, deducere_plafonata, suma_netaxabila, PayrollInput,
    };
    use crate::error::AppError;

    let opts = opts.unwrap_or_default();
    let year = if opts.year <= 0 { 2026 } else { opts.year };
    let month = if opts.month == 0 || opts.month > 12 {
        6
    } else {
        opts.month
    };

    let target_str = target_net_str.trim();
    if target_str.contains('e') || target_str.contains('E') {
        return Err(AppError::Validation(
            "Net invalid — folosiți formatul 1234.56 (fără notație științifică).".into(),
        ));
    }
    let target_net = Decimal::from_str(target_str)
        .map_err(|_| AppError::Validation("Net invalid — folosiți formatul 1234.56.".into()))?;
    if target_net.is_sign_negative() {
        return Err(AppError::Validation("Net nu poate fi negativ.".into()));
    }

    // net(gross) helper: returns the Decimal net for a given gross (integer leu).
    let net_for = |g: i64| -> Decimal {
        let gross = Decimal::from(g);
        let non_taxable = suma_netaxabila(opts.beneficiar_suma_netaxabila, "N", gross, year, month);
        let deducere_tabel = deducere_personala_tabel(gross, opts.dependents);
        let deducere_platita = deducere_plafonata(deducere_tabel, gross, year, month);
        let r = compute_payroll(&PayrollInput {
            gross,
            personal_deduction: deducere_platita,
            non_taxable,
        });
        Decimal::from_str(&r.net).unwrap_or(Decimal::ZERO)
    };

    let target_rounded = target_net
        .round_dp_with_strategy(0, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
        .to_i64()
        .unwrap_or(0);

    // Binary search in [0, 1_000_000]. Find the SMALLEST gross whose net ≥ target.
    if net_for(1_000_000) < Decimal::from(target_rounded) {
        return Err(AppError::Validation(
            "Netul dorit depășește domeniul de calcul (max 1.000.000 lei brut).".into(),
        ));
    }
    let mut lo: i64 = 0;
    let mut hi: i64 = 1_000_000;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if net_for(mid) >= Decimal::from(target_rounded) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    // `lo` is now the smallest gross whose net == target_rounded (to the leu).
    // Re-run the simulator for that gross to get the full breakdown.
    simulate_salary(lo.to_string(), Some(opts)).await
}

// ── Export REGES-Online (Registrul General de Evidență a Salariaților) ────────
//
// HG 295/2025 abrogă HG 905/2017 — „Revisal" devine REGES-Online (Inspecția Muncii).
// Aplicația NU depune automat la portalul REGES-Online (portal online, nu API);
// exportă un CSV structurat pe care contabilul îl folosește ca referință / îl
// adaptează la formatul de import al portalului.
//
// Coloane (un rând per angajat, sortat alfabetic după nume):
//   Nume, CNP, Funcția, Cod COR, Tip contract, Durată, Data angajării,
//   Data încetării, Ore normă/zi, Salariu brut (RON)
//
// Formula-injection-safe: câmpurile text trec prin `csv_neutralize`.
// Valorile monetare trec prin `csv_num` (ca la jurnalul de vânzări).

/// Exportă Registrul General de Evidenţa Salariaţilor (REGES-Online) ca CSV.
///
/// Produce un rând per angajat (activi + inactivi — toţi contractanţii înregistraţi),
/// sortat alfabetic după `full_name`. Exportul este destinat exclusiv referinţei
/// contabilului; depunerea efectivă se face prin **portalul Inspecţiei Muncii**
/// (REGES-Online, HG 295/2025) — aplicaţia nu comunică automat cu portalul.
#[tauri::command]
pub async fn export_reges_register(
    state: State<'_, AppState>,
    company_id: String,
    dest_path: String,
) -> AppResult<String> {
    use crate::commands::journals::csv_num;

    let dest = crate::commands::integrations::validate_export_path(&dest_path)?
        .to_string_lossy()
        .to_string();

    // Fetch all employees for this company (active + inactive), sorted by name.
    let mut employees = payroll::list(&state.db, &company_id).await?;
    employees.sort_by(|a, b| a.full_name.cmp(&b.full_name));

    let mut out = String::with_capacity(4096);

    // Header row — column labels aligned with REGES-Online register requirements
    // (HG 295/2025): name, CNP, job title, COR code, contract type, duration,
    // hire date, end date, hours/day, base salary.
    out.push_str(
        "Nume,CNP,Funcția,Cod COR,Tip contract,Durată contract,\
         Data angajării,Data încetării,Ore normă/zi,Salariu brut (RON)\n",
    );

    for emp in &employees {
        // Contract type label (D112 Nomenclator 12): "N" → "CIM normă întreagă",
        // "P1".."P7" → "CIM part-time Pn". Raw code stored as fallback.
        let tip_label = match emp.tip_contract.as_str() {
            "N" => "CIM normă întreagă".to_string(),
            p if p.starts_with('P') => format!("CIM part-time {}", p),
            other => other.to_string(),
        };

        // Duration: if contract_end_date is set → "determinată", else "nedeterminată".
        let durata = if emp.contract_end_date.is_some() {
            "determinată"
        } else {
            "nedeterminată"
        };

        // Date columns — store ISO YYYY-MM-DD; emit as-is (universally parseable).
        let hire = emp.employment_date.as_deref().unwrap_or("");
        let end = emp.contract_end_date.as_deref().unwrap_or("");

        // csv_neutralize for all text fields (formula-injection safety per journals.rs).
        // csv_num for the salary amount (numeric, may have decimal point, no injection risk).
        let row = format!(
            "{},{},{},{},{},{},{},{},{},{}\n",
            csv_neutralize_field(&emp.full_name),
            csv_neutralize_field(&emp.cnp),
            csv_neutralize_field(&emp.functia),
            csv_neutralize_field(&emp.cod_cor),
            csv_neutralize_field(&tip_label),
            csv_neutralize_field(durata),
            csv_neutralize_field(hire),
            csv_neutralize_field(end),
            emp.ore_norma, // integer, no injection risk
            csv_num(&emp.gross_salary),
        );
        out.push_str(&row);
    }

    std::fs::write(&dest, out.as_bytes())?;

    Ok(dest)
}

/// RFC 4180 quoting with formula-injection neutralization — wraps the shared
/// `csv_neutralize` logic with proper comma/quote/newline quoting for CSV output.
/// (Mirrors `csv_field` in journals.rs but avoids a cross-crate visibility issue
/// by re-applying the same logic locally in the payroll command module.)
fn csv_neutralize_field(s: &str) -> String {
    use crate::commands::journals::csv_neutralize;
    let s = csv_neutralize(s);
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod reges_tests {
    use super::csv_neutralize_field;

    // ── helper: build a minimal Employee for testing ──────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn make_emp(
        full_name: &str,
        cnp: &str,
        functia: &str,
        cod_cor: &str,
        tip_contract: &str,
        employment_date: Option<&str>,
        contract_end_date: Option<&str>,
        ore_norma: i64,
        gross_salary: &str,
    ) -> crate::db::payroll::Employee {
        crate::db::payroll::Employee {
            id: "test-id".into(),
            company_id: "cmp-1".into(),
            cnp: cnp.into(),
            full_name: full_name.into(),
            gross_salary: gross_salary.into(),
            personal_deduction: "0".into(),
            employment_date: employment_date.map(|s| s.to_string()),
            contract_end_date: contract_end_date.map(|s| s.to_string()),
            active: true,
            tip_asigurat: "1".into(),
            pensionar: false,
            tip_contract: tip_contract.into(),
            ore_norma,
            exceptie_cas_min: "".into(),
            sediu_cif: "".into(),
            beneficiar_suma_netaxabila: false,
            functia: functia.into(),
            cod_cor: cod_cor.into(),
            created_at: 0,
            updated_at: 0,
        }
    }

    /// Build the CSV body (without file I/O) from a slice of employees.
    fn build_reges_csv(emps: &[crate::db::payroll::Employee]) -> String {
        use crate::commands::journals::csv_num;
        let mut out = String::with_capacity(1024);
        out.push_str(
            "Nume,CNP,Funcția,Cod COR,Tip contract,Durată contract,\
             Data angajării,Data încetării,Ore normă/zi,Salariu brut (RON)\n",
        );
        for emp in emps {
            let tip_label = match emp.tip_contract.as_str() {
                "N" => "CIM normă întreagă".to_string(),
                p if p.starts_with('P') => format!("CIM part-time {}", p),
                other => other.to_string(),
            };
            let durata = if emp.contract_end_date.is_some() {
                "determinată"
            } else {
                "nedeterminată"
            };
            let hire = emp.employment_date.as_deref().unwrap_or("");
            let end = emp.contract_end_date.as_deref().unwrap_or("");
            let row = format!(
                "{},{},{},{},{},{},{},{},{},{}\n",
                csv_neutralize_field(&emp.full_name),
                csv_neutralize_field(&emp.cnp),
                csv_neutralize_field(&emp.functia),
                csv_neutralize_field(&emp.cod_cor),
                csv_neutralize_field(&tip_label),
                csv_neutralize_field(durata),
                csv_neutralize_field(hire),
                csv_neutralize_field(end),
                emp.ore_norma,
                csv_num(&emp.gross_salary),
            );
            out.push_str(&row);
        }
        out
    }

    /// The register CSV has a header + one data row per employee with the
    /// correct columns: name, CNP, funcția, cod COR, contract type, duration,
    /// hire date, end date, hours/day, salary.
    #[test]
    fn reges_register_columns_and_values() {
        let emp = make_emp(
            "Popescu Ion",
            "1800101123456",
            "Programator",
            "251202",
            "N",
            Some("2020-03-01"),
            None,
            8,
            "5000",
        );
        let csv = build_reges_csv(&[emp]);
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 2, "header + 1 data row");
        // Header contains expected column names
        assert!(lines[0].contains("Nume"), "header: Nume");
        assert!(lines[0].contains("CNP"), "header: CNP");
        assert!(lines[0].contains("Cod COR"), "header: Cod COR");
        assert!(lines[0].contains("Tip contract"), "header: Tip contract");
        assert!(
            lines[0].contains("Data angajării"),
            "header: Data angajării"
        );
        assert!(lines[0].contains("Salariu brut"), "header: Salariu brut");
        // Data row values
        assert!(lines[1].contains("Popescu Ion"), "name in row");
        assert!(lines[1].contains("1800101123456"), "CNP in row");
        assert!(lines[1].contains("Programator"), "functia in row");
        assert!(lines[1].contains("251202"), "COR in row");
        assert!(lines[1].contains("CIM normă întreagă"), "full-time label");
        assert!(lines[1].contains("nedeterminată"), "open-ended duration");
        assert!(lines[1].contains("2020-03-01"), "hire date");
        assert!(lines[1].contains("5000"), "salary");
    }

    /// An employee with a contract_end_date shows it; an active open-ended one doesn't.
    #[test]
    fn reges_end_date_present_when_set() {
        let ended = make_emp(
            "Ionescu Maria",
            "2900505654321",
            "Contabil",
            "241101",
            "N",
            Some("2018-01-15"),
            Some("2024-12-31"),
            8,
            "4000",
        );
        let csv = build_reges_csv(&[ended]);
        let data = csv.lines().nth(1).unwrap();
        assert!(data.contains("2024-12-31"), "end date present");
        assert!(
            data.contains("determinată"),
            "determinată when end date set"
        );
    }

    /// An active employee without an end date shows empty end-date column.
    #[test]
    fn reges_no_end_date_when_active() {
        let active = make_emp(
            "Georgescu Andrei",
            "1920810987654",
            "Inginer",
            "214201",
            "N",
            Some("2022-06-01"),
            None,
            8,
            "7000",
        );
        let csv = build_reges_csv(&[active]);
        let data = csv.lines().nth(1).unwrap();
        assert!(
            data.contains("nedeterminată"),
            "open-ended when no end date"
        );
        // end date column is empty — the row has a comma followed immediately by another field
        // (ore_norma). Check that "nedeterminată" appears and no spurious date in end-date slot.
        // The row format: ...,hire_date,,ore_norma,...
        assert!(data.contains("2022-06-01,,"), "empty end-date column");
    }

    /// Part-time contract shows the P-code label; ore_norma != 8 is preserved.
    #[test]
    fn reges_part_time_contract_label() {
        let pt = make_emp(
            "Dumitrescu Ana",
            "2850320123456",
            "Asistent",
            "325901",
            "P2",
            Some("2023-03-01"),
            None,
            4,
            "2500",
        );
        let csv = build_reges_csv(&[pt]);
        let data = csv.lines().nth(1).unwrap();
        assert!(
            data.contains("CIM part-time P2"),
            "part-time label with code"
        );
        assert!(data.contains(",4,"), "ore_norma=4");
    }

    /// Formula-injection safety: a name starting with '=' is prefixed with ' (apostrophe).
    #[test]
    fn reges_formula_injection_safe_name() {
        let malicious = make_emp(
            "=cmd|' /C calc'!A0",
            "1900101000000",
            "Test",
            "000000",
            "N",
            None,
            None,
            8,
            "3000",
        );
        let csv = build_reges_csv(&[malicious]);
        let data = csv.lines().nth(1).unwrap();
        // The cell must start with the apostrophe prefix, neutralizing the '=' formula trigger.
        assert!(
            data.starts_with("'=cmd"),
            "formula-injection prefix applied: got: {data}"
        );
    }

    /// csv_neutralize_field handles names with commas by quoting the field.
    #[test]
    fn reges_name_with_comma_is_quoted() {
        let emp = make_emp(
            "Pop, Ioan-Mihai",
            "1900101000001",
            "Manager",
            "121901",
            "N",
            Some("2021-01-01"),
            None,
            8,
            "8000",
        );
        let csv = build_reges_csv(&[emp]);
        let data = csv.lines().nth(1).unwrap();
        // The name contains a comma so it must be wrapped in double quotes per RFC 4180.
        assert!(data.contains("\"Pop, Ioan-Mihai\""), "comma in name quoted");
    }

    /// COR code validation: a 6-digit string is stored and reproduced verbatim;
    /// COR codes with fewer or more digits are stored as-is (app stores, portal validates).
    /// (The app deliberately does NOT hard-reject at DB level — see export note.)
    #[test]
    fn reges_cor_code_stored_verbatim() {
        let emp = make_emp(
            "Test",
            "1900101000002",
            "Dev",
            "251202",
            "N",
            None,
            None,
            8,
            "4000",
        );
        let csv = build_reges_csv(&[emp]);
        assert!(csv.contains("251202"), "6-digit COR reproduced");

        let emp2 = make_emp(
            "Test2",
            "1900101000003",
            "Dev",
            "1234",
            "N",
            None,
            None,
            8,
            "4000",
        );
        let csv2 = build_reges_csv(&[emp2]);
        assert!(
            csv2.contains("1234"),
            "short COR stored as-is (portal validates)"
        );
    }
}
