//! Declarații fiscale — D300 Decont TVA (vânzări + achiziții).
//!
//! D300 este decontul de TVA lunar/trimestrial depus la ANAF.
//! Această implementare acoperă:
//! - **Vânzări** (TVA colectată): din facturile cu status VALIDATED.
//! - **Achiziții** (TVA deductibilă): din received_invoice_vat_lines (Wave B).
//!   Facturile primite fără defalcare TVA (net_amount IS NULL) sunt raportate
//!   separat prin `purchase_unparsed_count`.

use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::Row;
use std::collections::BTreeMap;
use std::str::FromStr;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

// ── Structs ───────────────────────────────────────────────────────────────────

/// Un grup de TVA colectat (cotă + categorie).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct D300Group {
    /// Cota TVA (e.g. "19.00", "9.00", "5.00", "0.00").
    pub vat_rate: String,
    /// Categoria TVA (BIZ-12: "S", "Z", "E", "AE", "K", "G", "O").
    pub vat_category: String,
    /// Baza impozabilă (subtotal net), aranjată cu 2 zecimale.
    pub base: String,
    /// TVA colectat, aranjat cu 2 zecimale.
    pub vat: String,
}

/// Raportul D300 — TVA colectat (vânzări) + TVA deductibil (achiziții).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct D300Report {
    /// CUI-ul companiei emitente.
    pub company_cui: String,
    /// Data de început a perioadei (YYYY-MM-DD).
    pub period_from: String,
    /// Data de sfârșit a perioadei (YYYY-MM-DD).
    pub period_to: String,
    /// Grupuri TVA colectat sortate descrescător după cotă.
    pub groups: Vec<D300Group>,
    /// Total baze impozabile (RON), 2 zecimale.
    pub total_base: String,
    /// Total TVA colectat (RON), 2 zecimale.
    pub total_vat: String,
    /// Numărul de facturi VALIDATED incluse.
    pub invoice_count: i64,
    // ── Wave B: achiziții ────────────────────────────────────────────────────
    /// Grupuri TVA deductibil (achiziții), din received_invoice_vat_lines.
    pub purchase_groups: Vec<D300Group>,
    /// Total baze impozabile achiziții (RON), 2 zecimale.
    pub total_deductible_base: String,
    /// Total TVA deductibil (RON), 2 zecimale.
    pub total_deductible_vat: String,
    /// Numărul de facturi primite (status != REJECTED) în perioadă.
    pub purchase_invoice_count: i64,
    /// Facturi primite fără defalcare TVA (net_amount IS NULL) — date parțiale.
    pub purchase_unparsed_count: i64,
    /// TVA netă de plată = TVA colectată − TVA deductibilă (negativă = de recuperat).
    pub net_vat: String,
}

// ── Commands ──────────────────────────────────────────────────────────────────

