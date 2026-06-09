//! Evaluarea stocurilor (gestiune) — FIFO + CMP (cost mediu ponderat), OMFP 1802/2014 pct. 8.5.
//!
//! Două motoare PURE (testate) operează pe un flux cronologic de evenimente per produs (IN = recepție
//! la cost de achiziție; OUT = descărcare gestiune). Costul ieșirilor (COGS) nu poate fi atribuit la
//! inserare (o intrare retroactivă poate ajunge mai devreme), deci la fiecare mutație se RECALCULEAZĂ
//! întreg fluxul produsului (recompute_product) și se rescrie registrul (stock_ledger) + cache-ul de
//! pe produs. Banii folosesc round2 (MidpointAwayFromZero); cantitățile 6 zecimale.

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
    d.round_dp(6)
}

/// FIFO (primul intrat – primul ieșit): each OUT consumes the oldest receipt layers at their own cost.
pub fn fifo_value(events: &[StockEvent]) -> Vec<ValuedEvent> {
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
                    if let Some(front) = layers.front_mut() {
                        let take = need.min(front.1);
                        cogs += round2(take * front.2);
                        front.1 -= take;
                        need -= take;
                        if front.1 <= Decimal::ZERO {
                            layers.pop_front();
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

/// CMP (cost mediu ponderat — media mobilă): the average is recomputed on each receipt; OUTs are
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

// ─── DB layer ────────────────────────────────────────────────────────────────

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
}

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s.trim()).unwrap_or(Decimal::ZERO)
}

/// Insert a raw ledger event, then recompute the product's full valued stream.
pub async fn record_movement(
    pool: &SqlitePool,
    input: &StockMovementInput,
    dir: Dir,
) -> AppResult<()> {
    let qty = Decimal::from_str(input.qty.trim())
        .map_err(|_| AppError::Validation("Cantitate invalidă.".into()))?;
    if qty <= Decimal::ZERO {
        return Err(AppError::Validation(
            "Cantitatea trebuie să fie > 0.".into(),
        ));
    }
    let unit_cost = match dir {
        Dir::In => dec(input.unit_cost.as_deref().unwrap_or("0")),
        Dir::Out => Decimal::ZERO,
    };
    sqlx::query(
        "INSERT INTO stock_ledger (id, company_id, product_id, entry_date, seq, direction, qty, \
         unit_cost, value, run_qty, run_value, fifo_remaining, doc_type, doc_ref, source_type, \
         created_at) VALUES (?1,?2,?3,?4,0,?5,?6,?7,'0.00','0.000000','0.00','0.000000',?8,?9,'MANUAL',?10)",
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
    .bind(now_unix())
    .execute(pool)
    .await?;

    recompute_product(pool, &input.company_id, &input.product_id).await
}

/// Replay the product's full event stream with the chosen method and rewrite the ledger + the
/// product cache (stock_qty / avg_cost / stock_value) in one transaction.
pub async fn recompute_product(
    pool: &SqlitePool,
    company_id: &str,
    product_id: &str,
) -> AppResult<()> {
    let method: String =
        sqlx::query_scalar("SELECT COALESCE(valuation_method,'CMP') FROM products WHERE id=?1")
            .bind(product_id)
            .fetch_optional(pool)
            .await?
            .unwrap_or_else(|| "CMP".to_string());

    let stock_account: String =
        sqlx::query_scalar("SELECT COALESCE(stock_account,'371') FROM products WHERE id=?1")
            .bind(product_id)
            .fetch_optional(pool)
            .await?
            .unwrap_or_else(|| "371".to_string());

    let rows: Vec<(String, String, String, String, String)> = sqlx::query_as(
        "SELECT id, direction, qty, unit_cost, entry_date FROM stock_ledger \
         WHERE company_id=?1 AND product_id=?2 ORDER BY entry_date, seq, created_at",
    )
    .bind(company_id)
    .bind(product_id)
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

    let valued = if method == "FIFO" {
        fifo_value(&events)
    } else {
        cmp_value(&events)
    };

    let mut tx = pool.begin().await?;
    for v in &valued {
        sqlx::query(
            "UPDATE stock_ledger SET unit_cost=?2, value=?3, run_qty=?4, run_value=?5, \
             fifo_remaining=?6 WHERE id=?1",
        )
        .bind(&v.id)
        .bind(format!("{:.2}", v.unit_cost))
        .bind(format!("{:.2}", v.value))
        .bind(format!("{:.6}", v.run_qty))
        .bind(format!("{:.2}", v.run_value))
        .bind(format!("{:.6}", v.fifo_remaining))
        .execute(&mut *tx)
        .await?;
    }
    let (qty, value) = valued
        .last()
        .map(|v| (v.run_qty, v.run_value))
        .unwrap_or((Decimal::ZERO, Decimal::ZERO));
    let avg = if qty > Decimal::ZERO {
        round2(value / qty)
    } else {
        Decimal::ZERO
    };
    sqlx::query("UPDATE products SET stock_qty=?2, avg_cost=?3, stock_value=?4 WHERE id=?1")
        .bind(product_id)
        .bind(format!("{:.6}", qty))
        .bind(format!("{:.2}", avg))
        .bind(format!("{:.2}", value))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    // Re-post each movement's GL leg from the freshly-valued ledger (idempotent per ledger row, so a
    // backdated IN that re-values later OUTs re-posts them correctly).
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
    Ok(())
}

/// The valued stock ledger (fișa de magazie) for a product.
pub async fn ledger(
    pool: &SqlitePool,
    company_id: &str,
    product_id: &str,
) -> AppResult<Vec<LedgerRow>> {
    Ok(sqlx::query_as::<_, LedgerRow>(
        "SELECT id, entry_date, direction, qty, unit_cost, value, run_qty, run_value, doc_type, \
         doc_ref FROM stock_ledger WHERE company_id=?1 AND product_id=?2 \
         ORDER BY entry_date, seq, created_at",
    )
    .bind(company_id)
    .bind(product_id)
    .fetch_all(pool)
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // IN 10@5, IN 10@7, OUT 15 → 10@5 + 5@7 = 50 + 35 = 85; remaining 5@7 = 35.
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
    fn cmp_textbook() {
        // IN 10@5 (val 50), IN 10@7 (val 50+70=120, avg 6), OUT 15 @6 = 90; remaining 5 @6 = 30.
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
        // IN 5@4, OUT 8 → 5@4 + 3@4(shortfall) = 32, negative flagged.
        let v = fifo_value(&[ev(Dir::In, "5", "4"), ev(Dir::Out, "8", "0")]);
        assert!(v[1].negative_stock);
        assert_eq!(v[1].run_qty, d("-3.000000"));
    }
}
