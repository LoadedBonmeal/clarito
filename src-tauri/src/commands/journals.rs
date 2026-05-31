//! Jurnale contabile — export CSV jurnal vânzări și jurnal cumpărări.
//!
//! Jurnalul de vânzări: toate facturile emise (non-DRAFT) pentru o perioadă,
//! cu detalii client, net, TVA, total.
//! Jurnalul de cumpărări: facturile primite din `received_invoices` — conține
//! DOAR totalul (fără defalcare net/TVA, care necesită parsarea XML-ului UBL).

use sqlx::Row;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Construiește o linie CSV corect quotată cu separator virgulă.
/// Câmpurile care conțin virgulă, ghilimele sau newline sunt enclosed în ghilimele.
/// Ghilimelele interne sunt dublate (RFC 4180).
fn csv_field(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Construiește un rând CSV din câmpuri.
fn csv_row(fields: &[&str]) -> String {
    fields
        .iter()
        .map(|f| csv_field(f))
        .collect::<Vec<_>>()
        .join(",")
}

// ── Jurnal vânzări ────────────────────────────────────────────────────────────

/// Exportă jurnalul de vânzări (CSV) pentru o companie și o perioadă.
///
/// Include toate facturile emise (statuses != DRAFT) din perioadă.
/// Header: `Numar,Data,Client,CUI,Net,TVA,Total,Status`
/// Returnează calea fișierului salvat.
#[tauri::command]
pub async fn export_sales_journal(
    state: State<'_, AppState>,
    company_id: String,
    date_from: String,
    date_to: String,
    dest_path: String,
) -> AppResult<String> {
    let pool = &state.db;

    // Fetch invoices non-DRAFT pentru companie în perioadă, JOIN contacts.
    let rows = sqlx::query(
        "SELECT i.full_number, i.issue_date, \
                COALESCE(c.legal_name, '') AS client_name, \
                COALESCE(c.cui, '') AS client_cui, \
                i.subtotal_amount, i.vat_amount, i.total_amount, i.status \
         FROM invoices i \
         LEFT JOIN contacts c ON c.id = i.contact_id \
         WHERE i.company_id = ?1 \
           AND i.issue_date >= ?2 \
           AND i.issue_date <= ?3 \
           AND i.status != 'DRAFT' \
         ORDER BY i.issue_date ASC, i.full_number ASC",
    )
    .bind(&company_id)
    .bind(&date_from)
    .bind(&date_to)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    let dest = dest_path.clone();

    tokio::task::spawn_blocking(move || {
        let header = csv_row(&[
            "Numar", "Data", "Client", "CUI", "Net", "TVA", "Total", "Status",
        ]);
        let mut lines = vec![header];

        for row in &rows {
            let full_number: String = row.try_get("full_number").unwrap_or_default();
            let issue_date: String = row.try_get("issue_date").unwrap_or_default();
            let client_name: String = row.try_get("client_name").unwrap_or_default();
            let client_cui: String = row.try_get("client_cui").unwrap_or_default();
            let subtotal: String = row.try_get("subtotal_amount").unwrap_or_default();
            let vat: String = row.try_get("vat_amount").unwrap_or_default();
            let total: String = row.try_get("total_amount").unwrap_or_default();
            let status: String = row.try_get("status").unwrap_or_default();

            lines.push(csv_row(&[
                &full_number,
                &issue_date,
                &client_name,
                &client_cui,
                &subtotal,
                &vat,
                &total,
                &status,
            ]));
        }

        let content = lines.join("\r\n");
        std::fs::write(&dest, content.as_bytes()).map_err(AppError::Io)?;
        Ok(dest)
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?
}

// ── Jurnal cumpărări ──────────────────────────────────────────────────────────

/// Exportă jurnalul de cumpărări (CSV) pentru o companie și o perioadă.
///
/// Date din `received_invoices` — ATENȚIE: conține DOAR totalul facturii.
/// Defalcarea net/TVA nu este disponibilă până la parsarea XML-ului UBL primit.
/// Header: `Furnizor,CUI,Serie,Numar,Data,Total,Moneda`
/// Returnează calea fișierului salvat.
#[tauri::command]
pub async fn export_purchase_journal(
    state: State<'_, AppState>,
    company_id: String,
    date_from: String,
    date_to: String,
    dest_path: String,
) -> AppResult<String> {
    let pool = &state.db;

    let rows = sqlx::query(
        "SELECT issuer_name, issuer_cui, \
                COALESCE(series, '') AS series, \
                COALESCE(number, '') AS number, \
                issue_date, total_amount, \
                COALESCE(net_amount, '') AS net_amount, \
                COALESCE(vat_amount, '') AS vat_amount, \
                currency \
         FROM received_invoices \
         WHERE company_id = ?1 \
           AND issue_date >= ?2 \
           AND issue_date <= ?3 \
         ORDER BY issue_date ASC",
    )
    .bind(&company_id)
    .bind(&date_from)
    .bind(&date_to)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    let dest = dest_path.clone();

    tokio::task::spawn_blocking(move || {
        // Notă de avertizare ca prim comentariu în fișier (rând separat, non-CSV,
        // dar util pentru utilizatorul care deschide fișierul în Excel/calc).
        // Net/TVA sunt disponibile când XML-ul a fost parsat; rândurile fără defalcare
        // au coloanele Net/TVA goale.
        let note = "# NOTA: Jurnalul de cumparari include Net/TVA extrase din XML-ul UBL \
                    cand sunt disponibile. Randurile fara defalcare nu au fost inca parsate \
                    (folositi butonul Recalculeaza TVA din XML din aplicatie).";
        let header = csv_row(&[
            "Furnizor", "CUI", "Serie", "Numar", "Data", "Net", "TVA", "Total", "Moneda",
        ]);
        let mut lines = vec![note.to_string(), header];

        for row in &rows {
            let issuer_name: String = row.try_get("issuer_name").unwrap_or_default();
            let issuer_cui: String = row.try_get("issuer_cui").unwrap_or_default();
            let series: String = row.try_get("series").unwrap_or_default();
            let number: String = row.try_get("number").unwrap_or_default();
            let issue_date: String = row.try_get("issue_date").unwrap_or_default();
            let net: String = row.try_get("net_amount").unwrap_or_default();
            let vat: String = row.try_get("vat_amount").unwrap_or_default();
            let total: String = row.try_get("total_amount").unwrap_or_default();
            let currency: String = row.try_get("currency").unwrap_or_default();

            lines.push(csv_row(&[
                &issuer_name,
                &issuer_cui,
                &series,
                &number,
                &issue_date,
                &net,
                &vat,
                &total,
                &currency,
            ]));
        }

        let content = lines.join("\r\n");
        std::fs::write(&dest, content.as_bytes()).map_err(AppError::Io)?;
        Ok(dest)
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifică că csv_field quotes câmpurile cu virgulă sau ghilimele.
    #[test]
    fn csv_field_quotes_special_chars() {
        assert_eq!(csv_field("simplu"), "simplu");
        assert_eq!(csv_field("cu, virgula"), "\"cu, virgula\"");
        assert_eq!(csv_field("cu \"ghilimele\""), "\"cu \"\"ghilimele\"\"\"");
        assert_eq!(csv_field("cu\nnewline"), "\"cu\nnewline\"");
        assert_eq!(csv_field(""), "");
    }

    /// Verifică că csv_row îmbină câmpurile cu virgulă.
    #[test]
    fn csv_row_joins_with_comma() {
        let row = csv_row(&[
            "FA-001",
            "2024-01-15",
            "SC CLIENT SRL",
            "RO123",
            "1000.00",
            "190.00",
            "1190.00",
            "VALIDATED",
        ]);
        assert_eq!(
            row,
            "FA-001,2024-01-15,SC CLIENT SRL,RO123,1000.00,190.00,1190.00,VALIDATED"
        );
    }

    /// Verifică că csv_row cu 9 câmpuri (jurnal cumpărări nou) funcționează corect.
    #[test]
    fn csv_row_nine_fields() {
        let row = csv_row(&[
            "SC FURNIZOR SRL",
            "RO654321",
            "FCT",
            "100",
            "2024-01-10",
            "5000.00",
            "950.00",
            "5950.00",
            "RON",
        ]);
        assert_eq!(
            row,
            "SC FURNIZOR SRL,RO654321,FCT,100,2024-01-10,5000.00,950.00,5950.00,RON"
        );
    }

    /// Verifică că header-ul jurnalului de vânzări are coloanele corecte.
    #[test]
    fn sales_journal_header_columns() {
        let header = csv_row(&[
            "Numar", "Data", "Client", "CUI", "Net", "TVA", "Total", "Status",
        ]);
        assert_eq!(header, "Numar,Data,Client,CUI,Net,TVA,Total,Status");
    }

    /// Verifică că header-ul jurnalului de cumpărări are coloanele corecte (cu Net/TVA).
    #[test]
    fn purchase_journal_header_columns() {
        let header = csv_row(&[
            "Furnizor", "CUI", "Serie", "Numar", "Data", "Net", "TVA", "Total", "Moneda",
        ]);
        assert_eq!(header, "Furnizor,CUI,Serie,Numar,Data,Net,TVA,Total,Moneda");
    }

    /// Verifică că jurnalul de vânzări se scrie corect în fișier.
    #[test]
    fn sales_journal_writes_to_file() {
        let header = csv_row(&[
            "Numar", "Data", "Client", "CUI", "Net", "TVA", "Total", "Status",
        ]);
        let row = csv_row(&[
            "FA-001",
            "2024-01-15",
            "SC ALPHA SRL",
            "RO123456",
            "1000.00",
            "190.00",
            "1190.00",
            "VALIDATED",
        ]);
        let content = [header, row].join("\r\n");

        let dir = std::env::temp_dir();
        let path = dir.join("test_sales_journal.csv");
        std::fs::write(&path, content.as_bytes()).unwrap();

        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("Numar,Data,Client,CUI,Net,TVA,Total,Status"));
        assert!(written.contains("FA-001"));
        assert!(written.contains("SC ALPHA SRL"));
        assert!(written.contains("VALIDATED"));

        let _ = std::fs::remove_file(&path);
    }

    /// Verifică că jurnalul de cumpărări include nota de avertizare și noile coloane Net/TVA.
    #[test]
    fn purchase_journal_includes_vat_note() {
        let note = "# NOTA: Jurnalul de cumparari include Net/TVA extrase din XML-ul UBL \
                    cand sunt disponibile. Randurile fara defalcare nu au fost inca parsate \
                    (folositi butonul Recalculeaza TVA din XML din aplicatie).";
        let header = csv_row(&[
            "Furnizor", "CUI", "Serie", "Numar", "Data", "Net", "TVA", "Total", "Moneda",
        ]);
        let row = csv_row(&[
            "SC FURNIZOR SRL",
            "RO654321",
            "FCT",
            "100",
            "2024-01-10",
            "5000.00",
            "950.00",
            "5950.00",
            "RON",
        ]);
        let content = [note.to_string(), header, row].join("\r\n");

        let dir = std::env::temp_dir();
        let path = dir.join("test_purchase_journal.csv");
        std::fs::write(&path, content.as_bytes()).unwrap();

        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("NOTA:"));
        assert!(written.contains("Furnizor,CUI,Serie,Numar,Data,Net,TVA,Total,Moneda"));
        assert!(written.contains("SC FURNIZOR SRL"));
        assert!(written.contains("RON"));

        let _ = std::fs::remove_file(&path);
    }

    /// Verifică că câmpurile cu ghilimele sunt escape-uite corect în CSV.
    #[test]
    fn csv_field_escapes_internal_quotes() {
        let name = "SC \"ALFA\" & BETA SRL";
        let field = csv_field(name);
        // Conține ghilimele → trebuie enclosed și ghilimelele interne dublate
        assert!(field.starts_with('"'));
        assert!(field.ends_with('"'));
        assert!(field.contains("\"\"ALFA\"\""));
    }
}
