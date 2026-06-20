//! Generare PDF simplu pentru factură folosind `printpdf 0.7`.

use printpdf::*;
// Use printpdf's re-exported image crate (avoids name ambiguity with printpdf::image module).
use printpdf::image_crate as img_crate;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::str::FromStr;

use crate::db::companies::Company;
use crate::db::contacts::Contact;
use crate::db::invoices::LineItem;
use crate::error::{AppError, AppResult};
use crate::ubl::generator::GeneratorInput;

// ─── Layout constants (mm) ────────────────────────────────────────────────────

const PAGE_W: f32 = 210.0;
const PAGE_H: f32 = 297.0;
const MARGIN: f32 = 15.0;
const COL_MID: f32 = PAGE_W / 2.0 + 5.0; // separator between left/right

// Sizes
const FONT_TITLE: f32 = 16.0;
const FONT_HEADING: f32 = 11.0;
const FONT_NORMAL: f32 = 9.0;
const FONT_SMALL: f32 = 8.0;
const LINE_H: f32 = 5.0; // mm per text line

// Liberation Sans — sourced from the shared fonts module (single binary copy).
use crate::ubl::fonts::{FONT_BOLD_BYTES, FONT_REGULAR_BYTES};

// ─── Template config ──────────────────────────────────────────────────────────

/// Configures the visual appearance of the generated PDF invoice.
///
/// The default preset is `"clasic"` which produces output byte-equivalent to
/// the original (no colour, no logo unless `seller.logo_path` is set).
#[derive(Debug, Clone)]
pub struct InvoiceTemplate {
    /// One of: `"clasic"`, `"modern"`, `"minimal"`.
    pub preset: String,
    /// Accent colour as `"#RRGGBB"`. Used according to the preset rules.
    pub accent_hex: String,
    /// Custom branding line(s) under the date line (slogan / mențiuni legale —
    /// ex. "Capital social: 200 lei · J40/1234/2020"). Max 2 lines (split by '\n');
    /// empty = nothing drawn.
    pub header_note: String,
    /// Custom block at the bottom of the invoice (mulțumiri / termeni de plată /
    /// mențiuni). Max 3 lines (split by '\n'); empty = nothing drawn.
    pub footer_note: String,
    /// Show the "suma în litere" line under the total. Default true.
    pub show_words: bool,
    /// Show the "Detaliu TVA" breakdown table. Default true.
    pub show_vat_detail: bool,
}

impl Default for InvoiceTemplate {
    fn default() -> Self {
        Self {
            preset: "clasic".into(),
            accent_hex: "#000000".into(),
            header_note: String::new(),
            footer_note: String::new(),
            show_words: true,
            show_vat_detail: true,
        }
    }
}

/// Build a DEMO `GeneratorInput` for the template-preview feature: a fictitious invoice
/// (2 lines, 21% + 11%) issued by the REAL `seller` company, so the preview shows the
/// user's own logo/IBAN/identity with sample data. Never persisted.
pub fn sample_preview_input(seller: Company) -> GeneratorInput {
    let company_id = seller.id.clone();
    let buyer = Contact {
        id: "preview-client".into(),
        company_id: company_id.clone(),
        contact_type: "CUSTOMER".into(),
        cui: Some("RO12345674".into()),
        legal_name: "Client Demo SRL".into(),
        vat_payer: true,
        cash_vat: false,
        is_individual: false,
        address: Some("Str. Exemplu nr. 10".into()),
        city: Some("Cluj-Napoca".into()),
        county: Some("Cluj".into()),
        country: "RO".into(),
        email: None,
        phone: None,
        currency: None,
        iban: None,
        bank_name: None,
        swift: None,
        payment_term_days: None,
        created_at: 0,
        updated_at: 0,
    };
    let mk_line = |pos: i64,
                   name: &str,
                   qty: &str,
                   price: &str,
                   rate: &str,
                   base: &str,
                   vat: &str,
                   total: &str| LineItem {
        id: format!("preview-line-{pos}"),
        invoice_id: "preview-invoice".into(),
        position: pos,
        name: name.into(),
        description: None,
        quantity: qty.into(),
        unit: "H87".into(),
        unit_price: price.into(),
        vat_rate: rate.into(),
        vat_category: "S".into(),
        subtotal_amount: base.into(),
        vat_amount: vat.into(),
        total_amount: total.into(),
        cpv_code: None,
        art331_code: None,
        revenue_kind: "goods".into(),
    };
    let lines = vec![
        mk_line(
            1,
            "Servicii consultanță (demo)",
            "10.00",
            "100.00",
            "21.00",
            "1000.00",
            "210.00",
            "1210.00",
        ),
        mk_line(
            2,
            "Materiale tipărite (demo)",
            "5.00",
            "40.00",
            "11.00",
            "200.00",
            "22.00",
            "222.00",
        ),
    ];
    let invoice = crate::db::invoices::Invoice {
        id: "preview-invoice".into(),
        company_id,
        contact_id: buyer.id.clone(),
        series: seller.invoice_series.clone(),
        number: seller.last_invoice_number + 1,
        full_number: format!("{}-DEMO-0001", seller.invoice_series),
        issue_date: "2026-06-15".into(),
        due_date: "2026-07-15".into(),
        currency: "RON".into(),
        exchange_rate: None,
        subtotal_amount: "1200.00".into(),
        vat_amount: "232.00".into(),
        total_amount: "1432.00".into(),
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
        notes: Some("Previzualizare șablon — factură demonstrativă, nu se emite.".into()),
        payment_means_code: "30".into(),
        storno_of_invoice_id: None,
        created_at: 0,
        updated_at: 0,
    };
    GeneratorInput {
        invoice,
        lines,
        seller,
        buyer,
        storno_ref: None,
    }
}

