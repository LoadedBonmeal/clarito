//! Salarizare — angajați + statul de salarii lunar (nucleul D112).
//!
//! Un angajat are salariu brut + deducere personală. Rularea lunară calculează stările individuale
//! (via [`crate::anaf_decl::d112::compute_payroll`], ratele 2026) și postează nota contabilă
//! agregată în GL (via [`crate::db::gl::post_payroll`]).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

use crate::anaf_decl::d112::{
    cm_indemn_treatment, compute_payroll, compute_payroll_with_leave, exempt_part_time_min_base,
    part_time_min_base, suma_netaxabila, LeaveCert, LeavePayrollInput, PayrollInput,
};
use crate::db::gl::IndemnityTotals;
use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Employee {
    pub id: String,
    pub company_id: String,
    pub cnp: String,
    pub full_name: String,
    pub gross_salary: String,
    pub personal_deduction: String,
    pub employment_date: Option<String>,
    pub active: bool,
    /// D112 asiguratA fields: A_1 tip asigurat (Nomenclator 5, "1"=salariat), A_2 pensionar (0/1),
    /// A_3 tip contract (Nomenclator 12, "N"=normă întreagă, "P1".."P7"=parțial), A_4 ore normă (6/7/8).
    pub tip_asigurat: String,
    pub pensionar: bool,
    pub tip_contract: String,
    pub ore_norma: i64,
    /// art. 146 (5^7) excepție de la baza minimă CAS/CASS part-time: ''/'elev_student'/'ucenic'/
    /// 'dizabilitate'/'contracte_multiple' (pensionarii via `pensionar`).
    pub exceptie_cas_min: String,
    /// CIF-ul sediului secundar la care e repartizat salariatul (D112 angajatorF2); '' = sediu
    /// principal.
    pub sediu_cif: String,
    /// Beneficiar al sumei netaxabile din salariul minim (art. III OUG 89/2025): atestarea că
    /// salariatul e cu normă întreagă, salariul de bază = salariul minim și nu a fost diminuat în
    /// 2026. Activează carve-out-ul (300/200 lei) în [`crate::anaf_decl::d112::suma_netaxabila`].
    pub beneficiar_suma_netaxabila: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateEmployeeInput {
    pub company_id: String,
    pub cnp: String,
    pub full_name: String,
    pub gross_salary: String,
    #[serde(default)]
    pub personal_deduction: Option<String>,
    pub employment_date: Option<String>,
    #[serde(default)]
    pub tip_asigurat: Option<String>,
    #[serde(default)]
    pub pensionar: Option<bool>,
    #[serde(default)]
    pub tip_contract: Option<String>,
    #[serde(default)]
    pub ore_norma: Option<i64>,
    #[serde(default)]
    pub exceptie_cas_min: Option<String>,
    #[serde(default)]
    pub sediu_cif: Option<String>,
    #[serde(default)]
    pub beneficiar_suma_netaxabila: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEmployeeInput {
    pub cnp: Option<String>,
    pub full_name: Option<String>,
    pub gross_salary: Option<String>,
    pub personal_deduction: Option<String>,
    pub employment_date: Option<String>,
    pub active: Option<bool>,
    pub tip_asigurat: Option<String>,
    pub pensionar: Option<bool>,
    pub tip_contract: Option<String>,
    pub ore_norma: Option<i64>,
    pub exceptie_cas_min: Option<String>,
    pub sediu_cif: Option<String>,
    pub beneficiar_suma_netaxabila: Option<bool>,
}

const COLS: &str = "id, company_id, cnp, full_name, gross_salary, personal_deduction, \
                    employment_date, active, tip_asigurat, pensionar, tip_contract, ore_norma, \
                    exceptie_cas_min, sediu_cif, beneficiar_suma_netaxabila, created_at, updated_at";

pub async fn list(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<Employee>> {
    let q = format!(
        "SELECT {COLS} FROM employees WHERE company_id = ?1 ORDER BY active DESC, full_name"
    );
    Ok(sqlx::query_as::<_, Employee>(&q)
        .bind(company_id)
        .fetch_all(pool)
        .await?)
}

pub async fn get(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<Employee> {
    let q = format!("SELECT {COLS} FROM employees WHERE id = ?1 AND company_id = ?2");
    sqlx::query_as::<_, Employee>(&q)
        .bind(id)
        .bind(company_id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)
}

/// Parse a money amount (non-negative), returning its canonical Decimal string. Rejects garbage so a
/// salary never silently becomes 0 and corrupts the payroll totals / GL postings.
fn parse_money(label: &str, s: &str) -> AppResult<String> {
    let d = Decimal::from_str(s.trim()).map_err(|_| {
        AppError::Validation(format!("{label} invalid — folosiți formatul 1234.56."))
    })?;
    if d.is_sign_negative() {
        return Err(AppError::Validation(format!(
            "{label} nu poate fi negativ."
        )));
    }
    Ok(d.to_string())
}

pub async fn create(pool: &SqlitePool, input: CreateEmployeeInput) -> AppResult<Employee> {
    if input.full_name.trim().is_empty() {
        return Err(AppError::Validation(
            "Numele angajatului e obligatoriu.".into(),
        ));
    }
    let gross = parse_money("Salariul brut", &input.gross_salary)?;
    let ded = parse_money(
        "Deducerea personală",
        input.personal_deduction.as_deref().unwrap_or("0"),
    )?;
    let id = new_id();
    let now = now_unix();
    sqlx::query(
        "INSERT INTO employees (id, company_id, cnp, full_name, gross_salary, personal_deduction, \
         employment_date, active, tip_asigurat, pensionar, tip_contract, ore_norma, \
         exceptie_cas_min, sediu_cif, beneficiar_suma_netaxabila, created_at, updated_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,1,?8,?9,?10,?11,?12,?13,?14,?15,?15)",
    )
    .bind(&id)
    .bind(&input.company_id)
    .bind(input.cnp.trim())
    .bind(input.full_name.trim())
    .bind(&gross)
    .bind(&ded)
    .bind(&input.employment_date)
    .bind(input.tip_asigurat.as_deref().unwrap_or("1"))
    .bind(input.pensionar.unwrap_or(false))
    .bind(input.tip_contract.as_deref().unwrap_or("N"))
    .bind(input.ore_norma.unwrap_or(8))
    .bind(input.exceptie_cas_min.as_deref().unwrap_or(""))
    .bind(input.sediu_cif.as_deref().unwrap_or("").trim())
    .bind(input.beneficiar_suma_netaxabila.unwrap_or(false))
    .bind(now)
    .execute(pool)
    .await?;
    get(pool, &id, &input.company_id).await
}

pub async fn update(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    input: UpdateEmployeeInput,
) -> AppResult<Employee> {
    let cur = get(pool, id, company_id).await?;
    // Validate any supplied money fields (partial update) so garbage never silently becomes 0.
    let gross = match input.gross_salary.as_deref() {
        Some(s) => parse_money("Salariul brut", s)?,
        None => cur.gross_salary.clone(),
    };
    let ded = match input.personal_deduction.as_deref() {
        Some(s) => parse_money("Deducerea personală", s)?,
        None => cur.personal_deduction.clone(),
    };
    sqlx::query(
        "UPDATE employees SET cnp=?3, full_name=?4, gross_salary=?5, personal_deduction=?6, \
         employment_date=?7, active=?8, tip_asigurat=?9, pensionar=?10, tip_contract=?11, \
         ore_norma=?12, exceptie_cas_min=?13, sediu_cif=?14, beneficiar_suma_netaxabila=?15, \
         updated_at=?16 \
         WHERE id=?1 AND company_id=?2",
    )
    .bind(id)
    .bind(company_id)
    .bind(input.cnp.as_deref().unwrap_or(&cur.cnp))
    .bind(input.full_name.as_deref().unwrap_or(&cur.full_name))
    .bind(&gross)
    .bind(&ded)
    .bind(input.employment_date.or(cur.employment_date))
    .bind(input.active.unwrap_or(cur.active))
    .bind(input.tip_asigurat.as_deref().unwrap_or(&cur.tip_asigurat))
    .bind(input.pensionar.unwrap_or(cur.pensionar))
    .bind(input.tip_contract.as_deref().unwrap_or(&cur.tip_contract))
    .bind(input.ore_norma.unwrap_or(cur.ore_norma))
    .bind(
        input
            .exceptie_cas_min
            .as_deref()
            .unwrap_or(&cur.exceptie_cas_min),
    )
    .bind(input.sediu_cif.as_deref().unwrap_or(&cur.sediu_cif))
    .bind(
        input
            .beneficiar_suma_netaxabila
            .unwrap_or(cur.beneficiar_suma_netaxabila),
    )
    .bind(now_unix())
    .execute(pool)
    .await?;
    get(pool, id, company_id).await
}

pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    get(pool, id, company_id).await?;
    sqlx::query("DELETE FROM employees WHERE id=?1 AND company_id=?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    Ok(())
}

// ─── Sedii secundare (D112 angajatorF2) ──────────────────────────────────────

/// Un sediu secundar / punct de lucru — impozitul pe salarii al angajaților repartizați aici se
/// declară separat în D112 (angajatorF2), per CIF.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SecondaryOffice {
    pub id: String,
    pub company_id: String,
    pub cif: String,
    pub name: String,
    pub created_at: i64,
}

pub async fn list_sedii(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<SecondaryOffice>> {
    Ok(sqlx::query_as::<_, SecondaryOffice>(
        "SELECT id, company_id, cif, name, created_at FROM secondary_offices \
         WHERE company_id=?1 ORDER BY cif",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?)
}

pub async fn create_sediu(
    pool: &SqlitePool,
    company_id: &str,
    cif: &str,
    name: &str,
) -> AppResult<SecondaryOffice> {
    let cif = cif.trim();
    // CIF sediu secundar: obligatoriu + validat cu același algoritm mod-11 ca al companiei (valid_cui
    // întoarce true pentru gol, de aceea respingem explicit golul).
    if cif.is_empty() || !crate::anaf_decl::valid_cui(cif) {
        return Err(AppError::Validation(
            "CIF sediu secundar invalid — verificați cifra de control.".into(),
        ));
    }
    let id = new_id();
    sqlx::query(
        "INSERT INTO secondary_offices (id, company_id, cif, name, created_at) \
         VALUES (?1,?2,?3,?4,?5)",
    )
    .bind(&id)
    .bind(company_id)
    .bind(cif)
    .bind(name.trim())
    .bind(now_unix())
    .execute(pool)
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            AppError::Validation(format!("Există deja un sediu secundar cu CIF {cif}."))
        } else {
            AppError::from(e)
        }
    })?;
    list_sedii(pool, company_id)
        .await?
        .into_iter()
        .find(|s| s.id == id)
        .ok_or(AppError::NotFound)
}

pub async fn delete_sediu(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    sqlx::query("DELETE FROM secondary_offices WHERE id=?1 AND company_id=?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// One employee's computed salary state for the month.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmployeeState {
    pub employee_id: String,
    pub full_name: String,
    pub gross: String,
    pub cas: String,
    pub cass: String,
    pub income_tax: String,
    pub net: String,
    pub cam: String,
    /// CCI 0,85% (concedii și indemnizații, angajator).
    pub concedii: String,
}

/// The monthly payroll register + the GL post result.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PayrollRun {
    pub states: Vec<EmployeeState>,
    pub total_gross: String,
    pub total_cas: String,
    pub total_cass: String,
    pub total_income_tax: String,
    pub total_net: String,
    pub total_cam: String,
    /// Contribuția 0,85% pentru concedii și indemnizații (angajator, OUG 158/2005).
    pub total_concedii: String,
    pub posted: bool,
    pub entry_date: String,
}

/// Day of week (0=Sunday..6=Saturday) via Sakamoto's algorithm.
pub fn weekday(y: i32, m: u32, d: u32) -> u32 {
    let t = [0i32, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let yy = if m < 3 { y - 1 } else { y };
    (((yy + yy / 4 - yy / 100 + yy / 400 + t[(m - 1) as usize] + d as i32) % 7 + 7) % 7) as u32
}

pub fn days_in_month(y: i32, m: u32) -> u32 {
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

/// Romanian legal public holidays (zile libere legale) of the year, as `(month, day)`. ANAF's D112
/// validator excludes these from the working-day count (NZL) — e.g. June 2026 = 21 working days, not 22
/// (1 iunie = Ziua Copilului + a doua zi de Rusalii). Hardcoded per year (the movable Orthodox-Easter
/// dates — Vinerea Mare / Paște / Rusalii — aren't trivially derivable) with a drift guard, like
/// `min_wage_lei`. UPDATE the table each new year.
fn ro_holidays(year: i32) -> &'static [(u32, u32)] {
    match year {
        2026 => &[
            (1, 1),
            (1, 2),
            (1, 6),
            (1, 7),
            (1, 24), // Anul Nou, Boboteaza, Sf. Ion, Unirea Principatelor
            (4, 10),
            (4, 12),
            (4, 13), // Vinerea Mare, Paște (ortodox 12.04.2026), a doua zi de Paște
            (5, 1),
            (5, 31),
            (6, 1), // Ziua Muncii, Rusalii (a doua zi 1.06 = și Ziua Copilului)
            (8, 15),
            (11, 30),
            (12, 1),
            (12, 25),
            (12, 26), // Adormirea, Sf. Andrei, Ziua Națională, Crăciun
        ],
        _ => {
            tracing::warn!(
                year,
                "ro_holidays: an neacoperit — NZL ignoră sărbătorile legale (poate fi supraevaluat); \
actualizați tabelul cu zilele libere ale anului"
            );
            &[]
        }
    }
}

/// True if `(year, month, day)` is a working day: Mon-Fri AND not a Romanian legal holiday.
pub fn is_working_day(year: i32, month: u32, day: u32) -> bool {
    let w = weekday(year, month, day);
    w != 0 && w != 6 && !ro_holidays(year).contains(&(month, day))
}

/// Working days in a month — the D112 NZL (Mon-Fri minus legal holidays) used for proration.
pub fn working_days(year: i32, month: u32) -> u32 {
    (1..=days_in_month(year, month))
        .filter(|&d| is_working_day(year, month, d))
        .count() as u32
}

/// Working days (Mon-Fri) of a medical-leave certificate `[start, end]` that fall WITHIN the given
/// month. For SALARY proration we count ALL leave working days (the leave suspends the contract, so no
/// salary), INCLUDING the 2026 first unpaid day (OUG 91/2025: it still counts as medical leave) — not
/// just the indemnity-paid days `D_14 + D_15`. Dates are ISO `YYYY-MM-DD`; a span crossing the month is
/// clamped to the month.
pub fn leave_working_days_in_month(year: i32, month: u32, start_iso: &str, end_iso: &str) -> u32 {
    let dim = days_in_month(year, month);
    // In-month day for an ISO date: before the month → `before`, after → `after`, in-month → the day.
    let day_in_month = |iso: &str, before: u32, after: u32| -> u32 {
        let p: Vec<&str> = iso.split('-').collect();
        if p.len() != 3 {
            return after;
        }
        let (y, m, d) = (
            p[0].parse::<i32>().unwrap_or(year),
            p[1].parse::<u32>().unwrap_or(month),
            p[2].parse::<u32>().unwrap_or(0),
        );
        if y < year || (y == year && m < month) {
            before
        } else if y > year || (y == year && m > month) {
            after
        } else {
            d.clamp(1, dim)
        }
    };
    let s = day_in_month(start_iso, 1, dim + 1); // started after month ⇒ no in-month days
    let e = day_in_month(end_iso, 0, dim); // ended before month ⇒ no in-month days
    if s > e {
        return 0;
    }
    (s..=e).filter(|&d| is_working_day(year, month, d)).count() as u32
}

/// Compute the monthly salary states for all active employees and post the aggregate to the GL.
pub async fn run_payroll(
    pool: &SqlitePool,
    company_id: &str,
    period_from: &str,
    period_to: &str,
) -> AppResult<PayrollRun> {
    let dec = |s: &str| Decimal::from_str(s).unwrap_or(Decimal::ZERO);
    let month: u32 = period_from
        .get(5..7)
        .and_then(|m| m.parse().ok())
        .unwrap_or(1);
    let year: i32 = period_from
        .get(0..4)
        .and_then(|y| y.parse().ok())
        .unwrap_or(2026);
    let employees = list(pool, company_id).await?;
    // Concedii medicale ale lunii → proratare salariu + indemnizație. Grupate pe angajat.
    let period_ym = format!("{year:04}-{month:02}");
    let leaves = crate::db::concedii::list(pool, company_id, &period_ym).await?;
    let mut leaves_by_emp: std::collections::HashMap<
        String,
        Vec<crate::db::concedii::MedicalLeave>,
    > = std::collections::HashMap::new();
    for l in leaves {
        leaves_by_emp
            .entry(l.employee_id.clone())
            .or_default()
            .push(l);
    }
    let nzl = working_days(year, month);
    let leid0 = |d: Decimal| {
        d.round_dp_with_strategy(0, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
    };
    let f = |d: Decimal| {
        format!(
            "{:.2}",
            d.round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
        )
    };

    let mut states = Vec::new();
    // Worked-salary aggregates (intră în nota GL: 641/421, 4315/4316/444 salariale, 646/436 CAM).
    let (mut t_gross, mut t_cas, mut t_cass, mut t_tax, mut t_net, mut t_cam) = (
        Decimal::ZERO,
        Decimal::ZERO,
        Decimal::ZERO,
        Decimal::ZERO,
        Decimal::ZERO,
        Decimal::ZERO,
    );
    // Employer-borne part-time minimum-base CAS/CASS difference (art. 146 (5^6)).
    let (mut t_cas_diff, mut t_cass_diff) = (Decimal::ZERO, Decimal::ZERO);
    // CCI 0,85% (concedii și indemnizații, angajator) — postată separat (6458/4373).
    let mut t_concedii = Decimal::ZERO;
    // Indemnizații de concediu medical (postate separat: 6458/4382/423).
    let mut indemn = IndemnityTotals::default();

    for e in employees.iter().filter(|e| e.active) {
        let gross = dec(&e.gross_salary);
        let non_taxable = suma_netaxabila(
            e.beneficiar_suma_netaxabila,
            &e.tip_contract,
            gross,
            year,
            month,
        );

        // Salariatul cu concediu medical: salariul se proratează la zilele lucrate; indemnizația intră
        // în baza CAS/CASS și de impozit. GL: partea salarială (lucrată) separat de indemnizație.
        if let Some(emp_leaves) = leaves_by_emp.get(&e.id) {
            let certs: Vec<LeaveCert> = emp_leaves
                .iter()
                .map(|l| {
                    let (cass_due, taxable) = cm_indemn_treatment(&l.cod_indemnizatie);
                    LeaveCert {
                        indemn_employer: leid0(dec(&l.suma_angajator)),
                        indemn_fnuass: leid0(dec(&l.suma_fnuass)),
                        leave_working_days: leave_working_days_in_month(
                            year,
                            month,
                            &l.data_inceput,
                            &l.data_sfarsit,
                        ),
                        cass_due,
                        taxable,
                    }
                })
                .collect();
            let lr = compute_payroll_with_leave(&LeavePayrollInput {
                gross,
                personal_deduction: dec(&e.personal_deduction),
                non_taxable,
                working_days: nzl,
                certs,
            });
            // Partea salarială lucrată (pentru split-ul GL): contribuțiile pe brutul lucrat.
            let w = compute_payroll(&PayrollInput {
                gross: lr.worked_gross,
                personal_deduction: dec(&e.personal_deduction),
                non_taxable,
            });
            let (wcas, wcass, wtax) = (dec(&w.cas), dec(&w.cass), dec(&w.income_tax));
            t_gross += lr.worked_gross;
            t_cas += wcas;
            t_cass += wcass;
            t_tax += wtax;
            t_cam += lr.cam;
            t_concedii += lr.concedii;
            t_net += lr.net;
            // Indemnizația = combinat − lucrat (sumează exact la combinat ⇒ creditele = D112).
            indemn.employer += lr.indemn_employer;
            indemn.fnuass += lr.indemn_fnuass;
            indemn.cas += lr.cas - wcas;
            indemn.cass += lr.cass - wcass;
            indemn.tax += lr.income_tax - wtax;
            states.push(EmployeeState {
                employee_id: e.id.clone(),
                full_name: e.full_name.clone(),
                gross: f(lr.worked_gross + lr.indemn_total), // total venit (lucrat + indemnizație)
                cas: f(lr.cas),
                cass: f(lr.cass),
                income_tax: f(lr.income_tax),
                net: f(lr.net),
                cam: f(lr.cam),
                concedii: f(lr.concedii),
            });
            continue;
        }

        let r = compute_payroll(&PayrollInput {
            gross,
            personal_deduction: dec(&e.personal_deduction),
            non_taxable,
        });
        let exempt = exempt_part_time_min_base(e.pensionar, &e.exceptie_cas_min);
        if let Some((_, cas_diff, cass_diff)) =
            part_time_min_base(gross, &e.tip_contract, exempt, year, month)
        {
            t_cas_diff += cas_diff;
            t_cass_diff += cass_diff;
        }
        t_gross += dec(&r.gross);
        t_cas += dec(&r.cas);
        t_cass += dec(&r.cass);
        t_tax += dec(&r.income_tax);
        t_net += dec(&r.net);
        t_cam += dec(&r.cam);
        t_concedii += dec(&r.concedii);
        states.push(EmployeeState {
            employee_id: e.id.clone(),
            full_name: e.full_name.clone(),
            gross: r.gross,
            cas: r.cas,
            cass: r.cass,
            income_tax: r.income_tax,
            net: r.net,
            cam: r.cam,
            concedii: r.concedii,
        });
    }

    let post = crate::db::gl::post_payroll(
        pool,
        company_id,
        period_from,
        period_to,
        t_gross,
        t_cas,
        t_cass,
        t_tax,
        t_cam,
        t_concedii,
        t_cas_diff,
        t_cass_diff,
        indemn.clone(),
    )
    .await?;

    Ok(PayrollRun {
        states,
        // Totaluri de afișare = lucrat + indemnizație (Σ states).
        total_gross: f(t_gross + indemn.employer + indemn.fnuass),
        total_cas: f(t_cas + indemn.cas),
        total_cass: f(t_cass + indemn.cass),
        total_income_tax: f(t_tax + indemn.tax),
        total_net: f(t_net),
        total_cam: f(t_cam),
        total_concedii: f(t_concedii),
        posted: post.posted,
        entry_date: post.entry_date,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    async fn setup() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co1','12345678','Test SRL','Str 1','Cluj','CJ','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn sedii_crud_and_cif_checksum() {
        let pool = setup().await;
        // A structurally-valid RO CUI (mod-11) is accepted; a wrong-checksum one is rejected.
        let ok = create_sediu(&pool, "co1", "12345674", "Punct lucru").await;
        assert!(ok.is_ok(), "valid CIF should be accepted: {ok:?}");
        assert!(create_sediu(&pool, "co1", "12345678", "x").await.is_err()); // bad checksum
        assert!(create_sediu(&pool, "co1", "abc", "x").await.is_err()); // non-numeric
        assert!(create_sediu(&pool, "co1", "", "x").await.is_err()); // empty
                                                                     // Duplicate CIF rejected.
        assert!(create_sediu(&pool, "co1", "12345674", "dup").await.is_err());
        let list = list_sedii(&pool, "co1").await.unwrap();
        assert_eq!(list.len(), 1);
        delete_sediu(&pool, &list[0].id, "co1").await.unwrap();
        assert!(list_sedii(&pool, "co1").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn run_payroll_aggregates_and_posts_gl() {
        let pool = setup().await;
        for (cnp, name) in [("1", "A"), ("2", "B")] {
            create(
                &pool,
                CreateEmployeeInput {
                    company_id: "co1".into(),
                    cnp: cnp.into(),
                    full_name: name.into(),
                    gross_salary: "5000".into(),
                    personal_deduction: Some("0".into()),
                    employment_date: None,
                    tip_asigurat: None,
                    pensionar: None,
                    tip_contract: None,
                    ore_norma: None,
                    exceptie_cas_min: None,
                    sediu_cif: None,
                    beneficiar_suma_netaxabila: None,
                },
            )
            .await
            .unwrap();
        }
        let run = run_payroll(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        // 2 × (gross 5000, CAS 1250, CASS 500, impozit 325, net 2925, CAM 113).
        assert_eq!(run.total_gross, "10000.00");
        assert_eq!(run.total_cas, "2500.00");
        assert_eq!(run.total_cass, "1000.00");
        assert_eq!(run.total_income_tax, "650.00");
        assert_eq!(run.total_net, "5850.00");
        assert_eq!(run.total_cam, "226.00");
        assert_eq!(run.total_concedii, "86.00"); // 2 × pct(5000, 0.85%) = 2 × 43
        assert_eq!(run.states.len(), 2);
        assert!(run.posted);

        // GL: 641 debit 10.000 (cheltuieli), 421 credit = net 5.850, 4315 credit 2.500, 646 = CAM.
        let tb = crate::db::gl::trial_balance(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        let bal = |code: &str| {
            tb.rows
                .iter()
                .find(|r| r.account_code == code)
                .map(|r| (r.closing_debit.clone(), r.closing_credit.clone()))
        };
        assert_eq!(bal("641"), Some(("10000.00".into(), "0.00".into())));
        assert_eq!(bal("421"), Some(("0.00".into(), "5850.00".into())));
        assert_eq!(bal("4315"), Some(("0.00".into(), "2500.00".into())));
        assert_eq!(bal("646"), Some(("226.00".into(), "0.00".into())));
        // CCI 0,85%: D 6458 / C 4373 = 86 (fără concediu, 6458 = doar CCI).
        assert_eq!(bal("6458"), Some(("86.00".into(), "0.00".into())));
        assert_eq!(bal("4373"), Some(("0.00".into(), "86.00".into())));
        assert!(tb.balanced, "payroll journal balances");
    }

    #[tokio::test]
    async fn run_payroll_with_medical_leave_splits_salary_and_indemnity_in_gl() {
        let pool = setup().await;
        let emp = create(
            &pool,
            CreateEmployeeInput {
                company_id: "co1".into(),
                cnp: "1".into(),
                full_name: "Pop Ion".into(),
                gross_salary: "5500".into(),
                personal_deduction: Some("0".into()),
                employment_date: None,
                tip_asigurat: None,
                pensionar: None,
                tip_contract: None,
                ore_norma: None,
                exceptie_cas_min: None,
                sediu_cif: None,
                beneficiar_suma_netaxabila: None,
            },
        )
        .await
        .unwrap();
        // Certificat boală obișnuită (cod 01), 8-12 iunie 2026 = 5 zile lucrătoare; indemnizație 600.
        crate::db::concedii::create(
            &pool,
            crate::db::concedii::MedicalLeaveInput {
                company_id: "co1".into(),
                employee_id: emp.id.clone(),
                period_ym: "2026-06".into(),
                serie: Some("AB".into()),
                numar: Some("123".into()),
                cod_indemnizatie: Some("01".into()),
                data_acordare: Some("2026-06-08".into()),
                data_inceput: Some("2026-06-08".into()),
                data_sfarsit: Some("2026-06-12".into()),
                zile_angajator: Some(4),
                zile_fnuass: Some(0),
                baza_calcul: Some("24000".into()),
                zile_baza: Some(130),
                suma_angajator: Some("600".into()),
                suma_fnuass: Some("0".into()),
                procent: Some(75),
                loc_prescriere: Some(1),
                cod_boala: Some("A09".into()),
            },
        )
        .await
        .unwrap();

        let run = run_payroll(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        // Iunie 2026 = 21 zile lucrătoare (1 iunie e sărbătoare legală); 5 zile concediu ⇒ 16 lucrate;
        // brut lucrat 5500×16/21 = 4190. Indemnizație 600. CAS 25%×4790 = 1198, CASS 10%×4790 = 479,
        // impozit 10%×3113 = 311, CAM 2,25%×4190 = 94, net total 2802.
        assert_eq!(run.total_gross, "4790.00"); // lucrat 4190 + indemnizație 600
        assert_eq!(run.total_cas, "1198.00");
        assert_eq!(run.total_cass, "479.00");
        assert_eq!(run.total_income_tax, "311.00");
        assert_eq!(run.total_cam, "94.00");
        assert_eq!(run.total_concedii, "36.00"); // CCI 0,85% × 4190 = 35.615 → 36
        assert_eq!(run.total_net, "2802.00");

        let tb = crate::db::gl::trial_balance(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        let bal = |code: &str| {
            tb.rows
                .iter()
                .find(|r| r.account_code == code)
                .map(|r| (r.closing_debit.clone(), r.closing_credit.clone()))
        };
        // Salariul lucrat (641) e separat de indemnizație (6458); 421 = salariu net, 423 = indemniz. netă.
        assert_eq!(bal("641"), Some(("4190.00".into(), "0.00".into())));
        // 6458 = 600 indemnizație angajator + 36 CCI 0,85%.
        assert_eq!(bal("6458"), Some(("636.00".into(), "0.00".into())));
        assert_eq!(bal("4373"), Some(("0.00".into(), "36.00".into()))); // CCI 0,85% datorată
        assert_eq!(bal("421"), Some(("0.00".into(), "2451.00".into()))); // 4190 − 1048 − 419 − 272
        assert_eq!(bal("423"), Some(("0.00".into(), "351.00".into()))); // 600 − 150 − 60 − 39
                                                                        // Creditele de contribuții = cele combinate (lucrat + indemnizație) = obligațiile D112.
        assert_eq!(bal("4315"), Some(("0.00".into(), "1198.00".into())));
        assert_eq!(bal("4316"), Some(("0.00".into(), "479.00".into())));
        assert_eq!(bal("444"), Some(("0.00".into(), "311.00".into())));
        assert_eq!(bal("646"), Some(("94.00".into(), "0.00".into())));
        assert!(tb.balanced, "payroll + indemnity + CCI journal balances");
    }

    #[test]
    fn working_days_excludes_legal_holidays() {
        // Iunie 2026: 22 zile L-V, dar 1 iunie (luni) e sărbătoare legală ⇒ 21 NZL (ca la validatorul
        // ANAF — regula S21.1). Fără excluderea sărbătorii ar fi 22 (bug prins de testul e2e).
        assert_eq!(working_days(2026, 6), 21);
        // Ianuarie 2026: 1 ian = joi; 22 zile L-V minus 4 sărbători în zile lucrătoare (1,2,6,7) = 18.
        assert_eq!(working_days(2026, 1), 18);
    }
}
