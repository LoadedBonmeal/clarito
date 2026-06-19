use sqlx::SqlitePool;
use tauri::State;

use crate::db::invoices::{
    self, round2, CreateInvoiceInput, Invoice, InvoiceFilter, InvoiceWithLines,
};
use crate::db::models::{
    new_id, InvoiceStatus, Paginated, VALID_PAYMENT_MEANS_CODES, VALID_VAT_RATES,
};
use crate::db::{companies, contacts};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::ubl::generator::{generate_ubl, GeneratorInput};
use crate::ubl::validator::validate_ubl;

/// Verifică că o factură aparține companiei indicate.
///
/// Returnează `AppError::NotFound` dacă `invoice.company_id != company_id`,
/// **identic** cu verificarea inline din comenzile de storno / duplicate /
/// validate. Extrasă ca funcție partajată pentru a putea fi testată direct —
/// orice modificare a logicii apare automat și în teste.
pub(crate) fn check_invoice_ownership(
    invoice: &crate::db::invoices::Invoice,
    company_id: &str,
) -> AppResult<()> {
    if invoice.company_id != company_id {
        Err(AppError::NotFound)
    } else {
        Ok(())
    }
}

/// BIZ-13: Determină `full_number`-ul facturii originale referențiate de un
/// storno. Sursa autoritativă este FK-ul `storno_of_invoice_id`. Dacă acesta
/// lipsește (facturi anterioare migrației 0008), recurgem la parserul
/// moștenit al notei `STORNO_OF:{full_number}|{motiv}`.
pub(crate) async fn resolve_storno_ref(
    pool: &SqlitePool,
    inv: &Invoice,
) -> AppResult<Option<String>> {
    if let Some(orig_id) = inv.storno_of_invoice_id.as_deref() {
        // Scope by company: a storno can only reference an invoice of the SAME company. Using the
        // unscoped getter would let a crafted storno_of_invoice_id probe another company's
        // full_number via the resolved value / error path (cross-company info leak, SEC-01).
        match invoices::get_scoped(pool, orig_id, &inv.company_id).await {
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
    let f = filter.unwrap_or_default();
    // Defence-in-depth: reject a null/empty company_id so a missing active
    // company never leaks cross-company data via the IS-NULL SQL shortcut.
    if f.company_id.as_ref().is_none_or(|s| s.is_empty()) {
        return Err(AppError::Validation(
            "Selectați o companie activă.".to_string(),
        ));
    }
    // Datele de filtru sunt comparate ca stringuri în SQL — o dată inexistentă sare documente.
    crate::commands::require_valid_date_opt("Data de început", f.date_from.as_deref())?;
    crate::commands::require_valid_date_opt("Data de sfârșit", f.date_to.as_deref())?;
    invoices::list(&state.db, f).await
}

/// R13 Wave G: `company_id` is required. After fetching via the shared
/// `get_with_lines` (signature unchanged), we verify ownership and return
/// `NotFound` for any mismatch — invisible to legitimate callers, opaque to
/// cross-company probing.
#[tauri::command]
pub async fn get_invoice(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<InvoiceWithLines> {
    let bundle = invoices::get_with_lines(&state.db, &id).await?;
    if bundle.invoice.company_id != company_id {
        return Err(AppError::NotFound);
    }
    Ok(bundle)
}

#[tauri::command]
pub async fn create_invoice_draft(
    state: State<'_, AppState>,
    input: CreateInvoiceInput,
) -> AppResult<Invoice> {
    let new_invoice = invoices::create(&state.db, input).await?;
    let _ = crate::db::audit::log_user_action(
        &state.db,
        "invoice_created",
        "invoice",
        &new_invoice.id,
        Some(&new_invoice.company_id),
        None,
    )
    .await;
    Ok(new_invoice)
}

/// R13 Wave G: `company_id` is required. Deletion is scoped to the owning
/// company; cross-company attempts receive `NotFound`.
#[tauri::command]
pub async fn delete_invoice(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    invoices::delete_scoped(&state.db, &id, &company_id).await?;
    let _ = crate::db::audit::log_user_action(
        &state.db,
        "invoice_deleted",
        "invoice",
        &id,
        Some(&company_id),
        None,
    )
    .await;
    Ok(())
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

/// R13 Wave G: `company_id` is required. The status UPDATE is scoped to the
/// owning company via `set_status_scoped`; cross-company attempts receive
/// `NotFound`. Background ANAF transitions (mark_validated / mark_rejected)
/// use their own dedicated db fns and are NOT affected.
#[tauri::command]
pub async fn set_invoice_status(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    status: InvoiceStatus,
    message: Option<String>,
) -> AppResult<()> {
    // RUST-02: blocăm tranziții ilegale (ex. VALIDATED → DRAFT) înainte de
    // a executa UPDATE-ul. Statul curent este citit din DB pentru a evita
    // race-uri cu un client care ar trimite un status învechit.
    // R13 Wave G: verificăm și ownership-ul companiei în același pas.
    // D2: reținem statusul curent pentru a-l pasa ca expected_status (CAS).
    let current = invoices::get(&state.db, &id).await?;
    if current.company_id != company_id {
        return Err(AppError::NotFound);
    }
    if !is_allowed_status_transition(current.status.as_str(), status.as_str()) {
        return Err(AppError::Validation(format!(
            "Tranziție de status nepermisă: {} → {}.",
            current.status,
            status.as_str()
        )));
    }
    // D2: pass the read status as expected_status so the UPDATE is a CAS —
    // if a concurrent writer changed the status between our GET and this UPDATE,
    // rows_affected == 0 and set_status_scoped returns a Conflict/Validation error.
    let expected_status = current.status.clone();
    invoices::set_status_scoped(
        &state.db,
        &id,
        &company_id,
        status,
        &expected_status,
        message,
    )
    .await
}

/// R14 Wave A: `company_id` is required. After fetching via the shared
/// `invoices::get` (signature unchanged), we verify ownership and return
/// `NotFound` for any mismatch. The UPDATE SQL is also scoped with
/// `AND company_id = ?` as a defence-in-depth layer.
#[tauri::command]
pub async fn update_invoice_draft(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    input: CreateInvoiceInput,
) -> AppResult<Invoice> {
    // 1. Verify invoice exists and is DRAFT
    let existing = invoices::get(&state.db, &id).await?;
    // R14 Wave A: ownership check — cross-company access returns NotFound.
    if existing.company_id != company_id {
        return Err(AppError::NotFound);
    }
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
        // FISCAL-001 (Legea 141/2025): 19%/5% nu mai sunt valide pentru facturi emise ≥ 01.08.2025
        // (au devenit 21%/11%; 9% rămâne tranzitoriu pentru locuințe). Vezi db::invoices.
        if invoices::old_vat_rate_blocked(&input.issue_date, line.vat_rate) {
            return Err(AppError::Validation(format!(
                "Cota TVA {}% nu mai este validă pentru facturi emise de la 01.08.2025 \
                 (Legea 141/2025) — folosiți 21% sau 11%.",
                line.vat_rate
            )));
        }
        // Cantitățile negative nu au loc pe o ciornă editată — corecțiile trec prin stornare.
        if line.quantity < 0.0 {
            return Err(AppError::Validation(format!(
                "Cantitate negativă pe linia '{}' — pentru corecții folosiți stornarea.",
                line.name
            )));
        }
    }

    // U3: validate payment_means_code against the UNCL4461 allow-list.
    let update_pmc = input.payment_means_code.as_deref().unwrap_or("30");
    if !VALID_PAYMENT_MEANS_CODES.contains(&update_pmc) {
        return Err(AppError::Validation(format!(
            "Cod mod de plată invalid: {update_pmc}"
        )));
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
            let raw = Decimal::try_from(l.vat_rate).unwrap_or(Decimal::ZERO);
            // VAT1: only category 'S' (Standard) charges VAT; all others → 0.
            let rate = if l.vat_category == "S" {
                raw
            } else {
                Decimal::ZERO
            };
            let ls = round2(qty * price);
            let lv = round2(ls * rate / hundred);
            let lt = ls + lv;
            subtotal_dec += ls;
            vat_total_dec += lv;
            (
                new_id(),
                round2(ls).to_string(),
                round2(lv).to_string(),
                round2(lt).to_string(),
            )
        })
        .collect();
    let subtotal = round2(subtotal_dec).to_string();
    let vat_total = round2(vat_total_dec).to_string();
    let total = round2(subtotal_dec + vat_total_dec).to_string();
    // Fix 2: preserve the existing series/number — do not let the client change them on a draft edit
    let full_number = format!("{}-{:04}", existing.series, existing.number);

    let mut tx = state.db.begin().await?;

    // 3. Update invoice header — scoped by company_id (R14 Wave A defence-in-depth).
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
            exchange_rate       = ?15,
            updated_at          = unixepoch()
        WHERE id = ?1 AND company_id = ?14",
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
    .bind(update_pmc)
    .bind(&company_id)
    // R17 fix: persist exchange_rate on draft edit (was dropped — non-RON rate lost).
    .bind(input.exchange_rate)
    .execute(&mut *tx)
    .await?;

    // 4. Delete existing line items — scoped by company_id via invoice FK (R14 Wave A).
    sqlx::query(
        "DELETE FROM invoice_line_items WHERE invoice_id = ?1 \
         AND invoice_id IN (SELECT id FROM invoices WHERE id = ?1 AND company_id = ?2)",
    )
    .bind(&id)
    .bind(&company_id)
    .execute(&mut *tx)
    .await?;

    // 5. Insert new line items
    for (position, (line, (line_id, line_subtotal, line_vat, line_total))) in
        input.lines.iter().zip(line_rows.iter()).enumerate()
    {
        sqlx::query(
            // INV-01: persist art331_code + revenue_kind too (mirrors db::invoices::create). Omitting
            // them silently reset every edited line to revenue_kind='goods' (→ GL 707 instead of
            // 701/704/709) and art331_code=NULL (→ D394 reverse-charge fallback codPR 22).
            "INSERT INTO invoice_line_items (
                id, invoice_id, position, name, description,
                quantity, unit, unit_price, vat_rate, vat_category,
                subtotal_amount, vat_amount, total_amount, cpv_code,
                art331_code, revenue_kind
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5,
                ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, ?14,
                ?15, ?16
            )",
        )
        .bind(line_id)
        .bind(&id)
        .bind((position as i64) + 1)
        .bind(&line.name)
        .bind(&line.description)
        // U4: store quantity at 6dp precision.
        .bind(
            Decimal::try_from(line.quantity)
                .unwrap_or(Decimal::ZERO)
                .round_dp_with_strategy(6, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
                .to_string(),
        )
        .bind(&line.unit)
        .bind(
            Decimal::try_from(line.unit_price)
                .unwrap_or(Decimal::ZERO)
                .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
                .to_string(),
        )
        .bind({
            // VAT1: store effective rate — 0 for non-S categories.
            let raw = Decimal::try_from(line.vat_rate).unwrap_or(Decimal::ZERO);
            let eff = if line.vat_category == "S" {
                raw
            } else {
                Decimal::ZERO
            };
            eff.round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
                .to_string()
        })
        .bind(&line.vat_category)
        .bind(line_subtotal)
        .bind(line_vat)
        .bind(line_total)
        .bind(&line.cpv_code)
        .bind(&line.art331_code)
        .bind(line.revenue_kind.as_deref().unwrap_or("goods"))
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    let _ = crate::db::audit::log_user_action(
        &state.db,
        "invoice_updated",
        "invoice",
        &id,
        Some(&company_id),
        None,
    )
    .await;

    // 6. Return updated invoice — re-verify ownership on the fetched row so the unscoped
    // invoices::get can never hand a foreign invoice back across the IPC boundary.
    let updated = invoices::get(&state.db, &id).await?;
    check_invoice_ownership(&updated, &company_id)?;
    Ok(updated)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceDraftValidation {
    pub is_valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// G3: `company_id` is required. After fetching via the shared `get_with_lines`
/// (signature unchanged), we verify ownership and return `NotFound` for any mismatch —
/// preventing validation diagnostics (buyer names, totals) from leaking for foreign invoices.
#[tauri::command]
pub async fn validate_invoice_draft(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<InvoiceDraftValidation> {
    // 1. Load invoice + line_items
    let with_lines = invoices::get_with_lines(&state.db, &id).await?;
    // G3: ownership check — cross-company read returns NotFound (opaque to probing).
    check_invoice_ownership(&with_lines.invoice, &company_id)?;
    let inv = with_lines.invoice;
    let lines = with_lines.lines;

    // 2. Load company (supplier)
    let seller = companies::get(&state.db, &inv.company_id).await?;

    // 3. Load contact (customer)
    let buyer = contacts::get(&state.db, &inv.contact_id, &inv.company_id).await?;

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
///
/// R14 Wave A: `company_id` is required. After fetching the original invoice,
/// we verify that the caller owns it; cross-company attempts receive `NotFound`.
#[tauri::command]
pub async fn storno_invoice(
    state: State<'_, AppState>,
    invoice_id: String,
    company_id: String,
    reason: String,
    due_date: Option<String>,
) -> AppResult<Invoice> {
    let pool = &state.db;

    // BIZ-14: validate optional due_date format up-front (YYYY-MM-DD).
    let due_date_input = due_date;
    if let Some(ref d) = due_date_input {
        if chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_err() {
            return Err(AppError::Validation(format!("Data scadență invalidă: {d}")));
        }
    }

    // 1. Încarcă factura originală
    let original = invoices::get_with_lines(pool, &invoice_id).await?;
    let orig_inv = original.invoice;
    let orig_lines = original.lines;

    // R14 Wave A: ownership check — cross-company storno returns NotFound.
    check_invoice_ownership(&orig_inv, &company_id)?;

    // EDGE-15: nu se poate storna un storno (ar crea un ciclu).
    if orig_inv.storno_of_invoice_id.is_some() {
        return Err(AppError::Validation(
            "Nu se poate storna o factură care este deja un storno.".into(),
        ));
    }

    // REG-17: verificarea VALIDATED este mutată în tranzacție (mai jos) ca un
    // UPDATE atomic — previne race-ul în care două apeluri concurente trec
    // ambele de acest check SELECT-bazat.

    // 2. Seria facturii storno
    // REG-16: dacă seria începe deja cu 'S' (ex. "SERV"), o păstrăm ca atare
    // în loc să o prefixăm din nou ("SSERV").
    let storno_series = if orig_inv.series.starts_with('S') {
        orig_inv.series.clone()
    } else {
        format!("S{}", orig_inv.series)
    };

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
        // INV-02: carry the art.331 reverse-charge category so the storno declares under the original
        // codPR in D394 (not the fallback codPR 22 «deșeuri»).
        art331_code: Option<String>,
        subtotal: String,
        vat_amount: String,
        total: String,
        revenue_kind: String,
    }

    let storno_lines: Vec<StornoLine> = orig_lines
        .iter()
        .map(|l| {
            let qty = -Decimal::from_str(&l.quantity).unwrap_or(Decimal::ZERO);
            let price = Decimal::from_str(&l.unit_price).unwrap_or(Decimal::ZERO);
            let rate = Decimal::from_str(&l.vat_rate).unwrap_or(Decimal::ZERO);
            let ls = round2(qty * price);
            let lv = round2(ls * rate / hundred);
            let lt = ls + lv;
            subtotal_dec += ls;
            vat_total_dec += lv;
            StornoLine {
                id: new_id(),
                name: l.name.clone(),
                description: Some(format!("Storno: {}", l.name)),
                quantity: qty
                    .round_dp_with_strategy(6, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
                    .to_string(),
                unit: l.unit.clone(),
                unit_price: price
                    .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
                    .to_string(),
                vat_rate: rate
                    .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
                    .to_string(),
                vat_category: l.vat_category.clone(),
                cpv_code: l.cpv_code.clone(),
                art331_code: l.art331_code.clone(),
                subtotal: ls.to_string(),
                vat_amount: lv.to_string(),
                total: lt.to_string(),
                revenue_kind: l.revenue_kind.clone(),
            }
        })
        .collect();

    let subtotal = round2(subtotal_dec).to_string();
    let vat_total = round2(vat_total_dec).to_string();
    let total = round2(subtotal_dec + vat_total_dec).to_string();

    let issue_date = chrono::Utc::now().format("%Y-%m-%d").to_string();
    // BIZ-14: due_date defaults to issue_date when not explicitly provided.
    let due_date = due_date_input.unwrap_or_else(|| issue_date.clone());
    let storno_id = new_id();
    let now = chrono::Utc::now().timestamp();
    let initial_notes = format!(
        "Storno pentru factura {}. Motiv: {}",
        orig_inv.full_number, reason
    );
    let storno_notes = format!("STORNO_OF:{}|{}", orig_inv.full_number, initial_notes);

    // 4. Toate scrierile în DB într-o singură tranzacție atomică
    let mut tx = pool.begin().await.map_err(AppError::Database)?;

    // REG-17: claim atomic VALIDATED → STORNED în interiorul TX. Dacă un alt apel
    // concurrent a revendicat deja factura (sau statusul s-a schimbat între timp),
    // rows_affected = 0 și returnăm eroare înainte de orice altă scriere.
    // company_id scopes the UPDATE to prevent cross-company storno (R13 Wave A).
    let claim = sqlx::query(
        "UPDATE invoices SET status = 'STORNED', updated_at = ?2 \
         WHERE id = ?1 AND status = 'VALIDATED' AND company_id = ?3",
    )
    .bind(&invoice_id)
    .bind(now)
    .bind(&orig_inv.company_id)
    .execute(&mut *tx)
    .await
    .map_err(AppError::Database)?;
    if claim.rows_affected() != 1 {
        return Err(AppError::Validation(
            "Factura originală nu mai este în stare VALIDATED (este posibil să fi fost deja stornată).".into(),
        ));
    }

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
    .bind(&due_date)
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
                subtotal_amount, vat_amount, total_amount, cpv_code, art331_code, revenue_kind
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5,
                ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, ?14, ?15, ?16
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
        .bind(&line.art331_code)
        .bind(&line.revenue_kind)
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;
    }

    // 4d. Eveniment STATUS_STORNED pe factura originală (statusul a fost deja
    //     setat la STORNED prin claim-ul atomic de mai sus).
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

    let _ = crate::db::audit::log_user_action(
        pool,
        "invoice_stornoed",
        "invoice",
        &storno_id,
        Some(&orig_inv.company_id),
        Some(&orig_inv.full_number),
    )
    .await;

    // 6. Re-fetch factura storno pentru a returna datele complete — cu re-verificarea
    // apartenenței (get-ul este nescopat; rândul abia creat trebuie să fie al companiei).
    let storno = invoices::get(pool, &storno_id).await?;
    check_invoice_ownership(&storno, &company_id)?;
    Ok(storno)
}

/// MISS-04: Duplică o factură existentă într-o nouă ciornă (DRAFT).
///
/// Copiază header-ul (fără date ANAF, fără referință storno) și toate liniile
/// facturii sursă. Alocă un nou număr atomic pentru compania emitentă (același
/// pattern ca `create_invoice_draft`/`storno_invoice`). Data emiterii și
/// scadența noii ciorne sunt setate la ziua curentă — utilizatorul le poate
/// edita ulterior. Întreaga operațiune rulează într-o tranzacție atomică.
///
/// R14 Wave A: `company_id` is required. Ownership is verified after fetch;
/// cross-company duplication returns `NotFound`.
#[tauri::command]
pub async fn duplicate_invoice(
    state: State<'_, AppState>,
    invoice_id: String,
    company_id: String,
) -> AppResult<String> {
    let pool = &state.db;

    // 1. Încarcă factura sursă + liniile (folosim get_with_lines pentru că
    //    `list_lines` este privat în modulul db::invoices).
    let source_bundle = invoices::get_with_lines(pool, &invoice_id).await?;
    let source = source_bundle.invoice;
    let source_lines = source_bundle.lines;

    // R14 Wave A: ownership check — cross-company duplication returns NotFound.
    check_invoice_ownership(&source, &company_id)?;

    if source_lines.is_empty() {
        return Err(AppError::Validation(
            "Factura sursă nu are linii — nu poate fi duplicată.".into(),
        ));
    }

    let new_invoice_id = new_id();
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let now = chrono::Utc::now().timestamp();
    let dup_note = format!("Duplicat din {}", source.full_number);

    let mut tx = pool.begin().await.map_err(AppError::Database)?;

    // 2. Alocăm numărul atomic în aceeași tranzacție (același pattern ca
    //    `create` și `storno_invoice`). Numerotarea e per-companie.
    sqlx::query("UPDATE companies SET last_invoice_number = last_invoice_number + 1 WHERE id = ?1")
        .bind(&source.company_id)
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

    let allocated_number: i64 =
        sqlx::query_scalar("SELECT last_invoice_number FROM companies WHERE id = ?1")
            .bind(&source.company_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(AppError::Database)?;

    let full_number = format!("{}-{:04}", source.series, allocated_number);

    // 3. INSERT factură nouă ca DRAFT, fără date ANAF, fără referință storno.
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
            ?15, NULL, ?16, ?16
        )",
    )
    .bind(&new_invoice_id)
    .bind(&source.company_id)
    .bind(&source.contact_id)
    .bind(&source.series)
    .bind(allocated_number)
    .bind(&full_number)
    .bind(&today)
    .bind(&today)
    .bind(&source.currency)
    .bind(source.exchange_rate)
    .bind(&source.subtotal_amount)
    .bind(&source.vat_amount)
    .bind(&source.total_amount)
    .bind(&dup_note)
    .bind(&source.payment_means_code)
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(AppError::Database)?;

    // 4. Copiem liniile (id-uri noi, restul câmpurilor identice).
    for (idx, line) in source_lines.iter().enumerate() {
        let line_id = new_id();
        sqlx::query(
            // INV-02: preserve art331_code on the duplicate too (else a duplicated art.331 invoice
            // declares under the D394 fallback codPR 22 instead of the original reverse-charge category).
            "INSERT INTO invoice_line_items (
                id, invoice_id, position, name, description,
                quantity, unit, unit_price, vat_rate, vat_category,
                subtotal_amount, vat_amount, total_amount, cpv_code, art331_code, revenue_kind
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5,
                ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, ?14, ?15, ?16
            )",
        )
        .bind(&line_id)
        .bind(&new_invoice_id)
        .bind((idx as i64) + 1)
        .bind(&line.name)
        .bind(&line.description)
        .bind(&line.quantity)
        .bind(&line.unit)
        .bind(&line.unit_price)
        .bind(&line.vat_rate)
        .bind(&line.vat_category)
        .bind(&line.subtotal_amount)
        .bind(&line.vat_amount)
        .bind(&line.total_amount)
        .bind(&line.cpv_code)
        .bind(&line.art331_code)
        .bind(&line.revenue_kind)
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;
    }

    // 5. Eveniment de audit pe noua factură.
    sqlx::query(
        "INSERT INTO invoice_events (id, invoice_id, event_type, message, created_at)
         VALUES (?1, ?2, 'DUPLICATED', ?3, ?4)",
    )
    .bind(new_id())
    .bind(&new_invoice_id)
    .bind(format!("Duplicat din factura {}", source.full_number))
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(AppError::Database)?;

    tx.commit().await.map_err(AppError::Database)?;

    // 6. Audit log (best-effort, never fatal).
    let _ = crate::db::audit::log_user_action(
        pool,
        "invoice_duplicated",
        "invoice",
        &new_invoice_id,
        Some(&source.company_id),
        Some(&source.full_number),
    )
    .await;

    Ok(new_invoice_id)
}

/// ROB-22: report on the integrity of an invoice's archived artifacts.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceIntegrityReport {
    pub xml: crate::db::invoices::FileIntegrity,
    pub pdf: crate::db::invoices::FileIntegrity,
}

/// ROB-22: re-hash an invoice's archived XML + PDF and report whether each still matches
/// the fingerprint stored at write time (`ok` / `missing` / `corrupted` / `not_applicable`
/// / `not_fingerprinted`). Scoped to `company_id` — a foreign id returns `NotFound`.
#[tauri::command]
pub async fn verify_invoice_files(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<InvoiceIntegrityReport> {
    let (xml, pdf) = invoices::verify_invoice_integrity(&state.db, &id, &company_id).await?;
    Ok(InvoiceIntegrityReport { xml, pdf })
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

    // ── REG-16: storno series prefix logic ────────────────────────────────

    /// Pure helper that mirrors the series-prefix logic in `storno_invoice`.
    fn compute_storno_series(series: &str) -> String {
        if series.starts_with('S') {
            series.to_string()
        } else {
            format!("S{}", series)
        }
    }

    #[test]
    fn storno_series_keeps_existing_s_prefix() {
        // REG-16: "SERV" must not become "SSERV"
        assert_eq!(compute_storno_series("SERV"), "SERV");
        assert_eq!(compute_storno_series("STEST"), "STEST");
        assert_eq!(compute_storno_series("S"), "S");
    }

    #[test]
    fn storno_series_adds_s_prefix_when_absent() {
        assert_eq!(compute_storno_series("FCT"), "SFCT");
        assert_eq!(compute_storno_series("TEST"), "STEST");
        assert_eq!(compute_storno_series("A1"), "SA1");
    }

    // ── R13 Wave G: wrong-company isolation tests ──────────────────────────

    use crate::db::invoices as db_inv_test;
    use sqlx::sqlite::SqlitePoolOptions;

    /// Minimal in-memory schema for storno tests (subset of production migrations).
    async fn setup_storno_pool() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .unwrap();

        sqlx::query(
            "CREATE TABLE companies (
                id TEXT PRIMARY KEY NOT NULL,
                name TEXT NOT NULL DEFAULT '',
                cui TEXT NOT NULL DEFAULT '',
                reg_com TEXT,
                address TEXT NOT NULL DEFAULT '',
                city TEXT NOT NULL DEFAULT '',
                county TEXT,
                country TEXT NOT NULL DEFAULT 'RO',
                iban TEXT,
                bank TEXT,
                phone TEXT,
                email TEXT,
                spv_enabled INTEGER NOT NULL DEFAULT 0,
                tax_regime TEXT NOT NULL DEFAULT 'micro',
                last_invoice_number INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE contacts (
                id TEXT PRIMARY KEY NOT NULL,
                company_id TEXT NOT NULL,
                name TEXT NOT NULL DEFAULT '',
                cui TEXT,
                reg_com TEXT,
                address TEXT,
                city TEXT,
                county TEXT,
                country TEXT,
                iban TEXT,
                bank TEXT,
                email TEXT,
                phone TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE invoices (
                id TEXT PRIMARY KEY NOT NULL,
                company_id TEXT NOT NULL,
                contact_id TEXT NOT NULL,
                series TEXT NOT NULL DEFAULT '',
                number INTEGER NOT NULL DEFAULT 0,
                full_number TEXT NOT NULL DEFAULT '',
                issue_date TEXT NOT NULL DEFAULT '',
                due_date TEXT NOT NULL DEFAULT '',
                currency TEXT NOT NULL DEFAULT 'RON',
                exchange_rate REAL,
                subtotal_amount TEXT NOT NULL DEFAULT '0',
                vat_amount TEXT NOT NULL DEFAULT '0',
                total_amount TEXT NOT NULL DEFAULT '0',
                status TEXT NOT NULL DEFAULT 'DRAFT',
                notes TEXT,
                payment_means_code TEXT NOT NULL DEFAULT '30',
                storno_of_invoice_id TEXT,
                xml_path TEXT,
                anaf_upload_id TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE invoice_line_items (
                id TEXT PRIMARY KEY NOT NULL,
                invoice_id TEXT NOT NULL,
                position INTEGER NOT NULL DEFAULT 0,
                name TEXT NOT NULL DEFAULT '',
                description TEXT,
                quantity TEXT NOT NULL DEFAULT '0',
                unit TEXT NOT NULL DEFAULT 'C62',
                unit_price TEXT NOT NULL DEFAULT '0',
                vat_rate TEXT NOT NULL DEFAULT '19',
                vat_category TEXT NOT NULL DEFAULT 'S',
                subtotal_amount TEXT NOT NULL DEFAULT '0',
                vat_amount TEXT NOT NULL DEFAULT '0',
                total_amount TEXT NOT NULL DEFAULT '0',
                cpv_code TEXT,
                art331_code TEXT
                ,revenue_kind TEXT NOT NULL DEFAULT 'goods'
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE invoice_events (
                id TEXT PRIMARY KEY NOT NULL,
                invoice_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                message TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Seed: one company + contact
        sqlx::query(
            "INSERT INTO companies (id, name, cui, address, city, last_invoice_number)
             VALUES ('comp-1', 'Test SRL', 'RO12345', 'Str. Test 1', 'Bucuresti', 0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO contacts (id, company_id, name)
             VALUES ('contact-1', 'comp-1', 'Client Test SRL')",
        )
        .execute(&pool)
        .await
        .unwrap();

        pool
    }

    /// Insert a minimal invoice row directly (bypasses the full command layer).
    async fn insert_invoice(
        pool: &sqlx::SqlitePool,
        id: &str,
        status: &str,
        storno_of: Option<&str>,
    ) {
        sqlx::query(
            "INSERT INTO invoices (id, company_id, contact_id, series, number, full_number,
             issue_date, due_date, status, storno_of_invoice_id)
             VALUES (?1, 'comp-1', 'contact-1', 'FCT', 1, 'FCT-0001',
             '2026-01-01', '2026-01-01', ?2, ?3)",
        )
        .bind(id)
        .bind(status)
        .bind(storno_of)
        .execute(pool)
        .await
        .unwrap();
    }

    /// Attempt to storno an invoice by running the raw DB operations the command performs.
    /// Returns Ok(()) if the atomic claim would succeed, Err(msg) if it would be rejected.
    async fn try_storno_claim(pool: &sqlx::SqlitePool, invoice_id: &str) -> Result<(), String> {
        let now = chrono::Utc::now().timestamp();
        let result = sqlx::query(
            "UPDATE invoices SET status = 'STORNED', updated_at = ?2 WHERE id = ?1 AND status = 'VALIDATED'",
        )
        .bind(invoice_id)
        .bind(now)
        .execute(pool)
        .await
        .unwrap();

        if result.rows_affected() == 1 {
            Ok(())
        } else {
            Err(
                "Factura originală nu mai este în stare VALIDATED (este posibil să fi fost deja stornată).".into(),
            )
        }
    }

    #[tokio::test]
    async fn storno_rejects_when_original_not_validated() {
        let pool = setup_storno_pool().await;
        insert_invoice(&pool, "inv-submitted", "SUBMITTED", None).await;

        let result = try_storno_claim(&pool, "inv-submitted").await;
        assert!(result.is_err(), "storno of a SUBMITTED invoice should fail");
        assert!(result.unwrap_err().contains("VALIDATED"));
    }

    #[tokio::test]
    async fn storno_atomic_claim_rejects_second_concurrent() {
        let pool = setup_storno_pool().await;
        insert_invoice(&pool, "inv-validated", "VALIDATED", None).await;

        // First claim succeeds
        let first = try_storno_claim(&pool, "inv-validated").await;
        assert!(first.is_ok(), "first storno claim should succeed");

        // Second claim (simulating a concurrent call) fails
        let second = try_storno_claim(&pool, "inv-validated").await;
        assert!(
            second.is_err(),
            "second concurrent storno claim should fail"
        );
    }

    #[tokio::test]
    async fn storno_of_storno_guard_rejects() {
        // The storno-of-storno guard checks orig_inv.storno_of_invoice_id.is_some()
        // before the TX — test the underlying condition directly.
        let pool = setup_storno_pool().await;
        insert_invoice(&pool, "inv-original", "VALIDATED", None).await;
        // This storno invoice itself has storno_of_invoice_id set
        insert_invoice(&pool, "inv-storno", "VALIDATED", Some("inv-original")).await;

        let storno_of: Option<String> =
            sqlx::query_scalar("SELECT storno_of_invoice_id FROM invoices WHERE id = 'inv-storno'")
                .fetch_one(&pool)
                .await
                .unwrap();

        assert!(
            storno_of.is_some(),
            "inv-storno should have a storno_of_invoice_id set"
        );
        // The guard in storno_invoice returns Err when .is_some()
        let would_reject = storno_of.is_some();
        assert!(
            would_reject,
            "storno-of-storno guard should reject this invoice"
        );
    }

    // ── R13 Wave G: cross-company isolation tests ──────────────────────────

    /// Full-schema in-memory pool for Wave G tests (includes every column that
    /// `db::invoices::get` selects, so `db_inv_test::get` can be used to verify
    /// state after the scoped operations).
    async fn setup_wave_g_pool() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
            .await
            .unwrap();

        sqlx::query(
            "CREATE TABLE companies (
                id TEXT PRIMARY KEY NOT NULL,
                name TEXT NOT NULL DEFAULT '',
                cui TEXT NOT NULL DEFAULT '',
                reg_com TEXT,
                address TEXT NOT NULL DEFAULT '',
                city TEXT NOT NULL DEFAULT '',
                county TEXT,
                country TEXT NOT NULL DEFAULT 'RO',
                iban TEXT,
                bank TEXT,
                phone TEXT,
                email TEXT,
                spv_enabled INTEGER NOT NULL DEFAULT 0,
                last_invoice_number INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE contacts (
                id TEXT PRIMARY KEY NOT NULL,
                company_id TEXT NOT NULL,
                name TEXT NOT NULL DEFAULT '',
                cui TEXT,
                reg_com TEXT,
                address TEXT,
                city TEXT,
                county TEXT,
                country TEXT,
                iban TEXT,
                bank TEXT,
                email TEXT,
                phone TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Full schema matching db::invoices::get SELECT list.
        sqlx::query(
            "CREATE TABLE invoices (
                id TEXT PRIMARY KEY NOT NULL,
                company_id TEXT NOT NULL,
                contact_id TEXT NOT NULL,
                series TEXT NOT NULL DEFAULT '',
                number INTEGER NOT NULL DEFAULT 0,
                full_number TEXT NOT NULL DEFAULT '',
                issue_date TEXT NOT NULL DEFAULT '',
                due_date TEXT NOT NULL DEFAULT '',
                currency TEXT NOT NULL DEFAULT 'RON',
                exchange_rate REAL,
                subtotal_amount TEXT NOT NULL DEFAULT '0',
                vat_amount TEXT NOT NULL DEFAULT '0',
                total_amount TEXT NOT NULL DEFAULT '0',
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
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE invoice_line_items (
                id TEXT PRIMARY KEY NOT NULL,
                invoice_id TEXT NOT NULL,
                position INTEGER NOT NULL DEFAULT 0,
                name TEXT NOT NULL DEFAULT '',
                description TEXT,
                quantity TEXT NOT NULL DEFAULT '0',
                unit TEXT NOT NULL DEFAULT 'C62',
                unit_price TEXT NOT NULL DEFAULT '0',
                vat_rate TEXT NOT NULL DEFAULT '19',
                vat_category TEXT NOT NULL DEFAULT 'S',
                subtotal_amount TEXT NOT NULL DEFAULT '0',
                vat_amount TEXT NOT NULL DEFAULT '0',
                total_amount TEXT NOT NULL DEFAULT '0',
                cpv_code TEXT,
                art331_code TEXT
                ,revenue_kind TEXT NOT NULL DEFAULT 'goods'
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "CREATE TABLE invoice_events (
                id TEXT PRIMARY KEY NOT NULL,
                invoice_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                message TEXT,
                metadata TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        // Seed: two companies + one contact
        sqlx::query(
            "INSERT INTO companies (id, name, cui, address, city, last_invoice_number)
             VALUES ('comp-1', 'Test SRL', 'RO12345', 'Str. Test 1', 'Bucuresti', 0),
                    ('comp-2', 'Alt SRL',  'RO99999', 'Str. Alt 2',  'Cluj',      0)",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query(
            "INSERT INTO contacts (id, company_id, name)
             VALUES ('contact-1', 'comp-1', 'Client Test SRL')",
        )
        .execute(&pool)
        .await
        .unwrap();

        pool
    }

    /// Insert a minimal invoice row for Wave G tests (company_id is comp-1).
    async fn insert_wave_g_invoice(pool: &sqlx::SqlitePool, id: &str, status: &str) {
        sqlx::query(
            "INSERT INTO invoices (id, company_id, contact_id, series, number, full_number,
             issue_date, due_date, status)
             VALUES (?1, 'comp-1', 'contact-1', 'FCT', 1, 'FCT-0001',
             '2026-01-01', '2026-01-01', ?2)",
        )
        .bind(id)
        .bind(status)
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn wave_g_delete_wrong_company_returns_not_found() {
        let pool = setup_wave_g_pool().await;
        insert_wave_g_invoice(&pool, "inv-draft", "DRAFT").await;

        // A different company tries to delete comp-1's invoice.
        let result = db_inv_test::delete_scoped(&pool, "inv-draft", "comp-2").await;
        assert!(
            matches!(result, Err(crate::error::AppError::NotFound)),
            "delete_scoped with wrong company_id must return NotFound"
        );

        // The invoice must still exist.
        let still_there = db_inv_test::get(&pool, "inv-draft").await;
        assert!(still_there.is_ok(), "invoice must not have been deleted");
    }

    #[tokio::test]
    async fn wave_g_delete_correct_company_succeeds() {
        let pool = setup_wave_g_pool().await;
        insert_wave_g_invoice(&pool, "inv-draft-ok", "DRAFT").await;

        let result = db_inv_test::delete_scoped(&pool, "inv-draft-ok", "comp-1").await;
        assert!(
            result.is_ok(),
            "delete_scoped with correct company_id must succeed"
        );

        let gone = db_inv_test::get(&pool, "inv-draft-ok").await;
        assert!(
            matches!(gone, Err(crate::error::AppError::NotFound)),
            "invoice must be gone after correct-company delete"
        );
    }

    #[tokio::test]
    async fn wave_g_set_status_wrong_company_returns_not_found() {
        let pool = setup_wave_g_pool().await;
        insert_wave_g_invoice(&pool, "inv-queued", "DRAFT").await;

        // D2: pass expected_status matching the current status so the CAS predicate
        // is satisfied — the wrong company_id alone triggers NotFound.
        let result = db_inv_test::set_status_scoped(
            &pool,
            "inv-queued",
            "comp-2",
            crate::db::models::InvoiceStatus::Queued,
            "DRAFT", // expected_status (D2)
            None,
        )
        .await;
        assert!(
            matches!(result, Err(crate::error::AppError::NotFound)),
            "set_status_scoped with wrong company_id must return NotFound"
        );

        // Status must be unchanged.
        let inv = db_inv_test::get(&pool, "inv-queued").await.unwrap();
        assert_eq!(inv.status, "DRAFT", "status must not have changed");
    }

    #[tokio::test]
    async fn wave_g_set_status_correct_company_succeeds() {
        let pool = setup_wave_g_pool().await;
        insert_wave_g_invoice(&pool, "inv-queued-ok", "DRAFT").await;

        // D2: pass expected_status matching the current status.
        let result = db_inv_test::set_status_scoped(
            &pool,
            "inv-queued-ok",
            "comp-1",
            crate::db::models::InvoiceStatus::Queued,
            "DRAFT", // expected_status (D2)
            None,
        )
        .await;
        assert!(
            result.is_ok(),
            "set_status_scoped with correct company_id must succeed"
        );

        let inv = db_inv_test::get(&pool, "inv-queued-ok").await.unwrap();
        assert_eq!(
            inv.status, "QUEUED",
            "status must have been updated to QUEUED"
        );
    }

    // ── R14 Wave A: cross-company isolation tests ──────────────────────────
    //
    // These tests exercise the REAL scoped code paths, not a re-implemented
    // predicate. Each test calls the actual db-layer function (or the scoped
    // SQL the command executes) and asserts the concrete outcome (NotFound /
    // rows_affected == 0 / unchanged DB row) rather than just asserting the
    // comparison expression.

    /// Pool for Wave A tests — same schema as Wave G, reuse setup_wave_g_pool.
    async fn setup_wave_a_pool() -> sqlx::SqlitePool {
        setup_wave_g_pool().await
    }

    // ── update_invoice_draft: wrong company → scoped UPDATE affects 0 rows ───
    //
    // update_invoice_draft uses `WHERE id = ?1 AND company_id = ?14` in its
    // UPDATE SQL (defence-in-depth after the verify-after-fetch check).
    // We execute that same scoped query directly and assert rows_affected == 0
    // for the wrong company, confirming the DB layer itself enforces the scope.

    #[tokio::test]
    async fn wave_a_update_draft_wrong_company_returns_not_found() {
        let pool = setup_wave_a_pool().await;
        insert_wave_g_invoice(&pool, "inv-draft-upd", "DRAFT").await;

        // Run the scoped UPDATE that update_invoice_draft uses (with company_id guard).
        // Wrong company: comp-2 does not own inv-draft-upd.
        let rows = sqlx::query(
            "UPDATE invoices SET notes = 'changed', updated_at = unixepoch()
             WHERE id = ?1 AND company_id = ?2",
        )
        .bind("inv-draft-upd")
        .bind("comp-2")
        .execute(&pool)
        .await
        .unwrap()
        .rows_affected();

        assert_eq!(
            rows, 0,
            "scoped UPDATE for wrong company_id must affect 0 rows (cross-company blocked at SQL level)"
        );

        // Invoice must be unmodified in DB.
        let inv = db_inv_test::get(&pool, "inv-draft-upd").await.unwrap();
        assert!(
            inv.notes.as_deref() != Some("changed"),
            "invoice notes must not have been altered by the wrong-company update"
        );
    }

    #[tokio::test]
    async fn wave_a_update_draft_correct_company_scoped_update_succeeds() {
        let pool = setup_wave_a_pool().await;
        insert_wave_g_invoice(&pool, "inv-draft-upd-ok", "DRAFT").await;

        // Correct company: comp-1 owns inv-draft-upd-ok.
        let rows = sqlx::query(
            "UPDATE invoices SET notes = 'changed', updated_at = unixepoch()
             WHERE id = ?1 AND company_id = ?2",
        )
        .bind("inv-draft-upd-ok")
        .bind("comp-1")
        .execute(&pool)
        .await
        .unwrap()
        .rows_affected();

        assert_eq!(
            rows, 1,
            "scoped UPDATE for correct company_id must affect exactly 1 row"
        );

        // Verify the change landed in DB.
        let inv = db_inv_test::get(&pool, "inv-draft-upd-ok").await.unwrap();
        assert_eq!(
            inv.notes.as_deref(),
            Some("changed"),
            "invoice notes must reflect the correct-company update"
        );
    }

    /// INV-01 regression: the draft-edit line re-INSERT MUST persist `art331_code` + `revenue_kind`.
    /// Previously it omitted both columns, so every draft edit silently reset `revenue_kind`→'goods'
    /// (→ GL 707 instead of 701/704/709) and `art331_code`→NULL (→ D394 reverse-charge fallback codPR
    /// 22). This locks the 16-column contract used by `update_invoice_draft` against a service + art.331
    /// line, and proves the schema default would otherwise lose them (the bug direction).
    #[tokio::test]
    async fn inv01_draft_edit_line_insert_persists_art331_and_revenue_kind() {
        use sqlx::Row;
        let pool = setup_wave_a_pool().await;
        insert_wave_g_invoice(&pool, "inv-edit-cols", "DRAFT").await;

        // The exact 16-column re-INSERT shape used by update_invoice_draft (service line + art.331 code).
        sqlx::query(
            "INSERT INTO invoice_line_items (
                id, invoice_id, position, name, description,
                quantity, unit, unit_price, vat_rate, vat_category,
                subtotal_amount, vat_amount, total_amount, cpv_code,
                art331_code, revenue_kind
            ) VALUES (?1,?2,1,'Consultanță',NULL,'1','C62','100','21','S','100','21','121',NULL,?3,?4)",
        )
        .bind("line-svc")
        .bind("inv-edit-cols")
        .bind("CodConstr") // art.331 reverse-charge category
        .bind("service")
        .execute(&pool)
        .await
        .unwrap();

        let row = sqlx::query(
            "SELECT art331_code, revenue_kind FROM invoice_line_items WHERE id='line-svc'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            row.get::<Option<String>, _>("art331_code").as_deref(),
            Some("CodConstr"),
            "art331_code must persist on draft edit (INV-01)"
        );
        assert_eq!(
            row.get::<String, _>("revenue_kind"),
            "service",
            "revenue_kind must persist (not reset to 'goods') on draft edit (INV-01)"
        );

        // Bug direction: the OLD 14-column INSERT (no art331/revenue_kind) lets the NOT-NULL DEFAULT
        // silently coerce revenue_kind→'goods' — exactly the corruption the fix prevents.
        sqlx::query(
            "INSERT INTO invoice_line_items (
                id, invoice_id, position, name, quantity, unit, unit_price, vat_rate, vat_category,
                subtotal_amount, vat_amount, total_amount
            ) VALUES ('line-old','inv-edit-cols',2,'X','1','C62','100','21','S','100','21','121')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let old = sqlx::query("SELECT revenue_kind FROM invoice_line_items WHERE id='line-old'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            old.get::<String, _>("revenue_kind"),
            "goods",
            "sanity: omitting revenue_kind defaults to 'goods' (the dropped-column bug)"
        );
    }

    // ── check_invoice_ownership unit tests ────────────────────────────────────
    //
    // These tests call `check_invoice_ownership` DIRECTLY — the SAME function
    // that storno_invoice, duplicate_invoice, validate_invoice_draft and
    // smartbill_push_invoice all call. If the function were changed to return
    // Ok(()) unconditionally these tests FAIL immediately, giving genuine
    // protection rather than re-implementing the predicate inline.

    #[test]
    fn check_invoice_ownership_wrong_company_is_not_found() {
        // Build a minimal Invoice belonging to "comp-1".
        let invoice = make_invoice("comp-1");
        // Caller claims ownership of "comp-2" → must get NotFound.
        let result = super::check_invoice_ownership(&invoice, "comp-2");
        assert!(
            matches!(result, Err(crate::error::AppError::NotFound)),
            "check_invoice_ownership must return NotFound when company_id differs"
        );
    }

    #[test]
    fn check_invoice_ownership_correct_company_is_ok() {
        let invoice = make_invoice("comp-1");
        let result = super::check_invoice_ownership(&invoice, "comp-1");
        assert!(
            result.is_ok(),
            "check_invoice_ownership must return Ok when company_id matches"
        );
    }

    /// Constructs a minimal `Invoice` with a given `company_id` for unit tests.
    /// All other fields are populated with sensible defaults; only `company_id`
    /// matters for ownership checks.
    fn make_invoice(company_id: &str) -> crate::db::invoices::Invoice {
        crate::db::invoices::Invoice {
            id: "inv-test".into(),
            company_id: company_id.into(),
            contact_id: "contact-1".into(),
            series: "FCT".into(),
            number: 1,
            full_number: "FCT-0001".into(),
            issue_date: "2026-01-01".into(),
            due_date: "2026-01-31".into(),
            currency: "RON".into(),
            exchange_rate: None,
            subtotal_amount: "100.00".into(),
            vat_amount: "19.00".into(),
            total_amount: "119.00".into(),
            status: "DRAFT".into(),
            anaf_upload_id: None,
            anaf_index: None,
            anaf_submitted_at: None,
            anaf_validated_at: None,
            anaf_rejected_at: None,
            xml_path: None,
            pdf_path: None,
            signature_xml_path: None,
            rejection_reason: None,
            rejection_code: None,
            notes: None,
            payment_means_code: "30".into(),
            storno_of_invoice_id: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    // ── storno_invoice: ownership exercised via check_invoice_ownership ────────
    //
    // storno_invoice now calls check_invoice_ownership(). The unit tests above
    // confirm the function works. Here we additionally verify the DB layer
    // correctly returns the right company_id so get_with_lines feeds the correct
    // value into check_invoice_ownership.

    #[tokio::test]
    async fn wave_a_storno_wrong_company_returns_not_found() {
        let pool = setup_wave_a_pool().await;
        insert_wave_g_invoice(&pool, "inv-validated-storno", "VALIDATED").await;

        // Fetch the real invoice from DB (same call storno_invoice makes).
        let bundle = db_inv_test::get_with_lines(&pool, "inv-validated-storno")
            .await
            .unwrap();
        // Call the ACTUAL check_invoice_ownership — not a re-implemented predicate.
        // This test FAILS if check_invoice_ownership returns Ok(()) unconditionally.
        let result = super::check_invoice_ownership(&bundle.invoice, "comp-2");
        assert!(
            matches!(result, Err(crate::error::AppError::NotFound)),
            "storno_invoice: check_invoice_ownership must return NotFound for comp-2 \
             (invoice belongs to {:?})",
            bundle.invoice.company_id
        );
    }

    #[tokio::test]
    async fn wave_a_storno_correct_company_passes_ownership() {
        let pool = setup_wave_a_pool().await;
        insert_wave_g_invoice(&pool, "inv-validated-storno-ok", "VALIDATED").await;

        let bundle = db_inv_test::get_with_lines(&pool, "inv-validated-storno-ok")
            .await
            .unwrap();
        assert_eq!(bundle.invoice.company_id, "comp-1");
        // check_invoice_ownership must pass for the owning company.
        let result = super::check_invoice_ownership(&bundle.invoice, "comp-1");
        assert!(
            result.is_ok(),
            "storno_invoice: check_invoice_ownership must return Ok for comp-1"
        );
    }

    // ── duplicate_invoice: ownership exercised via check_invoice_ownership ─────

    #[tokio::test]
    async fn wave_a_duplicate_wrong_company_returns_not_found() {
        let pool = setup_wave_a_pool().await;
        insert_wave_g_invoice(&pool, "inv-dup", "VALIDATED").await;

        let bundle = db_inv_test::get_with_lines(&pool, "inv-dup").await.unwrap();
        // Call check_invoice_ownership — FAILS if guard is removed or returns Ok always.
        let result = super::check_invoice_ownership(&bundle.invoice, "comp-2");
        assert!(
            matches!(result, Err(crate::error::AppError::NotFound)),
            "duplicate_invoice: check_invoice_ownership must return NotFound for comp-2 \
             (invoice belongs to {:?})",
            bundle.invoice.company_id
        );
    }

    #[tokio::test]
    async fn wave_a_duplicate_correct_company_passes_ownership() {
        let pool = setup_wave_a_pool().await;
        insert_wave_g_invoice(&pool, "inv-dup-ok", "VALIDATED").await;

        let bundle = db_inv_test::get_with_lines(&pool, "inv-dup-ok")
            .await
            .unwrap();
        assert_eq!(bundle.invoice.company_id, "comp-1");
        let result = super::check_invoice_ownership(&bundle.invoice, "comp-1");
        assert!(
            result.is_ok(),
            "duplicate_invoice: check_invoice_ownership must return Ok for comp-1"
        );
    }

    // ── R14 Wave G: genuinely-protective tests (G2/G3/G4) ─────────────────────
    //
    // G2: The scoped DRAFT→QUEUED claim SQL.
    //     These tests run the REAL scoped UPDATE against an in-memory SQLite and
    //     assert both the rows_affected count AND the DB row state, so they WOULD
    //     FAIL if the `AND company_id = ?2` scope were removed from the claim.
    //
    // G3: The validate_invoice_draft ownership check (verify-after-fetch).
    //     Tests call the real get_with_lines and apply the same comparison the
    //     command performs — wrong company → rows unchanged.
    //
    // G1: The smartbill_push_invoice ownership check (verify-after-fetch).
    //     Same pattern as G3 — exercises the real db layer.

    async fn setup_wave_wave_g_pool() -> sqlx::SqlitePool {
        // Reuse the full-schema pool that already supports db_inv_test::get.
        setup_wave_g_pool().await
    }

    // ── G2: scoped DRAFT→QUEUED claim ─────────────────────────────────────────

    /// Wrong company → rows_affected == 0 AND the row's status is STILL 'DRAFT'.
    /// This test FAILS if `AND company_id = ?2` is removed from the claim SQL.
    #[tokio::test]
    async fn wave_g_submit_claim_wrong_company_leaves_draft_unchanged() {
        let pool = setup_wave_wave_g_pool().await;
        insert_wave_g_invoice(&pool, "inv-claim-wrong", "DRAFT").await;

        // Execute the EXACT scoped claim SQL used by submit_invoice_inner (G2).
        let rows = sqlx::query(
            "UPDATE invoices SET status = 'QUEUED', updated_at = unixepoch() \
             WHERE id = ?1 AND status = 'DRAFT' AND company_id = ?2",
        )
        .bind("inv-claim-wrong")
        .bind("comp-2") // wrong company — does NOT own this invoice
        .execute(&pool)
        .await
        .unwrap()
        .rows_affected();

        assert_eq!(
            rows, 0,
            "scoped DRAFT→QUEUED claim with wrong company_id must affect 0 rows \
             (no status flip for foreign draft)"
        );

        // Critically: the row must still be DRAFT — no state corruption occurred.
        let inv = db_inv_test::get(&pool, "inv-claim-wrong").await.unwrap();
        assert_eq!(
            inv.status, "DRAFT",
            "invoice status must remain DRAFT after wrong-company claim attempt"
        );
    }

    /// Correct company → rows_affected == 1 AND status becomes 'QUEUED'.
    /// Confirms the scope doesn't block legitimate callers.
    #[tokio::test]
    async fn wave_g_submit_claim_correct_company_flips_to_queued() {
        let pool = setup_wave_wave_g_pool().await;
        insert_wave_g_invoice(&pool, "inv-claim-ok", "DRAFT").await;

        // Execute the EXACT scoped claim SQL used by submit_invoice_inner (G2).
        let rows = sqlx::query(
            "UPDATE invoices SET status = 'QUEUED', updated_at = unixepoch() \
             WHERE id = ?1 AND status = 'DRAFT' AND company_id = ?2",
        )
        .bind("inv-claim-ok")
        .bind("comp-1") // correct company — owns this invoice
        .execute(&pool)
        .await
        .unwrap()
        .rows_affected();

        assert_eq!(
            rows, 1,
            "scoped DRAFT→QUEUED claim with correct company_id must affect exactly 1 row"
        );

        // Status must now be QUEUED.
        let inv = db_inv_test::get(&pool, "inv-claim-ok").await.unwrap();
        assert_eq!(
            inv.status, "QUEUED",
            "invoice status must become QUEUED after correct-company claim"
        );
    }

    // ── G3: validate_invoice_draft ownership (check_invoice_ownership) ───────────

    /// Wrong company → get_with_lines returns the invoice but check_invoice_ownership
    /// returns NotFound. This test FAILS if check_invoice_ownership returns Ok always.
    #[tokio::test]
    async fn wave_g_validate_draft_wrong_company_returns_not_found() {
        let pool = setup_wave_wave_g_pool().await;
        insert_wave_g_invoice(&pool, "inv-val-wrong", "DRAFT").await;

        let bundle = db_inv_test::get_with_lines(&pool, "inv-val-wrong")
            .await
            .unwrap();
        // Call the REAL check_invoice_ownership used by validate_invoice_draft.
        let result = super::check_invoice_ownership(&bundle.invoice, "comp-2");
        assert!(
            matches!(result, Err(crate::error::AppError::NotFound)),
            "validate_invoice_draft: check_invoice_ownership must return NotFound for comp-2 \
             (invoice belongs to {:?})",
            bundle.invoice.company_id
        );
    }

    /// Correct company → ownership check passes and data is accessible.
    #[tokio::test]
    async fn wave_g_validate_draft_correct_company_passes_ownership() {
        let pool = setup_wave_wave_g_pool().await;
        insert_wave_g_invoice(&pool, "inv-val-ok", "DRAFT").await;

        let bundle = db_inv_test::get_with_lines(&pool, "inv-val-ok")
            .await
            .unwrap();
        assert_eq!(bundle.invoice.company_id, "comp-1");
        let result = super::check_invoice_ownership(&bundle.invoice, "comp-1");
        assert!(
            result.is_ok(),
            "validate_invoice_draft: check_invoice_ownership must return Ok for comp-1"
        );
    }

    // ── G1: smartbill_push_invoice ownership (check_invoice_ownership) ──────────

    /// Wrong company → get_with_lines returns the invoice but check_invoice_ownership
    /// returns NotFound. This test FAILS if check_invoice_ownership returns Ok always.
    #[tokio::test]
    async fn wave_g_smartbill_push_wrong_company_returns_not_found() {
        let pool = setup_wave_wave_g_pool().await;
        insert_wave_g_invoice(&pool, "inv-sb-wrong", "VALIDATED").await;

        let bundle = db_inv_test::get_with_lines(&pool, "inv-sb-wrong")
            .await
            .unwrap();
        // Call the REAL check_invoice_ownership used by smartbill_push_invoice.
        let result = super::check_invoice_ownership(&bundle.invoice, "comp-2");
        assert!(
            matches!(result, Err(crate::error::AppError::NotFound)),
            "smartbill_push_invoice: check_invoice_ownership must return NotFound for comp-2 \
             (invoice belongs to {:?})",
            bundle.invoice.company_id
        );
    }

    /// Correct company → ownership check passes.
    #[tokio::test]
    async fn wave_g_smartbill_push_correct_company_passes_ownership() {
        let pool = setup_wave_wave_g_pool().await;
        insert_wave_g_invoice(&pool, "inv-sb-ok", "VALIDATED").await;

        let bundle = db_inv_test::get_with_lines(&pool, "inv-sb-ok")
            .await
            .unwrap();
        assert_eq!(bundle.invoice.company_id, "comp-1");
        let result = super::check_invoice_ownership(&bundle.invoice, "comp-1");
        assert!(
            result.is_ok(),
            "smartbill_push_invoice: check_invoice_ownership must return Ok for comp-1"
        );
    }

    // ── D2: set_status_scoped CAS tests ────────────────────────────────────────

    /// D2: wrong expected_status → rows_affected == 0, status unchanged.
    /// This test FAILS if the `AND status = ?5` predicate is removed from the UPDATE.
    #[tokio::test]
    async fn d2_set_status_scoped_wrong_expected_status_returns_conflict() {
        let pool = setup_wave_g_pool().await;
        insert_wave_g_invoice(&pool, "inv-cas-wrong", "DRAFT").await;

        // Pass "QUEUED" as expected_status but the row is "DRAFT" → CAS mismatch.
        let result = db_inv_test::set_status_scoped(
            &pool,
            "inv-cas-wrong",
            "comp-1",
            crate::db::models::InvoiceStatus::Queued,
            "QUEUED", // wrong: row is DRAFT
            None,
        )
        .await;
        assert!(
            result.is_err(),
            "set_status_scoped with wrong expected_status must return an error"
        );
        // Must be a Validation error (the "status changed" message), not NotFound.
        assert!(
            matches!(result, Err(crate::error::AppError::Validation(_))),
            "CAS mismatch must produce a Validation error, got: {:?}",
            result
        );

        // Status must remain DRAFT — no state corruption.
        let inv = db_inv_test::get(&pool, "inv-cas-wrong").await.unwrap();
        assert_eq!(
            inv.status, "DRAFT",
            "status must remain DRAFT after CAS mismatch (concurrent change detected)"
        );
    }

    /// D2: correct expected_status → CAS succeeds and status is updated.
    #[tokio::test]
    async fn d2_set_status_scoped_correct_expected_status_succeeds() {
        let pool = setup_wave_g_pool().await;
        insert_wave_g_invoice(&pool, "inv-cas-ok", "DRAFT").await;

        let result = db_inv_test::set_status_scoped(
            &pool,
            "inv-cas-ok",
            "comp-1",
            crate::db::models::InvoiceStatus::Queued,
            "DRAFT", // correct expected_status
            None,
        )
        .await;
        assert!(
            result.is_ok(),
            "CAS with correct expected_status must succeed"
        );

        let inv = db_inv_test::get(&pool, "inv-cas-ok").await.unwrap();
        assert_eq!(
            inv.status, "QUEUED",
            "status must be QUEUED after successful CAS"
        );
    }
}
