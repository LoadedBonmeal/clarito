//! Tauri commands — salarizare (angajați + stat de salarii lunar).

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::str::FromStr;
use tauri::State;

use crate::anaf_decl::d112::{
    cm_indemn_treatment, compute_payroll, compute_payroll_with_leave, deducere_plafonata,
    suma_netaxabila, LeaveCert, LeavePayrollInput, PayrollInput,
};
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
use crate::db::payroll::{active_working_days, leave_working_days_in_month, working_days};

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
    let employees = payroll::list(&state.db, &company_id).await?;
    let period_ym = format!("{year:04}-{month:02}");
    let leaves = crate::db::concedii::list(&state.db, &company_id, &period_ym).await?;
    let extra_income = crate::db::payroll_diurna::open_extra_income_by_employee(
        &state.db,
        &company_id,
        &period_ym,
    )
    .await
    .unwrap_or_default();
    let xml = build_d112_xml(
        &company,
        &employees,
        &leaves,
        year,
        month,
        caen.trim(),
        is_rectificative,
        &extra_income,
    )?;
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
    let _ = crate::db::declaration_filings::record(
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
    let employees = payroll::list(&state.db, &company_id).await?;
    let period_ym = format!("{year:04}-{month:02}");
    let leaves = crate::db::concedii::list(&state.db, &company_id, &period_ym).await?;
    let extra_income = crate::db::payroll_diurna::open_extra_income_by_employee(
        &state.db,
        &company_id,
        &period_ym,
    )
    .await
    .unwrap_or_default();
    build_d112_xml(
        &company,
        &employees,
        &leaves,
        year,
        month,
        caen.trim(),
        is_rectificative,
        &extra_income,
    )
}

