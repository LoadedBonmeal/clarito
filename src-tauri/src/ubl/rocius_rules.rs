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
            "[BR-RO-016] CIF/identificator cumpărător lipsește. Adăugați CIF-ul clientului."
                .into(),
        )
    } else {
        None
    }
}

fn rule_br_ro_017_buyer_vat_prefix(ctx: &RuleContext<'_>) -> Option<String> {
    if ctx.buyer.vat_payer {
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
        Some("[BR-RO-020] Factura nu conține nicio linie. Adăugați cel puțin un produs/serviciu.".into())
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
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return false;
    }
    let y: u16 = parts[0].parse().unwrap_or(0);
    let m: u8 = parts[1].parse().unwrap_or(0);
    let d: u8 = parts[2].parse().unwrap_or(0);
    y >= 2000 && y <= 2099 && m >= 1 && m <= 12 && d >= 1 && d <= 31
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
    if ctx.invoice.currency != "RON" && ctx.invoice.exchange_rate.is_none() {
        Some(format!(
            "[BR-RO-028] Moneda facturii este '{}' (diferită de RON) dar cursul de schimb lipsește.",
            ctx.invoice.currency
        ))
    } else {
        None
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
            bad.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", ")
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
                    Decimal::from(5), Decimal::from(9), Decimal::from(11),
                    Decimal::from(19), Decimal::from(21),
                ];
                if !valid_s_rates.contains(&rate_dec) {
                    errs.push(format!(
                        "linia {}: categoria S trebuie să aibă cota TVA 5%, 9%, 11%, 19% sau 21% (actual: {}%)",
                        pos, line.vat_rate
                    ));
                }
            }
            "Z" | "E" | "AE" | "K" | "G" | "O" => {
                if Decimal::from_str(&line.vat_rate).unwrap_or(Decimal::ZERO) != Decimal::ZERO {
                    errs.push(format!(
                        "linia {}: categoria {} trebuie să aibă cota TVA 0% (actual: {}%)",
                        pos, line.vat_category, line.vat_rate
                    ));
                }
            }
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
        let stored = Decimal::from_str(&line.subtotal_amount).unwrap_or(Decimal::ZERO).round_dp(2);
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
        let stored_vat = Decimal::from_str(&line.vat_amount).unwrap_or(Decimal::ZERO).round_dp(2);
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
    let header = Decimal::from_str(&ctx.invoice.subtotal_amount).unwrap_or(Decimal::ZERO).round_dp(2);
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
    let header = Decimal::from_str(&ctx.invoice.vat_amount).unwrap_or(Decimal::ZERO).round_dp(2);
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
    let net = Decimal::from_str(&ctx.invoice.subtotal_amount).unwrap_or(Decimal::ZERO).round_dp(2);
    let vat = Decimal::from_str(&ctx.invoice.vat_amount).unwrap_or(Decimal::ZERO).round_dp(2);
    let expected = (net + vat).round_dp(2);
    let actual = Decimal::from_str(&ctx.invoice.total_amount).unwrap_or(Decimal::ZERO).round_dp(2);
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
    let hundred = Decimal::from(100u32);
    let mut cat_net: HashMap<String, Decimal> = HashMap::new();
    let mut cat_vat: HashMap<String, Decimal> = HashMap::new();
    let mut cat_rate: HashMap<String, Decimal> = HashMap::new();

    for line in ctx.lines {
        let net = Decimal::from_str(&line.subtotal_amount).unwrap_or(Decimal::ZERO).round_dp(2);
        let vat = Decimal::from_str(&line.vat_amount).unwrap_or(Decimal::ZERO).round_dp(2);
        let rate = Decimal::from_str(&line.vat_rate).unwrap_or(Decimal::ZERO);
        *cat_net
            .entry(line.vat_category.clone())
            .or_insert(Decimal::ZERO) += net;
        *cat_vat
            .entry(line.vat_category.clone())
            .or_insert(Decimal::ZERO) += vat;
        cat_rate
            .entry(line.vat_category.clone())
            .or_insert(rate);
    }

    let mut errs: Vec<String> = Vec::new();
    for (cat, net) in &cat_net {
        if let (Some(&rate), Some(&vat)) = (cat_rate.get(cat), cat_vat.get(cat)) {
            let expected_vat = (net * rate / hundred).round_dp(2);
            let diff = (expected_vat - vat.round_dp(2)).abs();
            if diff > Decimal::new(2, 2) {
                errs.push(format!(
                    "categoria {}: TVA calculat {:.2} ≠ TVA sumă linii {:.2}",
                    cat, expected_vat, vat
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
    let is_storno = ctx.storno_ref.is_some()
        || ctx.invoice.series.starts_with('S');
    if is_storno && ctx.storno_ref.map(|r| r.is_empty()).unwrap_or(true) {
        Some("[BR-RO-050] Factura storno (tip 381) necesită referință la factura originală (BillingReference). Refaceți storno-ul din detaliile facturii originale.".into())
    } else {
        None
    }
}

fn rule_br_ro_051_storno_lines_negative(ctx: &RuleContext<'_>) -> Option<String> {
    let is_storno = ctx.storno_ref.is_some()
        || ctx.invoice.series.starts_with('S');
    if is_storno {
        let positive: Vec<usize> = ctx
            .lines
            .iter()
            .enumerate()
            .filter(|(_, l)| Decimal::from_str(&l.quantity).unwrap_or(Decimal::ZERO) > Decimal::ZERO)
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
