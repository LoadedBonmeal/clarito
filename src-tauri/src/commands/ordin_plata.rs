//! Ordin de Plată (OP) — Regulament BNR 2/2016, art. 3.
//!
//! Assembles the data required to print an OP from a received-invoice payment.
//! This is a READ-ONLY document command: no GL, no mutations.
//!
//! Mandatory elements (BNR 2/2016 art. 3):
//!   • Plătitor  — denumire, IBAN, CUI/cod fiscal (= the company's own data)
//!   • Bancă plătitoare — from the company profile (bank_name)
//!   • Beneficiar — denumire, CUI, IBAN (= the supplier / contact)
//!   • Suma + moneda (cifre + litere pentru RON)
//!   • Data emiterii (ISO date)
//!   • Referință / explicații (invoice reference / notes from the payment)
//!   • Nr. OP (sequential within the document; we use the payment id short-hash)
//!
//! The frontend assembles the printable HTML from this data struct (same pattern
//! as the chitanță / registru de casă print flow).

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::ubl::pdf::amount_to_romanian_words;

// ─── Output type ─────────────────────────────────────────────────────────────

/// All data needed to render a printable Ordin de Plată document.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrdinPlataData {
    /// Number shown on the OP (short hash of the payment id — unique, stable).
    pub op_number: String,
    /// Issue date from the received-payment row (YYYY-MM-DD).
    pub issue_date: String,

    // ── Plătitor (our company) ──────────────────────────────────────────────
    pub platitor_name: String,
    pub platitor_cui: String,
    /// Company IBAN (from the companies table). May be empty if not configured.
    pub platitor_iban: String,
    /// Company bank name.
    pub platitor_banca: String,

    // ── Beneficiar (supplier / contact) ────────────────────────────────────
    pub beneficiar_name: String,
    /// Supplier's CUI / fiscal code. Empty when not on file.
    pub beneficiar_cui: String,
    /// Supplier IBAN from the contact record. Empty when not on file.
    pub beneficiar_iban: String,
    /// Supplier bank name from the contact record.
    pub beneficiar_banca: String,

    // ── Suma ────────────────────────────────────────────────────────────────
    pub amount: String,
    pub currency: String,
    /// Romanian words for the amount (populated only when currency == "RON").
    pub amount_words: String,

    // ── Referință ───────────────────────────────────────────────────────────
    /// E.g. "Plată factură FURN-2026-00042" assembled from the received-invoice number.
    pub reference: String,
    /// Additional notes from the payment record.
    pub notes: String,
}

// ─── Command ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrdinPlataArgs {
    /// The `received_invoice_payments.id` (a single payment row).
    pub payment_id: String,
    pub company_id: String,
}

#[tauri::command]
pub async fn get_ordin_plata_data(
    state: State<'_, AppState>,
    args: OrdinPlataArgs,
) -> AppResult<OrdinPlataData> {
    ordin_plata_data(&state.db, &args.payment_id, &args.company_id).await
}

// ─── DB assembly (also exposed for tests) ────────────────────────────────────

