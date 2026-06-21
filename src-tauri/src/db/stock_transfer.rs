//! Bon de transfer inter-gestiune (formular 14-3-3A, OMFP 2634/2015).
//!
//! Un transfer mută stocul aceluiași produs dintr-o gestiune sursă (A) într-o gestiune destinație
//! (B) la costul evaluat de motorul FIFO/LIFO/CMP al lui A, fără nicio notă contabilă sintetică.
//!
//! **Neutralitate GL**: rândurile stock_ledger generate de un transfer au doc_type='TRANSFER'.
//! Funcția `post_stock_movement` din gl.rs tratează aceste rânduri ca GL-neutre: șterge orice
//! jurnal STOCK anterior pentru id-ul respectiv și returnează fără a posta noi înregistrări.
//! Invariantul se menține și la recompute (replay-ul trece is_transfer=true la reluare).
//!
//! **Valoare păstrată**: costul TOTAL evaluat la ieșire din A este transferat integral în B
//! (unit_cost = value_out / qty, cu rotunjire la 2 zecimale). Aceasta este o simplificare MVP
//! acceptată (granularitatea per-lot FIFO nu se transferă; valoarea totală este conservată).
//!
//! **Stocul total**: qty_A scade cu qty, qty_B crește cu qty → totalul produsului rămâne egal.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

use crate::db::invoices::round2;
use crate::db::models::{new_id, now_unix};
use crate::db::stock_valuation::{self, Dir, StockMovementInput};
use crate::error::{AppError, AppResult};

// ─── Types ────────────────────────────────────────────────────────────────────

/// Input for a stock transfer command.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferInput {
    pub product_id: String,
    pub from_gestiune_id: String,
    pub to_gestiune_id: String,
    pub transfer_date: String,
    pub qty: String,
    pub transfer_ref: Option<String>,
    pub notes: Option<String>,
}

/// Persisted transfer record (read-model).
#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct StockTransfer {
    pub id: String,
    pub company_id: String,
    pub product_id: String,
    pub from_gestiune_id: String,
    pub to_gestiune_id: String,
    pub transfer_date: String,
    pub qty: String,
    pub unit_cost: String,
    pub value: String,
    pub transfer_ref: Option<String>,
    pub notes: Option<String>,
    pub created_at: i64,
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s.trim()).unwrap_or(Decimal::ZERO)
}

/// Return the current on-hand qty for `product_id` in `gestiune_id`.
/// This is the run_qty of the chronologically-last stock_ledger row for that gestiune.
async fn on_hand_qty(
    pool: &SqlitePool,
    company_id: &str,
    product_id: &str,
    gestiune_id: &str,
) -> AppResult<Decimal> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT sl.run_qty FROM stock_ledger sl \
         WHERE sl.company_id=?1 AND sl.product_id=?2 AND sl.gestiune_id=?3 \
           AND NOT EXISTS ( \
             SELECT 1 FROM stock_ledger s2 \
             WHERE s2.company_id=sl.company_id AND s2.product_id=sl.product_id \
               AND s2.gestiune_id=sl.gestiune_id \
               AND ( s2.entry_date > sl.entry_date \
                  OR (s2.entry_date = sl.entry_date AND s2.seq > sl.seq) \
                  OR (s2.entry_date = sl.entry_date AND s2.seq = sl.seq AND s2.created_at > sl.created_at) \
                  OR (s2.entry_date = sl.entry_date AND s2.seq = sl.seq AND s2.created_at = sl.created_at AND s2.rowid > sl.rowid) ) \
           ) \
         LIMIT 1",
    )
    .bind(company_id)
    .bind(product_id)
    .bind(gestiune_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(rq,)| dec(&rq)).unwrap_or(Decimal::ZERO))
}

// ─── Core operation ───────────────────────────────────────────────────────────

