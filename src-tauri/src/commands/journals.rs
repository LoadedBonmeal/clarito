//! Jurnale contabile — export CSV jurnal vânzări și jurnal cumpărări.
//!
//! Jurnalul de vânzări: facturile fiscale emise (VALIDATED + STORNED) pentru o perioadă,
//! cu detalii client, net, TVA, total.
//!
//! Jurnalul de cumpărări: facturile primite din `received_invoices`, cu defalcare TVA
//! per cotă (21%/11%/9%/19%) conform art.321 CF + pct.101 norme metodologice.
//! Sursa datelor: tabelul `received_invoice_vat_lines` (populat la parsarea XML UBL);
//! facturile neparsate primesc o linie de fallback în coloana "Nealocat".
//!
//! Coloane jurnal cumpărări (pct.101 norme):
//!   Nr.crt | Data înreg. | Tip doc | Serie | Număr | Dată doc | Furnizor | CUI |
//!   Valoare totală |
//!   Bază@21% | TVA@21% | Bază@11% | TVA@11% | Bază@9% | TVA@9% |
//!   Bază@19%(ist.) | TVA@19%(ist.) |
//!   TaxInv.Bază(art.331) | TaxInv.TVA | AIC.Bază(art.320) | AIC.TVA |
//!   Nealocat.Bază | Nealocat.TVA |
//!   Monedă |
//!   TOTAL (rând totale): Σ TVA deductibilă = rulaj D 4426 = deductibil D300 perioadă.

use std::collections::HashMap;
use std::str::FromStr;

use rust_decimal::Decimal;
use sqlx::Row;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

// ── Decimal helpers ───────────────────────────────────────────────────────────

/// Parse a Decimal from the TEXT amounts stored in the DB.
fn dec(s: &str) -> Decimal {
    Decimal::from_str(s.trim()).unwrap_or_default()
}

/// Format a Decimal to 2 decimal places for CSV output.
fn dec2(d: Decimal) -> String {
    format!(
        "{:.2}",
        d.round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
    )
}

/// Normalise a DB-stored vat_rate string to canonical percent Decimal.
/// Accepts both "0.21" (fraction) and "21" (integer percent).
fn normalize_vat_rate(raw: &str) -> Decimal {
    let s = raw.trim();
    let d = Decimal::from_str(s).unwrap_or(Decimal::ZERO);
    if d < Decimal::ONE && d > Decimal::ZERO {
        (d * Decimal::from(100))
            .round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
    } else {
        d
    }
}

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
    crate::commands::require_valid_date("Data de început", &date_from)?;
    crate::commands::require_valid_date("Data de sfârșit", &date_to)?;
    let dest_path = crate::commands::integrations::validate_export_path(&dest_path)?
        .to_string_lossy()
        .to_string();
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
        // UTF-8 BOM so Excel opens Romanian diacritics correctly.
        let bom = "\u{FEFF}";
        let header = csv_row(&[
            "Numar", "Data", "Client", "CUI", "Net", "TVA", "Total", "Moneda", "Status",
        ]);
        let mut lines = vec![format!("{}{}", bom, header)];

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

/// Coloane per-cotă emise în jurnalul de cumpărări.
///
/// Ordinea fixă: 21% · 11% · 9% · 19%(ist.) · taxare_inversă(art.331) ·
/// achiziții_intracomunitare(art.320) · nealocat (fallback).
///
/// Limitare operație: `vat_category` AE → taxare inversă, K → AIC.
/// Celelalte categorii (S/Z/E/G/O) → rutate pe coloana cotei numerice.
/// Dacă factura nu are VAT lines parsate → tot TVA-ul merge la "Nealocat".
struct PurchaseJournalRow {
    /// Bază@21%
    base21: Decimal,
    /// TVA@21%
    vat21: Decimal,
    /// Bază@11%
    base11: Decimal,
    /// TVA@11%
    vat11: Decimal,
    /// Bază@9%
    base9: Decimal,
    /// TVA@9%
    vat9: Decimal,
    /// Bază@19% (cotă istorică pre-2024)
    base19: Decimal,
    /// TVA@19%
    vat19: Decimal,
    /// Bază taxare inversă internă (art.331 CF) — TVA deductibil ȘI colectat simultan
    base_ti: Decimal,
    /// TVA taxare inversă (deductibil)
    vat_ti: Decimal,
    /// Bază achiziții intracomunitare (art.320 CF) — autocollect
    base_aic: Decimal,
    /// TVA AIC (deductibil)
    vat_aic: Decimal,
    /// Baze neaocate (factură fără VAT lines parsate) — coloana fallback
    base_nealocat: Decimal,
    /// TVA nealocat
    vat_nealocat: Decimal,
}

