//! Tauri commands pentru chitanțe (cash receipts).
//!
//! Toate comenzile sunt company-scoped: `company_id` este obligatoriu
//! și este verificat în layer-ul DB. Cross-company access returnează NotFound.

use printpdf::*;
use rust_decimal::Decimal;
use std::str::FromStr;
use tauri::{AppHandle, Manager, State};

use crate::db::receipts::{self, Receipt, ReceiptInput};
use crate::db::{companies, contacts};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::ubl::pdf::amount_to_romanian_words;

// ─── PDF layout constants ──────────────────────────────────────────────────

const PAGE_W: f32 = 210.0;
const PAGE_H: f32 = 148.0; // A5 landscape for a half-page receipt
const MARGIN: f32 = 15.0;
const FONT_TITLE: f32 = 18.0;
const FONT_HEADING: f32 = 11.0;
const FONT_NORMAL: f32 = 9.5;
const FONT_SMALL: f32 = 8.5;
const LINE_H: f32 = 5.5;

// Liberation Sans — sourced from the shared fonts module (single binary copy).
use crate::ubl::fonts::{FONT_BOLD_BYTES, FONT_REGULAR_BYTES};

// ─── PDF path helper ───────────────────────────────────────────────────────

fn receipt_pdf_path(handle: &AppHandle, company_id: &str, receipt_id: &str) -> std::path::PathBuf {
    let dir = handle
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("receipts")
        .join(company_id);
    std::fs::create_dir_all(&dir).ok();
    dir.join(format!("{receipt_id}.pdf"))
}

// ─── Commands ─────────────────────────────────────────────────────────────

/// R15 Wave 3: List all receipts for a company.
#[tauri::command]
pub async fn list_receipts(
    state: State<'_, AppState>,
    company_id: String,
) -> AppResult<Vec<Receipt>> {
    receipts::list(&state.db, &company_id).await
}

