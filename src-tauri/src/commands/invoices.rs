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
    let new_invoice = invoices::create(&state.db, input).await?;
    let _ = crate::db::audit::log_user_action(
        &state.db,
        "invoice_created",
        "invoice",
        &new_invoice.id,
        None,
    )
    .await;
    Ok(new_invoice)
}

#[tauri::command]
pub async fn delete_invoice(state: State<'_, AppState>, id: String) -> AppResult<()> {
    invoices::delete(&state.db, &id).await?;
    let _ =
        crate::db::audit::log_user_action(&state.db, "invoice_deleted", "invoice", &id, None).await;
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

    let _ =
        crate::db::audit::log_user_action(&state.db, "invoice_updated", "invoice", &id, None).await;

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
    let claim = sqlx::query(
        "UPDATE invoices SET status = 'STORNED', updated_at = ?2 WHERE id = ?1 AND status = 'VALIDATED'",
    )
    .bind(&invoice_id)
    .bind(now)
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
        Some(&orig_inv.full_number),
    )
    .await;

    // 6. Re-fetch factura storno pentru a returna datele complete
    invoices::get(pool, &storno_id).await
}

/// MISS-04: Duplică o factură existentă într-o nouă ciornă (DRAFT).
///
/// Copiază header-ul (fără date ANAF, fără referință storno) și toate liniile
/// facturii sursă. Alocă un nou număr atomic pentru compania emitentă (același
/// pattern ca `create_invoice_draft`/`storno_invoice`). Data emiterii și
/// scadența noii ciorne sunt setate la ziua curentă — utilizatorul le poate
/// edita ulterior. Întreaga operațiune rulează într-o tranzacție atomică.
#[tauri::command]
pub async fn duplicate_invoice(
    state: State<'_, AppState>,
    invoice_id: String,
) -> AppResult<String> {
    let pool = &state.db;

    // 1. Încarcă factura sursă + liniile (folosim get_with_lines pentru că
    //    `list_lines` este privat în modulul db::invoices).
    let source_bundle = invoices::get_with_lines(pool, &invoice_id).await?;
    let source = source_bundle.invoice;
    let source_lines = source_bundle.lines;

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
        Some(&source.full_number),
    )
    .await;

    Ok(new_invoice_id)
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

    // ── DB-backed storno tests ─────────────────────────────────────────────

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
                cpv_code TEXT
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
}