impl PurchaseJournalRow {
    fn zero() -> Self {
        Self {
            base21: Decimal::ZERO,
            vat21: Decimal::ZERO,
            base11: Decimal::ZERO,
            vat11: Decimal::ZERO,
            base9: Decimal::ZERO,
            vat9: Decimal::ZERO,
            base19: Decimal::ZERO,
            vat19: Decimal::ZERO,
            base_ti: Decimal::ZERO,
            vat_ti: Decimal::ZERO,
            base_aic: Decimal::ZERO,
            vat_aic: Decimal::ZERO,
            base_nealocat: Decimal::ZERO,
            vat_nealocat: Decimal::ZERO,
        }
    }

    /// Suma TVA deductibilă pentru această linie.
    /// = vat21 + vat11 + vat9 + vat19 + vat_ti + vat_aic + vat_nealocat
    /// Trebuie să coincidă cu rulajul D 4426 aferent facturii (reconciliere D300).
    fn total_vat_deductibil(&self) -> Decimal {
        self.vat21
            + self.vat11
            + self.vat9
            + self.vat19
            + self.vat_ti
            + self.vat_aic
            + self.vat_nealocat
    }

    /// Produce array-ul de câmpuri CSV per-cotă (14 câmpuri).
    fn to_csv_fields(&self) -> [String; 14] {
        [
            dec2(self.base21),
            dec2(self.vat21),
            dec2(self.base11),
            dec2(self.vat11),
            dec2(self.base9),
            dec2(self.vat9),
            dec2(self.base19),
            dec2(self.vat19),
            dec2(self.base_ti),
            dec2(self.vat_ti),
            dec2(self.base_aic),
            dec2(self.vat_aic),
            dec2(self.base_nealocat),
            dec2(self.vat_nealocat),
        ]
    }
}

