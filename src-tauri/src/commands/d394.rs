//! D394 — Declarația informativă privind livrările/prestările și achizițiile
//! pe teritoriul național.
//!
//! Implementăm **livrările (vânzări)** din facturi VALIDATED, grupate pe partener
//! (contact CUI + legal_name), cu baza impozabilă și TVA totale per partener.
//! Achizițiile sunt un placeholder onest — `received_invoices` stochează doar
//! totalul, fără defalcare net/TVA (BIZ: se va adăuga după parsarea XML-ului
//! facturilor primite).

use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::Row;
use std::collections::BTreeMap;
use std::str::FromStr;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

// ── Structs ───────────────────────────────────────────────────────────────────

/// Un partener din raportul D394 — livrări.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct D394Partner {
    /// CUI-ul partenerului (client). Poate fi "" dacă nu e completat.
    pub partner_cui: String,
    /// Denumirea legală a partenerului.
    pub partner_name: String,
    /// Numărul de facturi VALIDATED emise către partener în perioadă.
    pub invoice_count: i64,
    /// Baza impozabilă totală (net), 2 zecimale.
    pub base: String,
    /// TVA colectat total, 2 zecimale.
    pub vat: String,
}

/// Raportul D394 — livrări (vânzări) per partener.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct D394Report {
    /// CUI-ul companiei emitente.
    pub company_cui: String,
    /// Data de început a perioadei (YYYY-MM-DD).
    pub period_from: String,
    /// Data de sfârșit a perioadei (YYYY-MM-DD).
    pub period_to: String,
    /// Parteneri sortați descrescător după baza impozabilă.
    pub partners: Vec<D394Partner>,
    /// Total baze impozabile (RON), 2 zecimale.
    pub total_base: String,
    /// Total TVA colectat (RON), 2 zecimale.
    pub total_vat: String,
    /// Numărul total de facturi VALIDATED incluse.
    pub invoice_count: i64,
}

// ── Commands ──────────────────────────────────────────────────────────────────

/// Calculează declarația D394 — livrări (vânzări) grupate pe partener,
/// pentru o companie și o perioadă.
///
/// Sunt incluse DOAR facturile cu status VALIDATED (BIZ-11).
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
    let rows = sqlx::query(
        "SELECT i.contact_id, \
                COALESCE(c.cui, '') AS partner_cui, \
                c.legal_name AS partner_name, \
                COUNT(*) AS invoice_count, \
                GROUP_CONCAT(i.subtotal_amount, '|') AS subtotals, \
                GROUP_CONCAT(i.vat_amount, '|') AS vats \
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

        let base_sum: Decimal = subtotals
            .split('|')
            .map(|s| Decimal::from_str(s.trim()).unwrap_or(Decimal::ZERO))
            .fold(Decimal::ZERO, |acc, v| acc + v);

        let vat_sum: Decimal = vats
            .split('|')
            .map(|s| Decimal::from_str(s.trim()).unwrap_or(Decimal::ZERO))
            .fold(Decimal::ZERO, |acc, v| acc + v);

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

    Ok(D394Report {
        company_cui,
        period_from,
        period_to,
        partners: partners_vec,
        total_base: total_base.round_dp(2).to_string(),
        total_vat: total_vat.round_dp(2).to_string(),
        invoice_count: total_invoice_count,
    })
}

/// Generează fișierul XML D394 și îl scrie la calea specificată.
/// Returnează calea fișierului salvat.
///
/// Formatul XML conține livrările (vânzări) per partener și un bloc
/// `<Achizitii>` placeholder (received_invoices nu are defalcare TVA).
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
    xml.push_str("<!-- ATENȚIE: Livrările (vânzări) sunt calculate din facturi VALIDATED.   -->\n");
    xml.push_str(
        "<!-- Achizițiile necesită parsarea XML-ului facturilor primite (placeholder).-->\n",
    );
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

    // ── Achizitii (placeholder) ───────────────────────────────────────────────
    xml.push_str("  <Achizitii>\n");
    xml.push_str(
        "    <!-- Neimplementat: received_invoices stochează doar totalul facturilor primite. -->\n",
    );
    xml.push_str(
        "    <!-- Defalcarea net/TVA va fi disponibilă după parsarea XML-ului UBL primit.   -->\n",
    );
    xml.push_str("    <TotalBazaImpozabila>0.00</TotalBazaImpozabila>\n");
    xml.push_str("    <TotalTVA>0.00</TotalTVA>\n");
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

    /// Verifică că build_and_write_xml produce un XML valid cu elementele cerute.
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
        // Achizitii placeholder
        assert!(content.contains("<Achizitii>"));
        assert!(content.contains("Neimplementat"));

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
}
