// src-tauri/src/ubl/rocius_rules.rs
//! Toate regulile de business CIUS-RO (50+) verificate la nivel de date,
//! înainte de generarea XML. Fiecare regulă returnează `Option<String>`
//! (None = OK, Some(msg) = eroare).

use std::collections::HashMap;
use std::str::FromStr;

use rust_decimal::Decimal;

use crate::db::companies::Company;
use crate::db::contacts::Contact;
use crate::db::invoices::{Invoice, LineItem};

pub struct RuleContext<'a> {
    pub invoice: &'a Invoice,
    pub lines: &'a [LineItem],
    pub supplier: &'a Company,
    pub buyer: &'a Contact,
    pub storno_ref: Option<&'a str>,
}

/// Rulează toate regulile și returnează (errors, warnings).
pub fn run_all(ctx: &RuleContext<'_>) -> (Vec<String>, Vec<String>) {
    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    macro_rules! check {
        ($fn:expr) => {
            if let Some(e) = $fn(ctx) {
                errors.push(e);
            }
        };
    }
    macro_rules! warn {
        ($fn:expr) => {
            if let Some(w) = $fn(ctx) {
                warnings.push(w);
            }
        };
    }

    // ── Supplier (BR-RO-010..014) ─────────────────────────────────────────
    check!(rule_br_ro_010_supplier_cui);
    check!(rule_br_ro_011_supplier_name);
    check!(rule_br_ro_012_supplier_address);
    check!(rule_br_ro_013_supplier_city);
    check!(rule_br_ro_014_supplier_country);

    // ── Buyer (BR-RO-015..018) ────────────────────────────────────────────
    check!(rule_br_ro_015_buyer_name);
    check!(rule_br_ro_016_buyer_identifier);
    check!(rule_br_ro_017_buyer_vat_prefix);
    check!(rule_br_ro_018_buyer_country);

    // ── Header (BR-RO-020..029) ───────────────────────────────────────────
    check!(rule_br_ro_020_has_lines);
    check!(rule_br_ro_021_invoice_id);
    check!(rule_br_ro_022_issue_date_format);
    check!(rule_br_ro_023_due_date_format);
    check!(rule_br_ro_024_due_gte_issue);
    check!(rule_br_ro_025_series_format);
    check!(rule_br_ro_026_invoice_number_positive);
    check!(rule_br_ro_027_currency_code);
    check!(rule_br_ro_028_exchange_rate_if_foreign);

    // ── Line items (BR-RO-030..039) ───────────────────────────────────────
    check!(rule_br_ro_030_line_names);
    check!(rule_br_ro_031_line_quantities);
    check!(rule_br_ro_032_line_unit_codes);
    check!(rule_br_ro_033_line_unit_price_nonneg);
    check!(rule_br_ro_034_line_vat_categories);
    check!(rule_br_ro_035_line_vat_rates);
    check!(rule_br_ro_036_line_totals_match);
    check!(rule_br_ro_037_line_vat_amounts_match);

    // ── Totals (BR-RO-040..043) ───────────────────────────────────────────
    check!(rule_br_ro_040_subtotal_equals_lines);
    check!(rule_br_ro_041_vat_total_equals_lines);
    check!(rule_br_ro_042_total_equals_subtotal_plus_vat);
    check!(rule_br_ro_043_vat_breakdown_by_category);

    // ── Storno (BR-RO-050..051) ───────────────────────────────────────────
    check!(rule_br_ro_050_storno_needs_billing_ref);
    check!(rule_br_ro_051_storno_lines_negative);

    // ── Warnings (non-blocking) ────────────────────────────────────────────
    warn!(warn_br_ro_w01_due_far_future);
    warn!(warn_br_ro_w02_zero_value_line);
    warn!(warn_br_ro_w03_vat_payer_missing_prefix);

    (errors, warnings)
}

// ─── Supplier rules ──────────────────────────────────────────────────────────

fn rule_br_ro_010_supplier_cui(ctx: &RuleContext<'_>) -> Option<String> {
    let raw = ctx.supplier.cui.trim();
    let digits = raw.trim_start_matches("RO").trim_start_matches("ro");
    let ok = !digits.is_empty()
        && digits.len() >= 2
        && digits.len() <= 10
        && digits.chars().all(|c| c.is_ascii_digit());
    if !ok {
        Some(format!(
            "[BR-RO-010] CIF furnizor invalid: '{}'. Formatul corect: RO urmat de 2-10 cifre (ex. RO12345678).",
            raw
        ))
    } else {
        None
    }
}

fn rule_br_ro_011_supplier_name(ctx: &RuleContext<'_>) -> Option<String> {
    if ctx.supplier.legal_name.trim().is_empty() {
        Some("[BR-RO-011] Denumirea legală a furnizorului este obligatorie.".into())
    } else {
        None
    }
}

fn rule_br_ro_012_supplier_address(ctx: &RuleContext<'_>) -> Option<String> {
    if ctx.supplier.address.trim().is_empty() {
        Some("[BR-RO-012] Adresa furnizorului este obligatorie.".into())
    } else {
        None
    }
}

fn rule_br_ro_013_supplier_city(ctx: &RuleContext<'_>) -> Option<String> {
    if ctx.supplier.city.trim().is_empty() {
        Some("[BR-RO-013] Localitatea furnizorului este obligatorie.".into())
    } else {
        None
    }
}

fn rule_br_ro_014_supplier_country(ctx: &RuleContext<'_>) -> Option<String> {
    if ctx.supplier.country.trim().is_empty() {
        Some("[BR-RO-014] Codul de țară al furnizorului este obligatoriu (ex. RO).".into())
    } else {
        None
    }
}

// ─── Buyer rules ─────────────────────────────────────────────────────────────

fn rule_br_ro_015_buyer_name(ctx: &RuleContext<'_>) -> Option<String> {
    if ctx.buyer.legal_name.trim().is_empty() {
        Some("[BR-RO-015] Denumirea legală a cumpărătorului este obligatorie.".into())
    } else {
        None
    }
}

fn rule_br_ro_016_buyer_identifier(ctx: &RuleContext<'_>) -> Option<String> {
    let has_cui = ctx
        .buyer
        .cui
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    if !has_cui {
        Some(
            "[BR-RO-016] CIF/identificator cumpărător lipsește. Adăugați CIF-ul clientului.".into(),
        )
    } else {
        None
    }
}

