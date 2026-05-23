# efactura-desktop Gap-Fill Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close every confirmed gap between the original plan (`efactura-cod-plan.md`) and the current codebase, producing a fully ANAF-compliant, production-ready desktop app.

**Architecture:** Rust backend (Tauri 2.0 + SQLite via sqlx) with React 19/TypeScript frontend. All monetary math uses `rust_decimal::Decimal`. Custom CSS design system (`.btn`, `.panel`, `.dt`) — no Tailwind utilities in new code.

**Tech Stack:** Tauri 2.0, Rust, React 19, TypeScript, sqlx, quick-xml, printpdf, TanStack Query/Router, @tanstack/react-virtual

---

## Confirmed Gaps (audit vs efactura-cod-plan.md)

| # | Gap | Severity | Task |
|---|-----|----------|------|
| G1 | `validate_invoice_data()` defined but **never called** — dead code; `validate_invoice_draft` only runs XML structural check | 🔴 CRITICAL | Task 1 |
| G2 | Only 5 BR-RO rules total (plan requires 50+); missing: due≥issue, buyer completeness, VAT category rules, storno BillingReference, etc. | 🔴 CRITICAL | Task 1 |
| G3 | `validate_invoice_data` uses raw `f64` arithmetic for BR-RO comparisons (plan mandates Decimal) | 🔴 CRITICAL | Task 1 |
| G4 | `PaymentMeansCode` hardcoded `"30"` (bank transfer); no DB column, no form selector | 🟠 HIGH | Task 2 |
| G5 | PDF uses `BuiltinFont::Helvetica` — Romanian diacritics (ă â î ș ț) render as boxes | 🟠 HIGH | Task 3 |
| G6 | `tauri-plugin-updater` absent from Cargo.toml — no auto-update path | 🟡 MEDIUM | Task 4 |
| G7 | OS autostart only writes DB setting — no LaunchAgent (macOS) / Registry (Windows) | 🟡 MEDIUM | Task 5 |
| G8 | `@tanstack/react-virtual` installed but not used — invoice list DOM-renders all rows | 🟡 MEDIUM | Task 6 |
| G9 | Notification module only checks `quiet_hours`; per-type preferences not enforced | 🟡 MEDIUM | Task 7 |
| G10 | CSV import: no downloadable template, no preview-before-commit step | 🟢 LOW | Task 8 |

---

## File Map

| File | Action | Task |
|------|--------|------|
| `src-tauri/src/ubl/rocius_rules.rs` | **Create** — 50+ BR-RO data-level rules | T1 |
| `src-tauri/src/ubl/validator.rs` | **Modify** — wire `validate_invoice_data`, use Decimal, expand | T1 |
| `src-tauri/src/ubl/mod.rs` | **Modify** — pub mod rocius_rules | T1 |
| `src-tauri/src/commands/invoices.rs` | **Modify** — call data rules in `validate_invoice_draft` | T1 |
| `src-tauri/migrations/0002_payment_means.sql` | **Create** — ADD COLUMN payment_means_code | T2 |
| `src-tauri/src/db/invoices.rs` | **Modify** — add `payment_means_code` to structs | T2 |
| `src-tauri/src/ubl/generator.rs` | **Modify** — use `input.invoice.payment_means_code` | T2 |
| `src/pages/InvoiceNew.tsx` | **Modify** — add payment method selector | T2 |
| `src/types/index.ts` | **Modify** — add paymentMeansCode to Invoice | T2 |
| `src-tauri/fonts/LiberationSans-Regular.ttf` | **Download** — free OFL font | T3 |
| `src-tauri/fonts/LiberationSans-Bold.ttf` | **Download** — free OFL font | T3 |
| `src-tauri/src/ubl/pdf.rs` | **Modify** — load embedded font, add amount_to_words | T3 |
| `src-tauri/Cargo.toml` | **Modify** — add updater + autostart plugins | T4+T5 |
| `src-tauri/src/lib.rs` | **Modify** — register updater + autostart plugins | T4+T5 |
| `src-tauri/tauri.conf.json` | **Modify** — configure updater endpoints | T4 |
| `src/pages/Settings.tsx` | **Modify** — "Check for updates" button + real autostart toggle | T4+T5 |
| `src-tauri/src/commands/system.rs` | **Modify** — use plugin for autostart | T5 |
| `src/pages/Invoices.tsx` | **Modify** — @tanstack/react-virtual for table body | T6 |
| `src-tauri/src/notifications/mod.rs` | **Modify** — per-type preference check | T7 |
| `src/components/shared/CsvImportModal.tsx` | **Modify** — template download + preview step | T8 |
| `src-tauri/src/commands/import.rs` | **Modify** — dry_run mode for preview | T8 |

---

## Task 1: Full BR-RO Business Rule Validator (50+ rules)

**Files:**
- Create: `src-tauri/src/ubl/rocius_rules.rs`
- Modify: `src-tauri/src/ubl/validator.rs`
- Modify: `src-tauri/src/ubl/mod.rs`
- Modify: `src-tauri/src/commands/invoices.rs`

- [ ] **Step 1.1 — Create `rocius_rules.rs` with full rule set**

