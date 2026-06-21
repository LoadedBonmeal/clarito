//! Cecuri & Bilete la ordin — registru instrumente de plată.
//!
//! Monografie contabilă (OMFP 1802/2014 + Legea 58/1934 + Legea 59/1934):
//!
//! ## CEC primit (direction=received)
//!   - primire (registered):   D 413  = C 4111
//!   - depunere la bancă:      D 5112 = C 413
//!   - încasare efectivă:      D 5121 = C 5112
//!   - refuz (dishonor):       D 4111 = C 5112
//!
//! ## BO primit (direction=received)
//!   - primire (registered):   D 413  = C 4111
//!   - depunere la bancă:      D 5113 = C 413
//!   - încasare efectivă:      D 5121 = C 5113
//!   - scontare (discount BO): D 5114 = C 413  →  D 5121(net) + D 667(scont) [+ D 627(comision)] = C 5114
//!   - refuz (dishonor):       D 4111 = C 5113
//!
//! ## Instrument emis (direction=issued, payer)
//!   - acceptare (registered): D 401  = C 403
//!   - plată la scadență:      D 403  = C 5121
//!
//! GL este idempotent per (company_id, source_type='PAYMENT_INSTRUMENT', source_id).
//! `generate_gl_entries` NU atinge source_type='PAYMENT_INSTRUMENT' → notele persistă la regenerare.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

use crate::db::gl::{post_manual_journal, ManualJournal};
use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

// ─── Domain structs ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PaymentInstrument {
    pub id: String,
    pub company_id: String,
    /// "CEC" | "BO"
    pub kind: String,
    /// "received" | "issued"
    pub direction: String,
    pub partner_id: Option<String>,
    pub partner_cui: Option<String>,
    pub number: Option<String>,
    pub amount: String,
    pub currency: String,
    pub issue_date: String,
    /// Obligatoriu pentru BO (Legea 59/1934); NULL pentru CEC (plătibil la vedere).
    pub scadenta: Option<String>,
    /// registered → deposited → collected/dishonored (received)
    /// registered → deposited → paid (issued)
    /// deposited  → discounted (BO received only)
    pub status: String,
    /// Suma scontului (doar BO scontat, contul 667).
    pub discount_amount: Option<String>,
    /// Comision bancar (627), opțional.
    pub commission_amount: Option<String>,
    pub notes: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePaymentInstrumentInput {
    pub company_id: String,
    pub kind: String,
    pub direction: String,
    pub partner_id: Option<String>,
    pub partner_cui: Option<String>,
    pub number: Option<String>,
    pub amount: String,
    pub currency: Option<String>,
    pub issue_date: String,
    pub scadenta: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePaymentInstrumentInput {
    pub partner_id: Option<String>,
    pub partner_cui: Option<String>,
    pub number: Option<String>,
    pub amount: String,
    pub currency: Option<String>,
    pub issue_date: String,
    pub scadenta: Option<String>,
    pub notes: Option<String>,
}

// ─── Validation ──────────────────────────────────────────────────────────────

fn validate_kind(kind: &str) -> AppResult<()> {
    match kind {
        "CEC" | "BO" => Ok(()),
        _ => Err(AppError::Validation(format!(
            "Tip instrument invalid: '{kind}'. Valori acceptate: CEC, BO."
        ))),
    }
}

fn validate_direction(dir: &str) -> AppResult<()> {
    match dir {
        "received" | "issued" => Ok(()),
        _ => Err(AppError::Validation(format!(
            "Direcție invalidă: '{dir}'. Valori acceptate: received, issued."
        ))),
    }
}

/// Legea 59/1934 art.29: CEC = plătibil la vedere → fără scadență;
/// BO trebuie să aibă scadentă.
fn validate_scadenta(kind: &str, scadenta: Option<&str>) -> AppResult<()> {
    match kind {
        "CEC" if scadenta.map(|s| !s.is_empty()).unwrap_or(false) => {
            return Err(AppError::Validation(
                "CEC-ul este plătibil la vedere (Legea 58/1934 art.32) — nu poate avea scadentă."
                    .to_string(),
            ));
        }
        "BO" if scadenta.map(|s| s.trim().is_empty()).unwrap_or(true) => {
            return Err(AppError::Validation(
                "Biletul la ordin trebuie să aibă scadentă (Legea 59/1934).".to_string(),
            ));
        }
        _ => {}
    }
    Ok(())
}

fn validate_amount(amount: &str) -> AppResult<Decimal> {
    let d = Decimal::from_str(amount.trim()).map_err(|_| {
        AppError::Validation(format!(
            "Suma invalidă: '{amount}'. Folosiți format zecimal (ex: 1234.56)."
        ))
    })?;
    if d <= Decimal::ZERO {
        return Err(AppError::Validation(
            "Suma trebuie să fie pozitivă.".to_string(),
        ));
    }
    Ok(d)
}

// ─── CRUD ────────────────────────────────────────────────────────────────────

/// Creează un nou instrument de plată (status='registered') și postează nota GL de primire/acceptare.
pub async fn create(
    pool: &SqlitePool,
    input: CreatePaymentInstrumentInput,
) -> AppResult<PaymentInstrument> {
    validate_kind(&input.kind)?;
    validate_direction(&input.direction)?;
    validate_scadenta(&input.kind, input.scadenta.as_deref())?;
    let _amount = validate_amount(&input.amount)?;

    let id = new_id();
    let now = now_unix();
    let currency = input.currency.as_deref().unwrap_or("RON").to_string();

    sqlx::query(
        r#"INSERT INTO payment_instruments
            (id, company_id, kind, direction, partner_id, partner_cui,
             number, amount, currency, issue_date, scadenta, status,
             discount_amount, commission_amount, notes, created_at, updated_at)
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,'registered',NULL,NULL,?12,?13,?13)"#,
    )
    .bind(&id)
    .bind(&input.company_id)
    .bind(&input.kind)
    .bind(&input.direction)
    .bind(&input.partner_id)
    .bind(&input.partner_cui)
    .bind(&input.number)
    .bind(&input.amount)
    .bind(&currency)
    .bind(&input.issue_date)
    .bind(&input.scadenta)
    .bind(&input.notes)
    .bind(now)
    .execute(pool)
    .await?;

    // GL: nota de primire/acceptare
    post_receive_gl(
        pool,
        &id,
        &input.company_id,
        &input.kind,
        &input.direction,
        &input.amount,
        &input.issue_date,
    )
    .await?;

    fetch_one(pool, &id, &input.company_id).await
}

