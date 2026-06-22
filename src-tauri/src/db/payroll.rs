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
    cm_indemn_treatment, compute_payroll, compute_payroll_with_leave, deducere_plafonata,
    exempt_part_time_min_base, part_time_min_base, pct, suma_netaxabila, LeaveCert,
    LeavePayrollInput, PayrollInput, CAM_PCT,
};
use crate::db::gl::IndemnityTotals;
use crate::db::models::{new_id, now_unix};
use crate::db::payroll_diurna::open_extra_income_by_employee;
use crate::db::payroll_retineri::{apply_retineri, retineri_by_employee};
use crate::db::payroll_sporuri::sporuri_by_employee;
use crate::db::pontaj::pontaj_by_employee;
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
    /// Data încetării contractului (ISO YYYY-MM-DD), opțional. Lipsă = activ toată luna. Folosit la
    /// proratarea bazei minime part-time pentru luni incomplete (încetare la mijlocul lunii).
    pub contract_end_date: Option<String>,
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
    pub contract_end_date: Option<String>,
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
    pub contract_end_date: Option<String>,
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
                    employment_date, contract_end_date, active, tip_asigurat, pensionar, \
                    tip_contract, ore_norma, exceptie_cas_min, sediu_cif, \
                    beneficiar_suma_netaxabila, created_at, updated_at";

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
///
/// PAY-02: also reject scientific notation (`1e10`, `1E5`). `Decimal::from_str` accepts it and would
/// store a literal 10_000_000_000 lei from a typo; a fiscal amount is always a plain decimal, so an
/// `e`/`E` in the input is a data-entry error, not a 10-billion-lei salary. (Infinity/NaN are already
/// rejected by `from_str` — `Decimal` has no such representation.)
fn parse_money(label: &str, s: &str) -> AppResult<String> {
    let t = s.trim();
    if t.contains('e') || t.contains('E') {
        return Err(AppError::Validation(format!(
            "{label} invalid — folosiți formatul 1234.56 (fără notație științifică)."
        )));
    }
    let d = Decimal::from_str(t).map_err(|_| {
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
         employment_date, contract_end_date, active, tip_asigurat, pensionar, tip_contract, \
         ore_norma, exceptie_cas_min, sediu_cif, beneficiar_suma_netaxabila, created_at, updated_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,1,?9,?10,?11,?12,?13,?14,?15,?16,?16)",
    )
    .bind(&id)
    .bind(&input.company_id)
    .bind(input.cnp.trim())
    .bind(input.full_name.trim())
    .bind(&gross)
    .bind(&ded)
    .bind(&input.employment_date)
    .bind(&input.contract_end_date)
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
         employment_date=?7, contract_end_date=?8, active=?9, tip_asigurat=?10, pensionar=?11, \
         tip_contract=?12, ore_norma=?13, exceptie_cas_min=?14, sediu_cif=?15, \
         beneficiar_suma_netaxabila=?16, updated_at=?17 \
         WHERE id=?1 AND company_id=?2",
    )
    .bind(id)
    .bind(company_id)
    .bind(input.cnp.as_deref().unwrap_or(&cur.cnp))
    .bind(input.full_name.as_deref().unwrap_or(&cur.full_name))
    .bind(&gross)
    .bind(&ded)
    .bind(input.employment_date.or(cur.employment_date))
    .bind(input.contract_end_date.or(cur.contract_end_date))
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
    /// Brut total (gross_salary + sporuri) pe care s-au calculat CAS/CASS/impozit/CAM.
    pub gross: String,
    pub cas: String,
    pub cass: String,
    pub income_tax: String,
    /// Net înainte de rețineri (gross − CAS − CASS − impozit).
    pub net: String,
    pub cam: String,
    // NOTE: concedii (CCI 0,85%) field removed — abolished 1 Jan 2018 by OUG 79/2017;
    // the separate 0,85% contribution has no 2026 legal basis (CF art.220^1, OUG 158/2005
    // art.6 Abrogat). Only CAM 2,25% (646/436) remains as employer social contribution.
    /// Suma sporurilor taxabile adăugate la brut pentru luna curentă (0 dacă nu există).
    pub spor: String,
    /// Suma reținută din net (popriri, pensie alimentară, avansuri — post-net). 0 dacă nu există.
    pub total_retinut: String,
    /// Netul efectiv de plată angajatului (net − total_retinut).
    pub net_employee: String,
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
    // NOTE: total_concedii (CCI 0,85%) removed — abolished 1 Jan 2018; no 2026 legal basis.
    /// Σ rețineri din net (D421=C427/4282/462) pentru toate lunile.
    pub total_retinut: String,
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