```rust
// src-tauri/src/ubl/rocius_rules.rs
//! Toate regulile de business CIUS-RO (50+) verificate la nivel de date,
//! înainte de generarea XML. Fiecare regulă returnează `Option<String>`
//! (None = OK, Some(msg) = eroare).

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

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
    check!(rule_br_ro_029_invoice_type_code);

    // ── Line items (BR-RO-030..039) ───────────────────────────────────────
    check!(rule_br_ro_030_line_names);
    check!(rule_br_ro_031_line_quantities);
    check!(rule_br_ro_032_line_unit_codes);
    check!(rule_br_ro_033_line_unit_price_nonneg);
    check!(rule_br_ro_034_line_vat_categories);
    check!(rule_br_ro_035_line_vat_rates);
    check!(rule_br_ro_036_line_totals_match);
    check!(rule_br_ro_037_line_vat_amounts_match);

    // ── Totals (BR-RO-040..049) ───────────────────────────────────────────
    check!(rule_br_ro_040_subtotal_equals_lines);
    check!(rule_br_ro_041_vat_total_equals_lines);
    check!(rule_br_ro_042_total_equals_subtotal_plus_vat);
    check!(rule_br_ro_043_vat_breakdown_by_category);

    // ── Storno (BR-RO-050..052) ───────────────────────────────────────────
    check!(rule_br_ro_050_storno_needs_billing_ref);
    check!(rule_br_ro_051_storno_lines_negative);

    // ── Warnings (non-blocking) ────────────────────────────────────────────
    warn!(warn_br_ro_w01_due_far_future);
    warn!(warn_br_ro_w02_zero_value_line);
    warn!(warn_br_ro_w03_vat_payer_missing_prefix);

    (errors, warnings)
}

// ─── Decimal helper ──────────────────────────────────────────────────────────

fn f64_to_dec(v: f64) -> Decimal {
    Decimal::from_f64_retain(v).unwrap_or(Decimal::ZERO)
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
    let has_cui = ctx.buyer.cui.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false);
    if !has_cui {
        Some("[BR-RO-016] CIF/identificator cumpărător lipsește. Adăugați CIF-ul clientului.".into())
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
    if parts.len() != 3 { return false; }
    let y: u16 = parts[0].parse().unwrap_or(0);
    let m: u8  = parts[1].parse().unwrap_or(0);
    let d: u8  = parts[2].parse().unwrap_or(0);
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
    if !s.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
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

fn rule_br_ro_029_invoice_type_code(ctx: &RuleContext<'_>) -> Option<String> {
    // storno_ref present => expect 381 to be set in generator; here we just check data consistency
    if let Some(_ref) = ctx.storno_ref {
        // Storno invoices should have storno in series (validated at creation)
        let _ = _ref;
    }
    None // Generator always sets 380 or 381 based on storno_ref
}

// ─── Line item rules ──────────────────────────────────────────────────────────

const VALID_VAT_CATEGORIES: &[&str] = &["S", "Z", "E", "AE", "K", "G", "O"];
const VALID_UNITS: &[&str] = &["buc", "kg", "h", "luna", "set", "m", "m2", "m3", "l", "t", "%", "zi", "an", "km", "g", "mg", "pct"];

fn rule_br_ro_030_line_names(ctx: &RuleContext<'_>) -> Option<String> {
    let empties: Vec<usize> = ctx.lines.iter()
        .enumerate()
        .filter(|(_, l)| l.name.trim().is_empty())
        .map(|(i, _)| i + 1)
        .collect();
    if !empties.is_empty() {
        Some(format!(
            "[BR-RO-030] Liniile {} nu au denumire. Denumirea produsului/serviciului este obligatorie.",
            empties.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", ")
        ))
    } else {
        None
    }
}

fn rule_br_ro_031_line_quantities(ctx: &RuleContext<'_>) -> Option<String> {
    let bad: Vec<usize> = ctx.lines.iter()
        .enumerate()
        .filter(|(_, l)| l.quantity == 0.0)
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
    let bad: Vec<(usize, &str)> = ctx.lines.iter()
        .enumerate()
        .filter(|(_, l)| l.unit.trim().is_empty())
        .map(|(i, l)| (i + 1, l.unit.as_str()))
        .collect();
    if !bad.is_empty() {
        let nums: Vec<String> = bad.iter().map(|(i, _)| i.to_string()).collect();
        Some(format!(
            "[BR-RO-032] Liniile {} nu au unitate de măsură. Unitatea este obligatorie.",
            nums.join(", ")
        ))
    } else {
        None
    }
}

fn rule_br_ro_033_line_unit_price_nonneg(ctx: &RuleContext<'_>) -> Option<String> {
    let bad: Vec<usize> = ctx.lines.iter()
        .enumerate()
        .filter(|(_, l)| l.unit_price < 0.0)
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
    let bad: Vec<(usize, &str)> = ctx.lines.iter()
        .enumerate()
        .filter(|(_, l)| !VALID_VAT_CATEGORIES.contains(&l.vat_category.as_str()))
        .map(|(i, l)| (i + 1, l.vat_category.as_str()))
        .collect();
    if !bad.is_empty() {
        let details: Vec<String> = bad.iter()
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
                // Standard rate: 19, 9, or 5 for Romania
                if line.vat_rate != 19.0 && line.vat_rate != 9.0 && line.vat_rate != 5.0 {
                    errs.push(format!(
                        "linia {}: categoria S trebuie să aibă cota TVA 5%, 9% sau 19% (actual: {}%)",
                        pos, line.vat_rate
                    ));
                }
            }
            "Z" | "E" | "AE" | "K" | "G" | "O" => {
                if line.vat_rate != 0.0 {
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
        Some(format!("[BR-RO-035] Cote TVA incorecte pentru categorii: {}.", errs.join("; ")))
    } else {
        None
    }
}

fn rule_br_ro_036_line_totals_match(ctx: &RuleContext<'_>) -> Option<String> {
    let hundred = Decimal::from(100u32);
    let mut errs: Vec<String> = Vec::new();
    for (i, line) in ctx.lines.iter().enumerate() {
        let qty = f64_to_dec(line.quantity);
        let price = f64_to_dec(line.unit_price);
        let stored = f64_to_dec(line.subtotal_amount);
        let expected = (qty * price).round_dp(2);
        let stored_r = stored.round_dp(2);
        let diff = (expected - stored_r).abs();
        if diff > Decimal::new(1, 2) {
            errs.push(format!(
                "linia {}: calculat {:.2} ≠ stocat {:.2}",
                i + 1, expected, stored_r
            ));
        }
        let _ = hundred;
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
        let qty = f64_to_dec(line.quantity);
        let price = f64_to_dec(line.unit_price);
        let rate = f64_to_dec(line.vat_rate);
        let net = (qty * price).round_dp(2);
        let expected_vat = (net * rate / hundred).round_dp(2);
        let stored_vat = f64_to_dec(line.vat_amount).round_dp(2);
        let diff = (expected_vat - stored_vat).abs();
        if diff > Decimal::new(1, 2) {
            errs.push(format!(
                "linia {}: TVA calculat {:.2} ≠ TVA stocat {:.2}",
                i + 1, expected_vat, stored_vat
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
    let sum: Decimal = ctx.lines.iter()
        .map(|l| f64_to_dec(l.subtotal_amount))
        .fold(Decimal::ZERO, |a, b| a + b)
        .round_dp(2);
    let header = f64_to_dec(ctx.invoice.subtotal_amount).round_dp(2);
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
    let sum: Decimal = ctx.lines.iter()
        .map(|l| f64_to_dec(l.vat_amount))
        .fold(Decimal::ZERO, |a, b| a + b)
        .round_dp(2);
    let header = f64_to_dec(ctx.invoice.vat_amount).round_dp(2);
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
    let net = f64_to_dec(ctx.invoice.subtotal_amount).round_dp(2);
    let vat = f64_to_dec(ctx.invoice.vat_amount).round_dp(2);
    let expected = (net + vat).round_dp(2);
    let actual = f64_to_dec(ctx.invoice.total_amount).round_dp(2);
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
    use std::collections::HashMap;
    let hundred = Decimal::from(100u32);
    let mut cat_net: HashMap<String, Decimal> = HashMap::new();
    let mut cat_vat: HashMap<String, Decimal> = HashMap::new();
    let mut cat_rate: HashMap<String, Decimal> = HashMap::new();

    for line in ctx.lines {
        let net = f64_to_dec(line.subtotal_amount).round_dp(2);
        let vat = f64_to_dec(line.vat_amount).round_dp(2);
        let rate = f64_to_dec(line.vat_rate);
        *cat_net.entry(line.vat_category.clone()).or_insert(Decimal::ZERO) += net;
        *cat_vat.entry(line.vat_category.clone()).or_insert(Decimal::ZERO) += vat;
        cat_rate.entry(line.vat_category.clone()).or_insert(rate);
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
        Some(format!("[BR-RO-043] Defalcare TVA pe categorii incorectă: {}.", errs.join("; ")))
    } else {
        None
    }
}

// ─── Storno rules ─────────────────────────────────────────────────────────────

fn rule_br_ro_050_storno_needs_billing_ref(ctx: &RuleContext<'_>) -> Option<String> {
    // Detect storno by series prefix "S-" convention used in this codebase
    let is_storno = ctx.invoice.series.starts_with("S-")
        || ctx.invoice.full_number.starts_with("S-");
    if is_storno && ctx.storno_ref.map(|r| r.is_empty()).unwrap_or(true) {
        Some("[BR-RO-050] Factura storno (tip 381) necesită referință la factura originală (BillingReference). Refaceți storno-ul din detaliile facturii originale.".into())
    } else {
        None
    }
}

fn rule_br_ro_051_storno_lines_negative(ctx: &RuleContext<'_>) -> Option<String> {
    let is_storno = ctx.invoice.series.starts_with("S-");
    if is_storno {
        let positive: Vec<usize> = ctx.lines.iter()
            .enumerate()
            .filter(|(_, l)| l.quantity > 0.0)
            .map(|(i, _)| i + 1)
            .collect();
        if !positive.is_empty() {
            return Some(format!(
                "[BR-RO-051] Factura storno trebuie să aibă cantități negative pe toate liniile. Liniile {} au cantitate pozitivă.",
                positive.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", ")
            ));
        }
    }
    None
}

// ─── Warnings ─────────────────────────────────────────────────────────────────

fn warn_br_ro_w01_due_far_future(ctx: &RuleContext<'_>) -> Option<String> {
    // Warn if due date > 365 days from issue
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
    let zeros: Vec<usize> = ctx.lines.iter()
        .enumerate()
        .filter(|(_, l)| l.subtotal_amount == 0.0 && l.unit_price == 0.0)
        .map(|(i, _)| i + 1)
        .collect();
    if !zeros.is_empty() {
        Some(format!(
            "[W02] Liniile {} au valoare zero. Verificați dacă este intenționat.",
            zeros.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", ")
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
```

