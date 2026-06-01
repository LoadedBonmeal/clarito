//! D394 — Declarația informativă privind livrările/prestările și achizițiile
//! pe teritoriul național.
//!
//! Implementăm **livrările (vânzări)** din facturi VALIDATED, grupate pe partener
//! (contact CUI + legal_name), cu baza impozabilă și TVA totale per partener.
//! **Achizițiile** (Wave B): received_invoices + received_invoice_vat_lines,
//! grupate per furnizor (issuer_cui + issuer_name). Facturile fără defalcare
//! TVA (net_amount IS NULL) sunt contorizate în `purchase_unparsed_count` și
//! nu contribuie la totaluri.

use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::Row;
use std::collections::BTreeMap;
use std::str::FromStr;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::ubl::fx::{amount_to_ron, parse_rate};

// ── Structs ───────────────────────────────────────────────────────────────────

/// Un partener din raportul D394 — livrări sau achiziții.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct D394Partner {
    /// CUI-ul partenerului (client/furnizor). Poate fi "" dacă nu e completat.
    pub partner_cui: String,
    /// Denumirea legală a partenerului.
    pub partner_name: String,
    /// Numărul de facturi VALIDATED emise/primite în perioadă.
    pub invoice_count: i64,
    /// Baza impozabilă totală (net), 2 zecimale.
    pub base: String,
    /// TVA colectat/deductibil total, 2 zecimale.
    pub vat: String,
}

/// Raportul D394 — livrări (vânzări) + achiziții per partener.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct D394Report {
    /// CUI-ul companiei emitente.
    pub company_cui: String,
    /// Data de început a perioadei (YYYY-MM-DD).
    pub period_from: String,
    /// Data de sfârșit a perioadei (YYYY-MM-DD).
    pub period_to: String,
    /// Parteneri livrări sortați descrescător după baza impozabilă.
    pub partners: Vec<D394Partner>,
    /// Total baze impozabile livrări (RON), 2 zecimale.
    pub total_base: String,
    /// Total TVA colectat livrări (RON), 2 zecimale.
    pub total_vat: String,
    /// Numărul total de facturi VALIDATED incluse (livrări).
    pub invoice_count: i64,
    // ── Wave B: achiziții ────────────────────────────────────────────────────
    /// Parteneri achiziții (furnizori) sortați descrescător după baza impozabilă.
    /// Include doar furnizorii cu cel puțin o linie VAT parsată.
    pub purchase_partners: Vec<D394Partner>,
    /// Total baze impozabile achiziții (RON), 2 zecimale.
    pub total_purchase_base: String,
    /// Total TVA deductibil achiziții (RON), 2 zecimale.
    pub total_purchase_vat: String,
    /// Numărul de facturi primite (status != REJECTED) în perioadă.
    pub purchase_invoice_count: i64,
    /// Facturi primite fără defalcare TVA (net_amount IS NULL).
    pub purchase_unparsed_count: i64,
}

// ── Commands ──────────────────────────────────────────────────────────────────

