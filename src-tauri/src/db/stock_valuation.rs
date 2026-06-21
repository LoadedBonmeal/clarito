//! Evaluarea stocurilor (gestiune) — FIFO + CMP (cost mediu ponderat) + LIFO, OMFP 1802/2014 pct. 96.
//!
//! Două motoare PURE (testate) operează pe un flux cronologic de evenimente per produs (IN = recepție
//! la cost de achiziție; OUT = descărcare gestiune). Costul ieșirilor (COGS) nu poate fi atribuit la
//! inserare (o intrare retroactivă poate ajunge mai devreme), deci la fiecare mutație se RECALCULEAZĂ
//! întreg fluxul produsului (recompute_product) și se rescrie registrul (stock_ledger) + cache-ul de
//! pe produs. Banii folosesc round2 (MidpointAwayFromZero); cantitățile 6 zecimale.
//!
//! Starting with migration 0064, stock_ledger rows carry a `gestiune_id`. The recompute functions
//! now operate per-gestiune (each gestiune's layers are isolated); the product cache (stock_qty /
//! avg_cost / stock_value) is kept as the SUM across all gestiuni.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::collections::VecDeque;
use std::str::FromStr;

use crate::db::invoices::round2;
use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    In,
    Out,
}

/// One stock event in the chronological stream (input to the valuation engines).
#[derive(Debug, Clone)]
pub struct StockEvent {
    pub id: String,
    pub dir: Dir,
    pub qty: Decimal,
    /// Unit cost — meaningful for IN (purchase cost); ignored for OUT (the engine assigns it).
    pub unit_cost: Decimal,
}

/// A valued event = the input event + the engine-assigned cost + running snapshot.
#[derive(Debug, Clone)]
pub struct ValuedEvent {
    pub id: String,
    pub dir: Dir,
    pub qty: Decimal,
    pub unit_cost: Decimal,
    pub value: Decimal,
    pub run_qty: Decimal,
    pub run_value: Decimal,
    /// Remaining un-issued qty of this IN layer (FIFO only; 0 for OUT).
    pub fifo_remaining: Decimal,
    /// True if an OUT exceeded the on-hand quantity (gestiune negativă — not allowed by OMFP).
    pub negative_stock: bool,
}