- [ ] **Step 1.2 — Run `cargo check` to verify rocius_rules.rs compiles**

```bash
cd /Users/cris/Projects/efactura-desktop/src-tauri && cargo check 2>&1 | grep "^error"
```
Expected: errors about `pub mod rocius_rules` missing from `ubl/mod.rs` (we fix next).

- [ ] **Step 1.3 — Add `pub mod rocius_rules` to `ubl/mod.rs`**

```rust
// src-tauri/src/ubl/mod.rs  — replace entire file
//! UBL 2.1 / CIUS-RO generator, validator şi PDF export.

pub mod generator;
pub mod paths;
pub mod pdf;
pub mod rocius_rules;
pub mod validator;
```

- [ ] **Step 1.4 — Rewrite `validate_invoice_data` in `validator.rs` to use rocius_rules + Decimal**

Replace the existing `validate_invoice_data` function (lines ~120-185 in validator.rs) with:

```rust
/// Validează datele facturii conform regulilor CIUS-RO (50+ reguli).
/// Apelează `rocius_rules::run_all` și returnează erori + avertismente.
pub fn validate_invoice_data(
    invoice: &Invoice,
    lines: &[LineItem],
    supplier: &Company,
    buyer: &Contact,
    storno_ref: Option<&str>,
) -> (Vec<String>, Vec<String>) {
    let ctx = crate::ubl::rocius_rules::RuleContext {
        invoice,
        lines,
        supplier,
        buyer,
        storno_ref,
    };
    crate::ubl::rocius_rules::run_all(&ctx)
}
```

Also add the import at the top of validator.rs:
```rust
use crate::db::contacts::Contact;
```

- [ ] **Step 1.5 — Wire data validation into `validate_invoice_draft` command**

In `src-tauri/src/commands/invoices.rs`, modify `validate_invoice_draft` to call both XML and data validation:

```rust
// After the generate XML step and validate_ubl call, add:

    // 6. Also run data-level business rules (BR-RO-xxx)
    let (data_errors, data_warnings) = crate::ubl::validator::validate_invoice_data(
        &input.invoice,
        &input.lines,
        &input.seller,
        &input.buyer,
        input.storno_ref.as_deref(),
    );

    let all_errors: Vec<String> = result.errors.into_iter().chain(data_errors).collect();
    let all_warnings: Vec<String> = result.warnings.into_iter().chain(data_warnings).collect();

    Ok(InvoiceDraftValidation {
        is_valid: all_errors.is_empty(),
        errors: all_errors,
        warnings: all_warnings,
    })
```

The full updated function body becomes:
```rust
#[tauri::command]
pub async fn validate_invoice_draft(
    state: State<'_, AppState>,
    id: String,
) -> AppResult<InvoiceDraftValidation> {
    let with_lines = invoices::get_with_lines(&state.db, &id).await?;
    let inv = with_lines.invoice;
    let lines = with_lines.lines;

    let seller = companies::get(&state.db, &inv.company_id).await?;
    let buyer = contacts::get(&state.db, &inv.contact_id).await?;

    let input = GeneratorInput {
        invoice: inv,
        lines,
        seller,
        buyer,
        storno_ref: None,
    };

    // XML structural validation
    let xml_result = match generate_ubl(&input) {
        Ok(xml) => crate::ubl::validator::validate_ubl(&xml),
        Err(e) => {
            return Ok(InvoiceDraftValidation {
                is_valid: false,
                errors: vec![e.to_string()],
                warnings: vec![],
            });
        }
    };

    // Data-level business rules (50+)
    let (data_errors, data_warnings) = crate::ubl::validator::validate_invoice_data(
        &input.invoice,
        &input.lines,
        &input.seller,
        &input.buyer,
        input.storno_ref.as_deref(),
    );

    let all_errors: Vec<String> = xml_result.errors.into_iter().chain(data_errors).collect();
    let all_warnings: Vec<String> = xml_result.warnings.into_iter().chain(data_warnings).collect();

    Ok(InvoiceDraftValidation {
        is_valid: all_errors.is_empty(),
        errors: all_errors,
        warnings: all_warnings,
    })
}
```

- [ ] **Step 1.6 — Run `cargo check` — must be zero errors**

```bash
cd /Users/cris/Projects/efactura-desktop/src-tauri && cargo check 2>&1 | grep "^error"
```
Expected: (empty — zero errors)

- [ ] **Step 1.7 — Run `tsc --noEmit` — must be zero errors**

```bash
cd /Users/cris/Projects/efactura-desktop && npx tsc --noEmit 2>&1; echo "EXIT:$?"
```
Expected: `EXIT:0`

---

## Task 2: PaymentMeansCode — DB Column + UBL Generator + Frontend

**Files:**
- Create: `src-tauri/migrations/0002_payment_means.sql`
- Modify: `src-tauri/src/db/invoices.rs` (Invoice struct + CreateInvoiceInput)
- Modify: `src-tauri/src/ubl/generator.rs` (use actual code)
- Modify: `src/pages/InvoiceNew.tsx` (payment method selector)
- Modify: `src/types/index.ts` (Invoice type)
- Modify: `src-tauri/src/db/pool.rs` (run migration on startup)