/// Parse `"#RRGGBB"` into a printpdf `Color::Rgb`. Falls back to black on any
/// parse failure so a bad setting value never breaks PDF generation.
fn parse_accent(hex: &str) -> Color {
    let h = hex.trim().trim_start_matches('#');
    if h.len() == 6 {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&h[0..2], 16),
            u8::from_str_radix(&h[2..4], 16),
            u8::from_str_radix(&h[4..6], 16),
        ) {
            return Color::Rgb(Rgb::new(
                r as f32 / 255.0,
                g as f32 / 255.0,
                b as f32 / 255.0,
                None,
            ));
        }
    }
    Color::Rgb(Rgb::new(0.0, 0.0, 0.0, None))
}

fn black() -> Color {
    Color::Rgb(Rgb::new(0.0, 0.0, 0.0, None))
}

// ─── Public entry point ───────────────────────────────────────────────────────

pub fn generate_pdf(input: &GeneratorInput, template: &InvoiceTemplate) -> AppResult<Vec<u8>> {
    let (doc, page1, layer1) = PdfDocument::new("Factura", Mm(PAGE_W), Mm(PAGE_H), "Layer 1");
    let layer = doc.get_page(page1).get_layer(layer1);

    let font_normal = doc
        .add_external_font(std::io::Cursor::new(FONT_REGULAR_BYTES))
        .map_err(|e| AppError::Pdf(e.to_string()))?;
    let font_bold = doc
        .add_external_font(std::io::Cursor::new(FONT_BOLD_BYTES))
        .map_err(|e| AppError::Pdf(e.to_string()))?;

    let inv = &input.invoice;
    let seller = &input.seller;
    let buyer = &input.buyer;

    let accent = parse_accent(&template.accent_hex);
    let preset = template.preset.as_str();

    // Apply accent to title? (modern + minimal)
    let accent_title = matches!(preset, "modern" | "minimal");
    // Apply accent to section headings + dividers? (modern only)
    let accent_sections = matches!(preset, "modern");

    // Current Y cursor (counts down from top)
    let mut y: f32 = PAGE_H - MARGIN;

    // ── Company name (always black) ───────────────────────────────────────────
    layer.use_text(
        seller.legal_name.clone(),
        FONT_TITLE,
        Mm(MARGIN),
        Mm(y),
        &font_bold,
    );
    y -= 8.0;

    // ── Try to embed logo in top-right corner ─────────────────────────────────
    // Pass the current y (after the company name line) so the logo sits near the header top.
    try_embed_logo(&doc, &layer, seller, PAGE_H - MARGIN);

    // ── Invoice title ─────────────────────────────────────────────────────────
    let title = format!("FACTURA Nr. {}", inv.full_number);
    if accent_title {
        layer.set_fill_color(accent.clone());
    }
    layer.use_text(title, FONT_TITLE, Mm(MARGIN), Mm(y), &font_bold);
    if accent_title {
        layer.set_fill_color(black());
    }
    y -= 6.0;

    let date_line = format!(
        "Data emiterii: {}   Scadenta: {}   Moneda: {}",
        inv.issue_date, inv.due_date, inv.currency
    );
    layer.use_text(date_line, FONT_NORMAL, Mm(MARGIN), Mm(y), &font_normal);
    y -= 8.0;

    // Linie(le) de antet personalizate din șablon (slogan / mențiuni legale) — max 2 rânduri.
    if !template.header_note.trim().is_empty() {
        for note_line in template.header_note.lines().take(2) {
            let note_line = note_line.trim();
            if note_line.is_empty() {
                continue;
            }
            layer.use_text(
                truncate(note_line, 110),
                FONT_SMALL,
                Mm(MARGIN),
                Mm(y),
                &font_normal,
            );
            y -= 4.5;
        }
        y -= 1.5;
    }

    // Mențiunea obligatorie "TVA la încasare" (Cod fiscal art. 319 alin. (20) lit. r).
    if crate::ubl::generator::invoice_under_cash_vat(seller, &input.lines) {
        layer.use_text(
            "TVA la încasare",
            FONT_NORMAL,
            Mm(MARGIN),
            Mm(y),
            &font_bold,
        );
        y -= 6.0;
    }

    // ── Divider ───────────────────────────────────────────────────────────────
    if accent_sections {
        layer.set_outline_color(accent.clone());
    }
    draw_hline(&layer, MARGIN, PAGE_W - MARGIN, y + 2.0);
    if accent_sections {
        layer.set_outline_color(black());
    }
    y -= 2.0;

    // ── Seller / Buyer blocks ─────────────────────────────────────────────────
    let block_top = y;

    // Seller (left)
    if accent_sections {
        layer.set_fill_color(accent.clone());
    }
    layer.use_text("FURNIZOR", FONT_HEADING, Mm(MARGIN), Mm(y), &font_bold);
    if accent_sections {
        layer.set_fill_color(black());
    }
    y -= LINE_H;
    y = write_seller_block(&layer, &font_normal, &font_bold, seller, MARGIN, y);

    // Buyer (right) — reset y to block_top
    let mut y_right = block_top;
    if accent_sections {
        layer.set_fill_color(accent.clone());
    }
    layer.use_text(
        "CUMPARATOR",
        FONT_HEADING,
        Mm(COL_MID),
        Mm(y_right),
        &font_bold,
    );
    if accent_sections {
        layer.set_fill_color(black());
    }
    y_right -= LINE_H;
    write_buyer_block(&layer, &font_normal, buyer, COL_MID, y_right);

    // Advance past both blocks
    y -= 4.0;
    if y > block_top - 30.0 {
        y = block_top - 30.0;
    }

    // ── Line items table ──────────────────────────────────────────────────────
    if accent_sections {
        layer.set_outline_color(accent.clone());
    }
    draw_hline(&layer, MARGIN, PAGE_W - MARGIN, y);
    if accent_sections {
        layer.set_outline_color(black());
    }
    y -= 5.0;

    // Table header
    let headers = [
        "Nr",
        "Denumire",
        "UM",
        "Cant",
        "Pret unitar",
        "TVA%",
        "Valoare",
    ];
    let col_x = col_positions();
    for (i, h) in headers.iter().enumerate() {
        layer.use_text(*h, FONT_SMALL, Mm(col_x[i]), Mm(y), &font_bold);
    }
    y -= LINE_H;
    draw_hline(&layer, MARGIN, PAGE_W - MARGIN, y + 1.0);

    // Bottom margin below which a new page is needed (leave room for one row + footer)
    const BOTTOM_MARGIN: f32 = MARGIN + LINE_H + 4.0;

    // Track the active layer across potential page breaks
    let mut cur_layer = layer.clone();

    // Table rows — add page-break when y approaches the bottom margin
    for line in &input.lines {
        y -= LINE_H;

        // Page-break: if y has gone below the bottom margin, open a new page
        if y < BOTTOM_MARGIN {
            let (new_page, new_layer_idx) = doc.add_page(Mm(PAGE_W), Mm(PAGE_H), "Layer 1");
            cur_layer = doc.get_page(new_page).get_layer(new_layer_idx);
            y = PAGE_H - MARGIN;

            // Re-draw the table header on the new page
            for (i, h) in headers.iter().enumerate() {
                cur_layer.use_text(*h, FONT_SMALL, Mm(col_x[i]), Mm(y), &font_bold);
            }
            y -= LINE_H;
            draw_hline(&cur_layer, MARGIN, PAGE_W - MARGIN, y + 1.0);
            y -= LINE_H;
        }

        write_line_row(&cur_layer, &font_normal, line, &col_x, y, &inv.currency);
    }

    y -= 2.0;
    draw_hline(&cur_layer, MARGIN, PAGE_W - MARGIN, y);
    y -= 6.0;

    // ── Footer page-break guard ───────────────────────────────────────────────
    // The footer block (VAT breakdown + totals + notes) needs approximately
    // FOOTER_MIN_HEIGHT mm. If the remaining space on the current page is less
    // than this, open a new page before drawing any footer content.
    // This prevents the footer from being drawn below the page bottom (which
    // printpdf does not clip — it becomes invisible in the PDF).
    const FOOTER_MIN_HEIGHT: f32 = 40.0; // conservative estimate: VAT rows + totals
    if y < MARGIN + FOOTER_MIN_HEIGHT {
        let (new_page, new_layer_idx) = doc.add_page(Mm(PAGE_W), Mm(PAGE_H), "Layer 1");
        cur_layer = doc.get_page(new_page).get_layer(new_layer_idx);
        y = PAGE_H - MARGIN;
    }

    // ── VAT breakdown table (left side) ──────────────────────────────────────
    // BIZ-19: group by (rate, vat_category) so 0% Exempt and 0% Zero-rated
    // surface as separate rows instead of being merged into "0%".
    // Opțional din șablon (show_vat_detail) — unele firme preferă factura compactă.
    if template.show_vat_detail {
        let mut vat_groups: std::collections::BTreeMap<(i64, String), (Decimal, Decimal)> =
            std::collections::BTreeMap::new();
        for line in &input.lines {
            let rate_dec = Decimal::from_str(&line.vat_rate).unwrap_or(Decimal::ZERO);
            let rate_key = (rate_dec * Decimal::from(100)).to_i64().unwrap_or(0);
            let category = line.vat_category.clone();
            let entry = vat_groups
                .entry((rate_key, category))
                .or_insert((Decimal::ZERO, Decimal::ZERO));
            entry.0 += Decimal::from_str(&line.subtotal_amount).unwrap_or(Decimal::ZERO);
            entry.1 += Decimal::from_str(&line.vat_amount).unwrap_or(Decimal::ZERO);
        }
        if !vat_groups.is_empty() {
            cur_layer.use_text("Detaliu TVA:", FONT_SMALL, Mm(MARGIN), Mm(y), &font_bold);
            y -= LINE_H - 1.0;
            let hdrs = ["Cotă", "Bază impozabilă", "TVA"];
            let vt_cols = [MARGIN, MARGIN + 22.0, MARGIN + 54.0];
            for (i, h) in hdrs.iter().enumerate() {
                cur_layer.use_text(*h, FONT_SMALL - 0.5, Mm(vt_cols[i]), Mm(y), &font_bold);
            }
            y -= LINE_H - 1.0;
            draw_hline(&cur_layer, MARGIN, MARGIN + 79.0, y + 1.0);
            for ((rate_key, category), (base, vat)) in &vat_groups {
                // Guard between VAT rows in case there are many groups
                if y < MARGIN + LINE_H {
                    let (new_page, new_layer_idx) = doc.add_page(Mm(PAGE_W), Mm(PAGE_H), "Layer 1");
                    cur_layer = doc.get_page(new_page).get_layer(new_layer_idx);
                    y = PAGE_H - MARGIN;
                }
                let rate_pct = *rate_key as f64 / 100.0;
                let label = vat_label(rate_pct, category);
                cur_layer.use_text(label, FONT_SMALL, Mm(vt_cols[0]), Mm(y), &font_normal);
                cur_layer.use_text(
                    format!("{:.2} {}", base, inv.currency),
                    FONT_SMALL,
                    Mm(vt_cols[1]),
                    Mm(y),
                    &font_normal,
                );
                cur_layer.use_text(
                    format!("{:.2} {}", vat, inv.currency),
                    FONT_SMALL,
                    Mm(vt_cols[2]),
                    Mm(y),
                    &font_normal,
                );
                y -= LINE_H - 1.0;
            }
        }
    }

    // Guard before totals block (subtotal + TVA + TOTAL + words = ~4 lines)
    if y < MARGIN + 4.0 * LINE_H {
        let (new_page, new_layer_idx) = doc.add_page(Mm(PAGE_W), Mm(PAGE_H), "Layer 1");
        cur_layer = doc.get_page(new_page).get_layer(new_layer_idx);
        y = PAGE_H - MARGIN;
    }

    // ── Totals (right side) ───────────────────────────────────────────────────
    let totals_x = PAGE_W - MARGIN - 70.0;
    cur_layer.use_text(
        format!("Subtotal: {} {}", inv.subtotal_amount, inv.currency),
        FONT_NORMAL,
        Mm(totals_x),
        Mm(y),
        &font_normal,
    );
    y -= LINE_H;
    cur_layer.use_text(
        format!("TVA: {} {}", inv.vat_amount, inv.currency),
        FONT_NORMAL,
        Mm(totals_x),
        Mm(y),
        &font_normal,
    );
    y -= LINE_H;
    cur_layer.use_text(
        format!("TOTAL: {} {}", inv.total_amount, inv.currency),
        FONT_HEADING,
        Mm(totals_x),
        Mm(y),
        &font_bold,
    );
    y -= LINE_H;

    // Total în cuvinte (Romanian words) — plan Task 5.3. Opțional din șablon (show_words).
    // BIZ-21: pass Decimal directly to preserve exact cents (no f64 round-trip).
    if template.show_words {
        let total_dec = Decimal::from_str(&inv.total_amount).unwrap_or(Decimal::ZERO);
        let words = amount_to_romanian_words(total_dec);
        cur_layer.use_text(
            format!("({words})"),
            FONT_SMALL,
            Mm(totals_x),
            Mm(y),
            &font_normal,
        );
    }

    // Notes — STORNO_OF: prefix is replaced with a human-readable label
    if let Some(notes) = &inv.notes {
        let display_notes = if let Some(rest) = notes.strip_prefix("STORNO_OF:") {
            let number = rest.split('|').next().unwrap_or(rest);
            format!("Storno factura {}", number)
        } else {
            notes.clone()
        };
        if !display_notes.is_empty() {
            y -= 10.0;
            // Guard before notes block
            if y < MARGIN + 2.0 * LINE_H {
                let (new_page, new_layer_idx) = doc.add_page(Mm(PAGE_W), Mm(PAGE_H), "Layer 1");
                cur_layer = doc.get_page(new_page).get_layer(new_layer_idx);
                y = PAGE_H - MARGIN;
            }
            cur_layer.use_text("Note:", FONT_NORMAL, Mm(MARGIN), Mm(y), &font_bold);
            y -= LINE_H;
            cur_layer.use_text(display_notes, FONT_SMALL, Mm(MARGIN), Mm(y), &font_normal);
        }
    }

    // Nota de subsol personalizată din șablon (mulțumiri / termeni de plată) — max 3 rânduri,
    // separată printr-o linie subțire. Apare pe ultima pagină, după blocul Note.
    if !template.footer_note.trim().is_empty() {
        y -= 10.0;
        let needed = 3.0 + template.footer_note.lines().take(3).count() as f32 * 4.5;
        if y < MARGIN + needed {
            let (new_page, new_layer_idx) = doc.add_page(Mm(PAGE_W), Mm(PAGE_H), "Layer 1");
            cur_layer = doc.get_page(new_page).get_layer(new_layer_idx);
            y = PAGE_H - MARGIN;
        }
        draw_hline(&cur_layer, MARGIN, PAGE_W - MARGIN, y + 2.0);
        y -= 2.0;
        for note_line in template.footer_note.lines().take(3) {
            let note_line = note_line.trim();
            if note_line.is_empty() {
                continue;
            }
            cur_layer.use_text(
                truncate(note_line, 120),
                FONT_SMALL,
                Mm(MARGIN),
                Mm(y),
                &font_normal,
            );
            y -= 4.5;
        }
    }

    let _ = y; // silence unused-variable warning after the trailing blocks

    doc.save_to_bytes()
        .map_err(|e| AppError::Pdf(e.to_string()))
}

