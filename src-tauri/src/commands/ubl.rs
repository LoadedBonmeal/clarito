//! Comenzi Tauri pentru generarea XML UBL şi PDF.

use tauri::AppHandle;
use tauri::State;

use crate::db::{companies, contacts, invoices};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::ubl::generator::{generate_ubl, GeneratorInput};
use crate::ubl::pdf::generate_pdf;
use crate::ubl::validator::{validate_ubl, ValidationResult};
use crate::ubl::paths;

#[tauri::command]
pub async fn generate_invoice_xml(
    state: State<'_, AppState>,
    app: AppHandle,
    invoice_id: String,
) -> AppResult<String> {
    // 1. Încarcă factura cu linii
    let with_lines = invoices::get_with_lines(&state.db, &invoice_id).await?;
    let inv = with_lines.invoice;
    let lines = with_lines.lines;

    // 2. Încarcă furnizorul
    let seller = companies::get(&state.db, &inv.company_id).await?;

    // 3. Încarcă cumpărătorul
    let buyer = contacts::get(&state.db, &inv.contact_id).await?;

    // 4. Determină referința storno (dacă există)
    let storno_ref = inv.notes.as_deref().and_then(|n| {
        n.strip_prefix("STORNO_OF:").map(|orig| orig.to_string())
    });

    // 5. Generează XML
    let input = GeneratorInput {
        invoice: inv.clone(),
        lines,
        seller,
        buyer,
        storno_ref,
    };
    let xml = generate_ubl(&input)?;

    // 6. Calculează calea şi scrie fişierul
    let path = paths::xml_path(&app, &inv.company_id, &invoice_id);
    std::fs::write(&path, xml.as_bytes())?;

    // 7. Actualizează DB
    let path_str = path
        .to_str()
        .ok_or_else(|| AppError::Xml("Cale fişier invalidă UTF-8".to_string()))?;
    invoices::set_xml_path(&state.db, &invoice_id, path_str).await?;

    Ok(path_str.to_string())
}

#[tauri::command]
pub async fn generate_invoice_pdf(
    state: State<'_, AppState>,
    app: AppHandle,
    invoice_id: String,
) -> AppResult<String> {
    // 1. Încarcă factura cu linii
    let with_lines = invoices::get_with_lines(&state.db, &invoice_id).await?;
    let inv = with_lines.invoice;
    let lines = with_lines.lines;

    // 2. Încarcă furnizorul
    let seller = companies::get(&state.db, &inv.company_id).await?;

    // 3. Încarcă cumpărătorul
    let buyer = contacts::get(&state.db, &inv.contact_id).await?;

    // 4. Generează PDF
    let input = GeneratorInput {
        invoice: inv.clone(),
        lines,
        seller,
        buyer,
        storno_ref: None,
    };
    let pdf_bytes = generate_pdf(&input)?;

    // 5. Calculează calea şi scrie fişierul
    let path = paths::pdf_path(&app, &inv.company_id, &invoice_id);
    std::fs::write(&path, &pdf_bytes)?;

    // 6. Actualizează DB
    let path_str = path
        .to_str()
        .ok_or_else(|| AppError::Pdf("Cale fişier invalidă UTF-8".to_string()))?;
    invoices::set_pdf_path(&state.db, &invoice_id, path_str).await?;

    Ok(path_str.to_string())
}

#[tauri::command]
pub async fn validate_invoice_xml(
    state: State<'_, AppState>,
    invoice_id: String,
) -> AppResult<ValidationResult> {
    // 1. Obţine calea XML din DB
    let with_lines = invoices::get_with_lines(&state.db, &invoice_id).await?;
    let xml_path = with_lines.invoice.xml_path.ok_or_else(|| {
        AppError::Validation("XML nu a fost generat încă pentru această factură.".to_string())
    })?;

    // 2. Citeşte fişierul
    let xml = std::fs::read_to_string(&xml_path)?;

    // 3. Validează
    Ok(validate_ubl(&xml))
}