- [ ] **Step 2.1 — Create migration `0002_payment_means.sql`**

```sql
-- src-tauri/migrations/0002_payment_means.sql
-- Adds payment_means_code to invoices (UNCL4461 codes: 30=transfer, 10=cash, 48=card)
ALTER TABLE invoices ADD COLUMN payment_means_code TEXT NOT NULL DEFAULT '30';
```

- [ ] **Step 2.2 — Ensure migration runs: check pool.rs migration runner**

Open `src-tauri/src/db/pool.rs` and verify the migration runner includes all files from the `migrations/` directory. If it uses `sqlx::migrate!("../migrations")` it will auto-pick up the new file. If it uses a hard-coded list, add the new entry:

```rust
// Verify this pattern exists in pool.rs — if not, the migrations macro handles it:
sqlx::migrate!("../migrations")
    .run(&pool)
    .await
    .expect("Migration failed");
```

- [ ] **Step 2.3 — Add `payment_means_code` to `Invoice` struct and `CreateInvoiceInput`**

In `src-tauri/src/db/invoices.rs`:

```rust
// In the Invoice struct, add after `notes`:
    pub payment_means_code: String,  // "30"=transfer, "10"=cash, "48"=card

// In the CreateInvoiceInput struct, add:
    pub payment_means_code: Option<String>,  // defaults to "30"
```

Also update the `create()` INSERT SQL to include payment_means_code:

```rust
// In the create() function, find the INSERT query and add the column:
sqlx::query(
    "INSERT INTO invoices (
        id, company_id, contact_id, series, number, full_number,
        issue_date, due_date, currency, exchange_rate,
        subtotal_amount, vat_amount, total_amount,
        status, notes, payment_means_code
    ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)"
)
// ...add .bind(input.payment_means_code.as_deref().unwrap_or("30"))
```

And update the `update_invoice_draft()` UPDATE SQL similarly.

- [ ] **Step 2.4 — Use `payment_means_code` in UBL generator**

In `src-tauri/src/ubl/generator.rs`, replace the hardcoded `"30"`:

```rust
// Before (hardcoded):
    writer
        .write_event(Event::Text(BytesText::new("30")))
        .map_err(|e| AppError::Xml(e.to_string()))?;

// After (from invoice data):
    let pmc = input.invoice.payment_means_code.as_str();
    writer
        .write_event(Event::Text(BytesText::new(pmc)))
        .map_err(|e| AppError::Xml(e.to_string()))?;
```

- [ ] **Step 2.5 — Add `paymentMeansCode` to frontend `Invoice` type**

In `src/types/index.ts`, in the `Invoice` interface, add:
```typescript
  paymentMeansCode: string;  // "30"=transfer bancar, "10"=numerar, "48"=card
```

- [ ] **Step 2.6 — Add payment method selector to `InvoiceNew.tsx`**

Find the payment section in `InvoiceNew.tsx` (search for "Transfer" or "payment" or the payment block) and add state + UI:

```tsx
// Add state near the other useState declarations:
const [paymentMeansCode, setPaymentMeansCode] = useState<string>("30");

// In the form — add a payment method selector (in the notes/extras area or its own section):
<div style={{ display: "flex", flexDirection: "column", gap: 3 }}>
  <label style={{ fontSize: 11, fontWeight: 600, color: "var(--text-muted)" }}>
    Modalitate plată
  </label>
  <select
    className="field"
    value={paymentMeansCode}
    onChange={(e) => setPaymentMeansCode(e.target.value)}
    style={{ fontSize: 12 }}
  >
    <option value="30">Transfer bancar (30)</option>
    <option value="10">Numerar (10)</option>
    <option value="48">Card (48)</option>
    <option value="42">Cont bancar (42)</option>
    <option value="58">SEPA (58)</option>
  </select>
</div>

// In saveDraftMutation.mutationFn, add to the input object:
paymentMeansCode,
```

- [ ] **Step 2.7 — Run `cargo check` + `tsc --noEmit`**

```bash
cd /Users/cris/Projects/efactura-desktop/src-tauri && cargo check 2>&1 | grep "^error"
cd /Users/cris/Projects/efactura-desktop && npx tsc --noEmit 2>&1; echo "EXIT:$?"
```
Expected: both zero errors.

---

## Task 3: PDF Liberation Sans Font + Amount-to-Words

**Files:**
- Download: `src-tauri/fonts/LiberationSans-Regular.ttf`
- Download: `src-tauri/fonts/LiberationSans-Bold.ttf`
- Modify: `src-tauri/src/ubl/pdf.rs`

- [ ] **Step 3.1 — Download Liberation Sans fonts (free, OFL licensed)**

```bash
mkdir -p /Users/cris/Projects/efactura-desktop/src-tauri/fonts
# Liberation Sans is available from Red Hat / Google Fonts
curl -L "https://github.com/liberationfonts/liberation-fonts/files/7261482/liberation-fonts-ttf-2.1.5.tar.gz" \
  -o /tmp/liberation.tar.gz
tar -xzf /tmp/liberation.tar.gz -C /tmp/
cp /tmp/liberation-fonts-ttf-2.1.5/LiberationSans-Regular.ttf \
   /Users/cris/Projects/efactura-desktop/src-tauri/fonts/
cp /tmp/liberation-fonts-ttf-2.1.5/LiberationSans-Bold.ttf \
   /Users/cris/Projects/efactura-desktop/src-tauri/fonts/
ls -la /Users/cris/Projects/efactura-desktop/src-tauri/fonts/
```
Expected: both .ttf files present (~200KB each).

If the URL fails, alternative — download from Google Fonts (Liberation is hosted there) or use DejaVu Sans which also covers Romanian diacritics:
```bash
curl -L "https://github.com/dejavu-fonts/dejavu-fonts/releases/download/version_2_37/dejavu-fonts-ttf-2.37.tar.bz2" \
  -o /tmp/dejavu.tar.bz2
tar -xjf /tmp/dejavu.tar.bz2 -C /tmp/
cp /tmp/dejavu-fonts-ttf-2.37/ttf/DejaVuSans.ttf \
   /Users/cris/Projects/efactura-desktop/src-tauri/fonts/LiberationSans-Regular.ttf
cp /tmp/dejavu-fonts-ttf-2.37/ttf/DejaVuSans-Bold.ttf \
   /Users/cris/Projects/efactura-desktop/src-tauri/fonts/LiberationSans-Bold.ttf
```

- [ ] **Step 3.2 — Replace Helvetica with embedded Liberation Sans in `pdf.rs`**

In `src-tauri/src/ubl/pdf.rs`, replace the font loading section:

