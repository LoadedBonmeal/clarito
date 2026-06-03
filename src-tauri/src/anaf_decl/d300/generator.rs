//! D300 v12 XML generator.
//!
//! Emits a schema-conformant `<declaratie300>` element with all data as XML
//! attributes. The element is self-closed (no child elements — the v12 schema
//! uses `xs:anyType` restriction with only `xs:attribute` children).
//!
//! Uses `quick_xml::Writer` + `BytesStart::push_attribute` directly (rather than
//! the text-element helpers in `anaf_decl::xml`) because D300 is purely
//! attribute-based.

use std::io::Cursor;

use quick_xml::events::{BytesDecl, BytesStart, Event};
use quick_xml::Writer;

use crate::anaf_decl::version::SchemaVersion;
use crate::error::{AppError, AppResult};

use super::rows::D300Rows;

fn map_err(e: quick_xml::Error) -> AppError {
    AppError::Other(format!("XML write error: {e}"))
}

/// Generate a schema-valid D300 v12 XML string from pre-computed `D300Rows`.
///
/// The output is:
/// ```xml
/// <?xml version="1.0" encoding="UTF-8"?>
/// <declaratie300 xmlns="mfp:anaf:dgti:d300:declaratie:v12"
///   luna="9" an="2025" ... />
/// ```
pub fn generate_d300_xml(rows: &D300Rows, ver: &SchemaVersion) -> AppResult<String> {
    let mut w = Writer::new(Cursor::new(Vec::<u8>::new()));

    // <?xml version="1.0" encoding="UTF-8"?>
    w.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(map_err)?;

    let mut elem = BytesStart::new(ver.root_element);

    // ── Namespace ─────────────────────────────────────────────────────────────
    elem.push_attribute(("xmlns", ver.namespace));

    // ── Required header attributes (XSD use="required") ──────────────────────
    elem.push_attribute(("luna", rows.luna.to_string().as_str()));
    elem.push_attribute(("an", rows.an.to_string().as_str()));
    elem.push_attribute((
        "depusReprezentant",
        rows.depus_reprezentant.to_string().as_str(),
    ));
    elem.push_attribute(("bifa_interne", rows.bifa_interne.to_string().as_str()));
    elem.push_attribute(("temei", rows.temei.to_string().as_str()));
    elem.push_attribute(("nume_declar", rows.nume_declar.as_str()));
    elem.push_attribute(("prenume_declar", rows.prenume_declar.as_str()));
    elem.push_attribute(("functie_declar", rows.functie_declar.as_str()));
    elem.push_attribute(("cui", rows.cui.as_str()));
    elem.push_attribute(("den", rows.den.as_str()));
    elem.push_attribute(("adresa", rows.adresa.as_str()));
    elem.push_attribute(("banca", rows.banca.as_str()));
    elem.push_attribute(("cont", rows.cont.as_str()));
    elem.push_attribute(("caen", rows.caen.as_str()));
    elem.push_attribute(("tip_decont", rows.tip_decont.as_str()));
    // pro_rata: DblGen3_2 pattern \d{0,3}(\.\d{0,2})? — format to at most 2 dp
    let pro_rata_str = format!("{:.2}", rows.pro_rata);
    // Trim trailing zeros per pattern but keep at least the integer part
    let pro_rata_str = pro_rata_str
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string();
    let pro_rata_str = if pro_rata_str.is_empty() {
        "0".to_string()
    } else {
        pro_rata_str
    };
    elem.push_attribute(("pro_rata", pro_rata_str.as_str()));
    elem.push_attribute(("bifa_cereale", rows.bifa_cereale.as_str()));
    elem.push_attribute(("bifa_mob", rows.bifa_mob.as_str()));
    elem.push_attribute(("bifa_disp", rows.bifa_disp.as_str()));
    elem.push_attribute(("bifa_cons", rows.bifa_cons.as_str()));
    elem.push_attribute(("solicit_ramb", rows.solicit_ramb.as_str()));
    // nr_evid is a MANDATORY D300 attribute (DUK: omitting it → "atributul trebuie
    // sa existe"), so always emit it. A DUK-clean value must be a valid 23-char NDP
    // (număr de evidență a plății, with embedded period + check digit); generating/
    // validating that NDP is a deferred item, so the user-supplied value is emitted
    // verbatim and an invalid/placeholder one trips DUK rule R25 at validation time.
    let nr_evid_trimmed = rows.nr_evid.trim();
    elem.push_attribute((
        "nr_evid",
        if nr_evid_trimmed.is_empty() {
            "0"
        } else {
            nr_evid_trimmed
        },
    ));
    elem.push_attribute(("totalPlata_A", rows.total_plata_a.to_string().as_str()));

    // ── Optional R-rows (only emit when Some) ─────────────────────────────────

    macro_rules! push_opt {
        ($field:expr, $name:expr) => {
            if let Some(v) = $field {
                elem.push_attribute(($name, v.to_string().as_str()));
            }
        };
    }

    // Sales rows
    push_opt!(rows.r1_1, "R1_1");
    push_opt!(rows.r9_1, "R9_1");
    push_opt!(rows.r9_2, "R9_2");
    push_opt!(rows.r10_1, "R10_1");
    push_opt!(rows.r10_2, "R10_2");
    push_opt!(rows.r11_1, "R11_1");
    push_opt!(rows.r11_2, "R11_2");
    push_opt!(rows.r13_1, "R13_1");

    // Purchase rows
    push_opt!(rows.r5_1, "R5_1");
    push_opt!(rows.r5_2, "R5_2");
    push_opt!(rows.r22_1, "R22_1");
    push_opt!(rows.r22_2, "R22_2");
    push_opt!(rows.r23_1, "R23_1");
    push_opt!(rows.r23_2, "R23_2");
    push_opt!(rows.r25_1, "R25_1");
    push_opt!(rows.r25_2, "R25_2");

    // Totals
    push_opt!(rows.r17_1, "R17_1");
    push_opt!(rows.r17_2, "R17_2");
    push_opt!(rows.r27_1, "R27_1");
    push_opt!(rows.r27_2, "R27_2");
    push_opt!(rows.r28_2, "R28_2");
    push_opt!(rows.r32_2, "R32_2");
    push_opt!(rows.r33_2, "R33_2");
    push_opt!(rows.r34_2, "R34_2");
    push_opt!(rows.r37_2, "R37_2");
    push_opt!(rows.r40_2, "R40_2");
    push_opt!(rows.r41_2, "R41_2");
    push_opt!(rows.r42_2, "R42_2");

    // Self-close the element
    w.write_event(Event::Empty(elem)).map_err(map_err)?;

    // quick_xml doesn't add a trailing newline; add one for readability
    // (xmllint is whitespace-tolerant)
    let mut bytes = w.into_inner().into_inner();
    bytes.push(b'\n');

    String::from_utf8(bytes).map_err(|e| AppError::Other(format!("XML utf8 error: {e}")))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anaf_decl::d300::rows::D300Rows;
    use crate::anaf_decl::version::resolve;
    use crate::anaf_decl::DeclKind;
    use chrono::NaiveDate;

    fn test_ver() -> SchemaVersion {
        // Use 2026 period so version.rs resolves v12 (matches vendored XSD)
        let period = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        resolve(DeclKind::D300, period).expect("schema version")
    }

    fn minimal_rows() -> D300Rows {
        D300Rows {
            luna: 1,
            an: 2026,
            depus_reprezentant: 0,
            bifa_interne: 0,
            temei: 0,
            nume_declar: "POPESCU".to_string(),
            prenume_declar: "ION".to_string(),
            functie_declar: "DIRECTOR".to_string(),
            cui: "12345678".to_string(),
            den: "Test SRL".to_string(),
            adresa: "Str. Test 1, Bucuresti".to_string(),
            banca: "Banca Test".to_string(),
            cont: "RO49AAAA1B31007593840000".to_string(),
            caen: "6201".to_string(),
            tip_decont: "L".to_string(),
            pro_rata: 100.0,
            bifa_cereale: "N".to_string(),
            bifa_mob: "N".to_string(),
            bifa_disp: "N".to_string(),
            bifa_cons: "N".to_string(),
            solicit_ramb: "N".to_string(),
            nr_evid: "0".to_string(),
            total_plata_a: 42,
            r9_1: Some(1000),
            r9_2: Some(210),
            r17_1: Some(1000),
            r17_2: Some(210),
            r22_1: Some(800),
            r22_2: Some(168),
            r27_1: Some(800),
            r27_2: Some(168),
            r28_2: Some(168),
            r32_2: Some(168),
            r34_2: Some(42),
            r37_2: Some(42),
            r41_2: Some(42),
            ..Default::default()
        }
    }

    #[test]
    fn generates_xml_declaration_and_root() {
        let ver = test_ver();
        let rows = minimal_rows();
        let xml = generate_d300_xml(&rows, &ver).expect("generate_d300_xml");

        assert!(
            xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"),
            "must start with XML declaration"
        );
        assert!(
            xml.contains("<declaratie300 "),
            "must contain <declaratie300 element"
        );
        assert!(
            xml.contains("xmlns=\"mfp:anaf:dgti:d300:declaratie:v12\""),
            "must contain v12 namespace"
        );
    }

    #[test]
    fn required_attributes_present() {
        let ver = test_ver();
        let rows = minimal_rows();
        let xml = generate_d300_xml(&rows, &ver).expect("generate_d300_xml");

        for attr in &[
            "luna=\"1\"",
            "an=\"2026\"",
            "depusReprezentant=\"0\"",
            "bifa_interne=\"0\"",
            "temei=\"0\"",
            "cui=\"12345678\"",
            "tip_decont=\"L\"",
            "totalPlata_A=\"42\"",
            "solicit_ramb=\"N\"",
            "caen=\"6201\"",
        ] {
            assert!(xml.contains(attr), "missing attribute: {attr}\nxml: {xml}");
        }
    }

    #[test]
    fn optional_rows_only_emitted_when_some() {
        let ver = test_ver();
        let mut rows = minimal_rows();
        rows.r10_1 = None;
        rows.r10_2 = None;
        let xml = generate_d300_xml(&rows, &ver).expect("generate_d300_xml");

        assert!(!xml.contains("R10_1"), "R10_1 must be absent when None");
        assert!(!xml.contains("R10_2"), "R10_2 must be absent when None");
        assert!(xml.contains("R9_1"), "R9_1 must be present");
    }

    #[test]
    fn pro_rata_100_formatted_correctly() {
        let ver = test_ver();
        let rows = minimal_rows(); // pro_rata = 100.0
        let xml = generate_d300_xml(&rows, &ver).expect("generate_d300_xml");
        // Should emit "100" (trailing zeros stripped: "100.00" → "100")
        assert!(
            xml.contains("pro_rata=\"100\""),
            "pro_rata 100 should be '100'"
        );
    }

    #[test]
    fn self_closing_element() {
        let ver = test_ver();
        let rows = minimal_rows();
        let xml = generate_d300_xml(&rows, &ver).expect("generate_d300_xml");
        // Self-closing: must end with />
        let trimmed = xml.trim_end_matches('\n').trim();
        assert!(
            trimmed.ends_with("/>"),
            "element must be self-closed (/>), got: {}",
            &trimmed[trimmed.len().saturating_sub(20)..]
        );
    }
}
