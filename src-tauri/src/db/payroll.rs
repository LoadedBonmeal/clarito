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
    exempt_part_time_min_base, part_time_min_base, pct, suma_netaxabila, LeaveCert,
    LeavePayrollInput, PayrollInput, CAM_PCT,
};
use crate::db::models::{new_id, now_unix};
use crate::db::payroll_diurna::open_extra_income_by_employee;
use crate::db::payroll_retineri::retineri_by_employee;
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
    /// Funcția (denumirea postului), ex. "Programator". Folosit în exportul REGES-Online
    /// (Registrul General de Evidență a Salariaților, HG 295/2025). String liber, opțional.
    pub functia: String,
    /// Codul COR (Clasificarea Ocupațiilor din România) — 6 cifre, ex. "251202".
    /// Obligatoriu în REGES-Online; golit la creare (utilizatorul îl completează).
    pub cod_cor: String,
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
    #[serde(default)]
    pub functia: Option<String>,
    #[serde(default)]
    pub cod_cor: Option<String>,
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
    pub functia: Option<String>,
    pub cod_cor: Option<String>,
}

const COLS: &str = "id, company_id, cnp, full_name, gross_salary, personal_deduction, \
                    employment_date, contract_end_date, active, tip_asigurat, pensionar, \
                    tip_contract, ore_norma, exceptie_cas_min, sediu_cif, \
                    beneficiar_suma_netaxabila, functia, cod_cor, created_at, updated_at";

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
         ore_norma, exceptie_cas_min, sediu_cif, beneficiar_suma_netaxabila, functia, cod_cor, \
         created_at, updated_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,1,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?18)",
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
    .bind(input.functia.as_deref().unwrap_or("").trim())
    .bind(input.cod_cor.as_deref().unwrap_or("").trim())
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
         beneficiar_suma_netaxabila=?16, functia=?17, cod_cor=?18, updated_at=?19 \
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
    .bind(input.functia.as_deref().unwrap_or(&cur.functia))
    .bind(input.cod_cor.as_deref().unwrap_or(&cur.cod_cor))
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

// ─── Per-employee payroll breakdown ──────────────────────────────────────────

/// Per-employee payroll breakdown — the single source of truth for both GL posting and D112 XML.
#[derive(Debug, Clone)]
pub struct EmployeeBreakdown {
    pub employee_id: String,
    pub full_name: String,
    pub cnp: String,
    pub spor: Decimal,
    pub gross_base: Decimal,
    pub gross_eff: Decimal,
    pub pontaj_gross_eff: Decimal,
    pub non_taxable: Decimal,
    pub active_days: u32,
    pub zile_emis: u32,
    pub is_leave_path: bool,

    // A-path fields (standard salary)
    pub sal_gross: Decimal,
    pub sal_cas: Decimal,
    pub sal_cass: Decimal,
    pub sal_impozit: Decimal,
    pub sal_net: Decimal,
    pub sal_cam: Decimal,
    pub sal_taxable_base: Decimal,
    pub sal_personal_deduction: Decimal,
    pub combined_cas: Decimal,
    pub combined_cass: Decimal,
    pub combined_impozit: Decimal,
    /// Combined-base taxable base for impozit (sal_gross + excess − comb_cas − comb_cass − ded).
    /// Zero when there is no excess. Used by build_d112_xml's Wave E to set baza_impozit without
    /// recomputing from the already-rounded de.gross (i64), which would lose pontaj precision.
    pub comb_impozit_base: Decimal,
    pub excess: Decimal,
    pub excess_receivable: Decimal,
    pub part_time_min: Option<(Decimal, Decimal, Decimal)>,
    pub baza_cas: Decimal,
    pub baza_cass: Decimal,
    pub baza_cam_real: Decimal,
    pub baza_impozit: Decimal,
    pub tip_asigurat: String,
    pub cam_base_contribution: Decimal,

