//! Pre-export validation — checks that surface common DUKIntegrator-fatal issues
//! as friendly Romanian messages BEFORE the user exports a declaration.
//!
//! All checks are pure Rust (no Java / DUKIntegrator invocation).
//! Returns a `Vec<PreflightIssue>` that the frontend renders above the export
//! buttons via `PreflightPanel`.

use serde::Serialize;
use sqlx::Row;

use crate::anaf_decl::{valid_cui, DeclKind};
use crate::error::AppResult;

// ─── Types ─────────────────────────────────────────────────────────────────

/// A single pre-export validation finding.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightIssue {
    /// `"error"` — blocks correct submission; `"warning"` — advisory.
    pub severity: String,
    /// Machine-readable code (used for i18n / deduplication on the frontend).
    pub code: String,
    /// Short user-facing message in Romanian.
    pub message: String,
    /// Actionable hint in Romanian (may be empty).
    pub hint: String,
}

impl PreflightIssue {
    fn error(code: &str, message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            severity: "error".to_string(),
            code: code.to_string(),
            message: message.into(),
            hint: hint.into(),
        }
    }

    fn warning(code: &str, message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            severity: "warning".to_string(),
            code: code.to_string(),
            message: message.into(),
            hint: hint.into(),
        }
    }
}

// ─── Main preflight function ───────────────────────────────────────────────