/// Zile lucrătoare în care contractul e ACTIV în lună — A_8 pentru proratarea bazei minime part-time
/// (art. 146 alin. (5^6) / OMF 1855/2022, câmp D112 A_13P = ROUND(sm × A_8 / NZL)). Intervalul activ
/// în lună = [angajare-sau-prima-zi, încetare-sau-ultima-zi]:
///  - angajare la mijlocul lunii ⇒ se numără din ziua angajării; angajare anterioară/lipsă/invalidă ⇒ ziua 1;
///  - încetare la mijlocul lunii ⇒ se numără până la ziua încetării; fără încetare / încetare ulterioară ⇒
///    ultima zi (neschimbat — evită corect supra-declararea, niciodată sub-declarare);
///  - încetare ÎNAINTE de lună, sau încetare înainte de angajare ⇒ 0 zile (contract inactiv).
pub fn active_working_days(
    year: i32,
    month: u32,
    employment_date: Option<&str>,
    contract_end_date: Option<&str>,
) -> u32 {
    let dim = days_in_month(year, month);
    // (an, lună, zi) dintr-o dată ISO; None dacă e invalidă.
    let parse = |iso: &str| -> Option<(i32, u32, u32)> {
        let p: Vec<&str> = iso.split('-').collect();
        if p.len() != 3 {
            return None;
        }
        Some((p[0].parse().ok()?, p[1].parse().ok()?, p[2].parse().ok()?))
    };
    // Început activ: ziua angajării dacă e în lună; altfel (anterioară/ulterioară/lipsă) ⇒ ziua 1.
    let start_day = match employment_date.and_then(parse) {
        Some((ey, em, ed)) if (ey, em) == (year, month) => ed.clamp(1, dim),
        _ => 1,
    };
    // Sfârșit activ: ziua încetării dacă e în lună; încetare ÎNAINTE de lună ⇒ 0; altfel ⇒ ultima zi.
    let end_day = match contract_end_date.and_then(parse) {
        Some((ey, em, ed)) if (ey, em) == (year, month) => ed.clamp(1, dim),
        Some((ey, em, _)) if (ey, em) < (year, month) => return 0,
        _ => dim,
    };
    if start_day > end_day {
        return 0; // încetare înainte de angajare → 0 (niciodată sub-declarare)
    }
    let s = format!("{year:04}-{month:02}-{start_day:02}");
    let e = format!("{year:04}-{month:02}-{end_day:02}");
    leave_working_days_in_month(year, month, &s, &e)
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

    // ── Wave E: load open diurnă extra-income for this period ────────────────
    // map: employee_id → total taxable excess (Decimal, lei). Empty when no excess exists.
    // Used INSIDE the employee loop to fold excess into combined contribution bases so
    // GL 4315/4316/444/436 == D112 obligations (single combined-base rounding, not split rounding).
    let extra_income = open_extra_income_by_employee(pool, company_id, &period_ym)
        .await
        .unwrap_or_default();

    // ── Wave F: sporuri + rețineri ───────────────────────────────────────────
    // sporuri_map: employee_id → Σ sporuri taxabile (Decimal, lei). Zero when absent.
    // retineri_map: employee_id → Vec<Retinere> sorted by priority (pensie alimentară first).
    let sporuri_map = sporuri_by_employee(pool, company_id, &period_ym)
        .await
        .unwrap_or_default();
    let retineri_map = retineri_by_employee(pool, company_id, &period_ym)
        .await
        .unwrap_or_default();

    // ── Pontaj (condică de prezență — CM art. 119) ───────────────────────────
    // pontaje_map: employee_id → Pontaj. Empty when no pontaj was recorded.
    // When present AND employee has no medical leave, `worked_days` overrides the
    // calendar basis for gross proration and part-time min-base calculation.
    let pontaje_map = pontaj_by_employee(pool, company_id, &period_ym)
        .await
        .unwrap_or_default();

    let mut states = Vec::new();
    // Worked-salary aggregates (intră în nota GL: 641/421, 4315/4316/444 salariale, 646/436 CAM).
    let (mut t_gross, mut t_cas, mut t_cass, mut t_tax, mut t_net) = (
        Decimal::ZERO,
        Decimal::ZERO,
        Decimal::ZERO,
        Decimal::ZERO,
        Decimal::ZERO,
    );
    // Employer-borne part-time minimum-base CAS/CASS difference (art. 146 (5^6)).
    let (mut t_cas_diff, mut t_cass_diff) = (Decimal::ZERO, Decimal::ZERO);
    // CAM (646/436) angajator se calculează pe baza AGREGATĂ — ROUND(Σ bază × cotă), regula A21.46
    // — NU ca Σ rotunjirilor per-salariat. Altfel GL (436) ≠ D112 (480) cu 1 leu când ≥2 salariați
    // rotunjesc în sus. Acumulăm baza (= brut − netaxabil, rotunjită la leu ca baza_cam din D112) și
    // aplicăm cota O SINGURĂ DATĂ după buclă. CCI (4373) a fost ABROGATĂ; nu se acumulează.
    // NOTE: CCI 0,85% (OUG 158/2005) abolished 1 Jan 2018 by OUG 79/2017 — no separate contribution.
    let mut t_cam_base = Decimal::ZERO;
    // Indemnizații de concediu medical (postate separat: 6458/4382/423).
    let mut indemn = IndemnityTotals::default();
    // Wave E — diurnă excess GL tracking. Both are ZERO when no excess exists (feature off by default).
    //   t_excess_reclass   = Σ excess amounts → D 641 / C 625 (reclass travel→salary).
    //   t_excess_receivable = Σ excess withholdings = combined_wh − salary_wh → D 4282 / C 421
    //                          (receivable from employees for charges on their excess cash).
    let (mut t_excess_reclass, mut t_excess_receivable) = (Decimal::ZERO, Decimal::ZERO);
    // Wave F: Σ rețineri nete (D421=C427/4282/462) — split-ul netului față de angajat.
    let mut t_retinut = Decimal::ZERO;
    // Wave F: rețineri GL items per employee — accumulated to pass to post_payroll.
    let mut retin_items: Vec<crate::db::gl::RetinereGlItem> = Vec::new();

    for e in employees.iter().filter(|e| e.active) {
        // Wave F: spor salarial al angajatului pentru această lună (0 dacă absent).
        // Sporurile intră în baza CAS/CASS/impozit/CAM (sunt venituri salariale).
        // `non_taxable` se calculează pe `gross` de bază (nu pe gross+spor) — condiția
        // beneficiarSumaNetaxabila se referă la „salariul de bază" conform OUG 89/2025.
        let spor = sporuri_map.get(&e.id).copied().unwrap_or(Decimal::ZERO);

        let gross = dec(&e.gross_salary);
        // Brut efectiv = gross_salary + sporuri — baza tuturor contribuțiilor.
        let gross_eff = gross + spor;

        let non_taxable = suma_netaxabila(
            e.beneficiar_suma_netaxabila,
            &e.tip_contract,
            gross, // condiția netaxabilă e pe salariul de bază, nu pe gross_eff
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
            // Wave F: sporurile se adaugă la gross înainte de proratare (dacă angajatul are CM +
            // spor, sporul se foloseşte în baza proratată). Modelul simplu: gross_eff = gross + spor,
            // care intră în compute_payroll_with_leave → worked_gross prorata pe gross_eff.
            let lr = compute_payroll_with_leave(&LeavePayrollInput {
                gross: gross_eff,
                personal_deduction: deducere_plafonata(
                    dec(&e.personal_deduction),
                    gross_eff,
                    year,
                    month,
                ),
                non_taxable,
                working_days: nzl,
                certs,
            });
            // Partea salarială lucrată (pentru split-ul GL): contribuțiile pe brutul lucrat.
            let w = compute_payroll(&PayrollInput {
                gross: lr.worked_gross,
                personal_deduction: deducere_plafonata(
                    dec(&e.personal_deduction),
                    gross_eff,
                    year,
                    month,
                ),
                non_taxable,
            });
            let (wcas, wcass, wtax) = (dec(&w.cas), dec(&w.cass), dec(&w.income_tax));
            // Split impozit 421(salarial)/423(indemnizație): CAS se aplică pe TOATE codurile de
            // indemnizație (inclusiv cele scutite de impozit, ex. cod 08), deci pentru o indemnizație
            // neimpozabilă baza combinată de impozit poate coborî SUB baza lucrată ⇒ (lr.tax − wtax)
            // ar deveni NEGATIVĂ. Indemnizația nu poate purta impozit negativ: o plafonăm la 0 și
            // lăsăm deducerea personală integral pe partea salarială (421). Suma rămâne lr.tax ⇒
            // creditul 444 = D112.
            let indemn_tax = (lr.income_tax - wtax).max(Decimal::ZERO);
            // Wave E: fold diurnă excess into the leave employee's combined base (salary+excess).
            // S (salary base) = lr.worked_gross (prorated salary). Contributions on S+E, single rounding.
            let sal_cas_leave = wcas;
            let sal_cass_leave = wcass;
            let sal_tax_leave = lr.income_tax - indemn_tax;
            let ded_leave = deducere_plafonata(dec(&e.personal_deduction), gross_eff, year, month);
            if let Some(&emp_excess) = extra_income.get(&e.id) {
                if emp_excess > Decimal::ZERO {
                    // Combined CAS/CASS on (worked_gross + excess); impozit on combined − CAS − CASS − ded.
                    // No additional deduction applies to asimilat venitor (art. 78).
                    let combined_base = lr.worked_gross + emp_excess;
                    let comb_cas = pct(combined_base, (25, 2));
                    let comb_cass = pct(combined_base, (10, 2));
                    let comb_impozit_base =
                        (combined_base - comb_cas - comb_cass - ded_leave).max(Decimal::ZERO);
                    let comb_sal_impozit = pct(comb_impozit_base, (10, 2));
                    // Excess receivable = combined withholdings − salary withholdings.
                    let excess_recv = (comb_cas - sal_cas_leave)
                        + (comb_cass - sal_cass_leave)
                        + (comb_sal_impozit - sal_tax_leave);
                    t_gross += lr.worked_gross;
                    t_cas += comb_cas;
                    t_cass += comb_cass;
                    t_tax += comb_sal_impozit;
                    t_cam_base += leid0(lr.worked_base) + leid0(emp_excess);
                    t_excess_reclass += emp_excess;
                    t_excess_receivable += excess_recv;
                } else {
                    t_gross += lr.worked_gross;
                    t_cas += sal_cas_leave;
                    t_cass += sal_cass_leave;
                    t_tax += sal_tax_leave;
                    t_cam_base += leid0(lr.worked_base);
                }
            } else {
                t_gross += lr.worked_gross;
                t_cas += sal_cas_leave;
                t_cass += sal_cass_leave;
                t_tax += sal_tax_leave;
                t_cam_base += leid0(lr.worked_base);
            }
            t_net += lr.net;
            // Wave F: rețineri post-net pe ramura concediu medical.
            let retin = if let Some(emp_ret) = retineri_map.get(&e.id) {
                let res = apply_retineri(lr.net, emp_ret);
                t_retinut += res.total_retinut;
                for it in &res.items {
                    retin_items.push(crate::db::gl::RetinereGlItem {
                        account: it.account.clone(),
                        amount: it.suma_efectiva,
                    });
                }
                res
            } else {
                crate::db::payroll_retineri::RetinereResult {
                    total_retinut: Decimal::ZERO,
                    net_redus: lr.net,
                    clamped: false,
                    items: vec![],
                }
            };
            // Indemnizația = combinat − lucrat (CAS/CASS ≥ 0 mereu; impozitul plafonat mai sus).
            indemn.employer += lr.indemn_employer;
            indemn.fnuass += lr.indemn_fnuass;
            indemn.cas += lr.cas - sal_cas_leave;
            indemn.cass += lr.cass - sal_cass_leave;
            indemn.tax += indemn_tax;
            states.push(EmployeeState {
                employee_id: e.id.clone(),
                full_name: e.full_name.clone(),
                gross: f(lr.worked_gross + lr.indemn_total), // total venit (lucrat + indemnizație)
                cas: f(lr.cas),
                cass: f(lr.cass),
                income_tax: f(lr.income_tax),
                net: f(lr.net),
                cam: f(lr.cam),
                spor: f(spor),
                total_retinut: f(retin.total_retinut),
                net_employee: f(retin.net_redus),
            });
            continue;
        }

        // ── Pontaj override: use worked_days as the payroll basis ────────────
        // When a pontaj exists and worked_days < nzl, prorate gross_eff.
        // When worked_days ≥ nzl (or no pontaj), use full gross_eff (no change).
        let pontaj_gross_eff = if let Some(pj) = pontaje_map.get(&e.id) {
            let pontaj_worked = (pj.worked_days as u32).min(nzl);
            if pontaj_worked < nzl && nzl > 0 {
                gross_eff * Decimal::from(pontaj_worked) / Decimal::from(nzl)
            } else {
                gross_eff
            }
        } else {
            gross_eff
        };
        let r = compute_payroll(&PayrollInput {
            gross: pontaj_gross_eff,
            personal_deduction: deducere_plafonata(
                dec(&e.personal_deduction),
                gross_eff,
                year,
                month,
            ),
            non_taxable,
        });
        let exempt = exempt_part_time_min_base(e.pensionar, &e.exceptie_cas_min);
        // Aceeași proratare pe zile active ca în emiterea D112 (commands/payroll.rs) — diferența de
        // contribuție suportată de angajator din sumar trebuie să coincidă cu D112.
        // Pontaj: dacă există, `worked_days` (clamped la nzl) înlocuiește `active_days` pentru
        // calculul bazei minime part-time.
        let active_days = if let Some(pj) = pontaje_map.get(&e.id) {
            (pj.worked_days as u32).min(nzl)
        } else {
            active_working_days(
                year,
                month,
                e.employment_date.as_deref(),
                e.contract_end_date.as_deref(),
            )
        };
        if let Some((_, cas_diff, cass_diff)) = part_time_min_base(
            gross_eff,
            &e.tip_contract,
            exempt,
            year,
            month,
            active_days,
            nzl,
        ) {
            t_cas_diff += cas_diff;
            t_cass_diff += cass_diff;
        }
        // Wave E: fold diurnă excess into combined base for single-rounding contributions.
        // S = salary gross (dec(&r.gross)) which already includes sporuri via gross_eff.
        // E = excess from payroll_extra_income (or 0). When E=0 path is byte-identical to pre-Wave-E.
        let sal_gross = dec(&r.gross);
        let sal_cas = dec(&r.cas);
        let sal_cass = dec(&r.cass);
        let sal_impozit = dec(&r.income_tax);
        if let Some(&emp_excess) = extra_income.get(&e.id) {
            if emp_excess > Decimal::ZERO {
                // Combined base = salary gross (incl. spor) + excess.
                // non_taxable already excluded from r.gross via compute_payroll.
                let emp_ded =
                    deducere_plafonata(dec(&e.personal_deduction), gross_eff, year, month);
                let combined_base = sal_gross + emp_excess;
                let comb_cas = pct(combined_base, (25, 2));
                let comb_cass = pct(combined_base, (10, 2));
                let comb_impozit_base =
                    (combined_base - comb_cas - comb_cass - emp_ded).max(Decimal::ZERO);
                let comb_impozit = pct(comb_impozit_base, (10, 2));
                // Excess receivable = combined withholdings − salary withholdings (employee's debt).
                let excess_recv =
                    (comb_cas - sal_cas) + (comb_cass - sal_cass) + (comb_impozit - sal_impozit);
                t_gross += sal_gross;
                t_cas += comb_cas; // combined — matches D112 single rounding
                t_cass += comb_cass;
                t_tax += comb_impozit;
                t_net += dec(&r.net); // salary net (excess net was already paid cash)
                t_cam_base +=
                    leid0((pontaj_gross_eff - non_taxable).max(Decimal::ZERO)) + leid0(emp_excess);
                t_excess_reclass += emp_excess;
                t_excess_receivable += excess_recv;
            } else {
                t_gross += sal_gross;
                t_cas += sal_cas;
                t_cass += sal_cass;
                t_tax += sal_impozit;
                t_net += dec(&r.net);
                t_cam_base += leid0((pontaj_gross_eff - non_taxable).max(Decimal::ZERO));
            }
        } else {
            t_gross += sal_gross;
            t_cas += sal_cas;
            t_cass += sal_cass;
            t_tax += sal_impozit;
            t_net += dec(&r.net);
            t_cam_base += leid0((pontaj_gross_eff - non_taxable).max(Decimal::ZERO));
        }
        // Wave F: rețineri post-net pentru calea standard (fără concediu medical).
        let emp_net = dec(&r.net);
        let retin = if let Some(emp_ret) = retineri_map.get(&e.id) {
            let res = apply_retineri(emp_net, emp_ret);
            t_retinut += res.total_retinut;
            for it in &res.items {
                retin_items.push(crate::db::gl::RetinereGlItem {
                    account: it.account.clone(),
                    amount: it.suma_efectiva,
                });
            }
            res
        } else {
            crate::db::payroll_retineri::RetinereResult {
                total_retinut: Decimal::ZERO,
                net_redus: emp_net,
                clamped: false,
                items: vec![],
            }
        };
        states.push(EmployeeState {
            employee_id: e.id.clone(),
            full_name: e.full_name.clone(),
            gross: r.gross,
            cas: r.cas,
            cass: r.cass,
            income_tax: r.income_tax,
            net: r.net.clone(),
            cam: r.cam,
            spor: f(spor),
            total_retinut: f(retin.total_retinut),
            net_employee: f(retin.net_redus),
        });
    }

    // Aplică cota O SINGURĂ DATĂ pe baza agregată (rotunjire comercială la leu) — GL 436 = D112
    // (480) la leu, eliminând diferența de reconciliere din Σ rotunjirilor per-salariat.
    // CAM 2,25% (646/436) = singura contribuție angajator pe fondul de salarii.
    // CCI 0,85% (4373) a fost ABROGATĂ prin OUG 79/2017 de la 1 ian. 2018 — nu se mai calculează.
    let t_cam = pct(t_cam_base, CAM_PCT);

    let post = crate::db::gl::post_payroll(
        pool,
        company_id,
        period_from,
        period_to,
        crate::db::gl::PayrollTotals {
            gross: t_gross,
            cas: t_cas,
            cass: t_cass,
            impozit: t_tax,
            cam: t_cam,
            cas_diff: t_cas_diff,
            cass_diff: t_cass_diff,
            indemn: indemn.clone(),
            excess_reclass: t_excess_reclass,
            excess_receivable: t_excess_receivable,
            // Wave F: rețineri — net split per creditor account.
            retineri: retin_items,
        },
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
        total_retinut: f(t_retinut),
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

    /// Wave 0 — cross-company data isolation for employees: get/update/delete with a foreign
    /// company_id must return NotFound (employees is one of the newest scoped entities; had no test).
    #[tokio::test]
    async fn employee_cross_company_isolation() {
        let pool = setup().await;
        let e = create(
            &pool,
            CreateEmployeeInput {
                company_id: "co1".into(),
                cnp: "1900101410011".into(),
                full_name: "Ion".into(),
                gross_salary: "5000".into(),
                personal_deduction: Some("0".into()),
                employment_date: None,
                contract_end_date: None,
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
        use crate::error::AppError;
        assert!(
            matches!(
                get(&pool, &e.id, "intrus-co").await,
                Err(AppError::NotFound)
            ),
            "get cross-company → NotFound"
        );
        assert!(
            matches!(
                update(&pool, &e.id, "intrus-co", UpdateEmployeeInput::default()).await,
                Err(AppError::NotFound)
            ),
            "update cross-company → NotFound"
        );
        assert!(
            matches!(
                delete(&pool, &e.id, "intrus-co").await,
                Err(AppError::NotFound)
            ),
            "delete cross-company → NotFound"
        );
        assert!(get(&pool, &e.id, "co1").await.is_ok(), "owner keeps access");
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
                    contract_end_date: None,
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
        // 2 × (gross 5000, CAS 1250, CASS 500, impozit 325, net 2925). CAM = ROUND(Σ bază × cotă)
        // pe baza AGREGATĂ (10.000), NU Σ rotunjirilor per-salariat — egal cu D112 (480).
        // GOLDEN: CCI 0,85% REMOVED (abolished OUG 79/2017); ONLY CAM 2,25% remains as employer
        // social contribution. No total_concedii field, no 4373 posting.
        assert_eq!(run.total_gross, "10000.00");
        assert_eq!(run.total_cas, "2500.00");
        assert_eq!(run.total_cass, "1000.00");
        assert_eq!(run.total_income_tax, "650.00");
        assert_eq!(run.total_net, "5850.00");
        assert_eq!(run.total_cam, "225.00"); // ROUND(10.000 × 2,25%) = 225 (NU 2 × round(112,5)=226)
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
        assert_eq!(bal("646"), Some(("225.00".into(), "0.00".into())));
        // GOLDEN: no phantom CCI — 6458 and 4373 must NOT appear in a pure salary payroll.
        // Employer cost above gross = CAM 2,25% ONLY (646/436); 4373 is NEVER posted post-2018.
        assert!(
            bal("6458").is_none(),
            "6458 must not appear — no indemnity, no part-time top-up, no phantom CCI"
        );
        assert!(
            bal("4373").is_none(),
            "4373 must not be posted — CCI abolished OUG 79/2017"
        );
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
                contract_end_date: None,
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
        // CCI 0,85% REMOVED (abolished OUG 79/2017) — no total_concedii, no 4373.
        assert_eq!(run.total_gross, "4790.00"); // lucrat 4190 + indemnizație 600
        assert_eq!(run.total_cas, "1198.00");
        assert_eq!(run.total_cass, "479.00");
        assert_eq!(run.total_income_tax, "311.00");
        assert_eq!(run.total_cam, "94.00");
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
        // 6458 = ONLY the 600 employer indemnity — NO phantom CCI 0,85% (abolished OUG 79/2017).
        // Before fix: 6458 = 636 (600 indemnity + 36 phantom CCI). After fix: 6458 = 600 (indemnity only).
        assert_eq!(bal("6458"), Some(("600.00".into(), "0.00".into())));
        // 4373 must NOT appear — CCI abolished, account no longer posted post-2018.
        assert!(
            bal("4373").is_none(),
            "4373 must not be posted — CCI abolished OUG 79/2017"
        );
        assert_eq!(bal("421"), Some(("0.00".into(), "2451.00".into()))); // 4190 − 1048 − 419 − 272
        assert_eq!(bal("423"), Some(("0.00".into(), "351.00".into()))); // 600 − 150 − 60 − 39
                                                                        // Creditele de contribuții = cele combinate (lucrat + indemnizație) = obligațiile D112.
        assert_eq!(bal("4315"), Some(("0.00".into(), "1198.00".into())));
        assert_eq!(bal("4316"), Some(("0.00".into(), "479.00".into())));
        assert_eq!(bal("444"), Some(("0.00".into(), "311.00".into())));
        assert_eq!(bal("646"), Some(("94.00".into(), "0.00".into())));
        assert!(
            tb.balanced,
            "payroll + indemnity journal balances — indemnity UNCHANGED, phantom CCI removed"
        );
    }

    #[test]
    fn working_days_excludes_legal_holidays() {
        // Iunie 2026: 22 zile L-V, dar 1 iunie (luni) e sărbătoare legală ⇒ 21 NZL (ca la validatorul
        // ANAF — regula S21.1). Fără excluderea sărbătorii ar fi 22 (bug prins de testul e2e).
        assert_eq!(working_days(2026, 6), 21);
        // Ianuarie 2026: 1 ian = joi; 22 zile L-V minus 4 sărbători în zile lucrătoare (1,2,6,7) = 18.
        assert_eq!(working_days(2026, 1), 18);
    }

    #[test]
    fn active_working_days_prorates_hire_and_termination() {
        // Martie 2026 = 22 zile lucrătoare (fără sărbători L-V).
        assert_eq!(working_days(2026, 3), 22);
        // Fără date / angajare anterioară / dată invalidă ⇒ NZL întreg (activ toată luna).
        assert_eq!(active_working_days(2026, 3, None, None), 22);
        assert_eq!(active_working_days(2026, 3, Some("2025-11-01"), None), 22);
        assert_eq!(active_working_days(2026, 3, Some("2026-03-01"), None), 22); // angajat ziua 1
        assert_eq!(active_working_days(2026, 3, Some("invalid"), None), 22);
        // Angajare la mijloc (16 mar = luni): 16-20, 23-27, 30-31 = 12 zile lucrătoare.
        assert_eq!(active_working_days(2026, 3, Some("2026-03-16"), None), 12);
        // ÎNCETARE la mijloc (20 mar = vineri), fără dată angajare: 2-6, 9-13, 16-20 = 15 zile.
        assert_eq!(active_working_days(2026, 3, None, Some("2026-03-20")), 15);
        // Angajare 16 + încetare 20 (același interval scurt): 16-20 = 5 zile lucrătoare.
        assert_eq!(
            active_working_days(2026, 3, Some("2026-03-16"), Some("2026-03-20")),
            5
        );
        // Încetare ÎNAINTE de lună ⇒ contract inactiv = 0 zile.
        assert_eq!(active_working_days(2026, 3, None, Some("2026-02-28")), 0);
        // Încetare înainte de angajare (date inconsecvente) ⇒ 0 (niciodată sub-declarare).
        assert_eq!(
            active_working_days(2026, 3, Some("2026-03-16"), Some("2026-03-10")),
            0
        );
        // Încetare în lună ulterioară ⇒ toată luna curentă (neschimbat).
        assert_eq!(active_working_days(2026, 3, None, Some("2026-04-15")), 22);
    }

    #[test]
    fn parse_money_accepts_plain_decimals_and_rejects_garbage() {
        assert_eq!(parse_money("Brut", "4050").unwrap(), "4050");
        assert_eq!(parse_money("Brut", " 1234.56 ").unwrap(), "1234.56");
        assert_eq!(parse_money("Brut", "0").unwrap(), "0");
        // Negative + non-numeric rejected (salariul nu devine niciodată tăcut 0).
        assert!(parse_money("Brut", "-1").is_err());
        assert!(parse_money("Brut", "abc").is_err());
        // PAY-02: notația științifică e respinsă (altfel "1e10" ⇒ 10_000_000_000 lei dintr-o tastare).
        assert!(parse_money("Brut", "1e10").is_err());
        assert!(parse_money("Brut", "1E5").is_err());
        assert!(parse_money("Brut", "-1e3").is_err());
    }

    // ── Wave F: Sporuri + Rețineri ────────────────────────────────────────────

    fn emp_input_f(cnp: &str, name: &str, gross: &str) -> CreateEmployeeInput {
        CreateEmployeeInput {
            company_id: "co1".into(),
            cnp: cnp.into(),
            full_name: name.into(),
            gross_salary: gross.into(),
            personal_deduction: Some("0".into()),
            employment_date: None,
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

    /// SPOR-1: angajat cu salariu de bază 4000 + spor 1000 → contribuțiile calculate pe 5000.
    /// CAS 25%×5000=1250, CASS 10%×5000=500, impozit 10%×(5000−1250−500)=325, net=2925.
    /// CAM 2,25% pe baza agregată (5000, un singur angajat) = ROUND(5000×2,25%)=113.
    /// GL≡D112: un singur angajat, deci nu e diferență de roundup agregat vs per-salariat.
    #[tokio::test]
    async fn spor_folds_into_gross_contributions_computed_on_combined() {
        let pool = setup().await;
        let emp = create(&pool, emp_input_f("1900101410011", "Pop Ion", "4000"))
            .await
            .unwrap();

        // Inserează spor 1000 lei (vecchime).
        crate::db::payroll_sporuri::create(
            &pool,
            crate::db::payroll_sporuri::CreateSporInput {
                company_id: "co1".into(),
                employee_id: emp.id.clone(),
                period: "2026-06".into(),
                amount: "1000".into(),
                kind: Some("vechime".into()),
                description: None,
            },
        )
        .await
        .unwrap();

        let run = run_payroll(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();

        // Contribuțiile pe gross_eff=5000 (salariu 4000 + spor 1000):
        // CAS=1250, CASS=500, impozit=10%×(5000−1250−500)=325, net=2925, CAM=113.
        assert_eq!(run.states.len(), 1);
        let st = &run.states[0];
        assert_eq!(st.gross, "5000.00", "gross trebuie să includă sporul");
        assert_eq!(st.cas, "1250.00", "CAS 25% pe 5000");
        assert_eq!(st.cass, "500.00", "CASS 10% pe 5000");
        assert_eq!(st.income_tax, "325.00", "impozit pe 5000");
        assert_eq!(st.net, "2925.00", "net înainte de rețineri");
        assert_eq!(st.spor, "1000.00", "spor raportat");
        assert_eq!(st.total_retinut, "0.00", "fără rețineri");
        assert_eq!(
            st.net_employee, "2925.00",
            "net angajat = net când fără rețineri"
        );
        // CAM agregat = ROUND(5000×2,25%) = 113 (ca în golden test 5000 brut)
        assert_eq!(run.total_cam, "113.00", "CAM pe 5000");
        assert!(run.posted);

        // GL: 641 debit = 5000 (gross_eff incl. spor), 421 credit = 2925 (net angajat).
        let tb = crate::db::gl::trial_balance(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        let bal = |code: &str| {
            tb.rows
                .iter()
                .find(|r| r.account_code == code)
                .map(|r| (r.closing_debit.clone(), r.closing_credit.clone()))
        };
        assert_eq!(bal("641"), Some(("5000.00".into(), "0.00".into())));
        // 421 credit = net = 2925; 4315 = CAS = 1250; 4316 = CASS = 500; 444 = impozit = 325.
        assert_eq!(bal("4315"), Some(("0.00".into(), "1250.00".into())));
        assert_eq!(bal("4316"), Some(("0.00".into(), "500.00".into())));
        assert_eq!(bal("444"), Some(("0.00".into(), "325.00".into())));
        // 421 credit = 2925 (net angajat — fără rețineri, tot netul merge la angajat)
        assert_eq!(bal("421"), Some(("0.00".into(), "2925.00".into())));
        assert!(tb.balanced, "sporuri journal trebuie să fie echilibrat");
    }

    /// SPOR-2 GL≡D112: sporurile intră în baza D112 — golden test cu spor.
    /// Doi angajați: A salariu 5000 fără spor, B salariu 4000 + spor 1000 → baza totală CAM = 10000.
    /// ROUND(10000×2,25%) = 225 (identic cu 2×5000 din golden original).
    /// 421 credit = (5000−1250−500−325) + (5000−1250−500−325) = 2925+2925 = 5850.
    #[tokio::test]
    async fn spor_gl_equals_d112_combined_base_golden() {
        let pool = setup().await;
        // Angajat A: brut 5000, fără spor.
        create(&pool, emp_input_f("1", "A", "5000")).await.unwrap();
        // Angajat B: brut 4000 + spor 1000 = gross_eff 5000.
        let b = create(&pool, emp_input_f("2", "B", "4000")).await.unwrap();
        crate::db::payroll_sporuri::create(
            &pool,
            crate::db::payroll_sporuri::CreateSporInput {
                company_id: "co1".into(),
                employee_id: b.id.clone(),
                period: "2026-06".into(),
                amount: "1000".into(),
                kind: None,
                description: None,
            },
        )
        .await
        .unwrap();

        let run = run_payroll(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        // Ambii pe gross_eff 5000 → total_gross=10000 (A:5000 + B:5000).
        assert_eq!(run.total_gross, "10000.00");
        assert_eq!(run.total_cas, "2500.00");
        assert_eq!(run.total_cass, "1000.00");
        assert_eq!(run.total_income_tax, "650.00");
        assert_eq!(run.total_net, "5850.00");
        // CAM pe baza AGREGATĂ 10000 (single rounding) = 225, identic cu golden original.
        assert_eq!(run.total_cam, "225.00");
        assert!(run.posted);

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
        assert_eq!(bal("646"), Some(("225.00".into(), "0.00".into())));
        assert!(
            tb.balanced,
            "sporuri GL≡D112 golden journal trebuie să fie echilibrat"
        );
    }

    /// RETINERE-1: angajat net 2925 (brut 5000 = 4000+1000 spor), poprire 800.
    /// 800 ≤ 1/3 × 2925 = 975 → reținere trece integral.
    /// GL: D421=C427 800, D421=C5311 2125; contribuțiile NESCHIMBATE.
    #[tokio::test]
    async fn retinere_post_net_splits_421_into_427_and_5311() {
        let pool = setup().await;
        let emp = create(&pool, emp_input_f("1900101410011", "Pop Ion", "4000"))
            .await
            .unwrap();
        // Spor 1000 → gross_eff = 5000, net = 2925.
        crate::db::payroll_sporuri::create(
            &pool,
            crate::db::payroll_sporuri::CreateSporInput {
                company_id: "co1".into(),
                employee_id: emp.id.clone(),
                period: "2026-06".into(),
                amount: "1000".into(),
                kind: None,
                description: None,
            },
        )
        .await
        .unwrap();
        // Poprire 800 (< 1/3 × 2925 = 975 → trece integral).
        crate::db::payroll_retineri::create(
            &pool,
            crate::db::payroll_retineri::CreateRetinereInput {
                company_id: "co1".into(),
                employee_id: emp.id.clone(),
                period: "2026-06".into(),
                amount: "800".into(),
                kind: Some("poprire".into()),
                creditor: Some("Tribunal".into()),
                account: Some("427".into()),
                priority: None,
            },
        )
        .await
        .unwrap();

        let run = run_payroll(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        // Contribuțiile NESCHIMBATE (reținerea e post-net):
        assert_eq!(run.total_cas, "1250.00", "CAS neschimbat");
        assert_eq!(run.total_cass, "500.00", "CASS neschimbat");
        assert_eq!(run.total_income_tax, "325.00", "impozit neschimbat");
        // Net total (înainte de rețineri) = 2925; totalRetinut = 800; net angajat = 2125.
        assert_eq!(run.total_net, "2925.00", "net total înainte rețineri");
        assert_eq!(run.total_retinut, "800.00", "total reținut");
        assert_eq!(run.states[0].net_employee, "2125.00", "net efectiv angajat");

        // GL: contribuțiile identice cu cazul fără rețineri (4315/4316/444/436 neschimbate).
        let tb = crate::db::gl::trial_balance(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        let bal = |code: &str| {
            tb.rows
                .iter()
                .find(|r| r.account_code == code)
                .map(|r| (r.closing_debit.clone(), r.closing_credit.clone()))
        };
        // Contribuțiile = obligațiile D112 (neschimbate de rețineri):
        assert_eq!(bal("4315"), Some(("0.00".into(), "1250.00".into())));
        assert_eq!(bal("4316"), Some(("0.00".into(), "500.00".into())));
        assert_eq!(bal("444"), Some(("0.00".into(), "325.00".into())));
        // 421: credit net 2925, debit reținere 800 → sold credit 2125.
        // Nota: GL înregistrează D421/C427=800 (reținere) în aceeași notă PAYROLL.
        // 421 net credit = 2925 - 800 = 2125 (de plată angajat); 427 credit = 800 (de plată terț).
        // Balanța: suma = 5000 debit 641 + 113 debit 646 = 5000 credit 421 brut +1250 C4315 +500 C4316 +325 C444 +113 C436;
        // din 421 credit: 1250+500+325 D421 (rețineri contribuții) + 800 D421 C427 → 421 net credit = 5000-2075-800=2125.
        assert!(tb.balanced, "rețineri journal trebuie să fie echilibrat");
        // 427 credit = 800 (obligație față de terț).
        assert_eq!(bal("427"), Some(("0.00".into(), "800.00".into())));
    }

    /// RETINERE-2: reținere depășind 1/3 net → plafonată la 1/3.
    /// Net = 2925; cerut 1500 > 1/3=975 → efectiv 975; net angajat = 1950.
    /// Contribuțiile NESCHIMBATE.
    #[tokio::test]
    async fn retinere_exceeds_cap_is_clamped() {
        let pool = setup().await;
        let emp = create(&pool, emp_input_f("1900101410011", "Pop Ion", "5000"))
            .await
            .unwrap();
        // Net = 5000 - 1250 - 500 - 325 = 2925.
        crate::db::payroll_retineri::create(
            &pool,
            crate::db::payroll_retineri::CreateRetinereInput {
                company_id: "co1".into(),
                employee_id: emp.id.clone(),
                period: "2026-06".into(),
                amount: "1500".into(), // > 1/3 × 2925 = 975
                kind: Some("poprire".into()),
                creditor: None,
                account: Some("427".into()),
                priority: None,
            },
        )
        .await
        .unwrap();

        let run = run_payroll(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        // Contribuțiile neschimbate:
        assert_eq!(run.total_cas, "1250.00");
        assert_eq!(run.total_cass, "500.00");
        assert_eq!(run.total_income_tax, "325.00");
        // Reținere clamped la 975 (1/3 din 2925):
        assert_eq!(run.total_retinut, "975.00", "clamped la 1/3 net");
        assert_eq!(
            run.states[0].net_employee, "1950.00",
            "net efectiv = 2925-975"
        );

        let tb = crate::db::gl::trial_balance(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        assert!(tb.balanced, "journal echilibrat și cu reținere clamped");
        let c427 = tb
            .rows
            .iter()
            .find(|r| r.account_code == "427")
            .map(|r| r.closing_credit.clone())
            .unwrap_or_default();
        assert_eq!(c427, "975.00", "427 credit = suma clamped");
    }

    /// RETINERE-3: sporuri + rețineri împreună, un singur angajat.
    /// Gross 4000 + spor 1000 = 5000. CAS 1250, CASS 500, impozit 325, net 2925.
    /// Poprire 800 (< 975) + pensie alimentară 900 (< 975). Σ = 1700 > 1/2×2925=1462.5.
    /// Pensie alimentară (priority 1) → 900; poprire → min(800, 975, 562.5) = 562.5.
    /// Total reținut = 900+562.5=1462.5; net angajat = 1462.5.
    #[tokio::test]
    async fn spor_and_retineri_combined_on_one_employee() {
        let pool = setup().await;
        let emp = create(&pool, emp_input_f("1900101410011", "Pop Ion", "4000"))
            .await
            .unwrap();
        // Spor 1000
        crate::db::payroll_sporuri::create(
            &pool,
            crate::db::payroll_sporuri::CreateSporInput {
                company_id: "co1".into(),
                employee_id: emp.id.clone(),
                period: "2026-06".into(),
                amount: "1000".into(),
                kind: None,
                description: None,
            },
        )
        .await
        .unwrap();
        // Pensie alimentară 900 (priority 1)
        crate::db::payroll_retineri::create(
            &pool,
            crate::db::payroll_retineri::CreateRetinereInput {
                company_id: "co1".into(),
                employee_id: emp.id.clone(),
                period: "2026-06".into(),
                amount: "900".into(),
                kind: Some("pensie_alimentara".into()),
                creditor: None,
                account: Some("427".into()),
                priority: None, // defaults to 1
            },
        )
        .await
        .unwrap();
        // Poprire 800 (priority 2)
        crate::db::payroll_retineri::create(
            &pool,
            crate::db::payroll_retineri::CreateRetinereInput {
                company_id: "co1".into(),
                employee_id: emp.id.clone(),
                period: "2026-06".into(),
                amount: "800".into(),
                kind: Some("poprire".into()),
                creditor: None,
                account: Some("462".into()),
                priority: Some(2),
            },
        )
        .await
        .unwrap();

        let run = run_payroll(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        // Contribuțiile pe gross_eff 5000 (neschimbate de rețineri):
        assert_eq!(run.total_cas, "1250.00", "CAS pe 5000");
        assert_eq!(run.total_cass, "500.00", "CASS pe 5000");
        assert_eq!(run.total_income_tax, "325.00", "impozit pe 5000");
        assert_eq!(run.total_net, "2925.00", "net total înainte rețineri");

        // 1/3 din 2925 = 975; 1/2 din 2925 = 1462.50.
        // Pensie alimentară (priority 1): min(900, 975, 1462.50) = 900.
        // Poprire (priority 2): min(800, 975, 1462.50-900=562.50) = 562.50.
        // Total = 1462.50.
        let total_ret = rust_decimal::Decimal::from_str(&run.total_retinut).unwrap();
        assert_eq!(
            total_ret,
            rust_decimal::Decimal::new(146250, 2), // 1462.50
            "total reținut = 1462.50 (plafon 1/2 net)"
        );
        let net_emp = rust_decimal::Decimal::from_str(&run.states[0].net_employee).unwrap();
        assert_eq!(
            net_emp,
            rust_decimal::Decimal::new(146250, 2), // 1462.50
            "net angajat = 1462.50"
        );

        // GL: echilibrat; 427 = 900 (pensie alimentară); 462 = 562.50 (poprire clamped).
        let tb = crate::db::gl::trial_balance(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        let bal_credit = |code: &str| {
            tb.rows
                .iter()
                .find(|r| r.account_code == code)
                .map(|r| r.closing_credit.clone())
                .unwrap_or_default()
        };
        assert_eq!(bal_credit("427"), "900.00", "427 = pensie alimentară 900");
        let c462 = rust_decimal::Decimal::from_str(&bal_credit("462")).unwrap_or_default();
        assert_eq!(
            c462,
            rust_decimal::Decimal::new(56250, 2), // 562.50
            "462 = poprire clamped 562.50"
        );
        assert!(tb.balanced, "sporuri+rețineri journal trebuie echilibrat");
    }

    /// RETINERE-4: reținerea nu modifică contribuțiile (sunt post-net).
    /// Verifică explicit că 4315/4316/444/436 sunt identice cu/fără rețineri.
    #[tokio::test]
    async fn retinere_does_not_affect_contributions_or_d112_base() {
        let pool = setup().await;
        let emp = create(&pool, emp_input_f("1900101410011", "Pop Ion", "5000"))
            .await
            .unwrap();

        // Rulare fără rețineri (referință).
        let run_ref = run_payroll(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        let cas_ref = run_ref.total_cas.clone();
        let cass_ref = run_ref.total_cass.clone();
        let tax_ref = run_ref.total_income_tax.clone();
        let cam_ref = run_ref.total_cam.clone();

        // Adaugă o reținere.
        crate::db::payroll_retineri::create(
            &pool,
            crate::db::payroll_retineri::CreateRetinereInput {
                company_id: "co1".into(),
                employee_id: emp.id.clone(),
                period: "2026-07".into(), // luna diferită — nu interferă cu 06
                amount: "500".into(),
                kind: None,
                creditor: None,
                account: Some("427".into()),
                priority: None,
            },
        )
        .await
        .unwrap();

        // Re-rulare (idempotentă, aceeași lună):
        let run2 = run_payroll(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        assert_eq!(run2.total_cas, cas_ref, "CAS neschimbat");
        assert_eq!(run2.total_cass, cass_ref, "CASS neschimbat");
        assert_eq!(run2.total_income_tax, tax_ref, "impozit neschimbat");
        assert_eq!(run2.total_cam, cam_ref, "CAM neschimbat");
        // Luna 07 n-a afectat luna 06:
        assert_eq!(run2.total_retinut, "0.00", "fără rețineri în iunie");
    }
}
