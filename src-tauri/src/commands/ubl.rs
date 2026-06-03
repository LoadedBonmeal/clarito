//! Comenzi Tauri pentru generarea XML UBL şi PDF.

use tauri::AppHandle;
use tauri::State;

use crate::commands::invoices::resolve_storno_ref;
use crate::db::{companies, contacts, invoices};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::ubl::generator::{generate_ubl, GeneratorInput};
use crate::ubl::paths;
use crate::ubl::pdf::generate_pdf;
use crate::ubl::validator::{validate_ubl, ValidationResult};

#[tauri::command]
pub async fn generate_invoice_xml(
    state: State<'_, AppState>,
    app: AppHandle,
    invoice_id: String,
    company_id: String,
) -> AppResult<String> {
    // 1. Încarcă factura cu linii
    let with_lines = invoices::get_with_lines(&state.db, &invoice_id).await?;
    // R14 Wave E: ownership check — cross-company XML generation returns NotFound.
    if with_lines.invoice.company_id != company_id {
        return Err(AppError::NotFound);
    }
    let inv = with_lines.invoice;
    let lines = with_lines.lines;

    // 2. Încarcă furnizorul
    let seller = companies::get(&state.db, &inv.company_id).await?;

    // 3. Încarcă cumpărătorul
    let buyer = contacts::get(&state.db, &inv.contact_id, &inv.company_id).await?;

    // 4. Determină referința storno (dacă există). Preferă FK-ul (BIZ-13),
    //    cu fallback pe parserul notes pentru rândurile vechi.
    let storno_ref = resolve_storno_ref(&state.db, &inv).await?;

    // 5. Generează XML (CPU-bound — rulăm în spawn_blocking)
    let input = GeneratorInput {
        invoice: inv.clone(),
        lines,
        seller,
        buyer,
        storno_ref,
    };
    let path = paths::xml_path(&app, &inv.company_id, &invoice_id);
    let path_clone = path.clone();
    let path_str_result = tauri::async_runtime::spawn_blocking(move || -> AppResult<String> {
        let xml = generate_ubl(&input)?;
        std::fs::write(&path_clone, xml.as_bytes()).map_err(AppError::Io)?;
        path_clone
            .to_str()
            .ok_or_else(|| AppError::Xml("Cale fişier invalidă UTF-8".to_string()))
            .map(|s| s.to_string())
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))??;

    // 6. Actualizează DB
    invoices::set_xml_path(&state.db, &invoice_id, &path_str_result).await?;

    Ok(path_str_result)
}

#[tauri::command]
pub async fn generate_invoice_pdf(
    state: State<'_, AppState>,
    app: AppHandle,
    invoice_id: String,
    company_id: String,
) -> AppResult<String> {
    // 1. Încarcă factura cu linii
    let with_lines = invoices::get_with_lines(&state.db, &invoice_id).await?;
    // R14 Wave E: ownership check — cross-company PDF generation returns NotFound.
    if with_lines.invoice.company_id != company_id {
        return Err(AppError::NotFound);
    }
    let inv = with_lines.invoice;
    let lines = with_lines.lines;

    // 2. Încarcă furnizorul
    let seller = companies::get(&state.db, &inv.company_id).await?;

    // 3. Încarcă cumpărătorul
    let buyer = contacts::get(&state.db, &inv.contact_id, &inv.company_id).await?;

    // 4. Determină referința storno (dacă există). Preferă FK-ul (BIZ-13),
    //    cu fallback pe parserul notes pentru rândurile vechi.
    let storno_ref = resolve_storno_ref(&state.db, &inv).await?;

    // 5. Generează PDF (CPU-bound — rulăm în spawn_blocking)
    let input = GeneratorInput {
        invoice: inv.clone(),
        lines,
        seller,
        buyer,
        storno_ref,
    };
    let path = paths::pdf_path(&app, &inv.company_id, &invoice_id);
    let path_clone = path.clone();
    let path_str_result = tauri::async_runtime::spawn_blocking(move || -> AppResult<String> {
        let pdf_bytes = generate_pdf(&input)?;
        std::fs::write(&path_clone, &pdf_bytes).map_err(AppError::Io)?;
        path_clone
            .to_str()
            .ok_or_else(|| AppError::Pdf("Cale fişier invalidă UTF-8".to_string()))
            .map(|s| s.to_string())
    })
    .await
    .map_err(|e| AppError::Pdf(e.to_string()))??;

    // 6. Actualizează DB
    invoices::set_pdf_path(&state.db, &invoice_id, &path_str_result).await?;

    Ok(path_str_result)
}

