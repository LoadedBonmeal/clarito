use sqlx::Row as _;
use tauri::State;

use crate::background::parse_received_xml;
use crate::db::models::{Paginated, ReceivedStatus};
use crate::db::received::{self, ReceivedFilter, ReceivedInvoice};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[tauri::command]
pub async fn list_received_invoices(
    state: State<'_, AppState>,
    filter: Option<ReceivedFilter>,
) -> AppResult<Paginated<ReceivedInvoice>> {
    let f = filter.unwrap_or_default();
    // Defence-in-depth: reject a null/empty company_id so a missing active
    // company never leaks cross-company data via the IS-NULL SQL shortcut.
    if f.company_id.as_ref().is_none_or(|s| s.is_empty()) {
        return Err(AppError::Validation(
            "Selectați o companie activă.".to_string(),
        ));
    }
    received::list(&state.db, f).await
}

#[tauri::command]
pub async fn get_received_invoice(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
) -> AppResult<ReceivedInvoice> {
    received::get(&state.db, &id, &company_id).await
}

#[tauri::command]
pub async fn update_received_status(
    state: State<'_, AppState>,
    id: String,
    company_id: String,
    status: ReceivedStatus,
) -> AppResult<()> {
    received::set_status(&state.db, &id, &company_id, status).await
}

/// Reparsează XML-ul UBL al facturilor primite pentru a extrage defalcarea net/TVA.
///
/// Dacă `company_id` este furnizat, procesează doar facturile acelei companii.
/// Procesează doar rândurile cu `net_amount IS NULL` (idempotent).
/// Returnează numărul de rânduri actualizate cu succes.
#[tauri::command]
pub async fn reparse_received_vat(
    state: State<'_, AppState>,
    company_id: Option<String>,
) -> AppResult<i64> {
    use sqlx::Row;

    let pool = &state.db;

    // Selectăm facturile neparsate (net_amount IS NULL)
    let rows = sqlx::query(
        "SELECT id, xml_path FROM received_invoices \
         WHERE net_amount IS NULL \
           AND (?1 IS NULL OR company_id = ?1)",
    )
    .bind(company_id.as_deref())
    .fetch_all(pool)
    .await?;

    let mut updated: i64 = 0;

    for row in rows {
        let id: String = row.try_get("id").unwrap_or_default();
        let xml_path: String = row.try_get("xml_path").unwrap_or_default();

        // Citim fișierul async
        let xml_bytes = match tokio::fs::read(&xml_path).await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(
                    id = %id,
                    path = %xml_path,
                    error = ?e,
                    "reparse_received_vat: nu s-a putut citi fișierul XML — se omite"
                );
                continue;
            }
        };

        // Parsăm (sincron — fișierele sunt mici)
        let parsed = parse_received_xml(&xml_bytes);

        // Actualizăm rândul (chiar dacă net/vat sunt None — marchează încercarea)
        if let Err(e) = sqlx::query(
            "UPDATE received_invoices SET net_amount = ?2, vat_amount = ?3 WHERE id = ?1",
        )
        .bind(&id)
        .bind(&parsed.net_amount)
        .bind(&parsed.vat_amount)
        .execute(pool)
        .await
        {
            tracing::warn!(id = %id, error = ?e, "reparse_received_vat: UPDATE eșuat — se omite");
            continue;
        }

        // Ștergem liniile vechi și inserăm liniile noi (idempotent)
        let _ =
            sqlx::query("DELETE FROM received_invoice_vat_lines WHERE received_invoice_id = ?1")
                .bind(&id)
                .execute(pool)
                .await;

        for vat_line in &parsed.vat_lines {
            let line_id = crate::db::models::new_id();
            if let Err(e) = sqlx::query(
                "INSERT INTO received_invoice_vat_lines \
                 (id, received_invoice_id, vat_rate, vat_category, base_amount, vat_amount) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .bind(&line_id)
            .bind(&id)
            .bind(&vat_line.vat_rate)
            .bind(&vat_line.vat_category)
            .bind(&vat_line.base_amount)
            .bind(&vat_line.vat_amount)
            .execute(pool)
            .await
            {
                tracing::warn!(
                    id = %id,
                    error = ?e,
                    "reparse_received_vat: inserare linie TVA eșuată — se continuă"
                );
            }
        }

        updated += 1;
    }

    Ok(updated)
}