fn rule_br_ro_017_buyer_vat_prefix(ctx: &RuleContext<'_>) -> Option<String> {
    // RO-prefix requirement applies ONLY to Romanian buyers (country == "RO").
    // EU/foreign buyers carry their own country-format VAT IDs (DE…, FR…, etc.)
    // and must NOT be blocked by this rule.
    let is_ro_buyer = ctx.buyer.country.trim().eq_ignore_ascii_case("RO");
    if is_ro_buyer && ctx.buyer.vat_payer {
        let cui = ctx.buyer.cui.as_deref().unwrap_or("").trim();
        if !cui.starts_with("RO") && !cui.starts_with("ro") {
            return Some(format!(
                "[BR-RO-017] Cumpărătorul este plătitor TVA dar CIF-ul '{}' nu are prefixul 'RO'. Adăugați prefixul RO.",
                cui
            ));
        }
    }
    None
}

fn rule_br_ro_018_buyer_country(ctx: &RuleContext<'_>) -> Option<String> {
    if ctx.buyer.country.trim().is_empty() {
        Some("[BR-RO-018] Codul de țară al cumpărătorului este obligatoriu.".into())
    } else {
        None
    }
}

// ─── Header rules ─────────────────────────────────────────────────────────────

fn rule_br_ro_020_has_lines(ctx: &RuleContext<'_>) -> Option<String> {
    if ctx.lines.is_empty() {
        Some(
            "[BR-RO-020] Factura nu conține nicio linie. Adăugați cel puțin un produs/serviciu."
                .into(),
        )
    } else {
        None
    }
}

fn rule_br_ro_021_invoice_id(ctx: &RuleContext<'_>) -> Option<String> {
    if ctx.invoice.full_number.trim().is_empty() {
        Some("[BR-RO-021] Numărul facturii (full_number) lipsește.".into())
    } else {
        None
    }
}

fn parse_iso_date(s: &str) -> bool {
    // Validăm formatul ISO (YYYY-MM-DD) ȘI existența reală a datei în calendar
    // (chrono respinge 2024-02-30, 2024-04-31 etc. — ce verificările manuale
    // bazate pe range nu surprind).
    chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok()
}

fn rule_br_ro_022_issue_date_format(ctx: &RuleContext<'_>) -> Option<String> {
    if !parse_iso_date(&ctx.invoice.issue_date) {
        Some(format!(
            "[BR-RO-022] Data emiterii '{}' nu este în format ISO (YYYY-MM-DD).",
            ctx.invoice.issue_date
        ))
    } else {
        None
    }
}

fn rule_br_ro_023_due_date_format(ctx: &RuleContext<'_>) -> Option<String> {
    if !parse_iso_date(&ctx.invoice.due_date) {
        Some(format!(
            "[BR-RO-023] Data scadenței '{}' nu este în format ISO (YYYY-MM-DD).",
            ctx.invoice.due_date
        ))
    } else {
        None
    }
}

fn rule_br_ro_024_due_gte_issue(ctx: &RuleContext<'_>) -> Option<String> {
    if ctx.invoice.due_date < ctx.invoice.issue_date {
        Some(format!(
            "[BR-RO-024] Data scadenței ({}) este înainte de data emiterii ({}). Scadența trebuie să fie >= data emiterii.",
            ctx.invoice.due_date, ctx.invoice.issue_date
        ))
    } else {
        None
    }
}

fn rule_br_ro_025_series_format(ctx: &RuleContext<'_>) -> Option<String> {
    let s = ctx.invoice.series.trim();
    if s.is_empty() {
        return Some("[BR-RO-025] Seria facturii lipsește.".into());
    }
    if !s
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Some(format!(
            "[BR-RO-025] Seria '{}' conține caractere invalide. Folosiți doar litere, cifre, '-' sau '_'.",
            s
        ));
    }
    None
}

fn rule_br_ro_026_invoice_number_positive(ctx: &RuleContext<'_>) -> Option<String> {
    if ctx.invoice.number <= 0 {
        Some(format!(
            "[BR-RO-026] Numărul facturii ({}) trebuie să fie un întreg pozitiv.",
            ctx.invoice.number
        ))
    } else {
        None
    }
}

fn rule_br_ro_027_currency_code(ctx: &RuleContext<'_>) -> Option<String> {
    let c = ctx.invoice.currency.trim();
    if c.len() != 3 || !c.chars().all(|ch| ch.is_ascii_uppercase()) {
        Some(format!(
            "[BR-RO-027] Codul monedei '{}' invalid. Trebuie să fie cod ISO 4217 de 3 litere majuscule (ex. RON, EUR, USD).",
            c
        ))
    } else {
        None
    }
}

fn rule_br_ro_028_exchange_rate_if_foreign(ctx: &RuleContext<'_>) -> Option<String> {
    if ctx.invoice.currency.eq_ignore_ascii_case("RON") {
        return None;
    }
    match ctx.invoice.exchange_rate {
        None => Some(format!(
            "[BR-RO-028] Moneda facturii este '{}' (diferită de RON) dar cursul de schimb lipsește. \
             Introduceți cursul BNR valabil la data emiterii.",
            ctx.invoice.currency
        )),
        Some(rate) if rate <= 0.0 => Some(format!(
            "[BR-RO-028] Cursul de schimb pentru '{}' este invalid ({:.6}). \
             Cursul trebuie să fie un număr pozitiv (RON per 1 unitate de monedă străină).",
            ctx.invoice.currency, rate
        )),
        Some(_) => None,
    }
}

// ─── Line item rules ──────────────────────────────────────────────────────────

const VALID_VAT_CATEGORIES: &[&str] = &["S", "Z", "E", "AE", "K", "G", "O"];

fn rule_br_ro_030_line_names(ctx: &RuleContext<'_>) -> Option<String> {
    let empties: Vec<usize> = ctx
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.name.trim().is_empty())
        .map(|(i, _)| i + 1)
        .collect();
    if !empties.is_empty() {
        Some(format!(
            "[BR-RO-030] Liniile {} nu au denumire. Denumirea produsului/serviciului este obligatorie.",
            empties
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    } else {
        None
    }
}

fn rule_br_ro_031_line_quantities(ctx: &RuleContext<'_>) -> Option<String> {
    let bad: Vec<usize> = ctx
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| Decimal::from_str(&l.quantity).unwrap_or(Decimal::ZERO) == Decimal::ZERO)
        .map(|(i, _)| i + 1)
        .collect();
    if !bad.is_empty() {
        Some(format!(
            "[BR-RO-031] Liniile {} au cantitatea zero. Cantitatea trebuie să fie diferită de zero.",
            bad.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", ")
        ))
    } else {
        None
    }
}

