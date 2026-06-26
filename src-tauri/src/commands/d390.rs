//! D390 declarația recapitulativă (VIES) — aggregation + XML export commands.
//!
//! Aggregates the period's intra-EU operations into `<operatie>` rows:
//!   outbound sales lines vat_category='K' → L (goods) / P (services, by revenue_kind),
//!   inbound received lines vat_category='K' → A (goods) / S (services, by intra_eu_kind).
//! Partner VAT id is split into country (2-letter prefix) + codO; RO / non-EU partners are
//! excluded (those belong to D394, not D390). Bases are summed per (tip, country, codO) in RON.

use std::collections::{BTreeMap, HashSet};
use std::str::FromStr;

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use sqlx::Row;
use tauri::State;

use crate::anaf_decl::d390::{generator, D390Doc, D390Op, D390Submission};
use crate::db::companies;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::ubl::fx::{amount_to_ron, parse_rate};

/// EU member-state VAT prefixes (EL = Greece). RO is intentionally excluded below (domestic).
const EU_VAT_PREFIXES: &[&str] = &[
    "AT", "BE", "BG", "HR", "CY", "CZ", "DK", "EE", "FI", "FR", "DE", "EL", "HU", "IE", "IT", "LV",
    "LT", "LU", "MT", "NL", "PL", "PT", "RO", "SK", "SI", "ES", "SE",
];

/// Split an intra-EU partner VAT id into (country, codO). Returns None for RO (domestic),
/// non-EU / unrecognised prefixes (e.g. GB post-Brexit), or unprefixed/blank ids — none of
/// which are declarable on D390.
fn split_vat(vat: &str) -> Option<(String, String)> {
    let v = vat.trim().to_uppercase();
    let bytes = v.as_bytes();
    // Guard on BYTES (split_at is byte-indexed) AND require an ASCII 2-letter prefix so the
    // slice never lands mid-codepoint on dirty (multi-byte) input.
    if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() || !bytes[1].is_ascii_alphabetic() {
        return None;
    }
    let (prefix, rest) = v.split_at(2);
    // codO is alphanumeric — drops embedded spaces, punctuation and control chars.
    let cod_o: String = rest.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if prefix == "RO" || !EU_VAT_PREFIXES.contains(&prefix) || cod_o.is_empty() {
        return None;
    }
    Some((prefix.to_string(), cod_o))
}

