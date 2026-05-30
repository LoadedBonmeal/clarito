use sqlx::SqlitePool;
use tauri::State;

use crate::db::invoices::{self, CreateInvoiceInput, Invoice, InvoiceFilter, InvoiceWithLines};
use crate::db::models::{new_id, InvoiceStatus, Paginated, VALID_VAT_RATES};
use crate::db::{companies, contacts};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::ubl::generator::{generate_ubl, GeneratorInput};
use crate::ubl::validator::validate_ubl;

/// BIZ-13: Determină `full_number`-ul facturii originale referențiate de un
/// storno. Sursa autoritativă este FK-ul `storno_of_invoice_id`. Dacă acesta
/// lipsește (facturi anterioare migrației 0008), recurgem la parserul
/// moștenit al notei `STORNO_OF:{full_number}|{motiv}`.
pub(crate) async fn resolve_storno_ref(
    pool: &SqlitePool,
    inv: &Invoice,
) -> AppResult<Option<String>> {
    if let Some(orig_id) = inv.storno_of_invoice_id.as_deref() {
        match invoices::get(pool, orig_id).await {
            Ok(original) => return Ok(Some(original.full_number)),
            // Dacă FK-ul a rămas dangling dintr-un motiv neașteptat, ne
            // întoarcem la legacy parser în loc să eșuăm întreaga validare.
            Err(AppError::NotFound) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(inv.notes.as_deref().and_then(|n| {
        n.strip_prefix("STORNO_OF:")
            .map(|rest| rest.split('|').next().unwrap_or("").to_string())
    }))
}

#[tauri::command]
pub async fn list_invoices(
    state: State<'_, AppState>,
    filter: Option<InvoiceFilter>,
) -> AppResult<Paginated<Invoice>> {
    invoices::list(&state.db, filter.unwrap_or_default()).await
}

#[tauri::command]
pub async fn get_invoice(state: State<'_, AppState>, id: String) -> AppResult<InvoiceWithLines> {
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

/// RUST-02: Verifică dacă tranziția de status este permisă din punct de
/// vedere al ciclului de viață al unei facturi. Reguli:
/// - DRAFT → QUEUED
/// - QUEUED → SUBMITTED | VALIDATED | REJECTED
/// - SUBMITTED → VALIDATED | REJECTED
/// - VALIDATED → STORNED
/// - REJECTED / STORNED → (terminale, nicio tranziție)
///
/// Returnează `true` pentru no-op (acelaşi status) — UI-ul poate emite
/// idempotent același status fără să primească o eroare.
fn is_allowed_status_transition(current: &str, target: &str) -> bool {
    if current == target {
        return true;
    }
    match target {
        "QUEUED" => current == "DRAFT",
        "SUBMITTED" => current == "QUEUED",
        "VALIDATED" => matches!(current, "SUBMITTED" | "QUEUED"),
        "REJECTED" => matches!(current, "SUBMITTED" | "QUEUED"),
        "STORNED" => current == "VALIDATED",
        // DRAFT este starea inițială — nu se mai poate reveni la ea.
        _ => false,
    }
}

#[tauri::command]
pub async fn set_invoice_status(
    state: State<'_, AppState>,
    id: String,
    status: InvoiceStatus,
    message: Option<String>,
) -> AppResult<()> {
    // RUST-02: blocăm tranziții ilegale (ex. VALIDATED → DRAFT) înainte de
    // a executa UPDATE-ul. Statul curent este citit din DB pentru a evita
    // race-uri cu un client care ar trimite un status învechit.
    let current = invoices::get(&state.db, &id).await?;
    if !is_allowed_status_transition(current.status.as_str(), status.as_str()) {
        return Err(AppError::Validation(format!(
            "Tranziție de status nepermisă: {} → {}.",
            current.status,
            status.as_str()
        )));
    }
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

    for line in &input.lines {
        let rate =
            rust_decimal::Decimal::try_from(line.vat_rate).unwrap_or(rust_decimal::Decimal::ZERO);
        if !VALID_VAT_RATES.iter().any(|&r| {
            (rust_decimal::Decimal::from(r) - rate).abs() < rust_decimal::Decimal::new(1, 3)
        }) {
            return Err(AppError::Validation(format!(
                "Cotă TVA invalidă: {}%. Valori permise: 0, 5, 9, 11, 19, 21.",
                line.vat_rate
            )));
        }
    }

    // Use rust_decimal for exact monetary math (plan rule: never f64 for money)
    use rust_decimal::Decimal;
    let hundred = Decimal::from(100u32);

    let mut subtotal_dec = Decimal::ZERO;
    let mut vat_total_dec = Decimal::ZERO;
    let line_rows: Vec<(String, String, String, String)> = input
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
                ls.round_dp(2).to_string(),
                lv.round_dp(2).to_string(),
                lt.round_dp(2).to_string(),
            )
        })
        .collect();
    let subtotal = subtotal_dec.round_dp(2).to_string();
    let vat_total = vat_total_dec.round_dp(2).to_string();
    let total = (subtotal_dec + vat_total_dec).round_dp(2).to_string();
    // Fix 2: preserve the existing series/number — do not let the client change them on a draft edit
    let full_number = format!("{}-{:04}", existing.series, existing.number);

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
    .bind(&existing.series)
    .bind(existing.number)
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
        .bind(
            Decimal::try_from(line.quantity)
                .unwrap_or(Decimal::ZERO)
                .round_dp(2)
                .to_string(),
        )
        .bind(&line.unit)
        .bind(
            Decimal::try_from(line.unit_price)
                .unwrap_or(Decimal::ZERO)
                .round_dp(2)
                .to_string(),
        )
        .bind(
            Decimal::try_from(line.vat_rate)
                .unwrap_or(Decimal::ZERO)
                .round_dp(2)
                .to_string(),
        )
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

    // 4. Determină referința storno.
    //    Preferăm FK-ul explicit `storno_of_invoice_id` (BIZ-13). Dacă lipsește
    //    (factură creată înainte de migrația 0008), recurgem la parserul
    //    moștenit al notei "STORNO_OF:{full_number}|{motiv}".
    let storno_ref: Option<String> = resolve_storno_ref(&state.db, &inv).await?;

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
    let all_warnings: Vec<String> = xml_result
        .warnings
        .into_iter()
        .chain(data_warnings)
        .collect();

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

    // 2. Seria facturii storno
    let storno_series = format!("S{}", orig_inv.series);

    // 3. Calculăm liniile storno (cantități negative) și totalurile
    use rust_decimal::Decimal;
    use std::str::FromStr;
    let hundred = Decimal::from(100u32);

    let mut subtotal_dec = Decimal::ZERO;
    let mut vat_total_dec = Decimal::ZERO;

    struct StornoLine {
        id: String,
        name: String,
        description: Option<String>,
        quantity: String,
        unit: String,
        unit_price: String,
        vat_rate: String,
        vat_category: String,
        cpv_code: Option<String>,
        subtotal: String,
        vat_amount: String,
        total: String,
    }

    let storno_lines: Vec<StornoLine> = orig_lines
        .iter()
        .map(|l| {
            let qty = -Decimal::from_str(&l.quantity).unwrap_or(Decimal::ZERO);
            let price = Decimal::from_str(&l.unit_price).unwrap_or(Decimal::ZERO);
            let rate = Decimal::from_str(&l.vat_rate).unwrap_or(Decimal::ZERO);
            let ls = (qty * price).round_dp(2);
            let lv = (ls * rate / hundred).round_dp(2);
            let lt = ls + lv;
            subtotal_dec += ls;
            vat_total_dec += lv;
            StornoLine {
                id: new_id(),
                name: l.name.clone(),
                description: Some(format!("Storno: {}", l.name)),
                quantity: qty.round_dp(6).to_string(),
                unit: l.unit.clone(),
                unit_price: price.round_dp(2).to_string(),
                vat_rate: rate.round_dp(2).to_string(),
                vat_category: l.vat_category.clone(),
                cpv_code: l.cpv_code.clone(),
                subtotal: ls.to_string(),
                vat_amount: lv.to_string(),
                total: lt.to_string(),
            }
        })
        .collect();

    let subtotal = subtotal_dec.round_dp(2).to_string();
    let vat_total = vat_total_dec.round_dp(2).to_string();
    let total = (subtotal_dec + vat_total_dec).round_dp(2).to_string();

    let issue_date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let storno_id = new_id();
    let now = chrono::Utc::now().timestamp();
    let initial_notes = format!(
        "Storno pentru factura {}. Motiv: {}",
        orig_inv.full_number, reason
    );
    let storno_notes = format!("STORNO_OF:{}|{}", orig_inv.full_number, initial_notes);

    // 4. Toate scrierile în DB într-o singură tranzacție atomică
    let mut tx = pool.begin().await.map_err(AppError::Database)?;

    // 4a. Alocăm numărul atomic
    sqlx::query("UPDATE companies SET last_invoice_number = last_invoice_number + 1 WHERE id = ?1")
        .bind(&orig_inv.company_id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

    let allocated_number: i64 =
        sqlx::query_scalar("SELECT last_invoice_number FROM companies WHERE id = ?1")
            .bind(&orig_inv.company_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(AppError::Database)?;

    let full_number = format!("{}-{:04}", storno_series, allocated_number);

    // 4b. Inserăm factura storno. `storno_of_invoice_id` este FK explicit
    //     (BIZ-13) — sursa autoritativă a referinței. Notes-ul rămâne pentru
    //     compatibilitate cu rândurile vechi/UI care încă citesc din el.
    sqlx::query(
        "INSERT INTO invoices (
            id, company_id, contact_id, series, number, full_number,
            issue_date, due_date, currency, exchange_rate,
            subtotal_amount, vat_amount, total_amount, status, notes,
            payment_means_code, storno_of_invoice_id, created_at, updated_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6,
            ?7, ?8, ?9, ?10,
            ?11, ?12, ?13, 'DRAFT', ?14,
            ?15, ?17, ?16, ?16
        )",
    )
    .bind(&storno_id)
    .bind(&orig_inv.company_id)
    .bind(&orig_inv.contact_id)
    .bind(&storno_series)
    .bind(allocated_number)
    .bind(&full_number)
    .bind(&issue_date)
    .bind(&issue_date)
    .bind(&orig_inv.currency)
    .bind(orig_inv.exchange_rate)
    .bind(subtotal)
    .bind(vat_total)
    .bind(total)
    .bind(&storno_notes)
    .bind(&orig_inv.payment_means_code)
    .bind(now)
    .bind(&orig_inv.id)
    .execute(&mut *tx)
    .await
    .map_err(AppError::Database)?;

    // 4c. Inserăm liniile storno
    for (position, line) in storno_lines.iter().enumerate() {
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
        .bind(&line.id)
        .bind(&storno_id)
        .bind((position as i64) + 1)
        .bind(&line.name)
        .bind(&line.description)
        .bind(&line.quantity)
        .bind(&line.unit)
        .bind(&line.unit_price)
        .bind(&line.vat_rate)
        .bind(&line.vat_category)
        .bind(&line.subtotal)
        .bind(&line.vat_amount)
        .bind(&line.total)
        .bind(&line.cpv_code)
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;
    }

    // 4d. Marchează factura originală ca STORNED
    sqlx::query("UPDATE invoices SET status = 'STORNED', updated_at = ?2 WHERE id = ?1")
        .bind(&invoice_id)
        .bind(now)
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

    sqlx::query(
        "INSERT INTO invoice_events (id, invoice_id, event_type, message, created_at)
         VALUES (?1, ?2, 'STATUS_STORNED', ?3, ?4)",
    )
    .bind(new_id())
    .bind(&invoice_id)
    .bind(format!("Stornată prin {}. Motiv: {}", full_number, reason))
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(AppError::Database)?;

    // 4e. Eveniment STORNO_CREATED pe factura storno
    sqlx::query(
        "INSERT INTO invoice_events (id, invoice_id, event_type, message, created_at) \
         VALUES (?1, ?2, 'STORNO_CREATED', ?3, ?4)",
    )
    .bind(new_id())
    .bind(&storno_id)
    .bind(format!(
        "Factură storno pentru {}. Motiv: {}",
        orig_inv.full_number, reason
    ))
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(AppError::Database)?;

    // 5. Commit atomic — toate operațiile reușesc sau niciuna
    tx.commit().await.map_err(AppError::Database)?;

    // 6. Re-fetch factura storno pentru a returna datele complete
    invoices::get(pool, &storno_id).await
}

#[cfg(test)]
mod tests {
    use super::is_allowed_status_transition;

    #[test]
    fn status_cannot_go_backwards_from_validated() {
        assert!(!is_allowed_status_transition("VALIDATED", "DRAFT"));
        assert!(!is_allowed_status_transition("VALIDATED", "QUEUED"));
        assert!(!is_allowed_status_transition("VALIDATED", "SUBMITTED"));
        assert!(!is_allowed_status_transition("VALIDATED", "REJECTED"));
    }

    #[test]
    fn status_terminal_states_block_further_transitions() {
        for target in ["DRAFT", "QUEUED", "SUBMITTED", "VALIDATED"] {
            assert!(
                !is_allowed_status_transition("STORNED", target),
                "STORNED → {} should be blocked",
                target
            );
            assert!(
                !is_allowed_status_transition("REJECTED", target),
                "REJECTED → {} should be blocked",
                target
            );
        }
    }

    #[test]
    fn status_happy_path_transitions_allowed() {
        assert!(is_allowed_status_transition("DRAFT", "QUEUED"));
        assert!(is_allowed_status_transition("QUEUED", "SUBMITTED"));
        assert!(is_allowed_status_transition("SUBMITTED", "VALIDATED"));
        assert!(is_allowed_status_transition("SUBMITTED", "REJECTED"));
        assert!(is_allowed_status_transition("QUEUED", "VALIDATED"));
        assert!(is_allowed_status_transition("VALIDATED", "STORNED"));
        // No-op (UI idempotent emits) must succeed.
        assert!(is_allowed_status_transition("DRAFT", "DRAFT"));
        assert!(is_allowed_status_transition("VALIDATED", "VALIDATED"));
    }

    #[test]
    fn status_cannot_skip_steps_from_draft() {
        assert!(!is_allowed_status_transition("DRAFT", "SUBMITTED"));
        assert!(!is_allowed_status_transition("DRAFT", "VALIDATED"));
        assert!(!is_allowed_status_transition("DRAFT", "STORNED"));
    }
}