/// Run all pre-export validation checks for a declaration.
///
/// Returns the list of findings (errors + warnings) in discovery order.
/// An empty list means "no issues found — safe to export".
pub async fn preflight(
    pool: &sqlx::SqlitePool,
    company_id: &str,
    kind: DeclKind,
    period_from: &str,
    period_to: &str,
) -> AppResult<Vec<PreflightIssue>> {
    let mut issues: Vec<PreflightIssue> = Vec::new();

    // ── Check 1: Company identity ────────────────────────────────────────────

    let company_row = sqlx::query(
        "SELECT cui, legal_name, address, vat_payer \
         FROM companies WHERE id = ?1 LIMIT 1",
    )
    .bind(company_id)
    .fetch_optional(pool)
    .await?;

    let company = match company_row {
        None => {
            issues.push(PreflightIssue::error(
                "FIRMA_LIPSA",
                "Compania selectată nu a fost găsită în baza de date.",
                "Selectați o companie validă înainte de export.",
            ));
            // Cannot continue without a company row — return early.
            return Ok(issues);
        }
        Some(row) => row,
    };

    let cui: String = company.try_get("cui").unwrap_or_default();
    let legal_name: String = company.try_get("legal_name").unwrap_or_default();
    let address: String = company.try_get("address").unwrap_or_default();
    let vat_payer: i64 = company.try_get("vat_payer").unwrap_or(1);

    if !valid_cui(&cui) {
        issues.push(PreflightIssue::error(
            "CUI_FIRMA",
            format!(
                "CUI-ul firmei «{}» nu este valid (eșuează verificarea mod-11).",
                cui
            ),
            "Verificați CUI-ul firmei în Companii.",
        ));
    }

    if legal_name.trim().is_empty() || address.trim().is_empty() {
        issues.push(PreflightIssue::error(
            "DATE_FIRMA",
            "Denumirea sau adresa firmei este goală.",
            "Completați denumirea și adresa în Companii.",
        ));
    }

    // ── Check 2: VAT payer status ────────────────────────────────────────────

    // D300 and D394 are filed by VAT payers only.
    // D301, D700, D710 are accepted for non-payers and payers alike — no VAT-payer gate.
    if matches!(kind, DeclKind::D300 | DeclKind::D394) && vat_payer == 0 {
        issues.push(PreflightIssue::warning(
            "NEPLATITOR_TVA",
            "Firma nu este marcată ca plătitoare de TVA.",
            "D300/D394 se depun de plătitorii de TVA. Verificați setarea în Companii.",
        ));
    }

    // ── Check 3: Sales partner CUIs (Romanian contacts with bad CUI) ─────────

    // Check each Romanian contact's CUI individually (mod-11 via valid_cui).
    let bad_client_cui_rows = sqlx::query(
        "SELECT DISTINCT c.cui \
         FROM contacts c \
         JOIN invoices i ON i.contact_id = c.id \
         WHERE i.company_id = ?1 \
           AND i.status IN ('VALIDATED','STORNED') \
           AND i.issue_date >= ?2 \
           AND i.issue_date <= ?3 \
           AND c.country = 'RO' \
           AND c.cui IS NOT NULL \
           AND TRIM(c.cui) != ''",
    )
    .bind(company_id)
    .bind(period_from)
    .bind(period_to)
    .fetch_all(pool)
    .await?;

    let bad_client_count = bad_client_cui_rows
        .iter()
        .filter(|row| {
            let cui_val: String = row.try_get("cui").unwrap_or_default();
            !valid_cui(&cui_val)
        })
        .count();

    if bad_client_count > 0 {
        issues.push(PreflightIssue::warning(
            "CUI_CLIENTI",
            format!(
                "{} client(i) români din perioada selectată au CUI invalid.",
                bad_client_count
            ),
            "Corectați CUI-ul clienților în Contacte.",
        ));
    }

    // ── Check 4: Purchase supplier CUIs ─────────────────────────────────────

    // Only flag CUIs that "look Romanian" (2–10 digits after stripping RO prefix).
    // This avoids false-flagging foreign suppliers with non-Romanian ID formats.
    let supplier_cui_rows = sqlx::query(
        "SELECT DISTINCT issuer_cui \
         FROM received_invoices \
         WHERE company_id = ?1 \
           AND issue_date >= ?2 \
           AND issue_date <= ?3 \
           AND status != 'REJECTED' \
           AND issuer_cui IS NOT NULL \
           AND TRIM(issuer_cui) != ''",
    )
    .bind(company_id)
    .bind(period_from)
    .bind(period_to)
    .fetch_all(pool)
    .await?;

    let bad_supplier_count = supplier_cui_rows
        .iter()
        .filter(|row| {
            let raw: String = row.try_get("issuer_cui").unwrap_or_default();
            // Determine if this looks Romanian: strip optional "RO"/spaces and check
            // it is all digits with length 2–10.
            if looks_romanian(&raw) {
                !valid_cui(&raw)
            } else {
                false // skip non-Romanian identifiers (foreign suppliers)
            }
        })
        .count();

    if bad_supplier_count > 0 {
        issues.push(PreflightIssue::warning(
            "CUI_FURNIZORI",
            format!(
                "{} furnizor(i) români din perioada selectată au CUI invalid.",
                bad_supplier_count
            ),
            "Verificați CUI-ul furnizorilor în facturile primite.",
        ));
    }

    // ── Check 5: Unparsed received VAT (D300/D394 only) ──────────────────────

    if matches!(kind, DeclKind::D300 | DeclKind::D394) {
        let unparsed_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) \
             FROM received_invoices \
             WHERE company_id = ?1 \
               AND issue_date >= ?2 \
               AND issue_date <= ?3 \
               AND status != 'REJECTED' \
               AND net_amount IS NULL",
        )
        .bind(company_id)
        .bind(period_from)
        .bind(period_to)
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        if unparsed_count > 0 {
            issues.push(PreflightIssue::warning(
                "TVA_NEPARSAT",
                format!(
                    "{} factură/facturi primite nu au defalcare TVA parsată (net_amount IS NULL).",
                    unparsed_count
                ),
                "Folosiți «Recalculează TVA din XML» în Facturi primite.",
            ));
        }
    }

    // ── Check 5b: operations at old VAT rates → auto-included in regularizări ──
    // Sales at old 19%/5% and purchases at 19%/9%/5% (category S) are auto-included
    // in the regularizări rows R16/R30 by the generator (Wave 8). The accountant must
    // verify the auto-computed amounts before submitting the declaration.
    if matches!(kind, DeclKind::D300) {
        let bad_sales: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT i.id) FROM invoice_line_items l \
             JOIN invoices i ON i.id = l.invoice_id \
             WHERE i.company_id = ?1 AND i.status IN ('VALIDATED','STORNED') \
               AND i.issue_date >= ?2 AND i.issue_date <= ?3 \
               AND l.vat_category = 'S' AND CAST(l.vat_rate AS REAL) IN (19.0, 5.0)",
        )
        .bind(company_id)
        .bind(period_from)
        .bind(period_to)
        .fetch_one(pool)
        .await
        .unwrap_or(0);
        let bad_purch: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT ri.id) FROM received_invoice_vat_lines vl \
             JOIN received_invoices ri ON ri.id = vl.received_invoice_id \
             WHERE ri.company_id = ?1 AND ri.status != 'REJECTED' \
               AND ri.issue_date >= ?2 AND ri.issue_date <= ?3 \
               AND vl.vat_category = 'S' AND CAST(vl.vat_rate AS REAL) IN (19.0, 9.0, 5.0)",
        )
        .bind(company_id)
        .bind(period_from)
        .bind(period_to)
        .fetch_one(pool)
        .await
        .unwrap_or(0);
        if bad_sales > 0 || bad_purch > 0 {
            issues.push(PreflightIssue::warning(
                "D300_COTE_VECHI",
                format!(
                    "{bad_sales} vânzări (cote 19%/5%) și {bad_purch} achiziții (cote 19%/9%/5%) \
                     au fost incluse automat în regularizări (rd. 16 / rd. 32-33)."
                ),
                "Verificați sumele din secțiunea «Regularizări cote vechi» înainte de a depune \
                 decontul. Puteți corecta manual valorile R16/R30 dacă este necesar.",
            ));
        }
    }

    // ── Check 6: Empty period ────────────────────────────────────────────────

    let sales_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) \
         FROM invoices \
         WHERE company_id = ?1 \
           AND status IN ('VALIDATED','STORNED') \
           AND issue_date >= ?2 \
           AND issue_date <= ?3",
    )
    .bind(company_id)
    .bind(period_from)
    .bind(period_to)
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    let received_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) \
         FROM received_invoices \
         WHERE company_id = ?1 \
           AND status != 'REJECTED' \
           AND issue_date >= ?2 \
           AND issue_date <= ?3",
    )
    .bind(company_id)
    .bind(period_from)
    .bind(period_to)
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    if sales_count == 0 && received_count == 0 {
        issues.push(PreflightIssue::warning(
            "FARA_OPERATIUNI",
            "Nu există operațiuni în perioada selectată (nicio factură emisă sau primită).",
            "Verificați că perioada selectată este corectă.",
        ));
    }

    Ok(issues)
}

