//! Registru-inventar + inventariere (Legea 82/1991 art. 7/20/25; OMFP 2861/2009; OMFP 2634/2015).
//!
//! ## Forme OMFP 2634/2015
//!   - **Listă de inventariere cod 14-3-12**: sesiune + linii (qty scriptic → qty faptic → diferență).
//!   - **Registru-inventar cod 14-1-2**: 6 coloane, imutabil per (company, fiscal_year, seq_no).
//!
//! ## GL diff posting (simplificat — doar neimputabil)
//!   - Lipsă neimputabilă: **D 607 = C <stock_account>** pentru |diff| (descarcă gestiunea la cost).
//!   - Plus de inventar: **D <stock_account> = C 607** pentru diff (reduce cheltuiala).
//!
//! ## DEFERIT (comentat in-code + UI note)
//!   - Imputabil (461/4282 = %7588 + 4427 la valoare de înlocuire).
//!   - Ajustare TVA pe lipsă neimputabilă (635 = 4427, art. 304 C. fiscal).
//!   - Limite perisabilități (HG 831/2004) — necesită input suplimentar.
//!
//!   Acestea se postează via nota manuală (W4) până la implementare.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

use crate::db::gl::{post_manual_journal, ManualJournal};
use crate::db::invoices::round2;
use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s.trim()).unwrap_or(Decimal::ZERO)
}

fn fmt2(d: Decimal) -> String {
    format!("{:.2}", d)
}

fn fmt6(d: Decimal) -> String {
    format!("{:.6}", d)
}