/// Calculează declarația D394 — livrări (vânzări) grupate pe partener +
/// achiziții (Wave B) grupate pe furnizor, pentru o companie și o perioadă.
///
/// **Livrări**: doar facturile cu status VALIDATED (BIZ-11).
/// **Achiziții**: received_invoices (status != REJECTED), JOIN cu
/// received_invoice_vat_lines. Furnizorii fără nicio linie VAT parsată
/// contribuie la `purchase_unparsed_count` dar NU la `purchase_partners`.
/// Partenerii sunt sortați descrescător după baza impozabilă.
#[tauri::command]
pub async fn compute_d394(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
) -> AppResult<D394Report> {
    let pool = &state.db;

    // Fetch CUI-ul companiei — pattern identic cu compute_d300.
    let company_row = sqlx::query("SELECT cui FROM companies WHERE id = ?1 LIMIT 1")
        .bind(&company_id)
        .fetch_optional(pool)
        .await
        .map_err(AppError::Database)?
        .ok_or(AppError::NotFound)?;

    let company_cui: String = company_row
        .try_get("cui")
        .unwrap_or_else(|_| company_id.clone());

    // Query VALIDATED invoices în perioadă, JOIN contacts pentru CUI + denumire.
    // Agregăm SUM(subtotal_amount), SUM(vat_amount), COUNT(*) per contact.
    // NOTE: subtotal_amount/vat_amount sunt TEXT (migration 006) — parsăm în Rust.
    // Wave 4: also GROUP_CONCAT currency + exchange_rate for per-invoice RON conversion.
    let rows = sqlx::query(
        "SELECT i.contact_id, \
                COALESCE(c.cui, '') AS partner_cui, \
                c.legal_name AS partner_name, \
                COUNT(*) AS invoice_count, \
                GROUP_CONCAT(i.subtotal_amount, '|') AS subtotals, \
                GROUP_CONCAT(i.vat_amount, '|') AS vats, \
                GROUP_CONCAT(COALESCE(i.currency, 'RON'), '|') AS currencies, \
                GROUP_CONCAT(COALESCE(CAST(i.exchange_rate AS TEXT), ''), '|') AS rates \
         FROM invoices i \
         JOIN contacts c ON c.id = i.contact_id \
         WHERE i.status = 'VALIDATED' \
           AND i.issue_date >= ?1 \
           AND i.issue_date <= ?2 \
           AND i.company_id = ?3 \
         GROUP BY i.contact_id, c.cui, c.legal_name",
    )
    .bind(&period_from)
    .bind(&period_to)
    .bind(&company_id)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    // Acumulăm per partener într-un BTreeMap keyed by (partner_name, contact_id)
    // pentru a asigura consistență în caz de CUI duplicate.
    // Valoarea: (partner_cui, partner_name, invoice_count, base_sum, vat_sum).
    struct PartnerAcc {
        partner_cui: String,
        partner_name: String,
        invoice_count: i64,
        base: Decimal,
        vat: Decimal,
    }

    let mut partners: BTreeMap<String, PartnerAcc> = BTreeMap::new();

    for row in &rows {
        let contact_id: String = row.try_get("contact_id").unwrap_or_default();
        let partner_cui: String = row.try_get("partner_cui").unwrap_or_default();
        let partner_name: String = row.try_get("partner_name").unwrap_or_default();
        let invoice_count: i64 = row.try_get("invoice_count").unwrap_or(0);
        let subtotals: String = row.try_get("subtotals").unwrap_or_default();
        let vats: String = row.try_get("vats").unwrap_or_default();
        let currencies: String = row.try_get("currencies").unwrap_or_default();
        let rates_s: String = row.try_get("rates").unwrap_or_default();

        // Zip all parallel GROUP_CONCAT arrays for per-invoice RON conversion.
        let mut base_sum = Decimal::ZERO;
        let mut vat_sum = Decimal::ZERO;
        let vat_parts: Vec<&str> = vats.split('|').collect();
        let currency_parts: Vec<&str> = currencies.split('|').collect();
        let rate_parts: Vec<&str> = rates_s.split('|').collect();
        for (idx, sub_s) in subtotals.split('|').enumerate() {
            let sub = Decimal::from_str(sub_s.trim()).unwrap_or(Decimal::ZERO);
            let vat = Decimal::from_str(vat_parts.get(idx).copied().unwrap_or("").trim())
                .unwrap_or(Decimal::ZERO);
            let currency = currency_parts.get(idx).copied().unwrap_or("RON");
            let rate_str = rate_parts.get(idx).copied().unwrap_or("").trim();
            let fx = parse_rate(rate_str.parse::<f64>().ok());
            base_sum += amount_to_ron(sub, currency, fx);
            vat_sum += amount_to_ron(vat, currency, fx);
        }

        let acc = partners.entry(contact_id).or_insert(PartnerAcc {
            partner_cui: partner_cui.clone(),
            partner_name: partner_name.clone(),
            invoice_count: 0,
            base: Decimal::ZERO,
            vat: Decimal::ZERO,
        });
        acc.invoice_count += invoice_count;
        acc.base += base_sum;
        acc.vat += vat_sum;
    }

    // Calculăm totaluri și construim Vec<D394Partner> sortat descrescător după base.
    let mut total_base = Decimal::ZERO;
    let mut total_vat = Decimal::ZERO;
    let mut total_invoice_count: i64 = 0;

    let mut partners_vec: Vec<D394Partner> = partners
        .into_values()
        .map(|acc| {
            total_base += acc.base;
            total_vat += acc.vat;
            total_invoice_count += acc.invoice_count;
            D394Partner {
                partner_cui: acc.partner_cui,
                partner_name: acc.partner_name,
                invoice_count: acc.invoice_count,
                base: acc.base.round_dp(2).to_string(),
                vat: acc.vat.round_dp(2).to_string(),
            }
        })
        .collect();

    // Sortăm descrescător după baza impozabilă (mai util pentru declarație).
    partners_vec.sort_by(|a, b| {
        let ba = Decimal::from_str(&b.base).unwrap_or(Decimal::ZERO);
        let aa = Decimal::from_str(&a.base).unwrap_or(Decimal::ZERO);
        ba.cmp(&aa)
    });

    // ── Wave B: achiziții — received_invoices + received_invoice_vat_lines ────

    // Numărul de facturi primite în perioadă (status != REJECTED).
    let purchase_count_row = sqlx::query(
        "SELECT COUNT(*) as cnt FROM received_invoices \
         WHERE company_id = ?1 \
           AND issue_date >= ?2 \
           AND issue_date <= ?3 \
           AND status != 'REJECTED'",
    )
    .bind(&company_id)
    .bind(&period_from)
    .bind(&period_to)
    .fetch_one(pool)
    .await
    .map_err(AppError::Database)?;

    let purchase_invoice_count: i64 = purchase_count_row.try_get("cnt").unwrap_or(0);

    // Numărul de facturi primite fără defalcare TVA (net_amount IS NULL).
    let unparsed_count_row = sqlx::query(
        "SELECT COUNT(*) as cnt FROM received_invoices \
         WHERE company_id = ?1 \
           AND issue_date >= ?2 \
           AND issue_date <= ?3 \
           AND status != 'REJECTED' \
           AND net_amount IS NULL",
    )
    .bind(&company_id)
    .bind(&period_from)
    .bind(&period_to)
    .fetch_one(pool)
    .await
    .map_err(AppError::Database)?;

    let purchase_unparsed_count: i64 = unparsed_count_row.try_get("cnt").unwrap_or(0);

    // Fetch liniile VAT agregate per furnizor (issuer_cui + issuer_name).
    // Include doar furnizorii care au cel puțin o linie parsată (INNER JOIN).
    // Wave 4: also GROUP_CONCAT currency + exchange_rate per VAT line for RON conversion.
    // Note: vat_lines are at the line level but the currency/rate lives on the parent invoice.
    // We join ri to get the rate for each vl row.
    let purchase_rows = sqlx::query(
        "SELECT ri.issuer_cui, \
                ri.issuer_name, \
                COUNT(DISTINCT ri.id) AS invoice_count, \
                GROUP_CONCAT(vl.base_amount, '|') AS bases, \
                GROUP_CONCAT(vl.vat_amount, '|') AS vats, \
                GROUP_CONCAT(COALESCE(ri.currency, 'RON'), '|') AS currencies, \
                GROUP_CONCAT(COALESCE(CAST(ri.exchange_rate AS TEXT), ''), '|') AS rates \
         FROM received_invoices ri \
         JOIN received_invoice_vat_lines vl ON vl.received_invoice_id = ri.id \
         WHERE ri.company_id = ?1 \
           AND ri.issue_date >= ?2 \
           AND ri.issue_date <= ?3 \
           AND ri.status != 'REJECTED' \
         GROUP BY ri.issuer_cui, ri.issuer_name",
    )
    .bind(&company_id)
    .bind(&period_from)
    .bind(&period_to)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    struct SupplierAcc {
        issuer_cui: String,
        issuer_name: String,
        invoice_count: i64,
        base: Decimal,
        vat: Decimal,
    }

    // Keyed by (issuer_cui, issuer_name) pentru a gestiona furnizori cu același CUI.
    let mut suppliers: BTreeMap<(String, String), SupplierAcc> = BTreeMap::new();

    for row in &purchase_rows {
        let issuer_cui: String = row.try_get("issuer_cui").unwrap_or_default();
        let issuer_name: String = row.try_get("issuer_name").unwrap_or_default();
        let inv_count: i64 = row.try_get("invoice_count").unwrap_or(0);
        let bases: String = row.try_get("bases").unwrap_or_default();
        let vats_s: String = row.try_get("vats").unwrap_or_default();
        let currencies: String = row.try_get("currencies").unwrap_or_default();
        let rates_s: String = row.try_get("rates").unwrap_or_default();

        let mut base_sum = Decimal::ZERO;
        let mut vat_sum = Decimal::ZERO;
        let vat_parts: Vec<&str> = vats_s.split('|').collect();
        let currency_parts: Vec<&str> = currencies.split('|').collect();
        let rate_parts: Vec<&str> = rates_s.split('|').collect();
        for (idx, base_s) in bases.split('|').enumerate() {
            let base = Decimal::from_str(base_s.trim()).unwrap_or(Decimal::ZERO);
            let vat = Decimal::from_str(vat_parts.get(idx).copied().unwrap_or("").trim())
                .unwrap_or(Decimal::ZERO);
            let currency = currency_parts.get(idx).copied().unwrap_or("RON");
            let rate_str = rate_parts.get(idx).copied().unwrap_or("").trim();
            let fx = parse_rate(rate_str.parse::<f64>().ok());
            base_sum += amount_to_ron(base, currency, fx);
            vat_sum += amount_to_ron(vat, currency, fx);
        }

        let key = (issuer_cui.clone(), issuer_name.clone());
        let acc = suppliers.entry(key).or_insert(SupplierAcc {
            issuer_cui: issuer_cui.clone(),
            issuer_name: issuer_name.clone(),
            invoice_count: 0,
            base: Decimal::ZERO,
            vat: Decimal::ZERO,
        });
        acc.invoice_count += inv_count;
        acc.base += base_sum;
        acc.vat += vat_sum;
    }

    let mut total_purchase_base = Decimal::ZERO;
    let mut total_purchase_vat = Decimal::ZERO;

    let mut purchase_partners_vec: Vec<D394Partner> = suppliers
        .into_values()
        .map(|acc| {
            total_purchase_base += acc.base;
            total_purchase_vat += acc.vat;
            D394Partner {
                partner_cui: acc.issuer_cui,
                partner_name: acc.issuer_name,
                invoice_count: acc.invoice_count,
                base: acc.base.round_dp(2).to_string(),
                vat: acc.vat.round_dp(2).to_string(),
            }
        })
        .collect();

    // Sortăm descrescător după baza impozabilă (consistent cu livrările).
    purchase_partners_vec.sort_by(|a, b| {
        let ba = Decimal::from_str(&b.base).unwrap_or(Decimal::ZERO);
        let aa = Decimal::from_str(&a.base).unwrap_or(Decimal::ZERO);
        ba.cmp(&aa)
    });

    Ok(D394Report {
        company_cui,
        period_from,
        period_to,
        partners: partners_vec,
        total_base: total_base.round_dp(2).to_string(),
        total_vat: total_vat.round_dp(2).to_string(),
        invoice_count: total_invoice_count,
        purchase_partners: purchase_partners_vec,
        total_purchase_base: total_purchase_base.round_dp(2).to_string(),
        total_purchase_vat: total_purchase_vat.round_dp(2).to_string(),
        purchase_invoice_count,
        purchase_unparsed_count,
    })
}

