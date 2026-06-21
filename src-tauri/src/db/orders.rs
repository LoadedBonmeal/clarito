//! Comenzi (documente comerciale pre-contabile).
//! NO GL, no VAT filing, no e-Factura. Contorizare proprie: CMD-.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

use crate::db::invoices::{validate_and_total_lines, CreateInvoiceInput, CreateLineInput};
use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

// ─── Models ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Order {
    pub id: String,
    pub company_id: String,
    pub contact_id: Option<String>,
    pub series: Option<String>,
    pub number: i64,
    pub full_number: Option<String>,
    pub order_date: String,
    pub expected_delivery: Option<String>,
    pub currency: String,
    pub exchange_rate: Option<String>,
    pub subtotal_amount: String,
    pub vat_amount: String,
    pub total_amount: String,
    pub status: String,
    pub notes: Option<String>,
    pub converted_invoice_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct OrderLine {
    pub id: String,
    pub order_id: String,
    pub position: i64,
    pub name: String,
    pub description: Option<String>,
    pub quantity: String,
    pub unit: Option<String>,
    pub unit_price: String,
    pub vat_rate: String,
    pub vat_category: Option<String>,
    pub subtotal_amount: String,
    pub vat_amount: String,
    pub total_amount: String,
    pub revenue_kind: Option<String>,
    pub qty_reserved: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderWithLines {
    pub order: Order,
    pub lines: Vec<OrderLine>,
}

// ─── Inputs ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateOrderLineInput {
    pub name: String,
    pub description: Option<String>,
    pub quantity: f64,
    pub unit: Option<String>,
    pub unit_price: f64,
    pub vat_rate: f64,
    pub vat_category: String,
    pub revenue_kind: Option<String>,
    pub qty_reserved: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateOrderInput {
    pub company_id: String,
    pub contact_id: Option<String>,
    pub series: Option<String>,
    pub order_date: String,
    pub expected_delivery: Option<String>,
    pub currency: Option<String>,
    pub exchange_rate: Option<String>,
    pub notes: Option<String>,
    pub lines: Vec<CreateOrderLineInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOrderInput {
    pub contact_id: Option<String>,
    pub series: Option<String>,
    pub order_date: String,
    pub expected_delivery: Option<String>,
    pub currency: Option<String>,
    pub exchange_rate: Option<String>,
    pub notes: Option<String>,
    pub lines: Vec<CreateOrderLineInput>,
}

// ─── Helpers ───────────────────────────────────────────────────────────────

fn to_invoice_line(o: &CreateOrderLineInput) -> CreateLineInput {
    CreateLineInput {
        name: o.name.clone(),
        description: o.description.clone(),
        quantity: o.quantity,
        unit: o.unit.clone().unwrap_or_default(),
        unit_price: o.unit_price,
        vat_rate: o.vat_rate,
        vat_category: o.vat_category.clone(),
        cpv_code: None,
        art331_code: None,
        revenue_kind: o.revenue_kind.clone(),
    }
}

// ─── Queries ───────────────────────────────────────────────────────────────

pub async fn get(pool: &SqlitePool, id: &str) -> AppResult<Order> {
    sqlx::query_as::<_, Order>("SELECT * FROM orders WHERE id = ?1")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn get_with_lines(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
) -> AppResult<OrderWithLines> {
    let order =
        sqlx::query_as::<_, Order>("SELECT * FROM orders WHERE id = ?1 AND company_id = ?2")
            .bind(id)
            .bind(company_id)
            .fetch_optional(pool)
            .await?
            .ok_or(AppError::NotFound)?;

    let lines = sqlx::query_as::<_, OrderLine>(
        "SELECT * FROM order_lines WHERE order_id = ?1 ORDER BY position ASC",
    )
    .bind(id)
    .fetch_all(pool)
    .await?;

    Ok(OrderWithLines { order, lines })
}

pub async fn list(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<Order>> {
    let rows = sqlx::query_as::<_, Order>(
        "SELECT * FROM orders WHERE company_id = ?1 ORDER BY order_date DESC, number DESC",
    )
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// ─── Create ────────────────────────────────────────────────────────────────

pub async fn create(pool: &SqlitePool, input: CreateOrderInput) -> AppResult<Order> {
    let invoice_lines: Vec<CreateLineInput> = input.lines.iter().map(to_invoice_line).collect();
    let (subtotal, vat_total, total, line_rows) = validate_and_total_lines(&invoice_lines, "")?;

    let series = input
        .series
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("CMD");

    let order_id = new_id();
    let now = now_unix();
    let currency = input.currency.as_deref().unwrap_or("RON");

    let mut tx = pool.begin().await?;

    sqlx::query("UPDATE companies SET last_order_number = last_order_number + 1 WHERE id = ?1")
        .bind(&input.company_id)
        .execute(&mut *tx)
        .await?;

    let allocated_number: i64 =
        sqlx::query_scalar("SELECT last_order_number FROM companies WHERE id = ?1")
            .bind(&input.company_id)
            .fetch_one(&mut *tx)
            .await?;

    let full_number = format!("{}-{:04}", series, allocated_number);

    sqlx::query(
        "INSERT INTO orders (
            id, company_id, contact_id, series, number, full_number,
            order_date, expected_delivery, currency, exchange_rate,
            subtotal_amount, vat_amount, total_amount, status, notes,
            converted_invoice_id, created_at, updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6,
            ?7, ?8, ?9, ?10,
            ?11, ?12, ?13, 'draft', ?14,
            NULL, ?15, ?15
        )",
    )
    .bind(&order_id)
    .bind(&input.company_id)
    .bind(&input.contact_id)
    .bind(series)
    .bind(allocated_number)
    .bind(&full_number)
    .bind(&input.order_date)
    .bind(&input.expected_delivery)
    .bind(currency)
    .bind(&input.exchange_rate)
    .bind(&subtotal)
    .bind(&vat_total)
    .bind(&total)
    .bind(&input.notes)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    for (position, (o_line, (line_id, line_sub, line_vat, line_tot))) in
        input.lines.iter().zip(line_rows.iter()).enumerate()
    {
        let qty_str = Decimal::try_from(o_line.quantity)
            .unwrap_or(Decimal::ZERO)
            .round_dp_with_strategy(6, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
            .to_string();
        let price_str = Decimal::try_from(o_line.unit_price)
            .unwrap_or(Decimal::ZERO)
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
            .to_string();
        let rate_str = {
            let raw = Decimal::try_from(o_line.vat_rate).unwrap_or(Decimal::ZERO);
            let eff = if o_line.vat_category == "S" {
                raw
            } else {
                Decimal::ZERO
            };
            eff.round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
                .to_string()
        };
        let reserved_str = Decimal::try_from(o_line.qty_reserved.unwrap_or(0.0))
            .unwrap_or(Decimal::ZERO)
            .to_string();

        sqlx::query(
            "INSERT INTO order_lines (
                id, order_id, position, name, description,
                quantity, unit, unit_price, vat_rate, vat_category,
                subtotal_amount, vat_amount, total_amount, revenue_kind, qty_reserved
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5,
                ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, ?14, ?15
            )",
        )
        .bind(line_id)
        .bind(&order_id)
        .bind((position as i64) + 1)
        .bind(&o_line.name)
        .bind(&o_line.description)
        .bind(qty_str)
        .bind(&o_line.unit)
        .bind(price_str)
        .bind(rate_str)
        .bind(&o_line.vat_category)
        .bind(line_sub)
        .bind(line_vat)
        .bind(line_tot)
        .bind(&o_line.revenue_kind)
        .bind(reserved_str)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    get(pool, &order_id).await
}

// ─── Update ────────────────────────────────────────────────────────────────

pub async fn update(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    input: UpdateOrderInput,
) -> AppResult<Order> {
    let order =
        sqlx::query_as::<_, Order>("SELECT * FROM orders WHERE id = ?1 AND company_id = ?2")
            .bind(id)
            .bind(company_id)
            .fetch_optional(pool)
            .await?
            .ok_or(AppError::NotFound)?;

    if order.status != "draft" {
        return Err(AppError::Validation(format!(
            "Comanda poate fi modificată doar în status 'draft' (curent: '{}').",
            order.status
        )));
    }

    let invoice_lines: Vec<CreateLineInput> = input.lines.iter().map(to_invoice_line).collect();
    let (subtotal, vat_total, total, line_rows) = validate_and_total_lines(&invoice_lines, "")?;

    let series = input
        .series
        .as_deref()
        .filter(|s| !s.is_empty())
        .or(order.series.as_deref())
        .unwrap_or("CMD");
    let currency = input
        .currency
        .as_deref()
        .unwrap_or(&order.currency)
        .to_string();
    let now = now_unix();

    let mut tx = pool.begin().await?;

    sqlx::query("DELETE FROM order_lines WHERE order_id = ?1")
        .bind(id)
        .execute(&mut *tx)
        .await?;

    sqlx::query(
        "UPDATE orders SET
            contact_id = ?1, series = ?2,
            order_date = ?3, expected_delivery = ?4, currency = ?5, exchange_rate = ?6,
            subtotal_amount = ?7, vat_amount = ?8, total_amount = ?9,
            notes = ?10, updated_at = ?11
         WHERE id = ?12 AND company_id = ?13",
    )
    .bind(&input.contact_id)
    .bind(series)
    .bind(&input.order_date)
    .bind(&input.expected_delivery)
    .bind(&currency)
    .bind(&input.exchange_rate)
    .bind(&subtotal)
    .bind(&vat_total)
    .bind(&total)
    .bind(&input.notes)
    .bind(now)
    .bind(id)
    .bind(company_id)
    .execute(&mut *tx)
    .await?;

    for (position, (o_line, (line_id, line_sub, line_vat, line_tot))) in
        input.lines.iter().zip(line_rows.iter()).enumerate()
    {
        let qty_str = Decimal::try_from(o_line.quantity)
            .unwrap_or(Decimal::ZERO)
            .round_dp_with_strategy(6, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
            .to_string();
        let price_str = Decimal::try_from(o_line.unit_price)
            .unwrap_or(Decimal::ZERO)
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
            .to_string();
        let rate_str = {
            let raw = Decimal::try_from(o_line.vat_rate).unwrap_or(Decimal::ZERO);
            let eff = if o_line.vat_category == "S" {
                raw
            } else {
                Decimal::ZERO
            };
            eff.round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
                .to_string()
        };
        let reserved_str = Decimal::try_from(o_line.qty_reserved.unwrap_or(0.0))
            .unwrap_or(Decimal::ZERO)
            .to_string();

        sqlx::query(
            "INSERT INTO order_lines (
                id, order_id, position, name, description,
                quantity, unit, unit_price, vat_rate, vat_category,
                subtotal_amount, vat_amount, total_amount, revenue_kind, qty_reserved
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5,
                ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, ?14, ?15
            )",
        )
        .bind(line_id)
        .bind(id)
        .bind((position as i64) + 1)
        .bind(&o_line.name)
        .bind(&o_line.description)
        .bind(qty_str)
        .bind(&o_line.unit)
        .bind(price_str)
        .bind(rate_str)
        .bind(&o_line.vat_category)
        .bind(line_sub)
        .bind(line_vat)
        .bind(line_tot)
        .bind(&o_line.revenue_kind)
        .bind(reserved_str)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    get(pool, id).await
}

// ─── Status ────────────────────────────────────────────────────────────────

pub async fn set_status(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    status: &str,
) -> AppResult<Order> {
    let order =
        sqlx::query_as::<_, Order>("SELECT * FROM orders WHERE id = ?1 AND company_id = ?2")
            .bind(id)
            .bind(company_id)
            .fetch_optional(pool)
            .await?
            .ok_or(AppError::NotFound)?;

    let allowed: &[&str] = match order.status.as_str() {
        "draft" => &["sent", "accepted", "cancelled"],
        "sent" => &["accepted", "cancelled"],
        "accepted" => &["cancelled"],
        _ => &[],
    };

    if !allowed.contains(&status) {
        return Err(AppError::Validation(format!(
            "Tranziție status invalidă: '{}' → '{status}'.",
            order.status
        )));
    }

    let now = now_unix();
    sqlx::query(
        "UPDATE orders SET status = ?1, updated_at = ?2
         WHERE id = ?3 AND company_id = ?4",
    )
    .bind(status)
    .bind(now)
    .bind(id)
    .bind(company_id)
    .execute(pool)
    .await?;

    get(pool, id).await
}

// ─── Delete ────────────────────────────────────────────────────────────────

pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    let order =
        sqlx::query_as::<_, Order>("SELECT * FROM orders WHERE id = ?1 AND company_id = ?2")
            .bind(id)
            .bind(company_id)
            .fetch_optional(pool)
            .await?
            .ok_or(AppError::NotFound)?;

    if !["draft", "cancelled"].contains(&order.status.as_str()) {
        return Err(AppError::Validation(format!(
            "Comanda poate fi ștearsă doar în status 'draft' sau 'cancelled' (curent: '{}').",
            order.status
        )));
    }

    sqlx::query("DELETE FROM orders WHERE id = ?1 AND company_id = ?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;

    Ok(())
}

// ─── Available qty ─────────────────────────────────────────────────────────

/// Returns the total `qty_reserved` across all *accepted* orders for a given
/// product name within a company. Caller computes available = on_hand − reserved.
/// NOTE: never touches stock_ledger — qty_reserved is informational only.
pub async fn available_qty(
    pool: &SqlitePool,
    company_id: &str,
    product_name: &str,
) -> AppResult<String> {
    let total: Option<String> = sqlx::query_scalar(
        "SELECT CAST(COALESCE(SUM(CAST(ol.qty_reserved AS REAL)), 0) AS TEXT)
         FROM order_lines ol
         JOIN orders o ON o.id = ol.order_id
         WHERE o.company_id = ?1
           AND o.status = 'accepted'
           AND ol.name = ?2",
    )
    .bind(company_id)
    .bind(product_name)
    .fetch_one(pool)
    .await?;

    Ok(total.unwrap_or_else(|| "0".into()))
}

// ─── Convert to invoice ────────────────────────────────────────────────────

pub async fn convert_to_invoice(
    pool: &SqlitePool,
    company_id: &str,
    order_id: &str,
) -> AppResult<crate::db::invoices::Invoice> {
    // Atomic compare-and-set guard: claim the order for conversion before creating any invoice.
    // Prevents double fiscal number on crash/retry between invoice creation and status stamp.
    let rows_affected = sqlx::query(
        "UPDATE orders SET status = 'invoicing' \
         WHERE id = ?1 AND company_id = ?2 AND status = 'accepted' AND converted_invoice_id IS NULL",
    )
    .bind(order_id)
    .bind(company_id)
    .execute(pool)
    .await?
    .rows_affected();

    if rows_affected == 0 {
        let owl = get_with_lines(pool, order_id, company_id).await?;
        let order = &owl.order;
        if order.converted_invoice_id.is_some() || order.status == "invoiced" {
            return Err(AppError::Validation(
                "Comanda a fost deja convertită într-o factură.".into(),
            ));
        }
        return Err(AppError::Validation(format!(
            "Comanda trebuie să fie în status 'accepted' pentru conversie (curent: '{}').",
            order.status
        )));
    }

    let owl = get_with_lines(pool, order_id, company_id).await?;
    let order = &owl.order;

    let now = now_unix();
    let issue_date_str = chrono::DateTime::from_timestamp(now, 0)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| order.order_date.clone());

    let inv_lines: Vec<CreateLineInput> = owl
        .lines
        .iter()
        .map(|l| {
            let qty = l.quantity.parse::<f64>().unwrap_or(1.0);
            let price = l.unit_price.parse::<f64>().unwrap_or(0.0);
            let rate = l.vat_rate.parse::<f64>().unwrap_or(0.0);
            CreateLineInput {
                name: l.name.clone(),
                description: l.description.clone(),
                quantity: qty,
                unit: l.unit.clone().unwrap_or_default(),
                unit_price: price,
                vat_rate: rate,
                vat_category: l.vat_category.clone().unwrap_or_else(|| "S".into()),
                cpv_code: None,
                art331_code: None,
                revenue_kind: l.revenue_kind.clone(),
            }
        })
        .collect();

    let invoice_input = CreateInvoiceInput {
        company_id: company_id.to_string(),
        contact_id: order.contact_id.clone().unwrap_or_default(),
        series: "FACT".to_string(),
        issue_date: issue_date_str.clone(),
        due_date: issue_date_str,
        currency: Some(order.currency.clone()),
        exchange_rate: None,
        notes: order.notes.clone(),
        payment_means_code: None,
        lines: inv_lines,
    };

    let invoice = crate::db::invoices::create(pool, invoice_input).await?;

    sqlx::query(
        "UPDATE orders SET status = 'invoiced', converted_invoice_id = ?1, updated_at = ?2
         WHERE id = ?3 AND company_id = ?4",
    )
    .bind(&invoice.id)
    .bind(now)
    .bind(order_id)
    .bind(company_id)
    .execute(pool)
    .await?;

    Ok(invoice)
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    async fn setup_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory DB");

        sqlx::query(
            "CREATE TABLE companies (
                id TEXT PRIMARY KEY,
                legal_name TEXT NOT NULL,
                last_invoice_number INTEGER NOT NULL DEFAULT 0,
                last_quote_number INTEGER NOT NULL DEFAULT 0,
                last_order_number INTEGER NOT NULL DEFAULT 0
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE contacts (
                id TEXT PRIMARY KEY,
                company_id TEXT NOT NULL
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE invoices (
                id TEXT PRIMARY KEY,
                company_id TEXT NOT NULL,
                contact_id TEXT NOT NULL,
                series TEXT NOT NULL,
                number INTEGER NOT NULL,
                full_number TEXT NOT NULL,
                issue_date TEXT NOT NULL,
                due_date TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'RON',
                exchange_rate REAL,
                subtotal_amount TEXT NOT NULL,
                vat_amount TEXT NOT NULL,
                total_amount TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'DRAFT',
                anaf_upload_id TEXT,
                anaf_index TEXT,
                anaf_submitted_at INTEGER,
                anaf_validated_at INTEGER,
                anaf_rejected_at INTEGER,
                xml_path TEXT,
                pdf_path TEXT,
                signature_xml_path TEXT,
                rejection_reason TEXT,
                rejection_code TEXT,
                notes TEXT,
                payment_means_code TEXT NOT NULL DEFAULT '30',
                storno_of_invoice_id TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                UNIQUE(company_id, series, number)
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE invoice_line_items (
                id TEXT PRIMARY KEY,
                invoice_id TEXT NOT NULL,
                position INTEGER NOT NULL,
                name TEXT NOT NULL,
                description TEXT,
                quantity TEXT NOT NULL,
                unit TEXT NOT NULL DEFAULT '',
                unit_price TEXT NOT NULL,
                vat_rate TEXT NOT NULL,
                vat_category TEXT NOT NULL DEFAULT 'S',
                subtotal_amount TEXT NOT NULL,
                vat_amount TEXT NOT NULL,
                total_amount TEXT NOT NULL,
                cpv_code TEXT,
                art331_code TEXT,
                revenue_kind TEXT NOT NULL DEFAULT 'goods'
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE invoice_events (
                id TEXT PRIMARY KEY,
                invoice_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                message TEXT NOT NULL,
                metadata TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE orders (
                id TEXT PRIMARY KEY NOT NULL,
                company_id TEXT NOT NULL,
                contact_id TEXT,
                series TEXT,
                number INTEGER NOT NULL,
                full_number TEXT,
                order_date TEXT NOT NULL,
                expected_delivery TEXT,
                currency TEXT NOT NULL DEFAULT 'RON',
                exchange_rate TEXT,
                subtotal_amount TEXT NOT NULL,
                vat_amount TEXT NOT NULL,
                total_amount TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'draft',
                notes TEXT,
                converted_invoice_id TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                UNIQUE(company_id, series, number)
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE order_lines (
                id TEXT PRIMARY KEY NOT NULL,
                order_id TEXT NOT NULL,
                position INTEGER NOT NULL,
                name TEXT NOT NULL,
                description TEXT,
                quantity TEXT NOT NULL,
                unit TEXT,
                unit_price TEXT NOT NULL,
                vat_rate TEXT NOT NULL,
                vat_category TEXT,
                subtotal_amount TEXT NOT NULL,
                vat_amount TEXT NOT NULL,
                total_amount TEXT NOT NULL,
                revenue_kind TEXT,
                qty_reserved TEXT NOT NULL DEFAULT '0'
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO companies (id, legal_name, last_invoice_number, last_quote_number, last_order_number)
             VALUES ('co1', 'Test SRL', 0, 0, 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        pool
    }

    fn sample_line() -> CreateOrderLineInput {
        CreateOrderLineInput {
            name: "Produs test".into(),
            description: None,
            quantity: 5.0,
            unit: Some("buc".into()),
            unit_price: 50.0,
            vat_rate: 21.0,
            vat_category: "S".into(),
            revenue_kind: Some("goods".into()),
            qty_reserved: Some(5.0),
        }
    }

    fn sample_create_input() -> CreateOrderInput {
        CreateOrderInput {
            company_id: "co1".into(),
            contact_id: None,
            series: None,
            order_date: "2026-06-21".into(),
            expected_delivery: None,
            currency: None,
            exchange_rate: None,
            notes: None,
            lines: vec![sample_line()],
        }
    }

    #[tokio::test]
    async fn own_counter_does_not_touch_invoice_number() {
        let pool = setup_pool().await;

        create(&pool, sample_create_input()).await.unwrap();
        create(&pool, sample_create_input()).await.unwrap();

        let onum: i64 =
            sqlx::query_scalar("SELECT last_order_number FROM companies WHERE id='co1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        let inum: i64 =
            sqlx::query_scalar("SELECT last_invoice_number FROM companies WHERE id='co1'")
                .fetch_one(&pool)
                .await
                .unwrap();

        assert_eq!(onum, 2, "last_order_number should be 2 after 2 orders");
        assert_eq!(inum, 0, "last_invoice_number must stay 0");
    }

    #[tokio::test]
    async fn reservation_no_stock_ledger() {
        // Verify that available_qty works without a stock_ledger table (it doesn't use it).
        let pool = setup_pool().await;

        let order = create(&pool, sample_create_input()).await.unwrap();
        set_status(&pool, &order.id, "co1", "accepted")
            .await
            .unwrap();

        let reserved = available_qty(&pool, "co1", "Produs test").await.unwrap();
        let reserved_dec: Decimal = reserved.parse().unwrap();
        assert!(
            reserved_dec > Decimal::ZERO,
            "Should have reserved qty > 0 for accepted order"
        );
    }

    #[tokio::test]
    async fn cancel_releases_reservation() {
        let pool = setup_pool().await;

        let order = create(&pool, sample_create_input()).await.unwrap();
        set_status(&pool, &order.id, "co1", "accepted")
            .await
            .unwrap();

        // Before cancel: reserved.
        let before = available_qty(&pool, "co1", "Produs test").await.unwrap();
        let before_dec: Decimal = before.parse().unwrap();
        assert!(before_dec > Decimal::ZERO);

        // Cancel the order.
        set_status(&pool, &order.id, "co1", "cancelled")
            .await
            .unwrap();

        // After cancel: reservation count should be 0 (only accepted orders counted).
        let after = available_qty(&pool, "co1", "Produs test").await.unwrap();
        let after_dec: Decimal = after.parse().unwrap();
        assert_eq!(
            after_dec,
            Decimal::ZERO,
            "After cancel, reserved qty should be 0"
        );
    }
}