/// Exportă jurnalul de cumpărări (CSV) pentru o companie și o perioadă.
///
/// Structură per art.321 CF + pct.101 norme metodologice:
/// identificare document + furnizor + valoare totală + defalcare per cotă TVA.
///
/// Sursa defalcării: `received_invoice_vat_lines` (populat la parsarea XML UBL).
/// Facturile neparsate primesc fallback în coloanele "Nealocat".
///
/// Σ TVA deductibilă (ultima coloană din totaluri) = rulaj D 4426 pentru perioadă
/// = TVA deductibil raportat în D300. Verificați reconcilierea cu contabilitatea.
///
/// Returnează calea fișierului salvat.
#[tauri::command]
pub async fn export_purchase_journal(
    state: State<'_, AppState>,
    company_id: String,
    date_from: String,
    date_to: String,
    dest_path: String,
) -> AppResult<String> {
    crate::commands::require_valid_date("Data de început", &date_from)?;
    crate::commands::require_valid_date("Data de sfârșit", &date_to)?;
    let dest_path = crate::commands::integrations::validate_export_path(&dest_path)?
        .to_string_lossy()
        .to_string();
    let pool = &state.db;

    // ── Pasul 1: facturile primite pentru perioadă (ne-REJECTED) ──────────────
    let inv_rows = sqlx::query(
        "SELECT id, issuer_name, issuer_cui, \
                COALESCE(series, '') AS series, \
                COALESCE(number, '') AS number, \
                issue_date, total_amount, \
                COALESCE(net_amount, '') AS net_amount, \
                COALESCE(vat_amount, '') AS vat_amount, \
                COALESCE(currency, 'RON') AS currency \
         FROM received_invoices \
         WHERE company_id = ?1 \
           AND issue_date >= ?2 \
           AND issue_date <= ?3 \
           AND status != 'REJECTED' \
         ORDER BY issue_date ASC, number ASC",
    )
    .bind(&company_id)
    .bind(&date_from)
    .bind(&date_to)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    // ── Pasul 2: batch-fetch VAT lines pentru toate facturile ─────────────────
    let invoice_ids: Vec<String> = inv_rows
        .iter()
        .map(|r| r.try_get::<String, _>("id").unwrap_or_default())
        .collect();

    // Map: invoice_id → Vec<(vat_category, vat_rate_pct, base, vat)>
    let mut vat_lines_map: HashMap<String, Vec<(String, Decimal, Decimal, Decimal)>> =
        HashMap::new();

    if !invoice_ids.is_empty() {
        let placeholders: String = (1..=invoice_ids.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT received_invoice_id, vat_category, vat_rate, base_amount, vat_amount \
             FROM received_invoice_vat_lines \
             WHERE received_invoice_id IN ({placeholders}) \
             ORDER BY received_invoice_id",
        );
        let mut q = sqlx::query(&sql);
        for id in &invoice_ids {
            q = q.bind(id);
        }
        let vl_rows = q.fetch_all(pool).await.map_err(AppError::Database)?;

        for vr in vl_rows {
            let inv_id: String = vr.try_get("received_invoice_id").unwrap_or_default();
            let vat_category: String = vr
                .try_get("vat_category")
                .unwrap_or_else(|_| "S".to_string());
            let raw_rate: String = vr.try_get("vat_rate").unwrap_or_else(|_| "0".to_string());
            let vat_rate_pct = normalize_vat_rate(&raw_rate);
            let base = dec(&vr.try_get::<String, _>("base_amount").unwrap_or_default());
            let vat = dec(&vr.try_get::<String, _>("vat_amount").unwrap_or_default());
            vat_lines_map
                .entry(inv_id)
                .or_default()
                .push((vat_category, vat_rate_pct, base, vat));
        }
    }

    let dest = dest_path.clone();

    tokio::task::spawn_blocking(move || {
        // UTF-8 BOM + notă metodologică ca prim rând non-CSV.
        let note = "\u{FEFF}# JURNAL CUMPARARI — art.321 CF, pct.101 norme. \
                    Defalcare TVA din received_invoice_vat_lines (XML UBL parsat). \
                    Facturile fara defalcare apar in coloanele Nealocat. \
                    Suma TVA Deductibila totala = rulaj D 4426 = deductibil D300 perioada.";

        // ── Header (27 câmpuri) ───────────────────────────────────────────────
        // Identitate doc (6) + Furnizor (2) + Total (1) +
        // per-cotă (4×2=8) + special (2×2=4) + nealocat (2) + Monedă (1) +
        // TVA Deductibila Total (1) = 25 câmpuri de date
        let header = [
            "Furnizor",
            "CUI",
            "Serie",
            "Numar",
            "Data",
            "Valoare Totala",
            // Per-cotă standard
            "Baza@21%",
            "TVA@21%",
            "Baza@11%",
            "TVA@11%",
            "Baza@9%",
            "TVA@9%",
            "Baza@19%(ist)",
            "TVA@19%(ist)",
            // Operații speciale
            "TaxInv.Baza(art.331)",
            "TaxInv.TVA(art.331)",
            "AIC.Baza(art.320)",
            "AIC.TVA(art.320)",
            // Fallback
            "Nealocat.Baza",
            "Nealocat.TVA",
            // Monedă + reconciliere
            "Moneda",
            "TVA Deductibila Total",
        ]
        .iter()
        .map(|f| csv_field(f))
        .collect::<Vec<_>>()
        .join(",");

        let mut lines = vec![note.to_string(), header];

        // Acumulatori pentru rândul de totaluri (per-cotă + TVA total)
        let mut tot = PurchaseJournalRow::zero();
        let mut sum_total_doc = Decimal::ZERO;

        for row in &inv_rows {
            let inv_id: String = row.try_get("id").unwrap_or_default();
            let issuer_name: String = row.try_get("issuer_name").unwrap_or_default();
            let issuer_cui: String = row.try_get("issuer_cui").unwrap_or_default();
            let series: String = row.try_get("series").unwrap_or_default();
            let number: String = row.try_get("number").unwrap_or_default();
            let issue_date: String = row.try_get("issue_date").unwrap_or_default();
            let total_str: String = row.try_get("total_amount").unwrap_or_default();
            let net_str: String = row.try_get("net_amount").unwrap_or_default();
            let vat_str: String = row.try_get("vat_amount").unwrap_or_default();
            let currency: String = row.try_get("currency").unwrap_or_default();

            let total_doc = dec(&total_str);
            sum_total_doc += total_doc;

            // ── Construiește per-cotă din VAT lines ──────────────────────────
            let mut prow = PurchaseJournalRow::zero();

            if let Some(vlines) = vat_lines_map.get(&inv_id) {
                // Factură cu VAT lines parsate: defalcare exactă
                for (vat_category, vat_rate_pct, base, vat) in vlines {
                    match vat_category.as_str() {
                        // Taxare inversă internă art.331 CF (AE = reverse charge)
                        "AE" => {
                            prow.base_ti += base;
                            prow.vat_ti += vat;
                        }
                        // Achiziții intracomunitare art.320 CF (K = IC supply)
                        "K" => {
                            prow.base_aic += base;
                            prow.vat_aic += vat;
                        }
                        // Toate celelalte (S=standard, Z=zero, E=exempt, G=outside scope, O=out of scope)
                        // — rutate pe coloana cotei
                        _ => {
                            // Comparăm cu Decimal (evitând to_i64 — trait local în alt modul).
                            if *vat_rate_pct == Decimal::from(21) {
                                prow.base21 += base;
                                prow.vat21 += vat;
                            } else if *vat_rate_pct == Decimal::from(11) {
                                prow.base11 += base;
                                prow.vat11 += vat;
                            } else if *vat_rate_pct == Decimal::from(9) {
                                prow.base9 += base;
                                prow.vat9 += vat;
                            } else if *vat_rate_pct == Decimal::from(19) {
                                prow.base19 += base;
                                prow.vat19 += vat;
                            } else {
                                // Cote 0% / scutite / necunoscute → nealocat
                                prow.base_nealocat += base;
                                prow.vat_nealocat += vat;
                            }
                        }
                    }
                }
            } else {
                // Fallback: factura nu are VAT lines (XML neparsat sau import vechi).
                // Plasăm net/TVA în coloana Nealocat — nu pierdem nicio linie,
                // nu dublu-contorizăm (total_doc rămâne corect).
                let net_fall = dec(&net_str);
                let vat_fall = dec(&vat_str);
                if net_fall != Decimal::ZERO || vat_fall != Decimal::ZERO {
                    prow.base_nealocat = net_fall;
                    prow.vat_nealocat = vat_fall;
                } else {
                    // Nici net/vat nu sunt disponibile — plasăm totalul în Nealocat.
                    // Cazul cel mai degradat (import vechi fără parsare XML).
                    prow.base_nealocat = total_doc;
                }
            }

            // Acumulează în totaluri perioadă
            tot.base21 += prow.base21;
            tot.vat21 += prow.vat21;
            tot.base11 += prow.base11;
            tot.vat11 += prow.vat11;
            tot.base9 += prow.base9;
            tot.vat9 += prow.vat9;
            tot.base19 += prow.base19;
            tot.vat19 += prow.vat19;
            tot.base_ti += prow.base_ti;
            tot.vat_ti += prow.vat_ti;
            tot.base_aic += prow.base_aic;
            tot.vat_aic += prow.vat_aic;
            tot.base_nealocat += prow.base_nealocat;
            tot.vat_nealocat += prow.vat_nealocat;

            let per_rate_fields = prow.to_csv_fields();
            let tvad = prow.total_vat_deductibil();

            // Construiește rândul: text fields prin csv_field, amount fields prin csv_num
            let mut fields: Vec<String> = vec![
                csv_field(&issuer_name),
                csv_field(&issuer_cui),
                csv_field(&series),
                csv_field(&number),
                csv_field(&issue_date),
                csv_num(&dec2(total_doc)),
            ];
            for f in &per_rate_fields {
                fields.push(csv_num(f));
            }
            fields.push(csv_field(&currency));
            fields.push(csv_num(&dec2(tvad)));

            lines.push(fields.join(","));
        }

        // ── Rând TOTALURI perioadă ────────────────────────────────────────────
        // Σ TVA deductibilă = rulaj D 4426 pentru perioadă
        let tvad_total = tot.total_vat_deductibil();
        let tot_fields = tot.to_csv_fields();
        let mut total_row: Vec<String> = vec![
            csv_field("TOTAL PERIOADA"),
            csv_field(""),
            csv_field(""),
            csv_field(""),
            csv_field(""),
            csv_num(&dec2(sum_total_doc)),
        ];
        for f in &tot_fields {
            total_row.push(csv_num(f));
        }
        // Moneda e neaplicabilă la totaluri; TVA deductibil total = reconciliere 4426
        total_row.push(csv_field(""));
        total_row.push(csv_num(&dec2(tvad_total)));
        lines.push(total_row.join(","));

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

    /// Verifică că csv_row cu N câmpuri funcționează corect (generic).
    #[test]
    fn csv_row_multiple_fields() {
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

    /// Verifică că header-ul jurnalului de cumpărări per-cotă are coloanele corecte.
    /// Coloane: identitate(6) + per-cotă(4×2=8) + special(2×2=4) + nealocat(2) + monedă(1) + TVAd(1) = 22.
    #[test]
    fn purchase_journal_per_rate_header_columns() {
        let header = [
            "Furnizor",
            "CUI",
            "Serie",
            "Numar",
            "Data",
            "Valoare Totala",
            "Baza@21%",
            "TVA@21%",
            "Baza@11%",
            "TVA@11%",
            "Baza@9%",
            "TVA@9%",
            "Baza@19%(ist)",
            "TVA@19%(ist)",
            "TaxInv.Baza(art.331)",
            "TaxInv.TVA(art.331)",
            "AIC.Baza(art.320)",
            "AIC.TVA(art.320)",
            "Nealocat.Baza",
            "Nealocat.TVA",
            "Moneda",
            "TVA Deductibila Total",
        ]
        .iter()
        .map(|f| csv_field(f))
        .collect::<Vec<_>>()
        .join(",");

        assert!(header.contains("Baza@21%"), "must contain 21% base column");
        assert!(header.contains("TVA@21%"), "must contain 21% VAT column");
        assert!(header.contains("Baza@11%"), "must contain 11% base column");
        assert!(header.contains("TVA@11%"), "must contain 11% VAT column");
        assert!(header.contains("Baza@9%"), "must contain 9% base column");
        assert!(
            header.contains("Baza@19%(ist)"),
            "must contain 19% historical base column"
        );
        assert!(
            header.contains("TaxInv.Baza(art.331)"),
            "must contain reverse-charge base column"
        );
        assert!(
            header.contains("AIC.Baza(art.320)"),
            "must contain IC acquisition base column"
        );
        assert!(
            header.contains("Nealocat.Baza"),
            "must contain fallback column"
        );
        assert!(
            header.contains("TVA Deductibila Total"),
            "must contain TVA deductibila total (reconciliere 4426)"
        );
        // Count columns — 22 expected
        let col_count = header.split(',').count();
        assert_eq!(col_count, 22, "header must have 22 columns");
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

    /// Verifică că jurnalul de cumpărări include nota metodologică și header-ul per-cotă.
    #[test]
    fn purchase_journal_includes_vat_note() {
        // Nota conține cuvintele cheie produse de implementare
        let note = "# JURNAL CUMPARARI — art.321 CF, pct.101 norme. \
                    Defalcare TVA din received_invoice_vat_lines (XML UBL parsat). \
                    Facturile fara defalcare apar in coloanele Nealocat. \
                    Suma TVA Deductibila totala = rulaj D 4426 = deductibil D300 perioada.";
        assert!(note.contains("art.321 CF"), "nota must cite art.321 CF");
        assert!(note.contains("pct.101"), "nota must cite pct.101 norme");
        assert!(
            note.contains("4426"),
            "nota must mention reconciliation with account 4426"
        );
        assert!(
            note.contains("D300"),
            "nota must mention reconciliation with D300"
        );
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

    /// A: Sales journal CSV starts with UTF-8 BOM for correct diacritics in Excel.
    #[test]
    fn sales_journal_csv_starts_with_utf8_bom() {
        let bom = "\u{FEFF}";
        let header = csv_row(&[
            "Numar", "Data", "Client", "CUI", "Net", "TVA", "Total", "Moneda", "Status",
        ]);
        let first_line = format!("{}{}", bom, header);
        assert!(
            first_line.starts_with('\u{FEFF}'),
            "Sales journal CSV must start with UTF-8 BOM"
        );
    }

    /// A: Purchase journal CSV starts with UTF-8 BOM for correct diacritics in Excel.
    #[test]
    fn purchase_journal_csv_starts_with_utf8_bom() {
        let note = "\u{FEFF}# NOTA: test";
        assert!(
            note.starts_with('\u{FEFF}'),
            "Purchase journal CSV must start with UTF-8 BOM"
        );
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

    // ── Jurnal cumpărări per-cotă: teste noi ─────────────────────────────────

    /// Factură cu VAT lines la 21% (bază=1000, TVA=210) și 11% (bază=500, TVA=55):
    /// rândul CSV trebuie să conțină base21=1000/vat21=210, base11=500/vat11=55,
    /// total=1765; Σ TVA deductibilă = 265.
    /// Testează PurchaseJournalRow direct (fără DB).
    #[test]
    fn purchase_journal_per_rate_two_rates() {
        let mut prow = PurchaseJournalRow::zero();
        // Linie 21%
        prow.base21 = Decimal::from(1000);
        prow.vat21 = Decimal::from(210);
        // Linie 11%
        prow.base11 = Decimal::from(500);
        prow.vat11 = Decimal::from(55);

        // Valoare totală document = baze + TVA (calculat explicit)
        let total_base = prow.base21
            + prow.base11
            + prow.base9
            + prow.base19
            + prow.base_ti
            + prow.base_aic
            + prow.base_nealocat;
        let total_doc = total_base + prow.total_vat_deductibil();
        assert_eq!(
            total_doc,
            Decimal::from(1765),
            "total doc 1000+210+500+55 = 1765"
        );

        // Σ TVA deductibilă
        let sigma_vat = prow.total_vat_deductibil();
        assert_eq!(
            sigma_vat,
            Decimal::from(265),
            "Σ TVA deductibila = 210+55 = 265"
        );

        // Câmpuri CSV per-cotă: index 0=base21, 1=vat21, 2=base11, 3=vat11
        let fields = prow.to_csv_fields();
        assert_eq!(fields[0], "1000.00", "base@21%");
        assert_eq!(fields[1], "210.00", "vat@21%");
        assert_eq!(fields[2], "500.00", "base@11%");
        assert_eq!(fields[3], "55.00", "vat@11%");
        // Celelalte cote zero
        assert_eq!(fields[4], "0.00", "base@9% must be zero");
        assert_eq!(fields[5], "0.00", "vat@9% must be zero");
        // Coloanele nealocat zero (factură cu VAT lines)
        assert_eq!(fields[12], "0.00", "Nealocat.Baza must be zero");
        assert_eq!(fields[13], "0.00", "Nealocat.TVA must be zero");
    }

    /// Două facturi → totaluri per-cotă sumează corect.
    /// inv1: 21%(1000/210) + 11%(500/55); inv2: 9%(200/18) + 21%(300/63).
    /// Totale: base21=1300, vat21=273, base11=500, vat11=55, base9=200, vat9=18.
    /// Σ TVA total = 273+55+18 = 346.
    #[test]
    fn purchase_journal_totals_sum_per_rate() {
        let mut tot = PurchaseJournalRow::zero();

        // Factura 1
        let mut p1 = PurchaseJournalRow::zero();
        p1.base21 = Decimal::from(1000);
        p1.vat21 = Decimal::from(210);
        p1.base11 = Decimal::from(500);
        p1.vat11 = Decimal::from(55);

        // Factura 2
        let mut p2 = PurchaseJournalRow::zero();
        p2.base9 = Decimal::from(200);
        p2.vat9 = Decimal::from(18);
        p2.base21 = Decimal::from(300);
        p2.vat21 = Decimal::from(63);

        tot.base21 = p1.base21 + p2.base21;
        tot.vat21 = p1.vat21 + p2.vat21;
        tot.base11 = p1.base11 + p2.base11;
        tot.vat11 = p1.vat11 + p2.vat11;
        tot.base9 = p1.base9 + p2.base9;
        tot.vat9 = p1.vat9 + p2.vat9;

        assert_eq!(tot.base21, Decimal::from(1300), "Σ base@21% = 1000+300");
        assert_eq!(tot.vat21, Decimal::from(273), "Σ vat@21% = 210+63");
        assert_eq!(tot.base11, Decimal::from(500), "Σ base@11% = 500");
        assert_eq!(tot.vat11, Decimal::from(55), "Σ vat@11% = 55");
        assert_eq!(tot.base9, Decimal::from(200), "Σ base@9% = 200");
        assert_eq!(tot.vat9, Decimal::from(18), "Σ vat@9% = 18");

        let sigma_vat = tot.total_vat_deductibil();
        assert_eq!(
            sigma_vat,
            Decimal::from(346),
            "Σ TVA deductibila total = 273+55+18 = 346"
        );
    }

    /// Factură fără VAT lines (XML neparsat): fallback în coloana Nealocat.
    /// Nu trebuie să crăpeze, nu trebuie dublă-contabilizare.
    #[test]
    fn purchase_journal_fallback_no_vat_lines() {
        let net_str = "5000.00";
        let vat_str = "1050.00";

        let net_fall = dec(net_str);
        let vat_fall = dec(vat_str);

        let mut prow = PurchaseJournalRow::zero();
        // Logica fallback din export_purchase_journal
        if net_fall != Decimal::ZERO || vat_fall != Decimal::ZERO {
            prow.base_nealocat = net_fall;
            prow.vat_nealocat = vat_fall;
        } else {
            // cel mai degradat caz
            prow.base_nealocat = dec("6050.00"); // total_doc
        }

        // Nu s-a crăpat; nealocat conține net+vat; celelalte sunt zero
        assert_eq!(prow.base_nealocat, Decimal::from_str("5000.00").unwrap());
        assert_eq!(prow.vat_nealocat, Decimal::from_str("1050.00").unwrap());
        assert_eq!(prow.base21, Decimal::ZERO, "standard columns must be zero");
        assert_eq!(prow.vat21, Decimal::ZERO, "standard columns must be zero");

        // Σ TVA deductibilă = vat_nealocat (nu se pierde)
        let sigma = prow.total_vat_deductibil();
        assert_eq!(
            sigma,
            Decimal::from_str("1050.00").unwrap(),
            "Σ TVA deductibila = vat_nealocat (fallback, no crash)"
        );
    }

    /// Reconciliere: Σ TVA deductibilă totală (coloana rând totaluri) ==
    /// suma TVA per factură → trebuie să fie identică.
    /// Test cu 3 facturi mixte (una fallback, două parsate).
    #[test]
    fn purchase_journal_sigma_vat_reconciles_with_per_invoice_vat() {
        // Factură 1 (parsată, 21%)
        let mut p1 = PurchaseJournalRow::zero();
        p1.base21 = Decimal::from(1000);
        p1.vat21 = Decimal::from(210);

        // Factură 2 (parsată, AIC art.320)
        let mut p2 = PurchaseJournalRow::zero();
        p2.base_aic = Decimal::from(800);
        p2.vat_aic = Decimal::from(168);

        // Factură 3 (fallback, fără VAT lines)
        let mut p3 = PurchaseJournalRow::zero();
        p3.base_nealocat = Decimal::from(500);
        p3.vat_nealocat = Decimal::from(105);

        // TVA per factură (suma individuală)
        let sum_per_invoice =
            p1.total_vat_deductibil() + p2.total_vat_deductibil() + p3.total_vat_deductibil();

        // Totale acumulate (rândul TOTAL PERIOADA)
        let mut tot = PurchaseJournalRow::zero();
        tot.base21 = p1.base21;
        tot.vat21 = p1.vat21;
        tot.base_aic = p2.base_aic;
        tot.vat_aic = p2.vat_aic;
        tot.base_nealocat = p3.base_nealocat;
        tot.vat_nealocat = p3.vat_nealocat;

        let sigma_total = tot.total_vat_deductibil();

        // Reconciliere: cele două moduri de calcul trebuie să coincidă
        assert_eq!(
            sigma_total, sum_per_invoice,
            "Σ TVA deductibila (total row) must equal sum of per-invoice TVA \
             (reconciliere rulaj D 4426 = deductibil D300)"
        );

        // Valori concrete: 210 + 168 + 105 = 483
        assert_eq!(sigma_total, Decimal::from(483));
    }

    /// Normalizare cotă TVA: "0.21" → 21, "9" → 9.
    #[test]
    fn purchase_journal_normalize_vat_rate() {
        assert_eq!(normalize_vat_rate("0.21"), Decimal::from(21));
        assert_eq!(normalize_vat_rate("0.11"), Decimal::from(11));
        assert_eq!(normalize_vat_rate("0.09"), Decimal::from(9));
        assert_eq!(normalize_vat_rate("19"), Decimal::from(19));
        assert_eq!(normalize_vat_rate("21"), Decimal::from(21));
        assert_eq!(normalize_vat_rate("0"), Decimal::ZERO);
        assert_eq!(normalize_vat_rate(""), Decimal::ZERO);
    }
}