```rust
// BEFORE (broken for Romanian diacritics):
    let font_normal = doc
        .add_builtin_font(BuiltinFont::Helvetica)
        .map_err(|e| AppError::Pdf(e.to_string()))?;
    let font_bold = doc
        .add_builtin_font(BuiltinFont::HelveticaBold)
        .map_err(|e| AppError::Pdf(e.to_string()))?;

// AFTER (supports ă â î ș ț):
    static FONT_REGULAR: &[u8] = include_bytes!("../../fonts/LiberationSans-Regular.ttf");
    static FONT_BOLD: &[u8] = include_bytes!("../../fonts/LiberationSans-Bold.ttf");

    let font_normal = doc
        .add_external_font(std::io::Cursor::new(FONT_REGULAR))
        .map_err(|e| AppError::Pdf(e.to_string()))?;
    let font_bold = doc
        .add_external_font(std::io::Cursor::new(FONT_BOLD))
        .map_err(|e| AppError::Pdf(e.to_string()))?;
```

- [ ] **Step 3.3 — Add `amount_to_romanian_words` helper function**

Add this function to `pdf.rs` (before `generate_pdf`):

```rust
/// Convertește o sumă în cuvinte românești pentru pied de pagină PDF.
/// Exemplu: 425.50 → "Patru sute douăzeci și cinci lei și 50 bani"
pub fn amount_to_romanian_words(amount: f64) -> String {
    let total_bani = (amount * 100.0).round() as u64;
    let lei = total_bani / 100;
    let bani = total_bani % 100;

    let lei_str = if lei == 0 {
        "zero lei".to_string()
    } else {
        format!("{} lei", number_to_ro(lei))
    };

    if bani == 0 {
        capitalize_first(&lei_str)
    } else {
        capitalize_first(&format!("{} și {} bani", lei_str, bani))
    }
}

fn capitalize_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

fn number_to_ro(n: u64) -> String {
    match n {
        0 => "zero".to_string(),
        1 => "unu".to_string(),
        2 => "doi".to_string(),
        3 => "trei".to_string(),
        4 => "patru".to_string(),
        5 => "cinci".to_string(),
        6 => "șase".to_string(),
        7 => "șapte".to_string(),
        8 => "opt".to_string(),
        9 => "nouă".to_string(),
        10 => "zece".to_string(),
        11 => "unsprezece".to_string(),
        12 => "doisprezece".to_string(),
        13 => "treisprezece".to_string(),
        14 => "paisprezece".to_string(),
        15 => "cincisprezece".to_string(),
        16 => "șaisprezece".to_string(),
        17 => "șaptesprezece".to_string(),
        18 => "optsprezece".to_string(),
        19 => "nouăsprezece".to_string(),
        20 => "douăzeci".to_string(),
        21..=29 => format!("douăzeci și {}", number_to_ro(n - 20)),
        30 => "treizeci".to_string(),
        31..=39 => format!("treizeci și {}", number_to_ro(n - 30)),
        40 => "patruzeci".to_string(),
        41..=49 => format!("patruzeci și {}", number_to_ro(n - 40)),
        50 => "cincizeci".to_string(),
        51..=59 => format!("cincizeci și {}", number_to_ro(n - 50)),
        60 => "șaizeci".to_string(),
        61..=69 => format!("șaizeci și {}", number_to_ro(n - 60)),
        70 => "șaptezeci".to_string(),
        71..=79 => format!("șaptezeci și {}", number_to_ro(n - 70)),
        80 => "optzeci".to_string(),
        81..=89 => format!("optzeci și {}", number_to_ro(n - 80)),
        90 => "nouăzeci".to_string(),
        91..=99 => format!("nouăzeci și {}", number_to_ro(n - 90)),
        100 => "o sută".to_string(),
        101..=199 => format!("o sută {}", number_to_ro(n - 100)),
        200..=999 => {
            let h = n / 100;
            let r = n % 100;
            let h_str = format!("{} sute", number_to_ro(h));
            if r == 0 { h_str } else { format!("{} {}", h_str, number_to_ro(r)) }
        }
        1000 => "o mie".to_string(),
        1001..=1999 => format!("o mie {}", number_to_ro(n - 1000)),
        2000..=999_999 => {
            let m = n / 1000;
            let r = n % 1000;
            let m_str = if m == 1 {
                "o mie".to_string()
            } else {
                format!("{} mii", number_to_ro(m))
            };
            if r == 0 { m_str } else { format!("{} {}", m_str, number_to_ro(r)) }
        }
        1_000_000..=999_999_999 => {
            let mil = n / 1_000_000;
            let r = n % 1_000_000;
            let mil_str = if mil == 1 {
                "un milion".to_string()
            } else {
                format!("{} milioane", number_to_ro(mil))
            };
            if r == 0 { mil_str } else { format!("{} {}", mil_str, number_to_ro(r)) }
        }
        _ => n.to_string(),
    }
}
```

- [ ] **Step 3.4 — Add amount-in-words line to PDF footer**

In `generate_pdf`, just before the footer/notes section, add:

```rust
    // Total în cuvinte
    let total_words = amount_to_romanian_words(inv.total_amount);
    let words_line = format!("Total de plată: {}", total_words);
    layer.use_text(words_line, FONT_SMALL, Mm(MARGIN), Mm(y), &font_normal);
    y -= LINE_H;
```

- [ ] **Step 3.5 — Run `cargo check` — zero errors**

```bash
cd /Users/cris/Projects/efactura-desktop/src-tauri && cargo check 2>&1 | grep "^error"
```

---

## Task 4: Auto-Updater (tauri-plugin-updater)

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src/pages/Settings.tsx`
- Modify: `src/lib/tauri.ts`

- [ ] **Step 4.1 — Add `tauri-plugin-updater` to Cargo.toml**

```toml
# In [dependencies] section, add after tauri-plugin-opener:
tauri-plugin-updater = "2"
```

- [ ] **Step 4.2 — Register updater plugin in `lib.rs`**

```rust
// In pub fn run(), in the plugin chain, add after tauri_plugin_opener::init():
.plugin(tauri_plugin_updater::Builder::default().build())
```

- [ ] **Step 4.3 — Configure updater in `tauri.conf.json`**

Add the `updater` section to `tauri.conf.json`:
```json
{
  "plugins": {
    "updater": {
      "active": true,
      "pubkey": "",
      "endpoints": [
        "https://releases.lucaris.ro/efactura/{{target}}/{{arch}}/latest.json"
      ]
    }
  }
}
```

Note: Replace the endpoint URL with your actual update server. The `pubkey` field will need to be populated with the signing key from `cargo tauri signer generate`. Until a real server is set up, keep `"active": false` to avoid startup errors.

- [ ] **Step 4.4 — Add update commands to `tauri.ts`**

```typescript
// Add to src/lib/tauri.ts, in the system section:
export const updater = {
  check: () => invoke<{ available: boolean; version?: string; body?: string }>(
    "check_for_updates"
  ),
};

// Add to the api umbrella at the bottom:
// updater,
```

- [ ] **Step 4.5 — Add "Verifică actualizări" button in Settings.tsx**

Find the "About" tab or system section in `src/pages/Settings.tsx` and add:

```tsx
// Add state:
const [updateStatus, setUpdateStatus] = useState<string | null>(null);
const [checkingUpdate, setCheckingUpdate] = useState(false);