fn q6(d: Decimal) -> Decimal {
    d.round_dp_with_strategy(6, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
}

/// FIFO (primul intrat – primul ieșit): each OUT consumes the oldest receipt layers at their own cost.
pub fn fifo_value(events: &[StockEvent]) -> Vec<ValuedEvent> {
    layered_value(events, false)
}

/// LIFO (ultimul intrat – primul ieșit): each OUT consumes the NEWEST receipt layers at their own
/// cost. Permis pentru contabilitatea STATUTARĂ RO (OMFP 1802/2014 pct. 96 alin. (1) — CMP/FIFO/LIFO);
/// interzis doar pentru entitățile IFRS (IAS 2, din 2005). Motor identic cu FIFO, dar consumă din
/// coada stivei de loturi (cel mai recent), nu din față.
pub fn lifo_value(events: &[StockEvent]) -> Vec<ValuedEvent> {
    layered_value(events, true)
}

/// Shared layered-cost engine for FIFO / LIFO. `newest_first=false` => FIFO (consume the oldest IN
/// layer first); `newest_first=true` => LIFO (consume the newest IN layer first). Everything else
/// (rounding, running snapshot, negative-stock fallback to last cost, layer backfill) is identical.
fn layered_value(events: &[StockEvent], newest_first: bool) -> Vec<ValuedEvent> {
    let mut layers: VecDeque<(usize, Decimal, Decimal)> = VecDeque::new(); // (event_index, remaining, unit_cost)
    let mut out = Vec::with_capacity(events.len());
    let mut run_qty = Decimal::ZERO;
    let mut run_value = Decimal::ZERO;

    for (i, ev) in events.iter().enumerate() {
        match ev.dir {
            Dir::In => {
                layers.push_back((i, ev.qty, ev.unit_cost));
                run_qty += ev.qty;
                run_value = round2(run_value + round2(ev.qty * ev.unit_cost));
                out.push(ValuedEvent {
                    id: ev.id.clone(),
                    dir: Dir::In,
                    qty: ev.qty,
                    unit_cost: ev.unit_cost,
                    value: round2(ev.qty * ev.unit_cost),
                    run_qty: q6(run_qty),
                    run_value,
                    fifo_remaining: ev.qty,
                    negative_stock: false,
                });
            }
            Dir::Out => {
                let mut need = ev.qty;
                let mut cogs = Decimal::ZERO;
                let mut negative = false;
                while need > Decimal::ZERO {
                    // FIFO consumes the front (oldest) layer; LIFO the back (newest).
                    let layer = if newest_first {
                        layers.back_mut()
                    } else {
                        layers.front_mut()
                    };
                    if let Some(front) = layer {
                        let take = need.min(front.1);
                        cogs += round2(take * front.2);
                        front.1 -= take;
                        need -= take;
                        if front.1 <= Decimal::ZERO {
                            if newest_first {
                                layers.pop_back();
                            } else {
                                layers.pop_front();
                            }
                        }
                    } else {
                        // Stock-out: value the shortfall at the last known cost (or 0).
                        let last = out.iter().rev().find_map(|e| {
                            if e.dir == Dir::In {
                                Some(e.unit_cost)
                            } else {
                                None
                            }
                        });
                        cogs += round2(need * last.unwrap_or(Decimal::ZERO));
                        negative = true;
                        need = Decimal::ZERO;
                    }
                }
                run_qty -= ev.qty;
                run_value = round2(run_value - cogs);
                if run_qty <= Decimal::ZERO {
                    run_value = Decimal::ZERO;
                }
                let unit = if ev.qty.is_zero() {
                    Decimal::ZERO
                } else {
                    round2(cogs / ev.qty)
                };
                out.push(ValuedEvent {
                    id: ev.id.clone(),
                    dir: Dir::Out,
                    qty: ev.qty,
                    unit_cost: unit,
                    value: cogs,
                    run_qty: q6(run_qty),
                    run_value,
                    fifo_remaining: Decimal::ZERO,
                    negative_stock: negative,
                });
            }
        }
    }
    // Backfill fifo_remaining for IN rows from the final layer state.
    for (idx, remaining, _) in &layers {
        if let Some(v) = out.get_mut(*idx) {
            v.fifo_remaining = q6(*remaining);
        }
    }
    out
}

/// CMP (cost mediu ponderat — media mobila): the average is recomputed on each receipt; OUTs are
/// valued at the current average.
pub fn cmp_value(events: &[StockEvent]) -> Vec<ValuedEvent> {
    let mut run_qty = Decimal::ZERO;
    let mut run_value = Decimal::ZERO;
    let mut avg = Decimal::ZERO;
    let mut out = Vec::with_capacity(events.len());

    for ev in events {
        match ev.dir {
            Dir::In => {
                run_qty += ev.qty;
                run_value = round2(run_value + round2(ev.qty * ev.unit_cost));
                avg = if run_qty > Decimal::ZERO {
                    round2(run_value / run_qty)
                } else {
                    Decimal::ZERO
                };
                out.push(ValuedEvent {
                    id: ev.id.clone(),
                    dir: Dir::In,
                    qty: ev.qty,
                    unit_cost: ev.unit_cost,
                    value: round2(ev.qty * ev.unit_cost),
                    run_qty: q6(run_qty),
                    run_value,
                    fifo_remaining: Decimal::ZERO,
                    negative_stock: false,
                });
            }
            Dir::Out => {
                let value = round2(ev.qty * avg);
                let negative = ev.qty > run_qty;
                run_qty -= ev.qty;
                run_value = round2(run_value - value);
                if run_qty <= Decimal::ZERO {
                    run_value = Decimal::ZERO;
                }
                out.push(ValuedEvent {
                    id: ev.id.clone(),
                    dir: Dir::Out,
                    qty: ev.qty,
                    unit_cost: avg,
                    value,
                    run_qty: q6(run_qty),
                    run_value,
                    fifo_remaining: Decimal::ZERO,
                    negative_stock: negative,
                });
            }
        }
    }
    out
}

// --- DB layer ----------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StockMovementInput {
    pub company_id: String,
    pub product_id: String,
    pub entry_date: String,
    pub qty: String,
    /// Unit cost (IN only).
    #[serde(default)]
    pub unit_cost: Option<String>,
    pub doc_type: Option<String>,
    pub doc_ref: Option<String>,
    /// Gestiune (warehouse) — if None, resolved to the company's default gestiune at insert time.
    #[serde(default)]
    pub gestiune_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct LedgerRow {
    pub id: String,
    pub entry_date: String,
    pub direction: String,
    pub qty: String,
    pub unit_cost: String,
    pub value: String,
    pub run_qty: String,
    pub run_value: String,
    pub doc_type: Option<String>,
    pub doc_ref: Option<String>,
    pub gestiune_id: Option<String>,
}

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s.trim()).unwrap_or(Decimal::ZERO)
}

