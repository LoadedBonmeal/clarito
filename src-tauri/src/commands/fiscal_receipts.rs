//! Tauri commands pentru Bonuri fiscale / Raport Z (casa de marcat).
//!
//! DRAFT → POSTED → STORNAT lifecycle:
//!   - POSTED: apelează `post_fiscal_receipt` (GL VAT-tagged, idempotent).
//!   - STORNAT: inversează jurnalul GL prin ștergerea intrărilor (DELETE per source_id).

use rust_decimal::Decimal;
use std::str::FromStr;
use tauri::State;

use crate::db::fiscal_receipts::{
    self, FiscalReceipt, FiscalReceiptDetail, FiscalReceiptInput, FiscalReceiptInvoiceLink,
    FiscalReceiptVatLine, InvoiceLinkInput, VatLineInput,
};
use crate::db::gl;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

// ─── Receipt CRUD ─────────────────────────────────────────────────────────────

/// Crează un bon fiscal (DRAFT) pentru o companie.
#[tauri::command]
pub async fn create_fiscal_receipt(
    state: State<'_, AppState>,
    company_id: String,
    input: FiscalReceiptInput,
) -> AppResult<FiscalReceipt> {
    crate::commands::require_valid_date("Data raportului Z", &input.report_date)?;
    fiscal_receipts::create_receipt(&state.db, &company_id, input).await
}

/// Listează bonurile fiscale (cu filtru opțional de dată).
#[tauri::command]
pub async fn list_fiscal_receipts(
    state: State<'_, AppState>,
    company_id: String,
    date_from: Option<String>,
    date_to: Option<String>,
) -> AppResult<Vec<FiscalReceipt>> {
    crate::commands::require_valid_date_opt("Data de început", date_from.as_deref())?;
    crate::commands::require_valid_date_opt("Data de sfârșit", date_to.as_deref())?;
    fiscal_receipts::list_receipts(
        &state.db,
        &company_id,
        date_from.as_deref(),
        date_to.as_deref(),
    )
    .await
}