/// R15 Wave 3: Get a single receipt by id. Returns NotFound for wrong company.
#[tauri::command]
pub async fn get_receipt(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<Receipt> {
    receipts::get(&state.db, &id, &company_id).await
}

/// R15 Wave 3: Create a receipt for the given company.
/// Allocates the next receipt number atomically (bumps `companies.last_receipt_number`).
#[tauri::command]
pub async fn create_receipt(
    state: State<'_, AppState>,
    company_id: String,
    input: ReceiptInput,
) -> AppResult<Receipt> {
    receipts::create(&state.db, &company_id, input).await
}

/// R15 Wave 3: Delete a receipt. Cross-company deletion returns NotFound.
#[tauri::command]
pub async fn delete_receipt(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<()> {
    receipts::delete(&state.db, &id, &company_id).await
}

/// R15 Wave 3: Generate a chitanță PDF.
///
/// Verifies company ownership, builds a single-page chitanță (company header,
/// number CH-N, date, payer, amount + amount-in-words, optional invoice ref,
/// signature line), writes to the app archive path, updates pdf_path in DB,
/// returns the path string.
#[tauri::command]
pub async fn generate_receipt_pdf(
    state: State<'_, AppState>,
    app: AppHandle,
    id: String,
    company_id: String,
) -> AppResult<String> {
    // 1. Verify ownership.
    let receipt = receipts::get(&state.db, &id, &company_id).await?;

    // 2. Load company (issuer).
    let company = companies::get(&state.db, &receipt.company_id).await?;

    // 3. Optionally load contact name.
    let contact_name: Option<String> = if let Some(ref cid) = receipt.contact_id {
        contacts::get(&state.db, cid, &company_id)
            .await
            .ok()
            .map(|c| c.legal_name)
    } else {
        None
    };

    // 4. Build PDF bytes (CPU-bound — run in spawn_blocking).
    let path = receipt_pdf_path(&app, &company_id, &id);
    let path_clone = path.clone();

    let pdf_bytes = tauri::async_runtime::spawn_blocking(move || -> AppResult<Vec<u8>> {
        build_receipt_pdf(&receipt, &company, contact_name.as_deref())
    })
    .await
    .map_err(|e| AppError::Pdf(e.to_string()))??;

    // 5. Write to disk.
    std::fs::write(&path_clone, &pdf_bytes).map_err(AppError::Io)?;

    let path_str = path_clone
        .to_str()
        .ok_or_else(|| AppError::Pdf("Cale fișier invalidă UTF-8".to_string()))?
        .to_string();

    // 6. Persist pdf_path in DB.
    receipts::set_pdf_path(&state.db, &id, &path_str).await?;

    Ok(path_str)
}

// ─── PDF builder ──────────────────────────────────────────────────────────

fn build_receipt_pdf(
    receipt: &Receipt,
    company: &crate::db::companies::Company,
    contact_name: Option<&str>,
) -> AppResult<Vec<u8>> {
    let (doc, page1, layer1) = PdfDocument::new("Chitanta", Mm(PAGE_W), Mm(PAGE_H), "Layer 1");
    let layer = doc.get_page(page1).get_layer(layer1);

    let font_normal = doc
        .add_external_font(std::io::Cursor::new(FONT_REGULAR_BYTES))
        .map_err(|e| AppError::Pdf(e.to_string()))?;
    let font_bold = doc
        .add_external_font(std::io::Cursor::new(FONT_BOLD_BYTES))
        .map_err(|e| AppError::Pdf(e.to_string()))?;

    let mut y: f32 = PAGE_H - MARGIN;

    // ── Company name ──────────────────────────────────────────────────────
    layer.use_text(
        company.legal_name.clone(),
        FONT_HEADING,
        Mm(MARGIN),
        Mm(y),
        &font_bold,
    );
    y -= LINE_H;
    layer.use_text(
        format!("CUI: {}", company.cui),
        FONT_SMALL,
        Mm(MARGIN),
        Mm(y),
        &font_normal,
    );
    y -= LINE_H - 1.0;
    layer.use_text(
        format!("{}, {}, {}", company.address, company.city, company.country),
        FONT_SMALL,
        Mm(MARGIN),
        Mm(y),
        &font_normal,
    );
    y -= LINE_H + 2.0;

    // ── Title + number ────────────────────────────────────────────────────
    let full_number = receipt.full_number();
    layer.use_text(
        format!("CHITANTA Nr. {}", full_number),
        FONT_TITLE,
        Mm(MARGIN),
        Mm(y),
        &font_bold,
    );
    y -= LINE_H + 2.0;

    // ── Date ──────────────────────────────────────────────────────────────
    layer.use_text(
        format!("Data: {}", receipt.issue_date),
        FONT_NORMAL,
        Mm(MARGIN),
        Mm(y),
        &font_normal,
    );
    y -= LINE_H + 2.0;

    // ── Horizontal divider ────────────────────────────────────────────────
    draw_hline_receipt(&layer, MARGIN, PAGE_W - MARGIN, y + 1.5);
    y -= 3.0;

    // ── Payer ─────────────────────────────────────────────────────────────
    let payer = receipt
        .payer_name
        .as_deref()
        .or(contact_name)
        .unwrap_or("—");
    layer.use_text(
        format!("Am primit de la: {}", payer),
        FONT_NORMAL,
        Mm(MARGIN),
        Mm(y),
        &font_bold,
    );
    y -= LINE_H + 1.0;

    // ── Amount ────────────────────────────────────────────────────────────
    let amount_dec = Decimal::from_str(&receipt.amount).unwrap_or(Decimal::ZERO);
    layer.use_text(
        format!(
            "Suma: {} {} ({}).",
            receipt.amount,
            receipt.currency,
            amount_to_romanian_words(amount_dec)
        ),
        FONT_HEADING,
        Mm(MARGIN),
        Mm(y),
        &font_bold,
    );
    y -= LINE_H + 1.0;

    // ── Optional invoice reference ────────────────────────────────────────
    if let Some(ref inv_id) = receipt.invoice_id {
        layer.use_text(
            format!("Contravaloare factura: {}", inv_id),
            FONT_NORMAL,
            Mm(MARGIN),
            Mm(y),
            &font_normal,
        );
        y -= LINE_H;
    }

    // ── Notes ─────────────────────────────────────────────────────────────
    if let Some(ref notes) = receipt.notes {
        if !notes.trim().is_empty() {
            layer.use_text(
                format!("Observatii: {}", notes),
                FONT_SMALL,
                Mm(MARGIN),
                Mm(y),
                &font_normal,
            );
            y -= LINE_H;
        }
    }

    y -= 4.0;
    draw_hline_receipt(&layer, MARGIN, PAGE_W - MARGIN, y);
    y -= LINE_H + 2.0;

    // ── Signature lines ────────────────────────────────────────────────────
    let sig_left = MARGIN;
    let sig_right = PAGE_W / 2.0 + 10.0;

    layer.use_text(
        "Casier / Platitor:",
        FONT_SMALL,
        Mm(sig_left),
        Mm(y),
        &font_normal,
    );
    layer.use_text(
        "Primitor / Semnatura:",
        FONT_SMALL,
        Mm(sig_right),
        Mm(y),
        &font_normal,
    );
    y -= LINE_H + 4.0;

    // Signature blank lines
    draw_hline_receipt(&layer, sig_left, sig_left + 60.0, y);
    draw_hline_receipt(&layer, sig_right, sig_right + 60.0, y);

    doc.save_to_bytes()
        .map_err(|e| AppError::Pdf(e.to_string()))
}

fn draw_hline_receipt(layer: &PdfLayerReference, x1: f32, x2: f32, y: f32) {
    let points = vec![
        (Point::new(Mm(x1), Mm(y)), false),
        (Point::new(Mm(x2), Mm(y)), false),
    ];
    let line = Line {
        points,
        is_closed: false,
    };
    layer.add_line(line);
}
