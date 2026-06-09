//! Tauri commands — salarizare (angajați + stat de salarii lunar).

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::str::FromStr;
use tauri::State;

use crate::anaf_decl::d112::{compute_payroll, PayrollInput};
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

/// Exportă D112 (XML, schema DecUnica.xsd) pentru luna dată — antet + obligații angajator + câte un
/// asigurat per salariat activ (caz standard normă întreagă). Draft pentru import în aplicația D112.
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
        let r = compute_payroll(&PayrollInput {
            gross: dec(&e.gross_salary),
            personal_deduction: dec(&e.personal_deduction),
        });
        let gross = dec(&r.gross);
        let mut baza_cas = gross;
        let mut baza_cass = gross;
        let mut cas = dec(&r.cas);
        let mut cass = dec(&r.cass);
        // Part-time (contract Pi): baza CAS/CASS = salariul minim întreg (NU prorata cu norma orară),
        // categoriile art. 146 (5^7) exceptate — via the shared helper. Contribuția declarată e pe
        // baza majorată.
        let exempt =
            crate::anaf_decl::d112::exempt_part_time_min_base(e.pensionar, &e.exceptie_cas_min);
        if let Some((base, _, _)) =
            crate::anaf_decl::d112::part_time_min_base(gross, &e.tip_contract, exempt, month)
        {
            baza_cas = base;
            baza_cass = base;
            cas = round2(base * Decimal::new(25, 2));
            cass = round2(base * Decimal::new(10, 2));
        }
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
            tip_asigurat: e.tip_asigurat.clone(),
            pensionar: e.pensionar,
            tip_contract: e.tip_contract.clone(),
            // A_4 ore normă zilnică must be 6/7/8 (the position's daily norm); part-time is captured
            // via tip_contract (Pi) + the reduced base, not by lowering A_4.
            ore_norma: e.ore_norma.clamp(6, 8) as u32,
            baza_cas: leid(baza_cas),
            baza_cass: leid(baza_cass),
            sediu_cif: e.sediu_cif.clone(),
        });
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
        // Declarantul (persoană) se completează în aplicație; folosim denumirea ca substituent.
        nume_declar: company.legal_name.chars().take(75).collect(),
        prenume_declar: "-".into(),
        functie_declar: "Administrator".into(),
        cif: company.cui.clone(),
        caen,
        den: company.legal_name.chars().take(200).collect(),
        casa,
    };
    let xml = generate_d112_xml(&header, &d112_emps);
    std::fs::write(&dest_path, xml).map_err(|e| AppError::Other(e.to_string()))?;
    Ok(dest_path)
}
