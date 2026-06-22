//! Producție / BOM — OMFP 1802/2014 pct. 8, IAS 2 (cost complet: materiale + manoperă + regie).
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
//!   D 345 (Produse finite)                = cost COMPLET de producție
//!   C 711 (Variația stocurilor)            = cost COMPLET de producție
//!
//! Aceste note sunt generate automat de `record_movement` (post_stock_movement din gl.rs).
//! Producția NU este GL-neutră (spre deosebire de transfer) — doc_type='PRODUCTION' nu
//! setează is_transfer=true, deci GL-ul se postează normal.
//!
//! ## Cost complet capitalizat (OMFP 1802/2014 pct. 8 + IAS 2)
//!
//! full_cost = materials_cost + labour_cost + overhead_absorbed
//!
//! ### Absorbție regie (IAS 2 / pct. 8 al. 2-3):
//!
//! - Regie variabilă: ÎNTOTDEAUNA absorbită integral.
//! - Regie fixă: alocată pe capacitate normală:
//!   - activity_ratio = min(1, output_qty / normal_capacity_qty)
//!   - fixed_absorbed = overhead_fixed * activity_ratio
//!   - fixed_unabsorbed = overhead_fixed * (1 - activity_ratio) — cheltuiala perioadei, NU în 345
//! - overhead_absorbed = overhead_variable + fixed_absorbed
//! - Dacă utilizatorul introduce o singură valoare regie (fără split fix/variabil sau
//!   fără normal_capacity_qty), regia este tratată ca integral absorbabilă.
//! - Cap activity_ratio la 1 (nu supraabsorbție când output > capacitate normală).
//!
//! ### Modificare GL față de versiunea materials-only:
//!   NUMAI SUMA din linia D345=C711 se schimbă (de la materials_cost la full_cost).
//!   NU se adaugă linii D345=C641 sau D345=C6xx — aceea ar dubla cheltuiala.
//!   Consumurile (D601/602=C301/302) rămân la costul materialelor.
//!   Manopera (641/421) și regia (605/681/...) au fost deja cheltuite prin payroll / depreciere
//!   curente; creditul 711 la full_cost le compensează în măsura capitalizării în stoc.
//!
//! ### Costul stocului produs finit = full_cost → COGS la vânzare ulterioară = full_cost.
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

/// Un ordin de producție finalizat — include componentele costului complet.
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
    // Full-cost fields (added in migration 0078):
    pub labour_cost: String,
    pub overhead_cost: String,
    pub overhead_fixed: Option<String>,
    pub overhead_variable: Option<String>,
    pub normal_capacity_qty: Option<String>,
    pub overhead_absorbed: String,
    pub overhead_unabsorbed: String,
    pub full_cost: String,
    pub full_unit_cost: String,
    pub status: String,
    pub notes: Option<String>,
    // Added in migration 0087:
    pub planned_date: Option<String>,
    pub created_at: i64,
}

/// Input pentru lansarea producției — include costurile de manoperă și regie.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProduceInput {
    pub bom_id: String,
    pub gestiune_id: String,
    pub qty_produced: String,
    pub production_date: String,
    pub notes: Option<String>,
    /// Manoperă directă totală pentru acest ordin (641/421). Default 0.
    #[serde(default)]
    pub labour_cost: Option<String>,
    /// Regie totală (dacă nu se specifică split fix/variabil). Default 0.
    #[serde(default)]
    pub overhead_cost: Option<String>,
    /// Componenta FIXĂ a regiei (opțional, pentru absorbție IAS 2).
    #[serde(default)]
    pub overhead_fixed: Option<String>,
    /// Componenta VARIABILĂ a regiei (opțional).
    #[serde(default)]
    pub overhead_variable: Option<String>,
    /// Capacitate normală în unități (opțional, necesar pentru absorbție regie fixă IAS 2).
    #[serde(default)]
    pub normal_capacity_qty: Option<String>,
}

/// Input pentru crearea unui ordin planificat (fără consum stoc, fără GL).
///
/// Un ordin planificat înregistrează INTENȚIA de producție (BOM, cantitate, dată planificată,
/// gestiune, estimate costuri) fără a posta nicio mișcare de stoc sau notă GL.
/// La `execute_order()` se rulează logica completă de consum + producție + GL.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePlannedOrderInput {
    pub bom_id: String,
    pub gestiune_id: String,
    pub qty_produced: String,
    /// Data planificată a producției (YYYY-MM-DD).
    pub planned_date: String,
    /// Data efectivă — folosită la executare; dacă lipsește se folosește planned_date.
    #[serde(default)]
    pub production_date: Option<String>,
    pub notes: Option<String>,
    /// Estimare manoperă directă (informativă, NU postată la creare).
    #[serde(default)]
    pub labour_cost: Option<String>,
    /// Estimare regie totală (informativă, NU postată la creare).
    #[serde(default)]
    pub overhead_cost: Option<String>,
    #[serde(default)]
    pub overhead_fixed: Option<String>,
    #[serde(default)]
    pub overhead_variable: Option<String>,
    #[serde(default)]
    pub normal_capacity_qty: Option<String>,
}

/// Estimarea costului de producție pentru un ordin planificat.
/// Valorile sunt informative (la prețul actual de stoc); costul real se calculează la execute_order.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CostEstimate {
    /// Costul materialelor la prețul mediu curent (CMP/FIFO/LIFO estimat).
    pub estimated_material_cost: String,
    /// Estimare manoperă (din input).
    pub labour_cost: String,
    /// Estimare regie absorbită.
    pub overhead_absorbed: String,
    /// Estimare regie neabsorbită.
    pub overhead_unabsorbed: String,
    /// Estimare cost complet total.
    pub estimated_full_cost: String,
}

// ─── Absorption logic (IAS 2 / pct. 8 al. 2-3) ───────────────────────────────

/// Rezultatul calculului de absorbție a regiei.
#[derive(Debug, Clone)]
pub struct AbsorptionResult {
    /// Regia capitalizată efectiv în costul 345.
    pub overhead_absorbed: Decimal,
    /// Regia fixă neabsorbită — cheltuiala perioadei (NU în 345).
    pub overhead_unabsorbed: Decimal,
}

/// Calculează regia absorbită conform IAS 2 / OMFP 1802/2014 pct. 8 al. 2-3.
///
/// Cazuri:
///   1. Dacă se furnizează overhead_fixed + normal_capacity_qty: absorbție parțială conform IAS 2.
///   2. Dacă se furnizează overhead_variable + overhead_fixed (fără normal_capacity_qty): treat fixed ca variabilă.
///   3. Altfel (o singură valoare overhead_cost fără split): complet absorbită.
///
/// `qty_produced` — cantitatea efectiv produsă în acest ordin.
pub fn compute_absorption(
    overhead_cost: Decimal,
    overhead_fixed: Option<Decimal>,
    overhead_variable: Option<Decimal>,
    normal_capacity_qty: Option<Decimal>,
    qty_produced: Decimal,
) -> AbsorptionResult {
    // Dacă avem split fix/variabil ȘI capacitate normală → absorbție IAS 2
    if let (Some(fixed), Some(normal_cap)) = (overhead_fixed, normal_capacity_qty) {
        if normal_cap > Decimal::ZERO {
            let variable = overhead_variable.unwrap_or(Decimal::ZERO);
            // activity_ratio = min(1, qty_produced / normal_cap)
            let ratio = (qty_produced / normal_cap).min(Decimal::ONE);
            let fixed_absorbed = round2(fixed * ratio);
            let fixed_unabsorbed = round2(fixed - fixed_absorbed);
            let absorbed = round2(variable + fixed_absorbed);
            return AbsorptionResult {
                overhead_absorbed: absorbed,
                overhead_unabsorbed: fixed_unabsorbed,
            };
        }
    }
    // Fallback: tot overhead_cost este absorbit (variabilă pură sau nicio capacitate normală).
    // Dacă s-a introdus split dar fără normal_capacity, sumăm componentele.
    let total = if overhead_fixed.is_some() || overhead_variable.is_some() {
        round2(overhead_fixed.unwrap_or(Decimal::ZERO) + overhead_variable.unwrap_or(Decimal::ZERO))
    } else {
        overhead_cost
    };
    AbsorptionResult {
        overhead_absorbed: total,
        overhead_unabsorbed: Decimal::ZERO,
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s.trim()).unwrap_or(Decimal::ZERO)
}