/// Generează fișierul XML D394 și îl scrie la calea specificată.
/// Returnează calea fișierului salvat.
///
/// Formatul XML conține livrările (vânzări) per partener și achizițiile
/// (furnizori cu linii VAT parsate) + note pentru facturi neparsate.
#[tauri::command]
pub async fn export_d394(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
    dest_path: String,
) -> AppResult<String> {
    let report = compute_d394(state, company_id, period_from, period_to).await?;

    let dest = dest_path.clone();
    tokio::task::spawn_blocking(move || build_and_write_xml(report, dest))
        .await
        .map_err(|e| AppError::Other(e.to_string()))?
}

// ── XML builder ───────────────────────────────────────────────────────────────

fn build_and_write_xml(report: D394Report, dest_path: String) -> AppResult<String> {
    let generated_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let mut xml = String::with_capacity(8192);

    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<!-- D394 Declarație informativă livrări/achiziții — generat de RoFactura -->\n");
    xml.push_str(
        "<!-- Schema oficială ANAF D394 necesită depunere prin e-Formulare.         -->\n",
    );
    xml.push_str("<D394>\n");

    // ── Header ────────────────────────────────────────────────────────────────
    xml.push_str("  <Header>\n");
    xml.push_str("    <TipDeclaratie>D394</TipDeclaratie>\n");
    xml.push_str(&format!(
        "    <CUI>{}</CUI>\n",
        xml_escape(&report.company_cui)
    ));
    xml.push_str(&format!(
        "    <PerioadaDeLa>{}</PerioadaDeLa>\n",
        xml_escape(&report.period_from)
    ));
    xml.push_str(&format!(
        "    <PerioadaPanaLa>{}</PerioadaPanaLa>\n",
        xml_escape(&report.period_to)
    ));
    xml.push_str(&format!(
        "    <GeneratLa>{}</GeneratLa>\n",
        xml_escape(&generated_at)
    ));
    xml.push_str(&format!(
        "    <NrFacturi>{}</NrFacturi>\n",
        report.invoice_count
    ));
    xml.push_str("  </Header>\n");

    // ── Livrari (vânzări per partener) ───────────────────────────────────────
    xml.push_str("  <Livrari>\n");
    xml.push_str("    <!-- Parteneri sortați descrescător după baza impozabilă -->\n");

    for partner in &report.partners {
        xml.push_str("    <Partener>\n");
        xml.push_str(&format!(
            "      <CUI>{}</CUI>\n",
            xml_escape(&partner.partner_cui)
        ));
        xml.push_str(&format!(
            "      <Denumire>{}</Denumire>\n",
            xml_escape(&partner.partner_name)
        ));
        xml.push_str(&format!(
            "      <NrFacturi>{}</NrFacturi>\n",
            partner.invoice_count
        ));
        xml.push_str(&format!(
            "      <BazaImpozabila>{}</BazaImpozabila>\n",
            xml_escape(&partner.base)
        ));
        xml.push_str(&format!("      <TVA>{}</TVA>\n", xml_escape(&partner.vat)));
        xml.push_str("    </Partener>\n");
    }

    xml.push_str(&format!(
        "    <TotalBazaImpozabila>{}</TotalBazaImpozabila>\n",
        xml_escape(&report.total_base)
    ));
    xml.push_str(&format!(
        "    <TotalTVA>{}</TotalTVA>\n",
        xml_escape(&report.total_vat)
    ));
    xml.push_str("  </Livrari>\n");

    // ── Achizitii (Wave B — date reale din received_invoice_vat_lines) ────────
    xml.push_str("  <Achizitii>\n");
    xml.push_str(
        "    <!-- Furnizori cu linii VAT parsate, sortați descrescător după baza impozabilă -->\n",
    );
    if report.purchase_unparsed_count > 0 {
        xml.push_str(&format!(
            "    <!-- ATENȚIE: {} facturi primite nu au defalcare TVA — cifrele de mai jos sunt parțiale. -->\n",
            report.purchase_unparsed_count
        ));
    }

    for partner in &report.purchase_partners {
        xml.push_str("    <Partener>\n");
        xml.push_str(&format!(
            "      <CUI>{}</CUI>\n",
            xml_escape(&partner.partner_cui)
        ));
        xml.push_str(&format!(
            "      <Denumire>{}</Denumire>\n",
            xml_escape(&partner.partner_name)
        ));
        xml.push_str(&format!(
            "      <NrFacturi>{}</NrFacturi>\n",
            partner.invoice_count
        ));
        xml.push_str(&format!(
            "      <BazaImpozabila>{}</BazaImpozabila>\n",
            xml_escape(&partner.base)
        ));
        xml.push_str(&format!("      <TVA>{}</TVA>\n", xml_escape(&partner.vat)));
        xml.push_str("    </Partener>\n");
    }

    xml.push_str(&format!(
        "    <TotalBazaImpozabila>{}</TotalBazaImpozabila>\n",
        xml_escape(&report.total_purchase_base)
    ));
    xml.push_str(&format!(
        "    <TotalTVA>{}</TotalTVA>\n",
        xml_escape(&report.total_purchase_vat)
    ));
    xml.push_str("  </Achizitii>\n");

    xml.push_str("</D394>\n");

    std::fs::write(&dest_path, xml.as_bytes()).map_err(|e| AppError::Other(e.to_string()))?;

    Ok(dest_path)
}