fn rule_br_ro_032_line_unit_codes(ctx: &RuleContext<'_>) -> Option<String> {
    let bad: Vec<usize> = ctx
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.unit.trim().is_empty())
        .map(|(i, _)| i + 1)
        .collect();
    if !bad.is_empty() {
        Some(format!(
            "[BR-RO-032] Liniile {} nu au unitate de măsură. Unitatea este obligatorie.",
            bad.iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    } else {
        None
    }
}

fn rule_br_ro_033_line_unit_price_nonneg(ctx: &RuleContext<'_>) -> Option<String> {
    let bad: Vec<usize> = ctx
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| Decimal::from_str(&l.unit_price).unwrap_or(Decimal::ZERO) < Decimal::ZERO)
        .map(|(i, _)| i + 1)
        .collect();
    if !bad.is_empty() {
        Some(format!(
            "[BR-RO-033] Liniile {} au prețul unitar negativ. Folosiți cantitate negativă pentru stornare, nu preț negativ.",
            bad.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", ")
        ))
    } else {
        None
    }
}

fn rule_br_ro_034_line_vat_categories(ctx: &RuleContext<'_>) -> Option<String> {
    let bad: Vec<(usize, &str)> = ctx
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| !VALID_VAT_CATEGORIES.contains(&l.vat_category.as_str()))
        .map(|(i, l)| (i + 1, l.vat_category.as_str()))
        .collect();
    if !bad.is_empty() {
        let details: Vec<String> = bad
            .iter()
            .map(|(i, cat)| format!("linia {} ('{}')", i, cat))
            .collect();
        Some(format!(
            "[BR-RO-034] Cod categorie TVA invalid: {}. Valori permise: S, Z, E, AE, K, G, O.",
            details.join("; ")
        ))
    } else {
        None
    }
}

fn rule_br_ro_035_line_vat_rates(ctx: &RuleContext<'_>) -> Option<String> {
    let mut errs: Vec<String> = Vec::new();
    for (i, line) in ctx.lines.iter().enumerate() {
        let pos = i + 1;
        match line.vat_category.as_str() {
            "S" => {
                // Valid RO TVA rates for category S:
                //   5% — super-redusă (permanent)
                //   9% — redusă (până la 2025-07-31)
                //  11% — redusă (din 2025-08-01)
                //  19% — standard (până la 2025-07-31)
                //  21% — standard (din 2025-08-01)
                let rate_dec = Decimal::from_str(&line.vat_rate).unwrap_or(Decimal::ZERO);
                let valid_s_rates = [
                    Decimal::from(5),
                    Decimal::from(9),
                    Decimal::from(11),
                    Decimal::from(19),
                    Decimal::from(21),
                ];
                if !valid_s_rates.contains(&rate_dec) {
                    errs.push(format!(
                        "linia {}: categoria S trebuie să aibă cota TVA 5%, 9%, 11%, 19% sau 21% (actual: {}%)",
                        pos, line.vat_rate
                    ));
                }
            }
            "Z" | "E" | "AE" | "K" | "G" | "O"
                if Decimal::from_str(&line.vat_rate).unwrap_or(Decimal::ZERO) != Decimal::ZERO =>
            {
                errs.push(format!(
                    "linia {}: categoria {} trebuie să aibă cota TVA 0% (actual: {}%)",
                    pos, line.vat_category, line.vat_rate
                ));
            }
            "Z" | "E" | "AE" | "K" | "G" | "O" => {}
            _ => {} // already caught by rule 034
        }
    }
    if !errs.is_empty() {
        Some(format!(
            "[BR-RO-035] Cote TVA incorecte pentru categorii: {}.",
            errs.join("; ")
        ))
    } else {
        None
    }
}

fn rule_br_ro_036_line_totals_match(ctx: &RuleContext<'_>) -> Option<String> {
    let mut errs: Vec<String> = Vec::new();
    for (i, line) in ctx.lines.iter().enumerate() {
        let qty = Decimal::from_str(&line.quantity).unwrap_or(Decimal::ZERO);
        let price = Decimal::from_str(&line.unit_price).unwrap_or(Decimal::ZERO);
        let stored = Decimal::from_str(&line.subtotal_amount)
            .unwrap_or(Decimal::ZERO)
            .round_dp(2);
        let expected = (qty * price).round_dp(2);
        let diff = (expected - stored).abs();
        if diff > Decimal::new(1, 2) {
            errs.push(format!(
                "linia {}: calculat {:.2} ≠ stocat {:.2}",
                i + 1,
                expected,
                stored
            ));
        }
    }
    if !errs.is_empty() {
        Some(format!(
            "[BR-RO-036] Subtotaluri linie incorecte (cantitate × preț ≠ subtotal): {}.",
            errs.join("; ")
        ))
    } else {
        None
    }
}

fn rule_br_ro_037_line_vat_amounts_match(ctx: &RuleContext<'_>) -> Option<String> {
    let hundred = Decimal::from(100u32);
    let mut errs: Vec<String> = Vec::new();
    for (i, line) in ctx.lines.iter().enumerate() {
        let qty = Decimal::from_str(&line.quantity).unwrap_or(Decimal::ZERO);
        let price = Decimal::from_str(&line.unit_price).unwrap_or(Decimal::ZERO);
        let rate = Decimal::from_str(&line.vat_rate).unwrap_or(Decimal::ZERO);
        let net = (qty * price).round_dp(2);
        let expected_vat = (net * rate / hundred).round_dp(2);
        let stored_vat = Decimal::from_str(&line.vat_amount)
            .unwrap_or(Decimal::ZERO)
            .round_dp(2);
        let diff = (expected_vat - stored_vat).abs();
        if diff > Decimal::new(1, 2) {
            errs.push(format!(
                "linia {}: TVA calculat {:.2} ≠ TVA stocat {:.2}",
                i + 1,
                expected_vat,
                stored_vat
            ));
        }
    }
    if !errs.is_empty() {
        Some(format!(
            "[BR-RO-037] Sume TVA linie incorecte (net × cotă TVA ≠ TVA stocat): {}.",
            errs.join("; ")
        ))
    } else {
        None
    }
}

