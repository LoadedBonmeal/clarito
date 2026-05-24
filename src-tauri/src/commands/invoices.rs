use tauri::State;

use crate::db::invoices::{
    self, CreateInvoiceInput, Invoice, InvoiceFilter, InvoiceWithLines,
};
use crate::db::models::{new_id, InvoiceStatus, Paginated};
use crate::db::{companies, contacts};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::ubl::generator::{generate_ubl, GeneratorInput};
use crate::ubl::validator::validate_ubl;

#[tauri::command]
pub async fn list_invoices(
    state: State<'_, AppState>,
    filter: Option<InvoiceFilter>,
) -> AppResult<Paginated<Invoice>> {
    invoices::list(&state.db, filter.unwrap_or_default()).await
}

#[tauri::command]
pub async fn get_invoice(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<InvoiceWithLines> {
    invoices::get_with_lines(&state.db, &id).await
}

#[tauri::command]
pub async fn create_invoice_draft(
    state: State<'_, AppState>,
    input: CreateInvoiceInput,
) -> AppResult<Invoice> {
    invoices::create(&state.db, input).await
}

#[tauri::command]
pub async fn delete_invoice(state: State<'_, AppState>, id: String) -> AppResult<()> {
    invoices::delete(&state.db, &id).await
}

#[tauri::command]
pub async fn set_invoice_status(
    state: State<'_, AppState>,
    id: String,
    status: InvoiceStatus,
    message: Option<String>,
) -> AppResult<()> {
    invoices::set_status(&state.db, &id, status, message).await
}

#[tauri::command]
pub async fn update_invoice_draft(
    state: State<'_, AppState>,
    id: String,
    input: CreateInvoiceInput,
) -> AppResult<Invoice> {
    // 1. Verify invoice exists and is DRAFT
    let existing = invoices::get(&state.db, &id).await?;
    if existing.status != crate::db::models::InvoiceStatus::Draft.as_str() {
        return Err(AppError::Validation(format!(
            "Factura nu este schiță (status curent: {}). \
             Doar ciornele pot fi modificate.",
            existing.status
        )));
    }

    // 2. Calculate totals from new lines
    if input.lines.is_empty() {
        return Err(AppError::Validation(
            "Factura trebuie să aibă cel puțin o linie.".into(),
        ));
    }

    // Use rust_decimal for exact monetary math (plan rule: never f64 for money)
    use rust_decimal::Decimal;
    use rust_decimal::prelude::ToPrimitive;
    let hundred = Decimal::from(100u32);

    let mut subtotal_dec = Decimal::ZERO;
    let mut vat_total_dec = Decimal::ZERO;
    let line_rows: Vec<(String, f64, f64, f64)> = input
        .lines
        .iter()
        .map(|l| {
            let qty = Decimal::try_from(l.quantity).unwrap_or(Decimal::ZERO);
            let price = Decimal::try_from(l.unit_price).unwrap_or(Decimal::ZERO);
            let rate = Decimal::try_from(l.vat_rate).unwrap_or(Decimal::ZERO);
            let ls = (qty * price).round_dp(2);
            let lv = (ls * rate / hundred).round_dp(2);
            let lt = ls + lv;
            subtotal_dec += ls;
            vat_total_dec += lv;
            (
                new_id(),
                ls.to_f64().unwrap_or(0.0),
                lv.to_f64().unwrap_or(0.0),
                lt.to_f64().unwrap_or(0.0),
            )
        })
        .collect();
    let subtotal = subtotal_dec.to_f64().unwrap_or(0.0);
    let vat_total = vat_total_dec.to_f64().unwrap_or(0.0);
    let total = (subtotal_dec + vat_total_dec).to_f64().unwrap_or(0.0);
    let full_number = format!("{}-{:04}", input.series, input.number);

    let mut tx = state.db.begin().await?;

    // 3. Update invoice header
    sqlx::query(
        "UPDATE invoices SET
            contact_id          = ?2,
            series              = ?3,
            number              = ?4,
            full_number         = ?5,
            issue_date          = ?6,
            due_date            = ?7,
            currency            = ?8,
            notes               = ?9,
            subtotal_amount     = ?10,
            vat_amount          = ?11,
            total_amount        = ?12,
            payment_means_code  = ?13,
            updated_at          = unixepoch()
        WHERE id = ?1",
    )
    .bind(&id)
    .bind(&input.contact_id)
    .bind(&input.series)
    .bind(input.number)
    .bind(&full_number)
    .bind(&input.issue_date)
    .bind(&input.due_date)
    .bind(input.currency.as_deref().unwrap_or("RON"))
    .bind(&input.notes)
    .bind(subtotal)
    .bind(vat_total)
    .bind(total)
    .bind(input.payment_means_code.as_deref().unwrap_or("30"))
    .execute(&mut *tx)
    .await?;

    // 4. Delete existing line items
    sqlx::query("DELETE FROM invoice_line_items WHERE invoice_id = ?1")
        .bind(&id)
        .execute(&mut *tx)
        .await?;

    // 5. Insert new line items
    for (position, (line, (line_id, line_subtotal, line_vat, line_total))) in
        input.lines.iter().zip(line_rows.iter()).enumerate()
    {
        sqlx::query(
            "INSERT INTO invoice_line_items (
                id, invoice_id, position, name, description,
                quantity, unit, unit_price, vat_rate, vat_category,
                subtotal_amount, vat_amount, total_amount, cpv_code
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5,
                ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, ?14
            )",
        )
        .bind(line_id)
        .bind(&id)
        .bind((position as i64) + 1)
        .bind(&line.name)
        .bind(&line.description)
        .bind(line.quantity)
        .bind(&line.unit)
        .bind(line.unit_price)
        .bind(line.vat_rate)
        .bind(&line.vat_category)
        .bind(line_subtotal)
        .bind(line_vat)
        .bind(line_total)
        .bind(&line.cpv_code)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    // 6. Return updated invoice
    invoices::get(&state.db, &id).await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceDraftValidation {
    pub is_valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[tauri::command]
pub async fn validate_invoice_draft(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<InvoiceDraftValidation> {
    // 1. Load invoice + line_items
    let with_lines = invoices::get_with_lines(&state.db, &id).await?;
    let inv = with_lines.invoice;
    let lines = with_lines.lines;

    // 2. Load company (supplier)
    let seller = companies::get(&state.db, &inv.company_id).await?;

    // 3. Load contact (customer)
    let buyer = contacts::get(&state.db, &inv.contact_id).await?;

    // 4. Extract storno_ref from notes (format: "STORNO_OF:{full_number}|{reason}")
    let storno_ref: Option<String> = inv.notes.as_deref().and_then(|n| {
        n.strip_prefix("STORNO_OF:").map(|rest| {
            rest.split('|').next().unwrap_or("").to_string()
        })
    });

    // 5. Build generator input
    let input = GeneratorInput {
        invoice: inv,
        lines,
        seller,
        buyer,
        storno_ref,
    };

    let xml = match generate_ubl(&input) {
        Ok(xml) => xml,
        Err(e) => {
            return Ok(InvoiceDraftValidation {
                is_valid: false,
                errors: vec![e.to_string()],
                warnings: vec![],
            });
        }
    };

    // 6. Validate XML structure
    let xml_result = validate_ubl(&xml);

    // 7. Validate data-level business rules (50+ BR-RO rules, Decimal math)
    let (data_errors, data_warnings) = crate::ubl::validator::validate_invoice_data(
        &input.invoice,
        &input.lines,
        &input.seller,
        &input.buyer,
        input.storno_ref.as_deref(),
    );

    let all_errors: Vec<String> = xml_result.errors.into_iter().chain(data_errors).collect();
    let all_warnings: Vec<String> = xml_result.warnings.into_iter().chain(data_warnings).collect();

    Ok(InvoiceDraftValidation {
        is_valid: all_errors.is_empty(),
        errors: all_errors,
        warnings: all_warnings,
    })
}

/// Creează o factură de storno (credit note 381) care anulează factura originală.
///
/// - Copiază liniile facturii originale cu cantități negative.
/// - Setează InvoiceTypeCode = 381 și adaugă BillingReference la factura originală.
/// - Marchează factura originală ca STORNED.
#[tauri::command]
pub async fn storno_invoice(
    state: State<'_, AppState>,
    invoice_id: String,
    reason: String,
) -> AppResult<Invoice> {
    let pool = &state.db;

    // 1. Încarcă factura originală
    let original = invoices::get_with_lines(pool, &invoice_id).await?;
    let orig_inv = original.invoice;
    let orig_lines = original.lines;

    // Doar facturile VALIDATED pot fi stornate (DRAFT se șterge, nu se stornează)
    if orig_inv.status != crate::db::models::InvoiceStatus::Validated.as_str() {
        return Err(AppError::Validation(
            "Doar facturile validate de ANAF pot fi stornate. \
             Ciornele pot fi șterse direct."
                .into(),
        ));
    }

    // 2. Seria facturii storno (numărul e alocat atomic de invoices::create)
    let storno_series = format!("S{}", orig_inv.series);

    // 3. Creează liniile storno (cantități negative)
    let storno_lines: Vec<crate::db::invoices::CreateLineInput> = orig_lines
        .iter()
        .map(|l| crate::db::invoices::CreateLineInput {
            name: l.name.clone(),
            description: Some(format!("Storno: {}", l.name)),
            quantity: -l.quantity,
            unit: l.unit.clone(),
            unit_price: l.unit_price,
            vat_rate: l.vat_rate,
            vat_category: l.vat_category.clone(),
            cpv_code: l.cpv_code.clone(),
        })
        .collect();

    // 4. Creează factura storno
    // Nota: `number` este ignorat de invoices::create — numărul e alocat atomic acolo.
    let storno_input = crate::db::invoices::CreateInvoiceInput {
        company_id: orig_inv.company_id.clone(),
        contact_id: orig_inv.contact_id.clone(),
        series: storno_series,
        number: 0, // ignorat — alocat atomic în invoices::create
        issue_date: chrono::Utc::now().format("%Y-%m-%d").to_string(),
        due_date: chrono::Utc::now().format("%Y-%m-%d").to_string(),
        currency: Some(orig_inv.currency.clone()),
        exchange_rate: orig_inv.exchange_rate,
        notes: Some(format!("Storno pentru factura {}. Motiv: {}", orig_inv.full_number, reason)),
        payment_means_code: Some(orig_inv.payment_means_code.clone()),
        lines: storno_lines,
    };
    // Pregătim notes înainte de move (pentru referința STORNO_OF)
    let storno_notes = format!(
        "STORNO_OF:{}|{}",
        orig_inv.full_number,
        storno_input.notes.as_deref().unwrap_or("")
    );

    let storno_inv = invoices::create(pool, storno_input).await?;

    // 5. Marchează factura originală ca STORNED
    invoices::set_status(
        pool,
        &invoice_id,
        crate::db::models::InvoiceStatus::Storned,
        Some(format!("Stornată prin {}. Motiv: {}", storno_inv.full_number, reason)),
    )
    .await?;

    // 6. Adaugă referința storno în events și stochează legătura la originală
    let _ = sqlx::query(
        "INSERT INTO invoice_events (id, invoice_id, event_type, message, created_at) \
         VALUES (?1, ?2, 'STORNO_CREATED', ?3, unixepoch())",
    )
    .bind(new_id())
    .bind(&storno_inv.id)
    .bind(format!("Factură storno pentru {}. Motiv: {}", orig_inv.full_number, reason))
    .execute(pool)
    .await;

    // Salvăm numărul facturii originale în notes (prefixat) pentru a fi folosit
    // la generarea XML-ului UBL — BillingReference → storno_ref.
    // Format recunoscut de anaf_submit_invoice: "STORNO_OF:{full_number}|..."
    sqlx::query("UPDATE invoices SET notes = ?2, updated_at = unixepoch() WHERE id = ?1")
        .bind(&storno_inv.id)
        .bind(&storno_notes)
        .execute(pool)
        .await?;

    Ok(storno_inv)
}