/// Execute an inter-gestiune transfer for one product.
///
/// Guarantees:
/// - from ≠ to (else Validation error)
/// - both gestiuni owned by company
/// - product owned by company
/// - qty > 0
/// - on-hand in from_gestiune ≥ qty (Validation error "stoc insuficient în gestiunea sursă")
/// - OUT is valued by the FIFO/LIFO/CMP engine of from_gestiune
/// - IN into to_gestiune carries the EXACT same value (cost-preserving, no transfer gain/loss)
/// - NO GL entries are posted (transfers are GL-neutral; doc_type='TRANSFER' skips post_stock_movement)
pub async fn transfer_stock(
    pool: &SqlitePool,
    company_id: &str,
    input: TransferInput,
) -> AppResult<StockTransfer> {
    // ── Validate ──────────────────────────────────────────────────────────────

    if input.from_gestiune_id == input.to_gestiune_id {
        return Err(AppError::Validation(
            "Gestiunea sursă și gestiunea destinație nu pot fi identice.".into(),
        ));
    }

    let qty = Decimal::from_str(input.qty.trim())
        .map_err(|_| AppError::Validation("Cantitate invalidă.".into()))?;
    if qty <= Decimal::ZERO {
        return Err(AppError::Validation(
            "Cantitatea transferată trebuie să fie > 0.".into(),
        ));
    }

    // Verify product ownership (also checks company).
    stock_valuation::assert_product_owned(pool, company_id, &input.product_id).await?;

    // Verify both gestiuni belong to this company.
    let from_exists: Option<String> =
        sqlx::query_scalar("SELECT id FROM gestiune WHERE id=?1 AND company_id=?2")
            .bind(&input.from_gestiune_id)
            .bind(company_id)
            .fetch_optional(pool)
            .await?;
    if from_exists.is_none() {
        return Err(AppError::NotFound);
    }

    let to_exists: Option<String> =
        sqlx::query_scalar("SELECT id FROM gestiune WHERE id=?1 AND company_id=?2")
            .bind(&input.to_gestiune_id)
            .bind(company_id)
            .fetch_optional(pool)
            .await?;
    if to_exists.is_none() {
        return Err(AppError::NotFound);
    }

    // Auto-vindecare: înainte de a calcula stocul disponibil, recuperăm orice transfer incomplet
    // (crash între mișcări și document) — altfel un OUT orfan ar subevalua stocul disponibil aici.
    recover_incomplete_transfers(pool, company_id).await?;

    // On-hand guard: reject if from_gestiune has insufficient stock.
    let available =
        on_hand_qty(pool, company_id, &input.product_id, &input.from_gestiune_id).await?;
    if available < qty {
        return Err(AppError::Validation(format!(
            "Stoc insuficient în gestiunea sursă: disponibil {:.6}, solicitat {:.6}.",
            available, qty
        )));
    }

    // ── Allocate transfer id ──────────────────────────────────────────────────

    let transfer_id = new_id();

    // ── OUT from from_gestiune (engine-valued COGS) ───────────────────────────
    //
    // The valuation engine (FIFO/LIFO/CMP) assigns the cost of the OUT. We then
    // read the resulting `value` from the freshly-updated ledger row to carry it
    // into the IN on the receiving gestiune.

    let out_input = StockMovementInput {
        company_id: company_id.to_string(),
        product_id: input.product_id.clone(),
        entry_date: input.transfer_date.clone(),
        qty: format!("{:.6}", qty),
        unit_cost: None, // OUT: cost assigned by engine
        doc_type: Some("TRANSFER".to_string()),
        doc_ref: Some(transfer_id.clone()),
        gestiune_id: Some(input.from_gestiune_id.clone()),
    };

    stock_valuation::record_movement(pool, &out_input, Dir::Out).await?;

    // Find the OUT ledger row for this transfer to get the engine-assigned value.
    let out_row: Option<(String, String)> = sqlx::query_as(
        "SELECT id, value FROM stock_ledger \
         WHERE company_id=?1 AND product_id=?2 AND gestiune_id=?3 \
           AND doc_type='TRANSFER' AND doc_ref=?4 AND direction='OUT' \
         LIMIT 1",
    )
    .bind(company_id)
    .bind(&input.product_id)
    .bind(&input.from_gestiune_id)
    .bind(&transfer_id)
    .fetch_optional(pool)
    .await?;

    let (_out_ledger_id, out_value_str) = match out_row {
        Some(r) => r,
        None => {
            let _ = rollback_transfer_by_ref(pool, company_id, &transfer_id).await;
            return Err(AppError::Validation(
                "Eroare internă: rândul OUT din transfer nu a fost găsit.".into(),
            ));
        }
    };
    let out_value = dec(&out_value_str);

    // unit_cost = value / qty (value-preserving; rounding at 2 dp is the accepted MVP simplification)
    let unit_cost = if qty.is_zero() {
        Decimal::ZERO
    } else {
        round2(out_value / qty)
    };

    // ── IN into to_gestiune (at the cost from A) ──────────────────────────────
    //
    // MVP simplification: we use the single weighted unit_cost for the IN layer.
    // This preserves the TOTAL VALUE (out_value = qty × unit_cost ± 1c rounding),
    // but does not transfer individual FIFO lots. Documented tradeoff.

    let in_input = StockMovementInput {
        company_id: company_id.to_string(),
        product_id: input.product_id.clone(),
        entry_date: input.transfer_date.clone(),
        qty: format!("{:.6}", qty),
        unit_cost: Some(format!("{:.2}", unit_cost)),
        doc_type: Some("TRANSFER".to_string()),
        doc_ref: Some(transfer_id.clone()),
        gestiune_id: Some(input.to_gestiune_id.clone()),
    };

    if let Err(e) = stock_valuation::record_movement(pool, &in_input, Dir::In).await {
        // Compensare: anulează OUT-ul deja comis ca să nu dispară stoc (OUT fără IN).
        let _ = rollback_transfer_by_ref(pool, company_id, &transfer_id).await;
        return Err(e);
    }

    // ── Persist the stock_transfers record ────────────────────────────────────

    let transfer = StockTransfer {
        id: transfer_id.clone(),
        company_id: company_id.to_string(),
        product_id: input.product_id.clone(),
        from_gestiune_id: input.from_gestiune_id.clone(),
        to_gestiune_id: input.to_gestiune_id.clone(),
        transfer_date: input.transfer_date.clone(),
        qty: format!("{:.6}", qty),
        unit_cost: format!("{:.2}", unit_cost),
        value: format!("{:.2}", out_value),
        transfer_ref: input.transfer_ref.clone(),
        notes: input.notes.clone(),
        created_at: now_unix(),
    };

    let insert_res = sqlx::query(
        "INSERT INTO stock_transfers \
         (id, company_id, product_id, from_gestiune_id, to_gestiune_id, transfer_date, \
          qty, unit_cost, value, transfer_ref, notes, created_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
    )
    .bind(&transfer.id)
    .bind(&transfer.company_id)
    .bind(&transfer.product_id)
    .bind(&transfer.from_gestiune_id)
    .bind(&transfer.to_gestiune_id)
    .bind(&transfer.transfer_date)
    .bind(&transfer.qty)
    .bind(&transfer.unit_cost)
    .bind(&transfer.value)
    .bind(&transfer.transfer_ref)
    .bind(&transfer.notes)
    .bind(transfer.created_at)
    .execute(pool)
    .await;
    if let Err(e) = insert_res {
        // Compensare: anulează AMBELE mișcări (OUT + IN) ca să nu rămână un transfer fără document.
        let _ = rollback_transfer_by_ref(pool, company_id, &transfer_id).await;
        return Err(e.into());
    }

    Ok(transfer)
}