/// Pure core of the D112 XML build (no Tauri State / filesystem) — maps active employees + the month's
/// medical-leave certificates to the validated `:v7` XML. Separated from [`export_d112_xml`] so it is
/// testable end-to-end without IPC/IO. An employee with ≥1 certificate emits the B-path (salary
/// proration + indemnity, consistent with `run_payroll`/GL); the rest emit the standard `asiguratA` path.
/// `extra_income` — map of `employee_id → total excess (Decimal, lei)` from `payroll_extra_income`
/// for the given period. Folded into the D112 bases (CAS/CASS/CAM/impozit) and E3_23 (rând 8.2.1).
#[allow(clippy::too_many_arguments)]
fn build_d112_xml(
    company: &crate::db::companies::Company,
    employees: &[Employee],
    leaves: &[crate::db::concedii::MedicalLeave],
    year: i32,
    month: u32,
    caen: &str,
    is_rectificative: bool,
    extra_income: &std::collections::HashMap<String, Decimal>,
) -> AppResult<String> {
    if caen.len() != 4 || !caen.chars().all(|c| c.is_ascii_digit()) {
        return Err(AppError::Validation(
            "Cod CAEN invalid — introduceți 4 cifre (ex. 6201).".into(),
        ));
    }
    // Concediile medicale ale lunii → calea B (asiguratD). Grupate pe angajat (gol ⇒ calea A standard).
    let mut leaves_by_emp: std::collections::HashMap<
        &str,
        Vec<&crate::db::concedii::MedicalLeave>,
    > = std::collections::HashMap::new();
    for l in leaves {
        leaves_by_emp
            .entry(l.employee_id.as_str())
            .or_default()
            .push(l);
    }
    let dec = |s: &str| Decimal::from_str(s).unwrap_or(Decimal::ZERO);
    // Whole-lei, COMMERCIAL rounding (MidpointAwayFromZero) — never banker's `.round()`.
    let leid = |d: Decimal| {
        d.round_dp_with_strategy(0, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
            .to_i64()
            .unwrap_or(0)
    };
    let lei = |s: &str| leid(dec(s));

    let nzl = working_days(year, month); // zile lucrătoare în lună (Luni-Vineri)
    let mut d112_emps = Vec::new();
    for e in employees.iter().filter(|e| e.active) {
        // ROB-17: reject malformed CNP before serializing — DUKIntegrator/ANAF reject it otherwise.
        if !crate::anaf_decl::valid_cnp(&e.cnp) {
            return Err(AppError::Validation(format!(
                "CNP invalid pentru angajatul „{}\" ({}): trebuie 13 cifre cu cifra de control corectă.",
                e.full_name, e.cnp
            )));
        }
        let gross_in = dec(&e.gross_salary);
        // Suma netaxabilă (300/200 lei, art. III OUG 89/2025) — 0 dacă nu se aplică. Scade baza
        // tuturor celor patru prelevări (vezi compute_payroll).
        let non_taxable = suma_netaxabila(
            e.beneficiar_suma_netaxabila,
            &e.tip_contract,
            gross_in,
            year,
            month,
        );

        // ── Calea B (concediu medical) ──────────────────────────────────────
        // Salariatul cu ≥1 certificat în lună: salariul se proratează la zilele lucrate, iar
        // indemnizația de CM intră în baza CAS/CASS (CAS 25% + CASS 10% pe codurile ne-scutite) și în
        // baza de impozit. `compute_payroll_with_leave` produce numerele lunii; `emit_leave_blocks`
        // (în d112_xml) reconstruiește identic CAS/CASS din baza lucrată + indemnizație ⇒ D112 = motor.
        if let Some(emp_leaves) = leaves_by_emp.get(e.id.as_str()) {
            // EDGE: part-time + concediu medical. Baza minimă part-time (art. 146 alin. (5^6)) NU se
            // aplică pe luna cu concediu — combinație rară + tratament statutar ambiguu (proratarea
            // bazei minime la zilele active e ea însăși neimplementată). Se avertizează pentru
            // verificare manuală; vezi nota din `db/concedii.rs`.
            if e.tip_contract != "N"
                && !crate::anaf_decl::d112::exempt_part_time_min_base(
                    e.pensionar,
                    &e.exceptie_cas_min,
                )
            {
                tracing::warn!(
                    employee = %e.full_name,
                    contract = %e.tip_contract,
                    "D112: salariat part-time cu concediu medical — baza minimă part-time (art. 146 \
                (5^6)) NU se aplică pe luna cu concediu; verificați manual"
                );
            }
            let mut certs = Vec::new();
            let mut med_leaves = Vec::new();
            for l in emp_leaves {
                let (cass_due, taxable) = cm_indemn_treatment(&l.cod_indemnizatie);
                let emp_amt = lei(&l.suma_angajator); // indemnizație în lei întregi (ca în D112)
                let fn_amt = lei(&l.suma_fnuass);
                certs.push(LeaveCert {
                    indemn_employer: Decimal::from(emp_amt),
                    indemn_fnuass: Decimal::from(fn_amt),
                    // Proratarea salariului scade TOATE zilele lucrătoare de concediu (din intervalul
                    // certificatului), inclusiv prima zi neplătită 2026 — nu doar zilele indemnizate.
                    leave_working_days: leave_working_days_in_month(
                        year,
                        month,
                        &l.data_inceput,
                        &l.data_sfarsit,
                    ),
                    cass_due,
                    taxable,
                });
                med_leaves.push(D112MedicalLeave {
                    serie: l.serie.clone(),
                    numar: l.numar.clone(),
                    cod_indemn: l.cod_indemnizatie.clone(),
                    data_acordare: ro_date(&l.data_acordare),
                    data_inceput: ro_date(&l.data_inceput),
                    data_sfarsit: ro_date(&l.data_sfarsit),
                    zile_ang: l.zile_angajator,
                    zile_fnuass: l.zile_fnuass,
                    baza_calcul: lei(&l.baza_calcul),
                    zile_baza: l.zile_baza,
                    suma_ang: emp_amt,
                    suma_fnuass: fn_amt,
                    procent: l.procent,
                    loc_prescriere: l.loc_prescriere,
                    cod_boala: l.cod_boala.clone(),
                });
            }
            let lr = compute_payroll_with_leave(&LeavePayrollInput {
                gross: gross_in,
                personal_deduction: deducere_plafonata(
                    dec(&e.personal_deduction),
                    gross_in,
                    year,
                    month,
                ),
                non_taxable,
                working_days: nzl,
                certs,
            });
            let worked_base = leid(lr.worked_base);
            let (nume, prenume) = split_name(&e.full_name);
            d112_emps.push(D112Employee {
                cnp: e.cnp.clone(),
                nume,
                prenume,
                data_ang: e
                    .employment_date
                    .as_deref()
                    .map(ro_date)
                    .unwrap_or_default(),
                gross: leid(lr.worked_gross),
                cas: leid(lr.cas),
                cass: leid(lr.cass),
                impozit: leid(lr.income_tax),
                cam: leid(lr.cam),
                zile: lr.worked_days,
                tip_asigurat: e.tip_asigurat.clone(),
                pensionar: e.pensionar,
                tip_contract: e.tip_contract.clone(),
                ore_norma: e.ore_norma.clamp(6, 8) as u32,
                baza_cas: worked_base,
                baza_cass: worked_base,
                baza_cam: worked_base,
                sal_contract: leid(gross_in),
                baza_impozit: leid(lr.taxable_base),
                deducere: lei(&e.personal_deduction),
                sediu_cif: e.sediu_cif.clone(),
                e3_23: 0, // set by Wave-E post-processing below if there is diurnă surplus
                med_leaves,
            });
            continue;
        }

        let r = compute_payroll(&PayrollInput {
            gross: gross_in,
            personal_deduction: deducere_plafonata(
                dec(&e.personal_deduction),
                gross_in,
                year,
                month,
            ),
            non_taxable,
        });
        let gross = dec(&r.gross);
        // Baza CAS/CASS = brut − suma netaxabilă (carve-out). Pentru ne-beneficiari nt=0 → baza = brut.
        let mut baza_cas = gross - non_taxable;
        let mut baza_cass = gross - non_taxable;
        // PAY-01: baza CAM (A_5) rămâne ÎNTOTDEAUNA pe brutul REALIZAT (art. 220^6), spre deosebire de
        // CAS/CASS care se majorează la baza minimă part-time (art. 146 5^6). O fixăm înainte de lift ca
        // A_5 + obligația 480 să coincidă cu postarea GL 436 (db/payroll.rs: (brut − netaxabil).max(0)).
        let baza_cam_real = (gross - non_taxable).max(Decimal::ZERO);
        let mut cas = dec(&r.cas);
        let mut cass = dec(&r.cass);
        // Part-time (contract Pi): baza CAS/CASS = salariul minim întreg (NU prorata cu norma orară),
        // categoriile art. 146 (5^7) exceptate — via the shared helper. Contribuția declarată e pe
        // baza majorată.
        let exempt =
            crate::anaf_decl::d112::exempt_part_time_min_base(e.pensionar, &e.exceptie_cas_min);
        // Zile lucrătoare active în lună (angajare la mijlocul lunii ⇒ < NZL). Folosit ca A_8 emis
        // ȘI pentru proratarea bazei minime part-time, ca să coincidă cu regula DUK
        // A_13P = ROUND(sm × A_8 / NZL). Lună întreagă ⇒ active_days = NZL ⇒ baza întreagă (neschimbat).
        let active_days = active_working_days(
            year,
            month,
            e.employment_date.as_deref(),
            e.contract_end_date.as_deref(),
        );
        // PAY-02: A_8 (zile lucrate) = zilele active din lună pe ORICE cale (nu doar part-time). La o
        // angajare/încetare la mijlocul lunii, full-time, A_8 trebuie să reflecte intervalul activ, nu
        // luna întreagă. Lună întreagă ⇒ active_days = NZL (neschimbat). Convenție: `gross_salary` e
        // brutul REALIZAT al lunii (nu se proratează automat aici — proratarea brutului e o decizie a
        // contabilului); pentru part-time, A_13P = ROUND(sm × A_8 / NZL) e recalculat de DUK din A_8.
        let zile_emis = active_days;
        if let Some((base, _, _)) = crate::anaf_decl::d112::part_time_min_base(
            gross,
            &e.tip_contract,
            exempt,
            year,
            month,
            active_days,
            nzl,
        ) {
            baza_cas = base;
            baza_cass = base;
            cas = round2(base * Decimal::new(25, 2));
            cass = round2(base * Decimal::new(10, 2));
        }
        // Tip asigurat: beneficiarii sumei netaxabile, de la 07/2026 (modelul Ordin 605/95/928/
        // 2.314/2026), folosesc codul 1.11.2 (Nomenclator 5). Pentru ≤06/2026 sau ne-beneficiari se
        // păstrează codul configurat pe angajat. (1.11.3 — sistem propriu de pensii — necesită
        // tratament CAS distinct, neimplementat; nu se emite automat.)
        let tip_asigurat =
            if non_taxable > Decimal::ZERO && (year > 2026 || (year == 2026 && month >= 7)) {
                "1.11.2".to_string()
            } else {
                e.tip_asigurat.clone()
            };
        let (nume, prenume) = split_name(&e.full_name);
        d112_emps.push(D112Employee {
            cnp: e.cnp.clone(),
            nume,
            prenume,
            data_ang: e
                .employment_date
                .as_deref()
                .map(ro_date)
                .unwrap_or_default(),
            gross: leid(gross),
            cas: leid(cas),
            cass: leid(cass),
            impozit: lei(&r.income_tax),
            cam: lei(&r.cam),
            zile: zile_emis,
            tip_asigurat,
            pensionar: e.pensionar,
            tip_contract: e.tip_contract.clone(),
            // A_4 ore normă zilnică must be 6/7/8 (the position's daily norm); part-time is captured
            // via tip_contract (Pi) + the reduced base, not by lowering A_4.
            ore_norma: e.ore_norma.clamp(6, 8) as u32,
            baza_cas: leid(baza_cas),
            baza_cass: leid(baza_cass),
            // A_5 baza CAM = brutul realizat (brut − sumă netaxabilă), NU baza CAS/CASS majorată
            // part-time — CAM nu se majorează la baza minimă (art. 220^6). Vezi PAY-01.
            baza_cam: leid(baza_cam_real),
            // A_sal1 salariul de bază brut din contract (brutul realizat al lunii).
            sal_contract: leid(gross_in),
            // E3_14/E1_6 baza impozit + E3_12/E1_4 deducerea personală (din rezultatul de salarizare).
            baza_impozit: lei(&r.taxable_base),
            deducere: lei(&r.personal_deduction),
            sediu_cif: e.sediu_cif.clone(),
            e3_23: 0, // set by Wave-E post-processing below if there is diurnă surplus
            // Concediile medicale (calea B / asiguratD) se populează mai jos din registrul concedii.
            med_leaves: vec![],
        });
    }

    // ── Wave E: fold payroll_extra_income (diurnă surplus) into D112 bases ────────────────────
    // P1 fix: contributions MUST be computed on the COMBINED base (salary + excess) with a SINGLE
    // rounding — not salary-rounded + excess-rounded separately. This matches run_payroll GL which
    // also uses single combined-base rounding, so GL 4315/4316/444/436 == D112 412/432/602/480.
    //
    // For each employee with open extra income:
    //   combined_base = gross (employee's salary base) + excess
    //   combined_cas  = pct(combined_base, 25%)   → replaces de.cas
    //   combined_cass = pct(combined_base, 10%)   → replaces de.cass
    //   combined_impozit = pct(combined_base − combined_cas − combined_cass − deduction, 10%)
    //   → replaces de.impozit (no additional deduction for asimilat venitor, art. 78)
    //   de.gross and baza_cas/baza_cass/baza_cam are widened by excess_lei.
    //   E3_23 = excess_lei (informational rând 8.2.1, art.76(2)(k)).
    //
    // NOTE: the GL for the excess is handled entirely by run_payroll (D641/C625=excess_reclass;
    // D4282/C421=excess_receivable). The former DIURNA_ASIMILAT separate journal is NO LONGER
    // called from approve_report — contributions were computed separately there and caused drift.
    if !extra_income.is_empty() {
        // Build a CNP → employee_id map so we can look up by CNP (D112Employee uses CNP as key).
        let emp_by_cnp: std::collections::HashMap<&str, &Employee> =
            employees.iter().map(|e| (e.cnp.as_str(), e)).collect();

        for de in d112_emps.iter_mut() {
            // Find the employee record for this D112 slot.
            let Some(emp) = emp_by_cnp.get(de.cnp.as_str()) else {
                continue;
            };
            let excess = match extra_income.get(emp.id.as_str()) {
                Some(&ex) if ex > Decimal::ZERO => ex,
                _ => continue,
            };
            let excess_lei = leid(excess);
            use crate::anaf_decl::d112::pct;
            // Combined base (salary + excess) — single rounding matches GL run_payroll.
            // de.gross (salary gross, lei) is the existing salary base before excess.
            let sal_gross_dec = Decimal::from(de.gross);
            let combined = sal_gross_dec + excess;
            let comb_cas = leid(pct(combined, (25, 2)));
            let comb_cass = leid(pct(combined, (10, 2)));
            // impozit base = combined − CAS − CASS − deduction (same deduction as salary path).
            let ded = de.deducere; // already in lei
            let comb_impozit_base = (combined
                - Decimal::from(comb_cas)
                - Decimal::from(comb_cass)
                - Decimal::from(ded))
            .max(Decimal::ZERO);
            let comb_impozit = leid(pct(comb_impozit_base, (10, 2)));

            // Update D112Employee fields: set combined contributions (not add excess-only).
            de.gross += excess_lei;
            de.baza_cas += excess_lei;
            de.baza_cass += excess_lei;
            // baza_cam: aggregate CAM = ROUND(Σ baza_cam × 2,25%) in generate_d112_xml.
            // Include the excess in the CAM base (CAM 2.25% applies per art. 220^6 + art.76(2)(k)).
            de.baza_cam += excess_lei;
            // baza_impozit = combined − CAS − CASS − deduction (matches comb_impozit_base).
            de.baza_impozit = leid(comb_impozit_base);
            // Set combined contributions (replaces salary-only values with combined-base values).
            de.cas = comb_cas;
            de.cass = comb_cass;
            de.impozit = comb_impozit;
            // NOTE: de.cam is NOT updated — generate_d112_xml uses baza_cam aggregate, not per-employee cam.
            de.e3_23 = excess_lei; // rând 8.2.1 informational (diurnă impozabilă asimilată)
        }
    }
    // ─────────────────────────────────────────────────────────────────────────────────────────────

    // ROB-01: a D112 with zero insured persons is a malformed declaration (B_sal=0, no asigurat) —
    // ANAF rejects it. Guard with a clear error instead of emitting an empty draft.
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
        d_rec: u8::from(is_rectificative), // 0 = inițială, 1 = rectificativă
        // Declarantul (persoană) se completează în aplicație; folosim denumirea ca substituent.
        nume_declar: company.legal_name.chars().take(75).collect(),
        prenume_declar: "-".into(),
        functie_declar: "Administrator".into(),
        cif: company.cui.clone(),
        caen: caen.to_string(),
        den: company.legal_name.chars().take(200).collect(),
        casa,
    };
    // Modelul NOU D112 (Ordin comun 605/95/928/2.314/2026, MO 463/02.06.2026) se aplică veniturilor
    // din 07/2026 (prima depunere 25.08.2026). Sursele oficiale arată schimbări la nivel de
    // nomenclator/instrucțiuni (sumă netaxabilă 300→200, relabel tip asigurat 1.11.2/1.11.3,
    // simplificare concedii) — NU câmpuri XML noi; namespace-ul rămâne :v7, deci structura emisă aici
    // este corectă STRUCTURAL și pentru H2. La data implementării ANAF nu publicase încă
    // structura/XSD/DUKIntegrator pentru noul model → RE-VALIDAȚI contra artefactelor oficiale înainte
    // de depunere. FE avertizează utilizatorul; logăm și aici.
    if year > 2026 || (year == 2026 && month >= 7) {
        tracing::warn!(
            year,
            month,
            "D112 ≥ 07/2026: model nou (Ordin 605/95/928/2.314/2026) — structura :v7 emisă e \
conformă structural; re-validați cu artefactele oficiale ANAF înainte de depunere (25.08.2026)"
        );
    }
    // Pretty-print so the exported/previewed .xml is a readable document (DUK-safe whitespace).
    Ok(crate::anaf_decl::xml::pretty_print(&generate_d112_xml(
        &header, &d112_emps,
    )))
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
        let employees = crate::db::payroll::list(&pool, "co1").await.unwrap();
        let leaves = crate::db::concedii::list(&pool, "co1", "2026-06")
            .await
            .unwrap();
        let xml = build_d112_xml(
            &company,
            &employees,
            &leaves,
            2026,
            6,
            "6201",
            false,
            &std::collections::HashMap::new(),
        )
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
        let employees = crate::db::payroll::list(&pool, "co1").await.unwrap();
        let leaves = crate::db::concedii::list(&pool, "co1", "2026-06")
            .await
            .unwrap();
        let xml = build_d112_xml(
            &company,
            &employees,
            &leaves,
            2026,
            6,
            "6201",
            false,
            &std::collections::HashMap::new(),
        )
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
        let employees = crate::db::payroll::list(&pool, "co1").await.unwrap();
        let leaves = crate::db::concedii::list(&pool, "co1", "2026-06")
            .await
            .unwrap();

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
        let xml = build_d112_xml(
            &company,
            &employees,
            &leaves,
            2026,
            6,
            "6201",
            false,
            &std::collections::HashMap::new(),
        )
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

        // Build D112 with the same extra_income map.
        let company = crate::db::companies::get(&pool, "co1").await.unwrap();
        let employees = crate::db::payroll::list(&pool, "co1").await.unwrap();
        let extra_income =
            crate::db::payroll_diurna::open_extra_income_by_employee(&pool, "co1", "2026-06")
                .await
                .unwrap();
        let xml = build_d112_xml(
            &company,
            &employees,
            &[],
            2026,
            6,
            "6201",
            false,
            &extra_income,
        )
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
        let employees = crate::db::payroll::list(&pool, "co1").await.unwrap();
        let leaves = crate::db::concedii::list(&pool, "co1", "2026-06")
            .await
            .unwrap();

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

        let xml = build_d112_xml(
            &company,
            &employees,
            &leaves,
            2026,
            6,
            "6201",
            false,
            &std::collections::HashMap::new(),
        )
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
        let employees = crate::db::payroll::list(&pool, "co1").await.unwrap();
        let xml = build_d112_xml(
            &company,
            &employees,
            &[],
            2026,
            6,
            "6201",
            false,
            &std::collections::HashMap::new(),
        )
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
        let employees = crate::db::payroll::list(&pool, "co1").await.unwrap();
        let leaves = crate::db::concedii::list(&pool, "co1", "2026-06")
            .await
            .unwrap();
        let xml = build_d112_xml(
            &company,
            &employees,
            &leaves,
            2026,
            6,
            "6201",
            false,
            &std::collections::HashMap::new(),
        )
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
        let employees = crate::db::payroll::list(&pool, "co1").await.unwrap();
        let leaves = crate::db::concedii::list(&pool, "co1", "2026-06")
            .await
            .unwrap();
        let xml = build_d112_xml(
            &company,
            &employees,
            &leaves,
            2026,
            6,
            "6201",
            false,
            &std::collections::HashMap::new(),
        )
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
}
