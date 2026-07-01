//! Dezmembrare stocuri — OMFP 1802/2014.
//!
//! Ia 1 produs dezasamblat OUT din stoc → introduce N componente recuperate IN.
//!
//! ## Monografie (OMFP 1802/2014 pct. 8)
//!
//! Ieșire produs dezasamblat la valoare contabilă (carrying cost):
//!   D 607 «Cheltuieli privind mărfurile»   = carrying_cost
//!   C 3xx (stock_account al produsului)    = carrying_cost
//!
//! Intrare componente recuperate la valoare justă (fair value):
//!   D 3xx (stock_account al componentei)   = fair_value_fiecare_comp
//!   C 7588 «Alte venituri din exploatare»  = Σfair_values
//!
//! Diferența Σfair_values − carrying_cost = efect net P&L (7588 venit vs 607 cheltuială).
//! Nota contabilă este echilibrată: Σdebit = Σcredit.
//!
//! Sursă GL: source_type='DEZMEMBRARE', journal_id='STOCURI', journal_type='STOCK_DISMANTLING'.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

use crate::db::gestiune;
use crate::db::gl::{post_manual_journal, ManualJournal};
use crate::db::invoices::round2;
use crate::db::models::{new_id, now_unix};
use crate::db::period_locks;
use crate::db::stock_valuation::{self, assert_product_owned, Dir, StockMovementInput};
use crate::error::{AppError, AppResult};

// ─── Models ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Dezmembrare {
    pub id: String,
    pub company_id: String,
    pub gestiune_id: String,
    pub dismantled_product_id: String,
    pub dismantled_qty: String,           // 6dp
    pub dismantled_carrying_cost: String, // 2dp
    pub dezmembrare_date: String,
    pub status: String, // 'DRAFT' | 'POSTED'
    pub notes: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct DezMembraraLine {
    pub id: String,
    pub dezmembrare_id: String,
    pub position: i64,
    pub product_id: String,
    pub qty: String,              // 6dp
    pub unit_fair_value: String,  // 2dp
    pub total_fair_value: String, // 2dp
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DezmembrareWithLines {
    pub dezmembrare: Dezmembrare,
    pub lines: Vec<DezMembraraLine>,
}

// ─── Inputs ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DezmembrareLineInput {
    pub product_id: String,
    pub qty: f64,
    pub unit_fair_value: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDezmembrareInput {
    pub company_id: String,
    pub gestiune_id: Option<String>,
    pub dismantled_product_id: String,
    pub dismantled_qty: f64,
    pub dezmembrare_date: String,
    pub notes: Option<String>,
    pub lines: Vec<DezmembrareLineInput>, // componente recuperate
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s.trim()).unwrap_or(Decimal::ZERO)
}

/// Cantitatea on-hand (run_qty ultimei mișcări cronologice) pentru un produs în gestiune.
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

// ─── CRUD ─────────────────────────────────────────────────────────────────────