/// Anulează TOATE mișcările de stoc ale unui transfer (doc_ref) și recalculează fiecare (produs,
/// gestiune) afectat — derivat din rândurile existente, deci funcționează și fără a cunoaște dinainte
/// gestiunile (folosit atât la compensarea în-flight cât și la recuperarea la pornire). `record_movement`
/// se auto-comite per apel, deci un OUT comis urmat de un IN eșuat ar lăsa stoc dispărut (OUT fără IN);
/// această anulare readuce stocul total la valoarea inițială. Transferurile sunt GL-neutre
/// (post_stock_movement sare peste GL pentru doc_type='TRANSFER'), deci nu există note GL 'STOCK' de șters.
/// Best-effort la compensare: eroarea originală (cauza) e cea propagată.
async fn rollback_transfer_by_ref(
    pool: &SqlitePool,
    company_id: &str,
    doc_ref: &str,
) -> AppResult<()> {
    let affected: Vec<(String, String)> = sqlx::query_as(
        "SELECT DISTINCT product_id, gestiune_id FROM stock_ledger \
         WHERE company_id=?1 AND doc_ref=?2",
    )
    .bind(company_id)
    .bind(doc_ref)
    .fetch_all(pool)
    .await?;
    if affected.is_empty() {
        return Ok(());
    }
    sqlx::query("DELETE FROM stock_ledger WHERE company_id=?1 AND doc_ref=?2")
        .bind(company_id)
        .bind(doc_ref)
        .execute(pool)
        .await?;
    for (pid, gid) in &affected {
        stock_valuation::recompute_product_gestiune(pool, company_id, pid, gid).await?;
    }
    Ok(())
}