/// Aggregate the period's intra-EU operations into a D390 document.
pub(crate) async fn compute_d390_doc(
    pool: &sqlx::SqlitePool,
    company_id: &str,
    period_from: &str,
    period_to: &str,
) -> AppResult<D390Doc> {
    // key = (tip, tara, codO) → (denO, base_ron)
    let mut agg: BTreeMap<(String, String, String), (String, Decimal)> = BTreeMap::new();
    // Count DISTINCT invoices skipped (all of an invoice's K-lines share one partner), not lines.
    let mut dropped_invoices: HashSet<String> = HashSet::new();

    let mut add = |tip: &str, vat: &str, name: &str, base: Decimal| -> bool {
        match split_vat(vat) {
            Some((tara, cod_o)) => {
                let e = agg
                    .entry((tip.to_string(), tara, cod_o))
                    .or_insert((name.to_string(), Decimal::ZERO));
                if e.0.trim().is_empty() {
                    e.0 = name.to_string();
                }
                e.1 += base;
                true
            }
            None => false,
        }
    };

    // ── Inbound: received K lines → A (goods) / S (services) ──────────────────
    let recv = sqlx::query(
        "SELECT ri.id AS inv_id, ri.issuer_cui, ri.issuer_name, COALESCE(ri.intra_eu_kind,'goods') AS kind, \
                vl.base_amount, COALESCE(ri.currency,'RON') AS currency, ri.exchange_rate \
         FROM received_invoice_vat_lines vl \
         JOIN received_invoices ri ON ri.id = vl.received_invoice_id \
         WHERE ri.company_id = ?1 AND ri.issue_date >= ?2 AND ri.issue_date <= ?3 \
           AND ri.status != 'REJECTED' AND vl.vat_category = 'K'",
    )
    .bind(company_id)
    .bind(period_from)
    .bind(period_to)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;
    for r in &recv {
        let cui: String = r.try_get("issuer_cui").unwrap_or_default();
        let name: String = r.try_get("issuer_name").unwrap_or_default();
        let kind: String = r.try_get("kind").unwrap_or_else(|_| "goods".into());
        let base_s: String = r.try_get("base_amount").unwrap_or_default();
        let currency: String = r.try_get("currency").unwrap_or_else(|_| "RON".into());
        let fx = parse_rate(r.try_get::<Option<f64>, _>("exchange_rate").unwrap_or(None));
        let base = amount_to_ron(
            Decimal::from_str(&base_s).unwrap_or(Decimal::ZERO),
            &currency,
            fx,
        );
        let tip = if kind.trim() == "services" { "S" } else { "A" };
        if !add(tip, &cui, &name, base) {
            let inv_id: String = r.try_get("inv_id").unwrap_or_default();
            dropped_invoices.insert(inv_id);
        }
    }

    // ── Outbound: sales K lines → L (goods) / P (services) ────────────────────
    // Setul fiscal = status IN ('VALIDATED','STORNED'), inclusiv notele de credit (liniile lor
    // negative netează baza) — IDENTIC cu D300/D394/jurnale, altfel D390 nu se reconciliază cu
    // D300 când există stornări.
    let sales = sqlx::query(
        "SELECT i.id AS inv_id, c.cui AS partner_cui, c.legal_name AS partner_name, \
                COALESCE(l.revenue_kind,'goods') AS kind, l.subtotal_amount, \
                COALESCE(i.currency,'RON') AS currency, i.exchange_rate \
         FROM invoice_line_items l \
         JOIN invoices i ON i.id = l.invoice_id \
         LEFT JOIN contacts c ON c.id = i.contact_id \
         WHERE i.company_id = ?1 AND i.status IN ('VALIDATED','STORNED') \
           AND i.issue_date >= ?2 AND i.issue_date <= ?3 AND l.vat_category = 'K'",
    )
    .bind(company_id)
    .bind(period_from)
    .bind(period_to)
    .fetch_all(pool)
    .await
    .map_err(AppError::Database)?;
    for r in &sales {
        let cui: Option<String> = r.try_get("partner_cui").unwrap_or(None);
        let name: String = r.try_get("partner_name").unwrap_or_default();
        let kind: String = r.try_get("kind").unwrap_or_else(|_| "goods".into());
        let base_s: String = r.try_get("subtotal_amount").unwrap_or_default();
        let currency: String = r.try_get("currency").unwrap_or_else(|_| "RON".into());
        let fx = parse_rate(r.try_get::<Option<f64>, _>("exchange_rate").unwrap_or(None));
        let base = amount_to_ron(
            Decimal::from_str(&base_s).unwrap_or(Decimal::ZERO),
            &currency,
            fx,
        );
        let tip = if kind.trim() == "service" { "P" } else { "L" };
        let added = cui
            .as_deref()
            .map(|c| add(tip, c, &name, base))
            .unwrap_or(false);
        if !added {
            let inv_id: String = r.try_get("inv_id").unwrap_or_default();
            dropped_invoices.insert(inv_id);
        }
    }

    // Build ops (baza în lei întregi, rotunjire comercială), ordered. O bază NETĂ NEGATIVĂ pe
    // partener (stornare peste altă perioadă) nu poate fi reprezentată pe rândurile L/T/A/P/S —
    // ar aparține regularizărilor (R, neimplementat) — deci se exclude cu avertisment și se
    // numără la `dropped`, ca utilizatorul să o declare manual. Bazele netate la 0 dispar firesc.
    let mut negative_partners = 0i64;
    let operations: Vec<D390Op> = agg
        .into_iter()
        .map(|((tip, tara, cod_o), (den_o, base))| D390Op {
            tip,
            tara,
            cod_o,
            den_o,
            baza: base
                .round_dp_with_strategy(0, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
                .to_i64()
                .unwrap_or(0),
        })
        .filter(|o| {
            if o.baza < 0 {
                tracing::warn!(
                    tip = %o.tip, partener = %o.den_o, baza = o.baza,
                    "D390: bază netă negativă (stornare peste perioadă) — exclusă; declarați \
                     regularizarea (R) manual"
                );
                negative_partners += 1;
                return false;
            }
            o.baza != 0
        })
        .collect();

    let an: i32 = period_from
        .get(0..4)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let luna: u32 = period_from
        .get(5..7)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    if an < 2020 || !(1..=12).contains(&luna) {
        return Err(AppError::Validation(format!(
            "Perioadă D390 invalidă: '{period_from}' (aștept YYYY-MM-DD)"
        )));
    }
    Ok(D390Doc {
        luna,
        an,
        operations,
        dropped: dropped_invoices.len() as i64 + negative_partners,
    })
}

/// Compute the D390 document (preview) for a period.
#[tauri::command]
pub async fn compute_d390(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
) -> AppResult<D390Doc> {
    compute_d390_doc(&state.db, &company_id, &period_from, &period_to).await
}

/// Generate the D390 XML and write it to `dest_path`. Returns the saved path.
#[tauri::command]
pub async fn export_d390(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
    dest_path: String,
    submission: Option<D390Submission>,
) -> AppResult<String> {
    let dest = crate::commands::integrations::validate_export_path(&dest_path)?
        .to_string_lossy()
        .to_string();

    let company = companies::get(&state.db, &company_id).await?;
    let doc = compute_d390_doc(&state.db, &company_id, &period_from, &period_to).await?;
    if doc.operations.is_empty() {
        return Err(AppError::Validation(
            "Nu există operațiuni intra-UE de raportat în perioada selectată.".into(),
        ));
    }
    let submission = submission.unwrap_or_default();
    let xml = generator::generate_d390_xml(&doc, &submission, &company)?;

    let path = dest.clone();
    tokio::task::spawn_blocking(move || std::fs::write(&dest, xml).map(|_| dest))
        .await
        .map_err(|e| AppError::Other(format!("join: {e}")))?
        .map_err(|e| AppError::Other(format!("write D390: {e}")))?;
    // Înregistrează depunerea în istoric (best-effort — erorile sunt înghițite).
    // period_from are forma YYYY-MM-DD; primele 7 caractere = YYYY-MM.
    let d390_period = period_from.get(0..7).unwrap_or(&period_from).to_string();
    let d_rec = submission.d_rec;
    crate::db::declaration_filings::record_or_warn(
        &state.db,
        crate::db::declaration_filings::FilingInput {
            company_id: company_id.clone(),
            kind: "D390".into(),
            period: d390_period,
            is_rectificative: d_rec,
            file_path: Some(path.clone()),
        },
    )
    .await;
    Ok(path)
}

/// Construiește XML-ul D390 fără a-l scrie pe disc — pentru previzualizarea/editarea în
/// vizualizatorul XML din aplicație. Folosește EXACT aceeași sursă ca `export_d390`
/// (`compute_d390_doc` + `generate_d390_xml`), doar că întoarce șirul în loc să-l salveze.
/// D390 nu are validator DUW dedicat, deci vizualizatorul se deschide doar pentru citire.
#[tauri::command]
pub async fn preview_d390_xml(
    state: State<'_, AppState>,
    company_id: String,
    period_from: String,
    period_to: String,
    submission: Option<D390Submission>,
) -> AppResult<String> {
    let company = companies::get(&state.db, &company_id).await?;
    let doc = compute_d390_doc(&state.db, &company_id, &period_from, &period_to).await?;
    if doc.operations.is_empty() {
        return Err(AppError::Validation(
            "Nu există operațiuni intra-UE de raportat în perioada selectată.".into(),
        ));
    }
    let submission = submission.unwrap_or_default();
    generator::generate_d390_xml(&doc, &submission, &company)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool() -> sqlx::SqlitePool {
        let p = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&p).await.unwrap();
        p
    }

    #[test]
    fn split_vat_handles_eu_and_ro() {
        assert_eq!(
            split_vat("DE123456789"),
            Some(("DE".into(), "123456789".into()))
        );
        assert_eq!(split_vat("RO12345678"), None, "RO is domestic, not D390");
        assert_eq!(split_vat("12345678"), None, "no country prefix");
        assert_eq!(split_vat(""), None);
        assert_eq!(split_vat("GB123456789"), None, "GB non-EU post-Brexit");
        assert_eq!(split_vat("中12"), None, "multi-byte prefix must not panic");
        assert_eq!(
            split_vat("DE 123 456"),
            Some(("DE".into(), "123456".into())),
            "inner whitespace stripped from codO"
        );
    }

    #[tokio::test]
    async fn credit_note_included_negative_net_dropped_and_missing_vat_dropped() {
        let pool = pool().await;
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co','12345678','Test SRL','S','B','B','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();
        // EU customer but with NO cui on file → outbound K sale is DROPPED (under-reporting flag).
        sqlx::query(
            "INSERT INTO contacts (id, company_id, contact_type, cui, legal_name) \
             VALUES ('ct','co','CUSTOMER',NULL,'Kunde GmbH')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO invoices (id, company_id, contact_id, series, number, full_number, \
             issue_date, due_date, currency, subtotal_amount, vat_amount, total_amount, status, \
             payment_means_code, created_at, updated_at) \
             VALUES ('inv','co','ct','inv',1,'inv','2026-03-10','2026-04-10','RON','1000','0','1000','VALIDATED','42',1,1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO invoice_line_items (id, invoice_id, position, name, quantity, unit, \
             unit_price, vat_rate, vat_category, subtotal_amount, vat_amount, total_amount) \
             VALUES ('l','inv','1','M','1','buc','1000','0','K','1000','0','1000')",
        )
        .execute(&pool)
        .await
        .unwrap();
        // A storno credit note (storno_of_invoice_id set) to a valid DE customer — INCLUDED in the
        // fiscal set (D300 parity); its partner nets −500 with no original in-period → the negative
        // net cannot sit on L/P rows → dropped with a warning (R-row regularization is manual).
        sqlx::query(
            "INSERT INTO contacts (id, company_id, contact_type, cui, legal_name) \
             VALUES ('ct2','co','CUSTOMER','DE999','Kunde2')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO invoices (id, company_id, contact_id, series, number, full_number, \
             issue_date, due_date, currency, subtotal_amount, vat_amount, total_amount, status, \
             payment_means_code, storno_of_invoice_id, created_at, updated_at) \
             VALUES ('st','co','ct2','st',1,'st','2026-03-12','2026-04-12','RON','-500','0','-500','VALIDATED','42','inv',1,1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO invoice_line_items (id, invoice_id, position, name, quantity, unit, \
             unit_price, vat_rate, vat_category, subtotal_amount, vat_amount, total_amount) \
             VALUES ('sl','st','1','M','-1','buc','500','0','K','-500','0','-500')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let doc = compute_d390_doc(&pool, "co", "2026-03-01", "2026-03-31")
            .await
            .unwrap();
        assert!(
            doc.operations.is_empty(),
            "no-cui sale dropped + negative-net partner dropped → no operations emitted"
        );
        assert_eq!(
            doc.dropped, 2,
            "the no-cui EU sale + the negative-net partner are both flagged as dropped"
        );
    }

    #[tokio::test]
    async fn aggregates_inbound_and_outbound_by_partner_and_type() {
        let pool = pool().await;
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co','12345678','Test SRL','S','B','B','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Outbound: EU customer DE, a goods sale (K) → L.
        sqlx::query(
            "INSERT INTO contacts (id, company_id, contact_type, cui, legal_name) \
             VALUES ('ct','co','CUSTOMER','DE999888777','Kunde GmbH')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO invoices (id, company_id, contact_id, series, number, full_number, \
             issue_date, due_date, currency, exchange_rate, subtotal_amount, vat_amount, \
             total_amount, status, payment_means_code, created_at, updated_at) \
             VALUES ('inv','co','ct','inv',1,'inv','2026-03-10','2026-04-10','EUR',5.0,'2000','0','2000','VALIDATED','42',1,1)",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO invoice_line_items (id, invoice_id, position, name, quantity, unit, \
             unit_price, vat_rate, vat_category, subtotal_amount, vat_amount, total_amount, revenue_kind) \
             VALUES ('l','inv','1','Marfă','1','buc','2000','0','K','2000','0','2000','goods')",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Inbound: FR supplier services acquisition (K) → S.
        sqlx::query(
            "INSERT INTO received_invoices (id, company_id, anaf_download_id, issuer_cui, \
             issuer_name, total_amount, net_amount, vat_amount, currency, exchange_rate, \
             issue_date, xml_path, status, intra_eu_kind) \
             VALUES ('ri','co','dl','FR55512345','Fournisseur','1000','1000','0','EUR',5.0,'2026-03-15','/x.xml','REVIEWED','services')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO received_invoice_vat_lines (id, received_invoice_id, vat_rate, vat_category, base_amount, vat_amount) \
             VALUES ('vl','ri','0','K','1000','0')",
        )
        .execute(&pool)
        .await
        .unwrap();

        let doc = compute_d390_doc(&pool, "co", "2026-03-01", "2026-03-31")
            .await
            .unwrap();
        assert_eq!(doc.luna, 3);
        assert_eq!(doc.an, 2026);
        assert_eq!(doc.operations.len(), 2);
        let l = doc.operations.iter().find(|o| o.tip == "L").unwrap();
        assert_eq!(l.tara, "DE");
        assert_eq!(l.cod_o, "999888777");
        assert_eq!(l.baza, 10000, "2000 EUR × 5.0 = 10000 RON");
        let s = doc.operations.iter().find(|o| o.tip == "S").unwrap();
        assert_eq!(s.tara, "FR");
        assert_eq!(s.baza, 5000, "1000 EUR × 5.0 = 5000 RON");
    }
}