/// Escapes XML special characters in a string value.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    /// Verifică acumularea exactă Decimal per partener (fără drift float).
    #[test]
    fn d394_decimal_accumulation_exact() {
        let subtotals = "1000.00|200.50|350.75";
        let total: Decimal = subtotals
            .split('|')
            .map(|s| Decimal::from_str(s.trim()).unwrap_or(Decimal::ZERO))
            .fold(Decimal::ZERO, |acc, v| acc + v);
        assert_eq!(total, Decimal::from_str("1551.25").unwrap());
    }

    /// Verifică că partenerii cu aceeași cheie de contact se acumulează corect.
    #[test]
    fn d394_partner_accumulation_groups_correctly() {
        let mut partners: BTreeMap<String, (Decimal, Decimal, i64)> = BTreeMap::new();

        // Partener A — 2 facturi
        for (base, vat) in [("1000.00", "190.00"), ("500.00", "95.00")] {
            let e = partners.entry("contact-A".to_string()).or_insert((
                Decimal::ZERO,
                Decimal::ZERO,
                0,
            ));
            e.0 += Decimal::from_str(base).unwrap();
            e.1 += Decimal::from_str(vat).unwrap();
            e.2 += 1;
        }

        // Partener B — 1 factură
        {
            let e = partners.entry("contact-B".to_string()).or_insert((
                Decimal::ZERO,
                Decimal::ZERO,
                0,
            ));
            e.0 += Decimal::from_str("300.00").unwrap();
            e.1 += Decimal::from_str("57.00").unwrap();
            e.2 += 1;
        }

        assert_eq!(partners.len(), 2, "Trebuie să fie 2 parteneri distincți");

        let a = &partners["contact-A"];
        assert_eq!(a.0, Decimal::from_str("1500.00").unwrap());
        assert_eq!(a.1, Decimal::from_str("285.00").unwrap());
        assert_eq!(a.2, 2);

        let b = &partners["contact-B"];
        assert_eq!(b.0, Decimal::from_str("300.00").unwrap());
        assert_eq!(b.2, 1);
    }

    /// Verifică că xml_escape scapă corect caracterele speciale XML.
    #[test]
    fn xml_escape_handles_special_chars() {
        assert_eq!(xml_escape("SC ALPHA & BETA SRL"), "SC ALPHA &amp; BETA SRL");
        assert_eq!(xml_escape("<test>"), "&lt;test&gt;");
        assert_eq!(xml_escape("\"quoted\""), "&quot;quoted&quot;");
        assert_eq!(xml_escape("it's"), "it&apos;s");
        assert_eq!(xml_escape("RO12345678"), "RO12345678");
        assert_eq!(xml_escape(""), "");
    }

    /// Verifică că build_and_write_xml produce un XML valid cu elementele cerute
    /// (livrări + achiziții reale).
    #[test]
    fn build_xml_contains_required_elements() {
        let report = D394Report {
            company_cui: "RO12345678".to_string(),
            period_from: "2024-01-01".to_string(),
            period_to: "2024-01-31".to_string(),
            partners: vec![D394Partner {
                partner_cui: "RO98765432".to_string(),
                partner_name: "SC CLIENT SRL".to_string(),
                invoice_count: 3,
                base: "5000.00".to_string(),
                vat: "950.00".to_string(),
            }],
            total_base: "5000.00".to_string(),
            total_vat: "950.00".to_string(),
            invoice_count: 3,
            purchase_partners: vec![D394Partner {
                partner_cui: "RO11111111".to_string(),
                partner_name: "SC FURNIZOR SRL".to_string(),
                invoice_count: 2,
                base: "2000.00".to_string(),
                vat: "380.00".to_string(),
            }],
            total_purchase_base: "2000.00".to_string(),
            total_purchase_vat: "380.00".to_string(),
            purchase_invoice_count: 2,
            purchase_unparsed_count: 0,
        };

        let dir = std::env::temp_dir();
        let path = dir.join("test_d394.xml");
        let result = build_and_write_xml(report, path.to_string_lossy().to_string());
        assert!(result.is_ok(), "build_and_write_xml trebuie să reușească");

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("<D394>"));
        assert!(content.contains("<TipDeclaratie>D394</TipDeclaratie>"));
        assert!(content.contains("<CUI>RO12345678</CUI>"));
        assert!(content.contains("<NrFacturi>3</NrFacturi>"));
        assert!(content.contains("<Livrari>"));
        assert!(content.contains("<Partener>"));
        assert!(content.contains("<CUI>RO98765432</CUI>"));
        assert!(content.contains("<Denumire>SC CLIENT SRL</Denumire>"));
        assert!(content.contains("<BazaImpozabila>5000.00</BazaImpozabila>"));
        assert!(content.contains("<TVA>950.00</TVA>"));
        assert!(content.contains("<TotalBazaImpozabila>5000.00</TotalBazaImpozabila>"));
        // Achiziții reale (Wave B)
        assert!(content.contains("<Achizitii>"));
        assert!(content.contains("<CUI>RO11111111</CUI>"));
        assert!(content.contains("<Denumire>SC FURNIZOR SRL</Denumire>"));
        assert!(content.contains("<BazaImpozabila>2000.00</BazaImpozabila>"));
        assert!(content.contains("<TVA>380.00</TVA>"));
        assert!(content.contains("<TotalBazaImpozabila>2000.00</TotalBazaImpozabila>"));
        assert!(
            !content.contains("Neimplementat"),
            "Vechiul placeholder nu mai trebuie să apară"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// Verifică că nota de facturi neparsate apare în XML când purchase_unparsed_count > 0.
    #[test]
    fn build_xml_includes_unparsed_note_when_needed() {
        let report = D394Report {
            company_cui: "RO22222222".to_string(),
            period_from: "2024-03-01".to_string(),
            period_to: "2024-03-31".to_string(),
            partners: vec![],
            total_base: "0.00".to_string(),
            total_vat: "0.00".to_string(),
            invoice_count: 0,
            purchase_partners: vec![],
            total_purchase_base: "0.00".to_string(),
            total_purchase_vat: "0.00".to_string(),
            purchase_invoice_count: 4,
            purchase_unparsed_count: 4,
        };

        let dir = std::env::temp_dir();
        let path = dir.join("test_d394_unparsed.xml");
        let result = build_and_write_xml(report, path.to_string_lossy().to_string());
        assert!(result.is_ok());

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("4 facturi primite nu au defalcare TVA"),
            "XML trebuie să conțină nota pentru facturi neparsate"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// Verifică că sortarea descrescătoare după baza impozabilă funcționează.
    #[test]
    fn d394_partners_sorted_desc_by_base() {
        let mut partners = [
            D394Partner {
                partner_cui: "".to_string(),
                partner_name: "B".to_string(),
                invoice_count: 1,
                base: "100.00".to_string(),
                vat: "19.00".to_string(),
            },
            D394Partner {
                partner_cui: "".to_string(),
                partner_name: "A".to_string(),
                invoice_count: 1,
                base: "5000.00".to_string(),
                vat: "950.00".to_string(),
            },
            D394Partner {
                partner_cui: "".to_string(),
                partner_name: "C".to_string(),
                invoice_count: 1,
                base: "1000.00".to_string(),
                vat: "190.00".to_string(),
            },
        ];

        partners.sort_by(|a, b| {
            let ba = Decimal::from_str(&b.base).unwrap_or(Decimal::ZERO);
            let aa = Decimal::from_str(&a.base).unwrap_or(Decimal::ZERO);
            ba.cmp(&aa)
        });

        assert_eq!(partners[0].partner_name, "A"); // 5000 primul
        assert_eq!(partners[1].partner_name, "C"); // 1000 al doilea
        assert_eq!(partners[2].partner_name, "B"); // 100 ultimul
    }

    // ── Wave 4: FX normalisation ──────────────────────────────────────────────

    /// Wave 4: EUR partner invoice (base=1000, vat=190, rate=5.0) → 5000/950 RON.
    #[test]
    fn d394_sales_eur_invoice_converted_to_ron() {
        use crate::ubl::fx::{amount_to_ron, parse_rate};

        let base_ron = amount_to_ron(
            Decimal::from_str("1000.00").unwrap(),
            "EUR",
            parse_rate(Some(5.0)),
        );
        let vat_ron = amount_to_ron(
            Decimal::from_str("190.00").unwrap(),
            "EUR",
            parse_rate(Some(5.0)),
        );
        assert_eq!(
            base_ron,
            Decimal::from_str("5000.00").unwrap(),
            "EUR 1000 * 5.0 must equal RON 5000"
        );
        assert_eq!(
            vat_ron,
            Decimal::from_str("950.00").unwrap(),
            "EUR 190 * 5.0 must equal RON 950"
        );
    }

    /// Wave 4: RON partner invoice is unchanged.
    #[test]
    fn d394_sales_ron_invoice_unchanged() {
        use crate::ubl::fx::{amount_to_ron, parse_rate};

        let base = Decimal::from_str("1000.00").unwrap();
        let vat = Decimal::from_str("190.00").unwrap();
        assert_eq!(amount_to_ron(base, "RON", parse_rate(Some(5.0))), base);
        assert_eq!(amount_to_ron(vat, "RON", parse_rate(Some(5.0))), vat);
    }

    /// Wave 4: GROUP_CONCAT-style per-row zip accumulation correctly converts
    /// mixed EUR+RON invoices to RON for a single partner.
    #[test]
    fn d394_partner_mixed_currency_accumulation() {
        use crate::ubl::fx::{amount_to_ron, parse_rate};

        // Simulate two invoices for the same partner:
        // Invoice A: EUR 1000 base, EUR 190 vat, rate 5.0 → 5000/950 RON
        // Invoice B: RON 1000 base, RON 190 vat, no rate → 1000/190 RON
        let subtotal_parts = ["1000.00", "1000.00"];
        let vat_parts = ["190.00", "190.00"];
        let currency_parts = ["EUR", "RON"];
        let rate_parts = ["5", ""];

        let mut base_sum = Decimal::ZERO;
        let mut vat_sum = Decimal::ZERO;
        for i in 0..subtotal_parts.len() {
            let base = Decimal::from_str(subtotal_parts[i]).unwrap();
            let vat = Decimal::from_str(vat_parts[i]).unwrap();
            let currency = currency_parts[i];
            let rate_str = rate_parts[i].trim();
            let fx = parse_rate(rate_str.parse::<f64>().ok());
            base_sum += amount_to_ron(base, currency, fx);
            vat_sum += amount_to_ron(vat, currency, fx);
        }

        assert_eq!(
            base_sum,
            Decimal::from_str("6000.00").unwrap(),
            "5000+1000=6000 RON aggregate base"
        );
        assert_eq!(
            vat_sum,
            Decimal::from_str("1140.00").unwrap(),
            "950+190=1140 RON aggregate vat"
        );
    }

    /// Wave 4: EUR purchase (received) line (base=1000, vat=190, rate=5.0) → 5000/950 RON.
    #[test]
    fn d394_purchase_eur_line_converted_to_ron() {
        use crate::ubl::fx::{amount_to_ron, parse_rate};

        let base_ron = amount_to_ron(
            Decimal::from_str("1000.00").unwrap(),
            "EUR",
            parse_rate(Some(5.0)),
        );
        let vat_ron = amount_to_ron(
            Decimal::from_str("190.00").unwrap(),
            "EUR",
            parse_rate(Some(5.0)),
        );
        assert_eq!(base_ron, Decimal::from_str("5000.00").unwrap());
        assert_eq!(vat_ron, Decimal::from_str("950.00").unwrap());
    }

    /// Wave 4: RON purchase line is unchanged.
    #[test]
    fn d394_purchase_ron_line_unchanged() {
        use crate::ubl::fx::{amount_to_ron, parse_rate};

        let base = Decimal::from_str("1000.00").unwrap();
        let vat = Decimal::from_str("190.00").unwrap();
        assert_eq!(amount_to_ron(base, "RON", parse_rate(Some(5.0))), base);
        assert_eq!(amount_to_ron(vat, "RON", parse_rate(Some(5.0))), vat);
    }

    /// Verifică că acumularea per furnizor grupează corect achizițiile.
    #[test]
    fn d394_purchase_partner_accumulation_exact() {
        let mut suppliers: BTreeMap<(String, String), (Decimal, Decimal, i64)> = BTreeMap::new();

        // Furnizor A — 2 facturi cu linii VAT multiple
        for (base, vat) in [("1000.00", "190.00"), ("500.00", "95.00")] {
            let key = ("RO11111111".to_string(), "Furnizor A SRL".to_string());
            let e = suppliers
                .entry(key)
                .or_insert((Decimal::ZERO, Decimal::ZERO, 0));
            e.0 += Decimal::from_str(base).unwrap();
            e.1 += Decimal::from_str(vat).unwrap();
            e.2 += 1;
        }

        // Furnizor B — 1 factură
        {
            let key = ("RO22222222".to_string(), "Furnizor B SRL".to_string());
            let e = suppliers
                .entry(key)
                .or_insert((Decimal::ZERO, Decimal::ZERO, 0));
            e.0 += Decimal::from_str("800.00").unwrap();
            e.1 += Decimal::from_str("152.00").unwrap();
            e.2 += 1;
        }

        assert_eq!(suppliers.len(), 2, "Trebuie 2 furnizori distincți");

        let a = &suppliers[&("RO11111111".to_string(), "Furnizor A SRL".to_string())];
        assert_eq!(a.0, Decimal::from_str("1500.00").unwrap());
        assert_eq!(a.1, Decimal::from_str("285.00").unwrap());
        assert_eq!(a.2, 2);

        let b = &suppliers[&("RO22222222".to_string(), "Furnizor B SRL".to_string())];
        assert_eq!(b.0, Decimal::from_str("800.00").unwrap());
        assert_eq!(b.2, 1);
    }
}
