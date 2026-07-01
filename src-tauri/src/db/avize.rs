//! Aviz de însoțire a mărfii — formular OMFP 2634/2015 14-3-6A.
//!
//! Un aviz însoțește marfa livrată fizic FĂRĂ factură imediată; factura urmează ulterior.
//!
//! ## Fluxul contabil
//!
//! **La EMITERE aviz** (DRAFT → ISSUED):
//!   D 418 «Clienți-facturi de întocmit» = total (net + TVA)  [debit]
//!   C 707 (sau cont venituri per kind) = net                  [credit]
//!   C 4428 «TVA neexigibilă» = TVA                           [credit]
//!   Ieșire stoc: D 607 = C 371 (la costul de stoc)            [via record_movement]
//!
//! **La CONVERSIE la factură** (ISSUED → INVOICED):
//!   Venitul a fost DEJA recunoscut la aviz. Conversia RECLASIFICĂ doar:
//!   D 4111 «Clienți» = total                                  [debit]
//!   C 418 «Clienți-facturi de întocmit» = total               [credit]
//!   D 4428 «TVA neexigibilă» = TVA                            [debit]
//!   C 4427 «TVA colectată» = TVA                              [credit]
//!   NU se repostează 707 (venitul recunoscut o singură dată, la aviz).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

use crate::db::gl::{post_manual_journal, ManualJournal};
use crate::db::invoices::round2;
use crate::db::models::{new_id, now_unix};
use crate::db::period_locks::is_period_locked;
use crate::db::stock_valuation::{record_movement, Dir, StockMovementInput};
use crate::error::{AppError, AppResult};

// ─── Models ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Aviz {
    pub id: String,
    pub company_id: String,
    pub contact_id: String,
    pub series: String,
    pub number: i64,
    pub full_number: String,
    pub aviz_date: String,
    pub transport_means: Option<String>,
    pub driver_name: Option<String>,
    pub vehicle_plate: Option<String>,
    pub destination: Option<String>,
    pub status: String,
    pub invoice_id: Option<String>,
    pub gestiune_id: Option<String>,
    pub currency: String,
    pub exchange_rate: Option<f64>,
    pub subtotal_amount: String,
    pub vat_amount: String,
    pub total_amount: String,
    pub notes: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AvizLine {
    pub id: String,
    pub aviz_id: String,
    pub position: i64,
    pub product_id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub quantity: String,
    pub unit: String,
    pub unit_price: String,
    pub vat_rate: String,
    pub vat_category: String,
    pub subtotal_amount: String,
    pub vat_amount: String,
    pub total_amount: String,
    pub revenue_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvizWithLines {
    pub aviz: Aviz,
    pub lines: Vec<AvizLine>,
}

// ─── Input types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAvizLineInput {
    pub product_id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub quantity: f64,
    pub unit: String,
    pub unit_price: f64,
    pub vat_rate: f64,
    pub vat_category: String,
    pub revenue_kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAvizInput {
    pub company_id: String,
    pub contact_id: String,
    pub series: String,
    pub aviz_date: String,
    pub gestiune_id: Option<String>,
    pub transport_means: Option<String>,
    pub driver_name: Option<String>,
    pub vehicle_plate: Option<String>,
    pub destination: Option<String>,
    pub currency: Option<String>,
    pub exchange_rate: Option<f64>,
    pub notes: Option<String>,
    pub lines: Vec<CreateAvizLineInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertToInvoiceInput {
    pub aviz_id: String,
    pub company_id: String,
    /// The invoice that was created for this aviz (linked by the frontend).
    pub invoice_id: String,
}

// ─── Decimal helpers ──────────────────────────────────────────────────────────

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s.trim()).unwrap_or(Decimal::ZERO)
}

fn revenue_account(kind: &str) -> &'static str {
    match kind.trim() {
        "product" => "701",
        "service" => "704",
        "reduction" => "709",
        _ => "707",
    }
}

// ─── Fetch helpers ────────────────────────────────────────────────────────────

