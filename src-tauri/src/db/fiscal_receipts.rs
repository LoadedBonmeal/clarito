//! Bonuri fiscale / Raport Z (casa de marcat) — CRUD + validatori.
//!
//! Postarea GL se face în `db::gl::post_fiscal_receipt` (VAT-tagged, idempotentă).
//! Descărcarea de gestiune (K) este DELEGATĂ motorului de inventar lunar — acest modul
//! înregistrează DOAR veniturile + TVA + trezoreria.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::str::FromStr;

use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

// ─── Modele ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FiscalReceipt {
    pub id: String,
    pub company_id: String,
    pub serie_casa: String,
    pub nr_z: i64,
    pub report_date: String,
    pub nr_bonuri: i64,
    pub total: String,
    pub numerar: String,
    pub card: String,
    pub tichete: String,
    pub status: String,
    pub retail_method: i64,
    pub notes: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FiscalReceiptVatLine {
    pub id: String,
    pub receipt_id: String,
    pub vat_category: String,
    pub rate: String,
    pub baza: String,
    pub tva: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FiscalReceiptInvoiceLink {
    pub id: String,
    pub receipt_id: String,
    pub invoice_id: String,
    pub amount: String,
    pub pay_means: String,
}

// ─── Intrări ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FiscalReceiptInput {
    pub serie_casa: String,
    pub nr_z: i64,
    pub report_date: String,
    pub nr_bonuri: Option<i64>,
    pub total: String,
    pub numerar: String,
    pub card: String,
    pub tichete: Option<String>,
    pub retail_method: Option<i64>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VatLineInput {
    pub vat_category: Option<String>,
    pub rate: String,
    pub baza: String,
    pub tva: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceLinkInput {
    pub invoice_id: String,
    pub amount: String,
    pub pay_means: String,
}

// ─── Structuri de răspuns detaliat ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FiscalReceiptDetail {
    pub receipt: FiscalReceipt,
    pub vat_lines: Vec<FiscalReceiptVatLine>,
    pub invoice_links: Vec<FiscalReceiptInvoiceLink>,
}

// ─── Validatori ──────────────────────────────────────────────────────────────

fn dec(s: &str) -> Decimal {
    Decimal::from_str(s.trim()).unwrap_or(Decimal::ZERO)
}

/// Verifică că numerar+card+tichete == total (toleranță 0.005 RON).
pub fn validate_payment_split(
    total: &str,
    numerar: &str,
    card: &str,
    tichete: &str,
) -> AppResult<()> {
    let t = dec(total);
    let split = dec(numerar) + dec(card) + dec(tichete);
    if (t - split).abs() > Decimal::new(5, 3) {
        return Err(AppError::Validation(format!(
            "numerar+card+tichete ({split:.2}) ≠ total ({t:.2})"
        )));
    }
    Ok(())
}

/// Verifică că Σ(baza+tva) pe toate liniile TVA == total (toleranță 0.005 RON).
pub fn validate_vat_lines_total(total: &str, lines: &[VatLineInput]) -> AppResult<()> {
    let t = dec(total);
    let sum: Decimal = lines.iter().map(|l| dec(&l.baza) + dec(&l.tva)).sum();
    if (t - sum).abs() > Decimal::new(5, 3) {
        return Err(AppError::Validation(format!(
            "Σ(baza+tva) pe liniile TVA ({sum:.2}) ≠ total Z ({t:.2})"
        )));
    }
    Ok(())
}

/// Verifică că pay_means ∈ {CASH, CARD}.
pub fn validate_pay_means(pay_means: &str) -> AppResult<()> {
    match pay_means {
        "CASH" | "CARD" => Ok(()),
        other => Err(AppError::Validation(format!(
            "pay_means invalid: '{other}' — se acceptă CASH sau CARD"
        ))),
    }
}

// ─── CRUD Receipt ─────────────────────────────────────────────────────────────

/// Crează un bon fiscal (DRAFT).
pub async fn create_receipt(
    pool: &SqlitePool,
    company_id: &str,
    input: FiscalReceiptInput,
) -> AppResult<FiscalReceipt> {
    let tichete = input.tichete.as_deref().unwrap_or("0.00");
    validate_payment_split(&input.total, &input.numerar, &input.card, tichete)?;

    let id = new_id();
    let now = now_unix();

    sqlx::query(
        "INSERT INTO fiscal_receipts \
         (id, company_id, serie_casa, nr_z, report_date, nr_bonuri, \
          total, numerar, card, tichete, status, retail_method, notes, created_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'DRAFT',?11,?12,?13)",
    )
    .bind(&id)
    .bind(company_id)
    .bind(&input.serie_casa)
    .bind(input.nr_z)
    .bind(&input.report_date)
    .bind(input.nr_bonuri.unwrap_or(0))
    .bind(&input.total)
    .bind(&input.numerar)
    .bind(&input.card)
    .bind(tichete)
    .bind(input.retail_method.unwrap_or(0))
    .bind(&input.notes)
    .bind(now)
    .execute(pool)
    .await?;

    get_receipt(pool, &id, company_id).await
}

/// Listează bonurile fiscale pentru o companie (opțional filtrate pe perioadă).
pub async fn list_receipts(
    pool: &SqlitePool,
    company_id: &str,
    date_from: Option<&str>,
    date_to: Option<&str>,
) -> AppResult<Vec<FiscalReceipt>> {
    let rows = sqlx::query(
        "SELECT id, company_id, serie_casa, nr_z, report_date, nr_bonuri, \
                total, numerar, card, tichete, status, retail_method, notes, created_at \
         FROM fiscal_receipts \
         WHERE company_id = ?1 \
           AND (?2 IS NULL OR report_date >= ?2) \
           AND (?3 IS NULL OR report_date <= ?3) \
         ORDER BY report_date DESC, serie_casa, nr_z",
    )
    .bind(company_id)
    .bind(date_from)
    .bind(date_to)
    .fetch_all(pool)
    .await?;

    Ok(rows.iter().map(row_to_receipt).collect())
}

/// Preia un bon fiscal by id (scoped la company).
pub async fn get_receipt(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
) -> AppResult<FiscalReceipt> {
    let row = sqlx::query(
        "SELECT id, company_id, serie_casa, nr_z, report_date, nr_bonuri, \
                total, numerar, card, tichete, status, retail_method, notes, created_at \
         FROM fiscal_receipts WHERE id = ?1 AND company_id = ?2",
    )
    .bind(id)
    .bind(company_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok(row_to_receipt(&row))
}

/// Preia bonul cu liniile VAT și legăturile de facturi.
pub async fn get_receipt_detail(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
) -> AppResult<FiscalReceiptDetail> {
    let receipt = get_receipt(pool, id, company_id).await?;
    let vat_lines = list_vat_lines(pool, id).await?;
    let invoice_links = list_invoice_links(pool, id).await?;
    Ok(FiscalReceiptDetail {
        receipt,
        vat_lines,
        invoice_links,
    })
}

/// Actualizează câmpurile unui bon DRAFT.
pub async fn update_receipt(
    pool: &SqlitePool,
    id: &str,
    company_id: &str,
    input: FiscalReceiptInput,
) -> AppResult<FiscalReceipt> {
    let receipt = get_receipt(pool, id, company_id).await?;
    if receipt.status != "DRAFT" {
        return Err(AppError::Validation(
            "Bonul nu mai este în stare DRAFT — nu poate fi modificat.".to_string(),
        ));
    }
    let tichete = input.tichete.as_deref().unwrap_or("0.00");
    validate_payment_split(&input.total, &input.numerar, &input.card, tichete)?;

    sqlx::query(
        "UPDATE fiscal_receipts SET \
         serie_casa=?3, nr_z=?4, report_date=?5, nr_bonuri=?6, \
         total=?7, numerar=?8, card=?9, tichete=?10, \
         retail_method=?11, notes=?12 \
         WHERE id=?1 AND company_id=?2",
    )
    .bind(id)
    .bind(company_id)
    .bind(&input.serie_casa)
    .bind(input.nr_z)
    .bind(&input.report_date)
    .bind(input.nr_bonuri.unwrap_or(0))
    .bind(&input.total)
    .bind(&input.numerar)
    .bind(&input.card)
    .bind(tichete)
    .bind(input.retail_method.unwrap_or(0))
    .bind(&input.notes)
    .execute(pool)
    .await?;

    get_receipt(pool, id, company_id).await
}

/// Șterge un bon DRAFT (nu se poate șterge un bon POSTED/STORNAT).
pub async fn delete_receipt(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    let receipt = get_receipt(pool, id, company_id).await?;
    if receipt.status == "POSTED" {
        return Err(AppError::Validation(
            "Bonul este POSTED — stornați-l înainte de ștergere.".to_string(),
        ));
    }
    sqlx::query("DELETE FROM fiscal_receipts WHERE id=?1 AND company_id=?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    Ok(())
}

// ─── VAT lines CRUD ──────────────────────────────────────────────────────────

/// Listează liniile TVA ale unui bon.
pub async fn list_vat_lines(
    pool: &SqlitePool,
    receipt_id: &str,
) -> AppResult<Vec<FiscalReceiptVatLine>> {
    let rows = sqlx::query(
        "SELECT id, receipt_id, vat_category, rate, baza, tva \
         FROM fiscal_receipt_vat_lines WHERE receipt_id = ?1 \
         ORDER BY CAST(rate AS REAL) DESC",
    )
    .bind(receipt_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| FiscalReceiptVatLine {
            id: r.try_get("id").unwrap_or_default(),
            receipt_id: r.try_get("receipt_id").unwrap_or_default(),
            vat_category: r.try_get("vat_category").unwrap_or_default(),
            rate: r.try_get("rate").unwrap_or_default(),
            baza: r.try_get("baza").unwrap_or_default(),
            tva: r.try_get("tva").unwrap_or_default(),
        })
        .collect())
}

/// Înlocuiește TOATE liniile TVA ale unui bon DRAFT (upsert atomizat).
pub async fn replace_vat_lines(
    pool: &SqlitePool,
    receipt_id: &str,
    company_id: &str,
    lines: Vec<VatLineInput>,
) -> AppResult<Vec<FiscalReceiptVatLine>> {
    let receipt = get_receipt(pool, receipt_id, company_id).await?;
    if receipt.status != "DRAFT" {
        return Err(AppError::Validation(
            "Liniile TVA pot fi modificate doar pe bonuri DRAFT.".to_string(),
        ));
    }

    // Validare cote active
    validate_rates_active(pool, &lines).await?;

    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM fiscal_receipt_vat_lines WHERE receipt_id = ?1")
        .bind(receipt_id)
        .execute(&mut *tx)
        .await?;

    for l in &lines {
        let category = l.vat_category.as_deref().unwrap_or("S");
        sqlx::query(
            "INSERT INTO fiscal_receipt_vat_lines (id, receipt_id, vat_category, rate, baza, tva) \
             VALUES (?1,?2,?3,?4,?5,?6)",
        )
        .bind(new_id())
        .bind(receipt_id)
        .bind(category)
        .bind(&l.rate)
        .bind(&l.baza)
        .bind(&l.tva)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    list_vat_lines(pool, receipt_id).await
}

// ─── Invoice links CRUD ───────────────────────────────────────────────────────

/// Listează legăturile bon–factură.
pub async fn list_invoice_links(
    pool: &SqlitePool,
    receipt_id: &str,
) -> AppResult<Vec<FiscalReceiptInvoiceLink>> {
    let rows = sqlx::query(
        "SELECT id, receipt_id, invoice_id, amount, pay_means \
         FROM fiscal_receipt_invoice_links WHERE receipt_id = ?1",
    )
    .bind(receipt_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|r| FiscalReceiptInvoiceLink {
            id: r.try_get("id").unwrap_or_default(),
            receipt_id: r.try_get("receipt_id").unwrap_or_default(),
            invoice_id: r.try_get("invoice_id").unwrap_or_default(),
            amount: r.try_get("amount").unwrap_or_default(),
            pay_means: r.try_get("pay_means").unwrap_or_default(),
        })
        .collect())
}

/// Adaugă o legătură bon–factură (bon trebuie să fie DRAFT).
pub async fn add_invoice_link(
    pool: &SqlitePool,
    receipt_id: &str,
    company_id: &str,
    input: InvoiceLinkInput,
) -> AppResult<FiscalReceiptInvoiceLink> {
    let receipt = get_receipt(pool, receipt_id, company_id).await?;
    if receipt.status != "DRAFT" {
        return Err(AppError::Validation(
            "Legăturile pot fi adăugate doar pe bonuri DRAFT.".to_string(),
        ));
    }
    validate_pay_means(&input.pay_means)?;

    let id = new_id();
    sqlx::query(
        "INSERT INTO fiscal_receipt_invoice_links \
         (id, receipt_id, invoice_id, amount, pay_means) \
         VALUES (?1,?2,?3,?4,?5)",
    )
    .bind(&id)
    .bind(receipt_id)
    .bind(&input.invoice_id)
    .bind(&input.amount)
    .bind(&input.pay_means)
    .execute(pool)
    .await?;

    let row = sqlx::query(
        "SELECT id, receipt_id, invoice_id, amount, pay_means \
         FROM fiscal_receipt_invoice_links WHERE id = ?1",
    )
    .bind(&id)
    .fetch_one(pool)
    .await?;

    Ok(FiscalReceiptInvoiceLink {
        id: row.try_get("id").unwrap_or_default(),
        receipt_id: row.try_get("receipt_id").unwrap_or_default(),
        invoice_id: row.try_get("invoice_id").unwrap_or_default(),
        amount: row.try_get("amount").unwrap_or_default(),
        pay_means: row.try_get("pay_means").unwrap_or_default(),
    })
}

/// Elimină o legătură bon–factură (bon trebuie să fie DRAFT).
pub async fn remove_invoice_link(
    pool: &SqlitePool,
    link_id: &str,
    receipt_id: &str,
    company_id: &str,
) -> AppResult<()> {
    let receipt = get_receipt(pool, receipt_id, company_id).await?;
    if receipt.status != "DRAFT" {
        return Err(AppError::Validation(
            "Legăturile pot fi eliminate doar pe bonuri DRAFT.".to_string(),
        ));
    }
    sqlx::query("DELETE FROM fiscal_receipt_invoice_links WHERE id = ?1 AND receipt_id = ?2")
        .bind(link_id)
        .bind(receipt_id)
        .execute(pool)
        .await?;
    Ok(())
}

// ─── Validare cote active ─────────────────────────────────────────────────────

async fn validate_rates_active(pool: &SqlitePool, lines: &[VatLineInput]) -> AppResult<()> {
    let active_rates: std::collections::HashSet<String> =
        sqlx::query("SELECT rate FROM vat_rates WHERE active = 1")
            .fetch_all(pool)
            .await?
            .iter()
            .filter_map(|r| r.try_get::<String, _>("rate").ok())
            .collect();

    for l in lines {
        // Normalize: "21.00" → "21", "0" → "0" etc.
        let rate_norm = normalize_rate(&l.rate);
        if !active_rates.iter().any(|r| normalize_rate(r) == rate_norm) {
            return Err(AppError::Validation(format!(
                "Cota TVA {}% nu este activă în catalogul de cote.",
                l.rate
            )));
        }
    }
    Ok(())
}

fn normalize_rate(r: &str) -> String {
    match Decimal::from_str(r.trim()) {
        Ok(d) => {
            // Normalize: remove trailing zeros (e.g. "21.00" → "21")
            let s = format!("{}", d.normalize());
            s
        }
        Err(_) => r.trim().to_string(),
    }
}

// ─── Row mapper ───────────────────────────────────────────────────────────────

fn row_to_receipt(r: &sqlx::sqlite::SqliteRow) -> FiscalReceipt {
    FiscalReceipt {
        id: r.try_get("id").unwrap_or_default(),
        company_id: r.try_get("company_id").unwrap_or_default(),
        serie_casa: r.try_get("serie_casa").unwrap_or_default(),
        nr_z: r.try_get("nr_z").unwrap_or(0),
        report_date: r.try_get("report_date").unwrap_or_default(),
        nr_bonuri: r.try_get("nr_bonuri").unwrap_or(0),
        total: r.try_get("total").unwrap_or_default(),
        numerar: r.try_get("numerar").unwrap_or_default(),
        card: r.try_get("card").unwrap_or_default(),
        tichete: r.try_get("tichete").unwrap_or_default(),
        status: r.try_get("status").unwrap_or_default(),
        retail_method: r.try_get("retail_method").unwrap_or(0),
        notes: r.try_get("notes").unwrap_or(None),
        created_at: r.try_get("created_at").unwrap_or(0),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fiscal_receipt_validate_payment_split_ok() {
        assert!(validate_payment_split("1000.00", "700.00", "300.00", "0.00").is_ok());
    }

    #[test]
    fn fiscal_receipt_validate_payment_split_mismatch() {
        assert!(validate_payment_split("1000.00", "600.00", "300.00", "0.00").is_err());
    }

    #[test]
    fn fiscal_receipt_validate_payment_split_with_tichete() {
        assert!(validate_payment_split("1000.00", "500.00", "300.00", "200.00").is_ok());
    }

    #[test]
    fn fiscal_receipt_validate_vat_lines_total_ok() {
        let lines = vec![VatLineInput {
            vat_category: Some("S".to_string()),
            rate: "21".to_string(),
            baza: "826.45".to_string(),
            tva: "173.55".to_string(),
        }];
        assert!(validate_vat_lines_total("1000.00", &lines).is_ok());
    }

    #[test]
    fn fiscal_receipt_validate_vat_lines_total_mismatch() {
        let lines = vec![VatLineInput {
            vat_category: Some("S".to_string()),
            rate: "21".to_string(),
            baza: "826.45".to_string(),
            tva: "100.00".to_string(), // wrong
        }];
        assert!(validate_vat_lines_total("1000.00", &lines).is_err());
    }

    #[test]
    fn fiscal_receipt_validate_pay_means_valid() {
        assert!(validate_pay_means("CASH").is_ok());
        assert!(validate_pay_means("CARD").is_ok());
    }

    #[test]
    fn fiscal_receipt_validate_pay_means_invalid() {
        assert!(validate_pay_means("TRANSFER").is_err());
        assert!(validate_pay_means("cash").is_err());
    }
}
