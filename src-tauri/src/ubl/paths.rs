//! Helpers pentru calculul căilor de fişiere XML / PDF.

use std::path::PathBuf;
use tauri::AppHandle;
use tauri::Manager;

/// Returnează: `{app_data_dir}/invoices/{company_id}/{invoice_id}.xml`
/// Creează directorul dacă nu există.
pub fn xml_path(handle: &AppHandle, company_id: &str, invoice_id: &str) -> PathBuf {
    let dir = invoices_dir(handle, company_id);
    std::fs::create_dir_all(&dir).ok();
    dir.join(format!("{invoice_id}.xml"))
}

/// Returnează: `{app_data_dir}/invoices/{company_id}/{invoice_id}.pdf`
/// Creează directorul dacă nu există.
pub fn pdf_path(handle: &AppHandle, company_id: &str, invoice_id: &str) -> PathBuf {
    let dir = invoices_dir(handle, company_id);
    std::fs::create_dir_all(&dir).ok();
    dir.join(format!("{invoice_id}.pdf"))
}

fn invoices_dir(handle: &AppHandle, company_id: &str) -> PathBuf {
    handle
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("invoices")
        .join(company_id)
}