/// Auto-vindecare a transferurilor întrerupte. Orice mișcare de stoc cu doc_type='TRANSFER' al cărei
/// `doc_ref` NU are un rând corespunzător în `stock_transfers` reprezintă un transfer incomplet (crash
/// sau pană de curent între comiterea mișcărilor și înregistrarea documentului — fereastra pe care
/// compensarea în-proces nu o poate acoperi). Le anulăm, restabilind stocul total. Idempotentă; o rulăm
/// la începutul fiecărui transfer (și poate fi apelată și la pornirea aplicației). Returnează câte
/// transferuri incomplete au fost recuperate.
pub async fn recover_incomplete_transfers(pool: &SqlitePool, company_id: &str) -> AppResult<usize> {
    let orphans: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT doc_ref FROM stock_ledger \
         WHERE company_id=?1 AND doc_type='TRANSFER' AND doc_ref IS NOT NULL \
           AND doc_ref NOT IN (SELECT id FROM stock_transfers WHERE company_id=?1)",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    for (doc_ref,) in &orphans {
        rollback_transfer_by_ref(pool, company_id, doc_ref).await?;
    }
    Ok(orphans.len())
}

// ─── Queries ─────────────────────────────────────────────────────────────────

/// List all transfers for a company, newest first.
pub async fn list_transfers(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<StockTransfer>> {
    Ok(sqlx::query_as::<_, StockTransfer>(
        "SELECT id, company_id, product_id, from_gestiune_id, to_gestiune_id, transfer_date, \
         qty, unit_cost, value, transfer_ref, notes, created_at \
         FROM stock_transfers WHERE company_id=?1 ORDER BY transfer_date DESC, created_at DESC",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?)
}

/// Get a single transfer (multi-tenant guard).
pub async fn get_transfer(
    pool: &SqlitePool,
    company_id: &str,
    id: &str,
) -> AppResult<StockTransfer> {
    sqlx::query_as::<_, StockTransfer>(
        "SELECT id, company_id, product_id, from_gestiune_id, to_gestiune_id, transfer_date, \
         qty, unit_cost, value, transfer_ref, notes, created_at \
         FROM stock_transfers WHERE id=?1 AND company_id=?2",
    )
    .bind(id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::gestiune::{self, GestiuneInput};
    use crate::db::stock_valuation::{record_movement, Dir, StockMovementInput};
    use sqlx::SqlitePool;

    /// Spin up an in-memory DB, seed company + product, return (pool, company_id, product_id).
    async fn setup(method: &str) -> (SqlitePool, String, String) {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        let cid = "tr_co".to_string();
        let pid = format!("tr_p_{method}");

        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES (?1,'55555555','Transfer SRL','S','C','CJ','RO')",
        )
        .bind(&cid)
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO products (id, company_id, name, unit, valuation_method) \
             VALUES (?1,?2,'Marfa transfer','buc',?3)",
        )
        .bind(&pid)
        .bind(&cid)
        .bind(method)
        .execute(&pool)
        .await
        .unwrap();

        (pool, cid, pid)
    }

    async fn make_gestiune(pool: &SqlitePool, cid: &str, cod: &str) -> String {
        gestiune::create(
            pool,
            cid,
            GestiuneInput {
                cod: cod.to_string(),
                denumire: format!("Gestiune {cod}"),
                tip: None,
                metoda_evaluare: None,
                cont_stoc: None,
                adresa: None,
                dispersata_teritorial: None,
            },
        )
        .await
        .unwrap()
        .id
    }

    fn mv(
        cid: &str,
        pid: &str,
        date: &str,
        qty: &str,
        cost: Option<&str>,
        gid: &str,
    ) -> StockMovementInput {
        StockMovementInput {
            company_id: cid.to_string(),
            product_id: pid.to_string(),
            entry_date: date.to_string(),
            qty: qty.to_string(),
            unit_cost: cost.map(|c| c.to_string()),
            doc_type: None,
            doc_ref: None,
            gestiune_id: Some(gid.to_string()),
        }
    }

    /// Read on-hand qty for a product+gestiune from stock_ledger (last chrono row).
    async fn onhand(pool: &SqlitePool, cid: &str, pid: &str, gid: &str) -> Decimal {
        on_hand_qty(pool, cid, pid, gid).await.unwrap()
    }

    /// Count STOCK GL journal entries for a company+date range (all accounts).
    async fn gl_stock_count(pool: &SqlitePool, cid: &str) -> i64 {
        let row: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM gl_journal j \
             JOIN gl_entry e ON e.journal_pk = j.id \
             WHERE j.company_id=?1 AND j.source_type='STOCK' \
               AND j.source_id IN ( \
                 SELECT id FROM stock_ledger WHERE company_id=?1 AND doc_type='TRANSFER' \
               )",
        )
        .bind(cid)
        .fetch_one(pool)
        .await
        .unwrap();
        row.0
    }

    /// Sum of 607 debit+credit from STOCK journals for transfer ledger rows.
    async fn transfer_607_turnover(pool: &SqlitePool, cid: &str) -> (Decimal, Decimal) {
        // SUM always returns one row (possibly NULLs if no matching rows).
        let row: (Option<f64>, Option<f64>) = sqlx::query_as(
            "SELECT SUM(e.debit), SUM(e.credit) \
             FROM gl_journal j \
             JOIN gl_entry e ON e.journal_pk = j.id \
             WHERE j.company_id=?1 AND j.source_type='STOCK' \
               AND e.account_code LIKE '6%' \
               AND j.source_id IN ( \
                 SELECT id FROM stock_ledger WHERE company_id=?1 AND doc_type='TRANSFER' \
               )",
        )
        .bind(cid)
        .fetch_one(pool)
        .await
        .unwrap();

        fn d(v: Option<f64>) -> Decimal {
            Decimal::try_from(v.unwrap_or(0.0))
                .unwrap_or(Decimal::ZERO)
                .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
        }
        (d(row.0), d(row.1))
    }

    // ── Test 1: qty moves correctly ───────────────────────────────────────────

    #[tokio::test]
    async fn transfer_moves_qty_correctly() {
        let (pool, cid, pid) = setup("FIFO").await;
        let g_a = make_gestiune(&pool, &cid, "A").await;
        let g_b = make_gestiune(&pool, &cid, "B").await;

        // Seed 20 units in A at 5.00
        record_movement(
            &pool,
            &mv(&cid, &pid, "2026-01-01", "20", Some("5.00"), &g_a),
            Dir::In,
        )
        .await
        .unwrap();

        assert_eq!(onhand(&pool, &cid, &pid, &g_a).await, dec("20"));
        assert_eq!(onhand(&pool, &cid, &pid, &g_b).await, dec("0"));

        // Transfer 8 units A → B
        let t = transfer_stock(
            &pool,
            &cid,
            TransferInput {
                product_id: pid.clone(),
                from_gestiune_id: g_a.clone(),
                to_gestiune_id: g_b.clone(),
                transfer_date: "2026-01-05".into(),
                qty: "8".into(),
                transfer_ref: None,
                notes: None,
            },
        )
        .await
        .unwrap();

        // A decreases by 8, B increases by 8, total unchanged.
        assert_eq!(
            onhand(&pool, &cid, &pid, &g_a).await,
            dec("12"),
            "A: 20-8=12"
        );
        assert_eq!(onhand(&pool, &cid, &pid, &g_b).await, dec("8"), "B: 0+8=8");

        // Total product on-hand across all gestiuni
        let (tq, _): (Option<String>, Option<String>) = sqlx::query_as(
            "SELECT stock_qty, stock_value FROM products WHERE id=?1 AND company_id=?2",
        )
        .bind(&pid)
        .bind(&cid)
        .fetch_one(&pool)
        .await
        .unwrap();
        let total_qty: Decimal = dec(tq.as_deref().unwrap_or("0"));
        assert_eq!(total_qty, dec("20"), "Total qty unchanged at 20");

        // Transfer record persisted
        assert_eq!(t.qty, "8.000000");
    }

    // ── Test 2: cost preserved A → B ─────────────────────────────────────────

    #[tokio::test]
    async fn transfer_preserves_cost() {
        let (pool, cid, pid) = setup("CMP").await;
        let g_a = make_gestiune(&pool, &cid, "A").await;
        let g_b = make_gestiune(&pool, &cid, "B").await;

        // 10 @ 4.00 + 10 @ 6.00 → CMP = 5.00
        record_movement(
            &pool,
            &mv(&cid, &pid, "2026-01-01", "10", Some("4.00"), &g_a),
            Dir::In,
        )
        .await
        .unwrap();
        record_movement(
            &pool,
            &mv(&cid, &pid, "2026-01-02", "10", Some("6.00"), &g_a),
            Dir::In,
        )
        .await
        .unwrap();

        let t = transfer_stock(
            &pool,
            &cid,
            TransferInput {
                product_id: pid.clone(),
                from_gestiune_id: g_a.clone(),
                to_gestiune_id: g_b.clone(),
                transfer_date: "2026-01-10".into(),
                qty: "4".into(),
                transfer_ref: Some("BON-001".into()),
                notes: None,
            },
        )
        .await
        .unwrap();

        // CMP cost = (40+60)/20 = 5.00; value = 4 × 5.00 = 20.00
        assert_eq!(t.unit_cost, "5.00", "unit_cost = CMP 5.00");
        assert_eq!(t.value, "20.00", "value = 4 × 5.00 = 20.00");

        // IN into B carries the same cost
        let in_row: (String, String) = sqlx::query_as(
            "SELECT unit_cost, value FROM stock_ledger \
             WHERE company_id=?1 AND product_id=?2 AND gestiune_id=?3 \
               AND doc_type='TRANSFER' AND direction='IN' LIMIT 1",
        )
        .bind(&cid)
        .bind(&pid)
        .bind(&g_b)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(in_row.0, "5.00", "IN unit_cost in B = 5.00");
        assert_eq!(in_row.1, "20.00", "IN value in B = 20.00");
    }

    // ── Test 3: GL-NEUTRAL (the critical test) ────────────────────────────────

    #[tokio::test]
    async fn transfer_is_gl_neutral() {
        let (pool, cid, pid) = setup("FIFO").await;
        let g_a = make_gestiune(&pool, &cid, "A").await;
        let g_b = make_gestiune(&pool, &cid, "B").await;

        // Seed + transfer
        record_movement(
            &pool,
            &mv(&cid, &pid, "2026-02-01", "10", Some("3.50"), &g_a),
            Dir::In,
        )
        .await
        .unwrap();
        transfer_stock(
            &pool,
            &cid,
            TransferInput {
                product_id: pid.clone(),
                from_gestiune_id: g_a.clone(),
                to_gestiune_id: g_b.clone(),
                transfer_date: "2026-02-05".into(),
                qty: "5".into(),
                transfer_ref: None,
                notes: None,
            },
        )
        .await
        .unwrap();

        // No STOCK GL journals should exist for TRANSFER ledger rows.
        let count = gl_stock_count(&pool, &cid).await;
        assert_eq!(
            count, 0,
            "Transfer must post ZERO GL entries (count={count})"
        );

        // 6xx (607) turnover attributable to transfers must be zero.
        let (d607, c607) = transfer_607_turnover(&pool, &cid).await;
        assert_eq!(d607, Decimal::ZERO, "607 debit from transfers must be 0");
        assert_eq!(c607, Decimal::ZERO, "607 credit from transfers must be 0");
    }

    // ── Test 4: insufficient on-hand → rejected ───────────────────────────────

    #[tokio::test]
    async fn transfer_rejects_insufficient_stock() {
        let (pool, cid, pid) = setup("FIFO").await;
        let g_a = make_gestiune(&pool, &cid, "A").await;
        let g_b = make_gestiune(&pool, &cid, "B").await;

        record_movement(
            &pool,
            &mv(&cid, &pid, "2026-03-01", "5", Some("10.00"), &g_a),
            Dir::In,
        )
        .await
        .unwrap();

        let err = transfer_stock(
            &pool,
            &cid,
            TransferInput {
                product_id: pid.clone(),
                from_gestiune_id: g_a.clone(),
                to_gestiune_id: g_b.clone(),
                transfer_date: "2026-03-05".into(),
                qty: "10".into(), // more than available (5)
                transfer_ref: None,
                notes: None,
            },
        )
        .await;

        assert!(
            matches!(err, Err(AppError::Validation(_))),
            "Should reject over-transfer with Validation error, got: {err:?}"
        );
        // On-hand in A unchanged
        assert_eq!(
            onhand(&pool, &cid, &pid, &g_a).await,
            dec("5"),
            "A unchanged after rejection"
        );
    }

    // ── Test 5: from == to → rejected ────────────────────────────────────────

    #[tokio::test]
    async fn transfer_rejects_same_gestiune() {
        let (pool, cid, pid) = setup("CMP").await;
        let g_a = make_gestiune(&pool, &cid, "A").await;

        let err = transfer_stock(
            &pool,
            &cid,
            TransferInput {
                product_id: pid.clone(),
                from_gestiune_id: g_a.clone(),
                to_gestiune_id: g_a.clone(),
                transfer_date: "2026-04-01".into(),
                qty: "1".into(),
                transfer_ref: None,
                notes: None,
            },
        )
        .await;
        assert!(
            matches!(err, Err(AppError::Validation(_))),
            "from==to must be rejected"
        );
    }

    // ── Test 6: sale from B uses transferred cost ─────────────────────────────

    #[tokio::test]
    async fn sale_from_destination_uses_transferred_cost_fifo() {
        let (pool, cid, pid) = setup("FIFO").await;
        let g_a = make_gestiune(&pool, &cid, "A").await;
        let g_b = make_gestiune(&pool, &cid, "B").await;

        // Seed A: 10 @ 7.00
        record_movement(
            &pool,
            &mv(&cid, &pid, "2026-05-01", "10", Some("7.00"), &g_a),
            Dir::In,
        )
        .await
        .unwrap();

        // Transfer 6 units A → B
        transfer_stock(
            &pool,
            &cid,
            TransferInput {
                product_id: pid.clone(),
                from_gestiune_id: g_a.clone(),
                to_gestiune_id: g_b.clone(),
                transfer_date: "2026-05-05".into(),
                qty: "6".into(),
                transfer_ref: None,
                notes: None,
            },
        )
        .await
        .unwrap();

        // Sell 3 from B
        record_movement(
            &pool,
            &mv(&cid, &pid, "2026-05-10", "3", None, &g_b),
            Dir::Out,
        )
        .await
        .unwrap();

        // COGS from B should be at the transferred cost (7.00) → 3 × 7 = 21.00
        let out_row: (String, String) = sqlx::query_as(
            "SELECT unit_cost, value FROM stock_ledger \
             WHERE company_id=?1 AND product_id=?2 AND gestiune_id=?3 \
               AND direction='OUT' AND (doc_type IS NULL OR doc_type != 'TRANSFER') \
             ORDER BY entry_date DESC LIMIT 1",
        )
        .bind(&cid)
        .bind(&pid)
        .bind(&g_b)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(
            out_row.0, "7.00",
            "COGS unit_cost from B = 7.00 (transferred cost)"
        );
        assert_eq!(out_row.1, "21.00", "COGS value = 3 × 7 = 21.00");
    }

    // ── Test 7: regen safety — transfers stay GL-neutral after recompute ──────

    #[tokio::test]
    async fn transfer_stays_gl_neutral_after_recompute() {
        let (pool, cid, pid) = setup("CMP").await;
        let g_a = make_gestiune(&pool, &cid, "A").await;
        let g_b = make_gestiune(&pool, &cid, "B").await;

        record_movement(
            &pool,
            &mv(&cid, &pid, "2026-06-01", "10", Some("6.00"), &g_a),
            Dir::In,
        )
        .await
        .unwrap();
        transfer_stock(
            &pool,
            &cid,
            TransferInput {
                product_id: pid.clone(),
                from_gestiune_id: g_a.clone(),
                to_gestiune_id: g_b.clone(),
                transfer_date: "2026-06-05".into(),
                qty: "3".into(),
                transfer_ref: None,
                notes: None,
            },
        )
        .await
        .unwrap();

        // Manually trigger recompute for both gestiuni (as would happen on a backdated insertion).
        crate::db::stock_valuation::recompute_product_gestiune(&pool, &cid, &pid, &g_a)
            .await
            .unwrap();
        crate::db::stock_valuation::recompute_product_gestiune(&pool, &cid, &pid, &g_b)
            .await
            .unwrap();

        // After recompute, no GL entries for TRANSFER rows.
        let count = gl_stock_count(&pool, &cid).await;
        assert_eq!(
            count, 0,
            "After recompute, transfers must still post ZERO GL entries (count={count})"
        );
    }

    // ── Test 8: total product on-hand unchanged after transfer ────────────────

    #[tokio::test]
    async fn total_on_hand_unchanged_after_transfer() {
        let (pool, cid, pid) = setup("LIFO").await;
        let g_a = make_gestiune(&pool, &cid, "A").await;
        let g_b = make_gestiune(&pool, &cid, "B").await;

        record_movement(
            &pool,
            &mv(&cid, &pid, "2026-07-01", "15", Some("8.00"), &g_a),
            Dir::In,
        )
        .await
        .unwrap();

        // Get total before transfer
        let before: (Option<String>,) =
            sqlx::query_as("SELECT stock_qty FROM products WHERE id=?1 AND company_id=?2")
                .bind(&pid)
                .bind(&cid)
                .fetch_one(&pool)
                .await
                .unwrap();
        let qty_before = dec(before.0.as_deref().unwrap_or("0"));

        transfer_stock(
            &pool,
            &cid,
            TransferInput {
                product_id: pid.clone(),
                from_gestiune_id: g_a.clone(),
                to_gestiune_id: g_b.clone(),
                transfer_date: "2026-07-05".into(),
                qty: "7".into(),
                transfer_ref: None,
                notes: None,
            },
        )
        .await
        .unwrap();

        let after: (Option<String>,) =
            sqlx::query_as("SELECT stock_qty FROM products WHERE id=?1 AND company_id=?2")
                .bind(&pid)
                .bind(&cid)
                .fetch_one(&pool)
                .await
                .unwrap();
        let qty_after = dec(after.0.as_deref().unwrap_or("0"));

        assert_eq!(
            qty_before, qty_after,
            "Total product on-hand must be unchanged after transfer: before={qty_before}, after={qty_after}"
        );
    }

    /// Atomicity: if the IN fails after the OUT commits, the OUT must be compensated so stock
    /// does not vanish. We simulate the stranded-OUT state (OUT recorded, no IN) and assert the
    /// rollback helper restores the total on-hand and leaves no transfer rows behind.
    #[tokio::test]
    async fn compensation_restores_stock_after_stranded_out() {
        let (pool, cid, pid) = setup("CMP").await;
        let g_a = make_gestiune(&pool, &cid, "A").await;

        // 10 buc into A
        record_movement(
            &pool,
            &mv(&cid, &pid, "2026-01-01", "10", Some("5.00"), &g_a),
            Dir::In,
        )
        .await
        .unwrap();

        // Simulate a half-transfer: only the OUT(A) is recorded (as if the IN failed after commit).
        let tid = "half_transfer_id";
        let mut out = mv(&cid, &pid, "2026-01-05", "4", None, &g_a);
        out.doc_type = Some("TRANSFER".to_string());
        out.doc_ref = Some(tid.to_string());
        record_movement(&pool, &out, Dir::Out).await.unwrap();

        // Stock has "vanished": 4 left A but never arrived in B → product total = 6.
        let stranded: (Option<String>,) =
            sqlx::query_as("SELECT stock_qty FROM products WHERE id=?1 AND company_id=?2")
                .bind(&pid)
                .bind(&cid)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            dec(stranded.0.as_deref().unwrap_or("0")),
            Decimal::from(6),
            "stranded total should be 6 (stock lost)"
        );

        // Compensate.
        rollback_transfer_by_ref(&pool, &cid, tid).await.unwrap();

        // Total on-hand restored to 10; no transfer rows remain.
        let restored: (Option<String>,) =
            sqlx::query_as("SELECT stock_qty FROM products WHERE id=?1 AND company_id=?2")
                .bind(&pid)
                .bind(&cid)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            dec(restored.0.as_deref().unwrap_or("0")),
            Decimal::from(10),
            "total on-hand must be restored to 10 after compensation"
        );
        let remaining: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM stock_ledger WHERE company_id=?1 AND doc_ref=?2",
        )
        .bind(&cid)
        .bind(tid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(remaining, 0, "no transfer ledger rows should remain");
    }

    /// Crash-window recovery: a TRANSFER ledger row whose doc_ref has NO stock_transfers record (a
    /// transfer that committed movements but crashed before writing the document) is auto-healed by
    /// `recover_incomplete_transfers` — restoring the product total on-hand. This covers the hard-crash
    /// window that in-process compensation cannot.
    #[tokio::test]
    async fn recover_incomplete_transfers_heals_orphaned_movements() {
        let (pool, cid, pid) = setup("CMP").await;
        let g_a = make_gestiune(&pool, &cid, "A").await;
        let g_b = make_gestiune(&pool, &cid, "B").await;

        record_movement(
            &pool,
            &mv(&cid, &pid, "2026-01-01", "10", Some("5.00"), &g_a),
            Dir::In,
        )
        .await
        .unwrap();

        // Simulate a crashed transfer: BOTH movements committed (4 left A, arrived B) but NO
        // stock_transfers row was written (process died before the insert).
        let tid = "crashed_transfer";
        let mut out = mv(&cid, &pid, "2026-01-05", "4", None, &g_a);
        out.doc_type = Some("TRANSFER".to_string());
        out.doc_ref = Some(tid.to_string());
        record_movement(&pool, &out, Dir::Out).await.unwrap();
        let mut in_b = mv(&cid, &pid, "2026-01-05", "4", Some("5.00"), &g_b);
        in_b.doc_type = Some("TRANSFER".to_string());
        in_b.doc_ref = Some(tid.to_string());
        record_movement(&pool, &in_b, Dir::In).await.unwrap();

        // The orphan is detected + rolled back.
        let recovered = recover_incomplete_transfers(&pool, &cid).await.unwrap();
        assert_eq!(recovered, 1, "one incomplete transfer recovered");

        // Total on-hand back to 10; A back to 10, B back to 0; no orphan rows.
        assert_eq!(
            onhand(&pool, &cid, pid.as_str(), &g_a).await,
            Decimal::from(10),
            "A restored to 10"
        );
        assert_eq!(
            onhand(&pool, &cid, pid.as_str(), &g_b).await,
            Decimal::ZERO,
            "B restored to 0"
        );
        let remaining: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM stock_ledger WHERE company_id=?1 AND doc_ref=?2",
        )
        .bind(&cid)
        .bind(tid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(remaining, 0, "orphaned transfer rows cleaned up");

        // Idempotent: a second pass finds nothing.
        assert_eq!(
            recover_incomplete_transfers(&pool, &cid).await.unwrap(),
            0,
            "recovery is idempotent"
        );
    }
}