/// Listează toate instrumentele de plată ale companiei.
pub async fn list(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<PaymentInstrument>> {
    let rows = sqlx::query_as::<_, PaymentInstrument>(
        r#"SELECT id, company_id, kind, direction, partner_id, partner_cui,
                  number, amount, currency, issue_date, scadenta, status,
                  discount_amount, commission_amount, notes, created_at, updated_at
           FROM payment_instruments
           WHERE company_id = ?1
           ORDER BY issue_date DESC, created_at DESC"#,
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Returnează un singur instrument de plată (NotFound dacă nu există sau aparține altei companii).
pub async fn fetch_one(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
) -> AppResult<PaymentInstrument> {
    sqlx::query_as::<_, PaymentInstrument>(
        r#"SELECT id, company_id, kind, direction, partner_id, partner_cui,
                  number, amount, currency, issue_date, scadenta, status,
                  discount_amount, commission_amount, notes, created_at, updated_at
           FROM payment_instruments
           WHERE id = ?1 AND company_id = ?2"#,
    )
    .bind(id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}

/// Actualizează câmpurile descriptive (editabil doar când status='registered').
pub async fn update(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    input: UpdatePaymentInstrumentInput,
) -> AppResult<PaymentInstrument> {
    let pi = fetch_one(pool, id, company_id).await?;
    if pi.status != "registered" {
        return Err(AppError::Validation(
            "Instrumentul poate fi editat doar în status 'registered'.".to_string(),
        ));
    }
    validate_scadenta(&pi.kind, input.scadenta.as_deref())?;
    let _amount = validate_amount(&input.amount)?;
    let currency = input.currency.as_deref().unwrap_or("RON").to_string();
    let now = now_unix();

    sqlx::query(
        r#"UPDATE payment_instruments
           SET partner_id=?1, partner_cui=?2, number=?3, amount=?4, currency=?5,
               issue_date=?6, scadenta=?7, notes=?8, updated_at=?9
           WHERE id=?10 AND company_id=?11"#,
    )
    .bind(&input.partner_id)
    .bind(&input.partner_cui)
    .bind(&input.number)
    .bind(&input.amount)
    .bind(&currency)
    .bind(&input.issue_date)
    .bind(&input.scadenta)
    .bind(&input.notes)
    .bind(now)
    .bind(id)
    .bind(company_id)
    .execute(pool)
    .await?;

    // Re-post GL (idempotent — DELETE+reinsert)
    post_receive_gl(
        pool,
        id,
        company_id,
        &pi.kind,
        &pi.direction,
        &input.amount,
        &input.issue_date,
    )
    .await?;

    fetch_one(pool, id, company_id).await
}

/// Șterge un instrument de plată (și GL-ul asociat, cascadat prin source_id).
pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    let pi = fetch_one(pool, id, company_id).await?;
    // Ștergem toate notele GL asociate (toate event-urile, inclusiv cele de scontare:
    // discount_remit = D5114=C413, deposit_reverse = stornarea depunerii anterioare).
    for suffix in &[
        "receive",
        "deposit",
        "collect",
        "discount",
        "discount_remit",
        "deposit_reverse",
        "dishonor",
        "pay",
    ] {
        let source_id = format!("{id}_{suffix}");
        sqlx::query("DELETE FROM gl_journal WHERE company_id=?1 AND source_type='PAYMENT_INSTRUMENT' AND source_id=?2")
            .bind(&pi.company_id)
            .bind(&source_id)
            .execute(pool)
            .await?;
    }
    sqlx::query("DELETE FROM payment_instruments WHERE id=?1 AND company_id=?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    Ok(())
}

// ─── Lifecycle events ────────────────────────────────────────────────────────

/// DEPUNERE la bancă: registered → deposited.
/// CEC: D 5112 = C 413 ; BO: D 5113 = C 413
pub async fn event_deposit(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    date: &str,
) -> AppResult<PaymentInstrument> {
    let pi = fetch_one(pool, id, company_id).await?;
    guard_status(&pi.status, "registered", "depunere")?;
    guard_direction(&pi.direction, "received", "depunere la bancă")?;

    let amount = parse_amount(&pi.amount)?;
    let transit = if pi.kind == "CEC" { "5112" } else { "5113" };

    post_pi_gl(
        pool,
        id,
        company_id,
        "deposit",
        date,
        &format!("Depunere {} la bancă {}", pi.kind, id),
        &[
            ("413", Decimal::ZERO, amount),
            (transit, amount, Decimal::ZERO),
        ],
    )
    .await?;

    set_status(pool, id, company_id, "deposited").await?;
    fetch_one(pool, id, company_id).await
}

/// ÎNCASARE efectivă: deposited → collected.
/// CEC: D 5121 = C 5112 ; BO: D 5121 = C 5113
pub async fn event_collect(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    date: &str,
) -> AppResult<PaymentInstrument> {
    let pi = fetch_one(pool, id, company_id).await?;
    guard_status(&pi.status, "deposited", "încasare")?;
    guard_direction(&pi.direction, "received", "încasare")?;

    let amount = parse_amount(&pi.amount)?;
    let transit = if pi.kind == "CEC" { "5112" } else { "5113" };

    post_pi_gl(
        pool,
        id,
        company_id,
        "collect",
        date,
        &format!("Încasare {} {}", pi.kind, id),
        &[
            (transit, Decimal::ZERO, amount),
            ("5121", amount, Decimal::ZERO),
        ],
    )
    .await?;

    set_status(pool, id, company_id, "collected").await?;
    fetch_one(pool, id, company_id).await
}

/// SCONTARE (doar BO received, deposited → discounted):
///   1. Remitere la bancă: D 5114 = C 413  [dacă nu s-a depus mai întâi, altfel direct din 5113]
///   2. Decontare: D 5121(net) + D 667(scont) [+ D 627(comision)] = C 5114(brut)
///
/// Nota: dacă instrumentul e registered, facem mai întâi remiterea (5114) direct din 413.
/// Dacă e deposited (5113), NU putem sconta — scontarea se face direct din registered.
/// Conform monografiei, scontarea înlocuiește depunerea normală:
///   registered → remitere (D 5114 = C 413) → decontare (D 5121/667 = C 5114) → discounted
pub async fn event_discount(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    date: &str,
    discount_amount: &str,
    commission_amount: Option<&str>,
) -> AppResult<PaymentInstrument> {
    let pi = fetch_one(pool, id, company_id).await?;
    if pi.kind != "BO" {
        return Err(AppError::Validation(
            "Scontarea este disponibilă doar pentru Bilete la ordin (BO).".to_string(),
        ));
    }
    if pi.status != "registered" && pi.status != "deposited" {
        return Err(AppError::Validation(
            "Scontarea este posibilă doar din status 'registered' sau 'deposited'.".to_string(),
        ));
    }
    guard_direction(&pi.direction, "received", "scontare")?;

    let total = parse_amount(&pi.amount)?;
    let scont = parse_amount(discount_amount)?;
    let comision = commission_amount
        .map(parse_amount)
        .transpose()?
        .unwrap_or(Decimal::ZERO);

    if scont + comision >= total {
        return Err(AppError::Validation(
            "Scontul + comisionul nu poate depăși suma totală a instrumentului.".to_string(),
        ));
    }

    let net = total - scont - comision;

    // Pasul 1: dacă e deposited (5113), întoarcem mai întâi din 5113 → 413, altfel direct din 413.
    // În practică monografia standard: registered → D 5114 = C 413.
    // Dacă deja deposited (5113), mai întâi reverse deposit, apoi scontare.
    if pi.status == "deposited" {
        // Reverse deposit: D 413 = C 5113
        let transit = if pi.kind == "CEC" { "5112" } else { "5113" };
        post_pi_gl(
            pool,
            id,
            company_id,
            "deposit_reverse",
            date,
            &format!("Reverse depunere BO {} pentru scontare", id),
            &[
                (transit, Decimal::ZERO, total),
                ("413", total, Decimal::ZERO),
            ],
        )
        .await?;
    }

    // Remitere la bancă spre scontare: D 5114 = C 413
    post_pi_gl(
        pool,
        id,
        company_id,
        "discount_remit",
        date,
        &format!("Remitere BO {} spre scontare", id),
        &[
            ("413", Decimal::ZERO, total),
            ("5114", total, Decimal::ZERO),
        ],
    )
    .await?;

    // Decontare: D 5121(net) + D 667(scont) [+ D 627(comision)] = C 5114
    let mut lines: Vec<(&str, Decimal, Decimal)> = vec![
        ("5114", Decimal::ZERO, total),
        ("5121", net, Decimal::ZERO),
        ("667", scont, Decimal::ZERO),
    ];
    if comision > Decimal::ZERO {
        lines.push(("627", comision, Decimal::ZERO));
    }
    post_pi_gl(
        pool,
        id,
        company_id,
        "collect",
        date,
        &format!("Decontare scontare BO {}", id),
        &lines,
    )
    .await?;

    // Salvăm sumele scontului
    sqlx::query("UPDATE payment_instruments SET discount_amount=?1, commission_amount=?2, status='discounted', updated_at=?3 WHERE id=?4 AND company_id=?5")
        .bind(discount_amount)
        .bind(commission_amount)
        .bind(now_unix())
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;

    fetch_one(pool, id, company_id).await
}

/// REFUZ (dishonor): deposited → dishonored.
/// CEC: D 4111 = C 5112 ; BO: D 4111 = C 5113
pub async fn event_dishonor(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    date: &str,
) -> AppResult<PaymentInstrument> {
    let pi = fetch_one(pool, id, company_id).await?;
    guard_status(&pi.status, "deposited", "refuz/protest")?;
    guard_direction(&pi.direction, "received", "refuz/protest")?;

    let amount = parse_amount(&pi.amount)?;
    let transit = if pi.kind == "CEC" { "5112" } else { "5113" };

    post_pi_gl(
        pool,
        id,
        company_id,
        "dishonor",
        date,
        &format!("Refuz/protest {} {}", pi.kind, id),
        &[
            (transit, Decimal::ZERO, amount),
            ("4111", amount, Decimal::ZERO),
        ],
    )
    .await?;

    set_status(pool, id, company_id, "dishonored").await?;
    fetch_one(pool, id, company_id).await
}

/// PLATĂ la scadență (issued instruments): deposited → paid.
/// D 403 = C 5121
pub async fn event_pay(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    date: &str,
) -> AppResult<PaymentInstrument> {
    let pi = fetch_one(pool, id, company_id).await?;
    // Un instrument EMIS nu trece prin "deposited" (acela e pentru cele primite): la creare se
    // postează acceptarea D401=C403 (status 'registered'), iar plata la scadență D403=C5121 se face
    // direct din 'registered'.
    guard_status(&pi.status, "registered", "plată la scadență")?;
    guard_direction(&pi.direction, "issued", "plată la scadență")?;

    let amount = parse_amount(&pi.amount)?;

    post_pi_gl(
        pool,
        id,
        company_id,
        "pay",
        date,
        &format!("Plată {} emis {}", pi.kind, id),
        // D 403 (stingem efectul de plată) = C 5121 (ieșire numerar din bancă).
        &[
            ("403", amount, Decimal::ZERO),
            ("5121", Decimal::ZERO, amount),
        ],
    )
    .await?;

    set_status(pool, id, company_id, "paid").await?;
    fetch_one(pool, id, company_id).await
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

/// Postează nota GL de primire/acceptare (event='receive').
/// received: D 413 = C 4111
/// issued:   D 401 = C 403
async fn post_receive_gl(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    kind: &str,
    direction: &str,
    amount: &str,
    date: &str,
) -> AppResult<()> {
    let d = parse_amount(amount)?;
    let (debit_acct, credit_acct, desc) = match direction {
        "received" => ("413", "4111", format!("{kind} primit înregistrat")),
        "issued" => ("401", "403", format!("{kind} emis acceptat")),
        _ => return Err(AppError::Validation("direction invalid".to_string())),
    };
    post_pi_gl(
        pool,
        id,
        company_id,
        "receive",
        date,
        &desc,
        &[
            (credit_acct, Decimal::ZERO, d),
            (debit_acct, d, Decimal::ZERO),
        ],
    )
    .await
}

async fn post_pi_gl(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    event: &str,
    date: &str,
    description: &str,
    lines: &[(&str, Decimal, Decimal)],
) -> AppResult<()> {
    let source_id = format!("{id}_{event}");
    post_manual_journal(
        pool,
        &ManualJournal {
            company_id,
            journal_id: &source_id,
            journal_type: "INSTRUMENT",
            source_type: "PAYMENT_INSTRUMENT",
            source_id: &source_id,
            date,
            description,
        },
        lines,
    )
    .await
}

fn set_status<'a>(
    pool: &'a SqlitePool,
    id: &'a str,
    company_id: &'a str,
    status: &'a str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = AppResult<()>> + Send + 'a>> {
    Box::pin(async move {
        sqlx::query(
            "UPDATE payment_instruments SET status=?1, updated_at=?2 WHERE id=?3 AND company_id=?4",
        )
        .bind(status)
        .bind(now_unix())
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
        Ok(())
    })
}

fn parse_amount(s: &str) -> AppResult<Decimal> {
    Decimal::from_str(s.trim())
        .map_err(|_| AppError::Validation(format!("Suma stocată invalidă: '{s}'.")))
}

fn guard_status(current: &str, expected: &str, action: &str) -> AppResult<()> {
    if current != expected {
        return Err(AppError::Validation(format!(
            "Operația '{action}' necesită status '{expected}', dar instrumentul are status '{current}'."
        )));
    }
    Ok(())
}

fn guard_direction(current: &str, expected: &str, action: &str) -> AppResult<()> {
    if current != expected {
        return Err(AppError::Validation(format!(
            "Operația '{action}' este disponibilă doar pentru instrumente cu direction='{expected}'."
        )));
    }
    Ok(())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn validate_kind_ok() {
        assert!(validate_kind("CEC").is_ok());
        assert!(validate_kind("BO").is_ok());
    }

    #[test]
    fn validate_kind_invalid() {
        assert!(validate_kind("BILET").is_err());
        assert!(validate_kind("").is_err());
    }

    #[test]
    fn validate_direction_ok() {
        assert!(validate_direction("received").is_ok());
        assert!(validate_direction("issued").is_ok());
    }

    #[test]
    fn validate_direction_invalid() {
        assert!(validate_direction("out").is_err());
        assert!(validate_direction("").is_err());
    }

    #[test]
    fn cec_with_scadenta_rejected() {
        // Legea 58/1934 art.32: CEC = plătibil la vedere
        assert!(validate_scadenta("CEC", Some("2026-09-01")).is_err());
        assert!(validate_scadenta("CEC", None).is_ok());
        assert!(validate_scadenta("CEC", Some("")).is_ok()); // empty string = absent
    }

    #[test]
    fn bo_without_scadenta_rejected() {
        assert!(validate_scadenta("BO", None).is_err());
        assert!(validate_scadenta("BO", Some("")).is_err());
        assert!(validate_scadenta("BO", Some("2026-09-01")).is_ok());
    }

    #[test]
    fn amount_validation() {
        assert!(validate_amount("1234.56").is_ok());
        assert!(validate_amount("0.01").is_ok());
        assert!(validate_amount("0").is_err());
        assert!(validate_amount("-1").is_err());
        assert!(validate_amount("abc").is_err());
    }

    #[test]
    fn discount_cannot_exceed_total() {
        // scont + comision = 1001 > 1000 → eroare
        let total = dec("1000");
        let scont = dec("900");
        let comision = dec("101");
        assert!(scont + comision >= total);
    }

    #[test]
    fn discount_net_calculation() {
        let total = dec("1000");
        let scont = dec("50");
        let comision = dec("10");
        let net = total - scont - comision;
        assert_eq!(net, dec("940"));
    }

    // ─── GL integration tests (post real journals, assert the monografie + balance) ──────────

    async fn setup() -> (SqlitePool, String) {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let cid = "pi_co".to_string();
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES (?1,'44444444','PI SRL','S','C','CJ','RO')",
        )
        .bind(&cid)
        .execute(&pool)
        .await
        .unwrap();
        (pool, cid)
    }

    /// (debit, credit) sums for an account across ALL PAYMENT_INSTRUMENT journals.
    async fn acct(pool: &SqlitePool, cid: &str, account: &str) -> (f64, f64) {
        let row: (Option<f64>, Option<f64>) = sqlx::query_as(
            "SELECT SUM(CAST(e.debit AS REAL)), SUM(CAST(e.credit AS REAL)) FROM gl_entry e \
             JOIN gl_journal j ON j.id=e.journal_pk \
             WHERE j.company_id=?1 AND j.source_type='PAYMENT_INSTRUMENT' AND e.account_code=?2",
        )
        .bind(cid)
        .bind(account)
        .fetch_one(pool)
        .await
        .unwrap();
        (row.0.unwrap_or(0.0), row.1.unwrap_or(0.0))
    }

    fn mk(
        cid: &str,
        kind: &str,
        direction: &str,
        amount: &str,
        scadenta: Option<&str>,
    ) -> CreatePaymentInstrumentInput {
        CreatePaymentInstrumentInput {
            company_id: cid.to_string(),
            kind: kind.to_string(),
            direction: direction.to_string(),
            partner_id: None,
            partner_cui: Some("RO123".to_string()),
            number: Some("X-1".to_string()),
            amount: amount.to_string(),
            currency: None,
            issue_date: "2026-06-01".to_string(),
            scadenta: scadenta.map(|s| s.to_string()),
            notes: None,
        }
    }

    #[tokio::test]
    async fn received_cec_chain_gl_balanced() {
        let (pool, cid) = setup().await;
        let pi = create(&pool, mk(&cid, "CEC", "received", "1000.00", None))
            .await
            .unwrap();
        event_deposit(&pool, &pi.id, &cid, "2026-06-02")
            .await
            .unwrap();
        event_collect(&pool, &pi.id, &cid, "2026-06-10")
            .await
            .unwrap();
        // 413 nets 0 (D receive / C deposit); 5112 nets 0 (D deposit / C collect);
        // 4111 credited 1000 (receivable replaced); 5121 debited 1000 (cash in).
        let (d413, c413) = acct(&pool, &cid, "413").await;
        assert!((d413 - c413).abs() < 0.01, "413 nets to 0");
        let (d5112, c5112) = acct(&pool, &cid, "5112").await;
        assert!((d5112 - c5112).abs() < 0.01, "5112 nets to 0");
        let (_, c4111) = acct(&pool, &cid, "4111").await;
        assert!((c4111 - 1000.0).abs() < 0.01, "4111 C=1000");
        let (d5121, _) = acct(&pool, &cid, "5121").await;
        assert!((d5121 - 1000.0).abs() < 0.01, "5121 D=1000");
    }

    #[tokio::test]
    async fn issued_accept_then_pay_gl() {
        // Regression: the issued-payer lifecycle was dead (event_pay required 'deposited',
        // unreachable for issued). Now create posts D401=C403 (registered) and pay fires from there.
        let (pool, cid) = setup().await;
        let pi = create(
            &pool,
            mk(&cid, "BO", "issued", "500.00", Some("2026-09-01")),
        )
        .await
        .unwrap();
        assert_eq!(pi.status, "registered");
        let paid = event_pay(&pool, &pi.id, &cid, "2026-09-01").await.unwrap();
        assert_eq!(paid.status, "paid", "issued BO can now be paid");
        let (d403, c403) = acct(&pool, &cid, "403").await;
        assert!(
            (d403 - c403).abs() < 0.01,
            "403 nets to 0 (C accept / D pay)"
        );
        let (d401, _) = acct(&pool, &cid, "401").await;
        assert!(
            (d401 - 500.0).abs() < 0.01,
            "401 D=500 (supplier paid via effect)"
        );
        let (_, c5121) = acct(&pool, &cid, "5121").await;
        assert!((c5121 - 500.0).abs() < 0.01, "5121 C=500 (cash out)");
    }

    #[tokio::test]
    async fn scontare_balances_then_delete_cleans_gl() {
        let (pool, cid) = setup().await;
        let pi = create(
            &pool,
            mk(&cid, "BO", "received", "1000.00", Some("2026-12-01")),
        )
        .await
        .unwrap();
        event_deposit(&pool, &pi.id, &cid, "2026-06-02")
            .await
            .unwrap();
        event_discount(&pool, &pi.id, &cid, "2026-06-05", "30.00", Some("10.00"))
            .await
            .unwrap();
        // discount→667=30, commission→627=10, cash in 5121=960 (= 1000−30−10).
        let (d667, _) = acct(&pool, &cid, "667").await;
        assert!((d667 - 30.0).abs() < 0.01, "667 D=30 (dobândă de scont)");
        let (d627, _) = acct(&pool, &cid, "627").await;
        assert!((d627 - 10.0).abs() < 0.01, "627 D=10 (comision)");
        let (d5121, _) = acct(&pool, &cid, "5121").await;
        assert!((d5121 - 960.0).abs() < 0.01, "5121 D=960 (net)");
        // delete cleans ALL instrument journals incl. discount_remit + deposit_reverse.
        delete(&pool, &pi.id, &cid).await.unwrap();
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_journal WHERE company_id=?1 AND source_type='PAYMENT_INSTRUMENT'",
        )
        .bind(&cid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            n, 0,
            "all instrument journals deleted (incl. scontare suffixes)"
        );
    }

    #[tokio::test]
    async fn dishonor_reopens_4111() {
        let (pool, cid) = setup().await;
        let pi = create(&pool, mk(&cid, "CEC", "received", "200.00", None))
            .await
            .unwrap();
        event_deposit(&pool, &pi.id, &cid, "2026-06-02")
            .await
            .unwrap();
        let dh = event_dishonor(&pool, &pi.id, &cid, "2026-06-08")
            .await
            .unwrap();
        assert_eq!(dh.status, "dishonored");
        // dishonor posts D4111=C5112 → the receivable is re-established (debit on 4111).
        let (d4111, c4111) = acct(&pool, &cid, "4111").await;
        assert!(
            (d4111 - 200.0).abs() < 0.01,
            "4111 debited 200 on dishonor (reopened)"
        );
        assert!((c4111 - 200.0).abs() < 0.01, "4111 credited 200 on receive");
    }
}