/// Creează un bon de dezmembrare în stare DRAFT.
///
/// Validări:
/// - qty > 0
/// - cel puțin o linie componentă
/// - fiecare fair_value >= 0
/// - produsul dezasamblat aparține companiei (guard multi-tenant)
/// - fiecare componentă aparține companiei (guard multi-tenant)
/// - stoc disponibil >= cantitate dezasamblată
pub async fn create_dezmembrare(
    pool: &SqlitePool,
    input: CreateDezmembrareInput,
) -> AppResult<DezmembrareWithLines> {
    // ── Validare de bază ──────────────────────────────────────────────────────

    let dismantled_qty = Decimal::from_f64_retain(input.dismantled_qty).ok_or_else(|| {
        AppError::Validation("Cantitate invalidă pentru produsul dezasamblat.".into())
    })?;
    if dismantled_qty <= Decimal::ZERO {
        return Err(AppError::Validation(
            "Cantitatea dezasamblată trebuie să fie > 0.".into(),
        ));
    }

    if input.lines.is_empty() {
        return Err(AppError::Validation(
            "Dezmembrarea trebuie să conțină cel puțin o componentă recuperată.".into(),
        ));
    }

    for (i, line) in input.lines.iter().enumerate() {
        if line.unit_fair_value < 0.0 {
            return Err(AppError::Validation(format!(
                "Valoarea justă a componentei {} nu poate fi negativă.",
                i + 1
            )));
        }
        let qty = Decimal::from_f64_retain(line.qty).ok_or_else(|| {
            AppError::Validation(format!("Cantitate invalidă la linia {}.", i + 1))
        })?;
        if qty <= Decimal::ZERO {
            return Err(AppError::Validation(format!(
                "Cantitatea componentei {} trebuie să fie > 0.",
                i + 1
            )));
        }
    }

    // ── Guard multi-tenant: produsul dezasamblat ──────────────────────────────
    assert_product_owned(pool, &input.company_id, &input.dismantled_product_id).await?;

    // ── Guard multi-tenant: fiecare componentă ────────────────────────────────
    for line in &input.lines {
        assert_product_owned(pool, &input.company_id, &line.product_id).await?;
    }

    // ── Rezolvare gestiune_id ─────────────────────────────────────────────────
    let gestiune_id = match input.gestiune_id.as_deref().filter(|s| !s.is_empty()) {
        Some(gid) => gid.to_string(),
        None => gestiune::default_gestiune_id(pool, &input.company_id).await?,
    };

    // ── Verificare stoc disponibil ────────────────────────────────────────────
    let available = on_hand_qty(
        pool,
        &input.company_id,
        &input.dismantled_product_id,
        &gestiune_id,
    )
    .await?;
    if available < dismantled_qty {
        return Err(AppError::Validation(format!(
            "Stoc insuficient: disponibil {:.6}, necesar {:.6}.",
            available, dismantled_qty
        )));
    }

    // ── Period lock check ─────────────────────────────────────────────────────
    let period = &input.dezmembrare_date[..7]; // "YYYY-MM"
    if period_locks::is_period_locked(pool, &input.company_id, period).await? {
        return Err(AppError::Validation(format!(
            "Perioada {} este blocată. Nu se pot crea dezmembrări în această perioadă.",
            period
        )));
    }

    // ── Inserare cap ──────────────────────────────────────────────────────────
    let dez_id = new_id();
    let now = now_unix();

    sqlx::query(
        "INSERT INTO dezmembrari \
         (id, company_id, gestiune_id, dismantled_product_id, dismantled_qty, \
          dismantled_carrying_cost, dezmembrare_date, status, notes, created_at, updated_at) \
         VALUES (?1,?2,?3,?4,?5,'0.00',?6,'DRAFT',?7,?8,?9)",
    )
    .bind(&dez_id)
    .bind(&input.company_id)
    .bind(&gestiune_id)
    .bind(&input.dismantled_product_id)
    .bind(format!("{:.6}", dismantled_qty))
    .bind(&input.dezmembrare_date)
    .bind(&input.notes)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    // ── Inserare linii ────────────────────────────────────────────────────────
    let mut lines_out: Vec<DezMembraraLine> = Vec::with_capacity(input.lines.len());
    for (pos, line) in input.lines.iter().enumerate() {
        let lid = new_id();
        let qty = Decimal::from_f64_retain(line.qty).unwrap_or(Decimal::ZERO);
        let unit_fv = Decimal::from_f64_retain(line.unit_fair_value).unwrap_or(Decimal::ZERO);
        let total_fv = round2(qty * unit_fv);

        sqlx::query(
            "INSERT INTO dezmembrare_lines \
             (id, dezmembrare_id, position, product_id, qty, unit_fair_value, total_fair_value) \
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
        )
        .bind(&lid)
        .bind(&dez_id)
        .bind((pos + 1) as i64)
        .bind(&line.product_id)
        .bind(format!("{:.6}", qty))
        .bind(format!("{:.2}", unit_fv))
        .bind(format!("{:.2}", total_fv))
        .execute(pool)
        .await?;

        lines_out.push(DezMembraraLine {
            id: lid,
            dezmembrare_id: dez_id.clone(),
            position: (pos + 1) as i64,
            product_id: line.product_id.clone(),
            qty: format!("{:.6}", qty),
            unit_fair_value: format!("{:.2}", unit_fv),
            total_fair_value: format!("{:.2}", total_fv),
        });
    }

    let dezmembrare = sqlx::query_as::<_, Dezmembrare>(
        "SELECT id, company_id, gestiune_id, dismantled_product_id, dismantled_qty, \
         dismantled_carrying_cost, dezmembrare_date, status, notes, created_at, updated_at \
         FROM dezmembrari WHERE id=?1 AND company_id=?2",
    )
    .bind(&dez_id)
    .bind(&input.company_id)
    .fetch_one(pool)
    .await?;

    Ok(DezmembrareWithLines {
        dezmembrare,
        lines: lines_out,
    })
}

