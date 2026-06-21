//! Producție / BOM — OMFP 1802/2014 pct. 8 (cost de producție = materiale-only MVP).
//!
//! ## Monografie producție (standard RO)
//!
//! Consum materii prime (301→601):
//!   D 601 (Cheltuieli cu materiile prime) = valoare consum
//!   C 301 (Materii prime)                 = valoare consum
//!
//! Consum materiale consumabile (302→602):
//!   D 602 (Cheltuieli cu materialele consumabile) = valoare consum
//!   C 302 (Materiale consumabile)                 = valoare consum
//!
//! Obținere produse finite (345→711):
//!   D 345 (Produse finite)                = cost producție
//!   C 711 (Variația stocurilor)            = cost producție
//!
//! Aceste note sunt generate automat de `record_movement` (post_stock_movement din gl.rs).
//! Producția NU este GL-neutră (spre deosebire de transfer) — doc_type='PRODUCTION' nu
//! setează is_transfer=true, deci GL-ul se postează normal.
//!
//! ## Cost capitalizat (materials-only MVP)
//!
//! Costul unitar al produsului finit = Σ(qty_comp × cost_FIFO/LIFO/CMP) / qty_produsă.
//! Manopera directă (641/421) și regia (681, costul fix/variabil) NU sunt adăugate la costul
//! 345 în această versiune — rămân cheltuieli ale perioadei recunoscute separat.
//! Per OMFP 1802/2014 pct. 8 costul complet ar include manoperă + regie alocată pe capacitatea
//! normală (cu regie fixă neabsorbită → cheltuiala perioadei conform IAS 2 / pct. 8 al. (3)).
//! Alocarea regiei este un follow-up planificat.
//!
//! ## Atomicitate
//!
//! produce() consumă N componente (OUT) și produce 1 produs finit (IN). Dacă oricare pas
//! eșuează după ce au fost comise mișcări anterioare, `rollback_production_movements` anulează:
//!   1. Șterge toate rândurile stock_ledger cu doc_ref=order_id
//!   2. Șterge toate notele GL STOCK asociate (gl_journal WHERE source_type='STOCK' AND
//!      source_id IN ledger-ids pentru acel order) — producția POSTEAZĂ GL, deci trebuie
//!      curățată explicit (spre deosebire de transfer, care este GL-neutru)
//!   3. Recompute per (component, gestiune) și (produs_finit, gestiune)

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

use crate::db::invoices::round2;
use crate::db::models::{new_id, now_unix};
use crate::db::stock_valuation::{self, Dir, StockMovementInput};
use crate::error::{AppError, AppResult};

// ─── Types ────────────────────────────────────────────────────────────────────

/// Capul unui BOM (rețetă de producție).
#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Bom {
    pub id: String,
    pub company_id: String,
    pub product_id: String,
    pub name: String,
    pub output_qty: String,
    pub active: i64,
    pub created_at: i64,
}

/// O linie BOM (componentă + cantitate).
#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct BomLine {
    pub id: String,
    pub bom_id: String,
    pub component_product_id: String,
    pub qty: String,
    pub um: Option<String>,
    pub line_no: i64,
}

/// BOM complet (cap + linii).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BomWithLines {
    #[serde(flatten)]
    pub bom: Bom,
    pub lines: Vec<BomLine>,
}

/// Input pentru crearea unui BOM.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BomInput {
    pub product_id: String,
    pub name: String,
    pub output_qty: String,
    pub lines: Vec<BomLineInput>,
}

/// O linie BOM în input.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BomLineInput {
    pub component_product_id: String,
    pub qty: String,
    pub um: Option<String>,
    pub line_no: i64,
}

/// Un ordin de producție finalizat.
#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ProductieOrder {
    pub id: String,
    pub company_id: String,
    pub bom_id: String,
    pub product_id: String,
    pub gestiune_id: String,
    pub qty_produced: String,
    pub production_date: String,
    pub total_material_cost: String,
    pub unit_cost: String,
    pub status: String,
    pub notes: Option<String>,
    pub created_at: i64,
}

/// Input pentru lansarea producției.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProduceInput {
    pub bom_id: String,
    pub gestiune_id: String,
    pub qty_produced: String,
    pub production_date: String,
    pub notes: Option<String>,
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s.trim()).unwrap_or(Decimal::ZERO)
}

/// Cantitatea disponibilă (run_qty a ultimei mișcări cronologice) pentru un produs în gestiune.
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

