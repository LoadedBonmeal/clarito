use tauri::State;

use crate::background::parse_received_xml;
use crate::db::models::{Paginated, ReceivedStatus};
use crate::db::received::{self, ReceivedFilter, ReceivedInvoice};
use crate::error::AppResult;
use crate::state::AppState;

#[tauri::command]
pub async fn list_received_invoices(
    state: State<'_, AppState>,
    filter: Option<ReceivedFilter>,
) -> AppResult<Paginated<ReceivedInvoice>> {
    received::list(&state.db, filter.unwrap_or_default()).await
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