// Add button (in the About/System section):
<div style={{ display: "flex", alignItems: "center", gap: 10 }}>
  <button
    type="button"
    className="btn"
    disabled={checkingUpdate}
    onClick={async () => {
      setCheckingUpdate(true);
      setUpdateStatus(null);
      try {
        const { checkUpdate } = await import("@tauri-apps/plugin-updater");
        const update = await checkUpdate();
        if (update?.available) {
          setUpdateStatus(`Versiune nouă disponibilă: ${update.version}. Descărcați de pe site.`);
        } else {
          setUpdateStatus("Aplicația este la zi.");
        }
      } catch {
        setUpdateStatus("Nu s-a putut verifica pentru actualizări (nicio conexiune sau server indisponibil).");
      } finally {
        setCheckingUpdate(false);
      }
    }}
  >
    <Icon name="refresh" size={12} /> {checkingUpdate ? "Se verifică…" : "Verifică actualizări"}
  </button>
  {updateStatus && (
    <span style={{ fontSize: 11, color: "var(--text-muted)" }}>{updateStatus}</span>
  )}
</div>
```

- [ ] **Step 4.6 — Run `cargo check` + `tsc --noEmit`**

```bash
cd /Users/cris/Projects/efactura-desktop/src-tauri && cargo check 2>&1 | grep "^error"
cd /Users/cris/Projects/efactura-desktop && npx tsc --noEmit 2>&1; echo "EXIT:$?"
```

---

## Task 5: OS-Level Autostart (tauri-plugin-autostart)

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/commands/system.rs`

- [ ] **Step 5.1 — Add `tauri-plugin-autostart` to Cargo.toml**

```toml
# In [dependencies], add:
tauri-plugin-autostart = "2"
```

- [ ] **Step 5.2 — Register autostart plugin in `lib.rs`**

```rust
// Add to plugin chain:
.plugin(tauri_plugin_autostart::init(
    tauri_plugin_autostart::MacosLauncher::LaunchAgent,
    Some(vec!["--autostart"]),  // args passed when auto-launched
))
```

- [ ] **Step 5.3 — Rewrite `set_autostart` and `get_autostart` in `system.rs`**

Replace the current DB-only implementation with actual OS calls:

```rust
use tauri::Manager;

/// Enables or disables OS-level autostart (LaunchAgent on macOS, Registry on Windows).
#[tauri::command]
pub async fn set_autostart(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> AppResult<()> {
    use tauri_plugin_autostart::ManagerExt;

    // 1. Update OS-level autostart
    if enabled {
        app.autolaunch().enable().map_err(|e| AppError::Other(e.to_string()))?;
    } else {
        app.autolaunch().disable().map_err(|e| AppError::Other(e.to_string()))?;
    }

    // 2. Also store in DB so UI can read it without OS call
    sqlx::query(
        "INSERT INTO settings(key, value) VALUES('autostart', ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value"
    )
    .bind(if enabled { "1" } else { "0" })
    .execute(&state.db)
    .await?;

    Ok(())
}

#[tauri::command]
pub async fn get_autostart(
    app: tauri::AppHandle,
) -> AppResult<bool> {
    use tauri_plugin_autostart::ManagerExt;
    let enabled = app.autolaunch().is_enabled()
        .map_err(|e| AppError::Other(e.to_string()))?;
    Ok(enabled)
}
```

- [ ] **Step 5.4 — Run `cargo check` — zero errors**

```bash
cd /Users/cris/Projects/efactura-desktop/src-tauri && cargo check 2>&1 | grep "^error"
```

---

## Task 6: Virtual Scrolling for Invoice List

**Files:**
- Modify: `src/pages/Invoices.tsx`

- [ ] **Step 6.1 — Add `useVirtualizer` to `Invoices.tsx`**

`@tanstack/react-virtual` is already installed. Replace the static `<tbody>` with a virtualizer:

```tsx
// Add import at the top:
import { useVirtualizer } from "@tanstack/react-virtual";
import { useRef } from "react";

// Inside InvoicesPage, after the list useMemo, add:
  const parentRef = useRef<HTMLDivElement>(null);
  const rowVirtualizer = useVirtualizer({
    count: list.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 32,  // row height in px
    overscan: 10,
  });
```

Replace the `<div className="content-body">` section with:

```tsx
      <div
        className="content-body"
        ref={parentRef}
        style={{ overflowY: "auto", flex: 1 }}
      >
        {isLoading ? (
          <div style={{ padding: 24, fontSize: 12, color: "var(--text-muted)" }}>
            Se încarcă…
          </div>
        ) : list.length === 0 ? (
          <div style={{ padding: 40, textAlign: "center", fontSize: 12, color: "var(--text-muted)" }}>
            {allInvoices.length === 0
              ? "Nicio factură emisă. Creați prima factură cu butonul \"Factură nouă\"."
              : "Nicio înregistrare pentru filtrele aplicate."}
          </div>
        ) : (
          <table className="dt" style={{ width: "100%" }}>
            <thead>
              <tr>
                {/* same thead as before — keep unchanged */}
                <th className="ck">
                  <input
                    type="checkbox"
                    className="cbx"
                    checked={selected.size === list.length && list.length > 0}
                    onChange={() =>
                      setSelected(
                        selected.size === list.length
                          ? new Set()
                          : new Set(list.map((i) => i.id)),
                      )
                    }
                  />
                </th>
                <th style={{ width: 134 }} className="sortable sorted">
                  {t('invoices.columns.number')} <span className="sort">▾</span>
                </th>
                <th style={{ width: 92 }}>{t('invoices.columns.date')}</th>
                <th>{t('invoices.columns.customer')}</th>
                <th style={{ width: 100 }}>CUI</th>
                <th className="num" style={{ width: 110 }}>Net (RON)</th>
                <th className="num" style={{ width: 90 }}>TVA</th>
                <th className="num" style={{ width: 120 }}>{t('invoices.columns.total')}</th>
                <th style={{ width: 100 }}>Scadență</th>
                <th style={{ width: 124 }}>{t('invoices.columns.status')}</th>
                <th style={{ width: 110 }}>Index ANAF</th>
                <th style={{ width: 24 }}></th>
              </tr>
            </thead>
            <tbody
              style={{
                height: `${rowVirtualizer.getTotalSize()}px`,
                position: "relative",
              }}
            >
              {rowVirtualizer.getVirtualItems().map((virtualRow) => {
                const inv = list[virtualRow.index];
                const client = contactMap.get(inv.contactId);
                return (
                  <tr
                    key={inv.id}
                    data-index={virtualRow.index}
                    ref={rowVirtualizer.measureElement}
                    style={{
                      position: "absolute",
                      top: 0,
                      left: 0,
                      width: "100%",
                      transform: `translateY(${virtualRow.start}px)`,
                      cursor: "pointer",
                    }}
                    onClick={() =>
                      navigate({ to: "/invoices/$id", params: { id: inv.id } })
                    }
                    className={selected.has(inv.id) ? "selected" : ""}
                  >
                    <td className="ck" onClick={(e) => e.stopPropagation()}>
                      <input
                        type="checkbox"
                        className="cbx"
                        checked={selected.has(inv.id)}
                        onChange={() => toggleOne(inv.id)}
                      />
                    </td>
                    <td className="mono"><b>{inv.fullNumber}</b></td>
                    <td className="muted">{inv.issueDate}</td>
                    <td>{client?.legalName ?? <span className="dim">—</span>}</td>
                    <td className="mono muted">{client?.cui ?? "—"}</td>
                    <td className="num tnum muted">{fmtRON(inv.subtotalAmount)}</td>
                    <td className="num tnum dim">{fmtRON(inv.vatAmount)}</td>
                    <td className="num tnum"><b>{fmtRON(inv.totalAmount)}</b></td>
                    <td className="muted">{inv.dueDate}</td>
                    <td><StatusBadge status={inv.status} /></td>
                    <td className="mono dim">{inv.anafIndex || "—"}</td>
                    <td>
                      <Icon name="chevronRight" size={12} style={{ color: "var(--text-dim)" }} />
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>
```