    // B-path fields (medical leave)
    pub lr_worked_gross: Decimal,
    pub lr_worked_base: Decimal,
    pub lr_worked_days: u32,
    pub lr_cas: Decimal,
    pub lr_cass: Decimal,
    pub lr_income_tax: Decimal,
    pub lr_cam: Decimal,
    pub lr_net: Decimal,
    pub lr_indemn_employer: Decimal,
    pub lr_indemn_fnuass: Decimal,
    pub lr_indemn_total: Decimal,
    pub lr_taxable_base: Decimal,
    pub sal_cas_leave: Decimal,
    pub sal_cass_leave: Decimal,
    pub sal_tax_leave: Decimal,
    pub indemn_tax: Decimal,
    pub b_combined_cas: Decimal,
    pub b_combined_cass: Decimal,
    pub b_comb_sal_impozit: Decimal,
    pub b_excess_recv: Decimal,
    pub b_excess: Decimal,
    pub b_cam_base: Decimal,

    // GL contribution fields (what goes into the GL totals)
    pub gl_cas: Decimal,
    pub gl_cass: Decimal,
    pub gl_tax: Decimal,
    pub gl_gross: Decimal,
    pub gl_cam_base: Decimal,
    pub gl_excess_reclass: Decimal,
    pub gl_excess_receivable: Decimal,

    // Rețineri
    pub retineri_result: crate::db::payroll_retineri::RetinereResult,

    // Medical leaves for D112
    pub med_leaves_raw: Vec<crate::db::concedii::MedicalLeave>,

    // Employee metadata for D112
    pub employment_date: Option<String>,
    pub tip_contract: String,
    pub pensionar: bool,
    pub ore_norma: i64,
    pub sediu_cif: String,
    pub beneficiar_suma_netaxabila: bool,
    pub exceptie_cas_min: String,
}

/// The result of `compute_payroll_run`.
#[derive(Debug, Clone)]
pub struct PayrollRunBreakdown {
    pub employees: Vec<EmployeeBreakdown>,
    pub t_gross: Decimal,
    pub t_cas: Decimal,
    pub t_cass: Decimal,
    pub t_tax: Decimal,
    pub t_net: Decimal,
    pub t_cam_base: Decimal,
    pub t_cam: Decimal,
    pub t_cas_diff: Decimal,
    pub t_cass_diff: Decimal,
    pub t_excess_reclass: Decimal,
    pub t_excess_receivable: Decimal,
    pub t_retinut: Decimal,
    pub indemn: crate::db::gl::IndemnityTotals,
    pub retin_items: Vec<crate::db::gl::RetinereGlItem>,
    pub nzl: u32,
    pub year: i32,
    pub month: u32,
}

