//! Generare PDF simplu pentru factură folosind `printpdf 0.7`.

use printpdf::*;

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

// Liberation Sans embedded at compile-time — supports Romanian diacritics (ș, ț, ă, â, î)
static FONT_REGULAR_BYTES: &[u8] =
    include_bytes!("../../resources/fonts/LiberationSans-Regular.ttf");
static FONT_BOLD_BYTES: &[u8] =
    include_bytes!("../../resources/fonts/LiberationSans-Bold.ttf");

pub fn generate_pdf(input: &GeneratorInput) -> AppResult<Vec<u8>> {
    let (doc, page1, layer1) =
        PdfDocument::new("Factura", Mm(PAGE_W), Mm(PAGE_H), "Layer 1");
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

    // Current Y cursor (counts down from top)
    let mut y: f32 = PAGE_H - MARGIN;

    // ── Header ────────────────────────────────────────────────────────────────
    layer.use_text(
        seller.legal_name.clone(),
        FONT_TITLE,
        Mm(MARGIN),
        Mm(y),
        &font_bold,
    );
    y -= 8.0;

    let title = format!("FACTURA Nr. {}", inv.full_number);
    layer.use_text(title, FONT_TITLE, Mm(MARGIN), Mm(y), &font_bold);
    y -= 6.0;

    let date_line = format!(
        "Data emiterii: {}   Scadenta: {}   Moneda: {}",
        inv.issue_date, inv.due_date, inv.currency
    );
    layer.use_text(date_line, FONT_NORMAL, Mm(MARGIN), Mm(y), &font_normal);
    y -= 8.0;

    // ── Divider ───────────────────────────────────────────────────────────────
    draw_hline(&layer, MARGIN, PAGE_W - MARGIN, y + 2.0);
    y -= 2.0;

    // ── Seller / Buyer blocks ─────────────────────────────────────────────────
    let block_top = y;

    // Seller (left)
    layer.use_text("FURNIZOR", FONT_HEADING, Mm(MARGIN), Mm(y), &font_bold);
    y -= LINE_H;
    y = write_seller_block(&layer, &font_normal, &font_bold, seller, MARGIN, y);

    // Buyer (right) — reset y to block_top
    let mut y_right = block_top;
    layer.use_text(
        "CUMPARATOR",
        FONT_HEADING,
        Mm(COL_MID),
        Mm(y_right),
        &font_bold,
    );
    y_right -= LINE_H;
    write_buyer_block(&layer, &font_normal, buyer, COL_MID, y_right);

    // Advance past both blocks
    y -= 4.0;
    if y > block_top - 30.0 {
        y = block_top - 30.0;
    }

    // ── Line items table ──────────────────────────────────────────────────────
    draw_hline(&layer, MARGIN, PAGE_W - MARGIN, y);
    y -= 5.0;

    // Table header
    let headers = ["Nr", "Denumire", "UM", "Cant", "Pret unitar", "TVA%", "Valoare"];
    let col_x = col_positions();
    for (i, h) in headers.iter().enumerate() {
        layer.use_text(*h, FONT_SMALL, Mm(col_x[i]), Mm(y), &font_bold);
    }
    y -= LINE_H;
    draw_hline(&layer, MARGIN, PAGE_W - MARGIN, y + 1.0);

    // Table rows
    for line in &input.lines {
        y -= LINE_H;
        write_line_row(&layer, &font_normal, line, &col_x, y, &inv.currency);
    }

    y -= 2.0;
    draw_hline(&layer, MARGIN, PAGE_W - MARGIN, y);
    y -= 6.0;

    // ── VAT breakdown table (left side) ──────────────────────────────────────
    {
        let mut vat_groups: std::collections::BTreeMap<u32, (f64, f64)> = std::collections::BTreeMap::new();
        for line in &input.lines {
            let rate_key = (line.vat_rate * 100.0).round() as u32;
            let entry = vat_groups.entry(rate_key).or_insert((0.0, 0.0));
            entry.0 += line.subtotal_amount;
            entry.1 += line.vat_amount;
        }
        if !vat_groups.is_empty() {
            layer.use_text("Detaliu TVA:", FONT_SMALL, Mm(MARGIN), Mm(y), &font_bold);
            y -= LINE_H - 1.0;
            let hdrs = ["Cotă", "Bază impozabilă", "TVA"];
            let vt_cols = [MARGIN, MARGIN + 18.0, MARGIN + 50.0];
            for (i, h) in hdrs.iter().enumerate() {
                layer.use_text(*h, FONT_SMALL - 0.5, Mm(vt_cols[i]), Mm(y), &font_bold);
            }
            y -= LINE_H - 1.0;
            draw_hline(&layer, MARGIN, MARGIN + 75.0, y + 1.0);
            for (rate_key, (base, vat)) in &vat_groups {
                let rate_pct = *rate_key as f64 / 100.0;
                layer.use_text(format!("{:.0}%", rate_pct), FONT_SMALL, Mm(vt_cols[0]), Mm(y), &font_normal);
                layer.use_text(format!("{:.2} {}", base, inv.currency), FONT_SMALL, Mm(vt_cols[1]), Mm(y), &font_normal);
                layer.use_text(format!("{:.2} {}", vat, inv.currency), FONT_SMALL, Mm(vt_cols[2]), Mm(y), &font_normal);
                y -= LINE_H - 1.0;
            }
        }
    }

    // ── Totals (right side) ───────────────────────────────────────────────────
    let totals_x = PAGE_W - MARGIN - 70.0;
    layer.use_text(
        format!("Subtotal: {:.2} {}", inv.subtotal_amount, inv.currency),
        FONT_NORMAL,
        Mm(totals_x),
        Mm(y),
        &font_normal,
    );
    y -= LINE_H;
    layer.use_text(
        format!("TVA: {:.2} {}", inv.vat_amount, inv.currency),
        FONT_NORMAL,
        Mm(totals_x),
        Mm(y),
        &font_normal,
    );
    y -= LINE_H;
    layer.use_text(
        format!("TOTAL: {:.2} {}", inv.total_amount, inv.currency),
        FONT_HEADING,
        Mm(totals_x),
        Mm(y),
        &font_bold,
    );
    y -= LINE_H;

    // Total în cuvinte (Romanian words) — plan Task 5.3
    let words = amount_to_romanian_words(inv.total_amount);
    layer.use_text(
        format!("({words})"),
        FONT_SMALL,
        Mm(totals_x),
        Mm(y),
        &font_normal,
    );

    // Notes
    if let Some(notes) = &inv.notes {
        if !notes.is_empty() {
            y -= 10.0;
            layer.use_text("Note:", FONT_NORMAL, Mm(MARGIN), Mm(y), &font_bold);
            y -= LINE_H;
            layer.use_text(notes.clone(), FONT_SMALL, Mm(MARGIN), Mm(y), &font_normal);
        }
    }

    doc.save_to_bytes()
        .map_err(|e| AppError::Pdf(e.to_string()))
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
        format!("{:.2}", line.quantity),
        format!("{:.2} {}", line.unit_price, currency),
        format!("{:.0}%", line.vat_rate),
        format!("{:.2} {}", line.subtotal_amount, currency),
    ];
    for (i, val) in values.iter().enumerate() {
        layer.use_text(val.clone(), FONT_SMALL, Mm(col_x[i]), Mm(y), font);
    }
}

/// Coordonate X pentru coloanele tabelului.
fn col_positions() -> [f32; 7] {
    [
        MARGIN,       // Nr
        MARGIN + 8.0, // Denumire
        MARGIN + 65.0, // UM
        MARGIN + 75.0, // Cant
        MARGIN + 90.0, // Pret unitar
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
pub fn amount_to_romanian_words(amount: f64) -> String {
    let total_bani = (amount * 100.0).round() as u64;
    let lei = total_bani / 100;
    let bani = total_bani % 100;

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
        "", "unu", "doi", "trei", "patru", "cinci",
        "șase", "șapte", "opt", "nouă", "zece",
        "unsprezece", "doisprezece", "treisprezece", "paisprezece", "cincisprezece",
        "șaisprezece", "șaptesprezece", "optsprezece", "nouăsprezece",
    ];
    const TENS: &[&str] = &[
        "", "", "douăzeci", "treizeci", "patruzeci",
        "cincizeci", "șaizeci", "șaptezeci", "optzeci", "nouăzeci",
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