/// Compensare pentru un ordin de producție eșuat la mijloc.
///
/// Șterge:
///   1. Toate rândurile stock_ledger cu doc_ref=order_id (componente OUT + produs_finit IN)
///   2. Notele GL STOCK corespunzătoare acelor ledger ids (producția POSTEAZĂ GL, nu este neutră)
///
/// Recompute: toți produsul afectați (componente + produs finit) în gestiunea dată.
pub async fn rollback_production_movements(
    pool: &SqlitePool,
    company_id: &str,
    order_id: &str,
    affected_product_ids: &[String],
    gestiune_id: &str,
) -> AppResult<()> {
    // 1. Colectăm ledger ids înainte de ștergere (pentru curățarea GL STOCK)
    let ledger_ids: Vec<String> =
        sqlx::query_scalar("SELECT id FROM stock_ledger WHERE company_id=?1 AND doc_ref=?2")
            .bind(company_id)
            .bind(order_id)
            .fetch_all(pool)
            .await?;

    // 2. Ștergem notele GL STOCK pentru aceste ledger ids
    //    (producția NU este GL-neutră → trebuie curățate explicit)
    for lid in &ledger_ids {
        sqlx::query(
            "DELETE FROM gl_journal WHERE company_id=?1 AND source_type='STOCK' AND source_id=?2",
        )
        .bind(company_id)
        .bind(lid)
        .execute(pool)
        .await?;
    }

    // 3. Ștergem mișcările de stoc
    sqlx::query("DELETE FROM stock_ledger WHERE company_id=?1 AND doc_ref=?2")
        .bind(company_id)
        .bind(order_id)
        .execute(pool)
        .await?;

    // 4. Recompute pentru fiecare produs afectat (componente + produs finit)
    for pid in affected_product_ids {
        // Recompute numai dacă mai există mișcări în gestiunea dată (altfel skip safe)
        let exists: Option<String> = sqlx::query_scalar(
            "SELECT id FROM stock_ledger WHERE company_id=?1 AND product_id=?2 AND gestiune_id=?3 LIMIT 1",
        )
        .bind(company_id)
        .bind(pid)
        .bind(gestiune_id)
        .fetch_optional(pool)
        .await?;
        if exists.is_some() {
            stock_valuation::recompute_product_gestiune(pool, company_id, pid, gestiune_id).await?;
        } else {
            // Nu mai există mișcări: reset cache la 0
            sqlx::query(
                "UPDATE products SET stock_qty='0.000000', avg_cost='0.00', stock_value='0.00' \
                 WHERE id=?1 AND company_id=?2",
            )
            .bind(pid)
            .bind(company_id)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}

// ─── BOM CRUD ─────────────────────────────────────────────────────────────────

/// Creează un BOM (cap + linii). Validează:
///   - product_id aparține companiei (guard multi-tenant)
///   - output_qty > 0
///   - cel puțin o linie
///   - fiecare componentă aparține companiei
///   - qty linie > 0
pub async fn create_bom(
    pool: &SqlitePool,
    company_id: &str,
    input: BomInput,
) -> AppResult<BomWithLines> {
    // Validare output_qty
    let output_qty = Decimal::from_str(input.output_qty.trim())
        .map_err(|_| AppError::Validation("output_qty invalid.".into()))?;
    if output_qty <= Decimal::ZERO {
        return Err(AppError::Validation(
            "Cantitatea produsă per rețetă trebuie să fie > 0.".into(),
        ));
    }

    // Validare produs_finit aparține companiei
    stock_valuation::assert_product_owned(pool, company_id, &input.product_id).await?;

    // Cel puțin o linie
    if input.lines.is_empty() {
        return Err(AppError::Validation(
            "BOM-ul trebuie să conțină cel puțin o componentă.".into(),
        ));
    }

    // Validare linii
    for line in &input.lines {
        stock_valuation::assert_product_owned(pool, company_id, &line.component_product_id).await?;
        let qty = Decimal::from_str(line.qty.trim())
            .map_err(|_| AppError::Validation("Cantitate componentă invalidă.".into()))?;
        if qty <= Decimal::ZERO {
            return Err(AppError::Validation(
                "Cantitatea componentei trebuie să fie > 0.".into(),
            ));
        }
    }

    let bom_id = new_id();
    let now = now_unix();

    // Insert cap BOM
    sqlx::query(
        "INSERT INTO bom (id, company_id, product_id, name, output_qty, active, created_at) \
         VALUES (?1,?2,?3,?4,?5,1,?6)",
    )
    .bind(&bom_id)
    .bind(company_id)
    .bind(&input.product_id)
    .bind(&input.name)
    .bind(format!("{:.6}", output_qty))
    .bind(now)
    .execute(pool)
    .await?;

    // Insert linii
    let mut lines_out = Vec::with_capacity(input.lines.len());
    for line in &input.lines {
        let lid = new_id();
        let qty = Decimal::from_str(line.qty.trim()).unwrap_or(Decimal::ZERO);
        sqlx::query(
            "INSERT INTO bom_lines (id, bom_id, component_product_id, qty, um, line_no) \
             VALUES (?1,?2,?3,?4,?5,?6)",
        )
        .bind(&lid)
        .bind(&bom_id)
        .bind(&line.component_product_id)
        .bind(format!("{:.6}", qty))
        .bind(&line.um)
        .bind(line.line_no)
        .execute(pool)
        .await?;
        lines_out.push(BomLine {
            id: lid,
            bom_id: bom_id.clone(),
            component_product_id: line.component_product_id.clone(),
            qty: format!("{:.6}", qty),
            um: line.um.clone(),
            line_no: line.line_no,
        });
    }

    let bom = Bom {
        id: bom_id,
        company_id: company_id.to_string(),
        product_id: input.product_id,
        name: input.name,
        output_qty: format!("{:.6}", output_qty),
        active: 1,
        created_at: now,
    };
    Ok(BomWithLines {
        bom,
        lines: lines_out,
    })
}

/// Listează toate BOM-urile pentru o companie.
pub async fn list_bom(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<Bom>> {
    Ok(sqlx::query_as::<_, Bom>(
        "SELECT id, company_id, product_id, name, output_qty, active, created_at \
         FROM bom WHERE company_id=?1 ORDER BY name",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?)
}

/// Returnează un BOM cu liniile sale (guard multi-tenant).
pub async fn get_bom(pool: &SqlitePool, company_id: &str, bom_id: &str) -> AppResult<BomWithLines> {
    let bom: Option<Bom> = sqlx::query_as::<_, Bom>(
        "SELECT id, company_id, product_id, name, output_qty, active, created_at \
         FROM bom WHERE id=?1 AND company_id=?2",
    )
    .bind(bom_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?;
    let bom = bom.ok_or(AppError::NotFound)?;

    let lines: Vec<BomLine> = sqlx::query_as::<_, BomLine>(
        "SELECT id, bom_id, component_product_id, qty, um, line_no \
         FROM bom_lines WHERE bom_id=?1 ORDER BY line_no",
    )
    .bind(bom_id)
    .fetch_all(pool)
    .await?;

    Ok(BomWithLines { bom, lines })
}

/// Șterge un BOM (și liniile aferente prin CASCADE). Respinge dacă există ordine de producție.
pub async fn delete_bom(pool: &SqlitePool, company_id: &str, bom_id: &str) -> AppResult<()> {
    // Guard: nu permitem ștergerea unui BOM cu ordine atașate
    let used: Option<String> = sqlx::query_scalar(
        "SELECT id FROM productie_orders WHERE company_id=?1 AND bom_id=?2 LIMIT 1",
    )
    .bind(company_id)
    .bind(bom_id)
    .fetch_optional(pool)
    .await?;
    if used.is_some() {
        return Err(AppError::Validation(
            "Rețeta nu poate fi ștearsă — există ordine de producție asociate.".into(),
        ));
    }

    let rows = sqlx::query("DELETE FROM bom WHERE id=?1 AND company_id=?2")
        .bind(bom_id)
        .bind(company_id)
        .execute(pool)
        .await?
        .rows_affected();
    if rows == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Actualizează un BOM: șterge liniile vechi + le reinserează (delete-recreate pattern).
pub async fn update_bom(
    pool: &SqlitePool,
    company_id: &str,
    bom_id: &str,
    input: BomInput,
) -> AppResult<BomWithLines> {
    // Verificăm că BOM-ul aparține companiei
    let exists: Option<String> =
        sqlx::query_scalar("SELECT id FROM bom WHERE id=?1 AND company_id=?2")
            .bind(bom_id)
            .bind(company_id)
            .fetch_optional(pool)
            .await?;
    if exists.is_none() {
        return Err(AppError::NotFound);
    }

    // Validare
    let output_qty = Decimal::from_str(input.output_qty.trim())
        .map_err(|_| AppError::Validation("output_qty invalid.".into()))?;
    if output_qty <= Decimal::ZERO {
        return Err(AppError::Validation(
            "Cantitatea produsă per rețetă trebuie să fie > 0.".into(),
        ));
    }
    stock_valuation::assert_product_owned(pool, company_id, &input.product_id).await?;
    if input.lines.is_empty() {
        return Err(AppError::Validation(
            "BOM-ul trebuie să conțină cel puțin o componentă.".into(),
        ));
    }
    for line in &input.lines {
        stock_valuation::assert_product_owned(pool, company_id, &line.component_product_id).await?;
        let qty = Decimal::from_str(line.qty.trim())
            .map_err(|_| AppError::Validation("Cantitate componentă invalidă.".into()))?;
        if qty <= Decimal::ZERO {
            return Err(AppError::Validation(
                "Cantitatea componentei trebuie să fie > 0.".into(),
            ));
        }
    }

    // Update cap
    sqlx::query(
        "UPDATE bom SET product_id=?3, name=?4, output_qty=?5 WHERE id=?1 AND company_id=?2",
    )
    .bind(bom_id)
    .bind(company_id)
    .bind(&input.product_id)
    .bind(&input.name)
    .bind(format!("{:.6}", output_qty))
    .execute(pool)
    .await?;

    // Delete + re-insert linii
    sqlx::query("DELETE FROM bom_lines WHERE bom_id=?1")
        .bind(bom_id)
        .execute(pool)
        .await?;

    let mut lines_out = Vec::with_capacity(input.lines.len());
    for line in &input.lines {
        let lid = new_id();
        let qty = Decimal::from_str(line.qty.trim()).unwrap_or(Decimal::ZERO);
        sqlx::query(
            "INSERT INTO bom_lines (id, bom_id, component_product_id, qty, um, line_no) \
             VALUES (?1,?2,?3,?4,?5,?6)",
        )
        .bind(&lid)
        .bind(bom_id)
        .bind(&line.component_product_id)
        .bind(format!("{:.6}", qty))
        .bind(&line.um)
        .bind(line.line_no)
        .execute(pool)
        .await?;
        lines_out.push(BomLine {
            id: lid,
            bom_id: bom_id.to_string(),
            component_product_id: line.component_product_id.clone(),
            qty: format!("{:.6}", qty),
            um: line.um.clone(),
            line_no: line.line_no,
        });
    }

    let bom = sqlx::query_as::<_, Bom>(
        "SELECT id, company_id, product_id, name, output_qty, active, created_at \
         FROM bom WHERE id=?1 AND company_id=?2",
    )
    .bind(bom_id)
    .bind(company_id)
    .fetch_one(pool)
    .await?;

    Ok(BomWithLines {
        bom,
        lines: lines_out,
    })
}

// ─── Core operation: produce() ────────────────────────────────────────────────

/// Lansează un ordin de producție (produce qty_produced unități conform BOM).
///
/// Garantează (all-or-nothing):
///   - BOM aparține companiei, gestiunea aparține companiei, qty_produced > 0
///   - Verifică stoc suficient pentru TOATE componentele ÎNAINTE de a consuma oricare
///   - Consumă componentele (OUT, doc_type='PRODUCTION') → GL auto: D 601/602 = C 301/302
///   - Produce produsul finit (IN la cost material) → GL auto: D 345 = C 711
///   - Dacă oricare pas eșuează: rollback_production_movements (DELETE ledger rows + GL STOCK)
///   - Inserează productie_orders și returnează înregistrarea
pub async fn produce(
    pool: &SqlitePool,
    company_id: &str,
    input: ProduceInput,
) -> AppResult<ProductieOrder> {
    // ── Validare input ────────────────────────────────────────────────────────

    let qty_produced = Decimal::from_str(input.qty_produced.trim())
        .map_err(|_| AppError::Validation("Cantitate produsă invalidă.".into()))?;
    if qty_produced <= Decimal::ZERO {
        return Err(AppError::Validation(
            "Cantitatea produsă trebuie să fie > 0.".into(),
        ));
    }

    // ── Fetch BOM (cu guard multi-tenant) ────────────────────────────────────

    let bom_with_lines = get_bom(pool, company_id, &input.bom_id).await?;
    let bom = &bom_with_lines.bom;
    let lines = &bom_with_lines.lines;

    if lines.is_empty() {
        return Err(AppError::Validation("BOM-ul nu are componente.".into()));
    }

    // ── Verificare gestiune aparține companiei ────────────────────────────────

    let gest_exists: Option<String> =
        sqlx::query_scalar("SELECT id FROM gestiune WHERE id=?1 AND company_id=?2")
            .bind(&input.gestiune_id)
            .bind(company_id)
            .fetch_optional(pool)
            .await?;
    if gest_exists.is_none() {
        return Err(AppError::NotFound);
    }

    // ── Factor de scală: qty_produced / output_qty ───────────────────────────

    let output_qty = dec(&bom.output_qty);
    if output_qty <= Decimal::ZERO {
        return Err(AppError::Validation("output_qty BOM invalid (≤ 0).".into()));
    }
    let scale = qty_produced / output_qty;

    // ── Verificare stoc suficient pentru TOATE componentele (pre-consume) ────
    // Agregăm necesarul per COMPONENT DISTINCT (un component poate apărea pe mai multe linii BOM).
    // Verificarea per-linie ar lăsa o rețetă 2+3 cu stoc 4 să treacă (2≤4 ȘI 3≤4) și apoi să ducă
    // gestiunea la −1 — record_movement(Dir::Out) NU respinge stocul negativ, doar îl semnalează,
    // deci garanția all-or-nothing s-ar rupe (OMFP 1802/2014 interzice stocul negativ). Sumăm
    // cantitățile rotunjite per linie (identic cu felul în care se acumulează OUT-urile).
    let mut needed_per_component: std::collections::BTreeMap<String, Decimal> =
        std::collections::BTreeMap::new();
    for line in lines {
        let needed = round2(dec(&line.qty) * scale);
        *needed_per_component
            .entry(line.component_product_id.clone())
            .or_insert(Decimal::ZERO) += needed;
    }
    for (component_id, needed) in &needed_per_component {
        let available = on_hand_qty(pool, company_id, component_id, &input.gestiune_id).await?;
        if available < *needed {
            return Err(AppError::Validation(format!(
                "Stoc insuficient: {} (disponibil {:.6}, necesar {:.6}).",
                component_id, available, needed
            )));
        }
    }

    // ── Alocăm order_id ──────────────────────────────────────────────────────

    let order_id = new_id();

    // Colectăm product_ids afectate pentru rollback (componente + produs finit)
    let mut affected_products: Vec<String> = lines
        .iter()
        .map(|l| l.component_product_id.clone())
        .collect();
    affected_products.push(bom.product_id.clone());
    // Deduplicăm (rar, dar un component poate fi și produsul finit în teste patologice)
    affected_products.dedup();

    // ── Consumăm componentele (OUT) ──────────────────────────────────────────

    for line in lines {
        let needed = round2(dec(&line.qty) * scale);
        let out_input = StockMovementInput {
            company_id: company_id.to_string(),
            product_id: line.component_product_id.clone(),
            entry_date: input.production_date.clone(),
            qty: format!("{:.6}", needed),
            unit_cost: None, // OUT: costul e atribuit de motor FIFO/LIFO/CMP
            doc_type: Some("PRODUCTION".to_string()),
            doc_ref: Some(order_id.clone()),
            gestiune_id: Some(input.gestiune_id.clone()),
        };

        if let Err(e) = stock_valuation::record_movement(pool, &out_input, Dir::Out).await {
            // Compensare: anulăm tot ce s-a comis până acum
            let _ = rollback_production_movements(
                pool,
                company_id,
                &order_id,
                &affected_products,
                &input.gestiune_id,
            )
            .await;
            return Err(e);
        }
    }

    // Costul total al materialelor = suma TUTUROR ieșirilor acestei comenzi. Agregăm toate rândurile
    // OUT (robust la componente duplicate); NU citim per-iterație cu ORDER BY created_at LIMIT 1 —
    // created_at e la secundă, deci pentru o componentă care apare pe două linii tiebreak-ul era
    // ambiguu și putea dubla (sau pierde) valoarea unei ieșiri.
    let total_material_cost: Decimal = match sqlx::query_as::<_, (String,)>(
        "SELECT value FROM stock_ledger \
         WHERE company_id=?1 AND doc_ref=?2 AND direction='OUT'",
    )
    .bind(company_id)
    .bind(&order_id)
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows.iter().map(|(v,)| dec(v)).sum(),
        Err(e) => {
            let _ = rollback_production_movements(
                pool,
                company_id,
                &order_id,
                &affected_products,
                &input.gestiune_id,
            )
            .await;
            return Err(e.into());
        }
    };

    // ── Producem produsul finit (IN la costul materialelor) ──────────────────
    //
    // unit_cost = total_material_cost / qty_produced (rotunjit la 2 zecimale)
    // Aceasta capitalizează NUMAI costul materialelor în 345. Manopera directă
    // (641/421) și regia (681) rămân cheltuieli ale perioadei — follow-up planificat.

    let unit_cost = if qty_produced.is_zero() {
        Decimal::ZERO
    } else {
        round2(total_material_cost / qty_produced)
    };

    let in_input = StockMovementInput {
        company_id: company_id.to_string(),
        product_id: bom.product_id.clone(),
        entry_date: input.production_date.clone(),
        qty: format!("{:.6}", qty_produced),
        unit_cost: Some(format!("{:.2}", unit_cost)),
        doc_type: Some("PRODUCTION".to_string()),
        doc_ref: Some(order_id.clone()),
        gestiune_id: Some(input.gestiune_id.clone()),
    };

    if let Err(e) = stock_valuation::record_movement(pool, &in_input, Dir::In).await {
        let _ = rollback_production_movements(
            pool,
            company_id,
            &order_id,
            &affected_products,
            &input.gestiune_id,
        )
        .await;
        return Err(e);
    }

    // ── Inserăm ordinul de producție ─────────────────────────────────────────

    let order = ProductieOrder {
        id: order_id.clone(),
        company_id: company_id.to_string(),
        bom_id: input.bom_id.clone(),
        product_id: bom.product_id.clone(),
        gestiune_id: input.gestiune_id.clone(),
        qty_produced: format!("{:.6}", qty_produced),
        production_date: input.production_date.clone(),
        total_material_cost: format!("{:.2}", total_material_cost),
        unit_cost: format!("{:.2}", unit_cost),
        status: "finalized".to_string(),
        notes: input.notes.clone(),
        created_at: now_unix(),
    };

    let insert_res = sqlx::query(
        "INSERT INTO productie_orders \
         (id, company_id, bom_id, product_id, gestiune_id, qty_produced, production_date, \
          total_material_cost, unit_cost, status, notes, created_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
    )
    .bind(&order.id)
    .bind(&order.company_id)
    .bind(&order.bom_id)
    .bind(&order.product_id)
    .bind(&order.gestiune_id)
    .bind(&order.qty_produced)
    .bind(&order.production_date)
    .bind(&order.total_material_cost)
    .bind(&order.unit_cost)
    .bind(&order.status)
    .bind(&order.notes)
    .bind(order.created_at)
    .execute(pool)
    .await;

    if let Err(e) = insert_res {
        let _ = rollback_production_movements(
            pool,
            company_id,
            &order_id,
            &affected_products,
            &input.gestiune_id,
        )
        .await;
        return Err(e.into());
    }

    Ok(order)
}

// ─── Queries ─────────────────────────────────────────────────────────────────

/// Listează ordinele de producție pentru o companie (descrescător după dată).
pub async fn list_productie(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<ProductieOrder>> {
    Ok(sqlx::query_as::<_, ProductieOrder>(
        "SELECT id, company_id, bom_id, product_id, gestiune_id, qty_produced, \
         production_date, total_material_cost, unit_cost, status, notes, created_at \
         FROM productie_orders WHERE company_id=?1 \
         ORDER BY production_date DESC, created_at DESC",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?)
}

/// Returnează un ordin de producție (guard multi-tenant).
pub async fn get_productie(
    pool: &SqlitePool,
    company_id: &str,
    order_id: &str,
) -> AppResult<ProductieOrder> {
    sqlx::query_as::<_, ProductieOrder>(
        "SELECT id, company_id, bom_id, product_id, gestiune_id, qty_produced, \
         production_date, total_material_cost, unit_cost, status, notes, created_at \
         FROM productie_orders WHERE id=?1 AND company_id=?2",
    )
    .bind(order_id)
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

    // ── Helpers ───────────────────────────────────────────────────────────────

    async fn setup() -> (SqlitePool, String) {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let cid = "prod_co".to_string();
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES (?1,'11111111','Prod SRL','S','C','CJ','RO')",
        )
        .bind(&cid)
        .execute(&pool)
        .await
        .unwrap();
        (pool, cid)
    }

    async fn make_gestiune(pool: &SqlitePool, cid: &str, cod: &str) -> String {
        gestiune::create(
            pool,
            cid,
            GestiuneInput {
                cod: cod.to_string(),
                denumire: format!("Gest {cod}"),
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

    fn make_product_sql<'a>(
        pool: &'a SqlitePool,
        pid: &'a str,
        cid: &'a str,
        name: &'a str,
        ptype: &'a str,
        stock_account: &'a str,
        method: &'a str,
    ) -> impl std::future::Future<Output = ()> + 'a {
        let pid = pid.to_string();
        let cid = cid.to_string();
        let name = name.to_string();
        let ptype = ptype.to_string();
        let stock_account = stock_account.to_string();
        let method = method.to_string();
        async move {
            sqlx::query(
                "INSERT INTO products (id, company_id, name, unit, product_type, stock_account, \
                 valuation_method) VALUES (?1,?2,?3,'buc',?4,?5,?6)",
            )
            .bind(&pid)
            .bind(&cid)
            .bind(&name)
            .bind(&ptype)
            .bind(&stock_account)
            .bind(&method)
            .execute(pool)
            .await
            .unwrap();
        }
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

    async fn onhand(pool: &SqlitePool, cid: &str, pid: &str, gid: &str) -> Decimal {
        on_hand_qty(pool, cid, pid, gid).await.unwrap()
    }

    // ── Test 1: BOM explosion + scale factor ──────────────────────────────────

    /// Rețetă: 2×A + 3×B → 1 produs finit. Produce 10 → consumă 20 A + 30 B.
    #[tokio::test]
    async fn bom_explosion_scale_factor() {
        let (pool, cid) = setup().await;
        let g = make_gestiune(&pool, &cid, "G").await;

        // Produse
        make_product_sql(
            &pool,
            "pf1",
            &cid,
            "Produs Finit",
            "produs_finit",
            "345",
            "CMP",
        )
        .await;
        make_product_sql(
            &pool,
            "mp_a",
            &cid,
            "Materie A",
            "materie_prima",
            "301",
            "CMP",
        )
        .await;
        make_product_sql(
            &pool,
            "mp_b",
            &cid,
            "Materie B",
            "materie_prima",
            "301",
            "CMP",
        )
        .await;

        // Stoc inițial
        record_movement(
            &pool,
            &mv(&cid, "mp_a", "2026-01-01", "100", Some("5.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();
        record_movement(
            &pool,
            &mv(&cid, "mp_b", "2026-01-01", "100", Some("3.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();

        // BOM: 2×A + 3×B → 1 produs
        let bom = create_bom(
            &pool,
            &cid,
            BomInput {
                product_id: "pf1".into(),
                name: "Rețetă test".into(),
                output_qty: "1".into(),
                lines: vec![
                    BomLineInput {
                        component_product_id: "mp_a".into(),
                        qty: "2".into(),
                        um: None,
                        line_no: 1,
                    },
                    BomLineInput {
                        component_product_id: "mp_b".into(),
                        qty: "3".into(),
                        um: None,
                        line_no: 2,
                    },
                ],
            },
        )
        .await
        .unwrap();

        // Producem 10 → trebuie să consume 20 A + 30 B
        let order = produce(
            &pool,
            &cid,
            ProduceInput {
                bom_id: bom.bom.id.clone(),
                gestiune_id: g.clone(),
                qty_produced: "10".into(),
                production_date: "2026-01-10".into(),
                notes: None,
            },
        )
        .await
        .unwrap();

        // Verifică stocuri
        let a_remaining = onhand(&pool, &cid, "mp_a", &g).await;
        let b_remaining = onhand(&pool, &cid, "mp_b", &g).await;
        let pf_qty = onhand(&pool, &cid, "pf1", &g).await;

        assert_eq!(a_remaining, Decimal::from(80), "A: 100 - 20 = 80");
        assert_eq!(b_remaining, Decimal::from(70), "B: 100 - 30 = 70");
        assert_eq!(pf_qty, Decimal::from(10), "PF: 0 + 10 = 10");

        // Verifică costul total: 20×5 + 30×3 = 100 + 90 = 190; unit = 19
        assert_eq!(
            order.total_material_cost, "190.00",
            "total_material_cost=190"
        );
        assert_eq!(order.unit_cost, "19.00", "unit_cost=19");
    }

    /// Regression: the SAME component on two BOM lines (2 + 3) must value correctly. The old
    /// per-iteration `ORDER BY created_at DESC LIMIT 1` read was ambiguous at second-granularity and
    /// could double-count or drop one OUT; the SUM-of-all-OUTs aggregation is exact.
    #[tokio::test]
    async fn duplicate_component_values_correctly() {
        let (pool, cid) = setup().await;
        let g = make_gestiune(&pool, &cid, "GD").await;
        make_product_sql(&pool, "pf_d", &cid, "PF", "produs_finit", "345", "CMP").await;
        make_product_sql(&pool, "mp_d", &cid, "MP", "materie_prima", "301", "CMP").await;

        record_movement(
            &pool,
            &mv(&cid, "mp_d", "2026-01-01", "100", Some("5.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();

        // BOM with the SAME component on two lines: 2 + 3 = 5 per output.
        let bom = create_bom(
            &pool,
            &cid,
            BomInput {
                product_id: "pf_d".into(),
                name: "Dup".into(),
                output_qty: "1".into(),
                lines: vec![
                    BomLineInput {
                        component_product_id: "mp_d".into(),
                        qty: "2".into(),
                        um: None,
                        line_no: 1,
                    },
                    BomLineInput {
                        component_product_id: "mp_d".into(),
                        qty: "3".into(),
                        um: None,
                        line_no: 2,
                    },
                ],
            },
        )
        .await
        .unwrap();

        let order = produce(
            &pool,
            &cid,
            ProduceInput {
                bom_id: bom.bom.id.clone(),
                gestiune_id: g.clone(),
                qty_produced: "1".into(),
                production_date: "2026-01-10".into(),
                notes: None,
            },
        )
        .await
        .unwrap();

        // 100 − (2+3) = 95 consumed; total cost = 5 × 5.00 = 25.00 (NOT double-counted).
        assert_eq!(
            onhand(&pool, &cid, "mp_d", &g).await,
            Decimal::from(95),
            "MP: 100 - (2+3) = 95"
        );
        assert_eq!(
            order.total_material_cost, "25.00",
            "duplicate component summed exactly: 25.00"
        );
        assert_eq!(order.unit_cost, "25.00", "unit_cost = 25.00");
    }

    /// Regression (audit): the pre-consume check must AGGREGATE per distinct component. The same
    /// component on two lines (2+3=5) with on-hand 4 used to pass both per-line checks (2≤4, 3≤4)
    /// and drive the gestiune to −1; now the aggregate need (5) is rejected before any consumption.
    #[tokio::test]
    async fn duplicate_component_aggregate_stock_rejected() {
        let (pool, cid) = setup().await;
        let g = make_gestiune(&pool, &cid, "GA").await;
        make_product_sql(&pool, "pf_a", &cid, "PF", "produs_finit", "345", "CMP").await;
        make_product_sql(&pool, "mp_a", &cid, "MP", "materie_prima", "301", "CMP").await;

        // Only 4 in stock.
        record_movement(
            &pool,
            &mv(&cid, "mp_a", "2026-01-01", "4", Some("5.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();

        // Same component on two lines: 2 + 3 = 5 needed per output.
        let bom = create_bom(
            &pool,
            &cid,
            BomInput {
                product_id: "pf_a".into(),
                name: "Dup".into(),
                output_qty: "1".into(),
                lines: vec![
                    BomLineInput {
                        component_product_id: "mp_a".into(),
                        qty: "2".into(),
                        um: None,
                        line_no: 1,
                    },
                    BomLineInput {
                        component_product_id: "mp_a".into(),
                        qty: "3".into(),
                        um: None,
                        line_no: 2,
                    },
                ],
            },
        )
        .await
        .unwrap();

        let res = produce(
            &pool,
            &cid,
            ProduceInput {
                bom_id: bom.bom.id.clone(),
                gestiune_id: g.clone(),
                qty_produced: "1".into(),
                production_date: "2026-01-10".into(),
                notes: None,
            },
        )
        .await;

        assert!(res.is_err(), "must reject: aggregate need 5 > on-hand 4");
        // No consumption happened — stock unchanged, no PRODUCTION ledger rows.
        assert_eq!(
            onhand(&pool, &cid, "mp_a", &g).await,
            Decimal::from(4),
            "stock unchanged after rejection (no negative stock)"
        );
        let cnt: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM stock_ledger WHERE company_id=?1 AND doc_type='PRODUCTION'",
        )
        .bind(&cid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(cnt, 0, "no PRODUCTION ledger rows after rejection");
    }

    // ── Test 2: component stock decreases + finished good increases ───────────

    #[tokio::test]
    async fn produce_stock_delta() {
        let (pool, cid) = setup().await;
        let g = make_gestiune(&pool, &cid, "G2").await;

        make_product_sql(&pool, "pf2", &cid, "PF", "produs_finit", "345", "FIFO").await;
        make_product_sql(
            &pool,
            "mc1",
            &cid,
            "MC",
            "material_consumabil",
            "302",
            "FIFO",
        )
        .await;

        record_movement(
            &pool,
            &mv(&cid, "mc1", "2026-02-01", "50", Some("2.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();

        let bom = create_bom(
            &pool,
            &cid,
            BomInput {
                product_id: "pf2".into(),
                name: "BOM2".into(),
                output_qty: "1".into(),
                lines: vec![BomLineInput {
                    component_product_id: "mc1".into(),
                    qty: "5".into(),
                    um: Some("kg".into()),
                    line_no: 1,
                }],
            },
        )
        .await
        .unwrap();

        let before_mc = onhand(&pool, &cid, "mc1", &g).await;
        let before_pf = onhand(&pool, &cid, "pf2", &g).await;

        produce(
            &pool,
            &cid,
            ProduceInput {
                bom_id: bom.bom.id.clone(),
                gestiune_id: g.clone(),
                qty_produced: "4".into(),
                production_date: "2026-02-10".into(),
                notes: None,
            },
        )
        .await
        .unwrap();

        let after_mc = onhand(&pool, &cid, "mc1", &g).await;
        let after_pf = onhand(&pool, &cid, "pf2", &g).await;

        assert_eq!(
            before_mc - after_mc,
            Decimal::from(20),
            "MC consumed 20 (4×5)"
        );
        assert_eq!(after_pf - before_pf, Decimal::from(4), "PF increased by 4");
    }

    // ── Test 3: GL monografie (consum 601=301, obținere 345=711) ─────────────

    /// Query SUM(debit), SUM(credit) for an account directly from gl_entry (period-level totals,
    /// not closing balances). This avoids the closing-net ambiguity of trial_balance.
    async fn gl_turnover(pool: &SqlitePool, cid: &str, account: &str) -> (Decimal, Decimal) {
        let row: (Option<f64>, Option<f64>) = sqlx::query_as(
            "SELECT SUM(CAST(e.debit AS REAL)), SUM(CAST(e.credit AS REAL)) \
             FROM gl_journal j JOIN gl_entry e ON e.journal_pk = j.id \
             WHERE j.company_id=?1 AND e.account_code=?2",
        )
        .bind(cid)
        .bind(account)
        .fetch_one(pool)
        .await
        .unwrap();
        let d = Decimal::try_from(row.0.unwrap_or(0.0))
            .unwrap_or(Decimal::ZERO)
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
        let c = Decimal::try_from(row.1.unwrap_or(0.0))
            .unwrap_or(Decimal::ZERO)
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
        (d, c)
    }

    /// GL monografie: consum materie primă → D601=C301; obținere → D345=C711.
    /// Verificăm TOTAL debit/credit (turnover) per cont, nu soldul net (closing balance).
    #[tokio::test]
    async fn gl_monografie_601_301_345_711() {
        let (pool, cid) = setup().await;
        let g = make_gestiune(&pool, &cid, "G3").await;

        make_product_sql(&pool, "pf3", &cid, "PF3", "produs_finit", "345", "CMP").await;
        make_product_sql(&pool, "mp3", &cid, "MP3", "materie_prima", "301", "CMP").await;

        // Stoc: 10 @ 4.00 = 40 → D 301 = C 601 (recepție stoc)
        record_movement(
            &pool,
            &mv(&cid, "mp3", "2026-03-01", "10", Some("4.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();

        let bom = create_bom(
            &pool,
            &cid,
            BomInput {
                product_id: "pf3".into(),
                name: "BOM3".into(),
                output_qty: "1".into(),
                lines: vec![BomLineInput {
                    component_product_id: "mp3".into(),
                    qty: "2".into(),
                    um: None,
                    line_no: 1,
                }],
            },
        )
        .await
        .unwrap();

        // Produce 3 → consum 6 @ CMP 4.00 = 24.00 → D601=C301(24) + D345=C711(24)
        produce(
            &pool,
            &cid,
            ProduceInput {
                bom_id: bom.bom.id.clone(),
                gestiune_id: g.clone(),
                qty_produced: "3".into(),
                production_date: "2026-03-10".into(),
                notes: None,
            },
        )
        .await
        .unwrap();

        // Turnover totals per account (sum debit / sum credit across ALL gl_entry rows)
        // 301: IN=D40/C0  + consum=D0/C24  → total D=40, C=24
        // 601: IN=D0/C40  + consum=D24/C0  → total D=24, C=40
        // 345: obținere=D24/C0             → total D=24, C=0
        // 711: obținere=D0/C24             → total D=0,  C=24
        let (d301, c301) = gl_turnover(&pool, &cid, "301").await;
        let (d601, _c601) = gl_turnover(&pool, &cid, "601").await;
        let (d345, _c345) = gl_turnover(&pool, &cid, "345").await;
        let (_d711, c711) = gl_turnover(&pool, &cid, "711").await;

        // Consum OUT → D601 += 24, C301 += 24
        assert!(
            c301 >= Decimal::from(24),
            "301 total_credit ≥ 24 (consum), got {c301}"
        );
        assert!(
            d601 >= Decimal::from(24),
            "601 total_debit ≥ 24 (consum), got {d601}"
        );

        // Obținere IN → D345 = 24, C711 = 24
        assert!(
            d345 >= Decimal::from(24),
            "345 total_debit ≥ 24 (obținere), got {d345}"
        );
        assert!(
            c711 >= Decimal::from(24),
            "711 total_credit ≥ 24 (obținere), got {c711}"
        );

        // 601 și 301 trebuie să existe și să fie > 0 (nu 607)
        assert!(
            d601 > Decimal::ZERO,
            "D601 must be posted for materie_prima"
        );
        assert!(
            c301 > Decimal::ZERO,
            "C301 must be posted for materie_prima"
        );

        // GL global echilibrat: Σtotal_debit == Σtotal_credit
        let (total_d, total_c): (Decimal, Decimal) = {
            let row: (Option<f64>, Option<f64>) = sqlx::query_as(
                "SELECT SUM(CAST(e.debit AS REAL)), SUM(CAST(e.credit AS REAL)) \
                 FROM gl_journal j JOIN gl_entry e ON e.journal_pk = j.id \
                 WHERE j.company_id=?1",
            )
            .bind(&cid)
            .fetch_one(&pool)
            .await
            .unwrap();
            let d = Decimal::try_from(row.0.unwrap_or(0.0))
                .unwrap_or(Decimal::ZERO)
                .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
            let c = Decimal::try_from(row.1.unwrap_or(0.0))
                .unwrap_or(Decimal::ZERO)
                .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
            (d, c)
        };
        assert_eq!(
            total_d, total_c,
            "GL balanced: Σtotal_debit={total_d} == Σtotal_credit={total_c}"
        );

        let _ = d301; // used indirectly via c301 assertion; avoid unused warning
    }

    // ── Test 4: stoc insuficient → respins, niciun consum parțial ────────────

    #[tokio::test]
    async fn insufficient_stock_rejected_all_or_nothing() {
        let (pool, cid) = setup().await;
        let g = make_gestiune(&pool, &cid, "G4").await;

        make_product_sql(&pool, "pf4", &cid, "PF4", "produs_finit", "345", "CMP").await;
        make_product_sql(&pool, "mp4a", &cid, "A4", "materie_prima", "301", "CMP").await;
        make_product_sql(&pool, "mp4b", &cid, "B4", "materie_prima", "301", "CMP").await;

        // A: 5 buc (suficient pentru 1×5=5), B: 2 buc (insuficient pentru 1×5=5)
        record_movement(
            &pool,
            &mv(&cid, "mp4a", "2026-04-01", "5", Some("10.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();
        record_movement(
            &pool,
            &mv(&cid, "mp4b", "2026-04-01", "2", Some("8.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();

        let bom = create_bom(
            &pool,
            &cid,
            BomInput {
                product_id: "pf4".into(),
                name: "BOM4".into(),
                output_qty: "1".into(),
                lines: vec![
                    BomLineInput {
                        component_product_id: "mp4a".into(),
                        qty: "5".into(),
                        um: None,
                        line_no: 1,
                    },
                    BomLineInput {
                        component_product_id: "mp4b".into(),
                        qty: "5".into(),
                        um: None,
                        line_no: 2,
                    }, // insuficient
                ],
            },
        )
        .await
        .unwrap();

        let result = produce(
            &pool,
            &cid,
            ProduceInput {
                bom_id: bom.bom.id.clone(),
                gestiune_id: g.clone(),
                qty_produced: "1".into(),
                production_date: "2026-04-10".into(),
                notes: None,
            },
        )
        .await;

        assert!(
            matches!(result, Err(AppError::Validation(_))),
            "Trebuie respins cu Validation"
        );

        // Verificăm că nu a rămas niciun consum parțial
        let ledger_rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM stock_ledger WHERE company_id=?1 AND doc_type='PRODUCTION'",
        )
        .bind(&cid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            ledger_rows, 0,
            "No partial consumption after rejection (all-or-nothing)"
        );

        // Stocurile rămân intacte
        assert_eq!(onhand(&pool, &cid, "mp4a", &g).await, Decimal::from(5));
        assert_eq!(onhand(&pool, &cid, "mp4b", &g).await, Decimal::from(2));
        assert_eq!(onhand(&pool, &cid, "pf4", &g).await, Decimal::ZERO);
    }

    // ── Test 5: atomicitate (compensare manuală) ──────────────────────────────

    /// Simulăm un OUT parțial (ca și când IN-ul ar fi eșuat) și verificăm că
    /// rollback_production_movements readuce stocul și nu lasă rânduri GL STOCK orfane.
    #[tokio::test]
    async fn compensation_restores_stock_and_cleans_gl() {
        let (pool, cid) = setup().await;
        let g = make_gestiune(&pool, &cid, "G5").await;

        make_product_sql(&pool, "pf5", &cid, "PF5", "produs_finit", "345", "CMP").await;
        make_product_sql(&pool, "mp5", &cid, "MP5", "materie_prima", "301", "CMP").await;

        // Stoc inițial: 20 @ 6.00
        record_movement(
            &pool,
            &mv(&cid, "mp5", "2026-05-01", "20", Some("6.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();

        // Simulăm: OUT 4 buc (ca la consum producție), doc_ref=order_fake
        let fake_order = "order_fake_123";
        let mut out = mv(&cid, "mp5", "2026-05-05", "4", None, &g);
        out.doc_type = Some("PRODUCTION".to_string());
        out.doc_ref = Some(fake_order.to_string());
        record_movement(&pool, &out, Dir::Out).await.unwrap();

        // Verificăm că stocul a scăzut și există o notă GL STOCK
        let stranded_qty = onhand(&pool, &cid, "mp5", &g).await;
        assert_eq!(stranded_qty, Decimal::from(16), "Before rollback: 20-4=16");

        let gl_before: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_journal j \
             WHERE j.company_id=?1 AND j.source_type='STOCK' \
               AND j.source_id IN (SELECT id FROM stock_ledger WHERE company_id=?1 AND doc_ref=?2)",
        )
        .bind(&cid)
        .bind(fake_order)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert!(
            gl_before >= 1,
            "Should have GL STOCK entry before rollback: got {gl_before}"
        );

        // Compensare
        rollback_production_movements(
            &pool,
            &cid,
            fake_order,
            &["mp5".to_string(), "pf5".to_string()],
            &g,
        )
        .await
        .unwrap();

        // Stocul revine la 20
        let restored = onhand(&pool, &cid, "mp5", &g).await;
        assert_eq!(
            restored,
            Decimal::from(20),
            "After rollback: qty restored to 20"
        );

        // Niciun rând doc_ref=order_fake în stock_ledger
        let remaining_rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM stock_ledger WHERE company_id=?1 AND doc_ref=?2",
        )
        .bind(&cid)
        .bind(fake_order)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(remaining_rows, 0, "No ledger rows after rollback");

        // Nicio notă GL STOCK orfană
        let gl_after: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_journal j \
             WHERE j.company_id=?1 AND j.source_type='STOCK' \
               AND j.source_id IN (SELECT id FROM stock_ledger WHERE company_id=?1 AND doc_ref=?2)",
        )
        .bind(&cid)
        .bind(fake_order)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(gl_after, 0, "No orphan STOCK GL journals after rollback");
    }

    // ── Test 6: regen golden — producție GL supraviețuiește generate_gl_entries ─

    /// `generate_gl_entries` șterge doar source_type 'INVOICE'/'RECEIVED_INVOICE'/'PAYMENT'/...
    /// dar NU 'STOCK'. Notele de stoc (inclusiv producție) trebuie să supraviețuiască regen.
    #[tokio::test]
    async fn productie_gl_survives_generate_gl_entries() {
        let (pool, cid) = setup().await;
        let g = make_gestiune(&pool, &cid, "G6").await;

        make_product_sql(&pool, "pf6", &cid, "PF6", "produs_finit", "345", "CMP").await;
        make_product_sql(&pool, "mp6", &cid, "MP6", "materie_prima", "301", "CMP").await;

        record_movement(
            &pool,
            &mv(&cid, "mp6", "2026-06-01", "10", Some("5.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();

        let bom = create_bom(
            &pool,
            &cid,
            BomInput {
                product_id: "pf6".into(),
                name: "BOM6".into(),
                output_qty: "1".into(),
                lines: vec![BomLineInput {
                    component_product_id: "mp6".into(),
                    qty: "2".into(),
                    um: None,
                    line_no: 1,
                }],
            },
        )
        .await
        .unwrap();

        produce(
            &pool,
            &cid,
            ProduceInput {
                bom_id: bom.bom.id.clone(),
                gestiune_id: g.clone(),
                qty_produced: "2".into(),
                production_date: "2026-06-10".into(),
                notes: None,
            },
        )
        .await
        .unwrap();

        // Numărăm notele GL STOCK înainte de regen
        let before: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_journal WHERE company_id=?1 AND source_type='STOCK'",
        )
        .bind(&cid)
        .fetch_one(&pool)
        .await
        .unwrap();

        // Rulăm generate_gl_entries (șterge INVOICE/PAYMENT etc, NU STOCK)
        crate::db::gl::generate_gl_entries(&pool, &cid, "2026-06-01", "2026-06-30", false)
            .await
            .unwrap();

        let after: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_journal WHERE company_id=?1 AND source_type='STOCK'",
        )
        .bind(&cid)
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(
            before, after,
            "STOCK GL journals must survive generate_gl_entries ({before} before, {after} after)"
        );
    }

    // ── Test 7: 602=302 pentru material_consumabil ────────────────────────────

    #[tokio::test]
    async fn gl_602_302_for_material_consumabil() {
        let (pool, cid) = setup().await;
        let g = make_gestiune(&pool, &cid, "G7").await;

        make_product_sql(&pool, "pf7", &cid, "PF7", "produs_finit", "345", "CMP").await;
        make_product_sql(
            &pool,
            "mc7",
            &cid,
            "MC7",
            "material_consumabil",
            "302",
            "CMP",
        )
        .await;

        record_movement(
            &pool,
            &mv(&cid, "mc7", "2026-07-01", "20", Some("3.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();

        let bom = create_bom(
            &pool,
            &cid,
            BomInput {
                product_id: "pf7".into(),
                name: "BOM7".into(),
                output_qty: "1".into(),
                lines: vec![BomLineInput {
                    component_product_id: "mc7".into(),
                    qty: "4".into(),
                    um: None,
                    line_no: 1,
                }],
            },
        )
        .await
        .unwrap();

        produce(
            &pool,
            &cid,
            ProduceInput {
                bom_id: bom.bom.id.clone(),
                gestiune_id: g.clone(),
                qty_produced: "3".into(),
                production_date: "2026-07-10".into(),
                notes: None,
            },
        )
        .await
        .unwrap();

        // Verificăm turnover D602 > 0 (consum) și C302 > 0 (descărcare)
        // Notă: trial_balance.closing_debit/credit = sold net (ambiguu pt. conturi cu rulaj bidirecțional);
        // folosim turnover direct din gl_entry.
        let (d602, _c602) = gl_turnover(&pool, &cid, "602").await;
        let (_d302, c302) = gl_turnover(&pool, &cid, "302").await;

        assert!(
            d602 > Decimal::ZERO,
            "D602 must be posted for material_consumabil consumption"
        );
        assert!(
            c302 > Decimal::ZERO,
            "C302 must be posted for material_consumabil consumption"
        );

        // GL echilibrat (total debit == total credit)
        let (td, tc): (Decimal, Decimal) = {
            let row: (Option<f64>, Option<f64>) = sqlx::query_as(
                "SELECT SUM(CAST(e.debit AS REAL)), SUM(CAST(e.credit AS REAL)) \
                 FROM gl_journal j JOIN gl_entry e ON e.journal_pk = j.id WHERE j.company_id=?1",
            )
            .bind(&cid)
            .fetch_one(&pool)
            .await
            .unwrap();
            let d = Decimal::try_from(row.0.unwrap_or(0.0))
                .unwrap_or(Decimal::ZERO)
                .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
            let c = Decimal::try_from(row.1.unwrap_or(0.0))
                .unwrap_or(Decimal::ZERO)
                .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
            (d, c)
        };
        assert_eq!(td, tc, "GL balanced after 602/302 production");
    }

    // ── Test 8: BOM CRUD (create/get/list/delete/update) ─────────────────────

    #[tokio::test]
    async fn bom_crud() {
        let (pool, cid) = setup().await;
        make_product_sql(&pool, "pfX", &cid, "PFX", "produs_finit", "345", "CMP").await;
        make_product_sql(&pool, "mpX", &cid, "MPX", "materie_prima", "301", "CMP").await;
        make_product_sql(&pool, "mpY", &cid, "MPY", "materie_prima", "301", "CMP").await;

        let input = BomInput {
            product_id: "pfX".into(),
            name: "BOM CRUD".into(),
            output_qty: "5".into(),
            lines: vec![
                BomLineInput {
                    component_product_id: "mpX".into(),
                    qty: "3".into(),
                    um: Some("kg".into()),
                    line_no: 1,
                },
                BomLineInput {
                    component_product_id: "mpY".into(),
                    qty: "1".into(),
                    um: None,
                    line_no: 2,
                },
            ],
        };
        let created = create_bom(&pool, &cid, input).await.unwrap();
        assert_eq!(created.bom.name, "BOM CRUD");
        assert_eq!(created.lines.len(), 2);

        let got = get_bom(&pool, &cid, &created.bom.id).await.unwrap();
        assert_eq!(got.bom.id, created.bom.id);
        assert_eq!(got.lines.len(), 2);

        let list = list_bom(&pool, &cid).await.unwrap();
        assert!(!list.is_empty());

        // Update: schimbăm liniile
        let updated = update_bom(
            &pool,
            &cid,
            &created.bom.id,
            BomInput {
                product_id: "pfX".into(),
                name: "BOM CRUD Updated".into(),
                output_qty: "2".into(),
                lines: vec![BomLineInput {
                    component_product_id: "mpX".into(),
                    qty: "7".into(),
                    um: None,
                    line_no: 1,
                }],
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.bom.name, "BOM CRUD Updated");
        assert_eq!(updated.lines.len(), 1);

        // Delete
        delete_bom(&pool, &cid, &created.bom.id).await.unwrap();
        let gone = get_bom(&pool, &cid, &created.bom.id).await;
        assert!(matches!(gone, Err(AppError::NotFound)));
    }
}
