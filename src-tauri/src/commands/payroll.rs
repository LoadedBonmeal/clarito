//! Tauri commands — salarizare (angajați + stat de salarii lunar).

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::str::FromStr;
use tauri::State;

use crate::anaf_decl::d112::{compute_payroll, suma_netaxabila, PayrollInput};
use crate::anaf_decl::d112_xml::{generate_d112_xml, D112Employee, D112Header};
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

/// Day of week (0=Sunday..6=Saturday) via Sakamoto's algorithm.
fn weekday(y: i32, m: u32, d: u32) -> u32 {
    let t = [0i32, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let yy = if m < 3 { y - 1 } else { y };
    (((yy + yy / 4 - yy / 100 + yy / 400 + t[(m - 1) as usize] + d as i32) % 7 + 7) % 7) as u32
}

fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

/// Working days (Mon-Fri) in a month — the D112 NZL used for part-time base proration.
fn working_days(year: i32, month: u32) -> u32 {
    (1..=days_in_month(year, month))
        .filter(|&d| {
            let w = weekday(year, month, d);
            w != 0 && w != 6
        })
        .count() as u32
}

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

/// Exportă D112 (XML, namespace …declaratie:v6 — vezi d112_xml.rs) pentru luna dată — antet +
/// obligații angajator + câte un asigurat per salariat activ (caz standard normă întreagă). Draft
/// pentru import în aplicația D112.
#[tauri::command]
pub async fn export_d112_xml(
    state: State<'_, AppState>,
    company_id: String,
    year: i32,
    month: u32,
    caen: String,
    dest_path: String,
) -> AppResult<String> {
    let caen = caen.trim().to_string();
    if caen.len() != 4 || !caen.chars().all(|c| c.is_ascii_digit()) {
        return Err(AppError::Validation(
            "Cod CAEN invalid — introduceți 4 cifre (ex. 6201).".into(),
        ));
    }
    let company = crate::db::companies::get(&state.db, &company_id).await?;
    let employees = payroll::list(&state.db, &company_id).await?;
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
        let r = compute_payroll(&PayrollInput {
            gross: gross_in,
            personal_deduction: dec(&e.personal_deduction),
            non_taxable,
        });
        let gross = dec(&r.gross);
        // Baza CAS/CASS = brut − suma netaxabilă (carve-out). Pentru ne-beneficiari nt=0 → baza = brut.
        let mut baza_cas = gross - non_taxable;
        let mut baza_cass = gross - non_taxable;
        let mut cas = dec(&r.cas);
        let mut cass = dec(&r.cass);
        // Part-time (contract Pi): baza CAS/CASS = salariul minim întreg (NU prorata cu norma orară),
        // categoriile art. 146 (5^7) exceptate — via the shared helper. Contribuția declarată e pe
        // baza majorată.
        let exempt =
            crate::anaf_decl::d112::exempt_part_time_min_base(e.pensionar, &e.exceptie_cas_min);
        if let Some((base, _, _)) =
            crate::anaf_decl::d112::part_time_min_base(gross, &e.tip_contract, exempt, year, month)
        {
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
            zile: nzl,
            tip_asigurat,
            pensionar: e.pensionar,
            tip_contract: e.tip_contract.clone(),
            // A_4 ore normă zilnică must be 6/7/8 (the position's daily norm); part-time is captured
            // via tip_contract (Pi) + the reduced base, not by lowering A_4.
            ore_norma: e.ore_norma.clamp(6, 8) as u32,
            baza_cas: leid(baza_cas),
            baza_cass: leid(baza_cass),
            // A_5 baza CAM = baza CAS/CASS pentru salariatul normal (brut − sumă netaxabilă).
            baza_cam: leid(baza_cas),
            // A_sal1 salariul de bază brut din contract (brutul realizat al lunii).
            sal_contract: leid(gross_in),
            sediu_cif: e.sediu_cif.clone(),
        });
    }

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
        d_rec: 0, // declarație inițială
        // Declarantul (persoană) se completează în aplicație; folosim denumirea ca substituent.
        nume_declar: company.legal_name.chars().take(75).collect(),
        prenume_declar: "-".into(),
        functie_declar: "Administrator".into(),
        cif: company.cui.clone(),
        caen,
        den: company.legal_name.chars().take(200).collect(),
        casa,
    };
    // Modelul NOU D112 (Ordin comun 605/95/928/2.314/2026, MO 463/02.06.2026) se aplică veniturilor
    // din 07/2026 (prima depunere 25.08.2026). Sursele oficiale arată schimbări la nivel de
    // nomenclator/instrucțiuni (sumă netaxabilă 300→200, relabel tip asigurat 1.11.2/1.11.3,
    // simplificare concedii) — NU câmpuri XML noi; namespace-ul rămâne :v6, deci structura emisă aici
    // este corectă STRUCTURAL și pentru H2. La data implementării ANAF nu publicase încă
    // structura/XSD/DUKIntegrator pentru noul model → RE-VALIDAȚI contra artefactelor oficiale înainte
    // de depunere. FE avertizează utilizatorul; logăm și aici.
    if year > 2026 || (year == 2026 && month >= 7) {
        tracing::warn!(
            year,
            month,
            "D112 ≥ 07/2026: model nou (Ordin 605/95/928/2.314/2026) — structura :v6 emisă e \
conformă structural; re-validați cu artefactele oficiale ANAF înainte de depunere (25.08.2026)"
        );
    }
    let xml = generate_d112_xml(&header, &d112_emps);
    // Validate the caller-supplied destination (absolute, no '..', no UNC, whitelist ext) — the
    // IPC endpoint accepts an arbitrary string.
    let dest = crate::commands::integrations::validate_export_path(&dest_path)?;
    std::fs::write(&dest, xml).map_err(|e| AppError::Other(e.to_string()))?;
    Ok(dest_path)
}