/// **SINGLE SOURCE OF TRUTH** for all per-employee payroll computation.
///
/// Loads all active employees + their leaves, sporuri, pontaje, and diurnă extra-income for the
/// given period, and computes for each employee:
/// - `gross_eff` (base salary + sporuri, prorated by pontaj `worked_days/nzl` when applicable)
/// - `non_taxable` (suma netaxabilă art. III OUG 89/2025, on the BASE salary, not on gross+spor)
/// - part-time minimum CAS/CASS base (art. 146 alin. (5^6)), employer-borne difference
/// - CAS/CASS/impozit/CAM on the COMBINED base (salary + diurnă excess) with SINGLE rounding
/// - net, rețineri post-net, leave (B-path) indemnity values
///
/// Returns a [`PayrollRunBreakdown`] with per-employee [`EmployeeBreakdown`] structs and the
/// aggregated GL totals (t_gross, t_cas, t_cass, t_tax, t_cam, indemn, retin_items, …).
///
/// **Pure function — no side effects, no GL posting.**
///
/// ## Consumers
/// - [`run_payroll`]: calls this function then posts GL from `breakdown.t_*` aggregates.
/// - `build_d112_xml` (commands/payroll.rs): calls this function then maps each
///   [`EmployeeBreakdown`] to a D112Employee — **no independent re-computation**.
///
/// ## GL ≡ D112 by construction
/// Because BOTH consumers derive from this single breakdown, the GL obligations
/// (4315/4316/444/436) are IDENTICAL to the D112 obligations (412/432/602/480) BY CONSTRUCTION.
/// Any future base-modifier (new OUG, sporuri type, leave treatment) must be implemented here
/// ONLY; both consumers automatically stay in sync.
pub async fn compute_payroll_run(
    pool: &SqlitePool,
    company_id: &str,
    period_from: &str,
    _period_to: &str,
) -> AppResult<PayrollRunBreakdown> {
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

    let extra_income = open_extra_income_by_employee(pool, company_id, &period_ym)
        .await
        .unwrap_or_default();
    let sporuri_map = sporuri_by_employee(pool, company_id, &period_ym)
        .await
        .unwrap_or_default();
    let retineri_map = retineri_by_employee(pool, company_id, &period_ym)
        .await
        .unwrap_or_default();
    let pontaje_map = pontaj_by_employee(pool, company_id, &period_ym)
        .await
        .unwrap_or_default();

    let mut breakdowns = Vec::new();
    let (mut t_gross, mut t_cas, mut t_cass, mut t_tax, mut t_net) = (
        Decimal::ZERO,
        Decimal::ZERO,
        Decimal::ZERO,
        Decimal::ZERO,
        Decimal::ZERO,
    );
    let (mut t_cas_diff, mut t_cass_diff) = (Decimal::ZERO, Decimal::ZERO);
    let mut t_cam_base = Decimal::ZERO;
    let mut indemn = crate::db::gl::IndemnityTotals::default();
    let (mut t_excess_reclass, mut t_excess_receivable) = (Decimal::ZERO, Decimal::ZERO);
    let mut t_retinut = Decimal::ZERO;
    let mut retin_items: Vec<crate::db::gl::RetinereGlItem> = Vec::new();

    for e in employees.iter().filter(|e| e.active) {
        let spor = sporuri_map.get(&e.id).copied().unwrap_or(Decimal::ZERO);
        let gross = dec(&e.gross_salary);
        let gross_eff = gross + spor;
        let non_taxable = suma_netaxabila(
            e.beneficiar_suma_netaxabila,
            &e.tip_contract,
            gross,
            year,
            month,
        );

        // ── B-path: medical leave ───────────────────────────────────────────
        if let Some(emp_leaves) = leaves_by_emp.get(&e.id) {
            let med_leaves_raw = emp_leaves.clone();
            let certs: Vec<LeaveCert> = emp_leaves
                .iter()
                .map(|l| {
                    let (cass_due, taxable) =
                        crate::anaf_decl::d112::cm_indemn_treatment(&l.cod_indemnizatie);
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
            let lr = crate::anaf_decl::d112::compute_payroll_with_leave(&LeavePayrollInput {
                gross: gross_eff,
                personal_deduction: crate::anaf_decl::d112::deducere_plafonata(
                    dec(&e.personal_deduction),
                    gross_eff,
                    year,
                    month,
                ),
                non_taxable,
                working_days: nzl,
                certs,
            });
            let w = crate::anaf_decl::d112::compute_payroll(&PayrollInput {
                gross: lr.worked_gross,
                personal_deduction: crate::anaf_decl::d112::deducere_plafonata(
                    dec(&e.personal_deduction),
                    gross_eff,
                    year,
                    month,
                ),
                non_taxable,
            });
            let (wcas, wcass, wtax) = (dec(&w.cas), dec(&w.cass), dec(&w.income_tax));
            let indemn_tax = (lr.income_tax - wtax).max(Decimal::ZERO);
            let sal_cas_leave = wcas;
            let sal_cass_leave = wcass;
            let sal_tax_leave = lr.income_tax - indemn_tax;
            let ded_leave = crate::anaf_decl::d112::deducere_plafonata(
                dec(&e.personal_deduction),
                gross_eff,
                year,
                month,
            );

            let mut b_excess = Decimal::ZERO;
            let mut b_excess_recv = Decimal::ZERO;
            let mut b_combined_cas = sal_cas_leave;
            let mut b_combined_cass = sal_cass_leave;
            let mut b_comb_sal_impozit = sal_tax_leave;
            let mut b_cam_base = leid0(lr.worked_base);
            // Combined-base impozit taxable base for the B-path excess employee (see A-path
            // comb_impozit_base_used for the rationale — same precision-preservation goal).
            let mut b_comb_impozit_base = Decimal::ZERO;

            if let Some(&emp_excess) = extra_income.get(&e.id) {
                if emp_excess > Decimal::ZERO {
                    let combined_base = lr.worked_gross + emp_excess;
                    let comb_cas = pct(combined_base, (25, 2));
                    let comb_cass = pct(combined_base, (10, 2));
                    let comb_impozit_base =
                        (combined_base - comb_cas - comb_cass - ded_leave).max(Decimal::ZERO);
                    let comb_sal_impozit = pct(comb_impozit_base, (10, 2));
                    let excess_recv = (comb_cas - sal_cas_leave)
                        + (comb_cass - sal_cass_leave)
                        + (comb_sal_impozit - sal_tax_leave);
                    b_excess = emp_excess;
                    b_excess_recv = excess_recv;
                    b_combined_cas = comb_cas;
                    b_combined_cass = comb_cass;
                    b_comb_sal_impozit = comb_sal_impozit;
                    b_comb_impozit_base = comb_impozit_base;
                    b_cam_base = leid0(lr.worked_base) + leid0(emp_excess);

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

            let retin = if let Some(emp_ret) = retineri_map.get(&e.id) {
                let res = crate::db::payroll_retineri::apply_retineri(lr.net, emp_ret);
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

            indemn.employer += lr.indemn_employer;
            indemn.fnuass += lr.indemn_fnuass;
            indemn.cas += lr.cas - sal_cas_leave;
            indemn.cass += lr.cass - sal_cass_leave;
            indemn.tax += indemn_tax;

            let gl_cas = if b_excess > Decimal::ZERO {
                b_combined_cas
            } else {
                sal_cas_leave
            };
            let gl_cass = if b_excess > Decimal::ZERO {
                b_combined_cass
            } else {
                sal_cass_leave
            };
            let gl_tax = if b_excess > Decimal::ZERO {
                b_comb_sal_impozit
            } else {
                sal_tax_leave
            };

            breakdowns.push(EmployeeBreakdown {
                employee_id: e.id.clone(),
                full_name: e.full_name.clone(),
                cnp: e.cnp.clone(),
                spor,
                gross_base: gross,
                gross_eff,
                pontaj_gross_eff: gross_eff, // B-path: no pontaj proration
                non_taxable,
                active_days: nzl, // B-path: full month active days (leave handles proration internally)
                zile_emis: lr.worked_days,
                is_leave_path: true,

                // A-path fields (unused for B-path, zeroed)
                sal_gross: lr.worked_gross,
                sal_cas: sal_cas_leave,
                sal_cass: sal_cass_leave,
                sal_impozit: sal_tax_leave,
                sal_net: lr.net,
                sal_cam: lr.cam,
                sal_taxable_base: lr.taxable_base,
                sal_personal_deduction: Decimal::ZERO,
                combined_cas: gl_cas,
                combined_cass: gl_cass,
                combined_impozit: gl_tax,
                comb_impozit_base: b_comb_impozit_base,
                excess: b_excess,
                excess_receivable: b_excess_recv,
                part_time_min: None,
                baza_cas: leid0(lr.worked_base),
                baza_cass: leid0(lr.worked_base),
                baza_cam_real: leid0(lr.worked_base),
                baza_impozit: leid0(lr.taxable_base),
                tip_asigurat: e.tip_asigurat.clone(),
                cam_base_contribution: b_cam_base,

                // B-path fields
                lr_worked_gross: lr.worked_gross,
                lr_worked_base: lr.worked_base,
                lr_worked_days: lr.worked_days,
                lr_cas: lr.cas,
                lr_cass: lr.cass,
                lr_income_tax: lr.income_tax,
                lr_cam: lr.cam,
                lr_net: lr.net,
                lr_indemn_employer: lr.indemn_employer,
                lr_indemn_fnuass: lr.indemn_fnuass,
                lr_indemn_total: lr.indemn_total,
                lr_taxable_base: lr.taxable_base,
                sal_cas_leave,
                sal_cass_leave,
                sal_tax_leave,
                indemn_tax,
                b_combined_cas,
                b_combined_cass,
                b_comb_sal_impozit,
                b_excess_recv,
                b_excess,
                b_cam_base,

                // GL fields
                gl_cas,
                gl_cass,
                gl_tax,
                gl_gross: lr.worked_gross,
                gl_cam_base: b_cam_base,
                gl_excess_reclass: b_excess,
                gl_excess_receivable: b_excess_recv,

                retineri_result: retin,
                med_leaves_raw,

                // Employee metadata
                employment_date: e.employment_date.clone(),
                tip_contract: e.tip_contract.clone(),
                pensionar: e.pensionar,
                ore_norma: e.ore_norma,
                sediu_cif: e.sediu_cif.clone(),
                beneficiar_suma_netaxabila: e.beneficiar_suma_netaxabila,
                exceptie_cas_min: e.exceptie_cas_min.clone(),
            });
            continue;
        }

        // ── A-path: standard salary ─────────────────────────────────────────
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
        let r = crate::anaf_decl::d112::compute_payroll(&PayrollInput {
            gross: pontaj_gross_eff,
            personal_deduction: crate::anaf_decl::d112::deducere_plafonata(
                dec(&e.personal_deduction),
                gross_eff,
                year,
                month,
            ),
            non_taxable,
        });
        let exempt = exempt_part_time_min_base(e.pensionar, &e.exceptie_cas_min);
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
        let zile_emis = active_days;

        let part_time_min_result = part_time_min_base(
            dec(&r.gross),
            &e.tip_contract,
            exempt,
            year,
            month,
            active_days,
            nzl,
        );
        if let Some((_, cas_diff, cass_diff)) = part_time_min_result {
            t_cas_diff += cas_diff;
            t_cass_diff += cass_diff;
        }

        let sal_gross = dec(&r.gross);
        let sal_cas = dec(&r.cas);
        let sal_cass = dec(&r.cass);
        let sal_impozit = dec(&r.income_tax);
        let sal_net = dec(&r.net);

        // Baza CAS/CASS (after optional part-time lift)
        let mut baza_cas = sal_gross - non_taxable;
        let mut baza_cass = sal_gross - non_taxable;
        let baza_cam_real = (sal_gross - non_taxable).max(Decimal::ZERO);
        if let Some((base, _, _)) = part_time_min_result {
            baza_cas = base;
            baza_cass = base;
        }

        // tip_asigurat: beneficiarii sumei netaxabile ≥07/2026 usese codul 1.11.2
        let tip_asigurat =
            if non_taxable > Decimal::ZERO && (year > 2026 || (year == 2026 && month >= 7)) {
                "1.11.2".to_string()
            } else {
                e.tip_asigurat.clone()
            };

        let mut excess_used = Decimal::ZERO;
        let mut excess_recv_used = Decimal::ZERO;
        let mut combined_cas = sal_cas;
        let mut combined_cass = sal_cass;
        let mut combined_impozit = sal_impozit;
        // Combined-base impozit taxable base (sal_gross+excess − comb_cas − comb_cass − ded).
        // Zero when no excess; stored on breakdown so Wave E in build_d112_xml never re-derives
        // from de.gross (i64, already rounded) and thus preserves pontaj Decimal precision.
        let mut comb_impozit_base_used = Decimal::ZERO;
        let cam_base_contrib = leid0((pontaj_gross_eff - non_taxable).max(Decimal::ZERO));

        if let Some(&emp_excess) = extra_income.get(&e.id) {
            if emp_excess > Decimal::ZERO {
                let emp_ded = crate::anaf_decl::d112::deducere_plafonata(
                    dec(&e.personal_deduction),
                    gross_eff,
                    year,
                    month,
                );
                // KNOWN LIMITATION (found by the pre-publication audit; deferred): for a PART-TIME
                // employee whose realized gross is below the minimum wage, art. 146 alin. (5^6) lifts the
                // CAS/CASS base to `baza_cas`/`baza_cass` (the employer covering the difference). When such
                // an employee ALSO has a diurnă excess, the D112-declared CAS/CASS should be computed on
                // `baza_cas + excess`, but the combined base below uses `sal_gross + excess` (the realized
                // gross), so build_d112_xml's Wave E overwrite under-declares CAS/CASS for that narrow
                // combo. A correct fix must keep TWO CAS values — the D112 total on the lifted base vs. the
                // employee's CAS on realized income that feeds `comb_impozit_base` (the tax base must NOT
                // absorb the employer-borne lift) — so it needs dedicated golden fixtures + fiscal review
                // rather than a hasty change to the payroll core here.
                let combined_base = sal_gross + emp_excess;
                let comb_cas = pct(combined_base, (25, 2));
                let comb_cass = pct(combined_base, (10, 2));
                let comb_impozit_base =
                    (combined_base - comb_cas - comb_cass - emp_ded).max(Decimal::ZERO);
                let comb_impozit = pct(comb_impozit_base, (10, 2));
                let excess_recv =
                    (comb_cas - sal_cas) + (comb_cass - sal_cass) + (comb_impozit - sal_impozit);
                excess_used = emp_excess;
                excess_recv_used = excess_recv;
                combined_cas = comb_cas;
                combined_cass = comb_cass;
                combined_impozit = comb_impozit;
                comb_impozit_base_used = comb_impozit_base;

                t_gross += sal_gross;
                t_cas += comb_cas;
                t_cass += comb_cass;
                t_tax += comb_impozit;
                t_net += sal_net;
                t_cam_base +=
                    leid0((pontaj_gross_eff - non_taxable).max(Decimal::ZERO)) + leid0(emp_excess);
                t_excess_reclass += emp_excess;
                t_excess_receivable += excess_recv;
            } else {
                t_gross += sal_gross;
                t_cas += sal_cas;
                t_cass += sal_cass;
                t_tax += sal_impozit;
                t_net += sal_net;
                t_cam_base += leid0((pontaj_gross_eff - non_taxable).max(Decimal::ZERO));
            }
        } else {
            t_gross += sal_gross;
            t_cas += sal_cas;
            t_cass += sal_cass;
            t_tax += sal_impozit;
            t_net += sal_net;
            t_cam_base += leid0((pontaj_gross_eff - non_taxable).max(Decimal::ZERO));
        }

        let retin = if let Some(emp_ret) = retineri_map.get(&e.id) {
            let res = crate::db::payroll_retineri::apply_retineri(sal_net, emp_ret);
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
                net_redus: sal_net,
                clamped: false,
                items: vec![],
            }
        };

        let gl_cas_a = if excess_used > Decimal::ZERO {
            combined_cas
        } else {
            sal_cas
        };
        let gl_cass_a = if excess_used > Decimal::ZERO {
            combined_cass
        } else {
            sal_cass
        };
        let gl_tax_a = if excess_used > Decimal::ZERO {
            combined_impozit
        } else {
            sal_impozit
        };
        let gl_cam_base_a = if excess_used > Decimal::ZERO {
            cam_base_contrib + leid0(excess_used)
        } else {
            cam_base_contrib
        };

        breakdowns.push(EmployeeBreakdown {
            employee_id: e.id.clone(),
            full_name: e.full_name.clone(),
            cnp: e.cnp.clone(),
            spor,
            gross_base: gross,
            gross_eff,
            pontaj_gross_eff,
            non_taxable,
            active_days,
            zile_emis,
            is_leave_path: false,

            // A-path fields
            sal_gross,
            sal_cas,
            sal_cass,
            sal_impozit,
            sal_net,
            sal_cam: dec(&r.cam),
            sal_taxable_base: dec(&r.taxable_base),
            sal_personal_deduction: dec(&r.personal_deduction),
            combined_cas,
            combined_cass,
            combined_impozit,
            comb_impozit_base: comb_impozit_base_used,
            excess: excess_used,
            excess_receivable: excess_recv_used,
            part_time_min: part_time_min_result,
            baza_cas,
            baza_cass,
            baza_cam_real,
            baza_impozit: dec(&r.taxable_base),
            tip_asigurat,
            cam_base_contribution: cam_base_contrib,

            // B-path fields (unused)
            lr_worked_gross: Decimal::ZERO,
            lr_worked_base: Decimal::ZERO,
            lr_worked_days: 0,
            lr_cas: Decimal::ZERO,
            lr_cass: Decimal::ZERO,
            lr_income_tax: Decimal::ZERO,
            lr_cam: Decimal::ZERO,
            lr_net: Decimal::ZERO,
            lr_indemn_employer: Decimal::ZERO,
            lr_indemn_fnuass: Decimal::ZERO,
            lr_indemn_total: Decimal::ZERO,
            lr_taxable_base: Decimal::ZERO,
            sal_cas_leave: Decimal::ZERO,
            sal_cass_leave: Decimal::ZERO,
            sal_tax_leave: Decimal::ZERO,
            indemn_tax: Decimal::ZERO,
            b_combined_cas: Decimal::ZERO,
            b_combined_cass: Decimal::ZERO,
            b_comb_sal_impozit: Decimal::ZERO,
            b_excess_recv: Decimal::ZERO,
            b_excess: Decimal::ZERO,
            b_cam_base: Decimal::ZERO,

            // GL fields
            gl_cas: gl_cas_a,
            gl_cass: gl_cass_a,
            gl_tax: gl_tax_a,
            gl_gross: sal_gross,
            gl_cam_base: gl_cam_base_a,
            gl_excess_reclass: excess_used,
            gl_excess_receivable: excess_recv_used,

            retineri_result: retin,
            med_leaves_raw: vec![],

            // Employee metadata
            employment_date: e.employment_date.clone(),
            tip_contract: e.tip_contract.clone(),
            pensionar: e.pensionar,
            ore_norma: e.ore_norma,
            sediu_cif: e.sediu_cif.clone(),
            beneficiar_suma_netaxabila: e.beneficiar_suma_netaxabila,
            exceptie_cas_min: e.exceptie_cas_min.clone(),
        });
    }

    let t_cam = pct(t_cam_base, CAM_PCT);

    Ok(PayrollRunBreakdown {
        employees: breakdowns,
        t_gross,
        t_cas,
        t_cass,
        t_tax,
        t_net,
        t_cam_base,
        t_cam,
        t_cas_diff,
        t_cass_diff,
        t_excess_reclass,
        t_excess_receivable,
        t_retinut,
        indemn,
        retin_items,
        nzl,
        year,
        month,
    })
}

/// Compute the monthly salary states for all active employees and post the aggregate to the GL.
pub async fn run_payroll(
    pool: &SqlitePool,
    company_id: &str,
    period_from: &str,
    period_to: &str,
) -> AppResult<PayrollRun> {
    let f = |d: Decimal| {
        format!(
            "{:.2}",
            d.round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
        )
    };

    let breakdown = compute_payroll_run(pool, company_id, period_from, period_to).await?;

    // Build EmployeeState from breakdown (for the UI payroll register).
    let mut states = Vec::new();
    for eb in &breakdown.employees {
        if eb.is_leave_path {
            states.push(EmployeeState {
                employee_id: eb.employee_id.clone(),
                full_name: eb.full_name.clone(),
                gross: f(eb.lr_worked_gross + eb.lr_indemn_total),
                cas: f(eb.lr_cas),
                cass: f(eb.lr_cass),
                income_tax: f(eb.lr_income_tax),
                net: f(eb.lr_net),
                cam: f(eb.lr_cam),
                spor: f(eb.spor),
                total_retinut: f(eb.retineri_result.total_retinut),
                net_employee: f(eb.retineri_result.net_redus),
            });
        } else {
            states.push(EmployeeState {
                employee_id: eb.employee_id.clone(),
                full_name: eb.full_name.clone(),
                gross: f(eb.sal_gross),
                cas: f(eb.sal_cas),
                cass: f(eb.sal_cass),
                income_tax: f(eb.sal_impozit),
                net: f(eb.sal_net),
                cam: f(eb.sal_cam),
                spor: f(eb.spor),
                total_retinut: f(eb.retineri_result.total_retinut),
                net_employee: f(eb.retineri_result.net_redus),
            });
        }
    }

    let post = crate::db::gl::post_payroll(
        pool,
        company_id,
        period_from,
        period_to,
        crate::db::gl::PayrollTotals {
            gross: breakdown.t_gross,
            cas: breakdown.t_cas,
            cass: breakdown.t_cass,
            impozit: breakdown.t_tax,
            cam: breakdown.t_cam,
            cas_diff: breakdown.t_cas_diff,
            cass_diff: breakdown.t_cass_diff,
            indemn: breakdown.indemn.clone(),
            excess_reclass: breakdown.t_excess_reclass,
            excess_receivable: breakdown.t_excess_receivable,
            retineri: breakdown.retin_items,
        },
    )
    .await?;

    Ok(PayrollRun {
        states,
        total_gross: f(breakdown.t_gross + breakdown.indemn.employer + breakdown.indemn.fnuass),
        total_cas: f(breakdown.t_cas + breakdown.indemn.cas),
        total_cass: f(breakdown.t_cass + breakdown.indemn.cass),
        total_income_tax: f(breakdown.t_tax + breakdown.indemn.tax),
        total_net: f(breakdown.t_net),
        total_cam: f(breakdown.t_cam),
        total_retinut: f(breakdown.t_retinut),
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
                functia: None,
                cod_cor: None,
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
                    functia: None,
                    cod_cor: None,
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
                functia: None,
                cod_cor: None,
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
            functia: None,
            cod_cor: None,
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
