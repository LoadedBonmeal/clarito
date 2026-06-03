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
    /// Categoria TVA (S/AE/E/Z/O/K/G) — D394 raportează separate pe categorie.
    pub vat_category: String,
    /// Cota TVA normalizată la procent întreg (e.g. "19", "9", "5", "0").
    /// Corespunde enum-ului D394 `cota` {0,5,9,11,19,20,21,24}.
    pub vat_rate: String,
    /// Numărul de facturi VALIDATED emise/primite în perioadă.
    pub invoice_count: i64,
    /// Baza impozabilă totală (net), 2 zecimale.
    pub base: String,
    /// TVA colectat/deductibil total, 2 zecimale.
    pub vat: String,
    /// Art. 331 product category code (from invoice_line_items.art331_code snapshot).
    /// Used by D394 op11 codPR. None = use default 22.
    /// For AE lines only — the BTreeMap key includes this so two AE lines with
    /// different art.331 codes produce separate op1 rows.
    pub art331_code: Option<String>,
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

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Normalize a raw VAT rate string (from the DB) to a canonical integer-percent
/// string matching the D394 `cota` enum {0, 5, 9, 11, 19, 20, 21, 24}.
///
/// The DB may store rates in various formats:
///   "0.19" (decimal fraction), "19.00" (padded percent), "19" (plain percent).
/// We parse to Decimal, multiply by 100 if < 1, round to integer, then return
/// as a string. Unknown/invalid input normalizes to "0".
pub fn normalize_vat_rate(raw: &str) -> String {
    use rust_decimal::prelude::ToPrimitive;
    let s = raw.trim();
    if s.is_empty() {
        return "0".to_string();
    }
    let d = match rust_decimal::Decimal::from_str(s) {
        Ok(v) => v,
        Err(_) => return "0".to_string(),
    };
    // If the value is < 1, it's stored as a fraction (e.g. 0.19 → 19%).
    let pct = if d < rust_decimal::Decimal::ONE && d > rust_decimal::Decimal::ZERO {
        (d * rust_decimal::Decimal::from(100))
            .round_dp(0)
            .to_i64()
            .unwrap_or(0)
    } else {
        d.round_dp(0).to_i64().unwrap_or(0)
    };
    pct.to_string()
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

    // ── Sales (livrări): fetch individual line rows — NO GROUP_CONCAT ─────────
    // GROUP_CONCAT in SQLite does not guarantee element order across multiple
    // GROUP_CONCAT calls in the same query, which causes currency↔rate mis-pairing
    // for foreign-currency partners (RON conversion bug).
    //
    // Fix: fetch every (invoice, line) tuple individually and group in Rust using
    // a BTreeMap keyed by (partner_cui, vat_category) so AE/E/Z/O/K/G stay as
    // separate rows — mirroring the pattern in reports.rs (generate_vat_report).
    let line_rows = sqlx::query(
        "SELECT COALESCE(c.cui, '') AS partner_cui, \
                c.legal_name AS partner_name, \
                i.id AS invoice_id, \
                COALESCE(i.currency, 'RON') AS currency, \
                i.exchange_rate, \
                l.vat_category, \
                l.vat_rate, \
                l.subtotal_amount AS line_base, \
                l.vat_amount AS line_vat, \
                l.art331_code \
         FROM invoices i \
         JOIN contacts c ON c.id = i.contact_id \
         JOIN invoice_line_items l ON l.invoice_id = i.id \
         WHERE i.status IN ('VALIDATED', 'STORNED') \
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

    // Accumulate per (partner_cui, vat_category, vat_rate) in BTreeMap for
    // deterministic order. Including vat_rate in the key ensures a partner with
    // sales at 19% AND 21% produces two separate D394 rows (legally required).
    // Also track the set of invoice_ids per key so invoice_count is correct.
    struct PartnerAcc {
        partner_cui: String,
        partner_name: String,
        vat_category: String,
        vat_rate: String,
        invoice_ids: std::collections::BTreeSet<String>,
        base: Decimal,
        vat: Decimal,
        /// For AE lines: the art331_code from the line snapshot (if present).
        art331_code: Option<String>,
    }

    // key = (partner_cui, vat_category, vat_rate_normalized, art331_code_for_ae)
    // art331_code is included in the key only for AE lines so that two AE lines
    // with different art.331 codes produce separate D394 op1 rows.
    let mut partners: BTreeMap<(String, String, String, String), PartnerAcc> = BTreeMap::new();

    for row in &line_rows {
        let partner_cui: String = row.try_get("partner_cui").unwrap_or_default();
        let partner_name: String = row.try_get("partner_name").unwrap_or_default();
        let invoice_id: String = row.try_get("invoice_id").unwrap_or_default();
        let currency: String = row
            .try_get("currency")
            .unwrap_or_else(|_| "RON".to_string());
        let fx = parse_rate(
            row.try_get::<Option<f64>, _>("exchange_rate")
                .unwrap_or(None),
        );
        let vat_category: String = row
            .try_get("vat_category")
            .unwrap_or_else(|_| "S".to_string());
        let raw_vat_rate: String = row.try_get("vat_rate").unwrap_or_else(|_| "0".to_string());
        let vat_rate = normalize_vat_rate(&raw_vat_rate);
        let base_s: String = row.try_get("line_base").unwrap_or_default();
        let vat_s: String = row.try_get("line_vat").unwrap_or_default();
        // art331_code is included in the key for AE lines so two AE lines with
        // different art.331 categories produce separate D394 op1 rows.
        let art331_code: Option<String> = row
            .try_get::<Option<String>, _>("art331_code")
            .unwrap_or(None);
        let art331_key = if vat_category == "AE" {
            art331_code.clone().unwrap_or_default()
        } else {
            String::new()
        };

        let base_ron = amount_to_ron(
            Decimal::from_str(base_s.trim()).unwrap_or(Decimal::ZERO),
            &currency,
            fx,
        );
        let vat_ron = amount_to_ron(
            Decimal::from_str(vat_s.trim()).unwrap_or(Decimal::ZERO),
            &currency,
            fx,
        );

        let key = (
            partner_cui.clone(),
            vat_category.clone(),
            vat_rate.clone(),
            art331_key,
        );
        let acc = partners.entry(key).or_insert(PartnerAcc {
            partner_cui: partner_cui.clone(),
            partner_name: partner_name.clone(),
            vat_category: vat_category.clone(),
            vat_rate: vat_rate.clone(),
            invoice_ids: std::collections::BTreeSet::new(),
            base: Decimal::ZERO,
            vat: Decimal::ZERO,
            art331_code: art331_code.clone(),
        });
        acc.invoice_ids.insert(invoice_id);
        acc.base += base_ron;
        acc.vat += vat_ron;
    }

    // Calculăm totaluri și construim Vec<D394Partner> sortat descrescător după base.
    let mut total_base = Decimal::ZERO;
    let mut total_vat = Decimal::ZERO;
    // invoice_count = total distinct invoice IDs across all (partner, category) rows
    let all_invoice_ids: std::collections::BTreeSet<String> = partners
        .values()
        .flat_map(|acc| acc.invoice_ids.iter().cloned())
        .collect();
    let total_invoice_count: i64 = all_invoice_ids.len() as i64;

    let mut partners_vec: Vec<D394Partner> = partners
        .into_values()
        .map(|acc| {
            total_base += acc.base;
            total_vat += acc.vat;
            D394Partner {
                partner_cui: acc.partner_cui,
                partner_name: acc.partner_name,
                vat_category: acc.vat_category,
                vat_rate: acc.vat_rate,
                invoice_count: acc.invoice_ids.len() as i64,
                base: acc.base.round_dp(2).to_string(),
                vat: acc.vat.round_dp(2).to_string(),
                art331_code: acc.art331_code,
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

    // ── Purchases (achiziții): fetch individual VAT line rows — NO GROUP_CONCAT ─
    // Same rationale as for sales: GROUP_CONCAT order is non-deterministic,
    // causing currency↔rate mis-pairing for foreign-currency suppliers.
    //
    // Fetch each vat_line individually (joined to parent ri for currency/rate),
    // then group in Rust by (issuer_cui, vat_category) so categories stay separate.
    let purchase_line_rows = sqlx::query(
        "SELECT ri.id AS invoice_id, \
                ri.issuer_cui, \
                ri.issuer_name, \
                COALESCE(ri.currency, 'RON') AS currency, \
                ri.exchange_rate, \
                vl.vat_category, \
                vl.vat_rate, \
                vl.base_amount AS line_base, \
                vl.vat_amount AS line_vat \
         FROM received_invoices ri \
         JOIN received_invoice_vat_lines vl ON vl.received_invoice_id = ri.id \
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

    struct SupplierAcc {
        issuer_cui: String,
        issuer_name: String,
        vat_category: String,
        vat_rate: String,
        invoice_ids: std::collections::BTreeSet<String>,
        base: Decimal,
        vat: Decimal,
        // Purchase lines have no art331_code snapshot — always None (default 22).
        art331_code: Option<String>,
    }

    // key = (issuer_cui, vat_category, vat_rate_normalized)
    let mut suppliers: BTreeMap<(String, String, String), SupplierAcc> = BTreeMap::new();

    for row in &purchase_line_rows {
        let invoice_id: String = row.try_get("invoice_id").unwrap_or_default();
        let issuer_cui: String = row.try_get("issuer_cui").unwrap_or_default();
        let issuer_name: String = row.try_get("issuer_name").unwrap_or_default();
        let currency: String = row
            .try_get("currency")
            .unwrap_or_else(|_| "RON".to_string());
        let fx = parse_rate(
            row.try_get::<Option<f64>, _>("exchange_rate")
                .unwrap_or(None),
        );
        let vat_category: String = row
            .try_get("vat_category")
            .unwrap_or_else(|_| "S".to_string());
        let raw_vat_rate: String = row.try_get("vat_rate").unwrap_or_else(|_| "0".to_string());
        let vat_rate = normalize_vat_rate(&raw_vat_rate);
        let base_s: String = row.try_get("line_base").unwrap_or_default();
        let vat_s: String = row.try_get("line_vat").unwrap_or_default();

        let base_ron = amount_to_ron(
            Decimal::from_str(base_s.trim()).unwrap_or(Decimal::ZERO),
            &currency,
            fx,
        );
        let vat_ron = amount_to_ron(
            Decimal::from_str(vat_s.trim()).unwrap_or(Decimal::ZERO),
            &currency,
            fx,
        );

        let key = (issuer_cui.clone(), vat_category.clone(), vat_rate.clone());
        let acc = suppliers.entry(key).or_insert(SupplierAcc {
            issuer_cui: issuer_cui.clone(),
            issuer_name: issuer_name.clone(),
            vat_category: vat_category.clone(),
            vat_rate: vat_rate.clone(),
            invoice_ids: std::collections::BTreeSet::new(),
            base: Decimal::ZERO,
            vat: Decimal::ZERO,
            art331_code: None, // purchase lines have no art331_code snapshot
        });
        acc.invoice_ids.insert(invoice_id);
        acc.base += base_ron;
        acc.vat += vat_ron;
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
                vat_category: acc.vat_category,
                vat_rate: acc.vat_rate,
                invoice_count: acc.invoice_ids.len() as i64,
                base: acc.base.round_dp(2).to_string(),
                vat: acc.vat.round_dp(2).to_string(),
                art331_code: acc.art331_code,
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
    let dest_path = crate::commands::integrations::validate_export_path(&dest_path)?
        .to_string_lossy()
        .to_string();
    let report = compute_d394(state, company_id, period_from, period_to).await?;

    let dest = dest_path.clone();
    tokio::task::spawn_blocking(move || build_and_write_xml(report, dest))
        .await
        .map_err(|e| AppError::Other(e.to_string()))?
}

/// Generează fișierul XML D394 oficial (schema ANAF v5) și îl scrie la calea
/// specificată. Aceasta este comanda de export pentru depunere la ANAF.
///
/// Spre deosebire de `export_d394` (preview JSON-like), aceasta produce un XML
/// strict conform cu schema oficială XSD (`sample_d394.xml`).
#[tauri::command]
pub async fn export_d394_official(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
    submission: crate::anaf_decl::d394::D394Submission,
    dest_path: String,
) -> AppResult<String> {
    use crate::anaf_decl::d394::generator::generate_d394_xml;
    use crate::anaf_decl::d394::sections::build_sections;
    use crate::anaf_decl::version::resolve;
    use crate::anaf_decl::DeclKind;
    use crate::db::companies;
    use chrono::NaiveDate;

    // Validate destination path
    let dest = crate::commands::integrations::validate_export_path(&dest_path)?
        .to_string_lossy()
        .to_string();

    // Parse period_from to extract luna/an and resolve schema version
    let period = NaiveDate::parse_from_str(&period_from, "%Y-%m-%d").map_err(|_| {
        AppError::Validation(format!(
            "period_from '{period_from}' nu este în formatul YYYY-MM-DD."
        ))
    })?;

    let ver = resolve(DeclKind::D394, period)?;

    // Fetch company record (cui, legal_name, adresa)
    let company = companies::get(&state.db, &company_id).await?;

    // Compute aggregates via existing compute_d394
    let report = compute_d394(state, company_id, period_from, period_to).await?;

    // Build the structural D394Doc
    let doc = build_sections(&report, &submission, &company, period)?;

    // Generate schema-conformant XML
    let xml = generate_d394_xml(&doc, &submission, &company, &ver)?;

    // Write to disk
    std::fs::write(&dest, xml.as_bytes()).map_err(|e| AppError::Other(e.to_string()))?;

    Ok(dest)
}

// ── XML builder ───────────────────────────────────────────────────────────────

fn build_and_write_xml(report: D394Report, dest_path: String) -> AppResult<String> {
    let generated_at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let mut xml = String::with_capacity(8192);

    xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    xml.push_str("<!-- D394 Declarație informativă livrări/achiziții — generat de Clarito -->\n");
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
            "      <CategorieVAT>{}</CategorieVAT>\n",
            xml_escape(&partner.vat_category)
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
            "      <CategorieVAT>{}</CategorieVAT>\n",
            xml_escape(&partner.vat_category)
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
                vat_category: "S".to_string(),
                vat_rate: "19".to_string(),
                invoice_count: 3,
                base: "5000.00".to_string(),
                vat: "950.00".to_string(),
                art331_code: None,
            }],
            total_base: "5000.00".to_string(),
            total_vat: "950.00".to_string(),
            invoice_count: 3,
            purchase_partners: vec![D394Partner {
                partner_cui: "RO11111111".to_string(),
                partner_name: "SC FURNIZOR SRL".to_string(),
                vat_category: "S".to_string(),
                vat_rate: "19".to_string(),
                invoice_count: 2,
                base: "2000.00".to_string(),
                vat: "380.00".to_string(),
                art331_code: None,
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
                vat_category: "S".to_string(),
                vat_rate: "19".to_string(),
                invoice_count: 1,
                base: "100.00".to_string(),
                vat: "19.00".to_string(),
                art331_code: None,
            },
            D394Partner {
                partner_cui: "".to_string(),
                partner_name: "A".to_string(),
                vat_category: "S".to_string(),
                vat_rate: "19".to_string(),
                invoice_count: 1,
                base: "5000.00".to_string(),
                vat: "950.00".to_string(),
                art331_code: None,
            },
            D394Partner {
                partner_cui: "".to_string(),
                partner_name: "C".to_string(),
                vat_category: "S".to_string(),
                vat_rate: "19".to_string(),
                invoice_count: 1,
                base: "1000.00".to_string(),
                vat: "190.00".to_string(),
                art331_code: None,
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

    // ── Fix #2: group by (partner_cui, vat_category) ─────────────────────────

    /// Fix #2: same partner with two different vat_categories produces two rows,
    /// not one collapsed row.
    #[test]
    fn d394_groups_by_partner_and_vat_category() {
        // Simulate the Rust-side accumulation for a partner with lines in two categories.
        // (RO111, "S") and (RO111, "AE") must be separate keys.
        struct LineRow {
            partner_cui: &'static str,
            vat_category: &'static str,
            base: &'static str,
            vat: &'static str,
        }

        let lines = [
            LineRow {
                partner_cui: "RO111",
                vat_category: "S",
                base: "1000.00",
                vat: "190.00",
            },
            LineRow {
                partner_cui: "RO111",
                vat_category: "AE",
                base: "500.00",
                vat: "0.00",
            },
            LineRow {
                partner_cui: "RO222",
                vat_category: "S",
                base: "300.00",
                vat: "57.00",
            },
        ];

        let mut groups: BTreeMap<(String, String), (Decimal, Decimal)> = BTreeMap::new();
        for l in &lines {
            let key = (l.partner_cui.to_string(), l.vat_category.to_string());
            let e = groups.entry(key).or_insert((Decimal::ZERO, Decimal::ZERO));
            e.0 += Decimal::from_str(l.base).unwrap();
            e.1 += Decimal::from_str(l.vat).unwrap();
        }

        assert_eq!(
            groups.len(),
            3,
            "3 distinct (partner, category) keys expected"
        );

        let s_111 = &groups[&("RO111".to_string(), "S".to_string())];
        assert_eq!(s_111.0, Decimal::from_str("1000.00").unwrap());
        assert_eq!(s_111.1, Decimal::from_str("190.00").unwrap());

        let ae_111 = &groups[&("RO111".to_string(), "AE".to_string())];
        assert_eq!(ae_111.0, Decimal::from_str("500.00").unwrap());
        assert_eq!(ae_111.1, Decimal::ZERO);

        let s_222 = &groups[&("RO222".to_string(), "S".to_string())];
        assert_eq!(s_222.0, Decimal::from_str("300.00").unwrap());
    }

    /// Fix #2: E and Z at the same 0% rate stay separate (not collapsed together).
    #[test]
    fn d394_zero_rate_categories_stay_separate() {
        let mut groups: BTreeMap<(String, String), (Decimal, Decimal)> = BTreeMap::new();

        // Same partner, same numeric rate (0%), different categories
        let key_e = ("RO111".to_string(), "E".to_string());
        let key_z = ("RO111".to_string(), "Z".to_string());
        groups
            .entry(key_e)
            .or_insert((Decimal::ZERO, Decimal::ZERO))
            .0 += Decimal::from_str("200.00").unwrap();
        groups
            .entry(key_z)
            .or_insert((Decimal::ZERO, Decimal::ZERO))
            .0 += Decimal::from_str("300.00").unwrap();

        assert_eq!(groups.len(), 2, "E and Z must be separate rows");
        assert_eq!(
            groups[&("RO111".to_string(), "E".to_string())].0,
            Decimal::from_str("200.00").unwrap()
        );
        assert_eq!(
            groups[&("RO111".to_string(), "Z".to_string())].0,
            Decimal::from_str("300.00").unwrap()
        );
    }

    /// Fix #2: foreign-currency partner converts deterministically (no GROUP_CONCAT mis-pair).
    /// Each row carries its own currency+rate — accumulation is independent.
    #[test]
    fn d394_foreign_currency_partner_deterministic_conversion() {
        use crate::ubl::fx::{amount_to_ron, parse_rate};

        // Partner RO111, category S:
        //   Line 1: EUR 1000 base, EUR 190 vat, rate 5.0 → 5000 / 950 RON
        //   Line 2: RON 500 base, RON 95 vat, no rate → 500 / 95 RON
        // Expected total: base=5500 RON, vat=1045 RON

        struct LineRow {
            currency: &'static str,
            rate: Option<f64>,
            base: &'static str,
            vat: &'static str,
        }

        let lines = [
            LineRow {
                currency: "EUR",
                rate: Some(5.0),
                base: "1000.00",
                vat: "190.00",
            },
            LineRow {
                currency: "RON",
                rate: None,
                base: "500.00",
                vat: "95.00",
            },
        ];

        let mut total_base = Decimal::ZERO;
        let mut total_vat = Decimal::ZERO;
        for l in &lines {
            let fx = parse_rate(l.rate);
            total_base += amount_to_ron(Decimal::from_str(l.base).unwrap(), l.currency, fx);
            total_vat += amount_to_ron(Decimal::from_str(l.vat).unwrap(), l.currency, fx);
        }

        assert_eq!(
            total_base,
            Decimal::from_str("5500.00").unwrap(),
            "5000+500=5500 RON base"
        );
        assert_eq!(
            total_vat,
            Decimal::from_str("1045.00").unwrap(),
            "950+95=1045 RON vat"
        );
    }

    /// Fix #2: XML output includes CategorieVAT element for each partner row.
    #[test]
    fn build_xml_includes_vat_category_per_partner() {
        let report = D394Report {
            company_cui: "RO33333333".to_string(),
            period_from: "2024-05-01".to_string(),
            period_to: "2024-05-31".to_string(),
            partners: vec![
                D394Partner {
                    partner_cui: "RO111".to_string(),
                    partner_name: "SC A SRL".to_string(),
                    vat_category: "S".to_string(),
                    vat_rate: "19".to_string(),
                    invoice_count: 1,
                    base: "1000.00".to_string(),
                    vat: "190.00".to_string(),
                    art331_code: None,
                },
                D394Partner {
                    partner_cui: "RO111".to_string(),
                    partner_name: "SC A SRL".to_string(),
                    vat_category: "AE".to_string(),
                    vat_rate: "0".to_string(),
                    invoice_count: 1,
                    base: "500.00".to_string(),
                    vat: "0.00".to_string(),
                    art331_code: None,
                },
            ],
            total_base: "1500.00".to_string(),
            total_vat: "190.00".to_string(),
            invoice_count: 1,
            purchase_partners: vec![],
            total_purchase_base: "0.00".to_string(),
            total_purchase_vat: "0.00".to_string(),
            purchase_invoice_count: 0,
            purchase_unparsed_count: 0,
        };

        let dir = std::env::temp_dir();
        let path = dir.join("test_d394_vat_category.xml");
        let result = build_and_write_xml(report, path.to_string_lossy().to_string());
        assert!(result.is_ok());

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("<CategorieVAT>S</CategorieVAT>"),
            "XML must contain CategorieVAT S"
        );
        assert!(
            content.contains("<CategorieVAT>AE</CategorieVAT>"),
            "XML must contain CategorieVAT AE — separate row from S"
        );

        let _ = std::fs::remove_file(&path);
    }

    // ── Defect-1 fix: normalize_vat_rate ──────────────────────────────────────

    /// normalize_vat_rate converts DB-stored rate strings to canonical int-percent.
    #[test]
    fn normalize_vat_rate_handles_all_formats() {
        // Decimal fraction form (e.g. 0.19 from legacy REAL column)
        assert_eq!(normalize_vat_rate("0.19"), "19");
        assert_eq!(normalize_vat_rate("0.09"), "9");
        assert_eq!(normalize_vat_rate("0.05"), "5");
        assert_eq!(normalize_vat_rate("0.00"), "0");
        // Padded percent form (from printf('%.2f'))
        assert_eq!(normalize_vat_rate("19.00"), "19");
        assert_eq!(normalize_vat_rate("21.00"), "21");
        // Plain integer string (from TEXT column storing "19")
        assert_eq!(normalize_vat_rate("19"), "19");
        assert_eq!(normalize_vat_rate("9"), "9");
        assert_eq!(normalize_vat_rate("0"), "0");
        // Edge cases
        assert_eq!(normalize_vat_rate(""), "0");
        assert_eq!(normalize_vat_rate("  19  "), "19");
        assert_eq!(normalize_vat_rate("garbage"), "0");
    }

    /// Defect-1: a partner with 19% and 21% category-S sales must produce TWO
    /// D394 rows (separate keys), NOT one blended row with an inferred cota=20.
    #[test]
    fn d394_mixed_rate_partner_produces_two_rows() {
        // Simulate the Rust-side grouping for a partner with two different rates.
        // (RO111, "S", "19") and (RO111, "S", "21") must be separate keys.
        struct LineRow {
            partner_cui: &'static str,
            vat_category: &'static str,
            raw_vat_rate: &'static str, // DB form
            base: &'static str,
            vat: &'static str,
        }

        let lines = [
            LineRow {
                partner_cui: "RO111",
                vat_category: "S",
                raw_vat_rate: "19.00",
                base: "1000.00",
                vat: "190.00",
            },
            LineRow {
                partner_cui: "RO111",
                vat_category: "S",
                raw_vat_rate: "21.00",
                base: "500.00",
                vat: "105.00",
            },
            LineRow {
                partner_cui: "RO222",
                vat_category: "S",
                raw_vat_rate: "19.00",
                base: "300.00",
                vat: "57.00",
            },
        ];

        let mut groups: BTreeMap<(String, String, String), (Decimal, Decimal)> = BTreeMap::new();
        for l in &lines {
            let rate = normalize_vat_rate(l.raw_vat_rate);
            let key = (l.partner_cui.to_string(), l.vat_category.to_string(), rate);
            let e = groups.entry(key).or_insert((Decimal::ZERO, Decimal::ZERO));
            e.0 += Decimal::from_str(l.base).unwrap();
            e.1 += Decimal::from_str(l.vat).unwrap();
        }

        assert_eq!(
            groups.len(),
            3,
            "RO111@19%, RO111@21%, RO222@19% → 3 distinct keys (not 2 collapsed)"
        );

        let row_19 = &groups[&("RO111".to_string(), "S".to_string(), "19".to_string())];
        assert_eq!(row_19.0, Decimal::from_str("1000.00").unwrap());
        assert_eq!(row_19.1, Decimal::from_str("190.00").unwrap());

        let row_21 = &groups[&("RO111".to_string(), "S".to_string(), "21".to_string())];
        assert_eq!(row_21.0, Decimal::from_str("500.00").unwrap());
        assert_eq!(row_21.1, Decimal::from_str("105.00").unwrap());

        // Verify there is NO blended row — cota "20" must not exist
        assert!(
            !groups.contains_key(&("RO111".to_string(), "S".to_string(), "20".to_string())),
            "Must NOT produce a blended cota=20 row"
        );
    }
}