pub(crate) async fn ordin_plata_data(
    pool: &sqlx::SqlitePool,
    payment_id: &str,
    company_id: &str,
) -> AppResult<OrdinPlataData> {
    use rust_decimal::Decimal;
    use std::str::FromStr;

    // ── 1. Payment row ──────────────────────────────────────────────────────
    let pay_row = sqlx::query(
        "SELECT rp.id, rp.amount, rp.currency, rp.paid_at, rp.reference, rp.notes, \
                rp.received_invoice_id, \
                ri.issuer_cui, ri.issuer_name, \
                COALESCE(ri.series,'') AS inv_series, \
                COALESCE(ri.number,'')  AS inv_number \
         FROM received_invoice_payments rp \
         JOIN received_invoices ri ON ri.id = rp.received_invoice_id \
         WHERE rp.id = ?1 AND rp.company_id = ?2",
    )
    .bind(payment_id)
    .bind(company_id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::Database)?
    .ok_or(AppError::NotFound)?;

    use sqlx::Row;
    let amount_str: String = pay_row.try_get("amount").unwrap_or_default();
    let currency: String = pay_row.try_get("currency").unwrap_or_else(|_| "RON".into());
    let paid_at: String = pay_row.try_get("paid_at").unwrap_or_default();
    let reference: Option<String> = pay_row.try_get("reference").unwrap_or(None);
    let notes: Option<String> = pay_row.try_get("notes").unwrap_or(None);
    let issuer_cui: String = pay_row.try_get("issuer_cui").unwrap_or_default();
    let issuer_name: String = pay_row.try_get("issuer_name").unwrap_or_default();
    let inv_series: String = pay_row.try_get("inv_series").unwrap_or_default();
    let inv_number: String = pay_row.try_get("inv_number").unwrap_or_default();

    // ── 2. Company (plătitor) ────────────────────────────────────────────────
    let co_row = sqlx::query(
        "SELECT legal_name, cui, \
                COALESCE(iban,'')      AS iban, \
                COALESCE(bank_name,'') AS bank_name \
         FROM companies WHERE id = ?1 AND is_active = 1",
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await
    .map_err(AppError::Database)?
    .ok_or(AppError::NotFound)?;

    let platitor_name: String = co_row.try_get("legal_name").unwrap_or_default();
    let platitor_cui: String = co_row.try_get("cui").unwrap_or_default();
    let platitor_iban: String = co_row.try_get("iban").unwrap_or_default();
    let platitor_banca: String = co_row.try_get("bank_name").unwrap_or_default();

    // ── 3. Contact (beneficiar) — look up by issuer_cui in contacts ──────────
    // A supplier may be stored as a contact in this company's address book.
    let contact_row = sqlx::query(
        "SELECT COALESCE(iban,'')      AS iban, \
                COALESCE(bank_name,'') AS bank_name \
         FROM contacts \
         WHERE company_id = ?1 AND TRIM(cui) = TRIM(?2) \
         LIMIT 1",
    )
    .bind(company_id)
    .bind(&issuer_cui)
    .fetch_optional(pool)
    .await
    .map_err(AppError::Database)?;

    let (beneficiar_iban, beneficiar_banca) = match contact_row {
        Some(row) => (
            row.try_get::<String, _>("iban").unwrap_or_default(),
            row.try_get::<String, _>("bank_name").unwrap_or_default(),
        ),
        None => (String::new(), String::new()),
    };

    // ── 4. Derived fields ────────────────────────────────────────────────────
    // OP number: first 8 hex chars of the payment UUID (stable, unique, readable).
    let op_number = payment_id
        .replace('-', "")
        .chars()
        .take(8)
        .collect::<String>()
        .to_uppercase();

    // Build reference from the invoice number or use the stored payment reference.
    let invoice_ref = if !inv_series.is_empty() || !inv_number.is_empty() {
        format!("{} {}", inv_series.trim(), inv_number.trim())
            .trim()
            .to_string()
    } else {
        String::new()
    };
    let reference_str = reference
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| {
            if invoice_ref.is_empty() {
                String::new()
            } else {
                format!("Plată factură {invoice_ref}")
            }
        });

    // Amount in words for RON only.
    let amount_dec = Decimal::from_str(amount_str.trim()).unwrap_or(Decimal::ZERO);
    let amount_words = if currency.trim().eq_ignore_ascii_case("RON") {
        amount_to_romanian_words(amount_dec)
    } else {
        String::new()
    };

    Ok(OrdinPlataData {
        op_number,
        issue_date: paid_at.chars().take(10).collect(),
        platitor_name,
        platitor_cui,
        platitor_iban,
        platitor_banca,
        beneficiar_name: issuer_name,
        beneficiar_cui: issuer_cui,
        beneficiar_iban,
        beneficiar_banca,
        amount: amount_str,
        currency,
        amount_words,
        reference: reference_str,
        notes: notes.unwrap_or_default(),
    })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    async fn pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    async fn seed(pool: &SqlitePool) {
        // Company (plătitor)
        sqlx::query(
            "INSERT INTO companies \
             (id, cui, legal_name, address, city, county, country, iban, bank_name) \
             VALUES ('co','RO12345678','Test SRL','Str. Test 1','București','B','RO', \
                     'RO49AAAA1B31007593840000','Banca Test SA')",
        )
        .execute(pool)
        .await
        .unwrap();

        // Received invoice (from supplier RO99111222)
        sqlx::query(
            "INSERT INTO received_invoices \
             (id, company_id, anaf_download_id, issuer_cui, issuer_name, total_amount, \
              currency, issue_date, xml_path, status, series, number) \
             VALUES ('ri1','co','dl1','RO99111222','Furnizor SRL','5000', \
                     'RON','2026-03-10','/x.xml','APPROVED','FURN','42')",
        )
        .execute(pool)
        .await
        .unwrap();

        // Payment against the received invoice
        sqlx::query(
            "INSERT INTO received_invoice_payments \
             (id, received_invoice_id, company_id, amount, currency, paid_at, method, \
              reference, notes, exchange_rate) \
             VALUES ('pay1','ri1','co','5000','RON','2026-04-15','transfer', \
                     NULL, NULL, NULL)",
        )
        .execute(pool)
        .await
        .unwrap();

        // Supplier contact with IBAN
        sqlx::query(
            "INSERT INTO contacts \
             (id, company_id, contact_type, cui, legal_name, vat_payer, is_individual, \
              cash_vat, country, iban, bank_name) \
             VALUES ('c1','co','SUPPLIER','RO99111222','Furnizor SRL', \
                     0, 0, 0, 'RO', 'RO49BBBB1B31007593840001', 'Bancă Furnizor SA')",
        )
        .execute(pool)
        .await
        .unwrap();
    }

    // ── Test 1: correct payer / payee assembly ─────────────────────────────
    #[tokio::test]
    async fn ordin_plata_assembles_payer_payee() {
        let pool = pool().await;
        seed(&pool).await;

        let data = ordin_plata_data(&pool, "pay1", "co").await.unwrap();

        // Payer = company
        assert_eq!(data.platitor_name, "Test SRL");
        assert_eq!(data.platitor_cui, "RO12345678");
        assert_eq!(data.platitor_iban, "RO49AAAA1B31007593840000");
        assert_eq!(data.platitor_banca, "Banca Test SA");

        // Payee = supplier (from received invoice + contact IBAN)
        assert_eq!(data.beneficiar_name, "Furnizor SRL");
        assert_eq!(data.beneficiar_cui, "RO99111222");
        assert_eq!(data.beneficiar_iban, "RO49BBBB1B31007593840001");
        assert_eq!(data.beneficiar_banca, "Bancă Furnizor SA");

        // Amount + currency
        assert_eq!(data.amount, "5000");
        assert_eq!(data.currency, "RON");

        // Reference auto-generated from invoice number
        assert_eq!(data.reference, "Plată factură FURN 42");
    }

    // ── Test 2: notFound for wrong company ─────────────────────────────────
    #[tokio::test]
    async fn ordin_plata_wrong_company_returns_not_found() {
        let pool = pool().await;
        seed(&pool).await;

        let result = ordin_plata_data(&pool, "pay1", "other").await;
        assert!(result.is_err());
    }

    // ── Test 3: amount_to_romanian_words spot-checks ───────────────────────
    #[test]
    fn amount_words_ron() {
        use rust_decimal::Decimal;
        use std::str::FromStr;

        let cases: &[(&str, &str)] = &[
            (
                "1234.56",
                "O mie două sute treizeci și patru lei și 56 bani",
            ),
            ("100.00", "O sută lei"),
            ("0.50", "zero lei și 50 bani"),
            ("1000000.01", "Un milion lei și 1 bani"),
        ];
        for (input, expected) in cases {
            let dec = Decimal::from_str(input).unwrap();
            let words = amount_to_romanian_words(dec);
            assert_eq!(
                words, *expected,
                "amount_to_romanian_words({input}) = {words:?}, expected {expected:?}"
            );
        }
    }
}