fn opt_dec(s: &Option<String>) -> Option<Decimal> {
    s.as_deref().map(dec)
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
///   - Produce produsul finit (IN la full_cost) → GL auto: D 345 = C 711 (la full_cost)
///   - full_cost = materials_cost + labour_cost + overhead_absorbed (IAS 2 absorption)
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

    // Parsăm costurile suplimentare (default 0 dacă lipsesc sau invalid)
    let labour_cost = input
        .labour_cost
        .as_deref()
        .map(|s| Decimal::from_str(s.trim()).unwrap_or(Decimal::ZERO))
        .unwrap_or(Decimal::ZERO);
    let overhead_cost = input
        .overhead_cost
        .as_deref()
        .map(|s| Decimal::from_str(s.trim()).unwrap_or(Decimal::ZERO))
        .unwrap_or(Decimal::ZERO);
    let overhead_fixed = opt_dec(&input.overhead_fixed);
    let overhead_variable = opt_dec(&input.overhead_variable);
    let normal_capacity_qty = opt_dec(&input.normal_capacity_qty);

    if labour_cost.is_sign_negative() {
        return Err(AppError::Validation(
            "Manopera nu poate fi negativă.".into(),
        ));
    }
    if overhead_cost.is_sign_negative() {
        return Err(AppError::Validation("Regia nu poate fi negativă.".into()));
    }
    if let Some(of) = overhead_fixed {
        if of.is_sign_negative() {
            return Err(AppError::Validation(
                "Regia fixă nu poate fi negativă.".into(),
            ));
        }
    }
    if let Some(ov) = overhead_variable {
        if ov.is_sign_negative() {
            return Err(AppError::Validation(
                "Regia variabilă nu poate fi negativă.".into(),
            ));
        }
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
        return Err(AppError::Validation("output_qty BOM invalid (<=0).".into()));
    }
    let scale = qty_produced / output_qty;

    // ── Verificare stoc suficient pentru TOATE componentele (pre-consume) ────
    // Agregăm necesarul per COMPONENT DISTINCT (un component poate apărea pe mai multe linii BOM).
    // Verificarea per-linie ar lăsa o rețetă 2+3 cu stoc 4 să treacă (2<=4 SI 3<=4) și apoi să ducă
    // gestiunea la -1 — record_movement(Dir::Out) NU respinge stocul negativ, doar îl semnalează,
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

    // ── Calculul absorbției regiei (IAS 2) ────────────────────────────────────

    let absorption = compute_absorption(
        overhead_cost,
        overhead_fixed,
        overhead_variable,
        normal_capacity_qty,
        qty_produced,
    );

    // ── Costul complet capitalizat în 345 ─────────────────────────────────────
    //
    // full_cost = materiale + manoperă + regie_absorbită
    // NUMAI această sumă merge în D345=C711. NU se adaugă linii separate D345=C641/C6xx.
    // Regia fixă neabsorbită (overhead_unabsorbed) NU este capitalizată — rămâne cheltuiala
    // perioadei prin simplul fapt că nu o adăugăm la 345 (class 6 deja a absorbit-o la postarea
    // payroll / depreciere).

    let full_cost = round2(total_material_cost + labour_cost + absorption.overhead_absorbed);
    let full_unit_cost = if qty_produced.is_zero() {
        Decimal::ZERO
    } else {
        round2(full_cost / qty_produced)
    };

    // unit_cost (materials-only, backward compat) — folosit și în bonul de predare print
    let unit_cost_mat = if qty_produced.is_zero() {
        Decimal::ZERO
    } else {
        round2(total_material_cost / qty_produced)
    };

    // ── Producem produsul finit (IN la full_cost) ──────────────────────────────
    //
    // Stocul produsului finit (345) se înregistrează la full_cost per unitate.
    // Când produsul va fi vândut ulterior, COGS (D711=C345) va fi la full_cost.
    // Linia GL generată automat de record_movement/post_stock_movement:
    //   D 345 = C 711 = full_cost   ← NUMAI SUMA s-a schimbat față de materials-only.

    let in_input = StockMovementInput {
        company_id: company_id.to_string(),
        product_id: bom.product_id.clone(),
        entry_date: input.production_date.clone(),
        qty: format!("{:.6}", qty_produced),
        unit_cost: Some(format!("{:.2}", full_unit_cost)),
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

    // ── DEFECT 1 FIX: pin finished-good ledger value to exact full_cost ───────
    //
    // record_movement stores unit_cost = round2(full_cost / qty) and then
    // recompute_product_gestiune writes value = round2(qty × unit_cost), which
    // drifts by up to (qty−1) cents when full_cost is not divisible by qty
    // (e.g. full_cost=1000, qty=3 → unit=333.33, value=999.99 ≠ 1000.00).
    //
    // Strategy (b): after record_movement, find the production IN ledger row for
    // the finished good and patch its `value` to the exact full_cost.  run_value
    // is shifted by the same delta.  GL is re-posted with full_cost so that
    // D345=C711=full_cost exactly.  The product cache (stock_value) is also
    // adjusted by the same delta.
    {
        let prod_in_lookup = sqlx::query_as::<_, (String, String, String)>(
            "SELECT id, value, run_value \
             FROM stock_ledger \
             WHERE company_id=?1 AND product_id=?2 AND gestiune_id=?3 \
               AND doc_ref=?4 AND direction='IN' \
             LIMIT 1",
        )
        .bind(company_id)
        .bind(&bom.product_id)
        .bind(&input.gestiune_id)
        .bind(&order_id)
        .fetch_optional(pool)
        .await;

        let prod_in_row = match prod_in_lookup {
            Ok(r) => r,
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

        if let Some((ledger_id, stored_value_str, stored_run_value_str)) = prod_in_row {
            let stored_value = dec(&stored_value_str);
            let drift = full_cost - stored_value; // positive = engine under-recorded
            if drift != Decimal::ZERO {
                let corrected_run_value = round2(dec(&stored_run_value_str) + drift);

                // 1. Patch the ledger row value to exact full_cost.
                if let Err(e) = sqlx::query(
                    "UPDATE stock_ledger SET value=?2, run_value=?3 \
                     WHERE id=?1 AND company_id=?4",
                )
                .bind(&ledger_id)
                .bind(format!("{:.2}", full_cost))
                .bind(format!("{:.2}", corrected_run_value))
                .bind(company_id)
                .execute(pool)
                .await
                {
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

                // 2. Re-post GL with the corrected exact full_cost (D345=C711=full_cost).
                let stock_account_lookup = sqlx::query_scalar::<_, String>(
                    "SELECT COALESCE(stock_account,'345') FROM products \
                     WHERE id=?1 AND company_id=?2",
                )
                .bind(&bom.product_id)
                .bind(company_id)
                .fetch_optional(pool)
                .await;

                let stock_account = match stock_account_lookup {
                    Ok(opt) => opt.unwrap_or_else(|| "345".to_string()),
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

                if let Err(e) = crate::db::gl::post_stock_movement(
                    pool,
                    company_id,
                    &ledger_id,
                    &input.production_date,
                    &stock_account,
                    true,      // is_in
                    full_cost, // exact total, no rounding drift
                    false,     // not a transfer — posts D345=C711
                )
                .await
                {
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

                // 3. Sync the product cache (stock_value) with the drift.
                //    recompute_product_gestiune already ran inside record_movement;
                //    the patch above changes this row's value/run_value in the DB
                //    but the product cache still reflects the drifted value — adjust.
                let sv_lookup = sqlx::query_as::<_, (Option<String>,)>(
                    "SELECT stock_value FROM products WHERE id=?1 AND company_id=?2",
                )
                .bind(&bom.product_id)
                .bind(company_id)
                .fetch_optional(pool)
                .await;

                let sv_opt = match sv_lookup {
                    Ok(r) => r,
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

                let corrected_sv =
                    round2(dec(sv_opt.and_then(|(v,)| v).as_deref().unwrap_or("0")) + drift);

                if let Err(e) =
                    sqlx::query("UPDATE products SET stock_value=?2 WHERE id=?1 AND company_id=?3")
                        .bind(&bom.product_id)
                        .bind(format!("{:.2}", corrected_sv))
                        .bind(company_id)
                        .execute(pool)
                        .await
                {
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
            }
        }
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
        unit_cost: format!("{:.2}", unit_cost_mat),
        labour_cost: format!("{:.2}", labour_cost),
        overhead_cost: format!("{:.2}", overhead_cost),
        overhead_fixed: overhead_fixed.map(|d| format!("{:.2}", d)),
        overhead_variable: overhead_variable.map(|d| format!("{:.2}", d)),
        normal_capacity_qty: normal_capacity_qty.map(|d| format!("{:.6}", d)),
        overhead_absorbed: format!("{:.2}", absorption.overhead_absorbed),
        overhead_unabsorbed: format!("{:.2}", absorption.overhead_unabsorbed),
        full_cost: format!("{:.2}", full_cost),
        full_unit_cost: format!("{:.2}", full_unit_cost),
        status: "finalized".to_string(),
        notes: input.notes.clone(),
        planned_date: None, // direct-produce path; planned_date unused
        created_at: now_unix(),
    };

    let insert_res = sqlx::query(
        "INSERT INTO productie_orders \
         (id, company_id, bom_id, product_id, gestiune_id, qty_produced, production_date, \
          total_material_cost, unit_cost, labour_cost, overhead_cost, overhead_fixed, \
          overhead_variable, normal_capacity_qty, overhead_absorbed, overhead_unabsorbed, \
          full_cost, full_unit_cost, status, notes, planned_date, created_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22)",
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
    .bind(&order.labour_cost)
    .bind(&order.overhead_cost)
    .bind(&order.overhead_fixed)
    .bind(&order.overhead_variable)
    .bind(&order.normal_capacity_qty)
    .bind(&order.overhead_absorbed)
    .bind(&order.overhead_unabsorbed)
    .bind(&order.full_cost)
    .bind(&order.full_unit_cost)
    .bind(&order.status)
    .bind(&order.notes)
    .bind(&order.planned_date)
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
         production_date, total_material_cost, unit_cost, \
         COALESCE(labour_cost,'0') as labour_cost, \
         COALESCE(overhead_cost,'0') as overhead_cost, \
         overhead_fixed, overhead_variable, normal_capacity_qty, \
         COALESCE(overhead_absorbed,'0') as overhead_absorbed, \
         COALESCE(overhead_unabsorbed,'0') as overhead_unabsorbed, \
         COALESCE(full_cost, total_material_cost) as full_cost, \
         COALESCE(full_unit_cost, unit_cost) as full_unit_cost, \
         status, notes, planned_date, created_at \
         FROM productie_orders WHERE company_id=?1 \
         ORDER BY production_date DESC, created_at DESC",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?)
}

/// Listează ordinele de producție filtrate după status.
pub async fn list_productie_by_status(
    pool: &SqlitePool,
    company_id: &str,
    status: &str,
) -> AppResult<Vec<ProductieOrder>> {
    Ok(sqlx::query_as::<_, ProductieOrder>(
        "SELECT id, company_id, bom_id, product_id, gestiune_id, qty_produced, \
         production_date, total_material_cost, unit_cost, \
         COALESCE(labour_cost,'0') as labour_cost, \
         COALESCE(overhead_cost,'0') as overhead_cost, \
         overhead_fixed, overhead_variable, normal_capacity_qty, \
         COALESCE(overhead_absorbed,'0') as overhead_absorbed, \
         COALESCE(overhead_unabsorbed,'0') as overhead_unabsorbed, \
         COALESCE(full_cost, total_material_cost) as full_cost, \
         COALESCE(full_unit_cost, unit_cost) as full_unit_cost, \
         status, notes, planned_date, created_at \
         FROM productie_orders WHERE company_id=?1 AND status=?2 \
         ORDER BY production_date DESC, created_at DESC",
    )
    .bind(company_id)
    .bind(status)
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
         production_date, total_material_cost, unit_cost, \
         COALESCE(labour_cost,'0') as labour_cost, \
         COALESCE(overhead_cost,'0') as overhead_cost, \
         overhead_fixed, overhead_variable, normal_capacity_qty, \
         COALESCE(overhead_absorbed,'0') as overhead_absorbed, \
         COALESCE(overhead_unabsorbed,'0') as overhead_unabsorbed, \
         COALESCE(full_cost, total_material_cost) as full_cost, \
         COALESCE(full_unit_cost, unit_cost) as full_unit_cost, \
         status, notes, planned_date, created_at \
         FROM productie_orders WHERE id=?1 AND company_id=?2",
    )
    .bind(order_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}

// ─── Lifecycle: create_planned_order / execute_order / cancel_order ───────────

/// Creează un ordin de producție PLANIFICAT (status='planned').
///
/// Nu consumă stoc și nu postează nicio notă GL. Returnează ordinul + o estimare
/// informativă a costului la prețul mediu curent al componentelor.
///
/// RBAC: CreateDraft (creare planificare; nu PostGl).
pub async fn create_planned_order(
    pool: &SqlitePool,
    company_id: &str,
    input: CreatePlannedOrderInput,
) -> AppResult<(ProductieOrder, CostEstimate)> {
    // ── Validare cantitate ────────────────────────────────────────────────────
    let qty_produced = Decimal::from_str(input.qty_produced.trim())
        .map_err(|_| AppError::Validation("Cantitate produsă invalidă.".into()))?;
    if qty_produced <= Decimal::ZERO {
        return Err(AppError::Validation(
            "Cantitatea produsă trebuie să fie > 0.".into(),
        ));
    }

    // ── Parsăm estimările de cost (default 0) ─────────────────────────────────
    let labour_cost = input
        .labour_cost
        .as_deref()
        .map(|s| Decimal::from_str(s.trim()).unwrap_or(Decimal::ZERO))
        .unwrap_or(Decimal::ZERO);
    let overhead_cost = input
        .overhead_cost
        .as_deref()
        .map(|s| Decimal::from_str(s.trim()).unwrap_or(Decimal::ZERO))
        .unwrap_or(Decimal::ZERO);
    let overhead_fixed = opt_dec(&input.overhead_fixed);
    let overhead_variable = opt_dec(&input.overhead_variable);
    let normal_capacity_qty = opt_dec(&input.normal_capacity_qty);

    // ── Fetch BOM + guard multi-tenant ────────────────────────────────────────
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

    // ── Estimare cost materiale (la avg_cost curent) ──────────────────────────
    let output_qty = dec(&bom.output_qty);
    if output_qty <= Decimal::ZERO {
        return Err(AppError::Validation("output_qty BOM invalid (<=0).".into()));
    }
    let scale = qty_produced / output_qty;

    let mut estimated_material_cost = Decimal::ZERO;
    for line in lines {
        let needed = round2(dec(&line.qty) * scale);
        // Luăm avg_cost curent (poate fi 0 dacă nu există stoc)
        let avg_cost: Option<String> =
            sqlx::query_scalar("SELECT avg_cost FROM products WHERE id=?1 AND company_id=?2")
                .bind(&line.component_product_id)
                .bind(company_id)
                .fetch_optional(pool)
                .await?
                .flatten();
        let unit_avg = avg_cost.as_deref().map(dec).unwrap_or(Decimal::ZERO);
        estimated_material_cost += round2(needed * unit_avg);
    }

    // ── Absorbție estimată ────────────────────────────────────────────────────
    let absorption = compute_absorption(
        overhead_cost,
        overhead_fixed,
        overhead_variable,
        normal_capacity_qty,
        qty_produced,
    );

    let estimated_full_cost =
        round2(estimated_material_cost + labour_cost + absorption.overhead_absorbed);

    // ── Determintăm data producției (fallback la planned_date) ────────────────
    let production_date = input
        .production_date
        .clone()
        .unwrap_or_else(|| input.planned_date.clone());

    // ── Inserare rând cu status='planned', fără mișcări de stoc/GL ───────────
    let order_id = new_id();
    let now = now_unix();

    // Costurile estimate sunt stocate în câmpurile existente (ca estimare),
    // dar total_material_cost='0.00' și full_cost='0.00' rămân la zero până la execuție.
    sqlx::query(
        "INSERT INTO productie_orders \
         (id, company_id, bom_id, product_id, gestiune_id, qty_produced, production_date, \
          total_material_cost, unit_cost, labour_cost, overhead_cost, overhead_fixed, \
          overhead_variable, normal_capacity_qty, overhead_absorbed, overhead_unabsorbed, \
          full_cost, full_unit_cost, status, notes, planned_date, created_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,'0.00','0.00',?8,?9,?10,?11,?12,?13,?14,'0.00','0.00',\
                 'planned',?15,?16,?17)",
    )
    .bind(&order_id)
    .bind(company_id)
    .bind(&input.bom_id)
    .bind(&bom.product_id)
    .bind(&input.gestiune_id)
    .bind(format!("{:.6}", qty_produced))
    .bind(&production_date)
    .bind(format!("{:.2}", labour_cost))
    .bind(format!("{:.2}", overhead_cost))
    .bind(overhead_fixed.map(|d| format!("{:.2}", d)))
    .bind(overhead_variable.map(|d| format!("{:.2}", d)))
    .bind(normal_capacity_qty.map(|d| format!("{:.6}", d)))
    .bind(format!("{:.2}", absorption.overhead_absorbed))
    .bind(format!("{:.2}", absorption.overhead_unabsorbed))
    .bind(&input.notes)
    .bind(&input.planned_date)
    .bind(now)
    .execute(pool)
    .await?;

    let order = get_productie(pool, company_id, &order_id).await?;

    let estimate = CostEstimate {
        estimated_material_cost: format!("{:.2}", estimated_material_cost),
        labour_cost: format!("{:.2}", labour_cost),
        overhead_absorbed: format!("{:.2}", absorption.overhead_absorbed),
        overhead_unabsorbed: format!("{:.2}", absorption.overhead_unabsorbed),
        estimated_full_cost: format!("{:.2}", estimated_full_cost),
    };

    Ok((order, estimate))
}

/// Execută un ordin planificat (planned / in_progress → finalized).
///
/// Rulează logica completă de consum componente + producere produs finit + postare GL.
/// Guard idempotent: un ordin deja 'finalized' nu poate fi re-executat (fără double-consume).
/// Guard stoc: all-or-nothing — dacă vreun component este insuficient, ordinul rămâne 'planned'.
///
/// RBAC: PostGl.
pub async fn execute_order(
    pool: &SqlitePool,
    company_id: &str,
    order_id: &str,
) -> AppResult<ProductieOrder> {
    // ── Fetch ordin + guard multi-tenant ──────────────────────────────────────
    let order = get_productie(pool, company_id, order_id).await?;

    // ── Guard status ─────────────────────────────────────────────────────────
    match order.status.as_str() {
        "planned" | "in_progress" | "draft" => {} // pot fi executate
        "finalized" => {
            return Err(AppError::Validation(
                "Ordinul este deja finalizat. Re-executarea este interzisă (ar dubla consumul)."
                    .into(),
            ));
        }
        "cancelled" => {
            return Err(AppError::Validation(
                "Un ordin anulat nu poate fi executat. Creați un ordin nou.".into(),
            ));
        }
        s => {
            return Err(AppError::Validation(format!(
                "Status necunoscut: '{s}'. Nu se poate executa."
            )));
        }
    }

    // ── Construim un ProduceInput din datele ordinului planificat ─────────────
    // Preluăm data producției efectivă (production_date) din ordin.
    let produce_input = ProduceInput {
        bom_id: order.bom_id.clone(),
        gestiune_id: order.gestiune_id.clone(),
        qty_produced: order.qty_produced.clone(),
        production_date: order.production_date.clone(),
        notes: order.notes.clone(),
        labour_cost: Some(order.labour_cost.clone()),
        overhead_cost: Some(order.overhead_cost.clone()),
        overhead_fixed: order.overhead_fixed.clone(),
        overhead_variable: order.overhead_variable.clone(),
        normal_capacity_qty: order.normal_capacity_qty.clone(),
    };

    // ── Rulăm logica completă de producție (intern): validare stoc + consum + GL ─
    // Aceasta este o re-implementare care apelează direct logica din produce(),
    // dar actualizează ordinul existent în loc să îl creeze din nou.
    //
    // Strategie: create a temporary new order via the existing produce() inner logic,
    // then copy the results to the existing order row and delete the temp order.
    // Dar pentru a evita duplicarea codului, refactorizăm: extragem logica
    // de consum+GL în execute_produce_inner() și o apelăm din ambele locuri.
    //
    // Implementare concretă: apelăm direct produce_for_order() care face tot consumul
    // pe order_id-ul existent.
    execute_produce_inner(pool, company_id, order_id, produce_input).await
}

/// Logica internă de execuție a unui ordin de producție pe un order_id EXISTENT.
///
/// Folosit de `execute_order()`. Actualizează rândul existent (status → 'finalized',
/// costurile reale, mișcările de stoc + GL) în loc de a insera unul nou.
async fn execute_produce_inner(
    pool: &SqlitePool,
    company_id: &str,
    order_id: &str,
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

    let labour_cost = input
        .labour_cost
        .as_deref()
        .map(|s| Decimal::from_str(s.trim()).unwrap_or(Decimal::ZERO))
        .unwrap_or(Decimal::ZERO);
    let overhead_cost = input
        .overhead_cost
        .as_deref()
        .map(|s| Decimal::from_str(s.trim()).unwrap_or(Decimal::ZERO))
        .unwrap_or(Decimal::ZERO);
    let overhead_fixed = opt_dec(&input.overhead_fixed);
    let overhead_variable = opt_dec(&input.overhead_variable);
    let normal_capacity_qty = opt_dec(&input.normal_capacity_qty);

    // ── Fetch BOM ─────────────────────────────────────────────────────────────
    let bom_with_lines = get_bom(pool, company_id, &input.bom_id).await?;
    let bom = &bom_with_lines.bom;
    let lines = &bom_with_lines.lines;

    if lines.is_empty() {
        return Err(AppError::Validation("BOM-ul nu are componente.".into()));
    }

    // ── Scală + verificare stoc suficient ─────────────────────────────────────
    let output_qty = dec(&bom.output_qty);
    if output_qty <= Decimal::ZERO {
        return Err(AppError::Validation("output_qty BOM invalid (<=0).".into()));
    }
    let scale = qty_produced / output_qty;

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

    // ── Produse afectate (pentru rollback) ────────────────────────────────────
    let mut affected_products: Vec<String> = lines
        .iter()
        .map(|l| l.component_product_id.clone())
        .collect();
    affected_products.push(bom.product_id.clone());
    affected_products.dedup();

    // ── Consumăm componentele (OUT) ──────────────────────────────────────────
    for line in lines {
        let needed = round2(dec(&line.qty) * scale);
        let out_input = StockMovementInput {
            company_id: company_id.to_string(),
            product_id: line.component_product_id.clone(),
            entry_date: input.production_date.clone(),
            qty: format!("{:.6}", needed),
            unit_cost: None,
            doc_type: Some("PRODUCTION".to_string()),
            doc_ref: Some(order_id.to_string()),
            gestiune_id: Some(input.gestiune_id.clone()),
        };

        if let Err(e) = stock_valuation::record_movement(pool, &out_input, Dir::Out).await {
            let _ = rollback_production_movements(
                pool,
                company_id,
                order_id,
                &affected_products,
                &input.gestiune_id,
            )
            .await;
            return Err(e);
        }
    }

    // ── Costul total al materialelor ──────────────────────────────────────────
    let total_material_cost: Decimal = match sqlx::query_as::<_, (String,)>(
        "SELECT value FROM stock_ledger \
         WHERE company_id=?1 AND doc_ref=?2 AND direction='OUT'",
    )
    .bind(company_id)
    .bind(order_id)
    .fetch_all(pool)
    .await
    {
        Ok(rows) => rows.iter().map(|(v,)| dec(v)).sum(),
        Err(e) => {
            let _ = rollback_production_movements(
                pool,
                company_id,
                order_id,
                &affected_products,
                &input.gestiune_id,
            )
            .await;
            return Err(e.into());
        }
    };

    // ── Absorbție regie ───────────────────────────────────────────────────────
    let absorption = compute_absorption(
        overhead_cost,
        overhead_fixed,
        overhead_variable,
        normal_capacity_qty,
        qty_produced,
    );

    let full_cost = round2(total_material_cost + labour_cost + absorption.overhead_absorbed);
    let full_unit_cost = if qty_produced.is_zero() {
        Decimal::ZERO
    } else {
        round2(full_cost / qty_produced)
    };
    let unit_cost_mat = if qty_produced.is_zero() {
        Decimal::ZERO
    } else {
        round2(total_material_cost / qty_produced)
    };

    // ── Producem produsul finit (IN la full_cost) ──────────────────────────────
    let in_input = StockMovementInput {
        company_id: company_id.to_string(),
        product_id: bom.product_id.clone(),
        entry_date: input.production_date.clone(),
        qty: format!("{:.6}", qty_produced),
        unit_cost: Some(format!("{:.2}", full_unit_cost)),
        doc_type: Some("PRODUCTION".to_string()),
        doc_ref: Some(order_id.to_string()),
        gestiune_id: Some(input.gestiune_id.clone()),
    };

    if let Err(e) = stock_valuation::record_movement(pool, &in_input, Dir::In).await {
        let _ = rollback_production_movements(
            pool,
            company_id,
            order_id,
            &affected_products,
            &input.gestiune_id,
        )
        .await;
        return Err(e);
    }

    // ── Pinning exact full_cost (defect-fix: elimină drift rotunjire) ─────────
    {
        let prod_in_lookup = sqlx::query_as::<_, (String, String, String)>(
            "SELECT id, value, run_value \
             FROM stock_ledger \
             WHERE company_id=?1 AND product_id=?2 AND gestiune_id=?3 \
               AND doc_ref=?4 AND direction='IN' \
             LIMIT 1",
        )
        .bind(company_id)
        .bind(&bom.product_id)
        .bind(&input.gestiune_id)
        .bind(order_id)
        .fetch_optional(pool)
        .await;

        let prod_in_row = match prod_in_lookup {
            Ok(r) => r,
            Err(e) => {
                let _ = rollback_production_movements(
                    pool,
                    company_id,
                    order_id,
                    &affected_products,
                    &input.gestiune_id,
                )
                .await;
                return Err(e.into());
            }
        };

        if let Some((ledger_id, stored_value_str, stored_run_value_str)) = prod_in_row {
            let stored_value = dec(&stored_value_str);
            let drift = full_cost - stored_value;
            if drift != Decimal::ZERO {
                let corrected_run_value = round2(dec(&stored_run_value_str) + drift);

                if let Err(e) = sqlx::query(
                    "UPDATE stock_ledger SET value=?2, run_value=?3 \
                     WHERE id=?1 AND company_id=?4",
                )
                .bind(&ledger_id)
                .bind(format!("{:.2}", full_cost))
                .bind(format!("{:.2}", corrected_run_value))
                .bind(company_id)
                .execute(pool)
                .await
                {
                    let _ = rollback_production_movements(
                        pool,
                        company_id,
                        order_id,
                        &affected_products,
                        &input.gestiune_id,
                    )
                    .await;
                    return Err(e.into());
                }

                let stock_account_lookup = sqlx::query_scalar::<_, String>(
                    "SELECT COALESCE(stock_account,'345') FROM products \
                     WHERE id=?1 AND company_id=?2",
                )
                .bind(&bom.product_id)
                .bind(company_id)
                .fetch_optional(pool)
                .await;

                let stock_account = match stock_account_lookup {
                    Ok(opt) => opt.unwrap_or_else(|| "345".to_string()),
                    Err(e) => {
                        let _ = rollback_production_movements(
                            pool,
                            company_id,
                            order_id,
                            &affected_products,
                            &input.gestiune_id,
                        )
                        .await;
                        return Err(e.into());
                    }
                };

                if let Err(e) = crate::db::gl::post_stock_movement(
                    pool,
                    company_id,
                    &ledger_id,
                    &input.production_date,
                    &stock_account,
                    true,
                    full_cost,
                    false,
                )
                .await
                {
                    let _ = rollback_production_movements(
                        pool,
                        company_id,
                        order_id,
                        &affected_products,
                        &input.gestiune_id,
                    )
                    .await;
                    return Err(e);
                }

                let sv_lookup = sqlx::query_as::<_, (Option<String>,)>(
                    "SELECT stock_value FROM products WHERE id=?1 AND company_id=?2",
                )
                .bind(&bom.product_id)
                .bind(company_id)
                .fetch_optional(pool)
                .await;

                let sv_opt = match sv_lookup {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = rollback_production_movements(
                            pool,
                            company_id,
                            order_id,
                            &affected_products,
                            &input.gestiune_id,
                        )
                        .await;
                        return Err(e.into());
                    }
                };

                let corrected_sv =
                    round2(dec(sv_opt.and_then(|(v,)| v).as_deref().unwrap_or("0")) + drift);

                if let Err(e) =
                    sqlx::query("UPDATE products SET stock_value=?2 WHERE id=?1 AND company_id=?3")
                        .bind(&bom.product_id)
                        .bind(format!("{:.2}", corrected_sv))
                        .bind(company_id)
                        .execute(pool)
                        .await
                {
                    let _ = rollback_production_movements(
                        pool,
                        company_id,
                        order_id,
                        &affected_products,
                        &input.gestiune_id,
                    )
                    .await;
                    return Err(e.into());
                }
            }
        }
    }

    // ── Actualizăm rândul existent → status='finalized' + costuri reale ───────
    let update_res = sqlx::query(
        "UPDATE productie_orders SET \
         status='finalized', \
         total_material_cost=?2, unit_cost=?3, \
         labour_cost=?4, overhead_cost=?5, \
         overhead_absorbed=?6, overhead_unabsorbed=?7, \
         full_cost=?8, full_unit_cost=?9 \
         WHERE id=?1 AND company_id=?10",
    )
    .bind(order_id)
    .bind(format!("{:.2}", total_material_cost))
    .bind(format!("{:.2}", unit_cost_mat))
    .bind(format!("{:.2}", labour_cost))
    .bind(format!("{:.2}", overhead_cost))
    .bind(format!("{:.2}", absorption.overhead_absorbed))
    .bind(format!("{:.2}", absorption.overhead_unabsorbed))
    .bind(format!("{:.2}", full_cost))
    .bind(format!("{:.2}", full_unit_cost))
    .bind(company_id)
    .execute(pool)
    .await;

    if let Err(e) = update_res {
        let _ = rollback_production_movements(
            pool,
            company_id,
            order_id,
            &affected_products,
            &input.gestiune_id,
        )
        .await;
        return Err(e.into());
    }

    get_productie(pool, company_id, order_id).await
}

/// Anulează un ordin planificat (planned / draft / in_progress → cancelled).
///
/// Un ordin 'finalized' NU poate fi anulat prin această funcție — trebuie
/// folosit `rollback_production_movements` (stornare contabilă separată).
///
/// RBAC: Delete sau CreateDraft.
pub async fn cancel_order(
    pool: &SqlitePool,
    company_id: &str,
    order_id: &str,
) -> AppResult<ProductieOrder> {
    let order = get_productie(pool, company_id, order_id).await?;

    match order.status.as_str() {
        "planned" | "draft" | "in_progress" => {} // se pot anula
        "finalized" => {
            return Err(AppError::Validation(
                "Un ordin finalizat nu poate fi anulat direct — utilizați stornarea (rollback)."
                    .into(),
            ));
        }
        "cancelled" => {
            return Err(AppError::Validation("Ordinul este deja anulat.".into()));
        }
        s => {
            return Err(AppError::Validation(format!(
                "Status necunoscut: '{s}'. Nu se poate anula."
            )));
        }
    }

    sqlx::query("UPDATE productie_orders SET status='cancelled' WHERE id=?1 AND company_id=?2")
        .bind(order_id)
        .bind(company_id)
        .execute(pool)
        .await?;

    get_productie(pool, company_id, order_id).await
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

    // ── Helper: GL turnover per account ───────────────────────────────────────

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
                name: "Reteta test".into(),
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
                labour_cost: None,
                overhead_cost: None,
                overhead_fixed: None,
                overhead_variable: None,
                normal_capacity_qty: None,
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
        // Fără manoperă/regie, full_cost == total_material_cost
        assert_eq!(
            order.full_cost, "190.00",
            "full_cost=190 when no labour/overhead"
        );
        assert_eq!(order.full_unit_cost, "19.00", "full_unit_cost=19");
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
                labour_cost: None,
                overhead_cost: None,
                overhead_fixed: None,
                overhead_variable: None,
                normal_capacity_qty: None,
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
    /// component on two lines (2+3=5) with on-hand 4 used to pass both per-line checks (2<=4, 3<=4)
    /// and drive the gestiune to -1; now the aggregate need (5) is rejected before any consumption.
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
                labour_cost: None,
                overhead_cost: None,
                overhead_fixed: None,
                overhead_variable: None,
                normal_capacity_qty: None,
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
                labour_cost: None,
                overhead_cost: None,
                overhead_fixed: None,
                overhead_variable: None,
                normal_capacity_qty: None,
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

    /// GL monografie: consum materie prima → D601=C301; obtinere → D345=C711.
    /// Verificăm TOTAL debit/credit (turnover) per cont, nu soldul net (closing balance).
    #[tokio::test]
    async fn gl_monografie_601_301_345_711() {
        let (pool, cid) = setup().await;
        let g = make_gestiune(&pool, &cid, "G3").await;

        make_product_sql(&pool, "pf3", &cid, "PF3", "produs_finit", "345", "CMP").await;
        make_product_sql(&pool, "mp3", &cid, "MP3", "materie_prima", "301", "CMP").await;

        // Stoc: 10 @ 4.00 = 40
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
                labour_cost: None,
                overhead_cost: None,
                overhead_fixed: None,
                overhead_variable: None,
                normal_capacity_qty: None,
            },
        )
        .await
        .unwrap();

        let (d301, c301) = gl_turnover(&pool, &cid, "301").await;
        let (d601, _c601) = gl_turnover(&pool, &cid, "601").await;
        let (d345, _c345) = gl_turnover(&pool, &cid, "345").await;
        let (_d711, c711) = gl_turnover(&pool, &cid, "711").await;

        assert!(
            c301 >= Decimal::from(24),
            "301 total_credit >= 24 (consum), got {c301}"
        );
        assert!(
            d601 >= Decimal::from(24),
            "601 total_debit >= 24 (consum), got {d601}"
        );
        assert!(
            d345 >= Decimal::from(24),
            "345 total_debit >= 24 (obtinere), got {d345}"
        );
        assert!(
            c711 >= Decimal::from(24),
            "711 total_credit >= 24 (obtinere), got {c711}"
        );
        assert!(
            d601 > Decimal::ZERO,
            "D601 must be posted for materie_prima"
        );
        assert!(
            c301 > Decimal::ZERO,
            "C301 must be posted for materie_prima"
        );

        // GL global echilibrat
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

        let _ = d301; // used indirectly
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
                labour_cost: None,
                overhead_cost: None,
                overhead_fixed: None,
                overhead_variable: None,
                normal_capacity_qty: None,
            },
        )
        .await;

        assert!(
            matches!(result, Err(AppError::Validation(_))),
            "Trebuie respins cu Validation"
        );

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

        assert_eq!(onhand(&pool, &cid, "mp4a", &g).await, Decimal::from(5));
        assert_eq!(onhand(&pool, &cid, "mp4b", &g).await, Decimal::from(2));
        assert_eq!(onhand(&pool, &cid, "pf4", &g).await, Decimal::ZERO);
    }

    // ── Test 5: atomicitate (compensare manuală) ──────────────────────────────

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

        rollback_production_movements(
            &pool,
            &cid,
            fake_order,
            &["mp5".to_string(), "pf5".to_string()],
            &g,
        )
        .await
        .unwrap();

        let restored = onhand(&pool, &cid, "mp5", &g).await;
        assert_eq!(
            restored,
            Decimal::from(20),
            "After rollback: qty restored to 20"
        );

        let remaining_rows: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM stock_ledger WHERE company_id=?1 AND doc_ref=?2",
        )
        .bind(&cid)
        .bind(fake_order)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(remaining_rows, 0, "No ledger rows after rollback");

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
                labour_cost: None,
                overhead_cost: None,
                overhead_fixed: None,
                overhead_variable: None,
                normal_capacity_qty: None,
            },
        )
        .await
        .unwrap();

        let before: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_journal WHERE company_id=?1 AND source_type='STOCK'",
        )
        .bind(&cid)
        .fetch_one(&pool)
        .await
        .unwrap();

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
                labour_cost: None,
                overhead_cost: None,
                overhead_fixed: None,
                overhead_variable: None,
                normal_capacity_qty: None,
            },
        )
        .await
        .unwrap();

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

        delete_bom(&pool, &cid, &created.bom.id).await.unwrap();
        let gone = get_bom(&pool, &cid, &created.bom.id).await;
        assert!(matches!(gone, Err(AppError::NotFound)));
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // ── FULL COST TESTS (new in migration 0078) ───────────────────────────────
    // ═══════════════════════════════════════════════════════════════════════════

    // ── Test FC-1: materials=1000, labour=300, overhead=200 (single figure) ───

    /// Full cost: D345=C711 postat la 1500; OUT-urile materialelor rămân la 1000.
    #[tokio::test]
    async fn full_cost_materials_labour_overhead_single() {
        let (pool, cid) = setup().await;
        let g = make_gestiune(&pool, &cid, "GFC1").await;

        make_product_sql(
            &pool,
            "pf_fc1",
            &cid,
            "PF_FC1",
            "produs_finit",
            "345",
            "CMP",
        )
        .await;
        make_product_sql(
            &pool,
            "mp_fc1",
            &cid,
            "MP_FC1",
            "materie_prima",
            "301",
            "CMP",
        )
        .await;

        // Stoc: 100 @ 10.00 = 1000 (vom consuma tot pentru 1 unitate)
        record_movement(
            &pool,
            &mv(&cid, "mp_fc1", "2026-01-01", "100", Some("10.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();

        let bom = create_bom(
            &pool,
            &cid,
            BomInput {
                product_id: "pf_fc1".into(),
                name: "BOM_FC1".into(),
                output_qty: "1".into(),
                lines: vec![BomLineInput {
                    component_product_id: "mp_fc1".into(),
                    qty: "100".into(),
                    um: None,
                    line_no: 1,
                }],
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
                labour_cost: Some("300.00".into()),
                overhead_cost: Some("200.00".into()),
                overhead_fixed: None,
                overhead_variable: None,
                normal_capacity_qty: None,
            },
        )
        .await
        .unwrap();

        // (1) full_cost = 1000 + 300 + 200 = 1500
        assert_eq!(order.total_material_cost, "1000.00", "material cost = 1000");
        assert_eq!(order.labour_cost, "300.00", "labour = 300");
        assert_eq!(order.overhead_absorbed, "200.00", "overhead absorbed = 200");
        assert_eq!(
            order.overhead_unabsorbed, "0.00",
            "no unabsorbed (single figure)"
        );
        assert_eq!(order.full_cost, "1500.00", "full_cost = 1500");
        assert_eq!(
            order.full_unit_cost, "1500.00",
            "full_unit_cost = 1500 (1 unit)"
        );

        // (2) D345=C711 trebui să fie la 1500
        let (d345, _) = gl_turnover(&pool, &cid, "345").await;
        let (_, c711) = gl_turnover(&pool, &cid, "711").await;
        assert_eq!(d345, Decimal::from(1500), "D345 = 1500 (full cost)");
        assert_eq!(c711, Decimal::from(1500), "C711 = 1500 (full cost)");

        // (3) Materialele OUT legs la 1000 (D601=C301 la 1000)
        let (d601, _) = gl_turnover(&pool, &cid, "601").await;
        let (_, c301) = gl_turnover(&pool, &cid, "301").await;
        assert_eq!(
            d601,
            Decimal::from(1000),
            "material OUT D601 = 1000 (NOT 1500)"
        );
        assert_eq!(
            c301,
            Decimal::from(1000),
            "material OUT C301 = 1000 (NOT 1500)"
        );

        // (4) GL echilibrat
        let row: (Option<f64>, Option<f64>) = sqlx::query_as(
            "SELECT SUM(CAST(e.debit AS REAL)), SUM(CAST(e.credit AS REAL)) \
             FROM gl_journal j JOIN gl_entry e ON e.journal_pk = j.id WHERE j.company_id=?1",
        )
        .bind(&cid)
        .fetch_one(&pool)
        .await
        .unwrap();
        let td = Decimal::try_from(row.0.unwrap_or(0.0))
            .unwrap_or(Decimal::ZERO)
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
        let tc = Decimal::try_from(row.1.unwrap_or(0.0))
            .unwrap_or(Decimal::ZERO)
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
        assert_eq!(
            td, tc,
            "GL balanced after full cost production: {td} == {tc}"
        );

        // (5) Stocul produs finit valorat la full_cost (1500)
        let inv_val: Option<String> = sqlx::query_scalar(
            "SELECT run_value FROM stock_ledger WHERE company_id=?1 AND product_id='pf_fc1' AND direction='IN' LIMIT 1"
        )
        .bind(&cid)
        .fetch_optional(&pool)
        .await
        .unwrap();
        let inv_val_d = dec(inv_val.as_deref().unwrap_or("0"));
        assert_eq!(
            inv_val_d,
            Decimal::from(1500),
            "finished good inventory value = 1500 (full cost)"
        );
    }

    // ── Test FC-2: absorption — overhead_fixed=400, normal_capacity=100, output=80 ─

    /// activity_ratio = 0.8; fixed_absorbed = 320; unabsorbed = 80; full_cost excludes unabsorbed.
    #[tokio::test]
    async fn full_cost_absorption_partial_fixed() {
        let (pool, cid) = setup().await;
        let g = make_gestiune(&pool, &cid, "GFC2").await;

        make_product_sql(
            &pool,
            "pf_fc2",
            &cid,
            "PF_FC2",
            "produs_finit",
            "345",
            "CMP",
        )
        .await;
        make_product_sql(
            &pool,
            "mp_fc2",
            &cid,
            "MP_FC2",
            "materie_prima",
            "301",
            "CMP",
        )
        .await;

        // Stoc: 80 @ 1.00 = 80 (vom consuma 80 unități, cost materiale = 80)
        record_movement(
            &pool,
            &mv(&cid, "mp_fc2", "2026-01-01", "80", Some("1.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();

        let bom = create_bom(
            &pool,
            &cid,
            BomInput {
                product_id: "pf_fc2".into(),
                name: "BOM_FC2".into(),
                output_qty: "1".into(),
                lines: vec![BomLineInput {
                    component_product_id: "mp_fc2".into(),
                    qty: "1".into(),
                    um: None,
                    line_no: 1,
                }],
            },
        )
        .await
        .unwrap();

        // output=80, normal_capacity=100, overhead_fixed=400, overhead_variable=0
        // activity_ratio = min(1, 80/100) = 0.8
        // fixed_absorbed = 400 * 0.8 = 320
        // fixed_unabsorbed = 400 * 0.2 = 80
        // full_cost = 80 (mat) + 0 (labour) + 320 (absorbed) = 400
        let order = produce(
            &pool,
            &cid,
            ProduceInput {
                bom_id: bom.bom.id.clone(),
                gestiune_id: g.clone(),
                qty_produced: "80".into(),
                production_date: "2026-01-10".into(),
                notes: None,
                labour_cost: Some("0.00".into()),
                overhead_cost: Some("400.00".into()),
                overhead_fixed: Some("400.00".into()),
                overhead_variable: Some("0.00".into()),
                normal_capacity_qty: Some("100".into()),
            },
        )
        .await
        .unwrap();

        assert_eq!(order.total_material_cost, "80.00", "material cost = 80");
        assert_eq!(
            order.overhead_absorbed, "320.00",
            "overhead absorbed = 320 (not 400)"
        );
        assert_eq!(order.overhead_unabsorbed, "80.00", "unabsorbed = 80");
        assert_eq!(order.full_cost, "400.00", "full_cost = 80 + 0 + 320 = 400");

        // Unabsorbed (80) NOT in 345 valuation
        let (d345, _) = gl_turnover(&pool, &cid, "345").await;
        assert_eq!(
            d345,
            Decimal::from(400),
            "D345 = 400 (not 480 — unabsorbed excluded)"
        );

        // GL echilibrat
        let row: (Option<f64>, Option<f64>) = sqlx::query_as(
            "SELECT SUM(CAST(e.debit AS REAL)), SUM(CAST(e.credit AS REAL)) \
             FROM gl_journal j JOIN gl_entry e ON e.journal_pk = j.id WHERE j.company_id=?1",
        )
        .bind(&cid)
        .fetch_one(&pool)
        .await
        .unwrap();
        let td = Decimal::try_from(row.0.unwrap_or(0.0))
            .unwrap_or(Decimal::ZERO)
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
        let tc = Decimal::try_from(row.1.unwrap_or(0.0))
            .unwrap_or(Decimal::ZERO)
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
        assert_eq!(td, tc, "GL balanced after partial absorption");
    }

    // ── Test FC-3: output > normal_capacity → activity_ratio capped at 1 ─────

    /// No over-absorption: when qty_produced=120, normal_capacity=100,
    /// ratio=min(1, 120/100)=1 → fixed_absorbed=overhead_fixed (fully absorbed).
    #[tokio::test]
    async fn full_cost_absorption_cap_at_one() {
        // Pure unit test on compute_absorption — no DB needed.
        let result = compute_absorption(
            Decimal::from(400),       // overhead_cost (unused when split provided)
            Some(Decimal::from(400)), // overhead_fixed
            Some(Decimal::ZERO),      // overhead_variable
            Some(Decimal::from(100)), // normal_capacity_qty
            Decimal::from(120),       // qty_produced > capacity
        );
        // activity_ratio = min(1, 120/100) = 1
        // fixed_absorbed = 400 * 1 = 400, unabsorbed = 0
        assert_eq!(
            result.overhead_absorbed,
            Decimal::from(400),
            "fully absorbed (capped at 1)"
        );
        assert_eq!(
            result.overhead_unabsorbed,
            Decimal::ZERO,
            "no unabsorbed when output > capacity"
        );
    }

    // ── Test FC-4: materials-only (labour=0, overhead=0) → unchanged behaviour ─

    /// Backward compat: materials-only produce still works; full_cost == total_material_cost.
    #[tokio::test]
    async fn full_cost_materials_only_unchanged() {
        let (pool, cid) = setup().await;
        let g = make_gestiune(&pool, &cid, "GFC4").await;

        make_product_sql(
            &pool,
            "pf_fc4",
            &cid,
            "PF_FC4",
            "produs_finit",
            "345",
            "CMP",
        )
        .await;
        make_product_sql(
            &pool,
            "mp_fc4",
            &cid,
            "MP_FC4",
            "materie_prima",
            "301",
            "CMP",
        )
        .await;

        record_movement(
            &pool,
            &mv(&cid, "mp_fc4", "2026-01-01", "10", Some("4.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();

        let bom = create_bom(
            &pool,
            &cid,
            BomInput {
                product_id: "pf_fc4".into(),
                name: "BOM_FC4".into(),
                output_qty: "1".into(),
                lines: vec![BomLineInput {
                    component_product_id: "mp_fc4".into(),
                    qty: "2".into(),
                    um: None,
                    line_no: 1,
                }],
            },
        )
        .await
        .unwrap();

        // No labour, no overhead → should behave exactly as before
        let order = produce(
            &pool,
            &cid,
            ProduceInput {
                bom_id: bom.bom.id.clone(),
                gestiune_id: g.clone(),
                qty_produced: "3".into(),
                production_date: "2026-01-10".into(),
                notes: None,
                labour_cost: None,
                overhead_cost: None,
                overhead_fixed: None,
                overhead_variable: None,
                normal_capacity_qty: None,
            },
        )
        .await
        .unwrap();

        // 3 × 2 × 4.00 = 24.00 materials; full_cost must equal materials_cost
        assert_eq!(order.total_material_cost, "24.00", "material cost = 24");
        assert_eq!(
            order.full_cost, "24.00",
            "full_cost == material cost when no labour/overhead"
        );
        assert_eq!(order.labour_cost, "0.00", "labour = 0");
        assert_eq!(order.overhead_absorbed, "0.00", "overhead absorbed = 0");
        assert_eq!(order.overhead_unabsorbed, "0.00", "unabsorbed = 0");

        // D345=C711 must equal materials_cost (24), not more
        let (d345, _) = gl_turnover(&pool, &cid, "345").await;
        let (_, c711) = gl_turnover(&pool, &cid, "711").await;
        assert_eq!(
            d345,
            Decimal::from(24),
            "D345 = 24 (unchanged, materials only)"
        );
        assert_eq!(
            c711,
            Decimal::from(24),
            "C711 = 24 (unchanged, materials only)"
        );
    }

    // ── Test FC-5: the obținere journal balances ──────────────────────────────

    /// With full_cost, the single D345=C711 line must still balance by construction.
    /// (Implicitly tested by GL balance checks in FC-1 and FC-2; this test is explicit.)
    #[tokio::test]
    async fn full_cost_obtinere_journal_balances() {
        let (pool, cid) = setup().await;
        let g = make_gestiune(&pool, &cid, "GFC5").await;

        make_product_sql(
            &pool,
            "pf_fc5",
            &cid,
            "PF_FC5",
            "produs_finit",
            "345",
            "CMP",
        )
        .await;
        make_product_sql(
            &pool,
            "mp_fc5",
            &cid,
            "MP_FC5",
            "materie_prima",
            "301",
            "CMP",
        )
        .await;

        record_movement(
            &pool,
            &mv(&cid, "mp_fc5", "2026-01-01", "50", Some("2.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();

        let bom = create_bom(
            &pool,
            &cid,
            BomInput {
                product_id: "pf_fc5".into(),
                name: "BOM_FC5".into(),
                output_qty: "1".into(),
                lines: vec![BomLineInput {
                    component_product_id: "mp_fc5".into(),
                    qty: "5".into(),
                    um: None,
                    line_no: 1,
                }],
            },
        )
        .await
        .unwrap();

        // labour=150, overhead_variable=50 → absorbed=50; full_cost=5×10×2 + 150 + 50 = 300
        // Actually: mat = 5 qty_per_output * qty_produced(1) * 2.00/unit ... wait:
        // scale=1, consume 5 @ 2.00 = 10.00; labour=150; overhead_var=50; full=210
        let order = produce(
            &pool,
            &cid,
            ProduceInput {
                bom_id: bom.bom.id.clone(),
                gestiune_id: g.clone(),
                qty_produced: "1".into(),
                production_date: "2026-01-10".into(),
                notes: None,
                labour_cost: Some("150.00".into()),
                overhead_cost: Some("50.00".into()),
                overhead_fixed: None,
                overhead_variable: None,
                normal_capacity_qty: None,
            },
        )
        .await
        .unwrap();

        let mat = dec(&order.total_material_cost); // 5 * 2 = 10
        let full = dec(&order.full_cost); // 10 + 150 + 50 = 210
        assert_eq!(
            full,
            mat + Decimal::from(200),
            "full_cost = mat(10) + 200 = 210"
        );

        // The D345=C711 debit and credit must be equal (= full_cost).
        // The raw material receipt goes to D301/C607, NOT D345.
        // So D345 total = only the production obtinere IN leg = full_cost.
        let (d345, c345) = gl_turnover(&pool, &cid, "345").await;
        let (d711, c711) = gl_turnover(&pool, &cid, "711").await;
        assert_eq!(
            d345, full,
            "D345 total = full_cost only (no extra legs — materials receipt is D301/C607)"
        );
        // Check the production IN leg directly via stock_ledger.
        let prod_in_val: Option<String> = sqlx::query_scalar(
            "SELECT value FROM stock_ledger WHERE company_id=?1 AND product_id='pf_fc5' AND direction='IN' LIMIT 1"
        )
        .bind(&cid)
        .fetch_optional(&pool)
        .await
        .unwrap();
        let prod_val = dec(prod_in_val.as_deref().unwrap_or("0"));
        assert_eq!(
            prod_val, full,
            "stock_ledger IN value for finished good = full_cost"
        );

        // D711 (debit) from closing/future sale movements = 0 here (no sale yet).
        // C711 from obtinere = full_cost.
        assert_eq!(c711, full, "C711 = full_cost from obtinere leg");
        let _ = (c345, d711); // suppress unused warnings
    }

    // ── Test FC-6: absorption pure unit tests ────────────────────────────────

    #[test]
    fn absorption_variable_only() {
        // No split, no normal_cap: overhead fully absorbed
        let r = compute_absorption(Decimal::from(200), None, None, None, Decimal::from(10));
        assert_eq!(r.overhead_absorbed, Decimal::from(200));
        assert_eq!(r.overhead_unabsorbed, Decimal::ZERO);
    }

    #[test]
    fn absorption_fixed_at_full_capacity() {
        // output == normal_capacity → ratio=1 → fully absorbed
        let r = compute_absorption(
            Decimal::from(400),
            Some(Decimal::from(400)),
            Some(Decimal::ZERO),
            Some(Decimal::from(100)),
            Decimal::from(100),
        );
        assert_eq!(r.overhead_absorbed, Decimal::from(400));
        assert_eq!(r.overhead_unabsorbed, Decimal::ZERO);
    }

    #[test]
    fn absorption_fixed_partial_80_pct() {
        // output=80, normal=100 → ratio=0.8 → absorbed=320, unabsorbed=80
        let r = compute_absorption(
            Decimal::from(400),
            Some(Decimal::from(400)),
            Some(Decimal::ZERO),
            Some(Decimal::from(100)),
            Decimal::from(80),
        );
        assert_eq!(r.overhead_absorbed, Decimal::from(320));
        assert_eq!(r.overhead_unabsorbed, Decimal::from(80));
    }

    #[test]
    fn absorption_mixed_fixed_variable() {
        // fixed=300 @80%, variable=100 always → absorbed = 240+100=340, unabsorbed=60
        let r = compute_absorption(
            Decimal::from(400),
            Some(Decimal::from(300)),
            Some(Decimal::from(100)),
            Some(Decimal::from(100)),
            Decimal::from(80),
        );
        assert_eq!(r.overhead_absorbed, Decimal::from(340));
        assert_eq!(r.overhead_unabsorbed, Decimal::from(60));
    }

    #[test]
    fn absorption_zero_overhead() {
        let r = compute_absorption(Decimal::ZERO, None, None, None, Decimal::from(10));
        assert_eq!(r.overhead_absorbed, Decimal::ZERO);
        assert_eq!(r.overhead_unabsorbed, Decimal::ZERO);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // ── DEFECT-FIX TESTS ─────────────────────────────────────────────────────
    // ═══════════════════════════════════════════════════════════════════════════

    // ── DF-1: non-dividing qty=3, full_cost=1000 → D345=C711=1000 exactly ────

    /// Regression: qty=3 divides 1000 into 333.33 per unit, which round-trips as
    /// 999.99 (3×333.33).  The defect-1 fix must pin D345=C711=1000 exactly and
    /// the finished-good inventory ledger value must be 1000, not 999.99.
    #[tokio::test]
    async fn rounding_drift_non_dividing_qty3_full_cost_1000() {
        let (pool, cid) = setup().await;
        let g = make_gestiune(&pool, &cid, "GDF1").await;

        make_product_sql(
            &pool,
            "pf_df1",
            &cid,
            "PF_DF1",
            "produs_finit",
            "345",
            "CMP",
        )
        .await;
        make_product_sql(
            &pool,
            "mp_df1",
            &cid,
            "MP_DF1",
            "materie_prima",
            "301",
            "CMP",
        )
        .await;

        // 3 units of material @ 333.333... each → total 1000.00 (stored as 1000)
        // We seed 3 @ 333.33 + 1 @ 0.01 = 1000.00 to match exactly, but the simpler
        // approach: seed 1000 @ 1.00 then BOM uses all 1000 per 1 unit output.
        record_movement(
            &pool,
            &mv(&cid, "mp_df1", "2026-01-01", "1000", Some("1.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();

        let bom = create_bom(
            &pool,
            &cid,
            BomInput {
                product_id: "pf_df1".into(),
                name: "BOM_DF1".into(),
                // BOM produces 3 units consuming 1000 material (scale=1 when qty_produced=3).
                output_qty: "3".into(),
                lines: vec![BomLineInput {
                    component_product_id: "mp_df1".into(),
                    qty: "1000".into(), // 1000 material for 3 units → full_cost=1000
                    um: None,
                    line_no: 1,
                }],
            },
        )
        .await
        .unwrap();

        // Produce qty=3 (one BOM run, scale=1). full_cost = 1000.
        // unit = round2(1000/3) = 333.33; 3×333.33 = 999.99 → drift 0.01 without fix.
        // With the fix: value is pinned to 1000.00 exactly.
        let order = produce(
            &pool,
            &cid,
            ProduceInput {
                bom_id: bom.bom.id.clone(),
                gestiune_id: g.clone(),
                qty_produced: "3".into(),
                production_date: "2026-01-10".into(),
                notes: None,
                labour_cost: None,
                overhead_cost: None,
                overhead_fixed: None,
                overhead_variable: None,
                normal_capacity_qty: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(order.full_cost, "1000.00", "order.full_cost = 1000.00");

        // The finished-good stock ledger IN row must carry value = 1000.00 exactly.
        let inv_val: Option<String> = sqlx::query_scalar(
            "SELECT value FROM stock_ledger \
             WHERE company_id=?1 AND product_id='pf_df1' AND direction='IN' LIMIT 1",
        )
        .bind(&cid)
        .fetch_optional(&pool)
        .await
        .unwrap();
        let inv_val_d = dec(inv_val.as_deref().unwrap_or("0"));
        assert_eq!(
            inv_val_d,
            Decimal::from(1000),
            "finished-good inventory ledger value must be exactly 1000, not 999.99"
        );

        // D345=C711 must both equal 1000.00 exactly.
        let (d345, _) = gl_turnover(&pool, &cid, "345").await;
        let (_, c711) = gl_turnover(&pool, &cid, "711").await;
        assert_eq!(d345, Decimal::from(1000), "D345 must be exactly 1000");
        assert_eq!(c711, Decimal::from(1000), "C711 must be exactly 1000");

        // product cache stock_value must equal 1000.00.
        let sv: Option<String> = sqlx::query_scalar(
            "SELECT stock_value FROM products WHERE id='pf_df1' AND company_id=?1",
        )
        .bind(&cid)
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert_eq!(
            dec(sv.as_deref().unwrap_or("0")),
            Decimal::from(1000),
            "product cache stock_value must be exactly 1000"
        );

        // GL must be balanced.
        let row: (Option<f64>, Option<f64>) = sqlx::query_as(
            "SELECT SUM(CAST(e.debit AS REAL)), SUM(CAST(e.credit AS REAL)) \
             FROM gl_journal j JOIN gl_entry e ON e.journal_pk = j.id WHERE j.company_id=?1",
        )
        .bind(&cid)
        .fetch_one(&pool)
        .await
        .unwrap();
        let td = Decimal::try_from(row.0.unwrap_or(0.0))
            .unwrap_or(Decimal::ZERO)
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
        let tc = Decimal::try_from(row.1.unwrap_or(0.0))
            .unwrap_or(Decimal::ZERO)
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
        assert_eq!(td, tc, "GL balanced: {td} == {tc}");
    }

    // ── DF-2: non-dividing qty=7, full_cost=100 → D345=C711=100 exactly ──────

    /// materials=100, labour=0, overhead=0, qty=7 → full_cost=100.
    /// unit_cost = round2(100/7) = 14.29; 7×14.29 = 100.03 → drift +0.03.
    /// With fix: value pinned to 100.00 exactly.
    #[tokio::test]
    async fn rounding_drift_non_dividing_qty7_full_cost_100() {
        let (pool, cid) = setup().await;
        let g = make_gestiune(&pool, &cid, "GDF2").await;

        make_product_sql(
            &pool,
            "pf_df2",
            &cid,
            "PF_DF2",
            "produs_finit",
            "345",
            "CMP",
        )
        .await;
        make_product_sql(
            &pool,
            "mp_df2",
            &cid,
            "MP_DF2",
            "materie_prima",
            "301",
            "CMP",
        )
        .await;

        // Seed 7 @ 100/7 ≈ 14.286 each; to get full_cost=100.00 exactly, seed
        // 100 @ 1.00 and consume all 100 per output unit × qty=7.
        // Actually: 7 units of BOM output, consuming 100 material per output:
        // total material = 700. That's full_cost=700, not 100.
        // Instead: consume 100 material total for qty=7 output.
        // BOM: output_qty=1, consume 100/7 ≈ 14.286 per unit → not nice.
        // Simpler: seed 100 @ 1.00; BOM consumes all 100 per output_qty=7
        // → scale = 7/7 = 1 → consume 100 → total_material_cost=100.
        record_movement(
            &pool,
            &mv(&cid, "mp_df2", "2026-01-01", "100", Some("1.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();

        let bom = create_bom(
            &pool,
            &cid,
            BomInput {
                product_id: "pf_df2".into(),
                name: "BOM_DF2".into(),
                output_qty: "7".into(), // 7 units per BOM run
                lines: vec![BomLineInput {
                    component_product_id: "mp_df2".into(),
                    qty: "100".into(), // 100 material units per BOM run
                    um: None,
                    line_no: 1,
                }],
            },
        )
        .await
        .unwrap();

        // Produce qty=7 (one BOM run). scale=1. material_cost=100.
        // unit = round2(100/7) = 14.29; 7×14.29 = 100.03 → without fix: drift.
        let order = produce(
            &pool,
            &cid,
            ProduceInput {
                bom_id: bom.bom.id.clone(),
                gestiune_id: g.clone(),
                qty_produced: "7".into(),
                production_date: "2026-01-10".into(),
                notes: None,
                labour_cost: None,
                overhead_cost: None,
                overhead_fixed: None,
                overhead_variable: None,
                normal_capacity_qty: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(
            order.total_material_cost, "100.00",
            "total_material_cost = 100"
        );
        assert_eq!(order.full_cost, "100.00", "full_cost = 100.00");

        let inv_val: Option<String> = sqlx::query_scalar(
            "SELECT value FROM stock_ledger \
             WHERE company_id=?1 AND product_id='pf_df2' AND direction='IN' LIMIT 1",
        )
        .bind(&cid)
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert_eq!(
            dec(inv_val.as_deref().unwrap_or("0")),
            Decimal::from(100),
            "finished-good inventory value must be exactly 100 (not 100.03)"
        );

        let (d345, _) = gl_turnover(&pool, &cid, "345").await;
        let (_, c711) = gl_turnover(&pool, &cid, "711").await;
        assert_eq!(d345, Decimal::from(100), "D345 = 100 exactly");
        assert_eq!(c711, Decimal::from(100), "C711 = 100 exactly");
    }

    // ── DF-3: evenly-dividing case still passes (regression guard) ────────────

    /// Evenly-dividing case: qty=4, material 5 @ 10.00 = 50. full_cost=50.
    /// unit_cost = round2(50/4) = 12.50; 4×12.50 = 50.00 → no drift, fix is no-op.
    #[tokio::test]
    async fn rounding_no_drift_evenly_dividing() {
        let (pool, cid) = setup().await;
        let g = make_gestiune(&pool, &cid, "GDF3").await;

        make_product_sql(
            &pool,
            "pf_df3",
            &cid,
            "PF_DF3",
            "produs_finit",
            "345",
            "CMP",
        )
        .await;
        make_product_sql(
            &pool,
            "mp_df3",
            &cid,
            "MP_DF3",
            "materie_prima",
            "301",
            "CMP",
        )
        .await;

        record_movement(
            &pool,
            &mv(&cid, "mp_df3", "2026-01-01", "20", Some("10.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();

        let bom = create_bom(
            &pool,
            &cid,
            BomInput {
                product_id: "pf_df3".into(),
                name: "BOM_DF3".into(),
                output_qty: "1".into(),
                lines: vec![BomLineInput {
                    component_product_id: "mp_df3".into(),
                    qty: "5".into(),
                    um: None,
                    line_no: 1,
                }],
            },
        )
        .await
        .unwrap();

        // qty=4, mat_per_unit=5, cost=10.00 → total=200; unit=200/4=50.00; 4×50=200 (exact).
        let order = produce(
            &pool,
            &cid,
            ProduceInput {
                bom_id: bom.bom.id.clone(),
                gestiune_id: g.clone(),
                qty_produced: "4".into(),
                production_date: "2026-01-10".into(),
                notes: None,
                labour_cost: None,
                overhead_cost: None,
                overhead_fixed: None,
                overhead_variable: None,
                normal_capacity_qty: None,
            },
        )
        .await
        .unwrap();

        // total_material_cost = 4*5*10 = 200; full_cost = 200; full_unit_cost = 50.
        assert_eq!(
            order.full_cost, "200.00",
            "full_cost = 200.00 (evenly divides)"
        );
        assert_eq!(
            order.full_unit_cost, "50.00",
            "full_unit_cost = 50.00 (no drift)"
        );

        let inv_val: Option<String> = sqlx::query_scalar(
            "SELECT value FROM stock_ledger \
             WHERE company_id=?1 AND product_id='pf_df3' AND direction='IN' LIMIT 1",
        )
        .bind(&cid)
        .fetch_optional(&pool)
        .await
        .unwrap();
        assert_eq!(
            dec(inv_val.as_deref().unwrap_or("0")),
            Decimal::from(200),
            "inventory value = 200 (evenly-dividing case: 4×50 = 200)"
        );

        let (d345, _) = gl_turnover(&pool, &cid, "345").await;
        let (_, c711) = gl_turnover(&pool, &cid, "711").await;
        assert_eq!(d345, Decimal::from(200), "D345 = 200");
        assert_eq!(c711, Decimal::from(200), "C711 = 200");
    }

    // ── DF-4: negative overhead_fixed → Validation error ─────────────────────

    #[tokio::test]
    async fn negative_overhead_fixed_rejected() {
        let (pool, cid) = setup().await;
        let g = make_gestiune(&pool, &cid, "GDF4").await;

        make_product_sql(
            &pool,
            "pf_df4",
            &cid,
            "PF_DF4",
            "produs_finit",
            "345",
            "CMP",
        )
        .await;
        make_product_sql(
            &pool,
            "mp_df4",
            &cid,
            "MP_DF4",
            "materie_prima",
            "301",
            "CMP",
        )
        .await;

        record_movement(
            &pool,
            &mv(&cid, "mp_df4", "2026-01-01", "10", Some("5.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();

        let bom = create_bom(
            &pool,
            &cid,
            BomInput {
                product_id: "pf_df4".into(),
                name: "BOM_DF4".into(),
                output_qty: "1".into(),
                lines: vec![BomLineInput {
                    component_product_id: "mp_df4".into(),
                    qty: "2".into(),
                    um: None,
                    line_no: 1,
                }],
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
                labour_cost: Some("100.00".into()),
                overhead_cost: Some("0.00".into()),
                overhead_fixed: Some("-50.00".into()), // NEGATIVE — must be rejected
                overhead_variable: Some("0.00".into()),
                normal_capacity_qty: Some("10".into()),
            },
        )
        .await;

        assert!(
            matches!(res, Err(AppError::Validation(_))),
            "negative overhead_fixed must be rejected with Validation error, got: {res:?}"
        );
    }

    // ── DF-5: negative overhead_variable → Validation error ──────────────────

    #[tokio::test]
    async fn negative_overhead_variable_rejected() {
        let (pool, cid) = setup().await;
        let g = make_gestiune(&pool, &cid, "GDF5").await;

        make_product_sql(
            &pool,
            "pf_df5",
            &cid,
            "PF_DF5",
            "produs_finit",
            "345",
            "CMP",
        )
        .await;
        make_product_sql(
            &pool,
            "mp_df5",
            &cid,
            "MP_DF5",
            "materie_prima",
            "301",
            "CMP",
        )
        .await;

        record_movement(
            &pool,
            &mv(&cid, "mp_df5", "2026-01-01", "10", Some("5.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();

        let bom = create_bom(
            &pool,
            &cid,
            BomInput {
                product_id: "pf_df5".into(),
                name: "BOM_DF5".into(),
                output_qty: "1".into(),
                lines: vec![BomLineInput {
                    component_product_id: "mp_df5".into(),
                    qty: "2".into(),
                    um: None,
                    line_no: 1,
                }],
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
                labour_cost: None,
                overhead_cost: Some("0.00".into()),
                overhead_fixed: Some("100.00".into()),
                overhead_variable: Some("-10.00".into()), // NEGATIVE — must be rejected
                normal_capacity_qty: Some("10".into()),
            },
        )
        .await;

        assert!(
            matches!(res, Err(AppError::Validation(_))),
            "negative overhead_variable must be rejected with Validation error, got: {res:?}"
        );
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // ── LIFECYCLE TESTS (migration 0087) ──────────────────────────────────────
    // ═══════════════════════════════════════════════════════════════════════════

    // ── LC-1: create_planned_order → status='planned', no stock movement, no GL ─

    /// Un ordin planificat inserează un rând cu status='planned' și NU postează
    /// nicio mișcare de stoc sau notă GL.
    #[tokio::test]
    async fn lifecycle_planned_no_stock_no_gl() {
        let (pool, cid) = setup().await;
        let g = make_gestiune(&pool, &cid, "GLC1").await;

        make_product_sql(
            &pool,
            "pf_lc1",
            &cid,
            "PF_LC1",
            "produs_finit",
            "345",
            "CMP",
        )
        .await;
        make_product_sql(
            &pool,
            "mp_lc1",
            &cid,
            "MP_LC1",
            "materie_prima",
            "301",
            "CMP",
        )
        .await;

        // Stoc inițial: 50 @ 4.00
        record_movement(
            &pool,
            &mv(&cid, "mp_lc1", "2026-01-01", "50", Some("4.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();

        let bom = create_bom(
            &pool,
            &cid,
            BomInput {
                product_id: "pf_lc1".into(),
                name: "BOM_LC1".into(),
                output_qty: "1".into(),
                lines: vec![BomLineInput {
                    component_product_id: "mp_lc1".into(),
                    qty: "5".into(),
                    um: None,
                    line_no: 1,
                }],
            },
        )
        .await
        .unwrap();

        let (order, estimate) = create_planned_order(
            &pool,
            &cid,
            CreatePlannedOrderInput {
                bom_id: bom.bom.id.clone(),
                gestiune_id: g.clone(),
                qty_produced: "3".into(),
                planned_date: "2026-02-01".into(),
                production_date: None,
                notes: Some("Test plan".into()),
                labour_cost: Some("100.00".into()),
                overhead_cost: None,
                overhead_fixed: None,
                overhead_variable: None,
                normal_capacity_qty: None,
            },
        )
        .await
        .unwrap();

        // (1) Status = planned
        assert_eq!(order.status, "planned", "planned order has status=planned");

        // (2) Stocul componentei NEATINS
        assert_eq!(
            onhand(&pool, &cid, "mp_lc1", &g).await,
            Decimal::from(50),
            "component stock unchanged after planned order"
        );
        assert_eq!(
            onhand(&pool, &cid, "pf_lc1", &g).await,
            Decimal::ZERO,
            "finished good stock unchanged after planned order"
        );

        // (3) Nicio mișcare PRODUCTION în stock_ledger
        let prod_cnt: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM stock_ledger WHERE company_id=?1 AND doc_type='PRODUCTION'",
        )
        .bind(&cid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            prod_cnt, 0,
            "no PRODUCTION stock movements for planned order"
        );

        // (4) Nicio notă GL din mișcări PRODUCTION (GL-ul de la recepție inițială nu contează)
        let gl_cnt: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_journal j WHERE j.company_id=?1 AND j.source_type='STOCK' \
             AND j.source_id IN (SELECT id FROM stock_ledger WHERE company_id=?1 AND doc_type='PRODUCTION')",
        )
        .bind(&cid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            gl_cnt, 0,
            "no GL entries from PRODUCTION movements for planned order"
        );

        // (5) Estimarea costului este calculată (nu zero)
        let est_mat = dec(&estimate.estimated_material_cost);
        // 3 units × 5 component/unit × 4.00 avg_cost = 60.00
        assert_eq!(
            est_mat,
            Decimal::from(60),
            "estimated material cost = 3*5*4 = 60"
        );
        let est_full = dec(&estimate.estimated_full_cost);
        // 60 + 100 labour + 0 overhead = 160
        assert_eq!(
            est_full,
            Decimal::from(160),
            "estimated full cost = 60 + 100 labour = 160"
        );

        // (6) planned_date stocat corect
        assert_eq!(
            order.planned_date.as_deref(),
            Some("2026-02-01"),
            "planned_date stored"
        );
    }

    // ── LC-2: execute_order → planned → finalized, stock consumed, GL posted ──

    /// execute_order tranzitează un ordin planificat la 'finalized', consumă
    /// componentele și postează GL (D601=C301 + D345=C711).
    #[tokio::test]
    async fn lifecycle_execute_order_consumes_stock_posts_gl() {
        let (pool, cid) = setup().await;
        let g = make_gestiune(&pool, &cid, "GLC2").await;

        make_product_sql(
            &pool,
            "pf_lc2",
            &cid,
            "PF_LC2",
            "produs_finit",
            "345",
            "CMP",
        )
        .await;
        make_product_sql(
            &pool,
            "mp_lc2",
            &cid,
            "MP_LC2",
            "materie_prima",
            "301",
            "CMP",
        )
        .await;

        record_movement(
            &pool,
            &mv(&cid, "mp_lc2", "2026-01-01", "20", Some("5.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();

        let bom = create_bom(
            &pool,
            &cid,
            BomInput {
                product_id: "pf_lc2".into(),
                name: "BOM_LC2".into(),
                output_qty: "1".into(),
                lines: vec![BomLineInput {
                    component_product_id: "mp_lc2".into(),
                    qty: "4".into(),
                    um: None,
                    line_no: 1,
                }],
            },
        )
        .await
        .unwrap();

        let (planned, _) = create_planned_order(
            &pool,
            &cid,
            CreatePlannedOrderInput {
                bom_id: bom.bom.id.clone(),
                gestiune_id: g.clone(),
                qty_produced: "2".into(),
                planned_date: "2026-02-10".into(),
                production_date: Some("2026-02-10".into()),
                notes: None,
                labour_cost: None,
                overhead_cost: None,
                overhead_fixed: None,
                overhead_variable: None,
                normal_capacity_qty: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(planned.status, "planned");

        // Executăm
        let finalized = execute_order(&pool, &cid, &planned.id).await.unwrap();

        // (1) Status = finalized
        assert_eq!(finalized.status, "finalized", "status becomes finalized");

        // (2) Stocul componentei scade (2 unități × 4 = 8)
        assert_eq!(
            onhand(&pool, &cid, "mp_lc2", &g).await,
            Decimal::from(12),
            "component: 20 - 8 = 12"
        );
        // Stocul produs finit crește
        assert_eq!(
            onhand(&pool, &cid, "pf_lc2", &g).await,
            Decimal::from(2),
            "finished good: 0 + 2 = 2"
        );

        // (3) Costuri reale stocate
        // mat: 2 × 4 × 5.00 = 40.00
        assert_eq!(finalized.total_material_cost, "40.00", "material cost = 40");
        assert_eq!(
            finalized.full_cost, "40.00",
            "full cost = 40 (no labour/overhead)"
        );

        // (4) GL postat: D345=C711=40
        let (d345, _) = gl_turnover(&pool, &cid, "345").await;
        let (_, c711) = gl_turnover(&pool, &cid, "711").await;
        assert_eq!(d345, Decimal::from(40), "D345 = 40 after execute");
        assert_eq!(c711, Decimal::from(40), "C711 = 40 after execute");
    }

    // ── LC-3: re-execute rejected (idempotent guard, no double-consume) ────────

    /// Un ordin deja 'finalized' nu poate fi re-executat.
    /// Stocul rămâne neatins după tentativa de re-executare.
    #[tokio::test]
    async fn lifecycle_re_execute_rejected() {
        let (pool, cid) = setup().await;
        let g = make_gestiune(&pool, &cid, "GLC3").await;

        make_product_sql(
            &pool,
            "pf_lc3",
            &cid,
            "PF_LC3",
            "produs_finit",
            "345",
            "CMP",
        )
        .await;
        make_product_sql(
            &pool,
            "mp_lc3",
            &cid,
            "MP_LC3",
            "materie_prima",
            "301",
            "CMP",
        )
        .await;

        record_movement(
            &pool,
            &mv(&cid, "mp_lc3", "2026-01-01", "20", Some("3.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();

        let bom = create_bom(
            &pool,
            &cid,
            BomInput {
                product_id: "pf_lc3".into(),
                name: "BOM_LC3".into(),
                output_qty: "1".into(),
                lines: vec![BomLineInput {
                    component_product_id: "mp_lc3".into(),
                    qty: "2".into(),
                    um: None,
                    line_no: 1,
                }],
            },
        )
        .await
        .unwrap();

        let (planned, _) = create_planned_order(
            &pool,
            &cid,
            CreatePlannedOrderInput {
                bom_id: bom.bom.id.clone(),
                gestiune_id: g.clone(),
                qty_produced: "1".into(),
                planned_date: "2026-02-01".into(),
                production_date: Some("2026-02-01".into()),
                notes: None,
                labour_cost: None,
                overhead_cost: None,
                overhead_fixed: None,
                overhead_variable: None,
                normal_capacity_qty: None,
            },
        )
        .await
        .unwrap();

        // Prima execuție — OK
        execute_order(&pool, &cid, &planned.id).await.unwrap();
        let after_first = onhand(&pool, &cid, "mp_lc3", &g).await;
        assert_eq!(
            after_first,
            Decimal::from(18),
            "20 - 2 = 18 after first execute"
        );

        // A doua execuție — trebuie respinsă
        let re_exec = execute_order(&pool, &cid, &planned.id).await;
        assert!(
            matches!(re_exec, Err(AppError::Validation(_))),
            "re-execute must be rejected with Validation error"
        );

        // Stocul rămâne neatins (18, nu 16)
        assert_eq!(
            onhand(&pool, &cid, "mp_lc3", &g).await,
            Decimal::from(18),
            "stock unchanged after re-execute rejection (no double-consume)"
        );
    }

    // ── LC-4: cancel_order (planned → cancelled), finalized cannot be cancelled ─

    #[tokio::test]
    async fn lifecycle_cancel_order() {
        let (pool, cid) = setup().await;
        let g = make_gestiune(&pool, &cid, "GLC4").await;

        make_product_sql(
            &pool,
            "pf_lc4",
            &cid,
            "PF_LC4",
            "produs_finit",
            "345",
            "CMP",
        )
        .await;
        make_product_sql(
            &pool,
            "mp_lc4",
            &cid,
            "MP_LC4",
            "materie_prima",
            "301",
            "CMP",
        )
        .await;

        record_movement(
            &pool,
            &mv(&cid, "mp_lc4", "2026-01-01", "20", Some("2.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();

        let bom = create_bom(
            &pool,
            &cid,
            BomInput {
                product_id: "pf_lc4".into(),
                name: "BOM_LC4".into(),
                output_qty: "1".into(),
                lines: vec![BomLineInput {
                    component_product_id: "mp_lc4".into(),
                    qty: "3".into(),
                    um: None,
                    line_no: 1,
                }],
            },
        )
        .await
        .unwrap();

        let (planned, _) = create_planned_order(
            &pool,
            &cid,
            CreatePlannedOrderInput {
                bom_id: bom.bom.id.clone(),
                gestiune_id: g.clone(),
                qty_produced: "1".into(),
                planned_date: "2026-03-01".into(),
                production_date: None,
                notes: None,
                labour_cost: None,
                overhead_cost: None,
                overhead_fixed: None,
                overhead_variable: None,
                normal_capacity_qty: None,
            },
        )
        .await
        .unwrap();

        // Anulare ordin planificat → OK
        let cancelled = cancel_order(&pool, &cid, &planned.id).await.unwrap();
        assert_eq!(
            cancelled.status, "cancelled",
            "cancel sets status=cancelled"
        );

        // Stocul rămâne neatins
        assert_eq!(
            onhand(&pool, &cid, "mp_lc4", &g).await,
            Decimal::from(20),
            "stock unchanged after cancel"
        );

        // Niciun GL din mișcări PRODUCTION (GL-ul de la recepție inițială e așteptat)
        let gl_cnt: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_journal j WHERE j.company_id=?1 AND j.source_type='STOCK' \
             AND j.source_id IN (SELECT id FROM stock_ledger WHERE company_id=?1 AND doc_type='PRODUCTION')",
        )
        .bind(&cid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(gl_cnt, 0, "no GL after cancel");

        // Nu poți anula din nou
        let re_cancel = cancel_order(&pool, &cid, &planned.id).await;
        assert!(
            matches!(re_cancel, Err(AppError::Validation(_))),
            "re-cancelling cancelled order must fail"
        );

        // Un ordin finalizat NU poate fi anulat prin cancel_order
        let (planned2, _) = create_planned_order(
            &pool,
            &cid,
            CreatePlannedOrderInput {
                bom_id: bom.bom.id.clone(),
                gestiune_id: g.clone(),
                qty_produced: "1".into(),
                planned_date: "2026-03-05".into(),
                production_date: Some("2026-03-05".into()),
                notes: None,
                labour_cost: None,
                overhead_cost: None,
                overhead_fixed: None,
                overhead_variable: None,
                normal_capacity_qty: None,
            },
        )
        .await
        .unwrap();
        execute_order(&pool, &cid, &planned2.id).await.unwrap();

        let cancel_finalized = cancel_order(&pool, &cid, &planned2.id).await;
        assert!(
            matches!(cancel_finalized, Err(AppError::Validation(_))),
            "cancelling a finalized order must be rejected"
        );
    }

    // ── LC-5: direct produce() still creates finalized order (backward compat) ─

    /// Calea directă produce() creează un ordin 'finalized' cu GL postat — nicio schimbare.
    #[tokio::test]
    async fn lifecycle_direct_produce_backward_compat() {
        let (pool, cid) = setup().await;
        let g = make_gestiune(&pool, &cid, "GLC5").await;

        make_product_sql(
            &pool,
            "pf_lc5",
            &cid,
            "PF_LC5",
            "produs_finit",
            "345",
            "CMP",
        )
        .await;
        make_product_sql(
            &pool,
            "mp_lc5",
            &cid,
            "MP_LC5",
            "materie_prima",
            "301",
            "CMP",
        )
        .await;

        record_movement(
            &pool,
            &mv(&cid, "mp_lc5", "2026-01-01", "10", Some("6.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();

        let bom = create_bom(
            &pool,
            &cid,
            BomInput {
                product_id: "pf_lc5".into(),
                name: "BOM_LC5".into(),
                output_qty: "1".into(),
                lines: vec![BomLineInput {
                    component_product_id: "mp_lc5".into(),
                    qty: "2".into(),
                    um: None,
                    line_no: 1,
                }],
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
                qty_produced: "3".into(),
                production_date: "2026-01-10".into(),
                notes: None,
                labour_cost: None,
                overhead_cost: None,
                overhead_fixed: None,
                overhead_variable: None,
                normal_capacity_qty: None,
            },
        )
        .await
        .unwrap();

        // (1) Status = finalized
        assert_eq!(
            order.status, "finalized",
            "direct produce() still produces finalized order"
        );

        // (2) GL postat (D345=C711=36)
        let (d345, _) = gl_turnover(&pool, &cid, "345").await;
        let (_, c711) = gl_turnover(&pool, &cid, "711").await;
        assert_eq!(d345, Decimal::from(36), "D345 = 3*2*6 = 36");
        assert_eq!(c711, Decimal::from(36), "C711 = 36");

        // (3) Stocuri corecte: 10 - 6 = 4 componente, 3 produs finit
        assert_eq!(onhand(&pool, &cid, "mp_lc5", &g).await, Decimal::from(4));
        assert_eq!(onhand(&pool, &cid, "pf_lc5", &g).await, Decimal::from(3));
    }

    // ── LC-6: execute with insufficient stock → rejected, order stays planned ──

    #[tokio::test]
    async fn lifecycle_execute_insufficient_stock_stays_planned() {
        let (pool, cid) = setup().await;
        let g = make_gestiune(&pool, &cid, "GLC6").await;

        make_product_sql(
            &pool,
            "pf_lc6",
            &cid,
            "PF_LC6",
            "produs_finit",
            "345",
            "CMP",
        )
        .await;
        make_product_sql(
            &pool,
            "mp_lc6",
            &cid,
            "MP_LC6",
            "materie_prima",
            "301",
            "CMP",
        )
        .await;

        // Only 2 units in stock; need 6 (3 produced × 2 per BOM)
        record_movement(
            &pool,
            &mv(&cid, "mp_lc6", "2026-01-01", "2", Some("5.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();

        let bom = create_bom(
            &pool,
            &cid,
            BomInput {
                product_id: "pf_lc6".into(),
                name: "BOM_LC6".into(),
                output_qty: "1".into(),
                lines: vec![BomLineInput {
                    component_product_id: "mp_lc6".into(),
                    qty: "2".into(),
                    um: None,
                    line_no: 1,
                }],
            },
        )
        .await
        .unwrap();

        let (planned, _) = create_planned_order(
            &pool,
            &cid,
            CreatePlannedOrderInput {
                bom_id: bom.bom.id.clone(),
                gestiune_id: g.clone(),
                qty_produced: "3".into(), // needs 6, only 2 available
                planned_date: "2026-02-01".into(),
                production_date: Some("2026-02-01".into()),
                notes: None,
                labour_cost: None,
                overhead_cost: None,
                overhead_fixed: None,
                overhead_variable: None,
                normal_capacity_qty: None,
            },
        )
        .await
        .unwrap();

        let res = execute_order(&pool, &cid, &planned.id).await;
        assert!(
            matches!(res, Err(AppError::Validation(_))),
            "execute with insufficient stock must fail"
        );

        // Order still planned
        let still_planned = get_productie(&pool, &cid, &planned.id).await.unwrap();
        assert_eq!(
            still_planned.status, "planned",
            "order stays planned when execution fails due to insufficient stock"
        );

        // Stock untouched
        assert_eq!(
            onhand(&pool, &cid, "mp_lc6", &g).await,
            Decimal::from(2),
            "stock unchanged after failed execute"
        );

        // No GL from PRODUCTION movements (the initial receipt GL is expected)
        let gl_cnt: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM gl_journal j WHERE j.company_id=?1 AND j.source_type='STOCK' \
             AND j.source_id IN (SELECT id FROM stock_ledger WHERE company_id=?1 AND doc_type='PRODUCTION')",
        )
        .bind(&cid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(gl_cnt, 0, "no GL after failed execute");
    }

    // ── LC-7: list_productie_by_status returns only matching orders ───────────

    #[tokio::test]
    async fn lifecycle_list_by_status() {
        let (pool, cid) = setup().await;
        let g = make_gestiune(&pool, &cid, "GLC7").await;

        make_product_sql(
            &pool,
            "pf_lc7",
            &cid,
            "PF_LC7",
            "produs_finit",
            "345",
            "CMP",
        )
        .await;
        make_product_sql(
            &pool,
            "mp_lc7",
            &cid,
            "MP_LC7",
            "materie_prima",
            "301",
            "CMP",
        )
        .await;

        record_movement(
            &pool,
            &mv(&cid, "mp_lc7", "2026-01-01", "50", Some("1.00"), &g),
            Dir::In,
        )
        .await
        .unwrap();

        let bom = create_bom(
            &pool,
            &cid,
            BomInput {
                product_id: "pf_lc7".into(),
                name: "BOM_LC7".into(),
                output_qty: "1".into(),
                lines: vec![BomLineInput {
                    component_product_id: "mp_lc7".into(),
                    qty: "1".into(),
                    um: None,
                    line_no: 1,
                }],
            },
        )
        .await
        .unwrap();

        // Create 2 planned orders
        let (p1, _) = create_planned_order(
            &pool,
            &cid,
            CreatePlannedOrderInput {
                bom_id: bom.bom.id.clone(),
                gestiune_id: g.clone(),
                qty_produced: "1".into(),
                planned_date: "2026-02-01".into(),
                production_date: Some("2026-02-01".into()),
                notes: None,
                labour_cost: None,
                overhead_cost: None,
                overhead_fixed: None,
                overhead_variable: None,
                normal_capacity_qty: None,
            },
        )
        .await
        .unwrap();

        let (p2, _) = create_planned_order(
            &pool,
            &cid,
            CreatePlannedOrderInput {
                bom_id: bom.bom.id.clone(),
                gestiune_id: g.clone(),
                qty_produced: "2".into(),
                planned_date: "2026-02-15".into(),
                production_date: Some("2026-02-15".into()),
                notes: None,
                labour_cost: None,
                overhead_cost: None,
                overhead_fixed: None,
                overhead_variable: None,
                normal_capacity_qty: None,
            },
        )
        .await
        .unwrap();

        // Execute p1
        execute_order(&pool, &cid, &p1.id).await.unwrap();

        let planned_list = list_productie_by_status(&pool, &cid, "planned")
            .await
            .unwrap();
        let finalized_list = list_productie_by_status(&pool, &cid, "finalized")
            .await
            .unwrap();

        assert_eq!(planned_list.len(), 1, "1 planned order remaining");
        assert_eq!(planned_list[0].id, p2.id, "p2 still planned");
        assert_eq!(finalized_list.len(), 1, "1 finalized order");
        assert_eq!(finalized_list[0].id, p1.id, "p1 is finalized");
    }
}