// ─── Helper: "looks Romanian" CUI detection ────────────────────────────────

/// Returns `true` if `raw` appears to be a Romanian CUI (optionally prefixed
/// with "RO" and consisting of 2–10 digits). Used to skip foreign-supplier IDs
/// (e.g. German Steuernummer, Polish NIP, etc.) that would produce false positives.
fn looks_romanian(raw: &str) -> bool {
    let s = raw.trim();
    if s.is_empty() {
        return false;
    }
    // Strip optional "RO" prefix (case-insensitive) and surrounding whitespace.
    let s = if s.len() >= 2 && s[..2].eq_ignore_ascii_case("ro") {
        s[2..].trim()
    } else {
        s
    };
    if s.is_empty() {
        return false;
    }
    let n = s.len();
    // Romanian CUI body: 2–10 digits, not starting with '0'.
    (2..=10).contains(&n) && s.chars().all(|c| c.is_ascii_digit()) && !s.starts_with('0')
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Pool helper ──────────────────────────────────────────────────────────

    async fn setup_pool() -> sqlx::SqlitePool {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations");
        pool
    }

    // ── Company helpers ──────────────────────────────────────────────────────

    /// Insert a company with the given id and CUI.
    async fn insert_company(pool: &sqlx::SqlitePool, id: &str, cui: &str) {
        sqlx::query(
            "INSERT INTO companies \
             (id, cui, legal_name, address, city, county, country, vat_payer) \
             VALUES (?1, ?2, 'Test SRL', 'Str. Exemplu 1', 'Bucuresti', 'B', 'RO', 1)",
        )
        .bind(id)
        .bind(cui)
        .execute(pool)
        .await
        .expect("insert company");
    }

    /// Insert a received invoice. If `net_amount` is `None`, it is stored as NULL.
    async fn insert_received(
        pool: &sqlx::SqlitePool,
        company_id: &str,
        recv_id: &str,
        issuer_cui: &str,
        net_amount: Option<&str>,
        status: &str,
        issue_date: &str,
    ) {
        let dl_id = format!("dl-{recv_id}");
        sqlx::query(
            "INSERT INTO received_invoices \
             (id, company_id, anaf_download_id, issuer_cui, issuer_name, \
              total_amount, net_amount, vat_amount, currency, issue_date, \
              xml_path, status, downloaded_at, created_at) \
             VALUES (?1, ?2, ?3, ?4, 'Furnizor SRL', '1190.00', ?5, '190.00', \
                     'RON', ?6, 'x.xml', ?7, 1, 1)",
        )
        .bind(recv_id)
        .bind(company_id)
        .bind(&dl_id)
        .bind(issuer_cui)
        .bind(net_amount)
        .bind(issue_date)
        .bind(status)
        .execute(pool)
        .await
        .expect("insert received invoice");
    }

    // ── Test (a): company with INVALID CUI → CUI_FIRMA error ─────────────────

    #[tokio::test]
    async fn test_invalid_company_cui_produces_error() {
        let pool = setup_pool().await;
        // "11111111" fails mod-11 (verified: valid_cui returns false).
        insert_company(&pool, "co_bad", "11111111").await;

        let issues = preflight(&pool, "co_bad", DeclKind::D300, "2025-01-01", "2025-01-31")
            .await
            .expect("preflight");

        let cui_errors: Vec<_> = issues.iter().filter(|i| i.code == "CUI_FIRMA").collect();
        assert!(
            !cui_errors.is_empty(),
            "Expected CUI_FIRMA error for invalid CUI 11111111, got: {issues:?}"
        );
        assert_eq!(
            cui_errors[0].severity, "error",
            "CUI_FIRMA must be severity=error"
        );
    }

    // ── Test (b): valid company + empty period → FARA_OPERATIUNI warning ──────

    #[tokio::test]
    async fn test_empty_period_produces_warning() {
        let pool = setup_pool().await;
        // "12345674" is a valid CUI (passes mod-11, confirmed in cui_tests).
        insert_company(&pool, "co_valid", "12345674").await;

        let issues = preflight(
            &pool,
            "co_valid",
            DeclKind::D300,
            "2025-01-01",
            "2025-01-31",
        )
        .await
        .expect("preflight");

        let fara: Vec<_> = issues
            .iter()
            .filter(|i| i.code == "FARA_OPERATIUNI")
            .collect();
        assert!(
            !fara.is_empty(),
            "Expected FARA_OPERATIUNI warning for empty period, got: {issues:?}"
        );
        assert_eq!(fara[0].severity, "warning");
    }

    // ── Test (c): valid company + one unparsed received invoice → TVA_NEPARSAT

    #[tokio::test]
    async fn test_unparsed_received_produces_tva_neparsat_warning() {
        let pool = setup_pool().await;
        // "12345674" is a valid CUI.
        insert_company(&pool, "co_unp", "12345674").await;

        // Insert a received invoice with net_amount IS NULL (unparsed).
        insert_received(
            &pool,
            "co_unp",
            "ri_unp_1",
            "12345674", // valid supplier CUI — avoids CUI_FURNIZORI noise
            None,       // net_amount IS NULL → unparsed
            "NEW",
            "2025-01-15",
        )
        .await;

        let issues = preflight(&pool, "co_unp", DeclKind::D300, "2025-01-01", "2025-01-31")
            .await
            .expect("preflight");

        let tva: Vec<_> = issues.iter().filter(|i| i.code == "TVA_NEPARSAT").collect();
        assert!(
            !tva.is_empty(),
            "Expected TVA_NEPARSAT warning for unparsed received invoice, got: {issues:?}"
        );
        assert_eq!(tva[0].severity, "warning");

        // FARA_OPERATIUNI must NOT fire because there IS a received invoice.
        let fara: Vec<_> = issues
            .iter()
            .filter(|i| i.code == "FARA_OPERATIUNI")
            .collect();
        assert!(
            fara.is_empty(),
            "FARA_OPERATIUNI should not fire when there are received invoices"
        );
    }

    // ── Helper: looks_romanian ───────────────────────────────────────────────

    #[test]
    fn test_looks_romanian() {
        assert!(looks_romanian("12345674"), "plain digits");
        assert!(looks_romanian("RO12345674"), "RO prefix");
        assert!(looks_romanian("ro12345674"), "ro lowercase");
        assert!(!looks_romanian("DE123456789"), "German ID");
        assert!(!looks_romanian(""), "empty");
        assert!(!looks_romanian("01234567"), "leading zero");
        assert!(!looks_romanian("1"), "too short");
    }
}