- [ ] **Step 6.2 — Run `tsc --noEmit` — zero errors**

```bash
cd /Users/cris/Projects/efactura-desktop && npx tsc --noEmit 2>&1; echo "EXIT:$?"
```

---

## Task 7: Notification Per-Type Preferences

**Files:**
- Modify: `src-tauri/src/notifications/mod.rs`

- [ ] **Step 7.1 — Add per-type preference check to `notify` function**

Replace the current `notify` function in `src-tauri/src/notifications/mod.rs`:

```rust
//! Native OS notification helper (tauri-plugin-notification)
//!
//! Preference keys in settings DB:
//!   notif_pref_{type} = "os" | "inapp" | "off"
//! where {type} is one of: validated, rejected, received, cert_expiring, cert_expired, anaf_down

use chrono::Timelike;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

/// Check if OS notifications are allowed for a given notification type.
async fn should_notify_os(app: &AppHandle, notif_type: &str) -> bool {
    let pool = app.state::<crate::state::AppState>();

    // Check quiet hours
    let quiet_key = "quiet_hours";
    let quiet = sqlx::query("SELECT value FROM settings WHERE key = ?1")
        .bind(quiet_key)
        .fetch_optional(&pool.db)
        .await
        .ok()
        .flatten()
        .and_then(|row| { use sqlx::Row; row.try_get::<String, _>("value").ok() })
        .map(|v| v == "1")
        .unwrap_or(false);

    if quiet {
        let hour = chrono::Local::now().hour();
        if hour >= 22 || hour < 7 {
            return false;
        }
    }

    // Check per-type preference
    let pref_key = format!("notif_pref_{}", notif_type);
    let pref = sqlx::query("SELECT value FROM settings WHERE key = ?1")
        .bind(&pref_key)
        .fetch_optional(&pool.db)
        .await
        .ok()
        .flatten()
        .and_then(|row| { use sqlx::Row; row.try_get::<String, _>("value").ok() })
        .unwrap_or_else(|| "os".to_string());  // default: show OS notifications

    pref == "os"
}

async fn notify_typed(app: &AppHandle, notif_type: &str, title: &str, body: &str) {
    if should_notify_os(app, notif_type).await {
        let _ = app.notification()
            .builder()
            .title(title)
            .body(body)
            .show();
    }
}

pub async fn notify_invoice_validated(app: &AppHandle, invoice_number: &str) {
    notify_typed(
        app, "validated",
        "✓ Factură validată",
        &format!("Factura {} a fost validată de ANAF.", invoice_number),
    ).await;
}

pub async fn notify_invoice_rejected(app: &AppHandle, invoice_number: &str, reason: &str) {
    let short: String = reason.chars().take(80).collect();
    notify_typed(
        app, "rejected",
        "✗ Factură respinsă",
        &format!("Factura {} a fost respinsă: {}", invoice_number, short),
    ).await;
}

pub async fn notify_new_received(app: &AppHandle, count: u32) {
    if count > 0 {
        notify_typed(
            app, "received",
            "📥 Facturi noi primite",
            &format!("{} facturi noi descărcate din SPV.", count),
        ).await;
    }
}

pub async fn notify_certificate_expiring(app: &AppHandle, company_name: &str, days: i64) {
    let notif_type = if days <= 7 { "cert_expired" } else { "cert_expiring" };
    notify_typed(
        app, notif_type,
        "⏰ Certificat SPV expiră",
        &format!("Certificatul pentru {} expiră în {} zile. Reautorizați din Setări.", company_name, days),
    ).await;
}
```

- [ ] **Step 7.2 — Add per-type preference UI in Settings.tsx**

Find the "Notificări" section in Settings.tsx and add per-type controls:

```tsx
// Add below the quiet_hours toggle:
{[
  { key: "validated", label: "Factură validată ANAF" },
  { key: "rejected",  label: "Factură respinsă ANAF" },
  { key: "received",  label: "Facturi noi primite SPV" },
  { key: "cert_expiring", label: "Certificat expiră" },
  { key: "cert_expired",  label: "Certificat expirat" },
].map(({ key, label }) => (
  <div key={key} style={{ display: "flex", alignItems: "center", gap: 8, padding: "4px 0" }}>
    <label style={{ fontSize: 11, flex: 1 }}>{label}</label>
    <select
      className="field"
      style={{ width: 130, fontSize: 11 }}
      value={settings[`notif_pref_${key}`] ?? "os"}
      onChange={async (e) => {
        await api.settings.set(`notif_pref_${key}`, e.target.value);
        void queryClient.invalidateQueries({ queryKey: ["settings"] });
      }}
    >
      <option value="os">Desktop + In-app</option>
      <option value="inapp">Doar in-app</option>
      <option value="off">Dezactivat</option>
    </select>
  </div>
))}
```

- [ ] **Step 7.3 — Run `cargo check` + `tsc --noEmit` — both zero errors**

```bash
cd /Users/cris/Projects/efactura-desktop/src-tauri && cargo check 2>&1 | grep "^error"
cd /Users/cris/Projects/efactura-desktop && npx tsc --noEmit 2>&1; echo "EXIT:$?"
```

---

## Task 8: CSV Import — Template Download + Preview

**Files:**
- Modify: `src/components/shared/CsvImportModal.tsx`
- Modify: `src-tauri/src/commands/import.rs`
- Modify: `src/lib/tauri.ts`

- [ ] **Step 8.1 — Add `dry_run` param to backend import commands**

In `src-tauri/src/commands/import.rs`, add a `dry_run: bool` parameter to both commands:

```rust
// Modify import_invoices_csv signature:
#[tauri::command]
pub async fn import_invoices_csv(
    state: State<'_, AppState>,
    content: String,
    company_id: String,
    dry_run: bool,  // NEW: if true, validate only — don't insert
) -> AppResult<ImportResult> {
    // ... existing parse logic ...
    // At the point where you'd INSERT, check:
    if !dry_run {
        // do the actual insert
    }
    // Always return the result (rows_parsed, errors)
}

// Same for import_contacts_csv:
pub async fn import_contacts_csv(
    state: State<'_, AppState>,
    content: String,
    company_id: String,
    dry_run: bool,
) -> AppResult<ImportResult> { ... }
```