/// Preia un bon fiscal (cu linii TVA și legături facturi).
#[tauri::command]
pub async fn get_fiscal_receipt(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<FiscalReceiptDetail> {
    fiscal_receipts::get_receipt_detail(&state.db, &id, &company_id).await
}

/// Actualizează un bon DRAFT.
#[tauri::command]
pub async fn update_fiscal_receipt(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    input: FiscalReceiptInput,
) -> AppResult<FiscalReceipt> {
    crate::commands::require_valid_date("Data raportului Z", &input.report_date)?;
    fiscal_receipts::update_receipt(&state.db, &id, &company_id, input).await
}

/// Șterge un bon DRAFT.
#[tauri::command]
pub async fn delete_fiscal_receipt(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    fiscal_receipts::delete_receipt(&state.db, &id, &company_id).await
}

// ─── VAT lines ────────────────────────────────────────────────────────────────

/// Înlocuiește liniile TVA ale unui bon DRAFT.
#[tauri::command]
pub async fn set_fiscal_receipt_vat_lines(
    state: State<'_, AppState>,
    receipt_id: String,
    company_id: String,
    lines: Vec<VatLineInput>,
) -> AppResult<Vec<FiscalReceiptVatLine>> {
    // Validare Σ(baza+tva) == total
    let receipt = fiscal_receipts::get_receipt(&state.db, &receipt_id, &company_id).await?;
    fiscal_receipts::validate_vat_lines_total(&receipt.total, &lines)?;
    fiscal_receipts::replace_vat_lines(&state.db, &receipt_id, &company_id, lines).await
}

// ─── Invoice links ────────────────────────────────────────────────────────────

/// Adaugă o legătură bon–factură (de-dup).
#[tauri::command]
pub async fn add_fiscal_receipt_invoice_link(
    state: State<'_, AppState>,
    receipt_id: String,
    company_id: String,
    input: InvoiceLinkInput,
) -> AppResult<FiscalReceiptInvoiceLink> {
    fiscal_receipts::add_invoice_link(&state.db, &receipt_id, &company_id, input).await
}

/// Elimină o legătură bon–factură.
#[tauri::command]
pub async fn remove_fiscal_receipt_invoice_link(
    state: State<'_, AppState>,
    link_id: String,
    receipt_id: String,
    company_id: String,
) -> AppResult<()> {
    fiscal_receipts::remove_invoice_link(&state.db, &link_id, &receipt_id, &company_id).await
}

// ─── Status lifecycle ────────────────────────────────────────────────────────

/// Schimbă statusul unui bon: DRAFT→POSTED (contabilizare GL) sau POSTED→STORNAT (inversare).
#[tauri::command]
pub async fn set_fiscal_receipt_status(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    status: String,
) -> AppResult<FiscalReceipt> {
    let receipt = fiscal_receipts::get_receipt(&state.db, &id, &company_id).await?;

    match (receipt.status.as_str(), status.as_str()) {
        ("DRAFT", "POSTED") => {
            // Validare înainte de postare
            let detail = fiscal_receipts::get_receipt_detail(&state.db, &id, &company_id).await?;
            if detail.vat_lines.is_empty() {
                return Err(AppError::Validation(
                    "Bonul nu are linii TVA — completați defalcarea pe cote înainte de postare."
                        .to_string(),
                ));
            }
            // Postare GL
            gl::post_fiscal_receipt(&state.db, &company_id, &id).await?;
            // Actualizare status → POSTED
            sqlx::query("UPDATE fiscal_receipts SET status='POSTED' WHERE id=?1 AND company_id=?2")
                .bind(&id)
                .bind(&company_id)
                .execute(&state.db)
                .await?;
        }
        ("POSTED", "STORNAT") => {
            // Storno: șterge jurnalul GL (CASCADE pe gl_entry)
            sqlx::query(
                "DELETE FROM gl_journal \
                 WHERE company_id=?1 AND source_type='FISCAL_RECEIPT' AND source_id=?2",
            )
            .bind(&company_id)
            .bind(&id)
            .execute(&state.db)
            .await?;
            sqlx::query(
                "DELETE FROM gl_journal \
                 WHERE company_id=?1 AND source_type='FISCAL_RECEIPT_SETTLE' AND source_id=?2",
            )
            .bind(&company_id)
            .bind(&id)
            .execute(&state.db)
            .await?;
            sqlx::query(
                "UPDATE fiscal_receipts SET status='STORNAT' WHERE id=?1 AND company_id=?2",
            )
            .bind(&id)
            .bind(&company_id)
            .execute(&state.db)
            .await?;
        }
        ("STORNAT", "DRAFT") => {
            // Re-deschidere STORNAT → DRAFT (permite corectare și re-postare)
            sqlx::query("UPDATE fiscal_receipts SET status='DRAFT' WHERE id=?1 AND company_id=?2")
                .bind(&id)
                .bind(&company_id)
                .execute(&state.db)
                .await?;
        }
        _ => {
            return Err(AppError::Validation(format!(
                "Tranziție de status nevalidă: {} → {}",
                receipt.status, status
            )));
        }
    }

    fiscal_receipts::get_receipt(&state.db, &id, &company_id).await
}

/// Postează decontul POS (card settlement): D 5121 = C 5125; comision D 627 = C 5121.
#[tauri::command]
pub async fn settle_fiscal_receipt_pos(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    commission: Option<String>,
) -> AppResult<FiscalReceipt> {
    let receipt = fiscal_receipts::get_receipt(&state.db, &id, &company_id).await?;
    if receipt.status != "POSTED" {
        return Err(AppError::Validation(
            "Decontul POS se poate posta doar pe un bon POSTED.".to_string(),
        ));
    }
    let commission_dec = commission
        .as_deref()
        .map(|s| Decimal::from_str(s.trim()).unwrap_or(Decimal::ZERO))
        .unwrap_or(Decimal::ZERO);
    gl::post_fiscal_receipt_settle(&state.db, &company_id, &id, commission_dec).await?;
    fiscal_receipts::get_receipt(&state.db, &id, &company_id).await
}