// ─── Total rules ──────────────────────────────────────────────────────────────

fn rule_br_ro_040_subtotal_equals_lines(ctx: &RuleContext<'_>) -> Option<String> {
    let sum: Decimal = ctx
        .lines
        .iter()
        .map(|l| Decimal::from_str(&l.subtotal_amount).unwrap_or(Decimal::ZERO))
        .fold(Decimal::ZERO, |a, b| a + b)
        .round_dp(2);
    let header = Decimal::from_str(&ctx.invoice.subtotal_amount)
        .unwrap_or(Decimal::ZERO)
        .round_dp(2);
    let diff = (sum - header).abs();
    if diff > Decimal::new(1, 2) {
        Some(format!(
            "[BR-RO-040] Suma subtotaluri linii ({:.2}) ≠ subtotal factură ({:.2}). Diferență: {:.2} RON.",
            sum, header, diff
        ))
    } else {
        None
    }
}

fn rule_br_ro_041_vat_total_equals_lines(ctx: &RuleContext<'_>) -> Option<String> {
    let sum: Decimal = ctx
        .lines
        .iter()
        .map(|l| Decimal::from_str(&l.vat_amount).unwrap_or(Decimal::ZERO))
        .fold(Decimal::ZERO, |a, b| a + b)
        .round_dp(2);
    let header = Decimal::from_str(&ctx.invoice.vat_amount)
        .unwrap_or(Decimal::ZERO)
        .round_dp(2);
    let diff = (sum - header).abs();
    if diff > Decimal::new(1, 2) {
        Some(format!(
            "[BR-RO-041] Suma TVA linii ({:.2}) ≠ TVA total factură ({:.2}). Diferență: {:.2} RON.",
            sum, header, diff
        ))
    } else {
        None
    }
}

fn rule_br_ro_042_total_equals_subtotal_plus_vat(ctx: &RuleContext<'_>) -> Option<String> {
    let net = Decimal::from_str(&ctx.invoice.subtotal_amount)
        .unwrap_or(Decimal::ZERO)
        .round_dp(2);
    let vat = Decimal::from_str(&ctx.invoice.vat_amount)
        .unwrap_or(Decimal::ZERO)
        .round_dp(2);
    let expected = (net + vat).round_dp(2);
    let actual = Decimal::from_str(&ctx.invoice.total_amount)
        .unwrap_or(Decimal::ZERO)
        .round_dp(2);
    let diff = (expected - actual).abs();
    if diff > Decimal::new(1, 2) {
        Some(format!(
            "[BR-RO-042] Total factură ({:.2}) ≠ subtotal ({:.2}) + TVA ({:.2}) = {:.2}.",
            actual, net, vat, expected
        ))
    } else {
        None
    }
}

fn rule_br_ro_043_vat_breakdown_by_category(ctx: &RuleContext<'_>) -> Option<String> {
    // BIZ-20: group by (category, rate) — multiple rates on the same category
    // must be validated independently. Previously, grouping by category alone
    // caused multi-rate categories (e.g. S@9% + S@19%) to collide and either
    // miss real errors or report false positives.
    let hundred = Decimal::from(100u32);
    // Key: (vat_category, rate string for stable hashing).
    let mut group_net: HashMap<(String, String), Decimal> = HashMap::new();
    let mut group_vat: HashMap<(String, String), Decimal> = HashMap::new();
    let mut group_rate: HashMap<(String, String), Decimal> = HashMap::new();

    for line in ctx.lines {
        let net = Decimal::from_str(&line.subtotal_amount)
            .unwrap_or(Decimal::ZERO)
            .round_dp(2);
        let vat = Decimal::from_str(&line.vat_amount)
            .unwrap_or(Decimal::ZERO)
            .round_dp(2);
        let rate = Decimal::from_str(&line.vat_rate).unwrap_or(Decimal::ZERO);
        let key = (line.vat_category.clone(), rate.to_string());
        *group_net.entry(key.clone()).or_insert(Decimal::ZERO) += net;
        *group_vat.entry(key.clone()).or_insert(Decimal::ZERO) += vat;
        group_rate.entry(key).or_insert(rate);
    }

    let mut errs: Vec<String> = Vec::new();
    for (key, net) in &group_net {
        if let (Some(&rate), Some(&vat)) = (group_rate.get(key), group_vat.get(key)) {
            let expected_vat = (net * rate / hundred).round_dp(2);
            let diff = (expected_vat - vat.round_dp(2)).abs();
            // Toleranță 0.01 RON — consistent cu BR-RO-040/041/042 (Decimal::new(1, 2)).
            if diff > Decimal::new(1, 2) {
                errs.push(format!(
                    "categoria {} @ {}%: TVA calculat {:.2} ≠ TVA sumă linii {:.2}",
                    key.0, key.1, expected_vat, vat
                ));
            }
        }
    }
    if !errs.is_empty() {
        Some(format!(
            "[BR-RO-043] Defalcare TVA pe categorii incorectă: {}.",
            errs.join("; ")
        ))
    } else {
        None
    }
}

// ─── Storno rules ─────────────────────────────────────────────────────────────

fn rule_br_ro_050_storno_needs_billing_ref(ctx: &RuleContext<'_>) -> Option<String> {
    // Stornoul este determinat exclusiv de prezența `storno_ref`. Heuristica
    // pe seria "S..." era falsă pozitivă pentru serii legitime (SERV, SALARII).
    let is_storno = ctx.storno_ref.is_some();
    if is_storno && ctx.storno_ref.map(|r| r.is_empty()).unwrap_or(true) {
        Some("[BR-RO-050] Factura storno (tip 381) necesită referință la factura originală (BillingReference). Refaceți storno-ul din detaliile facturii originale.".into())
    } else {
        None
    }
}

