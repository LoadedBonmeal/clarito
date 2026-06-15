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
use crate::db::payroll::{leave_working_days_in_month, working_days};

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
/// Registrul-jurnal (`run_payroll`). Draft pentru import în aplicația D112.
#[tauri::command]
pub async fn export_d112_xml(
    state: State<'_, AppState>,
    company_id: String,
    year: i32,
    month: u32,
    caen: String,
    dest_path: String,
) -> AppResult<String> {
    let company = crate::db::companies::get(&state.db, &company_id).await?;
    let employees = payroll::list(&state.db, &company_id).await?;
    let period_ym = format!("{year:04}-{month:02}");
    let leaves = crate::db::concedii::list(&state.db, &company_id, &period_ym).await?;
    let xml = build_d112_xml(&company, &employees, &leaves, year, month, caen.trim())?;
    // Validate the caller-supplied destination (absolute, no '..', no UNC, whitelist ext) — the IPC
    // endpoint accepts an arbitrary string.
    let dest = crate::commands::integrations::validate_export_path(&dest_path)?;
    std::fs::write(&dest, xml).map_err(|e| AppError::Other(e.to_string()))?;
    Ok(dest_path)
}

/// Pure core of the D112 XML build (no Tauri State / filesystem) — maps active employees + the month's
/// medical-leave certificates to the validated `:v7` XML. Separated from [`export_d112_xml`] so it is
/// testable end-to-end without IPC/IO. An employee with ≥1 certificate emits the B-path (salary
/// proration + indemnity, consistent with `run_payroll`/GL); the rest emit the standard `asiguratA` path.
fn build_d112_xml(
    company: &crate::db::companies::Company,
    employees: &[Employee],
    leaves: &[crate::db::concedii::MedicalLeave],
    year: i32,
    month: u32,
    caen: &str,
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
            // E3_14/E1_6 baza impozit + E3_12/E1_4 deducerea personală (din rezultatul de salarizare).
            baza_impozit: lei(&r.taxable_base),
            deducere: lei(&r.personal_deduction),
            sediu_cif: e.sediu_cif.clone(),
            // Concediile medicale (calea B / asiguratD) se populează mai jos din registrul concedii.
            med_leaves: vec![],
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
    Ok(generate_d112_xml(&header, &d112_emps))
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
        let xml = build_d112_xml(&company, &employees, &leaves, 2026, 6, "6201").unwrap();

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
        let xml = build_d112_xml(&company, &employees, &leaves, 2026, 6, "6201").unwrap();

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
        let xml = build_d112_xml(&company, &employees, &leaves, 2026, 6, "6201").unwrap();
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
        let xml = build_d112_xml(&company, &employees, &leaves, 2026, 6, "6201").unwrap();
        let path = std::env::temp_dir().join("d112_from_db.xml");
        std::fs::write(&path, &xml).unwrap();
        eprintln!("WROTE {}", path.display());
    }
}