async fn fetch_aviz(pool: &SqlitePool, company_id: &str, aviz_id: &str) -> AppResult<Aviz> {
    sqlx::query_as::<_, Aviz>(
        "SELECT id, company_id, contact_id, series, number, full_number, aviz_date, \
         transport_means, driver_name, vehicle_plate, destination, status, invoice_id, \
         gestiune_id, currency, exchange_rate, subtotal_amount, vat_amount, total_amount, \
         notes, created_at, updated_at \
         FROM avize \
         WHERE id = ?1 AND company_id = ?2",
    )
    .bind(aviz_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}

async fn fetch_aviz_lines(pool: &SqlitePool, aviz_id: &str) -> AppResult<Vec<AvizLine>> {
    let lines = sqlx::query_as::<_, AvizLine>(
        "SELECT id, aviz_id, position, product_id, name, description, quantity, unit, \
         unit_price, vat_rate, vat_category, subtotal_amount, vat_amount, total_amount, \
         revenue_kind \
         FROM aviz_lines \
         WHERE aviz_id = ?1 \
         ORDER BY position",
    )
    .bind(aviz_id)
    .fetch_all(pool)
    .await?;
    Ok(lines)
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Creează un aviz nou cu status DRAFT.
pub async fn create_aviz(pool: &SqlitePool, input: CreateAvizInput) -> AppResult<AvizWithLines> {
    if input.lines.is_empty() {
        return Err(AppError::Validation(
            "Avizul trebuie să conțină cel puțin o linie.".into(),
        ));
    }
    let period = &input.aviz_date[..7]; // "YYYY-MM"
    if is_period_locked(pool, &input.company_id, period).await? {
        return Err(AppError::Validation(format!(
            "Perioada {period} este blocată."
        )));
    }

    let currency = input.currency.as_deref().unwrap_or("RON").to_string();
    let aviz_id = new_id();
    let now = now_unix();

    // ── Calculate totals ──────────────────────────────────────────────────────
    struct LineCalc {
        id: String,
        qty: Decimal,
        unit_price: Decimal,
        vat_rate: Decimal,
        subtotal: Decimal,
        vat_amount: Decimal,
        total: Decimal,
    }

    let mut line_calcs = Vec::with_capacity(input.lines.len());
    let mut grand_subtotal = Decimal::ZERO;
    let mut grand_vat = Decimal::ZERO;

    for line in &input.lines {
        let qty = Decimal::try_from(line.quantity)
            .map_err(|_| AppError::Validation("Cantitate invalidă.".into()))?;
        let price = Decimal::try_from(line.unit_price)
            .map_err(|_| AppError::Validation("Preț unitar invalid.".into()))?;
        let rate = Decimal::try_from(line.vat_rate)
            .map_err(|_| AppError::Validation("Cotă TVA invalidă.".into()))?;

        let subtotal = round2(qty * price);
        let vat_amount = round2(subtotal * rate / Decimal::from(100));
        let total = round2(subtotal + vat_amount);

        grand_subtotal += subtotal;
        grand_vat += vat_amount;

        line_calcs.push(LineCalc {
            id: new_id(),
            qty,
            unit_price: price,
            vat_rate: rate,
            subtotal,
            vat_amount,
            total,
        });
    }
    let grand_total = round2(grand_subtotal + grand_vat);

    // ── Allocate next number (MAX+1 per company+series, within a transaction) ─
    let mut tx = pool.begin().await?;

    let max_number: Option<i64> =
        sqlx::query_scalar("SELECT MAX(number) FROM avize WHERE company_id = ?1 AND series = ?2")
            .bind(&input.company_id)
            .bind(&input.series)
            .fetch_optional(&mut *tx)
            .await?
            .flatten();

    let number = max_number.unwrap_or(0) + 1;
    let full_number = format!("{}-{:04}", input.series, number);

    sqlx::query(
        "INSERT INTO avize \
         (id, company_id, contact_id, series, number, full_number, aviz_date, \
          transport_means, driver_name, vehicle_plate, destination, status, invoice_id, \
          gestiune_id, currency, exchange_rate, subtotal_amount, vat_amount, total_amount, \
          notes, created_at, updated_at) \
         VALUES \
         (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,'DRAFT',NULL,?12,?13,?14,?15,?16,?17,?18,?19,?19)",
    )
    .bind(&aviz_id)
    .bind(&input.company_id)
    .bind(&input.contact_id)
    .bind(&input.series)
    .bind(number)
    .bind(&full_number)
    .bind(&input.aviz_date)
    .bind(&input.transport_means)
    .bind(&input.driver_name)
    .bind(&input.vehicle_plate)
    .bind(&input.destination)
    .bind(&input.gestiune_id)
    .bind(&currency)
    .bind(input.exchange_rate)
    .bind(format!("{:.2}", grand_subtotal))
    .bind(format!("{:.2}", grand_vat))
    .bind(format!("{:.2}", grand_total))
    .bind(&input.notes)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    for (pos, (line_input, calc)) in input.lines.iter().zip(line_calcs.iter()).enumerate() {
        let revenue_kind = line_input
            .revenue_kind
            .as_deref()
            .unwrap_or("goods")
            .to_string();
        sqlx::query(
            "INSERT INTO aviz_lines \
             (id, aviz_id, position, product_id, name, description, quantity, unit, \
              unit_price, vat_rate, vat_category, subtotal_amount, vat_amount, total_amount, \
              revenue_kind) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
        )
        .bind(&calc.id)
        .bind(&aviz_id)
        .bind((pos + 1) as i64)
        .bind(&line_input.product_id)
        .bind(&line_input.name)
        .bind(&line_input.description)
        .bind(format!("{:.6}", calc.qty))
        .bind(&line_input.unit)
        .bind(format!("{:.2}", calc.unit_price))
        .bind(format!("{:.2}", calc.vat_rate))
        .bind(&line_input.vat_category)
        .bind(format!("{:.2}", calc.subtotal))
        .bind(format!("{:.2}", calc.vat_amount))
        .bind(format!("{:.2}", calc.total))
        .bind(&revenue_kind)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    let aviz = fetch_aviz(pool, &input.company_id, &aviz_id).await?;
    let lines = fetch_aviz_lines(pool, &aviz_id).await?;
    Ok(AvizWithLines { aviz, lines })
}

/// Returnează un aviz (fără linii), scoped la company.
pub async fn get_aviz(pool: &SqlitePool, company_id: &str, aviz_id: &str) -> AppResult<Aviz> {
    fetch_aviz(pool, company_id, aviz_id).await
}

/// Returnează un aviz cu linii, scoped la company.
pub async fn get_aviz_with_lines(
    pool: &SqlitePool,
    company_id: &str,
    aviz_id: &str,
) -> AppResult<AvizWithLines> {
    let aviz = fetch_aviz(pool, company_id, aviz_id).await?;
    let lines = fetch_aviz_lines(pool, aviz_id).await?;
    Ok(AvizWithLines { aviz, lines })
}

/// Listează avizele unui company, ordonate descrescător după dată.
pub async fn list_avize(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<Aviz>> {
    let avize = sqlx::query_as::<_, Aviz>(
        "SELECT id, company_id, contact_id, series, number, full_number, aviz_date, \
         transport_means, driver_name, vehicle_plate, destination, status, invoice_id, \
         gestiune_id, currency, exchange_rate, subtotal_amount, vat_amount, total_amount, \
         notes, created_at, updated_at \
         FROM avize \
         WHERE company_id = ?1 \
         ORDER BY aviz_date DESC, number DESC",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(avize)
}

/// Emite un aviz (DRAFT → ISSUED): postează stoc OUT + nota GL 418/707/4428.
pub async fn issue_aviz(pool: &SqlitePool, company_id: &str, aviz_id: &str) -> AppResult<Aviz> {
    let aviz = fetch_aviz(pool, company_id, aviz_id).await?;

    if aviz.status != "DRAFT" {
        return Err(AppError::Validation(format!(
            "Avizul are status '{}'; doar avizele DRAFT pot fi emise.",
            aviz.status
        )));
    }

    let period = &aviz.aviz_date[..7];
    if is_period_locked(pool, company_id, period).await? {
        return Err(AppError::Validation(format!(
            "Perioada {period} este blocată."
        )));
    }

    let lines = fetch_aviz_lines(pool, aviz_id).await?;

    // ── 1. Stock OUT per line ─────────────────────────────────────────────────
    for line in &lines {
        if let Some(product_id) = &line.product_id {
            let input = StockMovementInput {
                company_id: company_id.to_string(),
                product_id: product_id.clone(),
                entry_date: aviz.aviz_date.clone(),
                qty: line.quantity.clone(),
                unit_cost: None,
                doc_type: Some("AVIZ".to_string()),
                doc_ref: Some(aviz_id.to_string()),
                gestiune_id: aviz.gestiune_id.clone(),
            };
            record_movement(pool, &input, Dir::Out).await?;

            // Stamp aviz_id on the just-inserted stock_ledger row (most-recent OUT for this doc_ref).
            sqlx::query(
                "UPDATE stock_ledger SET aviz_id = ?1 \
                 WHERE rowid = (\
                   SELECT rowid FROM stock_ledger \
                   WHERE doc_ref = ?2 AND direction = 'OUT' AND aviz_id IS NULL \
                   ORDER BY rowid DESC LIMIT 1\
                 )",
            )
            .bind(aviz_id)
            .bind(aviz_id)
            .execute(pool)
            .await?;
        }
    }

    // ── 2. Read the carrying cost from stock_ledger ───────────────────────────
    // MONEY-008: fetch TEXT value strings and sum as Decimal — avoid CAST(... AS REAL)
    // float round-trip which injects sub-cent drift on multi-line avize.
    let carrying_cost_rows: Vec<(String,)> = sqlx::query_as(
        "SELECT COALESCE(value,'0') FROM stock_ledger \
         WHERE aviz_id = ?1 AND direction = 'OUT'",
    )
    .bind(aviz_id)
    .fetch_all(pool)
    .await?;

    let carrying_cost: Decimal = carrying_cost_rows
        .iter()
        .map(|(v,)| dec(v))
        .fold(Decimal::ZERO, |acc, v| acc + v);
    // Round to 2dp for GL posting.
    let carrying_cost = round2(carrying_cost);

    // ── 3. Compute totals for GL posting ─────────────────────────────────────
    let vat_dec = dec(&aviz.vat_amount);
    let total_dec = dec(&aviz.total_amount);

    // P3: fetch partner CUI for GL partner-ledger stamping on 418/4111.
    let partner_cui: Option<String> =
        sqlx::query_scalar("SELECT cui FROM contacts WHERE id = ?1 AND company_id = ?2")
            .bind(&aviz.contact_id)
            .bind(company_id)
            .fetch_optional(pool)
            .await?
            .flatten();

    // Determine the dominant revenue account (use the first line's kind; if all lines
    // share the same kind, this is exact; mixed-kind avize are rare but defensible —
    // the caller can always split into multiple avize). For rigour we post one
    // 70x line per unique revenue_kind group.
    // Build (revenue_kind → net) map.
    let mut kind_map: std::collections::BTreeMap<String, Decimal> =
        std::collections::BTreeMap::new();
    for line in &lines {
        *kind_map
            .entry(line.revenue_kind.clone())
            .or_insert(Decimal::ZERO) += dec(&line.subtotal_amount);
    }

    // ── 4. Post revenue GL note (source_type='AVIZ') ─────────────────────────
    // D 418 = total; C 70x (per kind) = net; C 4428 = vat.
    // All must balance: 418 = Σ70x + 4428.
    let mut revenue_lines: Vec<(&'static str, Decimal, Decimal)> = Vec::new();
    revenue_lines.push(("418", total_dec, Decimal::ZERO)); // D 418

    for (kind, net) in &kind_map {
        let acct = revenue_account(kind);
        revenue_lines.push((acct, Decimal::ZERO, *net)); // C 70x
    }
    revenue_lines.push(("4428", Decimal::ZERO, vat_dec)); // C 4428

    // We need owned strings because the account codes from revenue_account are &'static str —
    // but the Vecs hold owned tuples. Let's use a vec of (String, Decimal, Decimal) and
    // then build the slice reference correctly.
    let revenue_owned: Vec<(String, Decimal, Decimal)> = revenue_lines
        .into_iter()
        .map(|(a, d, c)| (a.to_string(), d, c))
        .collect();
    let revenue_refs: Vec<(&str, Decimal, Decimal)> = revenue_owned
        .iter()
        .map(|(a, d, c)| (a.as_str(), *d, *c))
        .collect();

    let aviz_description = format!("Aviz {}", aviz.full_number);
    post_manual_journal(
        pool,
        &ManualJournal {
            company_id,
            journal_id: "VANZARI",
            journal_type: "SALES",
            source_type: "AVIZ",
            source_id: aviz_id,
            date: &aviz.aviz_date,
            description: &aviz_description,
            partner_cui: partner_cui.as_deref(), // P3: stamp CUI on 418 leg
        },
        &revenue_refs,
    )
    .await?;

    // ── 5. Post COGS GL note (source_type='AVIZ_COGS') ───────────────────────
    // D 607 = carrying_cost; C 371 = carrying_cost.
    if carrying_cost > Decimal::ZERO {
        let cogs_description = format!("Descărcare stoc aviz {}", aviz.full_number);
        post_manual_journal(
            pool,
            &ManualJournal {
                company_id,
                journal_id: "VANZARI",
                journal_type: "SALES",
                source_type: "AVIZ_COGS",
                source_id: aviz_id,
                date: &aviz.aviz_date,
                description: &cogs_description,
                partner_cui: None,
            },
            &[
                ("607", carrying_cost, Decimal::ZERO),
                ("371", Decimal::ZERO, carrying_cost),
            ],
        )
        .await?;
    }

    // ── 6. Update status ──────────────────────────────────────────────────────
    sqlx::query(
        "UPDATE avize SET status = 'ISSUED', updated_at = ?1 WHERE id = ?2 AND company_id = ?3",
    )
    .bind(now_unix())
    .bind(aviz_id)
    .bind(company_id)
    .execute(pool)
    .await?;

    fetch_aviz(pool, company_id, aviz_id).await
}

/// Convertește avizul la factură (ISSUED → INVOICED): reclasifică 418 → 4111, 4428 → 4427.
/// Idempotent dacă se apelează cu același invoice_id.
pub async fn convert_aviz_to_invoice(
    pool: &SqlitePool,
    company_id: &str,
    aviz_id: &str,
    invoice_id: &str,
) -> AppResult<Aviz> {
    let aviz = fetch_aviz(pool, company_id, aviz_id).await?;

    // Idempotent: same invoice_id already set → return as-is.
    if aviz.invoice_id.as_deref() == Some(invoice_id) {
        return Ok(aviz);
    }

    // Guard: must be ISSUED.
    if aviz.status != "ISSUED" {
        return Err(AppError::Validation(format!(
            "Avizul are status '{}'; conversia la factură necesită status ISSUED.",
            aviz.status
        )));
    }

    // Guard: different invoice_id already set (would be a conflict).
    if aviz.invoice_id.is_some() {
        return Err(AppError::Validation(
            "Avizul este deja asociat altei facturi.".into(),
        ));
    }

    // P1: fetch the linked invoice's issue_date (for correct GL date + period-lock check).
    let invoice_issue_date: String =
        sqlx::query_scalar("SELECT issue_date FROM invoices WHERE id = ?1 AND company_id = ?2")
            .bind(invoice_id)
            .bind(company_id)
            .fetch_optional(pool)
            .await?
            .flatten()
            .ok_or_else(|| AppError::Validation("Factura specificată nu există.".into()))?;

    // P2: period-lock on the invoice issue period (VAT exigibility is at invoice date).
    let invoice_period = &invoice_issue_date[..7];
    if is_period_locked(pool, company_id, invoice_period).await? {
        return Err(AppError::Validation(format!(
            "Perioada {invoice_period} este blocată — conversia avizului nu poate posta în ea."
        )));
    }

    // P3: fetch partner CUI for GL partner-ledger stamping on 4111/418.
    let partner_cui: Option<String> =
        sqlx::query_scalar("SELECT cui FROM contacts WHERE id = ?1 AND company_id = ?2")
            .bind(&aviz.contact_id)
            .bind(company_id)
            .fetch_optional(pool)
            .await?
            .flatten();

    let total_dec = dec(&aviz.total_amount);
    let vat_dec = dec(&aviz.vat_amount);

    // ── Post RECLASS GL note (source_type='AVIZ_RECLASS') ────────────────────
    // D 4111 = total; C 418 = total; D 4428 = vat; C 4427 = vat.
    // Balance: debit(4111 + 4428) = total + vat; credit(418 + 4427) = total + vat. ✓
    // P2: date = invoice issue_date (VAT exigibility at invoice, not aviz_date).
    let reclass_description = format!("Reclasificare aviz {} → factură", aviz.full_number);
    post_manual_journal(
        pool,
        &ManualJournal {
            company_id,
            journal_id: "VANZARI",
            journal_type: "SALES",
            source_type: "AVIZ_RECLASS",
            source_id: aviz_id,
            date: &invoice_issue_date, // P2: invoice date, not aviz_date
            description: &reclass_description,
            partner_cui: partner_cui.as_deref(), // P3: stamp CUI on 4111/418 legs
        },
        &[
            ("4111", total_dec, Decimal::ZERO), // D 4111
            ("418", Decimal::ZERO, total_dec),  // C 418
            ("4428", vat_dec, Decimal::ZERO),   // D 4428
            ("4427", Decimal::ZERO, vat_dec),   // C 4427
        ],
    )
    .await?;

    // P1: stamp aviz_id on the linked invoice so generate_gl_entries skips it.
    sqlx::query("UPDATE invoices SET aviz_id = ?1 WHERE id = ?2 AND company_id = ?3")
        .bind(aviz_id)
        .bind(invoice_id)
        .bind(company_id)
        .execute(pool)
        .await?;

    // ── Update aviz ───────────────────────────────────────────────────────────
    sqlx::query(
        "UPDATE avize SET status = 'INVOICED', invoice_id = ?1, updated_at = ?2 \
         WHERE id = ?3 AND company_id = ?4",
    )
    .bind(invoice_id)
    .bind(now_unix())
    .bind(aviz_id)
    .bind(company_id)
    .execute(pool)
    .await?;

    fetch_aviz(pool, company_id, aviz_id).await
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec as rdec;
    use sqlx::SqlitePool;

    /// Set up an in-memory DB with full migrations and seed:
    /// - company co1 (RO12345674)
    /// - contact cnt1 (CUSTOMER, co1)
    /// - gestiune for co1
    /// - product prod1 (co1, Marfa, buc)
    /// - stock IN: 10 units @ 50.00 RON each (total cost 500.00)
    async fn setup() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();

        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co1','RO12345674','Test SRL','Str. Test 1','București','B','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO contacts (id, company_id, contact_type, legal_name) \
             VALUES ('cnt1','co1','CUSTOMER','Client Test SRL')",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO products (id, company_id, name, unit) \
             VALUES ('prod1','co1','Marfa','buc')",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Stock IN: 10 buc @ 50.00 RON
        let stock_in = StockMovementInput {
            company_id: "co1".into(),
            product_id: "prod1".into(),
            entry_date: "2026-06-01".into(),
            qty: "10".into(),
            unit_cost: Some("50.00".into()),
            doc_type: Some("NIR".into()),
            doc_ref: Some("nir-001".into()),
            gestiune_id: None,
        };
        record_movement(&pool, &stock_in, Dir::In)
            .await
            .expect("stock IN setup failed");

        pool
    }

    fn make_input() -> CreateAvizInput {
        CreateAvizInput {
            company_id: "co1".into(),
            contact_id: "cnt1".into(),
            series: "AV".into(),
            aviz_date: "2026-06-15".into(),
            gestiune_id: None,
            transport_means: None,
            driver_name: None,
            vehicle_plate: None,
            destination: None,
            currency: None,
            exchange_rate: None,
            notes: None,
            lines: vec![CreateAvizLineInput {
                product_id: Some("prod1".into()),
                name: "Marfa".into(),
                description: None,
                quantity: 1.0,
                unit: "buc".into(),
                unit_price: 1000.00,
                vat_rate: 21.0,
                vat_category: "S".into(),
                revenue_kind: Some("goods".into()),
            }],
        }
    }

    #[tokio::test]
    async fn aviz_issue_posts_correct_gl() {
        let pool = setup().await;

        let awl = create_aviz(&pool, make_input()).await.expect("create OK");
        let aviz_id = awl.aviz.id.clone();

        issue_aviz(&pool, "co1", &aviz_id).await.expect("issue OK");

        // ── Verify GL entries for AVIZ ────────────────────────────────────────
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT e.account_code, e.debit, e.credit \
             FROM gl_entry e \
             JOIN gl_journal j ON j.id = e.journal_pk \
             WHERE j.company_id = 'co1' AND j.source_type = 'AVIZ' AND j.source_id = ?1",
        )
        .bind(&aviz_id)
        .fetch_all(&pool)
        .await
        .unwrap();

        let get = |acct: &str, field: &str| -> Decimal {
            rows.iter()
                .filter(|(a, _, _)| a == acct)
                .map(|(_, d, c)| if field == "d" { dec(d) } else { dec(c) })
                .fold(Decimal::ZERO, |acc, v| acc + v)
        };

        assert_eq!(get("418", "d"), rdec!(1210.00), "D 418 = 1210.00");
        assert_eq!(get("707", "c"), rdec!(1000.00), "C 707 = 1000.00");
        assert_eq!(get("4428", "c"), rdec!(210.00), "C 4428 = 210.00");

        // Journal must balance.
        let total_d: Decimal = rows.iter().map(|(_, d, _)| dec(d)).sum();
        let total_c: Decimal = rows.iter().map(|(_, _, c)| dec(c)).sum();
        assert_eq!(total_d, total_c, "AVIZ journal must balance");

        // ── Verify stock OUT ──────────────────────────────────────────────────
        let out_rows: Vec<(String,)> =
            sqlx::query_as("SELECT id FROM stock_ledger WHERE aviz_id = ?1 AND direction = 'OUT'")
                .bind(&aviz_id)
                .fetch_all(&pool)
                .await
                .unwrap();

        assert_eq!(out_rows.len(), 1, "exactly one OUT row in stock_ledger");
    }

    /// Insert a bare-minimum invoice row (DRAFT, no line items) so the FK
    /// avize.invoice_id → invoices.id is satisfied.  Used by existing convert/idempotent tests.
    async fn seed_invoice(pool: &SqlitePool, inv_id: &str) {
        sqlx::query(
            "INSERT INTO invoices \
             (id, company_id, contact_id, series, number, full_number, issue_date, due_date, \
              subtotal_amount, vat_amount, total_amount, status, created_at, updated_at) \
             VALUES (?1,'co1','cnt1','F',1,'F-0001','2026-06-15','2026-06-15', \
                    '1000.00','210.00','1210.00','DRAFT', \
                    strftime('%s','now'), strftime('%s','now'))",
        )
        .bind(inv_id)
        .execute(pool)
        .await
        .unwrap();
    }

    /// Insert a VALIDATED invoice with one line item — needed for generate_gl_entries to
    /// process it (skips invoices with no lines).
    async fn seed_validated_invoice_with_line(pool: &SqlitePool, inv_id: &str, number: i64) {
        let full_number = format!("F-{number:04}");
        sqlx::query(
            "INSERT INTO invoices \
             (id, company_id, contact_id, series, number, full_number, issue_date, due_date, \
              subtotal_amount, vat_amount, total_amount, status, created_at, updated_at) \
             VALUES (?1,'co1','cnt1','F',?2,?3,'2026-06-20','2026-06-20', \
                    '1000.00','210.00','1210.00','VALIDATED', \
                    strftime('%s','now'), strftime('%s','now'))",
        )
        .bind(inv_id)
        .bind(number)
        .bind(&full_number)
        .execute(pool)
        .await
        .unwrap();
        // Insert one line item so generate_gl_entries doesn't skip this invoice.
        let line_id = crate::db::models::new_id();
        sqlx::query(
            "INSERT INTO invoice_line_items \
             (id, invoice_id, position, name, quantity, unit, unit_price, \
              vat_rate, vat_category, subtotal_amount, vat_amount, total_amount, \
              revenue_kind) \
             VALUES (?1,?2,1,'Marfa','1','buc','1000.00','21','S','1000.00','210.00','1210.00','goods')",
        )
        .bind(&line_id)
        .bind(inv_id)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn aviz_convert_reclassifies_vat_revenue_once() {
        let pool = setup().await;
        seed_invoice(&pool, "inv-test-1").await;

        let awl = create_aviz(&pool, make_input()).await.expect("create OK");
        let aviz_id = awl.aviz.id.clone();

        issue_aviz(&pool, "co1", &aviz_id).await.expect("issue OK");

        let updated = convert_aviz_to_invoice(&pool, "co1", &aviz_id, "inv-test-1")
            .await
            .expect("convert OK");

        assert_eq!(updated.status, "INVOICED");
        assert_eq!(updated.invoice_id.as_deref(), Some("inv-test-1"));

        // ── AVIZ_RECLASS entries ──────────────────────────────────────────────
        let reclass_rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT e.account_code, e.debit, e.credit \
             FROM gl_entry e \
             JOIN gl_journal j ON j.id = e.journal_pk \
             WHERE j.company_id = 'co1' AND j.source_type = 'AVIZ_RECLASS' AND j.source_id = ?1",
        )
        .bind(&aviz_id)
        .fetch_all(&pool)
        .await
        .unwrap();

        let get_r = |acct: &str, field: &str| -> Decimal {
            reclass_rows
                .iter()
                .filter(|(a, _, _)| a == acct)
                .map(|(_, d, c)| if field == "d" { dec(d) } else { dec(c) })
                .fold(Decimal::ZERO, |acc, v| acc + v)
        };

        assert_eq!(get_r("4111", "d"), rdec!(1210.00), "D 4111 = 1210.00");
        assert_eq!(get_r("418", "c"), rdec!(1210.00), "C 418 = 1210.00");
        assert_eq!(get_r("4428", "d"), rdec!(210.00), "D 4428 = 210.00");
        assert_eq!(get_r("4427", "c"), rdec!(210.00), "C 4427 = 210.00");

        // 707 must NOT appear in AVIZ_RECLASS (revenue recognised once at aviz).
        let has_707 = reclass_rows.iter().any(|(a, _, _)| a == "707");
        assert!(!has_707, "707 must not appear in AVIZ_RECLASS entries");

        // ── Combined AVIZ + AVIZ_RECLASS: 707 total debit == 1000.00 ─────────
        let all_rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT e.account_code, e.debit, e.credit \
             FROM gl_entry e \
             JOIN gl_journal j ON j.id = e.journal_pk \
             WHERE j.company_id = 'co1' \
               AND j.source_type IN ('AVIZ','AVIZ_RECLASS') \
               AND j.source_id = ?1",
        )
        .bind(&aviz_id)
        .fetch_all(&pool)
        .await
        .unwrap();

        let total_707_credit: Decimal = all_rows
            .iter()
            .filter(|(a, _, _)| a == "707")
            .map(|(_, _, c)| dec(c))
            .sum();
        assert_eq!(
            total_707_credit,
            rdec!(1000.00),
            "707 credit recognised exactly once"
        );
    }

    /// P1 regression: aviz-linked invoice must NOT re-post 707/4427 in generate_gl_entries.
    /// Revenue (707) is recognised once at aviz issuance; convert posts the 418→4111 reclass.
    /// After generate_gl_entries: 707 credited exactly once, 4427 credited exactly once,
    /// 4111 debited exactly once (= receivable), 418 net = 0.
    #[tokio::test]
    async fn aviz_linked_invoice_no_double_post_revenue() {
        use crate::db::gl::generate_gl_entries;

        let pool = setup().await;
        // Seed a VALIDATED invoice (issue_date 2026-06-20, period 2026-06) with a line item.
        seed_validated_invoice_with_line(&pool, "inv-p1-1", 10).await;

        // Create + issue aviz (period 2026-06).
        let awl = create_aviz(&pool, make_input()).await.expect("create OK");
        let aviz_id = awl.aviz.id.clone();
        issue_aviz(&pool, "co1", &aviz_id).await.expect("issue OK");

        // Convert aviz → invoice. This stamps invoice.aviz_id and posts AVIZ_RECLASS.
        convert_aviz_to_invoice(&pool, "co1", &aviz_id, "inv-p1-1")
            .await
            .expect("convert OK");

        // Now run generate_gl_entries for the invoice period.
        generate_gl_entries(&pool, "co1", "2026-06-01", "2026-06-30", false)
            .await
            .expect("generate_gl_entries OK");

        // ── Gather ALL gl_entry rows for co1 in the period ───────────────────
        let all_rows: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT j.source_type, e.account_code, e.debit, e.credit \
             FROM gl_entry e \
             JOIN gl_journal j ON j.id = e.journal_pk \
             WHERE j.company_id = 'co1'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        let sum_by = |acct: &str, field: &str| -> Decimal {
            all_rows
                .iter()
                .filter(|(_, a, _, _)| a == acct)
                .map(|(_, _, d, c)| if field == "d" { dec(d) } else { dec(c) })
                .fold(Decimal::ZERO, |acc, v| acc + v)
        };

        // 707 credit: exactly 1000.00 (from AVIZ, not from INVOICE re-post).
        assert_eq!(
            sum_by("707", "c"),
            rdec!(1000.00),
            "707 credited exactly once — no double-post from aviz-linked invoice"
        );

        // 4427 credit: exactly 210.00 (from AVIZ_RECLASS only).
        assert_eq!(
            sum_by("4427", "c"),
            rdec!(210.00),
            "4427 credited exactly once — no double-post from aviz-linked invoice"
        );

        // 4111 debit: exactly 1210.00 (from AVIZ_RECLASS).
        assert_eq!(
            sum_by("4111", "d"),
            rdec!(1210.00),
            "4111 debited exactly once (AVIZ_RECLASS only)"
        );

        // 418 net: D 1210 (AVIZ) - C 1210 (AVIZ_RECLASS) = 0 — fully cleared.
        let net_418 = sum_by("418", "d") - sum_by("418", "c");
        assert_eq!(net_418, Decimal::ZERO, "418 net = 0 (fully reclassified)");

        // No INVOICE source_type journal must exist for the aviz-linked invoice.
        let invoice_journals: Vec<_> = all_rows
            .iter()
            .filter(|(st, _, _, _)| st == "INVOICE")
            .collect();
        assert!(
            invoice_journals.is_empty(),
            "no INVOICE GL journal should be generated for an aviz-linked invoice; \
             found: {invoice_journals:?}"
        );
    }

    #[tokio::test]
    async fn aviz_idempotent_convert() {
        let pool = setup().await;
        seed_invoice(&pool, "inv-test-1").await;

        let awl = create_aviz(&pool, make_input()).await.expect("create OK");
        let aviz_id = awl.aviz.id.clone();

        issue_aviz(&pool, "co1", &aviz_id).await.expect("issue OK");

        // First convert.
        convert_aviz_to_invoice(&pool, "co1", &aviz_id, "inv-test-1")
            .await
            .expect("first convert OK");

        // Idempotent: same invoice_id → should succeed.
        let result = convert_aviz_to_invoice(&pool, "co1", &aviz_id, "inv-test-1").await;
        assert!(
            result.is_ok(),
            "idempotent re-convert with same invoice_id must succeed"
        );

        // Different invoice_id → should fail with Validation error.
        let err = convert_aviz_to_invoice(&pool, "co1", &aviz_id, "inv-other")
            .await
            .expect_err("different invoice_id must fail");
        assert!(
            matches!(err, AppError::Validation(_)),
            "expected Validation error, got: {err:?}"
        );
    }
}