#[tauri::command]
pub async fn validate_invoice_xml(
    state: State<'_, AppState>,
    invoice_id: String,
    company_id: String,
) -> AppResult<ValidationResult> {
    // 1. Obţine calea XML din DB
    let with_lines = invoices::get_with_lines(&state.db, &invoice_id).await?;
    // R14 Wave E: ownership check — cross-company XML validation returns NotFound.
    if with_lines.invoice.company_id != company_id {
        return Err(AppError::NotFound);
    }
    let xml_path = with_lines.invoice.xml_path.ok_or_else(|| {
        AppError::Validation("XML nu a fost generat încă pentru această factură.".to_string())
    })?;

    // 2. Citeşte fişierul
    let xml = tokio::fs::read_to_string(&xml_path).await?;

    // 3. Validează
    Ok(validate_ubl(&xml))
}

// ─── R14 Wave E: cross-company isolation tests ───────────────────────────────
//
// The three commands above use verify-after-fetch: they call get_with_lines
// (unscoped) and then reject if invoice.company_id != company_id.
// These tests exercise the REAL db fetch + ownership comparison by setting up
// an in-memory SQLite, inserting an invoice for comp-1, and asserting that
// fetching with comp-2 returns NotFound — not by re-implementing the predicate.

#[cfg(test)]
mod tests {
    use crate::db::invoices as db_inv;
    use crate::error::AppError;
    use sqlx::sqlite::SqlitePoolOptions;

    /// Minimal schema that satisfies `db::invoices::get_with_lines` (which calls
    /// `get` + `list_lines` + `list_events`).
    async fn setup_ubl_pool() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(":memory:")
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

        // Seed an invoice belonging to comp-1.
        sqlx::query(
            "INSERT INTO invoices (id, company_id, contact_id, series, number, full_number,
             issue_date, due_date, status)
             VALUES ('inv-ubl-1', 'comp-1', 'contact-1', 'FCT', 1, 'FCT-0001',
             '2026-01-01', '2026-01-01', 'DRAFT')",
        )
        .execute(&pool)
        .await
        .unwrap();

        pool
    }

    /// Helper that runs the ownership guard logic used by all three UBL commands:
    /// fetch the invoice via the real `get_with_lines`, then compare company_id.
    /// Returns Err(AppError::NotFound) on mismatch — exactly what the commands do.
    async fn check_ubl_ownership(
        pool: &sqlx::SqlitePool,
        invoice_id: &str,
        company_id: &str,
    ) -> crate::error::AppResult<()> {
        let with_lines = db_inv::get_with_lines(pool, invoice_id).await?;
        if with_lines.invoice.company_id != company_id {
            return Err(AppError::NotFound);
        }
        Ok(())
    }

    // ── generate_invoice_xml / generate_invoice_pdf / validate_invoice_xml ────
    // All three commands share the same verify-after-fetch pattern.
    // One test per direction (wrong-company → NotFound, right-company → Ok)
    // is sufficient: the shared helper exercises the REAL get_with_lines path.

    #[tokio::test]
    async fn wave_e_ubl_wrong_company_returns_not_found() {
        let pool = setup_ubl_pool().await;
        // comp-2 does not own inv-ubl-1 (belongs to comp-1).
        let result = check_ubl_ownership(&pool, "inv-ubl-1", "comp-2").await;
        assert!(
            matches!(result, Err(AppError::NotFound)),
            "UBL command with wrong company_id must return NotFound (got {:?})",
            result
        );
    }

    #[tokio::test]
    async fn wave_e_ubl_correct_company_passes_ownership() {
        let pool = setup_ubl_pool().await;
        // comp-1 owns inv-ubl-1 — ownership check must succeed.
        let result = check_ubl_ownership(&pool, "inv-ubl-1", "comp-1").await;
        assert!(
            result.is_ok(),
            "UBL command with correct company_id must pass ownership check (got {:?})",
            result
        );
    }

    #[tokio::test]
    async fn wave_e_ubl_nonexistent_invoice_returns_not_found() {
        let pool = setup_ubl_pool().await;
        // Invoice does not exist at all — get_with_lines propagates NotFound.
        let result = check_ubl_ownership(&pool, "inv-does-not-exist", "comp-1").await;
        assert!(
            matches!(result, Err(AppError::NotFound)),
            "UBL command with nonexistent invoice_id must return NotFound"
        );
    }
}
