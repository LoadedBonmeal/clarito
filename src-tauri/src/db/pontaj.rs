//! Condică de prezență (pontaj) — CM art. 119.
//!
//! Un pontaj per angajat per lună (UNIQUE company_id + employee_id + period).
//! `worked_days` suprascrie baza calendaristică în `run_payroll` când nu există concediu medical.
//! `absence_days` și `leave_days` sunt câmpuri informative (condica de prezență) — nu se scad
//! dublu din `worked_days` (angajatorul furnizează deja cifra finală în `worked_days`).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

/// Un rând de pontaj lunar per angajat.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Pontaj {
    pub id: String,
    pub company_id: String,
    pub employee_id: String,
    /// Format YYYY-MM (ex. "2026-06").
    pub period: String,
    pub worked_days: i64,
    /// Ore suplimentare (Decimal ca TEXT; poate fi fracționar, ex. "1.5").
    pub overtime_hours: String,
    /// Ore noapte (Decimal ca TEXT).
    pub night_hours: String,
    pub absence_days: i64,
    pub leave_days: i64,
    pub notes: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePontajInput {
    pub company_id: String,
    pub employee_id: String,
    pub period: String,
    pub worked_days: i64,
    #[serde(default)]
    pub overtime_hours: Option<String>,
    #[serde(default)]
    pub night_hours: Option<String>,
    #[serde(default)]
    pub absence_days: Option<i64>,
    #[serde(default)]
    pub leave_days: Option<i64>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePontajInput {
    pub worked_days: Option<i64>,
    pub overtime_hours: Option<String>,
    pub night_hours: Option<String>,
    pub absence_days: Option<i64>,
    pub leave_days: Option<i64>,
    pub notes: Option<String>,
}

const COLS: &str = "id, company_id, employee_id, period, worked_days, overtime_hours, \
                    night_hours, absence_days, leave_days, notes, created_at, updated_at";

/// Validare ore (Decimal ≥ 0).
fn parse_hours(s: &str) -> AppResult<String> {
    let t = s.trim();
    if t.contains('e') || t.contains('E') {
        return Err(AppError::Validation(
            "Ore invalide — folosiți formatul 1.5 (fără notație științifică).".into(),
        ));
    }
    let d = Decimal::from_str(t).map_err(|_| {
        AppError::Validation("Ore invalide — folosiți formatul numeric (ex. 1.5).".into())
    })?;
    if d < Decimal::ZERO {
        return Err(AppError::Validation("Orele nu pot fi negative.".into()));
    }
    Ok(d.to_string())
}

fn validate_days(label: &str, v: i64) -> AppResult<()> {
    if v < 0 {
        return Err(AppError::Validation(format!(
            "{label} nu poate fi negativ."
        )));
    }
    Ok(())
}

pub async fn list(pool: &SqlitePool, company_id: &str, period: &str) -> AppResult<Vec<Pontaj>> {
    let q = format!(
        "SELECT {COLS} FROM pontaje \
         WHERE company_id=?1 AND period=?2 ORDER BY employee_id, created_at"
    );
    Ok(sqlx::query_as::<_, Pontaj>(&q)
        .bind(company_id)
        .bind(period)
        .fetch_all(pool)
        .await?)
}

pub async fn get(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<Pontaj> {
    let q = format!("SELECT {COLS} FROM pontaje WHERE id=?1 AND company_id=?2");
    sqlx::query_as::<_, Pontaj>(&q)
        .bind(id)
        .bind(company_id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn create(pool: &SqlitePool, input: CreatePontajInput) -> AppResult<Pontaj> {
    if input.period.len() != 7 {
        return Err(AppError::Validation(
            "Perioada trebuie să fie în format YYYY-MM.".into(),
        ));
    }
    validate_days("Zile lucrate", input.worked_days)?;
    let absence = input.absence_days.unwrap_or(0);
    let leave = input.leave_days.unwrap_or(0);
    validate_days("Zile absență", absence)?;
    validate_days("Zile concediu", leave)?;
    let overtime = parse_hours(input.overtime_hours.as_deref().unwrap_or("0"))?;
    let night = parse_hours(input.night_hours.as_deref().unwrap_or("0"))?;
    let notes = input.notes.as_deref().unwrap_or("").trim().to_string();

    let id = new_id();
    let now = now_unix();
    sqlx::query(
        "INSERT INTO pontaje \
         (id, company_id, employee_id, period, worked_days, overtime_hours, night_hours, \
          absence_days, leave_days, notes, created_at, updated_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?11)",
    )
    .bind(&id)
    .bind(&input.company_id)
    .bind(&input.employee_id)
    .bind(&input.period)
    .bind(input.worked_days)
    .bind(&overtime)
    .bind(&night)
    .bind(absence)
    .bind(leave)
    .bind(&notes)
    .bind(now)
    .execute(pool)
    .await?;
    get(pool, &id, &input.company_id).await
}

pub async fn update(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    input: UpdatePontajInput,
) -> AppResult<Pontaj> {
    let cur = get(pool, id, company_id).await?;
    let worked_days = input.worked_days.unwrap_or(cur.worked_days);
    validate_days("Zile lucrate", worked_days)?;
    let absence = input.absence_days.unwrap_or(cur.absence_days);
    let leave = input.leave_days.unwrap_or(cur.leave_days);
    validate_days("Zile absență", absence)?;
    validate_days("Zile concediu", leave)?;
    let overtime = match input.overtime_hours.as_deref() {
        Some(s) => parse_hours(s)?,
        None => cur.overtime_hours.clone(),
    };
    let night = match input.night_hours.as_deref() {
        Some(s) => parse_hours(s)?,
        None => cur.night_hours.clone(),
    };
    let notes = input
        .notes
        .as_deref()
        .unwrap_or(&cur.notes)
        .trim()
        .to_string();

    sqlx::query(
        "UPDATE pontaje SET worked_days=?3, overtime_hours=?4, night_hours=?5, \
         absence_days=?6, leave_days=?7, notes=?8, updated_at=?9 \
         WHERE id=?1 AND company_id=?2",
    )
    .bind(id)
    .bind(company_id)
    .bind(worked_days)
    .bind(&overtime)
    .bind(&night)
    .bind(absence)
    .bind(leave)
    .bind(&notes)
    .bind(now_unix())
    .execute(pool)
    .await?;
    get(pool, id, company_id).await
}

pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    get(pool, id, company_id).await?;
    sqlx::query("DELETE FROM pontaje WHERE id=?1 AND company_id=?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Pontaje per angajat pentru o perioadă — returnează map employee_id → Pontaj.
/// Folosit de `run_payroll` pentru a prelua `worked_days` ca bază de proratare.
pub async fn pontaj_by_employee(
    pool: &SqlitePool,
    company_id: &str,
    period: &str,
) -> AppResult<std::collections::HashMap<String, Pontaj>> {
    let rows: Vec<Pontaj> = sqlx::query_as::<_, Pontaj>(&format!(
        "SELECT {COLS} FROM pontaje \
             WHERE company_id=?1 AND period=?2"
    ))
    .bind(company_id)
    .bind(period)
    .fetch_all(pool)
    .await?;

    let mut map = std::collections::HashMap::new();
    for p in rows {
        map.insert(p.employee_id.clone(), p);
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        sqlx::query(
            "INSERT INTO employees \
             (id,company_id,cnp,full_name,gross_salary,personal_deduction,\
             active,tip_asigurat,pensionar,tip_contract,ore_norma,\
             exceptie_cas_min,sediu_cif,beneficiar_suma_netaxabila,created_at,updated_at) \
             VALUES ('emp1','co1','1900101410011','Ion Popescu',\
             '5000','0',1,'1',0,'N',8,'','',0,1,1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    // ── CRUD + UNIQUE constraint ──────────────────────────────────────────────

    #[tokio::test]
    async fn pontaj_crud_unique_constraint() {
        let pool = setup().await;

        // Create
        let p = create(
            &pool,
            CreatePontajInput {
                company_id: "co1".into(),
                employee_id: "emp1".into(),
                period: "2026-06".into(),
                worked_days: 20,
                overtime_hours: Some("2.5".into()),
                night_hours: None,
                absence_days: Some(1),
                leave_days: None,
                notes: Some("OK".into()),
            },
        )
        .await
        .unwrap();
        assert_eq!(p.worked_days, 20);
        assert_eq!(p.overtime_hours, "2.5");
        assert_eq!(p.absence_days, 1);
        assert_eq!(p.notes, "OK");

        // List
        let lst = list(&pool, "co1", "2026-06").await.unwrap();
        assert_eq!(lst.len(), 1);

        // Update
        let upd = update(
            &pool,
            &p.id,
            "co1",
            UpdatePontajInput {
                worked_days: Some(18),
                overtime_hours: None,
                night_hours: None,
                absence_days: None,
                leave_days: None,
                notes: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(upd.worked_days, 18);

        // Duplicate period → UNIQUE violation
        let dup = create(
            &pool,
            CreatePontajInput {
                company_id: "co1".into(),
                employee_id: "emp1".into(),
                period: "2026-06".into(),
                worked_days: 5,
                overtime_hours: None,
                night_hours: None,
                absence_days: None,
                leave_days: None,
                notes: None,
            },
        )
        .await;
        assert!(
            dup.is_err(),
            "UNIQUE(company,employee,period) must be enforced"
        );

        // Delete
        delete(&pool, &p.id, "co1").await.unwrap();
        let lst2 = list(&pool, "co1", "2026-06").await.unwrap();
        assert!(lst2.is_empty());
    }

    // ── Validation ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn pontaj_negative_worked_days_rejected() {
        let pool = setup().await;
        let bad = create(
            &pool,
            CreatePontajInput {
                company_id: "co1".into(),
                employee_id: "emp1".into(),
                period: "2026-06".into(),
                worked_days: -1,
                overtime_hours: None,
                night_hours: None,
                absence_days: None,
                leave_days: None,
                notes: None,
            },
        )
        .await;
        assert!(bad.is_err(), "negative worked_days must be rejected");
    }

    #[tokio::test]
    async fn pontaj_negative_hours_rejected() {
        let pool = setup().await;
        let bad = create(
            &pool,
            CreatePontajInput {
                company_id: "co1".into(),
                employee_id: "emp1".into(),
                period: "2026-06".into(),
                worked_days: 20,
                overtime_hours: Some("-1".into()),
                night_hours: None,
                absence_days: None,
                leave_days: None,
                notes: None,
            },
        )
        .await;
        assert!(bad.is_err(), "negative overtime_hours must be rejected");
    }

    // ── pontaj_by_employee helper ────────────────────────────────────────────

    #[tokio::test]
    async fn pontaj_by_employee_returns_map() {
        let pool = setup().await;
        create(
            &pool,
            CreatePontajInput {
                company_id: "co1".into(),
                employee_id: "emp1".into(),
                period: "2026-06".into(),
                worked_days: 15,
                overtime_hours: None,
                night_hours: None,
                absence_days: None,
                leave_days: None,
                notes: None,
            },
        )
        .await
        .unwrap();
        let map = pontaj_by_employee(&pool, "co1", "2026-06").await.unwrap();
        assert_eq!(map.len(), 1);
        assert_eq!(map["emp1"].worked_days, 15);

        // Different period → empty
        let map2 = pontaj_by_employee(&pool, "co1", "2026-07").await.unwrap();
        assert!(map2.is_empty());
    }

    // ── run_payroll integration tests ────────────────────────────────────────
    // These tests verify that:
    // 1. No pontaj → byte-identical payroll behavior
    // 2. Pontaj fewer days → prorates gross
    // 3. Pontaj with worked_days > nzl → clamped to nzl (same as no pontaj)
    // 4. Absence days are metadata (no double-subtraction from worked_days)

    #[tokio::test]
    async fn run_payroll_no_pontaj_is_byte_identical() {
        // Full-time employee, gross 5000, no pontaj — verify golden values.
        // June 2026: working_days = 21.
        // CAS = ROUND(5000 * 25%) = 1250
        // CASS = ROUND(5000 * 10%) = 500
        // impozit_base = 5000 - 1250 - 500 - 0 (no deduction, no non-taxable) = 3250
        // impozit = ROUND(3250 * 10%) = 325
        // net = 5000 - 1250 - 500 - 325 = 2925
        let pool = setup().await;

        let run = crate::db::payroll::run_payroll(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();

        assert_eq!(run.states.len(), 1);
        let s = &run.states[0];
        assert_eq!(s.gross, "5000.00", "gross should be 5000 (no pontaj)");
        assert_eq!(s.cas, "1250.00", "CAS 25% of 5000");
        assert_eq!(s.cass, "500.00", "CASS 10% of 5000");
        assert_eq!(s.income_tax, "325.00", "impozit 10% of 3250");
        assert_eq!(s.net, "2925.00", "net = 5000 - 1250 - 500 - 325");
    }

    #[tokio::test]
    async fn run_payroll_with_pontaj_fewer_worked_days_prorates_gross() {
        // Full-time employee gross 5000, June 2026 (21 working days).
        // Pontaj: 15 worked days → prorated gross = 5000 * 15 / 21 = 3571.43 (rounded to 2dp)
        // CAS = ROUND(3571.43 * 25%) = 892.86 rounded to lei = 893
        // We just verify gross changes and cascade is consistent.
        let pool = setup().await;

        create(
            &pool,
            CreatePontajInput {
                company_id: "co1".into(),
                employee_id: "emp1".into(),
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

        let run = crate::db::payroll::run_payroll(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();

        assert_eq!(run.states.len(), 1);
        let s = &run.states[0];
        // Gross should NOT be 5000 anymore; it's prorated on 15/21 days.
        let gross: rust_decimal::Decimal = rust_decimal::Decimal::from_str(&s.gross).unwrap();
        let expected_gross = rust_decimal::Decimal::from_str("5000").unwrap()
            * rust_decimal::Decimal::from(15)
            / rust_decimal::Decimal::from(21);
        // Allow ±1 lei tolerance for rounding
        let diff = (gross - expected_gross).abs();
        assert!(
            diff <= rust_decimal::Decimal::from(1),
            "prorated gross should be ~{expected_gross} but got {gross}"
        );
        // Net < 2925 (the no-pontaj baseline)
        let net: rust_decimal::Decimal = rust_decimal::Decimal::from_str(&s.net).unwrap();
        assert!(
            net < rust_decimal::Decimal::from(2925),
            "net must be prorated below 2925"
        );
    }

    #[tokio::test]
    async fn pontaj_worked_days_bounded_by_calendar_nzl() {
        // pontaj.worked_days = 25 in a 21-working-day month → clamped to 21 → same as no pontaj.
        let pool = setup().await;

        create(
            &pool,
            CreatePontajInput {
                company_id: "co1".into(),
                employee_id: "emp1".into(),
                period: "2026-06".into(),
                worked_days: 25, // exceeds nzl=21
                overtime_hours: None,
                night_hours: None,
                absence_days: None,
                leave_days: None,
                notes: None,
            },
        )
        .await
        .unwrap();

        let run = crate::db::payroll::run_payroll(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();

        let s = &run.states[0];
        // gross_eff clamped to 21/21 = full gross (no proration)
        assert_eq!(
            s.gross, "5000.00",
            "worked_days > nzl should clamp to nzl → no proration"
        );
        assert_eq!(
            s.net, "2925.00",
            "net must be identical to no-pontaj baseline when worked_days ≥ nzl"
        );
    }

    #[tokio::test]
    async fn pontaj_absence_days_do_not_double_count_leaves() {
        // absence_days is metadata; payroll uses worked_days only.
        // With worked_days=20 and absence_days=1, result should equal worked_days=20 with absence_days=0.
        let pool = setup().await;

        let pj = create(
            &pool,
            CreatePontajInput {
                company_id: "co1".into(),
                employee_id: "emp1".into(),
                period: "2026-06".into(),
                worked_days: 20,
                overtime_hours: None,
                night_hours: None,
                absence_days: Some(1), // metadata only
                leave_days: Some(0),
                notes: None,
            },
        )
        .await
        .unwrap();

        let run1 = crate::db::payroll::run_payroll(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();

        // Update to absence_days=0 (metadata change), keep worked_days=20
        update(
            &pool,
            &pj.id,
            "co1",
            UpdatePontajInput {
                worked_days: None,
                overtime_hours: None,
                night_hours: None,
                absence_days: Some(0),
                leave_days: None,
                notes: None,
            },
        )
        .await
        .unwrap();

        // Run again — result should be same because worked_days hasn't changed
        let run2 = crate::db::payroll::run_payroll(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();

        // Both runs have same worked_days=20 → same gross/net
        assert_eq!(
            run1.states[0].gross, run2.states[0].gross,
            "absence_days metadata must not affect gross"
        );
        assert_eq!(
            run1.states[0].net, run2.states[0].net,
            "absence_days metadata must not affect net"
        );
    }
}