// ─── Logo embedding (defensive) ───────────────────────────────────────────────

/// Try to embed the seller logo in the top-right of the header.
/// Any failure (missing file, decode error, unsupported format) is silently
/// ignored with a `tracing::warn!` — this function never returns an error.
fn try_embed_logo(
    _doc: &PdfDocumentReference,
    layer: &PdfLayerReference,
    seller: &Company,
    header_top_y: f32,
) {
    let path = match &seller.logo_path {
        Some(p) if !p.is_empty() => p.clone(),
        _ => return,
    };

    if !std::path::Path::new(&path).exists() {
        tracing::warn!("Invoice logo file not found: {}", path);
        return;
    }

    // image 0.24: ImageReader is at image::io::Reader.
    // with_guessed_format() may return std::io::Error; flatten both into ImageError.
    let dyn_img = match (|| -> Result<img_crate::DynamicImage, img_crate::ImageError> {
        let reader = img_crate::io::Reader::open(&path).map_err(img_crate::ImageError::IoError)?;
        let reader = reader
            .with_guessed_format()
            .map_err(img_crate::ImageError::IoError)?;
        reader.decode()
    })() {
        Ok(img) => img,
        Err(e) => {
            tracing::warn!("Invoice logo decode failed ({}): {}", path, e);
            return;
        }
    };

    // Target max logo width: 32 mm at 300 DPI.
    // At 300 DPI, 1 mm = 300/25.4 px ≈ 11.81 px.
    const DPI: f32 = 300.0;
    const MAX_W_MM: f32 = 32.0;
    let px_per_mm = DPI / 25.4;
    let max_w_px = MAX_W_MM * px_per_mm;

    let img_w_px = dyn_img.width() as f32;
    let scale = if img_w_px > max_w_px {
        max_w_px / img_w_px
    } else {
        1.0
    };

    let logo_w_mm = img_w_px * scale / px_per_mm;

    // Place logo in top-right corner, same Y baseline as the header.
    let logo_x_mm = PAGE_W - MARGIN - logo_w_mm;
    let logo_y_mm = header_top_y - 8.0; // offset to sit near seller name line

    let pdf_image = Image::from_dynamic_image(&dyn_img);
    pdf_image.add_to_layer(
        layer.clone(),
        ImageTransform {
            translate_x: Some(Mm(logo_x_mm)),
            translate_y: Some(Mm(logo_y_mm)),
            scale_x: Some(scale),
            scale_y: Some(scale),
            dpi: Some(DPI),
            ..Default::default()
        },
    );
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn write_seller_block(
    layer: &PdfLayerReference,
    font: &IndirectFontRef,
    font_bold: &IndirectFontRef,
    seller: &Company,
    x: f32,
    mut y: f32,
) -> f32 {
    layer.use_text(
        seller.legal_name.clone(),
        FONT_NORMAL,
        Mm(x),
        Mm(y),
        font_bold,
    );
    y -= LINE_H;
    layer.use_text(
        format!("CUI: {}", seller.cui),
        FONT_NORMAL,
        Mm(x),
        Mm(y),
        font,
    );
    y -= LINE_H;
    if let Some(reg) = &seller.registry_number {
        layer.use_text(
            format!("Reg. Com.: {}", reg),
            FONT_NORMAL,
            Mm(x),
            Mm(y),
            font,
        );
        y -= LINE_H;
    }
    layer.use_text(seller.address.clone(), FONT_NORMAL, Mm(x), Mm(y), font);
    y -= LINE_H;
    layer.use_text(
        format!("{}, {}, {}", seller.city, seller.county, seller.country),
        FONT_NORMAL,
        Mm(x),
        Mm(y),
        font,
    );
    y -= LINE_H;
    if let Some(iban) = &seller.iban {
        let bank_str = if let Some(bank) = &seller.bank_name {
            format!("IBAN: {} ({})", iban, bank)
        } else {
            format!("IBAN: {}", iban)
        };
        layer.use_text(bank_str, FONT_SMALL, Mm(x), Mm(y), font);
        y -= LINE_H;
    }
    y
}

fn write_buyer_block(
    layer: &PdfLayerReference,
    font: &IndirectFontRef,
    buyer: &Contact,
    x: f32,
    mut y: f32,
) {
    layer.use_text(buyer.legal_name.clone(), FONT_NORMAL, Mm(x), Mm(y), font);
    y -= LINE_H;
    if let Some(cui) = &buyer.cui {
        layer.use_text(format!("CUI: {}", cui), FONT_NORMAL, Mm(x), Mm(y), font);
        y -= LINE_H;
    }
    if let Some(addr) = &buyer.address {
        layer.use_text(addr.clone(), FONT_NORMAL, Mm(x), Mm(y), font);
        y -= LINE_H;
    }
    let city_line = format!(
        "{}, {}",
        buyer.city.as_deref().unwrap_or(""),
        buyer.county.as_deref().unwrap_or("")
    );
    if city_line.trim_matches([',', ' '].as_ref()).is_empty() {
        return;
    }
    layer.use_text(city_line, FONT_NORMAL, Mm(x), Mm(y), font);
}

fn write_line_row(
    layer: &PdfLayerReference,
    font: &IndirectFontRef,
    line: &LineItem,
    col_x: &[f32],
    y: f32,
    currency: &str,
) {
    let values = [
        line.position.to_string(),
        truncate(&line.name, 28),
        line.unit.clone(),
        line.quantity.clone(),
        format!("{} {}", line.unit_price, currency),
        format!(
            "{:.0}%",
            Decimal::from_str(&line.vat_rate)
                .unwrap_or(Decimal::ZERO)
                .to_f64()
                .unwrap_or(0.0)
        ),
        format!("{} {}", line.subtotal_amount, currency),
    ];
    for (i, val) in values.iter().enumerate() {
        layer.use_text(val.clone(), FONT_SMALL, Mm(col_x[i]), Mm(y), font);
    }
}

/// BIZ-19: produces a human-readable label for a (rate, vat_category) tuple
/// used in the PDF VAT breakdown. Distinguishes the various 0% categories.
fn vat_label(rate_pct: f64, category: &str) -> String {
    match category {
        "S" => format!("{:.0}%", rate_pct),
        "Z" => "0% (cotă zero)".to_string(),
        "E" => "0% (scutit)".to_string(),
        "AE" => "0% (taxare inversă)".to_string(),
        "K" => "0% (intracomunitar)".to_string(),
        "G" => "0% (export)".to_string(),
        "O" => "0% (în afara sferei)".to_string(),
        _ => format!("{:.0}% ({})", rate_pct, category),
    }
}

/// Coordonate X pentru coloanele tabelului.
fn col_positions() -> [f32; 7] {
    [
        MARGIN,         // Nr
        MARGIN + 8.0,   // Denumire
        MARGIN + 65.0,  // UM
        MARGIN + 75.0,  // Cant
        MARGIN + 90.0,  // Pret unitar
        MARGIN + 120.0, // TVA%
        MARGIN + 135.0, // Valoare
    ]
}

/// Trunchiază un string la `max` caractere.
fn truncate(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        let cut: String = chars[..max.saturating_sub(1)].iter().collect();
        format!("{}…", cut)
    }
}

