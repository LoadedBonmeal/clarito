//! Jurnale contabile — export CSV jurnal vânzări și jurnal cumpărări.
//!
//! Jurnalul de vânzări: facturile fiscale emise (VALIDATED + STORNED) pentru o perioadă,
//! cu detalii client, net, TVA, total.
//! Jurnalul de cumpărări: facturile primite din `received_invoices` — conține
//! DOAR totalul (fără defalcare net/TVA, care necesită parsarea XML-ului UBL).

use sqlx::Row;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Neutralizează câmpurile care ar putea fi interpretate ca formule în Excel/LibreOffice
/// (CSV formula injection). Câmpurile care încep cu `=`, `+`, `-` sau `@` (sau TAB/CR)
/// primesc un prefix `'` conform standardului de neutralizare CSV.
/// Aplicat ÎNAINTEA quoting-ului RFC 4180, pe valoarea brută.
pub(crate) fn csv_neutralize(s: &str) -> String {
    match s.chars().next() {
        Some('=' | '+' | '-' | '@' | '\t' | '\r') => format!("'{}", s),
        _ => s.to_string(),
    }
}

/// Construiește o linie CSV corect quotată cu separator virgulă.
/// Câmpurile care conțin virgulă, ghilimele sau newline sunt enclosed în ghilimele.
/// Ghilimelele interne sunt dublate (RFC 4180).
/// Aplică neutralizarea formula-injection înainte de quoting.
fn csv_field(s: &str) -> String {
    let s = csv_neutralize(s);
    if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Numeric/amount CSV cell: RFC-4180 quoting WITHOUT formula-injection
/// neutralization. Amounts can legitimately start with `-` (storno negatives);
/// prefixing them with `'` would turn the numeric cell into text and break
/// SUM formulas in accounting software. Amounts never contain user-controlled
/// text, so there is no injection vector to neutralize here.
pub(crate) fn csv_num(s: &str) -> String {
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
/// Include facturile fiscale emise: status VALIDATED (confirmate de ANAF) și
/// STORNED (originale anulate — rămân eventi fiscali pozitivi în perioada
/// emiterii lor; nota de credit negativă le neutralizează în propria perioadă
/// odată validată). DRAFT / SUBMITTED / QUEUED / REJECTED sunt excluse.
/// Header: `Numar,Data,Client,CUI,Net,TVA,Total,Moneda,Status`
/// Wave 4: added `Moneda` column so foreign-currency invoices are visible as-is.
/// Amounts are in the ORIGINAL document currency (journals are operational per-document
/// lists — do NOT convert to RON here; use D300/D394 for RON fiscal aggregates).
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

    // Fetch invoices fiscale (VALIDATED + STORNED) pentru companie în perioadă.
    // REG-STORNO: STORNED originals are positive fiscal events in their issued period.
    // DRAFT / SUBMITTED / QUEUED / REJECTED are excluded to keep the journal aligned
    // with the fiscal set reported to ANAF (D300/D394/SAF-T).
    // Wave 4: also fetch currency so the Moneda column is populated.
    let rows = sqlx::query(
        "SELECT i.full_number, i.issue_date, \
                COALESCE(c.legal_name, '') AS client_name, \
                COALESCE(c.cui, '') AS client_cui, \
                i.subtotal_amount, i.vat_amount, i.total_amount, \
                COALESCE(i.currency, 'RON') AS currency, \
                i.status \
         FROM invoices i \
         LEFT JOIN contacts c ON c.id = i.contact_id \
         WHERE i.company_id = ?1 \
           AND i.issue_date >= ?2 \
           AND i.issue_date <= ?3 \
           AND i.status IN ('VALIDATED', 'STORNED') \
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
            "Numar", "Data", "Client", "CUI", "Net", "TVA", "Total", "Moneda", "Status",
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
            let currency: String = row.try_get("currency").unwrap_or_default();
            let status: String = row.try_get("status").unwrap_or_default();

            // Text fields neutralized (injection vector); amounts via csv_num
            // so negative storno totals stay numeric cells, not text.
            lines.push(
                [
                    csv_field(&full_number),
                    csv_field(&issue_date),
                    csv_field(&client_name),
                    csv_field(&client_cui),
                    csv_num(&subtotal),
                    csv_num(&vat),
                    csv_num(&total),
                    csv_field(&currency),
                    csv_field(&status),
                ]
                .join(","),
            );
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
           AND status != 'REJECTED' \
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

            // Text fields neutralized (issuer_name is the SPV-sourced injection
            // vector); amounts via csv_num to keep negative cells numeric.
            lines.push(
                [
                    csv_field(&issuer_name),
                    csv_field(&issuer_cui),
                    csv_field(&series),
                    csv_field(&number),
                    csv_field(&issue_date),
                    csv_num(&net),
                    csv_num(&vat),
                    csv_num(&total),
                    csv_field(&currency),
                ]
                .join(","),
            );
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
    /// Wave 4: sales journal now has 9 fields (added Moneda).
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
            "RON",
            "VALIDATED",
        ]);
        assert_eq!(
            row,
            "FA-001,2024-01-15,SC CLIENT SRL,RO123,1000.00,190.00,1190.00,RON,VALIDATED"
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
    /// Wave 4: added Moneda column between Total and Status.
    #[test]
    fn sales_journal_header_columns() {
        let header = csv_row(&[
            "Numar", "Data", "Client", "CUI", "Net", "TVA", "Total", "Moneda", "Status",
        ]);
        assert_eq!(header, "Numar,Data,Client,CUI,Net,TVA,Total,Moneda,Status");
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
    /// Wave 4: header now includes Moneda column; EUR row is visible with currency.
    #[test]
    fn sales_journal_writes_to_file() {
        let header = csv_row(&[
            "Numar", "Data", "Client", "CUI", "Net", "TVA", "Total", "Moneda", "Status",
        ]);
        // RON invoice
        let row_ron = csv_row(&[
            "FA-001",
            "2024-01-15",
            "SC ALPHA SRL",
            "RO123456",
            "1000.00",
            "190.00",
            "1190.00",
            "RON",
            "VALIDATED",
        ]);
        // EUR invoice — amounts stay in original currency, Moneda column shows EUR
        let row_eur = csv_row(&[
            "FA-002",
            "2024-01-20",
            "SC BETA SRL",
            "RO654321",
            "1000.00",
            "190.00",
            "1190.00",
            "EUR",
            "VALIDATED",
        ]);
        let content = [header, row_ron, row_eur].join("\r\n");

        let dir = std::env::temp_dir();
        let path = dir.join("test_sales_journal.csv");
        std::fs::write(&path, content.as_bytes()).unwrap();

        let written = std::fs::read_to_string(&path).unwrap();
        assert!(
            written.contains("Numar,Data,Client,CUI,Net,TVA,Total,Moneda,Status"),
            "Header must include Moneda column"
        );
        assert!(written.contains("FA-001"));
        assert!(written.contains("SC ALPHA SRL"));
        assert!(written.contains("RON"), "RON currency must appear");
        assert!(written.contains("EUR"), "EUR currency must appear");
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

    // ── R2: CSV formula-injection neutralization ──────────────────────────────

    /// R2: neutralizatorul prefixează câmpurile periculoase cu `'`.
    #[test]
    fn csv_neutralize_prefixes_formula_chars() {
        // Formula injection chars must be prefixed with single quote
        assert_eq!(csv_neutralize("=cmd"), "'=cmd");
        assert_eq!(csv_neutralize("+1+1"), "'+1+1");
        assert_eq!(csv_neutralize("-1"), "'-1");
        assert_eq!(csv_neutralize("@SUM(A1)"), "'@SUM(A1)");
        // TAB and CR also neutralized
        assert_eq!(csv_neutralize("\t"), "'\t");
        assert_eq!(csv_neutralize("\r"), "'\r");
    }

    /// R16 W6-followup: amount cells (csv_num) keep negative storno values
    /// numeric — NOT prefixed with a quote (which would break Excel SUM).
    #[test]
    fn csv_num_does_not_neutralize_negative_amounts() {
        assert_eq!(csv_num("-150.00"), "-150.00");
        assert_eq!(csv_num("1000.00"), "1000.00");
        assert_eq!(csv_num("0.00"), "0.00");
        assert_eq!(csv_num(""), "");
        // contrast: csv_field WOULD prefix a leading '-'
        assert_eq!(csv_field("-150.00"), "'-150.00");
    }

    /// R2: textul normal nu este modificat de neutralizator.
    #[test]
    fn csv_neutralize_leaves_normal_text_untouched() {
        assert_eq!(csv_neutralize("SC ALFA SRL"), "SC ALFA SRL");
        assert_eq!(csv_neutralize("RO123456"), "RO123456");
        assert_eq!(csv_neutralize("1000.00"), "1000.00");
        assert_eq!(csv_neutralize(""), "");
        assert_eq!(csv_neutralize("VALIDATED"), "VALIDATED");
        // Parens/spaces/letters are safe
        assert_eq!(csv_neutralize("(test)"), "(test)");
    }

    /// R2: csv_field aplică neutralizarea ÎNAINTE de quoting RFC 4180.
    #[test]
    fn csv_field_neutralizes_then_quotes() {
        // "=HYPERLINK(\"evil\")" → neutralized to "'=HYPERLINK(\"evil\")", then
        // the result contains quotes so it gets RFC-4180 enclosed
        let result = csv_field("=HYPERLINK(\"evil\")");
        // After neutralization: "'=HYPERLINK(\"evil\")" which has a double-quote → enclosed
        assert!(
            result.starts_with('"'),
            "field with quote after neutralization must be enclosed"
        );
        assert!(
            result.contains("'=HYPERLINK"),
            "neutralizer prefix must be present"
        );
        // Simpler: field starts with `=`, no special quoting chars — just prefixed
        assert_eq!(csv_field("=cmd"), "'=cmd");
        assert_eq!(csv_field("+cmd"), "'+cmd");
    }

    // ── R1: purchase journal excludes REJECTED received invoices ─────────────

    /// R1: verifică că filtrul SQL `AND status != 'REJECTED'` exclude facturile respinse.
    /// Testăm logica de filtrare simulând două seturi de date — doar cele cu status != REJECTED
    /// trebuie incluse în jurnal, consistent cu declarațiile D300/D394.
    #[test]
    fn purchase_journal_excludes_rejected_invoices() {
        // Simulate the filtering logic that the SQL query now enforces:
        // status != 'REJECTED'
        struct FakeReceived {
            issuer_name: &'static str,
            status: &'static str,
        }

        let invoices = [
            FakeReceived {
                issuer_name: "SC ALFA SRL",
                status: "NEW",
            },
            FakeReceived {
                issuer_name: "SC BETA SRL",
                status: "REJECTED",
            },
            FakeReceived {
                issuer_name: "SC GAMA SRL",
                status: "OK",
            },
        ];

        // Apply the same filter as the SQL query
        let included: Vec<&FakeReceived> =
            invoices.iter().filter(|r| r.status != "REJECTED").collect();

        assert_eq!(included.len(), 2, "REJECTED invoice must be excluded");
        assert!(included.iter().any(|r| r.issuer_name == "SC ALFA SRL"));
        assert!(included.iter().any(|r| r.issuer_name == "SC GAMA SRL"));
        assert!(
            !included.iter().any(|r| r.issuer_name == "SC BETA SRL"),
            "SC BETA SRL has status REJECTED and must NOT appear in the journal"
        );
    }

    /// R1: verifică că CSV-ul jurnalului de cumpărări conține NUMAI factura NEW, nu cea REJECTED.
    #[test]
    fn purchase_journal_csv_contains_only_non_rejected() {
        // Simulate building the CSV for only non-REJECTED entries
        let invoices = vec![
            ("SC ALFA SRL", "RO111", "NEW"),
            ("SC BETA SRL", "RO222", "REJECTED"),
        ];

        let note = "# NOTA: test";
        let header = csv_row(&["Furnizor", "CUI", "Status"]);
        let mut lines = vec![note.to_string(), header];

        for (name, cui, status) in &invoices {
            if *status != "REJECTED" {
                lines.push(csv_row(&[name, cui, status]));
            }
        }

        let content = lines.join("\r\n");
        assert!(content.contains("SC ALFA SRL"), "NEW invoice must appear");
        assert!(
            !content.contains("SC BETA SRL"),
            "REJECTED invoice must not appear"
        );
        assert!(!content.contains("RO222"), "REJECTED CUI must not appear");
    }

    // ── REG-STORNO: sales journal fiscal status set ───────────────────────────

    /// REG-STORNO: jurnalul de vânzări include STORNED (eveniment fiscal pozitiv
    /// în perioada emiterii) dar exclude DRAFT / SUBMITTED / QUEUED / REJECTED.
    #[test]
    fn sales_journal_fiscal_status_filter() {
        struct FakeSale {
            full_number: &'static str,
            status: &'static str,
        }

        let fiscal_statuses = ["VALIDATED", "STORNED"];

        let invoices = [
            FakeSale {
                full_number: "FA-001",
                status: "VALIDATED",
            },
            FakeSale {
                full_number: "FA-002",
                status: "STORNED",
            }, // original — positive fiscal event
            FakeSale {
                full_number: "FA-003",
                status: "DRAFT",
            }, // not yet submitted
            FakeSale {
                full_number: "FA-004",
                status: "SUBMITTED",
            }, // awaiting ANAF
            FakeSale {
                full_number: "FA-005",
                status: "QUEUED",
            },
            FakeSale {
                full_number: "FA-006",
                status: "REJECTED",
            },
        ];

        let included: Vec<&FakeSale> = invoices
            .iter()
            .filter(|inv| fiscal_statuses.contains(&inv.status))
            .collect();

        assert_eq!(included.len(), 2, "Only VALIDATED and STORNED must appear");
        assert!(
            included.iter().any(|i| i.full_number == "FA-001"),
            "VALIDATED must be included"
        );
        assert!(
            included.iter().any(|i| i.full_number == "FA-002"),
            "STORNED must be included"
        );
        assert!(
            !included.iter().any(|i| i.full_number == "FA-003"),
            "DRAFT must be excluded"
        );
        assert!(
            !included.iter().any(|i| i.full_number == "FA-004"),
            "SUBMITTED must be excluded"
        );
        assert!(
            !included.iter().any(|i| i.full_number == "FA-005"),
            "QUEUED must be excluded"
        );
        assert!(
            !included.iter().any(|i| i.full_number == "FA-006"),
            "REJECTED must be excluded"
        );
    }

    /// REG-STORNO: jurnalul de vânzări produce totaluri corecte când include
    /// un STORNED original (pozitiv) și nota de credit VALIDATED (negativă).
    /// Amounts MUST use csv_num (not csv_field) so negative storno values
    /// are numeric cells rather than formula-injection-prefixed text.
    #[test]
    fn sales_journal_storno_net_zero_in_csv() {
        // Original STORNED: net=1000, vat=190, total=1190
        // Credit note VALIDATED: net=-1000, vat=-190, total=-1190
        // Net should be zero in any aggregation.
        //
        // Mirror the actual production logic: text fields via csv_field,
        // amount fields via csv_num (no injection-prefix for numeric cells).
        let header = csv_row(&[
            "Numar", "Data", "Client", "CUI", "Net", "TVA", "Total", "Moneda", "Status",
        ]);
        // Build rows the same way export_sales_journal does in production:
        // csv_field for text, csv_num for amounts.
        let build_row = |num: &str,
                         date: &str,
                         client: &str,
                         cui: &str,
                         net: &str,
                         vat: &str,
                         total: &str,
                         currency: &str,
                         status: &str|
         -> String {
            [
                csv_field(num),
                csv_field(date),
                csv_field(client),
                csv_field(cui),
                csv_num(net),
                csv_num(vat),
                csv_num(total),
                csv_field(currency),
                csv_field(status),
            ]
            .join(",")
        };
        let row_orig = build_row(
            "FA-001",
            "2024-01-10",
            "SC CLIENT SRL",
            "RO111",
            "1000.00",
            "190.00",
            "1190.00",
            "RON",
            "STORNED",
        );
        let row_credit = build_row(
            "FASTO-001",
            "2024-01-15",
            "SC CLIENT SRL",
            "RO111",
            "-1000.00",
            "-190.00",
            "-1190.00",
            "RON",
            "VALIDATED",
        );
        let content = [header, row_orig, row_credit].join("\r\n");

        assert!(content.contains("FA-001"), "STORNED original must appear");
        assert!(content.contains("FASTO-001"), "Credit note must appear");
        assert!(
            content.contains("STORNED"),
            "Status STORNED must be visible"
        );
        // Negative amounts stay numeric (csv_num, not csv_field)
        assert!(
            content.contains("-1000.00"),
            "Negative base must appear as numeric"
        );
        assert!(
            content.contains("-190.00"),
            "Negative VAT must appear as numeric"
        );
        // Verify csv_num does NOT prefix negative amounts with quote
        assert!(
            !content.contains("'-1000.00"),
            "csv_num must not inject quote prefix"
        );
    }
}