/// Verify the product exists AND belongs to the company (multi-tenant guard). NotFound otherwise.
pub async fn assert_product_owned(
    pool: &SqlitePool,
    company_id: &str,
    product_id: &str,
) -> AppResult<()> {
    let owned: Option<String> =
        sqlx::query_scalar("SELECT id FROM products WHERE id=?1 AND company_id=?2")
            .bind(product_id)
            .bind(company_id)
            .fetch_optional(pool)
            .await?;
    if owned.is_none() {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Insert a raw ledger event, then recompute the product's valued stream for the target gestiune.
/// Returns an optional user-facing warning (gestiune negativa after this movement).
pub async fn record_movement(
    pool: &SqlitePool,
    input: &StockMovementInput,
    dir: Dir,
) -> AppResult<Option<String>> {
    assert_product_owned(pool, &input.company_id, &input.product_id).await?;
    let qty = Decimal::from_str(input.qty.trim())
        .map_err(|_| AppError::Validation("Cantitate invalida.".into()))?;
    if qty <= Decimal::ZERO {
        return Err(AppError::Validation(
            "Cantitatea trebuie sa fie > 0.".into(),
        ));
    }
    // An IN must carry a valid, non-negative unit cost.
    let unit_cost = match dir {
        Dir::In => {
            let raw = input.unit_cost.as_deref().unwrap_or("");
            let c = Decimal::from_str(raw.trim()).map_err(|_| {
                AppError::Validation("Cost unitar invalid - folositi formatul 12.34.".into())
            })?;
            if c.is_sign_negative() {
                return Err(AppError::Validation(
                    "Costul unitar nu poate fi negativ.".into(),
                ));
            }
            c
        }
        Dir::Out => Decimal::ZERO,
    };

    // Resolve gestiune_id — use the provided one or fall back to the company's default.
    let gestiune_id = match &input.gestiune_id {
        Some(gid) if !gid.is_empty() => gid.clone(),
        _ => crate::db::gestiune::default_gestiune_id(pool, &input.company_id).await?,
    };

    sqlx::query(
        "INSERT INTO stock_ledger (id, company_id, product_id, entry_date, seq, direction, qty, \
         unit_cost, value, run_qty, run_value, fifo_remaining, doc_type, doc_ref, source_type, \
         gestiune_id, created_at) \
         VALUES (?1,?2,?3,?4,0,?5,?6,?7,'0.00','0.000000','0.00','0.000000',?8,?9,'MANUAL',?10,?11)",
    )
    .bind(new_id())
    .bind(&input.company_id)
    .bind(&input.product_id)
    .bind(&input.entry_date)
    .bind(if dir == Dir::In { "IN" } else { "OUT" })
    .bind(format!("{:.6}", qty))
    .bind(format!("{:.2}", unit_cost))
    .bind(&input.doc_type)
    .bind(&input.doc_ref)
    .bind(&gestiune_id)
    .bind(now_unix())
    .execute(pool)
    .await?;

    recompute_product_gestiune(pool, &input.company_id, &input.product_id, &gestiune_id).await
}

/// Replay the event stream for a SINGLE gestiune, rewrite those ledger rows + update the product
/// cache (stock_qty / avg_cost / stock_value) as the SUM across ALL gestiuni.
pub async fn recompute_product_gestiune(
    pool: &SqlitePool,
    company_id: &str,
    product_id: &str,
    gestiune_id: &str,
) -> AppResult<Option<String>> {
    // Read the policy with the company scope.
    let policy: Option<(String, String)> = sqlx::query_as(
        "SELECT COALESCE(valuation_method,'CMP'), COALESCE(stock_account,'371') \
         FROM products WHERE id=?1 AND company_id=?2",
    )
    .bind(product_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?;
    let (method, stock_account) = match policy {
        Some(p) => p,
        None => return Err(AppError::NotFound),
    };

    // Read only this gestiune's rows in chronological order.
    let rows: Vec<(String, String, String, String, String)> = sqlx::query_as(
        "SELECT id, direction, qty, unit_cost, entry_date FROM stock_ledger \
         WHERE company_id=?1 AND product_id=?2 AND gestiune_id=?3 \
         ORDER BY entry_date, seq, created_at",
    )
    .bind(company_id)
    .bind(product_id)
    .bind(gestiune_id)
    .fetch_all(pool)
    .await?;

    let dates: std::collections::HashMap<String, String> = rows
        .iter()
        .map(|(id, _, _, _, dt)| (id.clone(), dt.clone()))
        .collect();
    let events: Vec<StockEvent> = rows
        .iter()
        .map(|(id, d, q, uc, _)| StockEvent {
            id: id.clone(),
            dir: if d == "IN" { Dir::In } else { Dir::Out },
            qty: dec(q),
            unit_cost: dec(uc),
        })
        .collect();

    let valued = match method.as_str() {
        "FIFO" => fifo_value(&events),
        "LIFO" => lifo_value(&events),
        _ => cmp_value(&events),
    };

    let mut tx = pool.begin().await?;

    // Rewrite only this gestiune's ledger rows.
    for v in &valued {
        sqlx::query(
            "UPDATE stock_ledger SET unit_cost=?2, value=?3, run_qty=?4, run_value=?5, \
             fifo_remaining=?6 WHERE id=?1 AND company_id=?7",
        )
        .bind(&v.id)
        .bind(format!("{:.2}", v.unit_cost))
        .bind(format!("{:.2}", v.value))
        .bind(format!("{:.6}", v.run_qty))
        .bind(format!("{:.2}", v.run_value))
        .bind(format!("{:.6}", v.fifo_remaining))
        .bind(company_id)
        .execute(&mut *tx)
        .await?;
    }

    // Aggregate totals across ALL gestiuni for this product. We need the CHRONOLOGICALLY-last row per
    // gestiune (the on-hand after the last event in replay order = entry_date, seq, created_at) — NOT
    // MAX(rowid), which on a BACKDATED movement (earlier entry_date inserted later → higher rowid) would
    // pick a mid-stream running balance and drift the product cache. NOT EXISTS = "no later row in this
    // gestiune"; rowid is the final tiebreaker so exactly one row is selected per gestiune.
    let gestiune_totals: Vec<(String, String)> = sqlx::query_as(
        "SELECT sl.run_qty, sl.run_value \
         FROM stock_ledger sl \
         WHERE sl.company_id=?1 AND sl.product_id=?2 \
           AND NOT EXISTS ( \
             SELECT 1 FROM stock_ledger s2 \
             WHERE s2.company_id=sl.company_id AND s2.product_id=sl.product_id \
               AND s2.gestiune_id=sl.gestiune_id \
               AND ( s2.entry_date > sl.entry_date \
                  OR (s2.entry_date = sl.entry_date AND s2.seq > sl.seq) \
                  OR (s2.entry_date = sl.entry_date AND s2.seq = sl.seq AND s2.created_at > sl.created_at) \
                  OR (s2.entry_date = sl.entry_date AND s2.seq = sl.seq AND s2.created_at = sl.created_at AND s2.rowid > sl.rowid) ) \
           )",
    )
    .bind(company_id)
    .bind(product_id)
    .fetch_all(&mut *tx)
    .await?;

    let (total_qty, total_value) = gestiune_totals
        .iter()
        .fold((Decimal::ZERO, Decimal::ZERO), |(aq, av), (rq, rv)| {
            (aq + dec(rq), av + dec(rv))
        });
    let avg = if total_qty > Decimal::ZERO {
        round2(total_value / total_qty)
    } else {
        Decimal::ZERO
    };

    sqlx::query(
        "UPDATE products SET stock_qty=?2, avg_cost=?3, stock_value=?4 WHERE id=?1 AND company_id=?5",
    )
    .bind(product_id)
    .bind(format!("{:.6}", total_qty))
    .bind(format!("{:.2}", avg))
    .bind(format!("{:.2}", total_value))
    .bind(company_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    // Re-post each movement's GL leg from the freshly-valued ledger.
    for v in &valued {
        let date = dates.get(&v.id).map(String::as_str).unwrap_or("");
        crate::db::gl::post_stock_movement(
            pool,
            company_id,
            &v.id,
            date,
            &stock_account,
            v.dir == Dir::In,
            v.value,
        )
        .await?;
    }

    // Surface gestiune negativa (OMFP 1802 forbids it).
    let warning = valued
        .iter()
        .any(|v| v.negative_stock || v.run_qty.is_sign_negative())
        .then(|| {
            "Atentie: stocul a devenit negativ (gestiune negativa) - verificati receptiile."
                .to_string()
        });
    Ok(warning)
}

/// Replay ALL gestiuni for a product (backward-compatibility shim — calls recompute_product_gestiune
/// for each distinct gestiune_id). Returns a warning if any gestiune went negative.
pub async fn recompute_product(
    pool: &SqlitePool,
    company_id: &str,
    product_id: &str,
) -> AppResult<Option<String>> {
    let gestiune_ids: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT COALESCE(gestiune_id, 'gest-default-' || company_id) \
         FROM stock_ledger WHERE company_id=?1 AND product_id=?2",
    )
    .bind(company_id)
    .bind(product_id)
    .fetch_all(pool)
    .await?;

    if gestiune_ids.is_empty() {
        return Ok(None);
    }

    let mut any_warning: Option<String> = None;
    for gid in &gestiune_ids {
        let w = recompute_product_gestiune(pool, company_id, product_id, gid).await?;
        if w.is_some() {
            any_warning = w;
        }
    }
    Ok(any_warning)
}

/// The valued stock ledger (fisa de magazie) for a product, optionally filtered by gestiune_id.
pub async fn ledger(
    pool: &SqlitePool,
    company_id: &str,
    product_id: &str,
    gestiune_id: Option<&str>,
) -> AppResult<Vec<LedgerRow>> {
    if let Some(gid) = gestiune_id {
        Ok(sqlx::query_as::<_, LedgerRow>(
            "SELECT id, entry_date, direction, qty, unit_cost, value, run_qty, run_value, \
             doc_type, doc_ref, gestiune_id FROM stock_ledger \
             WHERE company_id=?1 AND product_id=?2 AND gestiune_id=?3 \
             ORDER BY entry_date, seq, created_at",
        )
        .bind(company_id)
        .bind(product_id)
        .bind(gid)
        .fetch_all(pool)
        .await?)
    } else {
        Ok(sqlx::query_as::<_, LedgerRow>(
            "SELECT id, entry_date, direction, qty, unit_cost, value, run_qty, run_value, \
             doc_type, doc_ref, gestiune_id FROM stock_ledger \
             WHERE company_id=?1 AND product_id=?2 \
             ORDER BY entry_date, seq, created_at",
        )
        .bind(company_id)
        .bind(product_id)
        .fetch_all(pool)
        .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    async fn setup() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        for (cid, pid, cui) in [("co1", "p1", "12345678"), ("co2", "p2", "87654321")] {
            sqlx::query(
                "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
                 VALUES (?1,?2,'T SRL','S','C','CJ','RO')",
            )
            .bind(cid)
            .bind(cui)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "INSERT INTO products (id, company_id, name, unit) VALUES (?1,?2,'Marfa','buc')",
            )
            .bind(pid)
            .bind(cid)
            .execute(&pool)
            .await
            .unwrap();
        }
        pool
    }

    #[tokio::test]
    async fn record_movement_rejects_cross_company() {
        let pool = setup().await;
        let input = StockMovementInput {
            company_id: "co2".into(),
            product_id: "p1".into(),
            entry_date: "2026-06-01".into(),
            qty: "5".into(),
            unit_cost: Some("10".into()),
            doc_type: None,
            doc_ref: None,
            gestiune_id: None,
        };
        assert!(matches!(
            record_movement(&pool, &input, Dir::In).await,
            Err(crate::error::AppError::NotFound)
        ));
        let own = StockMovementInput {
            company_id: "co1".into(),
            product_id: "p1".into(),
            ..input.clone()
        };
        assert!(record_movement(&pool, &own, Dir::In).await.is_ok());
    }

    #[tokio::test]
    async fn stock_in_gl_reclasses_to_stock_account_not_401() {
        let pool = setup().await;
        let input = StockMovementInput {
            company_id: "co1".into(),
            product_id: "p1".into(),
            entry_date: "2026-06-01".into(),
            qty: "10".into(),
            unit_cost: Some("5".into()),
            doc_type: None,
            doc_ref: None,
            gestiune_id: None,
        };
        record_movement(&pool, &input, Dir::In).await.unwrap();
        let tb = crate::db::gl::trial_balance(&pool, "co1", "2026-06-01", "2026-06-30")
            .await
            .unwrap();
        let bal = |code: &str| {
            tb.rows
                .iter()
                .find(|r| r.account_code == code)
                .map(|r| (r.closing_debit.clone(), r.closing_credit.clone()))
        };
        assert_eq!(bal("371"), Some(("50.00".into(), "0.00".into())));
        assert_eq!(bal("607"), Some(("0.00".into(), "50.00".into())));
        assert_eq!(bal("401"), None);
        assert!(tb.balanced);
    }

    #[tokio::test]
    async fn record_movement_rejects_garbage_cost() {
        let pool = setup().await;
        let bad = StockMovementInput {
            company_id: "co1".into(),
            product_id: "p1".into(),
            entry_date: "2026-06-01".into(),
            qty: "5".into(),
            unit_cost: Some("abc".into()),
            doc_type: None,
            doc_ref: None,
            gestiune_id: None,
        };
        assert!(record_movement(&pool, &bad, Dir::In).await.is_err());
    }

    fn ev(dir: Dir, qty: &str, cost: &str) -> StockEvent {
        StockEvent {
            id: format!("{dir:?}-{qty}"),
            dir,
            qty: Decimal::from_str(qty).unwrap(),
            unit_cost: Decimal::from_str(cost).unwrap(),
        }
    }
    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn fifo_textbook() {
        let v = fifo_value(&[
            ev(Dir::In, "10", "5"),
            ev(Dir::In, "10", "7"),
            ev(Dir::Out, "15", "0"),
        ]);
        assert_eq!(v[2].value, d("85.00"));
        assert_eq!(v[2].run_qty, d("5.000000"));
        assert_eq!(v[2].run_value, d("35.00"));
    }

    #[test]
    fn lifo_textbook() {
        let v = lifo_value(&[
            ev(Dir::In, "10", "5"),
            ev(Dir::In, "10", "7"),
            ev(Dir::Out, "15", "0"),
        ]);
        assert_eq!(v[2].value, d("95.00"), "LIFO COGS = 10@7 + 5@5 = 95");
        assert_eq!(v[2].run_qty, d("5.000000"));
        assert_eq!(
            v[2].run_value,
            d("25.00"),
            "remaining 5 of the @5 layer = 25"
        );
        assert_eq!(v[0].fifo_remaining, d("5.000000"));
    }

    #[test]
    fn fifo_unchanged_after_lifo_refactor() {
        let evs = [
            ev(Dir::In, "10", "5"),
            ev(Dir::In, "10", "7"),
            ev(Dir::Out, "15", "0"),
        ];
        let f = fifo_value(&evs);
        assert_eq!(f[2].value, d("85.00"));
        assert_eq!(f[2].run_value, d("35.00"));
        assert_eq!(f[1].fifo_remaining, d("5.000000"));
    }

    #[test]
    fn cmp_textbook() {
        let v = cmp_value(&[
            ev(Dir::In, "10", "5"),
            ev(Dir::In, "10", "7"),
            ev(Dir::Out, "15", "0"),
        ]);
        assert_eq!(v[1].run_value, d("120.00"));
        assert_eq!(v[2].unit_cost, d("6.00"));
        assert_eq!(v[2].value, d("90.00"));
        assert_eq!(v[2].run_value, d("30.00"));
    }

    #[test]
    fn fifo_stock_out_flags_negative() {
        let v = fifo_value(&[ev(Dir::In, "5", "4"), ev(Dir::Out, "8", "0")]);
        assert!(v[1].negative_stock);
        assert_eq!(v[1].run_qty, d("-3.000000"));
    }

    // -- INVARIANT: single-gestiune replay == old product-level replay ----------

    async fn seed_invariant_pool(method: &str) -> (SqlitePool, String) {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let cid = "inv_co";
        let pid = format!("inv_p_{method}");
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES (?1,'11111111','Inv SRL','S','C','CJ','RO')",
        )
        .bind(cid)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO products (id, company_id, name, unit, valuation_method) \
             VALUES (?1,?2,'P','buc',?3)",
        )
        .bind(&pid)
        .bind(cid)
        .bind(method)
        .execute(&pool)
        .await
        .unwrap();
        (pool, pid)
    }

    async fn capture_ledger(
        pool: &SqlitePool,
        company_id: &str,
        product_id: &str,
    ) -> Vec<(String, String, String, String, String)> {
        sqlx::query_as::<_, (String, String, String, String, String)>(
            "SELECT id, run_qty, run_value, unit_cost, fifo_remaining \
             FROM stock_ledger WHERE company_id=?1 AND product_id=?2 \
             ORDER BY entry_date, seq, created_at",
        )
        .bind(company_id)
        .bind(product_id)
        .fetch_all(pool)
        .await
        .unwrap()
    }

    async fn run_invariant_for_method(method: &str) {
        let (pool, pid) = seed_invariant_pool(method).await;
        let cid = "inv_co";

        let movements: &[(Dir, &str, &str)] = &[
            (Dir::In, "10", "5.00"),
            (Dir::In, "10", "7.00"),
            (Dir::Out, "15", "0.00"),
            (Dir::In, "5", "8.00"),
            (Dir::Out, "3", "0.00"),
        ];
        for (dir, qty, cost) in movements {
            let input = StockMovementInput {
                company_id: cid.into(),
                product_id: pid.clone(),
                entry_date: "2026-01-01".into(),
                qty: qty.to_string(),
                unit_cost: if *dir == Dir::In {
                    Some(cost.to_string())
                } else {
                    None
                },
                doc_type: None,
                doc_ref: None,
                gestiune_id: None,
            };
            record_movement(&pool, &input, *dir).await.unwrap();
        }

        let baseline = capture_ledger(&pool, cid, &pid).await;
        assert!(!baseline.is_empty());

        let default_gid = crate::db::gestiune::default_gestiune_id(&pool, cid)
            .await
            .unwrap();
        recompute_product_gestiune(&pool, cid, &pid, &default_gid)
            .await
            .unwrap();

        let after = capture_ledger(&pool, cid, &pid).await;
        assert_eq!(
            baseline.len(),
            after.len(),
            "row count changed for {method}"
        );
        for (i, (b, a)) in baseline.iter().zip(after.iter()).enumerate() {
            assert_eq!(
                b.1, a.1,
                "{method} row {i}: run_qty mismatch {} != {}",
                b.1, a.1
            );
            assert_eq!(
                b.2, a.2,
                "{method} row {i}: run_value mismatch {} != {}",
                b.2, a.2
            );
            assert_eq!(
                b.3, a.3,
                "{method} row {i}: unit_cost mismatch {} != {}",
                b.3, a.3
            );
            assert_eq!(
                b.4, a.4,
                "{method} row {i}: fifo_remaining mismatch {} != {}",
                b.4, a.4
            );
        }
    }

    #[tokio::test]
    async fn invariant_single_gestiune_fifo_byte_identical() {
        run_invariant_for_method("FIFO").await;
    }

    #[tokio::test]
    async fn invariant_single_gestiune_lifo_byte_identical() {
        run_invariant_for_method("LIFO").await;
    }

    #[tokio::test]
    async fn invariant_single_gestiune_cmp_byte_identical() {
        run_invariant_for_method("CMP").await;
    }

    #[tokio::test]
    async fn per_gestiune_isolation() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        let cid = "iso_co";
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES (?1,'22222222','Iso SRL','S','C','CJ','RO')",
        )
        .bind(cid)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO products (id, company_id, name, unit, valuation_method) \
             VALUES ('iso_p',?1,'P','buc','FIFO')",
        )
        .bind(cid)
        .execute(&pool)
        .await
        .unwrap();

        let g_a = crate::db::gestiune::create(
            &pool,
            cid,
            crate::db::gestiune::GestiuneInput {
                cod: "A".into(),
                denumire: "Gest A".into(),
                tip: None,
                metoda_evaluare: Some("FIFO".into()),
                cont_stoc: None,
                adresa: None,
                dispersata_teritorial: None,
            },
        )
        .await
        .unwrap();
        let g_b = crate::db::gestiune::create(
            &pool,
            cid,
            crate::db::gestiune::GestiuneInput {
                cod: "B".into(),
                denumire: "Gest B".into(),
                tip: None,
                metoda_evaluare: Some("FIFO".into()),
                cont_stoc: None,
                adresa: None,
                dispersata_teritorial: None,
            },
        )
        .await
        .unwrap();

        let in_a = StockMovementInput {
            company_id: cid.into(),
            product_id: "iso_p".into(),
            entry_date: "2026-01-01".into(),
            qty: "10".into(),
            unit_cost: Some("5.00".into()),
            doc_type: None,
            doc_ref: None,
            gestiune_id: Some(g_a.id.clone()),
        };
        let in_b = StockMovementInput {
            company_id: cid.into(),
            product_id: "iso_p".into(),
            entry_date: "2026-01-01".into(),
            qty: "10".into(),
            unit_cost: Some("7.00".into()),
            doc_type: None,
            doc_ref: None,
            gestiune_id: Some(g_b.id.clone()),
        };
        record_movement(&pool, &in_a, Dir::In).await.unwrap();
        record_movement(&pool, &in_b, Dir::In).await.unwrap();

        let out_a = StockMovementInput {
            company_id: cid.into(),
            product_id: "iso_p".into(),
            entry_date: "2026-01-02".into(),
            qty: "5".into(),
            unit_cost: None,
            doc_type: None,
            doc_ref: None,
            gestiune_id: Some(g_a.id.clone()),
        };
        record_movement(&pool, &out_a, Dir::Out).await.unwrap();

        let last_a: (String, String, String) = sqlx::query_as(
            "SELECT run_qty, run_value, unit_cost FROM stock_ledger \
             WHERE company_id=?1 AND product_id='iso_p' AND gestiune_id=?2 \
             ORDER BY entry_date DESC, seq DESC, created_at DESC LIMIT 1",
        )
        .bind(cid)
        .bind(&g_a.id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(last_a.0, "5.000000", "A: run_qty after OUT should be 5");
        assert_eq!(last_a.1, "25.00", "A: run_value should be 25 (5@5)");
        assert_eq!(
            last_a.2, "5.00",
            "A: OUT unit_cost should be 5.00 (FIFO from @5 layer)"
        );

        let last_b: (String, String) = sqlx::query_as(
            "SELECT run_qty, run_value FROM stock_ledger \
             WHERE company_id=?1 AND product_id='iso_p' AND gestiune_id=?2 \
             ORDER BY entry_date DESC, seq DESC, created_at DESC LIMIT 1",
        )
        .bind(cid)
        .bind(&g_b.id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            last_b.0, "10.000000",
            "B: run_qty should be unchanged at 10"
        );
        assert_eq!(last_b.1, "70.00", "B: run_value should be 70 (10@7)");

        let total: (Option<String>, Option<String>) = sqlx::query_as(
            "SELECT stock_qty, stock_value FROM products WHERE id='iso_p' AND company_id=?1",
        )
        .bind(cid)
        .fetch_one(&pool)
        .await
        .unwrap();
        let tq = total.0.as_deref().unwrap_or("0");
        let tv = total.1.as_deref().unwrap_or("0");
        let tq_f: f64 = tq.parse().unwrap_or(0.0);
        let tv_f: f64 = tv.parse().unwrap_or(0.0);
        assert!((tq_f - 15.0).abs() < 0.001, "total qty = 15, got {tq}");
        assert!(
            (tv_f - 95.0).abs() < 0.01,
            "total value = 95 (25+70), got {tv}"
        );
    }

    #[tokio::test]
    async fn product_cache_correct_for_backdated_movement() {
        // Regression: the product-level stock_qty/stock_value/avg_cost aggregation must take the
        // CHRONOLOGICALLY-last ledger row per gestiune (entry_date, seq, created_at), NOT MAX(rowid).
        // A backdated IN (earlier entry_date, recorded LAST → highest rowid) would, under MAX(rowid),
        // make the product cache pick the backdated row's mid-stream running balance (10/80) instead
        // of the true on-hand (16/98). FIFO makes the value unambiguous.
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let cid = "bd_co";
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES (?1,'33333333','Bd SRL','S','C','CJ','RO')",
        )
        .bind(cid)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO products (id, company_id, name, unit, valuation_method) \
             VALUES ('bd_p',?1,'P','buc','FIFO')",
        )
        .bind(cid)
        .execute(&pool)
        .await
        .unwrap();

        let mv = |date: &str, qty: &str, cost: Option<&str>| StockMovementInput {
            company_id: cid.into(),
            product_id: "bd_p".into(),
            entry_date: date.into(),
            qty: qty.into(),
            unit_cost: cost.map(|c| c.into()),
            doc_type: None,
            doc_ref: None,
            gestiune_id: None, // default gestiune
        };
        // Recorded in this order; the third is BACKDATED to before the first two.
        record_movement(&pool, &mv("2026-03-01", "10", Some("5.00")), Dir::In)
            .await
            .unwrap();
        record_movement(&pool, &mv("2026-03-10", "4", None), Dir::Out)
            .await
            .unwrap();
        record_movement(&pool, &mv("2026-02-01", "10", Some("8.00")), Dir::In)
            .await
            .unwrap();

        // FIFO replay in chrono order: 02-01 IN 10@8 (10/80) → 03-01 IN 10@5 (20/130) →
        // 03-10 OUT 4 from the @8 layer (−32) → on-hand 16 / 98.00. avg = 98/16 → 6.13.
        let (q, v): (Option<String>, Option<String>) = sqlx::query_as(
            "SELECT stock_qty, stock_value FROM products WHERE id='bd_p' AND company_id=?1",
        )
        .bind(cid)
        .fetch_one(&pool)
        .await
        .unwrap();
        let qf: f64 = q.as_deref().unwrap_or("0").parse().unwrap_or(0.0);
        let vf: f64 = v.as_deref().unwrap_or("0").parse().unwrap_or(0.0);
        assert!(
            (qf - 16.0).abs() < 0.001,
            "stock_qty must be the chrono-last 16, not the backdated row's 10 — got {q:?}"
        );
        assert!(
            (vf - 98.0).abs() < 0.01,
            "stock_value must be 98.00 (FIFO on-hand), not the backdated row's 80.00 — got {v:?}"
        );
    }
}