/// Desenează o linie orizontală.
fn draw_hline(layer: &PdfLayerReference, x1: f32, x2: f32, y: f32) {
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

// ─── amount_to_romanian_words ─────────────────────────────────────────────────

/// Convertește o sumă în lei + bani în cuvinte românești.
/// Ex: 425.50 → "Patru sute douăzeci și cinci lei și 50 bani"
/// Planul specifică această funcție în Task 5.3 (PDF generation).
///
/// BIZ-21: operates directly on `Decimal` to avoid f64 precision loss on
/// fractional amounts (e.g. 1234.99 must yield exactly "99 bani").
pub fn amount_to_romanian_words(amount: Decimal) -> String {
    // Work on the absolute value rounded to 2 decimals (1 ban precision).
    let amount = amount
        .abs()
        .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero);
    let lei = amount.trunc().to_u64().unwrap_or(0);
    let bani = ((amount.fract() * Decimal::from(100u32)).round())
        .to_u64()
        .unwrap_or(0);

    let lei_str = if lei == 0 {
        "zero lei".to_string()
    } else {
        let words = number_to_words_ro(lei);
        // Capitalize first letter
        let mut chars = words.chars();
        let capitalized = chars
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default()
            + chars.as_str();
        format!("{} lei", capitalized)
    };

    if bani > 0 {
        format!("{} și {} bani", lei_str, bani)
    } else {
        lei_str
    }
}