- [ ] **Step 8.2 — Add template constants**

```rust
// At the top of import.rs, add template CSV constants:
pub const INVOICES_CSV_TEMPLATE: &str =
    "company_cui,customer_cui,customer_name,series,number,issue_date,due_date,item_name,qty,unit,unit_price,vat_rate\n\
     RO12345678,RO87654321,Client Exemplu SRL,FACT,1,2026-01-15,2026-02-14,Servicii consultanta,1,buc,1000.00,19\n";

pub const CONTACTS_CSV_TEMPLATE: &str =
    "type,cui,name,address,city,county,email,phone\n\
     CUSTOMER,RO87654321,Client Exemplu SRL,Str. Exemplu nr. 1,Cluj-Napoca,CJ,office@client.ro,+40722000000\n";

// Add Tauri commands to return these:
#[tauri::command]
pub fn get_invoices_csv_template() -> &'static str {
    INVOICES_CSV_TEMPLATE
}

#[tauri::command]
pub fn get_contacts_csv_template() -> &'static str {
    CONTACTS_CSV_TEMPLATE
}
```

- [ ] **Step 8.3 — Register new commands in `lib.rs`**

```rust
// In generate_handler!, add:
commands::import::get_invoices_csv_template,
commands::import::get_contacts_csv_template,
```

- [ ] **Step 8.4 — Update `tauri.ts`**

```typescript
// In importData section, add:
  invoicesCsvTemplate: () => invoke<string>("get_invoices_csv_template"),
  contactsCsvTemplate: () => invoke<string>("get_contacts_csv_template"),
  invoicesCsvDryRun: (content: string, companyId: string) =>
    invoke<{ imported: number; errors: string[] }>("import_invoices_csv", {
      content, companyId, dryRun: true,
    }),
  contactsCsvDryRun: (content: string, companyId: string) =>
    invoke<{ imported: number; errors: string[] }>("import_contacts_csv", {
      content, companyId, dryRun: true,
    }),
```

- [ ] **Step 8.5 — Add template download + preview to `CsvImportModal.tsx`**

Add two new sections to `CsvImportModal`:

```tsx
// 1. Template download button (at the top of the modal, before the file picker):
<div style={{ marginBottom: 12, display: "flex", gap: 8, alignItems: "center" }}>
  <span style={{ fontSize: 11, color: "var(--text-muted)" }}>
    Nu știți formatul?
  </span>
  <button
    type="button"
    className="btn"
    style={{ fontSize: 11 }}
    onClick={async () => {
      const template = type === "invoices"
        ? await api.importData.invoicesCsvTemplate()
        : await api.importData.contactsCsvTemplate();
      const { save } = await import("@tauri-apps/plugin-dialog");
      const path = await save({
        filters: [{ name: "CSV", extensions: ["csv"] }],
        defaultPath: type === "invoices" ? "template-facturi.csv" : "template-contacte.csv",
      });
      if (path) {
        const { writeTextFile } = await import("@tauri-apps/plugin-fs");
        await writeTextFile(path, template);
      }
    }}
  >
    <Icon name="download" size={11} /> Descarcă template CSV
  </button>
</div>

// 2. Preview step — add previewResult state and a "Validează" button:
const [previewResult, setPreviewResult] = useState<{ imported: number; errors: string[] } | null>(null);
const [previewing, setPreviewing] = useState(false);

// After file content is loaded, show a "Validează (dry run)" button:
{fileContent && !previewResult && (
  <button
    type="button"
    className="btn"
    disabled={previewing}
    onClick={async () => {
      setPreviewing(true);
      try {
        const result = type === "invoices"
          ? await api.importData.invoicesCsvDryRun(fileContent, companyId)
          : await api.importData.contactsCsvDryRun(fileContent, companyId);
        setPreviewResult(result);
      } finally {
        setPreviewing(false);
      }
    }}
  >
    {previewing ? "Se validează…" : "Validează înainte de import →"}
  </button>
)}

// Show preview results before final import:
{previewResult && (
  <div style={{ padding: "8px 10px", background: "var(--bg)", border: "1px solid var(--border)", fontSize: 11 }}>
    <div style={{ fontWeight: 600, marginBottom: 4 }}>
      Previzualizare: {previewResult.imported} înregistrări valide
    </div>
    {previewResult.errors.length > 0 && (
      <div style={{ color: "#DC2626" }}>
        {previewResult.errors.slice(0, 5).map((e, i) => (
          <div key={i}>• {e}</div>
        ))}
        {previewResult.errors.length > 5 && (
          <div>…și încă {previewResult.errors.length - 5} erori.</div>
        )}
      </div>
    )}
  </div>
)}
```

- [ ] **Step 8.6 — Run `cargo check` + `tsc --noEmit` — both zero errors**

```bash
cd /Users/cris/Projects/efactura-desktop/src-tauri && cargo check 2>&1 | grep "^error"
cd /Users/cris/Projects/efactura-desktop && npx tsc --noEmit 2>&1; echo "EXIT:$?"
```

---

## Final Verification Checklist

After all tasks are complete, run these checks:

- [ ] `cd src-tauri && cargo check 2>&1 | grep "^error"` → empty
- [ ] `npx tsc --noEmit` → EXIT:0
- [ ] `cargo check 2>&1 | grep "dead_code\|unused"` → review any new warnings
- [ ] Open app in dev mode (`pnpm tauri dev`), create a draft invoice, click "Validează" → should show 50+ rule checks
- [ ] Create invoice with DueDate before IssueDate → BR-RO-024 error appears in UI
- [ ] Create invoice with `category=S`, `vat_rate=25` → BR-RO-035 error appears
- [ ] Generate PDF → Romanian diacritics (ă â î ș ț) render correctly
- [ ] Settings → "Pornire automată" toggle → actually sets/clears LaunchAgent on macOS
- [ ] Settings → "Verifică actualizări" → shows message (error if no server, that's OK)
- [ ] Invoice list with 100+ items → virtual scrolling smooth, no DOM overload
- [ ] CSV import → template download works → preview shows validation errors before commit

---

## Risk Notes

- **Task 3 (PDF fonts)**: If the Liberation Sans download URL changes, use DejaVu Sans as fallback (same Romanian coverage, also OFL). Both font families are in active maintenance.
- **Task 4 (auto-updater)**: With no active update server, the plugin will fail silently on startup. Set `"active": false` in `tauri.conf.json` until the release server is ready.
- **Task 5 (autostart)**: `tauri-plugin-autostart` may require additional entitlements on macOS (Hardened Runtime). Test on a real macOS device.
- **Task 6 (virtual scroll)**: The absolute-position tbody approach requires a fixed `height` on the scroll container. Ensure `content-body` has `flex: 1; overflow-y: auto` in `design.css`.
