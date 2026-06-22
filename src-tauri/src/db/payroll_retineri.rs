//! Rețineri/Popriri din salariu net (Codul muncii art.169 + Cod proc. civ.).
//!
//! Reținerea se aplică DUPĂ calcululul CAS/CASS/impozit (post-net).  Nu modifică baza
//! de contribuții sau D112 — doar împarte netul între angajat (5311) și terț (427/4282/462).
//!
//! Limite legale (art. 169 Codul muncii + art. 729 CPCP):
//!  - O singură reținere: max 1/3 din net;
//!  - Mai multe rețineri: Σ ≤ 1/2 din net;
//!  - Pensia alimentară are prioritate față de alte rețineri.
//!
//! GL: D 421 = C 427/4282/462 (rețineri) + D 421 = C 5311 (net redus).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

/// O reținere/poprire din salariu net.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Retinere {
    pub id: String,
    pub company_id: String,
    pub employee_id: String,
    pub period: String,
    pub amount: String,
    /// Tip: 'poprire' | 'pensie_alimentara' | 'avans' | 'sindicat' | 'alte'
    pub kind: String,
    /// Creditor (informativ, ex. "Tribunal Ilfov").
    pub creditor: String,
    /// Cont credit GL: '427' | '4282' | '462'
    pub account: String,
    /// Prioritate (1 = pensie alimentară, 2+ = altele).
    pub priority: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRetinereInput {
    pub company_id: String,
    pub employee_id: String,
    pub period: String,
    pub amount: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub creditor: Option<String>,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub priority: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRetinereInput {
    pub amount: Option<String>,
    pub kind: Option<String>,
    pub creditor: Option<String>,
    pub account: Option<String>,
    pub priority: Option<i64>,
}

/// Rezultatul aplicării reținerii la netul unui angajat: suma reținută efectiv + net redus.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetinereResult {
    /// Suma totală reținută (poate fi mai mică decât suma cerută dacă depășea plafonul legal).
    pub total_retinut: Decimal,
    /// Netul rămas de plată angajatului.
    pub net_redus: Decimal,
    /// True dacă vreuna dintre rețineri a fost plafonată (clamped) la limita legală.
    pub clamped: bool,
    /// Detaliu per reținere: (retinere_id, account, suma_efectiva).
    pub items: Vec<RetinereItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetinereItem {
    pub id: String,
    pub account: String,
    pub suma_efectiva: Decimal,
}

const COLS: &str = "id, company_id, employee_id, period, amount, kind, creditor, account, \
                    priority, created_at, updated_at";

/// Validare sumă reținere: strict pozitivă.
fn parse_amount(s: &str) -> AppResult<String> {
    let t = s.trim();
    if t.contains('e') || t.contains('E') {
        return Err(AppError::Validation(
            "Suma reținere invalidă — folosiți formatul 1234.56 (fără notație științifică).".into(),
        ));
    }
    let d = Decimal::from_str(t).map_err(|_| {
        AppError::Validation("Suma reținere invalidă — folosiți formatul 1234.56.".into())
    })?;
    if d <= Decimal::ZERO {
        return Err(AppError::Validation(
            "Suma reținere trebuie să fie strict pozitivă.".into(),
        ));
    }
    Ok(d.to_string())
}

/// Validare cont GL: trebuie să fie '427', '4282' sau '462'.
fn parse_account(s: &str) -> AppResult<String> {
    let t = s.trim();
    match t {
        "427" | "4282" | "462" => Ok(t.to_string()),
        _ => Err(AppError::Validation(
            "Contul reținerii trebuie să fie 427, 4282 sau 462.".into(),
        )),
    }
}

pub async fn list(pool: &SqlitePool, company_id: &str, period: &str) -> AppResult<Vec<Retinere>> {
    let q = format!(
        "SELECT {COLS} FROM payroll_retineri \
         WHERE company_id=?1 AND period=?2 ORDER BY employee_id, priority, created_at"
    );
    Ok(sqlx::query_as::<_, Retinere>(&q)
        .bind(company_id)
        .bind(period)
        .fetch_all(pool)
        .await?)
}

pub async fn get(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<Retinere> {
    let q = format!("SELECT {COLS} FROM payroll_retineri WHERE id=?1 AND company_id=?2");
    sqlx::query_as::<_, Retinere>(&q)
        .bind(id)
        .bind(company_id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn create(pool: &SqlitePool, input: CreateRetinereInput) -> AppResult<Retinere> {
    if input.period.len() != 7 {
        return Err(AppError::Validation(
            "Perioada reținere trebuie să fie în format YYYY-MM.".into(),
        ));
    }
    let amount = parse_amount(&input.amount)?;
    let kind = input.kind.as_deref().unwrap_or("alte").trim().to_string();
    let creditor = input.creditor.as_deref().unwrap_or("").trim().to_string();
    let account = parse_account(input.account.as_deref().unwrap_or("427"))?;
    // pensie alimentară auto-priority 1 when kind is set appropriately
    let priority = input
        .priority
        .unwrap_or(if kind == "pensie_alimentara" { 1 } else { 2 });

    let id = new_id();
    let now = now_unix();
    sqlx::query(
        "INSERT INTO payroll_retineri \
         (id, company_id, employee_id, period, amount, kind, creditor, account, priority, \
          created_at, updated_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10)",
    )
    .bind(&id)
    .bind(&input.company_id)
    .bind(&input.employee_id)
    .bind(&input.period)
    .bind(&amount)
    .bind(&kind)
    .bind(&creditor)
    .bind(&account)
    .bind(priority)
    .bind(now)
    .execute(pool)
    .await?;
    get(pool, &id, &input.company_id).await
}

pub async fn update(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    input: UpdateRetinereInput,
) -> AppResult<Retinere> {
    let cur = get(pool, id, company_id).await?;
    let amount = match input.amount.as_deref() {
        Some(s) => parse_amount(s)?,
        None => cur.amount.clone(),
    };
    let kind = input
        .kind
        .as_deref()
        .unwrap_or(&cur.kind)
        .trim()
        .to_string();
    let creditor = input
        .creditor
        .as_deref()
        .unwrap_or(&cur.creditor)
        .trim()
        .to_string();
    let account = match input.account.as_deref() {
        Some(s) => parse_account(s)?,
        None => cur.account.clone(),
    };
    let priority = input.priority.unwrap_or(cur.priority);

    sqlx::query(
        "UPDATE payroll_retineri SET amount=?3, kind=?4, creditor=?5, account=?6, \
         priority=?7, updated_at=?8 WHERE id=?1 AND company_id=?2",
    )
    .bind(id)
    .bind(company_id)
    .bind(&amount)
    .bind(&kind)
    .bind(&creditor)
    .bind(&account)
    .bind(priority)
    .bind(now_unix())
    .execute(pool)
    .await?;
    get(pool, id, company_id).await
}

pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    get(pool, id, company_id).await?;
    sqlx::query("DELETE FROM payroll_retineri WHERE id=?1 AND company_id=?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Rețineri per angajat pentru o perioadă — returnează map employee_id → Vec<Retinere>,
/// sortat după (priority ASC, created_at ASC) pentru aplicarea în ordinea legală.
pub async fn retineri_by_employee(
    pool: &SqlitePool,
    company_id: &str,
    period: &str,
) -> AppResult<std::collections::HashMap<String, Vec<Retinere>>> {
    let rows: Vec<Retinere> = sqlx::query_as::<_, Retinere>(&format!(
        "SELECT {COLS} FROM payroll_retineri \
             WHERE company_id=?1 AND period=?2 ORDER BY employee_id, priority, created_at"
    ))
    .bind(company_id)
    .bind(period)
    .fetch_all(pool)
    .await?;

    let mut map: std::collections::HashMap<String, Vec<Retinere>> =
        std::collections::HashMap::new();
    for r in rows {
        map.entry(r.employee_id.clone()).or_default().push(r);
    }
    Ok(map)
}

/// Aplică rețineri la netul unui angajat, respectând limitele legale (art. 169 CM).
///
/// **Algoritm**:
/// 1. Sortare după prioritate (pensie alimentară primul).
/// 2. Plafon individual per reținere: max 1/3 din `net`.
/// 3. Plafon cumulativ: Σ ≤ 1/2 din `net`.
/// 4. Rețineri depășind plafonul cumulativ → sumate până la 1/2 net, excesul ignorat.
///
/// Fiecare reținere este clamped la `min(suma_ceruta, 1/3_net, rest_pana_la_1/2_net)`.
/// Se returnează detaliul per reținere, suma totală și netul redus.
pub fn apply_retineri(net: Decimal, retineri: &[Retinere]) -> RetinereResult {
    // Legal ceilings (MidpointAwayFromZero la 2 zecimale — lei cu bani).
    let one_third = (net / Decimal::from(3))
        .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
    let half = (net / Decimal::from(2))
        .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);

    let mut total = Decimal::ZERO;
    let mut clamped = false;
    let mut items = Vec::new();

    for r in retineri {
        let cerut = Decimal::from_str(&r.amount).unwrap_or(Decimal::ZERO);
        let rest_pana_la_half = (half - total).max(Decimal::ZERO);
        // Individual cap: min(cerut, 1/3 net, rest until 1/2 cumulated)
        let efectiv = cerut.min(one_third).min(rest_pana_la_half);
        if efectiv < cerut {
            clamped = true;
        }
        if efectiv.is_zero() {
            // No more room (cumulative cap reached)
            continue;
        }
        total += efectiv;
        items.push(RetinereItem {
            id: r.id.clone(),
            account: r.account.clone(),
            suma_efectiva: efectiv,
        });
    }

    RetinereResult {
        total_retinut: total,
        net_redus: net - total,
        clamped,
        items,
    }
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
             VALUES ('emp1','co1','1900101410011','Ion',\
             '4000','0',1,'1',0,'N',8,'','',0,1,1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    fn make_retinere(id: &str, amount: &str, account: &str, priority: i64) -> Retinere {
        Retinere {
            id: id.to_string(),
            company_id: "co1".to_string(),
            employee_id: "emp1".to_string(),
            period: "2026-06".to_string(),
            amount: amount.to_string(),
            kind: "poprire".to_string(),
            creditor: "Tribunal".to_string(),
            account: account.to_string(),
            priority,
            created_at: 1,
            updated_at: 1,
        }
    }

    // ── apply_retineri cap logic ─────────────────────────────────────────────

    /// Net 3000, o poprire de 800: sub 1/3 (1000) → trece integral.
    #[test]
    fn retinere_within_cap_passes_intact() {
        let net = Decimal::from(3000);
        let r = vec![make_retinere("r1", "800", "427", 2)];
        let res = apply_retineri(net, &r);
        assert_eq!(res.total_retinut, Decimal::from(800));
        assert_eq!(res.net_redus, Decimal::from(2200));
        assert!(!res.clamped);
        assert_eq!(res.items.len(), 1);
    }

    /// Net 3000, o poprire de 1500 (> 1/3 = 1000) → clamped la 1000.
    #[test]
    fn retinere_exceeds_one_third_clamped() {
        let net = Decimal::from(3000);
        let r = vec![make_retinere("r1", "1500", "427", 2)];
        let res = apply_retineri(net, &r);
        assert_eq!(res.total_retinut, Decimal::from(1000));
        assert_eq!(res.net_redus, Decimal::from(2000));
        assert!(res.clamped);
    }

    /// Net 3000, două popriri de 800: 800+800=1600 > 1/2=1500 → a doua clamped la 700.
    #[test]
    fn cumulative_cap_half_net() {
        let net = Decimal::from(3000);
        let r = vec![
            make_retinere("r1", "800", "427", 2),
            make_retinere("r2", "800", "462", 2),
        ];
        let res = apply_retineri(net, &r);
        assert_eq!(res.total_retinut, Decimal::from(1500)); // capped at 1/2
        assert_eq!(res.net_redus, Decimal::from(1500));
        assert!(res.clamped);
        assert_eq!(res.items.len(), 2);
        // r1 passes intact (800 ≤ 1000), r2 clamped to 700 (1500 − 800 = 700)
        assert_eq!(res.items[0].suma_efectiva, Decimal::from(800));
        assert_eq!(res.items[1].suma_efectiva, Decimal::from(700));
    }

    /// Net 3000, pensie alimentară (priority 1) + poprire (priority 2): pensie trece primul.
    #[test]
    fn pensie_alimentara_priority() {
        let net = Decimal::from(3000);
        // Pension = 900, poprire = 700; sorted by priority: pensie first.
        let mut r = vec![
            make_retinere("r2", "700", "427", 2), // poprire
            make_retinere("r1", "900", "427", 1), // pensie alimentară
        ];
        // Sort by priority (as the engine does when loading from DB ORDER BY priority)
        r.sort_by_key(|x| x.priority);
        let res = apply_retineri(net, &r);
        assert_eq!(res.total_retinut, Decimal::from(1500)); // 900+700=1600 > 1500 → cap
                                                            // pensie first: 900; poprire: min(700, 1000, 600) = 600
        assert_eq!(res.items[0].suma_efectiva, Decimal::from(900)); // pensie
        assert_eq!(res.items[1].suma_efectiva, Decimal::from(600)); // clamped poprire
        assert!(res.clamped);
    }

    /// Net 3000, reținere = 0 → respinsă la creare; apply_retineri cu vectă goală → nul.
    #[test]
    fn empty_retineri_zero() {
        let res = apply_retineri(Decimal::from(3000), &[]);
        assert!(res.total_retinut.is_zero());
        assert_eq!(res.net_redus, Decimal::from(3000));
        assert!(!res.clamped);
    }

    // ── DB CRUD ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn retinere_crud() {
        let pool = setup().await;
        let r = create(
            &pool,
            CreateRetinereInput {
                company_id: "co1".into(),
                employee_id: "emp1".into(),
                period: "2026-06".into(),
                amount: "800".into(),
                kind: Some("poprire".into()),
                creditor: Some("Tribunal Ilfov".into()),
                account: Some("427".into()),
                priority: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(r.amount, "800");
        assert_eq!(r.account, "427");
        assert_eq!(r.priority, 2); // default for non-pensie

        // List
        let lst = list(&pool, "co1", "2026-06").await.unwrap();
        assert_eq!(lst.len(), 1);

        // Update
        let upd = update(
            &pool,
            &r.id,
            "co1",
            UpdateRetinereInput {
                amount: Some("500".into()),
                kind: None,
                creditor: None,
                account: None,
                priority: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(upd.amount, "500");

        // Delete
        delete(&pool, &r.id, "co1").await.unwrap();
        let lst2 = list(&pool, "co1", "2026-06").await.unwrap();
        assert!(lst2.is_empty());
    }

    #[tokio::test]
    async fn retinere_invalid_account_rejected() {
        let pool = setup().await;
        let bad = create(
            &pool,
            CreateRetinereInput {
                company_id: "co1".into(),
                employee_id: "emp1".into(),
                period: "2026-06".into(),
                amount: "100".into(),
                kind: None,
                creditor: None,
                account: Some("421".into()), // wrong — 421 is salary payable
                priority: None,
            },
        )
        .await;
        assert!(bad.is_err(), "account 421 should be rejected");
    }

    #[tokio::test]
    async fn retinere_zero_amount_rejected() {
        let pool = setup().await;
        let bad = create(
            &pool,
            CreateRetinereInput {
                company_id: "co1".into(),
                employee_id: "emp1".into(),
                period: "2026-06".into(),
                amount: "0".into(),
                kind: None,
                creditor: None,
                account: None,
                priority: None,
            },
        )
        .await;
        assert!(bad.is_err(), "zero amount should be rejected");
    }

    #[tokio::test]
    async fn pensie_alimentara_default_priority_one() {
        let pool = setup().await;
        let r = create(
            &pool,
            CreateRetinereInput {
                company_id: "co1".into(),
                employee_id: "emp1".into(),
                period: "2026-06".into(),
                amount: "300".into(),
                kind: Some("pensie_alimentara".into()),
                creditor: None,
                account: None,
                priority: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(
            r.priority, 1,
            "pensie alimentară should default to priority 1"
        );
    }
}