// ─── Models ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct InventorySession {
    pub id: String,
    pub company_id: String,
    pub reference_date: String,
    pub fiscal_year: i64,
    /// ANUAL | INCEPERE | INCETARE | PREDARE_GESTIUNE | CALAMITATE
    pub r#type: String,
    pub gestiune: Option<String>,
    /// DRAFT | FINALIZED
    pub status: String,
    pub comisie_members: String, // JSON array
    pub notes: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct InventoryLine {
    pub id: String,
    pub session_id: String,
    pub account_code: String,
    pub item_name: String,
    pub um: String,
    pub qty_scriptic: String,
    pub qty_faptic: String,
    pub unit_price: String,
    pub value_contabila: String,
    pub value_inventar: String,
    pub diff_value: String,
    pub diff_cause: Option<String>,
    pub imputable: i64,
    pub product_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RegistruInventarEntry {
    pub id: String,
    pub company_id: String,
    pub fiscal_year: i64,
    pub seq_no: i64,
    pub recap_text: String,
    pub value_contabila: String,
    pub value_inventar: String,
    pub diff_value: String,
    pub diff_cause: String,
    pub source_session_id: Option<String>,
    pub created_at: i64,
}

// ─── Inputs ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSessionInput {
    pub company_id: String,
    pub reference_date: String,
    pub fiscal_year: i64,
    pub r#type: Option<String>,
    pub gestiune: Option<String>,
    pub comisie_members: Option<String>, // JSON
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateLineFapticInput {
    pub line_id: String,
    pub session_id: String,
    pub company_id: String,
    pub qty_faptic: String,
    pub diff_cause: Option<String>,
    pub imputable: Option<bool>,
}

// ─── Session CRUD ─────────────────────────────────────────────────────────────

/// Create a new inventory session in DRAFT status.
pub async fn create_session(
    pool: &SqlitePool,
    input: CreateSessionInput,
) -> AppResult<InventorySession> {
    let session_type = input.r#type.as_deref().unwrap_or("ANUAL");
    let valid_types = [
        "ANUAL",
        "INCEPERE",
        "INCETARE",
        "PREDARE_GESTIUNE",
        "CALAMITATE",
    ];
    if !valid_types.contains(&session_type) {
        return Err(AppError::Validation(format!(
            "Tip inventariere invalid: {session_type}. Valori: ANUAL|INCEPERE|INCETARE|PREDARE_GESTIUNE|CALAMITATE"
        )));
    }
    let comisie = input.comisie_members.as_deref().unwrap_or("[]");
    let id = new_id();
    let now = now_unix();
    sqlx::query(
        "INSERT INTO inventory_sessions \
         (id, company_id, reference_date, fiscal_year, type, gestiune, status, comisie_members, notes, created_at, updated_at) \
         VALUES (?1,?2,?3,?4,?5,?6,'DRAFT',?7,?8,?9,?9)",
    )
    .bind(&id)
    .bind(&input.company_id)
    .bind(&input.reference_date)
    .bind(input.fiscal_year)
    .bind(session_type)
    .bind(&input.gestiune)
    .bind(comisie)
    .bind(&input.notes)
    .bind(now)
    .execute(pool)
    .await?;
    get_session(pool, &id, &input.company_id).await
}

pub async fn get_session(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
) -> AppResult<InventorySession> {
    let s: Option<InventorySession> = sqlx::query_as(
        "SELECT id, company_id, reference_date, fiscal_year, type, gestiune, status, \
         comisie_members, notes, created_at, updated_at \
         FROM inventory_sessions WHERE id=?1 AND company_id=?2",
    )
    .bind(id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?;
    s.ok_or(AppError::NotFound)
}

pub async fn list_sessions(
    pool: &SqlitePool,
    company_id: &str,
    fiscal_year: Option<i64>,
) -> AppResult<Vec<InventorySession>> {
    Ok(sqlx::query_as(
        "SELECT id, company_id, reference_date, fiscal_year, type, gestiune, status, \
         comisie_members, notes, created_at, updated_at \
         FROM inventory_sessions \
         WHERE company_id=?1 AND (?2 IS NULL OR fiscal_year=?2) \
         ORDER BY reference_date DESC, created_at DESC",
    )
    .bind(company_id)
    .bind(fiscal_year)
    .fetch_all(pool)
    .await?)
}

/// Delete a session — only DRAFT sessions may be deleted.
pub async fn delete_session(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    let s = get_session(pool, id, company_id).await?;
    if s.status == "FINALIZED" {
        return Err(AppError::Validation(
            "Sesiunea finalizată nu poate fi ștearsă.".into(),
        ));
    }
    sqlx::query("DELETE FROM inventory_sessions WHERE id=?1 AND company_id=?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    Ok(())
}

// ─── Lines ────────────────────────────────────────────────────────────────────

pub async fn list_lines(
    pool: &SqlitePool,
    session_id: &str,
    company_id: &str,
) -> AppResult<Vec<InventoryLine>> {
    // Verify session ownership.
    get_session(pool, session_id, company_id).await?;
    Ok(sqlx::query_as(
        "SELECT id, session_id, account_code, item_name, um, qty_scriptic, qty_faptic, \
         unit_price, value_contabila, value_inventar, diff_value, diff_cause, imputable, \
         product_id, created_at, updated_at \
         FROM inventory_lines WHERE session_id=?1 ORDER BY account_code, item_name",
    )
    .bind(session_id)
    .fetch_all(pool)
    .await?)
}

/// Update the faptic qty on a single line (and recompute derived fields).
/// Only DRAFT sessions allow changes.
pub async fn update_line_faptic(
    pool: &SqlitePool,
    input: UpdateLineFapticInput,
) -> AppResult<InventoryLine> {
    let s = get_session(pool, &input.session_id, &input.company_id).await?;
    if s.status == "FINALIZED" {
        return Err(AppError::Validation(
            "Sesiunea finalizată nu poate fi modificată.".into(),
        ));
    }
    let qty_f = Decimal::from_str(input.qty_faptic.trim())
        .map_err(|_| AppError::Validation("Cantitate faptică invalidă.".into()))?;
    if qty_f.is_sign_negative() {
        return Err(AppError::Validation(
            "Cantitatea faptică nu poate fi negativă.".into(),
        ));
    }
    // Fetch the unit_price + qty_scriptic for this line.
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT unit_price, qty_scriptic FROM inventory_lines WHERE id=?1 AND session_id=?2",
    )
    .bind(&input.line_id)
    .bind(&input.session_id)
    .fetch_optional(pool)
    .await?;
    let (unit_price_str, qty_s_str) = row.ok_or(AppError::NotFound)?;
    let unit_price = dec(&unit_price_str);
    let qty_s = dec(&qty_s_str);

    let value_inventar = round2(qty_f * unit_price);
    let value_contabila = round2(qty_s * unit_price);
    let diff_value = round2(value_inventar - value_contabila);
    let now = now_unix();

    sqlx::query(
        "UPDATE inventory_lines SET \
         qty_faptic=?2, value_inventar=?3, diff_value=?4, diff_cause=?5, imputable=?6, updated_at=?7 \
         WHERE id=?1 AND session_id=?8",
    )
    .bind(&input.line_id)
    .bind(fmt6(qty_f))
    .bind(fmt2(value_inventar))
    .bind(fmt2(diff_value))
    .bind(&input.diff_cause)
    .bind(input.imputable.unwrap_or(false) as i64)
    .bind(now)
    .bind(&input.session_id)
    .execute(pool)
    .await?;

    let line: InventoryLine = sqlx::query_as(
        "SELECT id, session_id, account_code, item_name, um, qty_scriptic, qty_faptic, \
         unit_price, value_contabila, value_inventar, diff_value, diff_cause, imputable, \
         product_id, created_at, updated_at \
         FROM inventory_lines WHERE id=?1",
    )
    .bind(&input.line_id)
    .fetch_one(pool)
    .await?;
    Ok(line)
}

// ─── Pre-fill ─────────────────────────────────────────────────────────────────

/// Pre-fill a session's lines from the current on-hand stock (stoc scriptic).
///
/// Pulls every product with non-zero stock_qty for this company, creates one line per
/// product using:
///   - qty_scriptic  = products.stock_qty (the running stock cache)
///   - unit_price    = products.avg_cost  (weighted avg, same column that `recompute_product` writes)
///   - value_contabila = qty_scriptic × unit_price
///   - qty_faptic    = qty_scriptic (pre-filled = scriptic; user then counts and corrects)
///   - diff_value    = 0 (no diff until user enters actual faptic)
///
/// Services (is_service=1) are excluded — they have no stock. Existing lines for this session are
/// replaced so the command is idempotent (re-prefill = fresh snapshot).
pub async fn prefill_session_lines(
    pool: &SqlitePool,
    session_id: &str,
    company_id: &str,
) -> AppResult<Vec<InventoryLine>> {
    let s = get_session(pool, session_id, company_id).await?;
    if s.status == "FINALIZED" {
        return Err(AppError::Validation(
            "Sesiunea finalizată nu poate fi re-pre-completată.".into(),
        ));
    }

    // Fetch stock: product_id, name, unit, stock_qty, avg_cost, stock_account.
    // avg_cost is written by recompute_product — it's the valuation-method-correct unit cost.
    // stock_account defaults to '371' (mărfuri) when NULL, consistent with recompute_product.
    let products: Vec<(String, String, String, String, String, String)> = sqlx::query_as(
        "SELECT id, name, unit, \
         COALESCE(stock_qty, '0.000000'), \
         COALESCE(avg_cost, '0.00'), \
         COALESCE(stock_account, '371') \
         FROM products \
         WHERE company_id=?1 AND is_service=0 AND active=1 \
         AND CAST(COALESCE(stock_qty, '0') AS REAL) <> 0.0 \
         ORDER BY name",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;

    let mut tx = pool.begin().await?;
    // Clear existing lines (idempotent re-prefill).
    sqlx::query("DELETE FROM inventory_lines WHERE session_id=?1")
        .bind(session_id)
        .execute(&mut *tx)
        .await?;
    let now = now_unix();
    for (pid, name, unit, stock_qty, avg_cost, stock_account) in &products {
        let qty = dec(stock_qty);
        let cost = dec(avg_cost);
        let value_contabila = round2(qty * cost);
        // Pre-fill faptic = scriptic (user will correct per physical count).
        let id = new_id();
        sqlx::query(
            "INSERT INTO inventory_lines \
             (id, session_id, account_code, item_name, um, qty_scriptic, qty_faptic, \
              unit_price, value_contabila, value_inventar, diff_value, product_id, created_at, updated_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?6,?7,?8,?8,'0.00',?9,?10,?10)",
        )
        .bind(&id)
        .bind(session_id)
        .bind(stock_account)
        .bind(name)
        .bind(unit)
        .bind(fmt6(qty))
        .bind(fmt2(cost))
        .bind(fmt2(value_contabila))
        .bind(pid)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    list_lines(pool, session_id, company_id).await
}

// ─── Finalize ─────────────────────────────────────────────────────────────────

/// Finalize a session: DRAFT → FINALIZED.
///
/// Snapshots each account group's net diff into `registru_inventar_entries` for
/// the session's fiscal_year, with sequential seq_no per (company, year). After
/// finalization the session and its lines are IMMUTABLE.
pub async fn finalize_session(
    pool: &SqlitePool,
    session_id: &str,
    company_id: &str,
) -> AppResult<InventorySession> {
    let s = get_session(pool, session_id, company_id).await?;
    if s.status == "FINALIZED" {
        return Err(AppError::Validation(
            "Sesiunea este deja finalizată.".into(),
        ));
    }
    let lines = list_lines(pool, session_id, company_id).await?;
    if lines.is_empty() {
        return Err(AppError::Validation(
            "Sesiunea nu are linii — pre-completați sau adăugați linii înainte de finalizare."
                .into(),
        ));
    }

    let now = now_unix();
    let mut tx = pool.begin().await?;

    // Mark session FINALIZED.
    sqlx::query("UPDATE inventory_sessions SET status='FINALIZED', updated_at=?2 WHERE id=?1")
        .bind(session_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;

    // Group lines by account_code for the registru snapshot.
    use std::collections::BTreeMap;
    let mut groups: BTreeMap<String, (Decimal, Decimal, Decimal, String)> = BTreeMap::new();
    for line in &lines {
        let entry = groups.entry(line.account_code.clone()).or_insert((
            Decimal::ZERO,
            Decimal::ZERO,
            Decimal::ZERO,
            line.diff_cause.clone().unwrap_or_default(),
        ));
        entry.0 += dec(&line.value_contabila);
        entry.1 += dec(&line.value_inventar);
        entry.2 += dec(&line.diff_value);
        // Use the most common diff_cause (simple: last non-null wins).
        if let Some(c) = &line.diff_cause {
            if !c.is_empty() {
                entry.3 = c.clone();
            }
        }
    }

    // Find the current max seq_no for this company/year.
    let max_seq: Option<i64> = sqlx::query_scalar(
        "SELECT MAX(seq_no) FROM registru_inventar_entries WHERE company_id=?1 AND fiscal_year=?2",
    )
    .bind(company_id)
    .bind(s.fiscal_year)
    .fetch_optional(&mut *tx)
    .await?
    .flatten();
    let mut seq = max_seq.unwrap_or(0);

    for (acct, (val_ctb, val_inv, diff, cause)) in &groups {
        seq += 1;
        let recap = format!("Stoc ct. {} (inventariere {})", acct, s.reference_date);
        let id = new_id();
        sqlx::query(
            "INSERT INTO registru_inventar_entries \
             (id, company_id, fiscal_year, seq_no, recap_text, value_contabila, value_inventar, \
              diff_value, diff_cause, source_session_id, created_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        )
        .bind(&id)
        .bind(company_id)
        .bind(s.fiscal_year)
        .bind(seq)
        .bind(&recap)
        .bind(fmt2(*val_ctb))
        .bind(fmt2(*val_inv))
        .bind(fmt2(*diff))
        .bind(cause)
        .bind(session_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    get_session(pool, session_id, company_id).await
}

// ─── GL diff posting ──────────────────────────────────────────────────────────

/// Post the BASIC neimputabil inventory diffs via `post_manual_journal`.
///
/// source_type = 'INVENTORY' — NOT in generate_gl_entries, so these persist across re-generation.
///
/// **Auto-posted** (neimputabil only):
///   - Lipsă (diff < 0):  D 607 "Cheltuieli privind mărfurile" = C <stock_account> |diff|
///   - Plus  (diff > 0):  D <stock_account> = C 607  diff
///
/// **DEFERIT** — requires manual journal (W4):
///   1. Imputabil (imputable=1): D 461 "Debitori diverși" / D 4282 = C 7588 + C 4427
///      (la valoarea de înlocuire, art. 275 Cod fiscal).
///   2. Ajustare TVA pe lipsă neimputabilă: D 635 = C 4427 (art. 304 alin. (1) lit. c C. fiscal).
///   3. Perisabilități (HG 831/2004): limitele legale necesită input suplimentar.
///
///   Postați aceste cazuri via nota manuală din meniu (Contabilitate → Note manuale).
pub async fn post_inventory_diffs(
    pool: &SqlitePool,
    session_id: &str,
    company_id: &str,
) -> AppResult<()> {
    let s = get_session(pool, session_id, company_id).await?;
    if s.status != "FINALIZED" {
        return Err(AppError::Validation(
            "Postarea diferențelor este permisă doar pentru sesiuni FINALIZATE.".into(),
        ));
    }
    let lines = list_lines(pool, session_id, company_id).await?;

    // Group net diff per stock account (only neimputabil / non-imputable).
    // imputabil lines need case-specific accounts (461/4282, 4427) — deferred.
    use std::collections::HashMap;
    let mut net_by_account: HashMap<String, Decimal> = HashMap::new();
    for line in &lines {
        if line.imputable != 0 {
            // DEFERRED: imputabil lines skipped — post manually via W4 nota manuală.
            tracing::warn!(
                line_id = %line.id,
                "INVENTORY GL: linie imputabilă — postarea 461/4282 = 7588 + 4427 este DEFERITĂ; \
                 postați manual via nota manuală (Contabilitate → Note manuale)."
            );
            continue;
        }
        let diff = dec(&line.diff_value);
        if diff.is_zero() {
            continue;
        }
        *net_by_account
            .entry(line.account_code.clone())
            .or_insert(Decimal::ZERO) += diff;
    }

    if net_by_account.is_empty() {
        return Ok(());
    }

    // Build a SINGLE balanced note with all accounts.
    // Lines: for each stock account with net diff ≠ 0 → two GL legs (607 + stock_acct).
    let mut gl_lines: Vec<(String, Decimal, Decimal)> = Vec::new();
    let mut total_607_debit = Decimal::ZERO;
    let mut total_607_credit = Decimal::ZERO;

    for (acct, net) in &net_by_account {
        let abs = net.abs();
        if *net < Decimal::ZERO {
            // Lipsă: D 607 = C <stock>
            total_607_debit += abs;
            gl_lines.push((acct.clone(), Decimal::ZERO, abs)); // C <stock>
        } else {
            // Plus: D <stock> = C 607
            total_607_credit += *net;
            gl_lines.push((acct.clone(), *net, Decimal::ZERO)); // D <stock>
        }
    }

    // Net 607 leg.
    if total_607_debit > total_607_credit {
        gl_lines.push((
            "607".to_string(),
            total_607_debit - total_607_credit,
            Decimal::ZERO,
        ));
    } else if total_607_credit > total_607_debit {
        gl_lines.push((
            "607".to_string(),
            Decimal::ZERO,
            total_607_credit - total_607_debit,
        ));
    } else if total_607_debit > Decimal::ZERO {
        // Equal: debit+credit for 607 (net zero — all plusuri cancel lipsuri exactly).
        gl_lines.push(("607".to_string(), total_607_debit, total_607_credit));
    }

    // Convert to the slice form that post_manual_journal expects.
    let lines_ref: Vec<(&str, Decimal, Decimal)> = gl_lines
        .iter()
        .map(|(a, d, c)| (a.as_str(), *d, *c))
        .collect();

    let desc = format!(
        "Diferențe inventar — sesiune {} ({}) neimputabil",
        session_id, s.reference_date
    );

    post_manual_journal(
        pool,
        &ManualJournal {
            company_id,
            journal_id: &format!("INV-{}", &session_id[..8]),
            journal_type: "MEM",
            source_type: "INVENTORY",
            source_id: session_id,
            date: &s.reference_date,
            description: &desc,
        },
        &lines_ref,
    )
    .await?;

    Ok(())
}

// ─── Registru-inventar queries ────────────────────────────────────────────────

pub async fn list_registru_entries(
    pool: &SqlitePool,
    company_id: &str,
    fiscal_year: i64,
) -> AppResult<Vec<RegistruInventarEntry>> {
    Ok(sqlx::query_as(
        "SELECT id, company_id, fiscal_year, seq_no, recap_text, value_contabila, value_inventar, \
         diff_value, diff_cause, source_session_id, created_at \
         FROM registru_inventar_entries \
         WHERE company_id=?1 AND fiscal_year=?2 \
         ORDER BY seq_no",
    )
    .bind(company_id)
    .bind(fiscal_year)
    .fetch_all(pool)
    .await?)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    async fn setup() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        // Seed: one company + two products with on-hand stock.
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co1','12345678','Test SRL','Str. 1','Cluj','CJ','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Product 1: 10 units @ 5.00 RON = 50.00
        sqlx::query(
            "INSERT INTO products \
             (id, company_id, name, unit, stock_qty, avg_cost, stock_value, stock_account, is_service, active) \
             VALUES ('p1','co1','Marfă A','buc','10.000000','5.00','50.00','371',0,1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Product 2: 20 units @ 3.50 RON = 70.00
        sqlx::query(
            "INSERT INTO products \
             (id, company_id, name, unit, stock_qty, avg_cost, stock_value, stock_account, is_service, active) \
             VALUES ('p2','co1','Materie B','kg','20.000000','3.50','70.00','301',0,1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Service product — must NOT appear in pre-fill (is_service=1, no stock).
        sqlx::query(
            "INSERT INTO products \
             (id, company_id, name, unit, is_service, active) \
             VALUES ('p3','co1','Serviciu','ora',1,1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        pool
    }

    fn input() -> CreateSessionInput {
        CreateSessionInput {
            company_id: "co1".into(),
            reference_date: "2026-12-31".into(),
            fiscal_year: 2026,
            r#type: Some("ANUAL".into()),
            gestiune: None,
            comisie_members: None,
            notes: None,
        }
    }

    // ── Test 1: pre-fill pulls stock_qty + avg_cost for non-service products ─

    #[tokio::test]
    async fn test_prefill_populates_lines_from_stock() {
        let pool = setup().await;
        let s = create_session(&pool, input()).await.unwrap();
        let lines = prefill_session_lines(&pool, &s.id, "co1").await.unwrap();

        assert_eq!(lines.len(), 2, "only 2 non-service products with stock");
        let a = lines
            .iter()
            .find(|l| l.product_id.as_deref() == Some("p1"))
            .unwrap();
        assert_eq!(a.qty_scriptic, "10.000000");
        assert_eq!(a.unit_price, "5.00");
        assert_eq!(a.value_contabila, "50.00");
        assert_eq!(a.account_code, "371");

        let b = lines
            .iter()
            .find(|l| l.product_id.as_deref() == Some("p2"))
            .unwrap();
        assert_eq!(b.qty_scriptic, "20.000000");
        assert_eq!(b.unit_price, "3.50");
        assert_eq!(b.value_contabila, "70.00");
        assert_eq!(b.account_code, "301");
    }

    // ── Test 2: diff computation — above/below scriptic ──────────────────────

    #[tokio::test]
    async fn test_diff_sign_and_magnitude() {
        let pool = setup().await;
        let s = create_session(&pool, input()).await.unwrap();
        let lines = prefill_session_lines(&pool, &s.id, "co1").await.unwrap();

        let line_a = lines
            .iter()
            .find(|l| l.product_id.as_deref() == Some("p1"))
            .unwrap();
        let line_b = lines
            .iter()
            .find(|l| l.product_id.as_deref() == Some("p2"))
            .unwrap();

        // MINUS: 8 faptic vs 10 scriptic → diff = (8-10)*5 = -10.00
        let minus = update_line_faptic(
            &pool,
            UpdateLineFapticInput {
                line_id: line_a.id.clone(),
                session_id: s.id.clone(),
                company_id: "co1".into(),
                qty_faptic: "8".into(),
                diff_cause: Some("neimputabil".into()),
                imputable: Some(false),
            },
        )
        .await
        .unwrap();
        assert_eq!(minus.value_inventar, "40.00"); // 8 × 5
        assert_eq!(minus.diff_value, "-10.00"); // 40 - 50

        // PLUS: 22 faptic vs 20 scriptic → diff = (22-20)*3.50 = 7.00
        let plus = update_line_faptic(
            &pool,
            UpdateLineFapticInput {
                line_id: line_b.id.clone(),
                session_id: s.id.clone(),
                company_id: "co1".into(),
                qty_faptic: "22".into(),
                diff_cause: None,
                imputable: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(plus.value_inventar, "77.00"); // 22 × 3.50
        assert_eq!(plus.diff_value, "7.00"); // 77 - 70
    }

    // ── Test 3: finalize → registru_inventar_entries with sequential seq_no ─

    #[tokio::test]
    async fn test_finalize_snapshots_registru_entries() {
        let pool = setup().await;
        let s = create_session(&pool, input()).await.unwrap();
        prefill_session_lines(&pool, &s.id, "co1").await.unwrap();

        let fin = finalize_session(&pool, &s.id, "co1").await.unwrap();
        assert_eq!(fin.status, "FINALIZED");

        let entries = list_registru_entries(&pool, "co1", 2026).await.unwrap();
        assert_eq!(entries.len(), 2, "one entry per distinct account_code");
        // Sequential seq_no starting from 1.
        let seq_nos: Vec<i64> = entries.iter().map(|e| e.seq_no).collect();
        assert!(seq_nos.contains(&1));
        assert!(seq_nos.contains(&2));

        // All entries reference this session.
        assert!(entries
            .iter()
            .all(|e| e.source_session_id.as_deref() == Some(&s.id)));
    }

    // ── Test 4: finalized session rejects edits ───────────────────────────────

    #[tokio::test]
    async fn test_finalized_session_rejects_edits() {
        let pool = setup().await;
        let s = create_session(&pool, input()).await.unwrap();
        let lines = prefill_session_lines(&pool, &s.id, "co1").await.unwrap();
        finalize_session(&pool, &s.id, "co1").await.unwrap();

        let line_a = &lines[0];
        let result = update_line_faptic(
            &pool,
            UpdateLineFapticInput {
                line_id: line_a.id.clone(),
                session_id: s.id.clone(),
                company_id: "co1".into(),
                qty_faptic: "5".into(),
                diff_cause: None,
                imputable: None,
            },
        )
        .await;
        assert!(matches!(result, Err(AppError::Validation(_))));

        // Finalized session cannot be deleted.
        let del = delete_session(&pool, &s.id, "co1").await;
        assert!(matches!(del, Err(AppError::Validation(_))));

        // Re-prefill also rejected.
        let reprefill = prefill_session_lines(&pool, &s.id, "co1").await;
        assert!(matches!(reprefill, Err(AppError::Validation(_))));
    }

    // ── Test 5: GL posting — minus neimputabil → D 607 = C 371 (balanced) ───

    #[tokio::test]
    async fn test_gl_post_minus_d607_c371_balanced() {
        let pool = setup().await;
        let s = create_session(&pool, input()).await.unwrap();
        let lines = prefill_session_lines(&pool, &s.id, "co1").await.unwrap();
        let line_a = lines
            .iter()
            .find(|l| l.product_id.as_deref() == Some("p1"))
            .unwrap();

        // MINUS 2 units → diff -10.00, account 371.
        update_line_faptic(
            &pool,
            UpdateLineFapticInput {
                line_id: line_a.id.clone(),
                session_id: s.id.clone(),
                company_id: "co1".into(),
                qty_faptic: "8".into(),
                diff_cause: Some("neimputabil".into()),
                imputable: Some(false),
            },
        )
        .await
        .unwrap();

        // Zero out line_b diff (set faptic = scriptic).
        let line_b = lines
            .iter()
            .find(|l| l.product_id.as_deref() == Some("p2"))
            .unwrap();
        update_line_faptic(
            &pool,
            UpdateLineFapticInput {
                line_id: line_b.id.clone(),
                session_id: s.id.clone(),
                company_id: "co1".into(),
                qty_faptic: "20".into(), // no diff
                diff_cause: None,
                imputable: None,
            },
        )
        .await
        .unwrap();

        finalize_session(&pool, &s.id, "co1").await.unwrap();
        post_inventory_diffs(&pool, &s.id, "co1").await.unwrap();

        // Verify GL: D 607 10.00 = C 371 10.00.
        let tb = crate::db::gl::trial_balance(&pool, "co1", "2026-01-01", "2026-12-31")
            .await
            .unwrap();
        let row = |code: &str| {
            tb.rows
                .iter()
                .find(|r| r.account_code == code)
                .map(|r| (r.closing_debit.clone(), r.closing_credit.clone()))
        };
        assert_eq!(
            row("607"),
            Some(("10.00".into(), "0.00".into())),
            "D 607 = 10"
        );
        assert_eq!(
            row("371"),
            Some(("0.00".into(), "10.00".into())),
            "C 371 = 10"
        );
        assert!(tb.balanced, "GL must be balanced");
    }

    // ── Test 6: GL posting — plus → D 371 = C 607 (balanced) ────────────────

    #[tokio::test]
    async fn test_gl_post_plus_d371_c607_balanced() {
        let pool = setup().await;
        let s = create_session(&pool, input()).await.unwrap();
        let lines = prefill_session_lines(&pool, &s.id, "co1").await.unwrap();

        // Zero diff on line_a (no impact).
        let line_a = lines
            .iter()
            .find(|l| l.product_id.as_deref() == Some("p1"))
            .unwrap();
        update_line_faptic(
            &pool,
            UpdateLineFapticInput {
                line_id: line_a.id.clone(),
                session_id: s.id.clone(),
                company_id: "co1".into(),
                qty_faptic: "10".into(), // no diff
                diff_cause: None,
                imputable: None,
            },
        )
        .await
        .unwrap();

        // PLUS 5 units on line_b (account 301): diff = +5 * 3.50 = 17.50
        let line_b = lines
            .iter()
            .find(|l| l.product_id.as_deref() == Some("p2"))
            .unwrap();
        update_line_faptic(
            &pool,
            UpdateLineFapticInput {
                line_id: line_b.id.clone(),
                session_id: s.id.clone(),
                company_id: "co1".into(),
                qty_faptic: "25".into(),
                diff_cause: None,
                imputable: None,
            },
        )
        .await
        .unwrap();

        finalize_session(&pool, &s.id, "co1").await.unwrap();
        post_inventory_diffs(&pool, &s.id, "co1").await.unwrap();

        let tb = crate::db::gl::trial_balance(&pool, "co1", "2026-01-01", "2026-12-31")
            .await
            .unwrap();
        let row = |code: &str| {
            tb.rows
                .iter()
                .find(|r| r.account_code == code)
                .map(|r| (r.closing_debit.clone(), r.closing_credit.clone()))
        };
        // D 301 = C 607  (17.50)
        assert_eq!(
            row("301"),
            Some(("17.50".into(), "0.00".into())),
            "D 301 = 17.50"
        );
        assert_eq!(
            row("607"),
            Some(("0.00".into(), "17.50".into())),
            "C 607 = 17.50"
        );
        assert!(tb.balanced);
    }

    // ── Test 7: GL posting survives generate_gl_entries (source_type not wiped) ─

    #[tokio::test]
    async fn test_inventory_gl_not_wiped_by_generate() {
        let pool = setup().await;
        let s = create_session(&pool, input()).await.unwrap();
        let lines = prefill_session_lines(&pool, &s.id, "co1").await.unwrap();
        let line_a = lines
            .iter()
            .find(|l| l.product_id.as_deref() == Some("p1"))
            .unwrap();
        update_line_faptic(
            &pool,
            UpdateLineFapticInput {
                line_id: line_a.id.clone(),
                session_id: s.id.clone(),
                company_id: "co1".into(),
                qty_faptic: "8".into(),
                diff_cause: Some("neimputabil".into()),
                imputable: Some(false),
            },
        )
        .await
        .unwrap();
        finalize_session(&pool, &s.id, "co1").await.unwrap();
        post_inventory_diffs(&pool, &s.id, "co1").await.unwrap();

        // Run generate_gl_entries — should NOT delete INVENTORY journals.
        crate::db::gl::generate_gl_entries(&pool, "co1", "2026-01-01", "2026-12-31")
            .await
            .unwrap();

        let tb = crate::db::gl::trial_balance(&pool, "co1", "2026-01-01", "2026-12-31")
            .await
            .unwrap();
        let row = |code: &str| {
            tb.rows
                .iter()
                .find(|r| r.account_code == code)
                .map(|r| r.closing_debit.clone())
        };
        assert_eq!(
            row("607"),
            Some("10.00".into()),
            "INVENTORY GL entry persists after generate_gl_entries"
        );
    }

    // ── Test 8: Decimal precision — exact 2dp values ──────────────────────────

    #[tokio::test]
    async fn test_decimal_precision_exact_2dp() {
        let pool = setup().await;
        let s = create_session(&pool, input()).await.unwrap();
        let lines = prefill_session_lines(&pool, &s.id, "co1").await.unwrap();
        let line_b = lines
            .iter()
            .find(|l| l.product_id.as_deref() == Some("p2"))
            .unwrap();

        // 21 units @ 3.50 → value_inventar = 73.50 (exact, no float error)
        let upd = update_line_faptic(
            &pool,
            UpdateLineFapticInput {
                line_id: line_b.id.clone(),
                session_id: s.id.clone(),
                company_id: "co1".into(),
                qty_faptic: "21".into(),
                diff_cause: None,
                imputable: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(upd.value_inventar, "73.50");
        assert_eq!(upd.diff_value, "3.50"); // 73.50 - 70.00
    }

    // ── Test 9: second finalize → sequential seq_no continues ────────────────

    #[tokio::test]
    async fn test_second_session_seq_no_continues() {
        let pool = setup().await;

        // First session → seq 1, 2.
        let s1 = create_session(&pool, input()).await.unwrap();
        prefill_session_lines(&pool, &s1.id, "co1").await.unwrap();
        finalize_session(&pool, &s1.id, "co1").await.unwrap();

        // Second session (mid-year) → seq 3, 4.
        let s2 = create_session(
            &pool,
            CreateSessionInput {
                reference_date: "2026-06-30".into(),
                r#type: Some("CALAMITATE".into()),
                ..input()
            },
        )
        .await
        .unwrap();
        prefill_session_lines(&pool, &s2.id, "co1").await.unwrap();
        finalize_session(&pool, &s2.id, "co1").await.unwrap();

        let entries = list_registru_entries(&pool, "co1", 2026).await.unwrap();
        assert_eq!(entries.len(), 4);
        let seq_nos: Vec<i64> = entries.iter().map(|e| e.seq_no).collect();
        assert!(seq_nos.contains(&3));
        assert!(seq_nos.contains(&4));
    }
}