fn number_to_words_ro(n: u64) -> String {
    if n == 0 {
        return "zero".to_string();
    }

    const ONES: &[&str] = &[
        "",
        "unu",
        "doi",
        "trei",
        "patru",
        "cinci",
        "șase",
        "șapte",
        "opt",
        "nouă",
        "zece",
        "unsprezece",
        "doisprezece",
        "treisprezece",
        "paisprezece",
        "cincisprezece",
        "șaisprezece",
        "șaptesprezece",
        "optsprezece",
        "nouăsprezece",
    ];
    const TENS: &[&str] = &[
        "",
        "",
        "douăzeci",
        "treizeci",
        "patruzeci",
        "cincizeci",
        "șaizeci",
        "șaptezeci",
        "optzeci",
        "nouăzeci",
    ];

    if n < 20 {
        return ONES[n as usize].to_string();
    }
    if n < 100 {
        let t = TENS[(n / 10) as usize];
        let r = n % 10;
        if r == 0 {
            return t.to_string();
        }
        return format!("{} și {}", t, ONES[r as usize]);
    }
    if n < 1_000 {
        let h = n / 100;
        let rest = n % 100;
        let h_word = match h {
            1 => "o sută".to_string(),
            2 => "două sute".to_string(),
            _ => format!("{} sute", ONES[h as usize]),
        };
        if rest == 0 {
            return h_word;
        }
        return format!("{} {}", h_word, number_to_words_ro(rest));
    }
    if n < 1_000_000 {
        let th = n / 1_000;
        let rest = n % 1_000;
        let th_word = match th {
            1 => "o mie".to_string(),
            2 => "două mii".to_string(),
            _ => format!("{} mii", number_to_words_ro(th)),
        };
        if rest == 0 {
            return th_word;
        }
        return format!("{} {}", th_word, number_to_words_ro(rest));
    }
    if n < 1_000_000_000 {
        let mil = n / 1_000_000;
        let rest = n % 1_000_000;
        let mil_word = match mil {
            1 => "un milion".to_string(),
            _ => format!("{} milioane", number_to_words_ro(mil)),
        };
        if rest == 0 {
            return mil_word;
        }
        return format!("{} {}", mil_word, number_to_words_ro(rest));
    }
    // For amounts >= 1 billion, just return the number
    n.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::companies::Company;
    use crate::db::contacts::Contact;
    use crate::db::invoices::{Invoice, LineItem};

    fn sample_input() -> GeneratorInput {
        let seller = Company {
            id: "company-1".to_string(),
            cui: "RO12345678".to_string(),
            legal_name: "Test SRL".to_string(),
            trade_name: None,
            registry_number: None,
            vat_payer: true,
            cash_vat: false,
            address: "Str. Exemplu nr. 1".to_string(),
            city: "București".to_string(),
            county: "Sector 1".to_string(),
            postal_code: None,
            country: "RO".to_string(),
            email: None,
            phone: None,
            iban: None,
            bank_name: None,
            is_active: true,
            spv_enabled: false,
            tax_regime: "micro".into(),
            invoice_series: "FAC".to_string(),
            last_invoice_number: 1,
            logo_path: None,
            created_at: 0,
            updated_at: 0,
        };

        let buyer = Contact {
            id: "contact-1".to_string(),
            company_id: "company-1".to_string(),
            contact_type: "CUSTOMER".to_string(),
            cui: Some("RO87654321".to_string()),
            legal_name: "Client SRL".to_string(),
            vat_payer: true,
            cash_vat: false,
            is_individual: false,
            address: Some("Str. Client nr. 2".to_string()),
            city: Some("Cluj-Napoca".to_string()),
            county: Some("Cluj".to_string()),
            country: "RO".to_string(),
            email: None,
            phone: None,
            currency: None,
            iban: None,
            bank_name: None,
            swift: None,
            payment_term_days: None,
            created_at: 0,
            updated_at: 0,
        };

        let invoice = Invoice {
            id: "invoice-1".to_string(),
            company_id: "company-1".to_string(),
            contact_id: "contact-1".to_string(),
            series: "FAC".to_string(),
            number: 1,
            full_number: "FAC-2024-0001".to_string(),
            issue_date: "2024-01-15".to_string(),
            due_date: "2024-02-15".to_string(),
            currency: "RON".to_string(),
            exchange_rate: None,
            subtotal_amount: "100.00".to_string(),
            vat_amount: "19.00".to_string(),
            total_amount: "119.00".to_string(),
            status: "DRAFT".to_string(),
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
            payment_means_code: "30".to_string(),
            storno_of_invoice_id: None,
            created_at: 0,
            updated_at: 0,
        };

        let line = LineItem {
            id: "line-1".to_string(),
            invoice_id: "invoice-1".to_string(),
            position: 1,
            name: "Serviciu consultanță".to_string(),
            description: None,
            quantity: "1.00".to_string(),
            unit: "H87".to_string(),
            unit_price: "100.00".to_string(),
            vat_rate: "19.00".to_string(),
            vat_category: "S".to_string(),
            subtotal_amount: "100.00".to_string(),
            vat_amount: "19.00".to_string(),
            total_amount: "119.00".to_string(),
            cpv_code: None,
            art331_code: None,
            revenue_kind: "goods".into(),
        };

        GeneratorInput {
            invoice,
            lines: vec![line],
            seller,
            buyer,
            storno_ref: None,
        }
    }

    // BIZ-21: amount-in-words must preserve exact decimal cents
    // (the old f64-based path lost precision on values like 1234.99).
    #[test]
    fn amount_in_words_uses_exact_decimal() {
        let val = Decimal::from_str("1234.99").unwrap();
        let words = amount_to_romanian_words(val);
        assert!(
            words.contains("99 bani"),
            "expected '99 bani' in output, got: {words}"
        );
    }

    #[test]
    fn amount_in_words_handles_whole_lei() {
        let val = Decimal::from_str("100.00").unwrap();
        let words = amount_to_romanian_words(val);
        assert!(
            !words.contains("bani"),
            "no bani suffix expected when fraction is zero, got: {words}"
        );
        assert!(
            words.to_lowercase().contains("lei"),
            "expected 'lei' in output, got: {words}"
        );
    }

    // ── Template PDF tests ────────────────────────────────────────────────────

    /// (a) Default template (clasic) generates valid PDF bytes.
    #[test]
    fn pdf_clasic_template_generates_ok() {
        let input = sample_input();
        let result = generate_pdf(&input, &InvoiceTemplate::default());
        let bytes = result.expect("clasic template must succeed");
        assert!(!bytes.is_empty(), "PDF must not be empty");
        assert!(
            bytes.starts_with(b"%PDF"),
            "PDF must start with %PDF header"
        );
    }

    /// (b) Modern template with accent colour generates valid PDF bytes.
    #[test]
    fn pdf_modern_template_generates_ok() {
        let input = sample_input();
        let tmpl = InvoiceTemplate {
            preset: "modern".into(),
            accent_hex: "#1a73e8".into(),
            ..Default::default()
        };
        let result = generate_pdf(&input, &tmpl);
        let bytes = result.expect("modern template must succeed");
        assert!(!bytes.is_empty(), "PDF must not be empty");
        assert!(bytes.starts_with(b"%PDF"), "must be a valid PDF");
    }

    /// Template knobs: header/footer notes render and toggles change the output.
    #[test]
    fn pdf_template_knobs_render() {
        let input = sample_input();
        let full = generate_pdf(
            &input,
            &InvoiceTemplate {
                header_note: "Capital social: 200 lei · J12/345/2020".into(),
                footer_note: "Vă mulțumim pentru colaborare!\nPlata în 15 zile de la emitere."
                    .into(),
                ..Default::default()
            },
        )
        .expect("template with notes must succeed");
        assert!(full.starts_with(b"%PDF"));

        let compact = generate_pdf(
            &input,
            &InvoiceTemplate {
                show_words: false,
                show_vat_detail: false,
                ..Default::default()
            },
        )
        .expect("compact template must succeed");
        let default = generate_pdf(&input, &InvoiceTemplate::default()).unwrap();
        // The toggles must actually remove content (compact strictly smaller than default).
        assert!(
            compact.len() < default.len(),
            "compact ({}) should be smaller than default ({})",
            compact.len(),
            default.len()
        );
    }

    /// The preview sample builder produces a renderable demo invoice.
    #[test]
    fn sample_preview_input_renders() {
        let seller = sample_input().seller;
        let input = sample_preview_input(seller);
        let bytes = generate_pdf(&input, &InvoiceTemplate::default()).expect("preview renders");
        assert!(bytes.starts_with(b"%PDF"));
    }

    /// (c) Logo path pointing to a tiny PNG — logo is embedded, no error.
    #[test]
    fn pdf_with_valid_logo_path_ok() {
        use std::io::Write;

        // Write a minimal 2x2 RGBA PNG to a tempfile.
        let mut tmpfile = tempfile::NamedTempFile::new().expect("tempfile");
        // 2x2 white PNG, generated via image crate
        let raw_img = img_crate::RgbaImage::from_pixel(2, 2, img_crate::Rgba([255, 255, 255, 255]));
        let dyn_img = img_crate::DynamicImage::ImageRgba8(raw_img);
        let mut png_bytes: Vec<u8> = Vec::new();
        dyn_img
            .write_to(
                &mut std::io::Cursor::new(&mut png_bytes),
                img_crate::ImageFormat::Png,
            )
            .expect("write PNG");
        tmpfile.write_all(&png_bytes).expect("write tmpfile");
        let path = tmpfile.path().to_str().unwrap().to_string();

        let mut input = sample_input();
        input.seller.logo_path = Some(path);

        let result = generate_pdf(&input, &InvoiceTemplate::default());
        let bytes = result.expect("PDF with logo must succeed");
        assert!(!bytes.is_empty());
        assert!(bytes.starts_with(b"%PDF"));
    }

    /// (d) Non-existent logo path — PDF still generated without error.
    #[test]
    fn pdf_with_nonexistent_logo_path_ok() {
        let mut input = sample_input();
        input.seller.logo_path = Some("/nonexistent/x.png".to_string());

        let result = generate_pdf(&input, &InvoiceTemplate::default());
        let bytes = result.expect("PDF with missing logo must still succeed (graceful fallback)");
        assert!(!bytes.is_empty());
        assert!(bytes.starts_with(b"%PDF"));
    }
}