/// Calculează decontul D300 (TVA colectat — vânzări + TVA deductibil — achiziții)
/// pentru o companie și o perioadă.
///
/// **Vânzări**: doar facturile cu status VALIDATED (BIZ-11).
/// **Achiziții**: facturile primite cu status != REJECTED, defalcate din
/// `received_invoice_vat_lines`. Facturile fără defalcare sunt contorizate în
/// `purchase_unparsed_count` și nu contribuie la totaluri.
/// Gruparea se face după (cotă, categorie) — BIZ-12.
#[tauri::command]
pub async fn compute_d300(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
) -> AppResult<D300Report> {
    use rust_decimal::prelude::ToPrimitive;

    let pool = &state.db;

    // Fetch CUI-ul companiei.
    let company_row = sqlx::query("SELECT cui FROM companies WHERE id = ?1 LIMIT 1")
        .bind(&company_id)
        .fetch_optional(pool)
        .await
        .map_err(AppError::Database)?
        .ok_or(AppError::NotFound)?;

    let company_cui: String = company_row
        .try_get("cui")
        .unwrap_or_else(|_| company_id.clone());

    // Numărul total de facturi VALIDATED în perioadă (pentru header).
    let count_row = sqlx::query(
        "SELECT COUNT(*) as cnt FROM invoices \
         WHERE status = 'VALIDATED' \
           AND issue_date >= ?1 \
           AND issue_date <= ?2 \
           AND company_id = ?3",
    )
    .bind(&period_from)
    .bind(&period_to)
    .bind(&company_id)
    .fetch_one(pool)
    .await
    .map_err(AppError::Database)?;

    let invoice_count: i64 = count_row.try_get("cnt").unwrap_or(0);

    // Fetch liniile de factură pentru grupare TVA — refolosind query-ul din reports.rs.
    // BIZ-12: grupăm după (vat_rate, vat_category) — cote identice cu categorii diferite
    // (e.g. 0% Scutit "E" vs. 0% Zero-rated "Z") rămân rânduri separate.
    let line_rows = sqlx::query(
        "SELECT l.vat_rate, l.vat_category, l.subtotal_amount, l.vat_amount \
         FROM invoice_line_items l \
         JOIN invoices i ON i.id = l.invoice_id \
         WHERE i.status = 'VALIDATED' \
           AND i.issue_date >= ?1 \
           AND i.issue_date <= ?2 \
           AND i.company_id = ?3",
    )
    .bind(&period_from)
    .bind(&period_to)
    .bind(&company_id)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    // Acumulăm în BTreeMap<(rate_key_i64, category), (rate_dec, base_sum, vat_sum)>
    // — același pattern ca în reports.rs::generate_vat_report.
    let mut groups: BTreeMap<(i64, String), (Decimal, Decimal, Decimal)> = BTreeMap::new();

    for row in &line_rows {
        let rate_s: String = row.try_get("vat_rate").unwrap_or_default();
        let category: String = row
            .try_get("vat_category")
            .unwrap_or_else(|_| String::from("S"));
        let base_s: String = row.try_get("subtotal_amount").unwrap_or_default();
        let vat_s: String = row.try_get("vat_amount").unwrap_or_default();

        let rate = Decimal::from_str(&rate_s).unwrap_or(Decimal::ZERO);
        let rate_key = (rate * Decimal::from(100)).round().to_i64().unwrap_or(0);

        let e = groups
            .entry((rate_key, category))
            .or_insert((rate, Decimal::ZERO, Decimal::ZERO));
        e.1 += Decimal::from_str(&base_s).unwrap_or(Decimal::ZERO);
        e.2 += Decimal::from_str(&vat_s).unwrap_or(Decimal::ZERO);
    }

    // Calculăm totalurile și construim Vec<D300Group> descrescător după cotă.
    let mut total_base = Decimal::ZERO;
    let mut total_vat = Decimal::ZERO;

    // BTreeMap e crescător → rev() pentru descrescător după cotă (ca în reports.rs).
    let groups_vec: Vec<D300Group> = groups
        .into_iter()
        .rev()
        .map(|((_rate_key, category), (rate, base_sum, vat_sum))| {
            total_base += base_sum;
            total_vat += vat_sum;
            D300Group {
                vat_rate: rate.round_dp(2).to_string(),
                vat_category: category,
                base: base_sum.round_dp(2).to_string(),
                vat: vat_sum.round_dp(2).to_string(),
            }
        })
        .collect();

    // ── Wave B: achiziții — received_invoice_vat_lines ────────────────────────

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

    // Fetch liniile VAT din facturile primite (JOIN pentru filtru perioadă + companie).
    let purchase_line_rows = sqlx::query(
        "SELECT vl.vat_rate, vl.vat_category, vl.base_amount, vl.vat_amount \
         FROM received_invoice_vat_lines vl \
         JOIN received_invoices ri ON ri.id = vl.received_invoice_id \
         WHERE ri.company_id = ?1 \
           AND ri.issue_date >= ?2 \
           AND ri.issue_date <= ?3 \
           AND ri.status != 'REJECTED'",
    )
    .bind(&company_id)
    .bind(&period_from)
    .bind(&period_to)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;

    // Acumulăm per (rate_key, category) — același pattern ca vânzări.
    let mut purchase_groups: BTreeMap<(i64, String), (Decimal, Decimal, Decimal)> = BTreeMap::new();

    for row in &purchase_line_rows {
        let rate_s: String = row.try_get("vat_rate").unwrap_or_default();
        let category: String = row
            .try_get("vat_category")
            .unwrap_or_else(|_| String::from("S"));
        let base_s: String = row.try_get("base_amount").unwrap_or_default();
        let vat_s: String = row.try_get("vat_amount").unwrap_or_default();

        let rate = Decimal::from_str(&rate_s).unwrap_or(Decimal::ZERO);
        let rate_key = (rate * Decimal::from(100)).round().to_i64().unwrap_or(0);

        let e = purchase_groups.entry((rate_key, category)).or_insert((
            rate,
            Decimal::ZERO,
            Decimal::ZERO,
        ));
        e.1 += Decimal::from_str(&base_s).unwrap_or(Decimal::ZERO);
        e.2 += Decimal::from_str(&vat_s).unwrap_or(Decimal::ZERO);
    }

    let mut total_deductible_base = Decimal::ZERO;
    let mut total_deductible_vat = Decimal::ZERO;

    let purchase_groups_vec: Vec<D300Group> = purchase_groups
        .into_iter()
        .rev()
        .map(|((_rate_key, category), (rate, base_sum, vat_sum))| {
            total_deductible_base += base_sum;
            total_deductible_vat += vat_sum;
            D300Group {
                vat_rate: rate.round_dp(2).to_string(),
                vat_category: category,
                base: base_sum.round_dp(2).to_string(),
                vat: vat_sum.round_dp(2).to_string(),
            }
        })
        .collect();

    // TVA netă de plată = colectată − deductibilă (negativă = de recuperat).
    let net_vat = total_vat - total_deductible_vat;

    Ok(D300Report {
        company_cui,
        period_from,
        period_to,
        groups: groups_vec,
        total_base: total_base.round_dp(2).to_string(),
        total_vat: total_vat.round_dp(2).to_string(),
        invoice_count,
        purchase_groups: purchase_groups_vec,
        total_deductible_base: total_deductible_base.round_dp(2).to_string(),
        total_deductible_vat: total_deductible_vat.round_dp(2).to_string(),
        purchase_invoice_count,
        purchase_unparsed_count,
        net_vat: net_vat.round_dp(2).to_string(),
    })
}

