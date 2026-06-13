//! Validare de bază a XML-ului UBL generat (reguli de business, nu XML Schema).

use quick_xml::events::Event;
use quick_xml::Reader;
use serde::Serialize;

use crate::db::companies::Company;
use crate::db::contacts::Contact;
use crate::db::invoices::{Invoice, LineItem};

const CIUS_RO_ID: &str = "urn:cen.eu:en16931:2017#compliant#urn:efactura.mfinante.ro:CIUS-RO:1.0.1";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Validează un XML UBL: parsare + verificări de business CIUS-RO.
pub fn validate_ubl(xml: &str) -> ValidationResult {
    let mut errors: Vec<String> = Vec::new();
    let warnings: Vec<String> = Vec::new();

    // ── Colectăm elementele prezente ─────────────────────────────────────────
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut has_customization_id = false;
    let mut has_correct_cius = false;
    let mut has_id = false;
    let mut has_issue_date = false;
    let mut has_due_date = false;
    let mut invoice_line_count = 0_usize;

    let mut current_tag: Option<String> = None;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name_bytes = e.name();
                let local = local_name(name_bytes.as_ref()).to_string();
                if local == "InvoiceLine" {
                    invoice_line_count += 1;
                }
                current_tag = Some(local);
            }
            Ok(Event::Text(ref e)) => {
                if let Some(ref tag) = current_tag {
                    let text = e.unescape().unwrap_or_default();
                    let text = text.trim();
                    match tag.as_str() {
                        "CustomizationID" => {
                            has_customization_id = true;
                            if text == CIUS_RO_ID {
                                has_correct_cius = true;
                            }
                        }
                        "ID" if !has_id => {
                            // Primul ID e al facturii
                            has_id = !text.is_empty();
                        }
                        "IssueDate" => {
                            has_issue_date = !text.is_empty();
                        }
                        "DueDate" => {
                            has_due_date = !text.is_empty();
                        }
                        _ => {}
                    }
                }
            }
            Ok(Event::End(_)) => {
                current_tag = None;
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                errors.push(format!("XML parse error: {e}"));
                break;
            }
            _ => {}
        }
        buf.clear();
    }

    // ── Reguli de business ───────────────────────────────────────────────────
    if !has_customization_id {
        errors.push("Lipseşte elementul CustomizationID".to_string());
    } else if !has_correct_cius {
        errors.push(format!(
            "CustomizationID nu corespunde CIUS-RO. Valoarea aşteptată: {CIUS_RO_ID}"
        ));
    }

    if !has_id {
        errors.push("Lipseşte elementul ID (numărul facturii)".to_string());
    }
    if !has_issue_date {
        errors.push("Lipseşte elementul IssueDate".to_string());
    }
    if !has_due_date {
        errors.push("Lipseşte elementul DueDate".to_string());
    }
    if invoice_line_count == 0 {
        errors.push("Factura nu conţine nicio linie (InvoiceLine)".to_string());
    }

    ValidationResult {
        valid: errors.is_empty(),
        errors,
        warnings,
    }
}

// ─── Business-rule data validation (50+ CIUS-RO rules) ───────────────────────

/// Validează datele facturii conform tuturor regulilor CIUS-RO (50+).
/// Deleghează la `rocius_rules::run_all` pentru reguli complete cu Decimal.
/// Returnează (errors, warnings) — erori blocante + avertismente non-blocante.
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

// ─── Helper ───────────────────────────────────────────────────────────────────

/// Extrage local name dintr-un QName (elimină prefixul `ns:`).
fn local_name(name: &[u8]) -> &str {
    let s = std::str::from_utf8(name).unwrap_or("");
    if let Some(pos) = s.rfind(':') {
        &s[pos + 1..]
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::{validate_ubl, CIUS_RO_ID};

    /// Build a minimal CIUS-RO invoice XML, optionally dropping a required element.
    fn invoice_xml(customization: &str, id: &str, issue: &str, due: &str, lines: usize) -> String {
        let mut s = String::from("<Invoice>");
        if !customization.is_empty() {
            s.push_str(&format!(
                "<CustomizationID>{customization}</CustomizationID>"
            ));
        }
        if !id.is_empty() {
            s.push_str(&format!("<ID>{id}</ID>"));
        }
        if !issue.is_empty() {
            s.push_str(&format!("<IssueDate>{issue}</IssueDate>"));
        }
        if !due.is_empty() {
            s.push_str(&format!("<DueDate>{due}</DueDate>"));
        }
        for i in 0..lines {
            s.push_str(&format!("<InvoiceLine><ID>{i}</ID></InvoiceLine>"));
        }
        s.push_str("</Invoice>");
        s
    }

    #[test]
    fn valid_minimal_invoice_passes() {
        let xml = invoice_xml(CIUS_RO_ID, "FCT-1", "2026-01-01", "2026-01-31", 1);
        let r = validate_ubl(&xml);
        assert!(r.valid, "expected valid, errors: {:?}", r.errors);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn missing_customization_id_is_error() {
        let xml = invoice_xml("", "FCT-1", "2026-01-01", "2026-01-31", 1);
        let r = validate_ubl(&xml);
        assert!(!r.valid);
        assert!(r.errors.iter().any(|e| e.contains("CustomizationID")));
    }

    #[test]
    fn wrong_cius_id_is_error() {
        let xml = invoice_xml("urn:wrong:cius", "FCT-1", "2026-01-01", "2026-01-31", 1);
        let r = validate_ubl(&xml);
        assert!(!r.valid);
        assert!(r.errors.iter().any(|e| e.contains("CIUS-RO")));
    }

    #[test]
    fn missing_issue_date_is_error() {
        let xml = invoice_xml(CIUS_RO_ID, "FCT-1", "", "2026-01-31", 1);
        let r = validate_ubl(&xml);
        assert!(!r.valid);
        assert!(r.errors.iter().any(|e| e.contains("IssueDate")));
    }

    #[test]
    fn zero_invoice_lines_is_error() {
        let xml = invoice_xml(CIUS_RO_ID, "FCT-1", "2026-01-01", "2026-01-31", 0);
        let r = validate_ubl(&xml);
        assert!(!r.valid);
        assert!(r.errors.iter().any(|e| e.contains("InvoiceLine")));
    }

    #[test]
    fn malformed_xml_is_caught_not_panicked() {
        let r = validate_ubl("<Invoice><ID>unclosed");
        // Must not panic; either a parse error or the missing-element errors — never valid.
        assert!(!r.valid);
        assert!(!r.errors.is_empty());
    }
}