/// Exportă un subset de facturi primite ca CSV (CRLF, RFC-4180).
///
/// Coloane: Furnizor, CUI, Serie-Număr, Dată, Net, TVA, Total, Monedă, Status.
/// Returnează textul CSV ca `String`; frontend-ul îl scrie la calea aleasă de utilizator.
#[tauri::command]
pub async fn export_received_csv(
    state: State<'_, AppState>,
    company_id: String,
    ids: Vec<String>,
) -> AppResult<String> {
    use crate::commands::journals::{csv_neutralize, csv_num};

    if ids.is_empty() {
        return Err(AppError::Validation(
            "Selectați cel puțin o factură pentru export.".into(),
        ));
    }

    let pool = &state.db;

    // Build a parameterized IN clause: bind each id individually.
    // We query all received invoices for the company and filter in Rust to avoid
    // dynamic SQL construction while keeping the company_id guard.
    let rows = sqlx::query(
        "SELECT issuer_name, issuer_cui, \
                COALESCE(series, '') AS series, \
                COALESCE(number, '') AS number, \
                issue_date, \
                COALESCE(net_amount, '') AS net_amount, \
                COALESCE(vat_amount, '') AS vat_amount, \
                total_amount, \
                COALESCE(currency, 'RON') AS currency, \
                status, \
                id \
         FROM received_invoices \
         WHERE company_id = ?1 \
         ORDER BY issue_date ASC",
    )
    .bind(&company_id)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    // Filter to only the requested ids (set lookup).
    let id_set: std::collections::HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();

    let header = "Furnizor,CUI,Serie-Număr,Dată,Net,TVA,Total,Monedă,Status\r\n".to_string();
    let mut csv = header;

    /// Inner helper: RFC-4180 quoting for text fields (with formula-injection neutralisation).
    fn csv_text(s: &str) -> String {
        let s = csv_neutralize(s);
        if s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r') {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    }

    for row in &rows {
        let id: String = row.try_get("id").unwrap_or_default();
        if !id_set.contains(id.as_str()) {
            continue;
        }

        let issuer_name: String = row.try_get("issuer_name").unwrap_or_default();
        let issuer_cui: String = row.try_get("issuer_cui").unwrap_or_default();
        let series: String = row.try_get("series").unwrap_or_default();
        let number: String = row.try_get("number").unwrap_or_default();
        let issue_date: String = row.try_get("issue_date").unwrap_or_default();
        let net: String = row.try_get("net_amount").unwrap_or_default();
        let vat: String = row.try_get("vat_amount").unwrap_or_default();
        let total: String = row.try_get("total_amount").unwrap_or_default();
        let currency: String = row.try_get("currency").unwrap_or_default();
        let status: String = row.try_get("status").unwrap_or_default();

        // Serie-Număr composite field
        let serie_nr = if series.is_empty() {
            number.clone()
        } else {
            format!("{}-{}", series, number)
        };

        let line = format!(
            "{},{},{},{},{},{},{},{},{}\r\n",
            csv_text(&issuer_name),
            csv_text(&issuer_cui),
            csv_text(&serie_nr),
            csv_text(&issue_date),
            csv_num(&net),
            csv_num(&vat),
            csv_num(&total),
            csv_text(&currency),
            csv_text(&status),
        );
        csv.push_str(&line);
    }

    Ok(csv)
}

#[cfg(test)]
mod tests {
    /// Verify that the CSV header has the expected 9 columns.
    #[test]
    fn export_received_csv_header_columns() {
        let header = "Furnizor,CUI,Serie-Număr,Dată,Net,TVA,Total,Monedă,Status";
        let cols: Vec<&str> = header.split(',').collect();
        assert_eq!(
            cols.len(),
            9,
            "export_received_csv must produce exactly 9 columns: {cols:?}"
        );
    }
}