/// Generează fișierul XML D300 și îl scrie la calea specificată.
/// Returnează calea fișierului salvat.
///
/// Formatul XML este un extract structurat al decontului D300 pentru vânzări + achiziții.
/// Header: CUI, perioadă, tip declarație. Body: grupuri TVA colectat + deductibil + TVA netă.
/// NOTE: Nu este formularul complet ANAF D300 cu schema oficială — depunerea
/// electronică necesită integrare cu sistemul ANAF e-Formulare.
#[tauri::command]
pub async fn export_d300(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
    dest_path: String,
) -> AppResult<String> {
    // Calculăm mai întâi raportul.
    let report = compute_d300(state, company_id, period_from, period_to).await?;

    let dest = dest_path.clone();
    // Construim XML-ul în spawn_blocking (I/O + string building) — pattern din saft.rs.
    tokio::task::spawn_blocking(move || build_and_write_xml(report, dest))
        .await
        .map_err(|e| AppError::Other(e.to_string()))?
}

// ── XML builder ───────────────────────────────────────────────────────────────

fn build_and_write_xml(report: D300Report, dest_path: String) -> AppResult<String> {
    let generated_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let mut xml = String::with_capacity(8192);

    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<!-- D300 Decont TVA — Extras vânzări + achiziții generat de RoFactura -->\n");
    xml.push_str("<!-- Schema oficială ANAF D300 necesită depunere prin e-Formulare.      -->\n");
    xml.push_str("<D300>\n");

    // ── Header ────────────────────────────────────────────────────────────────
    xml.push_str("  <Header>\n");
    xml.push_str("    <TipDeclaratie>D300</TipDeclaratie>\n");
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
        "    <NrFacturiValidate>{}</NrFacturiValidate>\n",
        report.invoice_count
    ));
    xml.push_str("  </Header>\n");

    // ── VanzariTVAColectat (livrări) ──────────────────────────────────────────
    xml.push_str("  <VanzariTVAColectat>\n");
    xml.push_str("    <!-- Grupuri TVA sortate descrescător după cotă -->\n");

    for group in &report.groups {
        xml.push_str("    <Grupa>\n");
        xml.push_str(&format!(
            "      <CotaTVA>{}</CotaTVA>\n",
            xml_escape(&group.vat_rate)
        ));
        xml.push_str(&format!(
            "      <CategorieTVA>{}</CategorieTVA>\n",
            xml_escape(&group.vat_category)
        ));
        xml.push_str(&format!(
            "      <BazaImpozabila>{}</BazaImpozabila>\n",
            xml_escape(&group.base)
        ));
        xml.push_str(&format!(
            "      <TVAColectat>{}</TVAColectat>\n",
            xml_escape(&group.vat)
        ));
        xml.push_str("    </Grupa>\n");
    }

    xml.push_str(&format!(
        "    <TotalBazaImpozabila>{}</TotalBazaImpozabila>\n",
        xml_escape(&report.total_base)
    ));
    xml.push_str(&format!(
        "    <TotalTVAColectat>{}</TotalTVAColectat>\n",
        xml_escape(&report.total_vat)
    ));
    xml.push_str("  </VanzariTVAColectat>\n");

    // ── AchizitiiTVADeductibil ────────────────────────────────────────────────
    xml.push_str("  <AchizitiiTVADeductibil>\n");
    xml.push_str("    <!-- TVA deductibilă din received_invoice_vat_lines (Wave B) -->\n");
    if report.purchase_unparsed_count > 0 {
        xml.push_str(&format!(
            "    <!-- ATENȚIE: {} facturi primite nu au defalcare TVA (net_amount IS NULL) — cifrele de mai jos sunt parțiale. -->\n",
            report.purchase_unparsed_count
        ));
    }

    for group in &report.purchase_groups {
        xml.push_str("    <Grupa>\n");
        xml.push_str(&format!(
            "      <CotaTVA>{}</CotaTVA>\n",
            xml_escape(&group.vat_rate)
        ));
        xml.push_str(&format!(
            "      <CategorieTVA>{}</CategorieTVA>\n",
            xml_escape(&group.vat_category)
        ));
        xml.push_str(&format!(
            "      <BazaImpozabila>{}</BazaImpozabila>\n",
            xml_escape(&group.base)
        ));
        xml.push_str(&format!(
            "      <TVADeductibil>{}</TVADeductibil>\n",
            xml_escape(&group.vat)
        ));
        xml.push_str("    </Grupa>\n");
    }

    xml.push_str(&format!(
        "    <TotalBazaImpozabila>{}</TotalBazaImpozabila>\n",
        xml_escape(&report.total_deductible_base)
    ));
    xml.push_str(&format!(
        "    <TotalTVADeductibil>{}</TotalTVADeductibil>\n",
        xml_escape(&report.total_deductible_vat)
    ));
    xml.push_str("  </AchizitiiTVADeductibil>\n");

    // ── TVA netă de plată / de recuperat ─────────────────────────────────────
    xml.push_str(&format!(
        "  <!-- TVADePlata = TVA colectată − TVA deductibilă; negativ înseamnă de recuperat -->\n  <TVADePlata>{}</TVADePlata>\n",
        xml_escape(&report.net_vat)
    ));

    xml.push_str("</D300>\n");

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

    /// Verifică că gruparea după (cotă, categorie) produce rânduri distincte —
    /// același comportament ca BIZ-12 din reports.rs.
    #[test]
    fn d300_groups_split_by_rate_and_category() {
        use std::collections::BTreeMap;

        let mut groups: BTreeMap<(i64, String), (Decimal, Decimal, Decimal)> = BTreeMap::new();

        // 19% Standard
        let rate_19 = (Decimal::from(19) * Decimal::from(100))
            .round()
            .to_string()
            .parse::<i64>()
            .unwrap_or(1900);
        let e = groups.entry((rate_19, "S".to_string())).or_insert((
            Decimal::from_str("0.19").unwrap(),
            Decimal::ZERO,
            Decimal::ZERO,
        ));
        e.1 += Decimal::from_str("1000.00").unwrap();
        e.2 += Decimal::from_str("190.00").unwrap();

        // 0% Scutit (E) și 0% Zero-rated (Z) — trebuie să rămână separate
        let rate_0 = 0_i64;
        for (cat, base, vat) in [("E", "200.00", "0.00"), ("Z", "100.00", "0.00")] {
            let e = groups.entry((rate_0, cat.to_string())).or_insert((
                Decimal::ZERO,
                Decimal::ZERO,
                Decimal::ZERO,
            ));
            e.1 += Decimal::from_str(base).unwrap();
            e.2 += Decimal::from_str(vat).unwrap();
        }

        assert_eq!(
            groups.len(),
            3,
            "19%S, 0%E și 0%Z trebuie să fie 3 grupuri distincte"
        );
        assert_eq!(
            groups[&(rate_19, "S".to_string())].1,
            Decimal::from_str("1000.00").unwrap()
        );
        assert_eq!(
            groups[&(rate_0, "E".to_string())].1,
            Decimal::from_str("200.00").unwrap()
        );
        assert_eq!(
            groups[&(rate_0, "Z".to_string())].1,
            Decimal::from_str("100.00").unwrap()
        );
    }

    /// Verifică acumularea exactă Decimal (fără drift float).
    #[test]
    fn d300_decimal_accumulation_exact() {
        let amounts = ["1000.00", "200.50", "350.75"];
        let total: Decimal = amounts.iter().map(|s| Decimal::from_str(s).unwrap()).sum();
        assert_eq!(total, Decimal::from_str("1551.25").unwrap());
    }

    /// Verifică că xml_escape scapă corect caracterele speciale.
    #[test]
    fn xml_escape_handles_special_chars() {
        assert_eq!(xml_escape("RO & SRL <test>"), "RO &amp; SRL &lt;test&gt;");
        assert_eq!(xml_escape("19.00"), "19.00");
        assert_eq!(xml_escape(""), "");
    }

    /// Verifică că build_and_write_xml produce un XML valid cu elementele cerute.
    #[test]
    fn build_xml_contains_required_elements() {
        let report = D300Report {
            company_cui: "RO12345678".to_string(),
            period_from: "2024-01-01".to_string(),
            period_to: "2024-01-31".to_string(),
            groups: vec![D300Group {
                vat_rate: "19.00".to_string(),
                vat_category: "S".to_string(),
                base: "1000.00".to_string(),
                vat: "190.00".to_string(),
            }],
            total_base: "1000.00".to_string(),
            total_vat: "190.00".to_string(),
            invoice_count: 5,
            purchase_groups: vec![D300Group {
                vat_rate: "19.00".to_string(),
                vat_category: "S".to_string(),
                base: "500.00".to_string(),
                vat: "95.00".to_string(),
            }],
            total_deductible_base: "500.00".to_string(),
            total_deductible_vat: "95.00".to_string(),
            purchase_invoice_count: 3,
            purchase_unparsed_count: 0,
            net_vat: "95.00".to_string(),
        };

        let dir = std::env::temp_dir();
        let path = dir.join("test_d300.xml");
        let result = build_and_write_xml(report, path.to_string_lossy().to_string());
        assert!(result.is_ok());

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("<D300>"));
        assert!(content.contains("<TipDeclaratie>D300</TipDeclaratie>"));
        assert!(content.contains("<CUI>RO12345678</CUI>"));
        assert!(content.contains("<CotaTVA>19.00</CotaTVA>"));
        assert!(content.contains("<TVAColectat>190.00</TVAColectat>"));
        assert!(content.contains("<TotalTVAColectat>190.00</TotalTVAColectat>"));
        assert!(content.contains("<NrFacturiValidate>5</NrFacturiValidate>"));
        assert!(content.contains("<AchizitiiTVADeductibil>"));
        assert!(content.contains("<TVADeductibil>95.00</TVADeductibil>"));
        assert!(content.contains("<TotalTVADeductibil>95.00</TotalTVADeductibil>"));
        assert!(content.contains("<TVADePlata>95.00</TVADePlata>"));

        let _ = std::fs::remove_file(&path);
    }

    /// Verifică că nota de facturi neparsate apare în XML când purchase_unparsed_count > 0.
    #[test]
    fn build_xml_includes_unparsed_note_when_needed() {
        let report = D300Report {
            company_cui: "RO11111111".to_string(),
            period_from: "2024-02-01".to_string(),
            period_to: "2024-02-29".to_string(),
            groups: vec![],
            total_base: "0.00".to_string(),
            total_vat: "0.00".to_string(),
            invoice_count: 0,
            purchase_groups: vec![],
            total_deductible_base: "0.00".to_string(),
            total_deductible_vat: "0.00".to_string(),
            purchase_invoice_count: 5,
            purchase_unparsed_count: 3,
            net_vat: "0.00".to_string(),
        };

        let dir = std::env::temp_dir();
        let path = dir.join("test_d300_unparsed.xml");
        let result = build_and_write_xml(report, path.to_string_lossy().to_string());
        assert!(result.is_ok());

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("3 facturi primite nu au defalcare TVA"),
            "XML trebuie să conțină nota pentru facturi neparsate"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// Verifică că net_vat = colectată − deductibilă, inclusiv cazul negativ (TVA de recuperat).
    #[test]
    fn d300_net_vat_can_be_negative() {
        let collected = Decimal::from_str("100.00").unwrap();
        let deductible = Decimal::from_str("150.00").unwrap();
        let net = collected - deductible;
        assert_eq!(net, Decimal::from_str("-50.00").unwrap());
        assert!(
            net.is_sign_negative(),
            "TVA de recuperat trebuie să fie negativă"
        );
    }

    /// Verifică gruparea achiziții pe (rată, categorie) — același pattern ca vânzări.
    #[test]
    fn d300_purchase_groups_split_by_rate_and_category() {
        use rust_decimal::prelude::ToPrimitive;
        use std::collections::BTreeMap;

        let mut purchase_groups: BTreeMap<(i64, String), (Decimal, Decimal, Decimal)> =
            BTreeMap::new();

        // Simulăm 2 linii la 19% S și una la 9% S.
        for (rate_str, cat, base_str, vat_str) in [
            ("0.19", "S", "1000.00", "190.00"),
            ("0.19", "S", "500.00", "95.00"),
            ("0.09", "S", "200.00", "18.00"),
        ] {
            let rate = Decimal::from_str(rate_str).unwrap();
            let rate_key = (rate * Decimal::from(100)).round().to_i64().unwrap_or(0);
            let e = purchase_groups
                .entry((rate_key, cat.to_string()))
                .or_insert((rate, Decimal::ZERO, Decimal::ZERO));
            e.1 += Decimal::from_str(base_str).unwrap();
            e.2 += Decimal::from_str(vat_str).unwrap();
        }

        assert_eq!(purchase_groups.len(), 2, "Trebuie 2 grupuri: 19%S și 9%S");

        let rate_19_key = (Decimal::from_str("0.19").unwrap() * Decimal::from(100))
            .round()
            .to_i64()
            .unwrap();
        let rate_9_key = (Decimal::from_str("0.09").unwrap() * Decimal::from(100))
            .round()
            .to_i64()
            .unwrap();

        let g19 = &purchase_groups[&(rate_19_key, "S".to_string())];
        assert_eq!(g19.1, Decimal::from_str("1500.00").unwrap());
        assert_eq!(g19.2, Decimal::from_str("285.00").unwrap());

        let g9 = &purchase_groups[&(rate_9_key, "S".to_string())];
        assert_eq!(g9.1, Decimal::from_str("200.00").unwrap());
        assert_eq!(g9.2, Decimal::from_str("18.00").unwrap());
    }
}