/// Postează o dezmembrare DRAFT:
///   - Înregistrează mișcarea de stoc OUT pentru produsul dezasamblat
///   - Preia carrying cost din stock_ledger (calculat de motor FIFO/LIFO/CMP)
///   - Înregistrează mișcări de stoc IN pentru fiecare componentă recuperată
///   - Postează nota GL (607 + 371_comp / stock_acct_dismantled + 7588)
///   - Actualizează status='POSTED' și dismantled_carrying_cost
pub async fn post_dezmembrare(
    pool: &SqlitePool,
    company_id: &str,
    dezmembrare_id: &str,
) -> AppResult<Dezmembrare> {
    // ── Fetch head ────────────────────────────────────────────────────────────
    let dez: Option<Dezmembrare> = sqlx::query_as::<_, Dezmembrare>(
        "SELECT id, company_id, gestiune_id, dismantled_product_id, dismantled_qty, \
         dismantled_carrying_cost, dezmembrare_date, status, notes, created_at, updated_at \
         FROM dezmembrari WHERE id=?1 AND company_id=?2",
    )
    .bind(dezmembrare_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?;
    let dez = dez.ok_or(AppError::NotFound)?;

    // ── Guard: trebuie DRAFT ──────────────────────────────────────────────────
    if dez.status != "DRAFT" {
        return Err(AppError::Validation(format!(
            "Dezmembrarea este deja '{}'. Doar bonurile DRAFT pot fi postate.",
            dez.status
        )));
    }

    // ── Period lock check ─────────────────────────────────────────────────────
    let period = &dez.dezmembrare_date[..7];
    if period_locks::is_period_locked(pool, company_id, period).await? {
        return Err(AppError::Validation(format!(
            "Perioada {} este blocată. Nu se poate posta dezmembrarea.",
            period
        )));
    }

    // ── Fetch linii ───────────────────────────────────────────────────────────
    let lines: Vec<DezMembraraLine> = sqlx::query_as::<_, DezMembraraLine>(
        "SELECT id, dezmembrare_id, position, product_id, qty, unit_fair_value, total_fair_value \
         FROM dezmembrare_lines WHERE dezmembrare_id=?1 ORDER BY position",
    )
    .bind(dezmembrare_id)
    .fetch_all(pool)
    .await?;

    if lines.is_empty() {
        return Err(AppError::Validation(
            "Dezmembrarea nu are linii de componente. Nu se poate posta.".into(),
        ));
    }

    let dismantled_qty = dec(&dez.dismantled_qty);

    // ── Re-verificare stoc disponibil (double-check înainte de postare) ────────
    let available = on_hand_qty(
        pool,
        company_id,
        &dez.dismantled_product_id,
        &dez.gestiune_id,
    )
    .await?;
    if available < dismantled_qty {
        return Err(AppError::Validation(format!(
            "Stoc insuficient la postare: disponibil {:.6}, necesar {:.6}.",
            available, dismantled_qty
        )));
    }

    // ── OUT: produs dezasamblat ────────────────────────────────────────────────
    let out_input = StockMovementInput {
        company_id: company_id.to_string(),
        product_id: dez.dismantled_product_id.clone(),
        entry_date: dez.dezmembrare_date.clone(),
        qty: format!("{:.6}", dismantled_qty),
        unit_cost: None, // OUT: costul e atribuit de motorul FIFO/LIFO/CMP
        doc_type: Some("DEZMEMBRARE".to_string()),
        doc_ref: Some(dezmembrare_id.to_string()),
        gestiune_id: Some(dez.gestiune_id.clone()),
    };
    stock_valuation::record_movement(pool, &out_input, Dir::Out).await?;

    // ── Preia carrying cost calculat de motor ─────────────────────────────────
    // Valoarea OUT (carrying cost) este scrisă de recompute_product_gestiune în stock_ledger.value
    let carrying_row: Option<(String,)> = sqlx::query_as(
        "SELECT value FROM stock_ledger \
         WHERE company_id=?1 AND doc_ref=?2 AND direction='OUT' \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(company_id)
    .bind(dezmembrare_id)
    .fetch_optional(pool)
    .await?;
    let carrying_cost = carrying_row.map(|(v,)| dec(&v)).unwrap_or(Decimal::ZERO);
    let carrying_cost = round2(carrying_cost);

    // ── IN: componente recuperate ─────────────────────────────────────────────
    for line in &lines {
        let comp_qty = dec(&line.qty);
        let unit_fv = dec(&line.unit_fair_value);
        let in_input = StockMovementInput {
            company_id: company_id.to_string(),
            product_id: line.product_id.clone(),
            entry_date: dez.dezmembrare_date.clone(),
            qty: format!("{:.6}", comp_qty),
            unit_cost: Some(format!("{:.2}", unit_fv)),
            doc_type: Some("DEZMEMBRARE".to_string()),
            doc_ref: Some(dezmembrare_id.to_string()),
            gestiune_id: Some(dez.gestiune_id.clone()),
        };
        stock_valuation::record_movement(pool, &in_input, Dir::In).await?;
    }

    // ── Calcul Σfair_values ───────────────────────────────────────────────────
    let total_fair_value_sum: Decimal = lines
        .iter()
        .map(|l| dec(&l.total_fair_value))
        .fold(Decimal::ZERO, |acc, v| acc + v);
    let total_fair_value_sum = round2(total_fair_value_sum);

    // ── Stock account produs dezasamblat ──────────────────────────────────────
    let dismantled_stock_account: Option<String> = sqlx::query_scalar(
        "SELECT COALESCE(stock_account,'371') FROM products WHERE id=?1 AND company_id=?2",
    )
    .bind(&dez.dismantled_product_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?;
    let dismantled_stock_account = dismantled_stock_account.unwrap_or_else(|| "371".to_string());

    // ── Construire linii GL ───────────────────────────────────────────────────
    //
    // Nota TREBUIE să fie echilibrată: Σdebit = Σcredit
    //   D 607                           = carrying_cost
    //   C {dismantled_stock_account}    = carrying_cost
    //   Per componentă:
    //   D {comp_stock_account}          = total_fair_value_comp
    //   C 7588                          = Σtotal_fair_values
    //
    // Verificare: Σdebit = 607(carrying) + Σ371_comps(fair_values)
    //             Σcredit = 371_dismantled(carrying) + 7588(Σfair_values)
    //             = same  ✓

    // Linie C: stoc produs dezasamblat OUT (credit)
    // Ne trebuie un `String` temporar cu account-ul; îl convertim mai jos
    let mut gl_lines_owned: Vec<(String, Decimal, Decimal)> = Vec::new();

    // D 607 — cheltuiala cu stocul ieșit
    gl_lines_owned.push(("607".to_string(), carrying_cost, Decimal::ZERO));
    // C {dismantled_stock_account} — ieșire stoc produs dezasamblat
    gl_lines_owned.push((dismantled_stock_account, Decimal::ZERO, carrying_cost));

    // D {comp_stock_account} per componentă + C 7588 global la final
    for line in &lines {
        // stock_account componentă (fallback '371')
        let comp_stock_account: Option<String> = sqlx::query_scalar(
            "SELECT COALESCE(stock_account,'371') FROM products WHERE id=?1 AND company_id=?2",
        )
        .bind(&line.product_id)
        .bind(company_id)
        .fetch_optional(pool)
        .await?;
        let comp_account = comp_stock_account.unwrap_or_else(|| "371".to_string());
        let tfv = dec(&line.total_fair_value);
        gl_lines_owned.push((comp_account, tfv, Decimal::ZERO));
    }
    // C 7588 — alte venituri din exploatare (intrare componente la fair value)
    gl_lines_owned.push(("7588".to_string(), Decimal::ZERO, total_fair_value_sum));

    // Convertim Vec<(String,…)> → Vec<(&str,…)> pentru post_manual_journal
    let gl_lines_refs: Vec<(&str, Decimal, Decimal)> = gl_lines_owned
        .iter()
        .map(|(acct, d, c)| (acct.as_str(), *d, *c))
        .collect();

    // ── Postare nota GL ───────────────────────────────────────────────────────
    let journal = ManualJournal {
        company_id,
        journal_id: "STOCURI",
        journal_type: "STOCK_DISMANTLING",
        source_type: "DEZMEMBRARE",
        source_id: dezmembrare_id,
        date: &dez.dezmembrare_date,
        description: &format!("Dezmembrare stoc {}", dezmembrare_id),
        partner_cui: None,
    };
    post_manual_journal(pool, &journal, &gl_lines_refs).await?;

    // ── Actualizare status + carrying_cost ────────────────────────────────────
    let now = now_unix();
    sqlx::query(
        "UPDATE dezmembrari \
         SET status='POSTED', dismantled_carrying_cost=?3, updated_at=?4 \
         WHERE id=?1 AND company_id=?2",
    )
    .bind(dezmembrare_id)
    .bind(company_id)
    .bind(format!("{:.2}", carrying_cost))
    .bind(now)
    .execute(pool)
    .await?;

    // ── Returnăm înregistrarea actualizată ────────────────────────────────────
    let updated = sqlx::query_as::<_, Dezmembrare>(
        "SELECT id, company_id, gestiune_id, dismantled_product_id, dismantled_qty, \
         dismantled_carrying_cost, dezmembrare_date, status, notes, created_at, updated_at \
         FROM dezmembrari WHERE id=?1 AND company_id=?2",
    )
    .bind(dezmembrare_id)
    .bind(company_id)
    .fetch_one(pool)
    .await?;

    Ok(updated)
}

/// Returnează capul unui bon de dezmembrare (guard multi-tenant).
pub async fn get_dezmembrare(
    pool: &SqlitePool,
    company_id: &str,
    dezmembrare_id: &str,
) -> AppResult<Dezmembrare> {
    let row: Option<Dezmembrare> = sqlx::query_as::<_, Dezmembrare>(
        "SELECT id, company_id, gestiune_id, dismantled_product_id, dismantled_qty, \
         dismantled_carrying_cost, dezmembrare_date, status, notes, created_at, updated_at \
         FROM dezmembrari WHERE id=?1 AND company_id=?2",
    )
    .bind(dezmembrare_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?;
    row.ok_or(AppError::NotFound)
}

/// Returnează un bon de dezmembrare cu liniile aferente (guard multi-tenant).
pub async fn get_dezmembrare_with_lines(
    pool: &SqlitePool,
    company_id: &str,
    dezmembrare_id: &str,
) -> AppResult<DezmembrareWithLines> {
    let dezmembrare = get_dezmembrare(pool, company_id, dezmembrare_id).await?;
    let lines: Vec<DezMembraraLine> = sqlx::query_as::<_, DezMembraraLine>(
        "SELECT id, dezmembrare_id, position, product_id, qty, unit_fair_value, total_fair_value \
         FROM dezmembrare_lines WHERE dezmembrare_id=?1 ORDER BY position",
    )
    .bind(dezmembrare_id)
    .fetch_all(pool)
    .await?;
    Ok(DezmembrareWithLines { dezmembrare, lines })
}

/// Listează toate bonurile de dezmembrare ale unei companii (cele mai recente primele).
pub async fn list_dezmembrari(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<Dezmembrare>> {
    Ok(sqlx::query_as::<_, Dezmembrare>(
        "SELECT id, company_id, gestiune_id, dismantled_product_id, dismantled_qty, \
         dismantled_carrying_cost, dezmembrare_date, status, notes, created_at, updated_at \
         FROM dezmembrari WHERE company_id=?1 ORDER BY dezmembrare_date DESC, created_at DESC",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::Row;

    /// Seed minimal: firmă + gestiune + produse cu stoc.
    ///
    /// Produse:
    ///   prod1 — «Produs Dezmembrat»  stock_account='371'  1 unitate @ 500.00
    ///   prod2 — «Component A»         stock_account='371'
    ///   prod3 — «Component B»         stock_account='371'
    async fn seed(pool: &SqlitePool) -> (String, String, String, String) {
        // Firmă
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co1','RO12345674','Test SRL','Str 1','Bucuresti','B','RO')",
        )
        .execute(pool)
        .await
        .unwrap();

        // Gestiune
        sqlx::query(
            "INSERT INTO gestiune (id, company_id, cod, denumire, is_default) \
             VALUES ('gest1','co1','PRINCIPALA','Gestiune principala',1)",
        )
        .execute(pool)
        .await
        .unwrap();

        // Produse
        for (id, name) in [
            ("prod1", "Produs Dezmembrat"),
            ("prod2", "Component A"),
            ("prod3", "Component B"),
        ] {
            sqlx::query(
                "INSERT INTO products (id, company_id, name, unit, stock_account, valuation_method) \
                 VALUES (?1,'co1',?2,'BUC','371','CMP')",
            )
            .bind(id)
            .bind(name)
            .execute(pool)
            .await
            .unwrap();
        }

        // Stoc inițial: prod1 — 1 unitate @ 500.00
        let mv = StockMovementInput {
            company_id: "co1".into(),
            product_id: "prod1".into(),
            entry_date: "2026-01-10".into(),
            qty: "1.000000".into(),
            unit_cost: Some("500.00".into()),
            doc_type: Some("NIR".into()),
            doc_ref: Some("nir-001".into()),
            gestiune_id: Some("gest1".into()),
        };
        stock_valuation::record_movement(pool, &mv, Dir::In)
            .await
            .unwrap();

        ("co1".into(), "gest1".into(), "prod1".into(), "prod2".into())
    }

    /// Test 1: nota GL este corectă (conturi, sume, echilibru).
    ///
    /// Dezmembrare: 1 × prod1 (carrying 500.00) → prod2 × 1 @ 300.00 + prod3 × 1 @ 250.00
    /// Nota așteptată:
    ///   D 607         = 500.00
    ///   C 371 (prod1) = 500.00
    ///   D 371 (prod2) = 300.00
    ///   D 371 (prod3) = 250.00
    ///   C 7588        = 550.00
    ///   Σdebit = 1050.00 = Σcredit ✓
    #[tokio::test]
    async fn dezmembrare_posts_correct_gl() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        seed(&pool).await;

        // Create
        let with_lines = create_dezmembrare(
            &pool,
            CreateDezmembrareInput {
                company_id: "co1".into(),
                gestiune_id: Some("gest1".into()),
                dismantled_product_id: "prod1".into(),
                dismantled_qty: 1.0,
                dezmembrare_date: "2026-02-15".into(),
                notes: None,
                lines: vec![
                    DezmembrareLineInput {
                        product_id: "prod2".into(),
                        qty: 1.0,
                        unit_fair_value: 300.0,
                    },
                    DezmembrareLineInput {
                        product_id: "prod3".into(),
                        qty: 1.0,
                        unit_fair_value: 250.0,
                    },
                ],
            },
        )
        .await
        .unwrap();

        assert_eq!(with_lines.dezmembrare.status, "DRAFT");

        // Post
        let posted = post_dezmembrare(&pool, "co1", &with_lines.dezmembrare.id)
            .await
            .unwrap();

        assert_eq!(posted.status, "POSTED");
        assert_eq!(posted.dismantled_carrying_cost, "500.00");

        // Verificare nota GL
        let rows = sqlx::query(
            "SELECT e.account_code, \
                    CAST(e.debit AS REAL) AS d, \
                    CAST(e.credit AS REAL) AS c \
             FROM gl_entry e \
             JOIN gl_journal j ON e.journal_pk = j.id \
             WHERE j.company_id='co1' AND j.source_type='DEZMEMBRARE' AND j.source_id=?1 \
             ORDER BY e.record_id",
        )
        .bind(&posted.id)
        .fetch_all(&pool)
        .await
        .unwrap();

        let mut sum_d = 0.0_f64;
        let mut sum_c = 0.0_f64;
        let mut found_607 = false;
        let mut found_7588 = false;

        for row in &rows {
            let acct: String = row.get("account_code");
            let d: f64 = row.get("d");
            let c: f64 = row.get("c");
            sum_d += d;
            sum_c += c;
            if acct == "607" {
                assert!((d - 500.0).abs() < 0.005, "607 debit trebuie să fie 500.00");
                found_607 = true;
            }
            if acct == "7588" {
                assert!(
                    (c - 550.0).abs() < 0.005,
                    "7588 credit trebuie să fie 550.00"
                );
                found_7588 = true;
            }
        }

        assert!(found_607, "linia D 607 trebuie să existe în GL");
        assert!(found_7588, "linia C 7588 trebuie să existe în GL");
        assert!(
            (sum_d - sum_c).abs() < 0.005,
            "nota GL trebuie să fie echilibrată; Σdebit={sum_d} Σcredit={sum_c}"
        );
        assert!(
            (sum_d - 1050.0).abs() < 0.005,
            "Σdebit trebuie să fie 1050.00 (500 + 300 + 250); actual={sum_d}"
        );

        // Verificare stoc prod1 scăzut cu 1
        let prod1_qty: String =
            sqlx::query_scalar("SELECT stock_qty FROM products WHERE id='prod1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(
            dec(&prod1_qty) <= Decimal::ZERO,
            "prod1 trebuie să fie epuizat din stoc"
        );

        // Verificare stoc prod2 crescut cu 1 @ 300.00
        let prod2_qty: String =
            sqlx::query_scalar("SELECT stock_qty FROM products WHERE id='prod2'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(
            (dec(&prod2_qty) - Decimal::ONE).abs() < dec("0.000001"),
            "prod2 trebuie să aibă qty=1"
        );
    }

    /// Test 2: nota GL este echilibrată (Σdebit == Σcredit) — verificare explicită.
    #[tokio::test]
    async fn dezmembrare_balanced_net_gain() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        seed(&pool).await;

        let with_lines = create_dezmembrare(
            &pool,
            CreateDezmembrareInput {
                company_id: "co1".into(),
                gestiune_id: Some("gest1".into()),
                dismantled_product_id: "prod1".into(),
                dismantled_qty: 1.0,
                dezmembrare_date: "2026-03-01".into(),
                notes: None,
                lines: vec![
                    DezmembrareLineInput {
                        product_id: "prod2".into(),
                        qty: 1.0,
                        unit_fair_value: 300.0,
                    },
                    DezmembrareLineInput {
                        product_id: "prod3".into(),
                        qty: 1.0,
                        unit_fair_value: 250.0,
                    },
                ],
            },
        )
        .await
        .unwrap();

        let posted = post_dezmembrare(&pool, "co1", &with_lines.dezmembrare.id)
            .await
            .unwrap();

        let row = sqlx::query(
            "SELECT COALESCE(SUM(CAST(e.debit AS REAL)),0) AS sd, \
                    COALESCE(SUM(CAST(e.credit AS REAL)),0) AS sc, \
                    COUNT(*) AS n \
             FROM gl_entry e \
             JOIN gl_journal j ON e.journal_pk = j.id \
             WHERE j.company_id='co1' AND j.source_type='DEZMEMBRARE' AND j.source_id=?1",
        )
        .bind(&posted.id)
        .fetch_one(&pool)
        .await
        .unwrap();

        let (sd, sc, n): (f64, f64, i64) = (row.get("sd"), row.get("sc"), row.get("n"));

        // 4 linii: D607, C371_out, D371_compA, D371_compB, C7588 → 5 linii
        assert_eq!(n, 5, "nota GL trebuie să aibă 5 linii");
        assert!(
            (sd - sc).abs() < 0.005,
            "nota GL trebuie să fie echilibrată; Σdebit={sd} Σcredit={sc}"
        );
        // Net gain: D607=500 vs C7588=550 → câștig net 50 RON
        // Σdebit = 607 + 371compA + 371compB = 500 + 300 + 250 = 1050
        assert!(
            (sd - 1050.0).abs() < 0.005,
            "Σdebit trebuie să fie 1050.00; actual={sd}"
        );
        assert!(
            (sc - 1050.0).abs() < 0.005,
            "Σcredit trebuie să fie 1050.00; actual={sc}"
        );
    }

    /// Test 3: dubla postare este respinsă cu eroare de validare.
    #[tokio::test]
    async fn dezmembrare_idempotent_post_guard() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        seed(&pool).await;

        let with_lines = create_dezmembrare(
            &pool,
            CreateDezmembrareInput {
                company_id: "co1".into(),
                gestiune_id: Some("gest1".into()),
                dismantled_product_id: "prod1".into(),
                dismantled_qty: 1.0,
                dezmembrare_date: "2026-04-01".into(),
                notes: None,
                lines: vec![DezmembrareLineInput {
                    product_id: "prod2".into(),
                    qty: 1.0,
                    unit_fair_value: 300.0,
                }],
            },
        )
        .await
        .unwrap();

        // Prima postare — trebuie să reușească
        post_dezmembrare(&pool, "co1", &with_lines.dezmembrare.id)
            .await
            .unwrap();

        // A doua postare — trebuie să fie respinsă
        let result = post_dezmembrare(&pool, "co1", &with_lines.dezmembrare.id).await;
        assert!(result.is_err(), "a doua postare trebuie să întoarcă eroare");
        match result.unwrap_err() {
            AppError::Validation(msg) => {
                assert!(
                    msg.contains("POSTED") || msg.contains("DRAFT"),
                    "eroarea trebuie să menționeze starea POSTED sau DRAFT; msg='{msg}'"
                );
            }
            other => panic!("eroare neașteptată: {other:?}"),
        }
    }
}