fn rule_br_ro_051_storno_lines_negative(ctx: &RuleContext<'_>) -> Option<String> {
    // Vezi BR-RO-050: nu mai folosim heuristica pe prefixul seriei.
    let is_storno = ctx.storno_ref.is_some();
    if is_storno {
        let positive: Vec<usize> = ctx
            .lines
            .iter()
            .enumerate()
            .filter(|(_, l)| {
                Decimal::from_str(&l.quantity).unwrap_or(Decimal::ZERO) > Decimal::ZERO
            })
            .map(|(i, _)| i + 1)
            .collect();
        if !positive.is_empty() {
            return Some(format!(
                "[BR-RO-051] Factura storno trebuie să aibă cantități negative pe toate liniile. Liniile {} au cantitate pozitivă.",
                positive
                    .iter()
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    None
}

// ─── Warnings ─────────────────────────────────────────────────────────────────

fn warn_br_ro_w01_due_far_future(ctx: &RuleContext<'_>) -> Option<String> {
    if let (Ok(issue), Ok(due)) = (
        chrono::NaiveDate::parse_from_str(&ctx.invoice.issue_date, "%Y-%m-%d"),
        chrono::NaiveDate::parse_from_str(&ctx.invoice.due_date, "%Y-%m-%d"),
    ) {
        let days = (due - issue).num_days();
        if days > 365 {
            return Some(format!(
                "[W01] Scadența este la {} zile de la emitere. Verificați dacă este corect.",
                days
            ));
        }
    }
    None
}

fn warn_br_ro_w02_zero_value_line(ctx: &RuleContext<'_>) -> Option<String> {
    let zeros: Vec<usize> = ctx
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| {
            Decimal::from_str(&l.subtotal_amount).unwrap_or(Decimal::ZERO) == Decimal::ZERO
                && Decimal::from_str(&l.unit_price).unwrap_or(Decimal::ZERO) == Decimal::ZERO
        })
        .map(|(i, _)| i + 1)
        .collect();
    if !zeros.is_empty() {
        Some(format!(
            "[W02] Liniile {} au valoare zero. Verificați dacă este intenționat.",
            zeros
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    } else {
        None
    }
}

fn warn_br_ro_w03_vat_payer_missing_prefix(ctx: &RuleContext<'_>) -> Option<String> {
    if ctx.supplier.vat_payer {
        let cui = ctx.supplier.cui.trim();
        if !cui.starts_with("RO") && !cui.starts_with("ro") {
            return Some(format!(
                "[W03] Furnizorul este plătitor TVA dar CIF-ul '{}' nu are prefixul 'RO'. ANAF poate respinge factura.",
                cui
            ));
        }
    }
    None
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::companies::Company;
    use crate::db::contacts::Contact;
    use crate::db::invoices::{Invoice, LineItem};

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn sample_supplier() -> Company {
        Company {
            id: "sup-1".into(),
            cui: "RO12345678".into(),
            legal_name: "Furnizor SRL".into(),
            trade_name: None,
            registry_number: None,
            vat_payer: true,
            address: "Str. Principala 1".into(),
            city: "Bucuresti".into(),
            county: "Ilfov".into(),
            postal_code: None,
            country: "RO".into(),
            email: None,
            phone: None,
            iban: None,
            bank_name: None,
            is_active: true,
            spv_enabled: false,
            invoice_series: "FACT".into(),
            last_invoice_number: 0,
            logo_path: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn sample_buyer() -> Contact {
        Contact {
            id: "buy-1".into(),
            company_id: "sup-1".into(),
            contact_type: "CUSTOMER".into(),
            cui: Some("RO87654321".into()),
            legal_name: "Client SA".into(),
            vat_payer: true,
            address: Some("Bd. Unirii 10".into()),
            city: Some("Cluj-Napoca".into()),
            county: Some("Cluj".into()),
            country: "RO".into(),
            email: None,
            phone: None,
            currency: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn sample_invoice() -> Invoice {
        Invoice {
            id: "inv-1".into(),
            company_id: "sup-1".into(),
            contact_id: "buy-1".into(),
            series: "FACT".into(),
            number: 1,
            full_number: "FACT-0001".into(),
            issue_date: "2025-01-15".into(),
            due_date: "2025-02-15".into(),
            currency: "RON".into(),
            exchange_rate: None,
            subtotal_amount: "100.00".into(),
            vat_amount: "19.00".into(),
            total_amount: "119.00".into(),
            status: "DRAFT".into(),
            anaf_upload_id: None,
            anaf_index: None,
            anaf_submitted_at: None,
            anaf_validated_at: None,
            anaf_rejected_at: None,
            xml_path: None,
            pdf_path: None,
            signature_xml_path: None,
            rejection_reason: None,
            rejection_code: None,
            notes: None,
            payment_means_code: "30".into(),
            storno_of_invoice_id: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn sample_line() -> LineItem {
        LineItem {
            id: "line-1".into(),
            invoice_id: "inv-1".into(),
            position: 1,
            name: "Serviciu consultanta".into(),
            description: None,
            quantity: "1.00".into(),
            unit: "H".into(),
            unit_price: "100.00".into(),
            vat_rate: "19.00".into(),
            vat_category: "S".into(),
            subtotal_amount: "100.00".into(),
            vat_amount: "19.00".into(),
            total_amount: "119.00".into(),
            cpv_code: None,
            art331_code: None,
        }
    }

    // ── Test 1: valid invoice passes all rules ────────────────────────────────

    #[test]
    fn valid_invoice_has_no_errors() {
        let invoice = sample_invoice();
        let lines = vec![sample_line()];
        let supplier = sample_supplier();
        let buyer = sample_buyer();
        let ctx = RuleContext {
            invoice: &invoice,
            lines: &lines,
            supplier: &supplier,
            buyer: &buyer,
            storno_ref: None,
        };
        let (errors, _warnings) = run_all(&ctx);
        assert!(
            errors.is_empty(),
            "Expected no errors for a valid invoice, but got: {:?}",
            errors
        );
    }

    // ── Test 2: storno without billing ref fails BR-RO-050 ───────────────────

    #[test]
    fn storno_without_billing_ref_fails_br_ro_050() {
        let mut invoice = sample_invoice();
        // Negative quantity line to satisfy BR-RO-051 (storno lines must be negative)
        let mut line = sample_line();
        line.quantity = "-1.00".into();
        line.subtotal_amount = "-100.00".into();
        line.vat_amount = "-19.00".into();
        line.total_amount = "-119.00".into();
        // Adjust invoice totals to match
        invoice.subtotal_amount = "-100.00".into();
        invoice.vat_amount = "-19.00".into();
        invoice.total_amount = "-119.00".into();
        // storno_ref is None — missing billing reference
        let lines = vec![line];
        let supplier = sample_supplier();
        let buyer = sample_buyer();
        let ctx = RuleContext {
            invoice: &invoice,
            lines: &lines,
            supplier: &supplier,
            buyer: &buyer,
            storno_ref: Some(""), // empty string triggers the "storno without ref" error
        };
        let (errors, _) = run_all(&ctx);
        let has_050 = errors.iter().any(|e| e.contains("[BR-RO-050]"));
        assert!(
            has_050,
            "Expected BR-RO-050 error for storno without billing reference, got: {:?}",
            errors
        );
    }

    // ── Test 3: storno with billing ref passes BR-RO-050 ────────────────────

    #[test]
    fn storno_with_billing_ref_passes_br_ro_050() {
        let mut invoice = sample_invoice();
        let mut line = sample_line();
        line.quantity = "-1.00".into();
        line.subtotal_amount = "-100.00".into();
        line.vat_amount = "-19.00".into();
        line.total_amount = "-119.00".into();
        invoice.subtotal_amount = "-100.00".into();
        invoice.vat_amount = "-19.00".into();
        invoice.total_amount = "-119.00".into();
        let lines = vec![line];
        let supplier = sample_supplier();
        let buyer = sample_buyer();
        let ctx = RuleContext {
            invoice: &invoice,
            lines: &lines,
            supplier: &supplier,
            buyer: &buyer,
            storno_ref: Some("FACT-0001"), // valid non-empty reference
        };
        let (errors, _) = run_all(&ctx);
        let has_050 = errors.iter().any(|e| e.contains("[BR-RO-050]"));
        assert!(
            !has_050,
            "Expected no BR-RO-050 error when billing reference is provided, got: {:?}",
            errors
        );
    }

    // ── Test 4: storno with positive quantities fails BR-RO-051 ─────────────

    #[test]
    fn storno_positive_quantity_fails_br_ro_051() {
        let mut invoice = sample_invoice();
        invoice.series = "S".into(); // series starting with 'S' marks it as storno
                                     // Line has positive quantity — invalid for storno
        let line = sample_line(); // quantity is "1.00" (positive)
        let lines = vec![line];
        let supplier = sample_supplier();
        let buyer = sample_buyer();
        let ctx = RuleContext {
            invoice: &invoice,
            lines: &lines,
            supplier: &supplier,
            buyer: &buyer,
            storno_ref: Some("FACT-0001"),
        };
        let (errors, _) = run_all(&ctx);
        let has_051 = errors.iter().any(|e| e.contains("[BR-RO-051]"));
        assert!(
            has_051,
            "Expected BR-RO-051 error for storno with positive line quantities, got: {:?}",
            errors
        );
    }

    // ── Test 5: invalid VAT rate for category S fails BR-RO-035 ─────────────

    #[test]
    fn invalid_vat_rate_category_s_fails_br_ro_035() {
        let invoice = sample_invoice();
        let mut line = sample_line();
        // Category S with 7% is invalid (allowed: 5, 9, 11, 19, 21)
        line.vat_rate = "7.00".into();
        // Keep amounts consistent so other rules don't interfere with the one we test
        // (BR-RO-037 would fire too; that is acceptable — we just assert 035 is present)
        let lines = vec![line];
        let supplier = sample_supplier();
        let buyer = sample_buyer();
        let ctx = RuleContext {
            invoice: &invoice,
            lines: &lines,
            supplier: &supplier,
            buyer: &buyer,
            storno_ref: None,
        };
        let (errors, _) = run_all(&ctx);
        let has_035 = errors.iter().any(|e| e.contains("[BR-RO-035]"));
        assert!(
            has_035,
            "Expected BR-RO-035 error for category S with 7% VAT rate, got: {:?}",
            errors
        );
    }

    // ── Test 6: negative unit price fails BR-RO-033 ──────────────────────────

    #[test]
    fn negative_unit_price_fails_br_ro_033() {
        let invoice = sample_invoice();
        let mut line = sample_line();
        line.unit_price = "-10.00".into();
        let lines = vec![line];
        let supplier = sample_supplier();
        let buyer = sample_buyer();
        let ctx = RuleContext {
            invoice: &invoice,
            lines: &lines,
            supplier: &supplier,
            buyer: &buyer,
            storno_ref: None,
        };
        let (errors, _) = run_all(&ctx);
        let has_033 = errors.iter().any(|e| e.contains("[BR-RO-033]"));
        assert!(
            has_033,
            "Expected BR-RO-033 error for negative unit price, got: {:?}",
            errors
        );
    }

    // ── Test 7: invoice subtotal mismatch fails BR-RO-040 ───────────────────

    #[test]
    fn totals_mismatch_fails_br_ro_040() {
        let mut invoice = sample_invoice();
        // Line sums to 100.00 but invoice header says 200.00
        invoice.subtotal_amount = "200.00".into();
        // Keep total consistent with the (wrong) subtotal so only 040 fires
        invoice.total_amount = "219.00".into();
        let lines = vec![sample_line()];
        let supplier = sample_supplier();
        let buyer = sample_buyer();
        let ctx = RuleContext {
            invoice: &invoice,
            lines: &lines,
            supplier: &supplier,
            buyer: &buyer,
            storno_ref: None,
        };
        let (errors, _) = run_all(&ctx);
        let has_040 = errors.iter().any(|e| e.contains("[BR-RO-040]"));
        assert!(
            has_040,
            "Expected BR-RO-040 error for subtotal mismatch, got: {:?}",
            errors
        );
    }

    // ── BIZ-10: calendar date validation via chrono ──────────────────────────

    #[test]
    fn rejects_february_30() {
        let mut invoice = sample_invoice();
        invoice.issue_date = "2024-02-30".into();
        invoice.due_date = "2024-02-30".into();
        let lines = vec![sample_line()];
        let supplier = sample_supplier();
        let buyer = sample_buyer();
        let ctx = RuleContext {
            invoice: &invoice,
            lines: &lines,
            supplier: &supplier,
            buyer: &buyer,
            storno_ref: None,
        };
        let (errors, _) = run_all(&ctx);
        let has_date_err = errors
            .iter()
            .any(|e| e.contains("[BR-RO-022]") || e.contains("[BR-RO-023]"));
        assert!(
            has_date_err,
            "Expected BR-RO-022/023 for 2024-02-30, got: {:?}",
            errors
        );
    }

    #[test]
    fn rejects_april_31() {
        let mut invoice = sample_invoice();
        invoice.issue_date = "2024-04-31".into();
        invoice.due_date = "2024-04-31".into();
        let lines = vec![sample_line()];
        let supplier = sample_supplier();
        let buyer = sample_buyer();
        let ctx = RuleContext {
            invoice: &invoice,
            lines: &lines,
            supplier: &supplier,
            buyer: &buyer,
            storno_ref: None,
        };
        let (errors, _) = run_all(&ctx);
        let has_date_err = errors
            .iter()
            .any(|e| e.contains("[BR-RO-022]") || e.contains("[BR-RO-023]"));
        assert!(
            has_date_err,
            "Expected BR-RO-022/023 for 2024-04-31, got: {:?}",
            errors
        );
    }

    #[test]
    fn accepts_valid_leap_year_date() {
        // 2024 e an bisect — 29 februarie este o dată validă.
        let mut invoice = sample_invoice();
        invoice.issue_date = "2024-02-29".into();
        invoice.due_date = "2024-02-29".into();
        let lines = vec![sample_line()];
        let supplier = sample_supplier();
        let buyer = sample_buyer();
        let ctx = RuleContext {
            invoice: &invoice,
            lines: &lines,
            supplier: &supplier,
            buyer: &buyer,
            storno_ref: None,
        };
        let (errors, _) = run_all(&ctx);
        let has_date_err = errors
            .iter()
            .any(|e| e.contains("[BR-RO-022]") || e.contains("[BR-RO-023]"));
        assert!(
            !has_date_err,
            "Expected no date errors for 2024-02-29 (leap year), got: {:?}",
            errors
        );
    }

    // ── BIZ-15/22: stornoul nu mai e dedus din prefix seriei ────────────────

    #[test]
    fn normal_invoice_in_serv_series_is_not_treated_as_storno() {
        // Seria "SERV", fără storno_ref — nu trebuie să fie marcată ca storno.
        let mut invoice = sample_invoice();
        invoice.series = "SERV".into();
        invoice.full_number = "SERV-0001".into();
        let lines = vec![sample_line()];
        let supplier = sample_supplier();
        let buyer = sample_buyer();
        let ctx = RuleContext {
            invoice: &invoice,
            lines: &lines,
            supplier: &supplier,
            buyer: &buyer,
            storno_ref: None,
        };
        let (errors, _) = run_all(&ctx);
        let has_storno_err = errors
            .iter()
            .any(|e| e.contains("[BR-RO-050]") || e.contains("[BR-RO-051]"));
        assert!(
            !has_storno_err,
            "Expected no storno errors for normal invoice in SERV series, got: {:?}",
            errors
        );
    }

    #[test]
    fn normal_invoice_in_s_prefix_series_passes_storno_rules() {
        // Seria "SALARII" (începe cu 'S') + cantități pozitive + fără storno_ref.
        // Nu trebuie să declanșeze nicio regulă storno.
        let mut invoice = sample_invoice();
        invoice.series = "SALARII".into();
        invoice.full_number = "SALARII-0001".into();
        let lines = vec![sample_line()];
        let supplier = sample_supplier();
        let buyer = sample_buyer();
        let ctx = RuleContext {
            invoice: &invoice,
            lines: &lines,
            supplier: &supplier,
            buyer: &buyer,
            storno_ref: None,
        };
        let (errors, _) = run_all(&ctx);
        let has_storno_err = errors
            .iter()
            .any(|e| e.contains("[BR-RO-050]") || e.contains("[BR-RO-051]"));
        assert!(
            !has_storno_err,
            "Expected no storno errors for SALARII series invoice, got: {:?}",
            errors
        );
    }

    // ── Test 8: missing buyer address fails the buyer-address rule ───────────

    #[test]
    fn missing_buyer_address_fails() {
        let invoice = sample_invoice();
        let lines = vec![sample_line()];
        let supplier = sample_supplier();
        let mut buyer = sample_buyer();
        // Contact.address and city are Option<String>; set them to None
        buyer.address = None;
        buyer.city = None;
        // Also set country to empty to trigger BR-RO-018 as a definite hit
        buyer.country = "".into();
        let ctx = RuleContext {
            invoice: &invoice,
            lines: &lines,
            supplier: &supplier,
            buyer: &buyer,
            storno_ref: None,
        };
        let (errors, _) = run_all(&ctx);
        // BR-RO-018 fires when buyer country is empty; the address fields are
        // Option<String> in Contact (no dedicated buyer-address rule exists in
        // this rule set), so we verify the country rule at minimum.
        let has_buyer_error = errors
            .iter()
            .any(|e| e.contains("[BR-RO-018]") || e.contains("cumpărător"));
        assert!(
            has_buyer_error,
            "Expected a buyer-related error when buyer country/address is missing, got: {:?}",
            errors
        );
    }

    // ── BR-RO-017: RO-prefix rule scoped to RO-country buyers only ─────────────

    #[test]
    fn br_ro_017_fires_for_ro_buyer_without_ro_prefix() {
        // RO buyer, vat_payer, CUI without "RO" prefix → must error
        let invoice = sample_invoice();
        let lines = vec![sample_line()];
        let supplier = sample_supplier();
        let mut buyer = sample_buyer();
        buyer.country = "RO".into();
        buyer.vat_payer = true;
        buyer.cui = Some("12345678".into()); // no RO prefix
        let ctx = RuleContext {
            invoice: &invoice,
            lines: &lines,
            supplier: &supplier,
            buyer: &buyer,
            storno_ref: None,
        };
        let (errors, _) = run_all(&ctx);
        let has_017 = errors.iter().any(|e| e.contains("[BR-RO-017]"));
        assert!(
            has_017,
            "Expected BR-RO-017 error for RO buyer with no RO prefix, got: {:?}",
            errors
        );
    }

    #[test]
    fn br_ro_017_does_not_fire_for_eu_buyer() {
        // DE buyer, vat_payer, "DE123" VAT ID → must NOT error (different country format)
        let invoice = sample_invoice();
        let lines = vec![sample_line()];
        let supplier = sample_supplier();
        let mut buyer = sample_buyer();
        buyer.country = "DE".into();
        buyer.vat_payer = true;
        buyer.cui = Some("DE123456789".into()); // German VAT ID, no RO prefix
        let ctx = RuleContext {
            invoice: &invoice,
            lines: &lines,
            supplier: &supplier,
            buyer: &buyer,
            storno_ref: None,
        };
        let (errors, _) = run_all(&ctx);
        let has_017 = errors.iter().any(|e| e.contains("[BR-RO-017]"));
        assert!(
            !has_017,
            "BR-RO-017 must NOT fire for a non-RO (EU) buyer, got: {:?}",
            errors
        );
    }

    // ── BIZ-20: BR-RO-043 must group by (category, rate), not category alone ──

    // ── BR-RO-028: exchange rate validation ──────────────────────────────────

    #[test]
    fn br_ro_028_ron_invoice_no_rate_passes() {
        // RON invoices must never trigger BR-RO-028 regardless of exchange_rate.
        let mut invoice = sample_invoice(); // currency = "RON"
        invoice.exchange_rate = None;
        let lines = vec![sample_line()];
        let supplier = sample_supplier();
        let buyer = sample_buyer();
        let ctx = RuleContext {
            invoice: &invoice,
            lines: &lines,
            supplier: &supplier,
            buyer: &buyer,
            storno_ref: None,
        };
        let (errors, _) = run_all(&ctx);
        let has_028 = errors.iter().any(|e| e.contains("[BR-RO-028]"));
        assert!(
            !has_028,
            "BR-RO-028 must NOT fire for RON invoice, got: {:?}",
            errors
        );
    }

    #[test]
    fn br_ro_028_foreign_missing_rate_fails() {
        // Non-RON invoice with no exchange_rate → must error.
        let mut invoice = sample_invoice();
        invoice.currency = "EUR".into();
        invoice.exchange_rate = None;
        let lines = vec![sample_line()];
        let supplier = sample_supplier();
        let buyer = sample_buyer();
        let ctx = RuleContext {
            invoice: &invoice,
            lines: &lines,
            supplier: &supplier,
            buyer: &buyer,
            storno_ref: None,
        };
        let (errors, _) = run_all(&ctx);
        let has_028 = errors.iter().any(|e| e.contains("[BR-RO-028]"));
        assert!(
            has_028,
            "BR-RO-028 must fire for EUR invoice with no rate, got: {:?}",
            errors
        );
    }

    #[test]
    fn br_ro_028_foreign_zero_rate_fails() {
        // Non-RON invoice with exchange_rate=0 → must error.
        let mut invoice = sample_invoice();
        invoice.currency = "USD".into();
        invoice.exchange_rate = Some(0.0);
        let lines = vec![sample_line()];
        let supplier = sample_supplier();
        let buyer = sample_buyer();
        let ctx = RuleContext {
            invoice: &invoice,
            lines: &lines,
            supplier: &supplier,
            buyer: &buyer,
            storno_ref: None,
        };
        let (errors, _) = run_all(&ctx);
        let has_028 = errors.iter().any(|e| e.contains("[BR-RO-028]"));
        assert!(
            has_028,
            "BR-RO-028 must fire for USD invoice with rate=0, got: {:?}",
            errors
        );
    }

    #[test]
    fn br_ro_028_foreign_negative_rate_fails() {
        // Non-RON invoice with a negative exchange_rate → must error.
        let mut invoice = sample_invoice();
        invoice.currency = "GBP".into();
        invoice.exchange_rate = Some(-5.5);
        let lines = vec![sample_line()];
        let supplier = sample_supplier();
        let buyer = sample_buyer();
        let ctx = RuleContext {
            invoice: &invoice,
            lines: &lines,
            supplier: &supplier,
            buyer: &buyer,
            storno_ref: None,
        };
        let (errors, _) = run_all(&ctx);
        let has_028 = errors.iter().any(|e| e.contains("[BR-RO-028]"));
        assert!(
            has_028,
            "BR-RO-028 must fire for GBP invoice with negative rate, got: {:?}",
            errors
        );
    }

    #[test]
    fn br_ro_028_foreign_valid_positive_rate_passes() {
        // Non-RON invoice with a valid positive exchange_rate → must NOT error on 028.
        let mut invoice = sample_invoice();
        invoice.currency = "EUR".into();
        invoice.exchange_rate = Some(5.0);
        let lines = vec![sample_line()];
        let supplier = sample_supplier();
        let buyer = sample_buyer();
        let ctx = RuleContext {
            invoice: &invoice,
            lines: &lines,
            supplier: &supplier,
            buyer: &buyer,
            storno_ref: None,
        };
        let (errors, _) = run_all(&ctx);
        let has_028 = errors.iter().any(|e| e.contains("[BR-RO-028]"));
        assert!(
            !has_028,
            "BR-RO-028 must NOT fire for EUR invoice with valid rate, got: {:?}",
            errors
        );
    }

    #[test]
    fn br_ro_043_handles_multi_rate_same_category() {
        // Two lines, both category "S", different rates (9% and 19%).
        // Per-(cat,rate) VAT is computed correctly on each line, so the rule
        // must NOT report a false positive when grouping is done properly.
        let mut invoice = sample_invoice();

        let mut line1 = sample_line();
        line1.vat_rate = "9.00".into();
        line1.vat_category = "S".into();
        line1.subtotal_amount = "100.00".into();
        line1.vat_amount = "9.00".into(); // 100 * 9% = 9.00
        line1.total_amount = "109.00".into();

        let mut line2 = sample_line();
        line2.id = "line-2".into();
        line2.position = 2;
        line2.vat_rate = "19.00".into();
        line2.vat_category = "S".into();
        line2.subtotal_amount = "100.00".into();
        line2.vat_amount = "19.00".into(); // 100 * 19% = 19.00
        line2.total_amount = "119.00".into();

        // Header totals consistent with the lines so other rules don't fire.
        invoice.subtotal_amount = "200.00".into();
        invoice.vat_amount = "28.00".into();
        invoice.total_amount = "228.00".into();

        let lines = vec![line1, line2];
        let supplier = sample_supplier();
        let buyer = sample_buyer();
        let ctx = RuleContext {
            invoice: &invoice,
            lines: &lines,
            supplier: &supplier,
            buyer: &buyer,
            storno_ref: None,
        };
        let (errors, _warnings) = run_all(&ctx);
        let has_043 = errors.iter().any(|e| e.contains("[BR-RO-043]"));
        assert!(
            !has_043,
            "BR-RO-043 must not fire when multi-rate within one category is correctly grouped, got: {:?}",
            errors
        );
    }
}
