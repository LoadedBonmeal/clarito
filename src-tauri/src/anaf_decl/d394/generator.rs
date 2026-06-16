//! D394 v5 XML generator.
//!
//! Emits a schema-conformant `<declaratie394 ...>` with nested child elements:
//!   `<informatii .../>` (required, self-closed)
//!   `<serieFacturi .../>*` (one tip=1 + one tip=2 when L/LS/V ops exist)
//!   `<rezumat1 .../>*` (one per (tip_partener, cota), self-closed)
//!   `<rezumat2 .../>*` (one per distinct cota≠0, self-closed)
//!   `<op1 ...>` (one per partner × operation, with optional `<op11/>` children)
//!     `<op11 .../>` (required when tip_partener=1 & tip∈{C,V})
//!   `</op1>`
//!
//! Uses `quick_xml::Writer` + `BytesStart::push_attribute` + `Event::Empty`
//! for self-closing elements. op1 with op11 children uses Start/End events.

use std::io::Cursor;

use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, Event};
use quick_xml::Writer;

use crate::anaf_decl::version::SchemaVersion;
use crate::db::companies::Company;
use crate::error::{AppError, AppResult};

use super::sections::{D394Doc, Detaliu, Informatii, Op1, Rezumat1, Rezumat2, SerieFacturi};
use super::D394Submission;

fn map_err(e: quick_xml::Error) -> AppError {
    AppError::Other(format!("XML write error: {e}"))
}

/// Strip the "RO" prefix from a CUI string (digits-only form required by CuiSType).
fn strip_ro(cui: &str) -> String {
    let s = cui.trim();
    let s = if s.to_uppercase().starts_with("RO") {
        &s[2..]
    } else {
        s
    };
    s.trim().to_string()
}

/// Sanitize an attribute value: strip XML-1.0-forbidden control characters. We must NOT
/// entity-escape here — `BytesStart::push_attribute` already escapes & < > ' " once, so
/// pre-escaping would double-escape (e.g. "A & B" → "A &amp;amp; B"). Same fix as the D390
/// generator.
fn xml_attr(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}

/// Generate a schema-valid D394 v5 XML string.
pub fn generate_d394_xml(
    doc: &D394Doc,
    submission: &D394Submission,
    company: &Company,
    ver: &SchemaVersion,
) -> AppResult<String> {
    let mut w = Writer::new_with_indent(Cursor::new(Vec::<u8>::new()), b' ', 2);

    // <?xml version="1.0" encoding="UTF-8"?>
    w.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(map_err)?;

    // ── Root element: <declaratie394 ...header attrs...> ─────────────────────
    let mut root = BytesStart::new(ver.root_element);

    root.push_attribute(("xmlns", ver.namespace));
    root.push_attribute(("luna", doc.luna.to_string().as_str()));
    root.push_attribute(("an", doc.an.to_string().as_str()));
    root.push_attribute(("tip_D394", submission.tip_d394.as_str()));
    root.push_attribute(("sistemTVA", if submission.sistem_tva { "1" } else { "0" }));

    let effective_op_efectuate = submission.op_efectuate || !doc.op1_list.is_empty();
    root.push_attribute((
        "op_efectuate",
        if effective_op_efectuate { "1" } else { "0" },
    ));

    // cui: CuiSType pattern [1-9]\d{1,9}
    let cui = strip_ro(&company.cui);
    root.push_attribute(("cui", cui.as_str()));

    let caen = submission.caen.chars().take(15).collect::<String>();
    root.push_attribute(("caen", caen.as_str()));

    let den = xml_attr(&company.legal_name.chars().take(200).collect::<String>());
    root.push_attribute(("den", den.as_str()));

    let adresa = {
        let parts: Vec<&str> = [
            company.address.as_str(),
            company.city.as_str(),
            company.county.as_str(),
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
        xml_attr(&parts.join(", ").chars().take(1000).collect::<String>())
    };
    root.push_attribute(("adresa", adresa.as_str()));

    let telefon = submission.telefon.chars().take(15).collect::<String>();
    root.push_attribute(("telefon", telefon.as_str()));

    root.push_attribute(("totalPlata_A", doc.total_plata_a.to_string().as_str()));

    let den_r = if submission.den_r.trim().is_empty() {
        xml_attr(&company.legal_name.chars().take(200).collect::<String>())
    } else {
        xml_attr(&submission.den_r.chars().take(200).collect::<String>())
    };
    root.push_attribute(("denR", den_r.as_str()));

    let functie_reprez = submission
        .functie_reprez
        .chars()
        .take(100)
        .collect::<String>();
    root.push_attribute(("functie_reprez", functie_reprez.as_str()));

    let adresa_r = if submission.adresa_r.trim().is_empty() {
        adresa.clone()
    } else {
        xml_attr(&submission.adresa_r.chars().take(1000).collect::<String>())
    };
    root.push_attribute(("adresaR", adresa_r.as_str()));

    root.push_attribute(("tip_intocmit", submission.tip_intocmit.to_string().as_str()));

    let den_intocmit = if submission.den_intocmit.trim().is_empty() {
        xml_attr(&company.legal_name.chars().take(75).collect::<String>())
    } else {
        xml_attr(&submission.den_intocmit.chars().take(75).collect::<String>())
    };
    root.push_attribute(("den_intocmit", den_intocmit.as_str()));

    root.push_attribute(("cif_intocmit", submission.cif_intocmit.to_string().as_str()));

    if submission.tip_intocmit == 0 {
        if let Some(ref calitate) = submission.calitate_intocmit {
            let calitate_str = xml_attr(&calitate.chars().take(75).collect::<String>());
            if !calitate_str.is_empty() {
                root.push_attribute(("calitate_intocmit", calitate_str.as_str()));
            }
        }
    }

    root.push_attribute(("optiune", if submission.optiune { "1" } else { "0" }));
    root.push_attribute(("prsAfiliat", if submission.prs_afiliat { "1" } else { "0" }));

    w.write_event(Event::Start(root)).map_err(map_err)?;

    // ── <informatii .../> ────────────────────────────────────────────────────
    emit_informatii(&mut w, &doc.informatii)?;

    // ── Element sequence order per XSD precedence table: ─────────────────────
    // informatii → rezumat1* → (detaliu*) → rezumat2* → serieFacturi* →
    // lista* → facturi* → op1* (with op11 children) → op2*

    // ── <rezumat1 .../>* ─────────────────────────────────────────────────────
    for r1 in &doc.rezumat1_list {
        emit_rezumat1(&mut w, r1)?;
    }

    // ── <rezumat2 .../>* ─────────────────────────────────────────────────────
    for r2 in &doc.rezumat2_list {
        emit_rezumat2(&mut w, r2)?;
    }

    // ── <serieFacturi .../>* ─────────────────────────────────────────────────
    for sf in &doc.serie_facturi {
        emit_serie_facturi(&mut w, sf)?;
    }

    // ── <op1 ...>* (with optional <op11/> children) ──────────────────────────
    for op in &doc.op1_list {
        emit_op1(&mut w, op)?;
    }

    w.write_event(Event::End(BytesEnd::new(ver.root_element)))
        .map_err(map_err)?;

    let mut bytes = w.into_inner().into_inner();
    bytes.push(b'\n');

    String::from_utf8(bytes).map_err(|e| AppError::Other(format!("XML utf8 error: {e}")))
}

fn emit_informatii(w: &mut Writer<Cursor<Vec<u8>>>, inf: &Informatii) -> AppResult<()> {
    let mut elem = BytesStart::new("informatii");

    elem.push_attribute(("nrCui1", inf.nr_cui1.to_string().as_str()));
    elem.push_attribute(("nrCui2", inf.nr_cui2.to_string().as_str()));
    elem.push_attribute(("nrCui3", inf.nr_cui3.to_string().as_str()));
    elem.push_attribute(("nrCui4", inf.nr_cui4.to_string().as_str()));
    elem.push_attribute(("nr_BF_i1", inf.nr_bf_i1.to_string().as_str()));
    elem.push_attribute(("incasari_i1", inf.incasari_i1.to_string().as_str()));
    elem.push_attribute(("incasari_i2", inf.incasari_i2.to_string().as_str()));
    elem.push_attribute(("nrFacturi_terti", inf.nr_facturi_terti.to_string().as_str()));
    elem.push_attribute(("nrFacturi_benef", inf.nr_facturi_benef.to_string().as_str()));
    elem.push_attribute(("nrFacturi", inf.nr_facturi.to_string().as_str()));
    elem.push_attribute(("nrFacturiL_PF", inf.nr_facturi_l_pf.to_string().as_str()));
    elem.push_attribute(("nrFacturiLS_PF", inf.nr_facturi_ls_pf.to_string().as_str()));
    elem.push_attribute(("val_LS_PF", inf.val_ls_pf.to_string().as_str()));
    elem.push_attribute(("solicit", inf.solicit.to_string().as_str()));

    macro_rules! push_opt {
        ($field:expr, $name:expr) => {
            if let Some(v) = $field {
                elem.push_attribute(($name, v.to_string().as_str()));
            }
        };
    }

    push_opt!(inf.tva_col24, "tvaCol24");
    push_opt!(inf.tva_col21, "tvaCol21");
    push_opt!(inf.tva_col11, "tvaCol11");
    push_opt!(inf.tva_col20, "tvaCol20");
    push_opt!(inf.tva_col19, "tvaCol19");
    push_opt!(inf.tva_col9, "tvaCol9");
    push_opt!(inf.tva_col5, "tvaCol5");

    push_opt!(inf.tva_ded24, "tvaDed24");
    push_opt!(inf.tva_ded21, "tvaDed21");
    push_opt!(inf.tva_ded11, "tvaDed11");
    push_opt!(inf.tva_ded20, "tvaDed20");
    push_opt!(inf.tva_ded19, "tvaDed19");
    push_opt!(inf.tva_ded9, "tvaDed9");
    push_opt!(inf.tva_ded5, "tvaDed5");

    // Required tvaDedAI* — always emit (even if 0)
    elem.push_attribute(("tvaDedAI24", inf.tva_ded_ai24.to_string().as_str()));
    elem.push_attribute(("tvaDedAI21", inf.tva_ded_ai21.to_string().as_str()));
    elem.push_attribute(("tvaDedAI11", inf.tva_ded_ai11.to_string().as_str()));
    elem.push_attribute(("tvaDedAI20", inf.tva_ded_ai20.to_string().as_str()));
    elem.push_attribute(("tvaDedAI19", inf.tva_ded_ai19.to_string().as_str()));
    elem.push_attribute(("tvaDedAI9", inf.tva_ded_ai9.to_string().as_str()));
    elem.push_attribute(("tvaDedAI5", inf.tva_ded_ai5.to_string().as_str()));

    push_opt!(inf.efectuat, "efectuat");

    w.write_event(Event::Empty(elem)).map_err(map_err)
}

fn emit_serie_facturi(w: &mut Writer<Cursor<Vec<u8>>>, sf: &SerieFacturi) -> AppResult<()> {
    let mut elem = BytesStart::new("serieFacturi");
    elem.push_attribute(("tip", sf.tip.to_string().as_str()));
    elem.push_attribute(("nrI", sf.nr_i.as_str()));
    w.write_event(Event::Empty(elem)).map_err(map_err)
}

fn emit_rezumat1(w: &mut Writer<Cursor<Vec<u8>>>, r: &Rezumat1) -> AppResult<()> {
    let mut elem = BytesStart::new("rezumat1");

    elem.push_attribute(("tip_partener", r.tip_partener.to_string().as_str()));
    elem.push_attribute(("cota", r.cota.to_string().as_str()));

    macro_rules! push_opt_r1 {
        ($field:expr, $name:expr) => {
            if let Some(v) = $field {
                elem.push_attribute(($name, v.to_string().as_str()));
            }
        };
    }

    push_opt_r1!(r.facturi_l, "facturiL");
    push_opt_r1!(r.baza_l, "bazaL");
    push_opt_r1!(r.tva_l, "tvaL");
    push_opt_r1!(r.facturi_ls, "facturiLS");
    push_opt_r1!(r.baza_ls, "bazaLS");
    push_opt_r1!(r.facturi_a, "facturiA");
    push_opt_r1!(r.baza_a, "bazaA");
    push_opt_r1!(r.tva_a, "tvaA");
    push_opt_r1!(r.facturi_ai, "facturiAI");
    push_opt_r1!(r.baza_ai, "bazaAI");
    push_opt_r1!(r.tva_ai, "tvaAI");
    push_opt_r1!(r.facturi_as, "facturiAS");
    push_opt_r1!(r.baza_as, "bazaAS");
    push_opt_r1!(r.facturi_v, "facturiV");
    push_opt_r1!(r.baza_v, "bazaV");
    push_opt_r1!(r.facturi_c, "facturiC");
    push_opt_r1!(r.baza_c, "bazaC");
    push_opt_r1!(r.tva_c, "tvaC");
    // N fields (for tp=2, cota=0 — emitted only when set)
    push_opt_r1!(r.facturi_n, "facturiN");
    push_opt_r1!(r.document_n, "document_N");
    push_opt_r1!(r.baza_n, "bazaN");

    if r.detaliu_list.is_empty() {
        // Self-closing when no detaliu children
        w.write_event(Event::Empty(elem)).map_err(map_err)?;
    } else {
        // Open rezumat1, emit detaliu children, close
        w.write_event(Event::Start(elem)).map_err(map_err)?;
        for det in &r.detaliu_list {
            emit_detaliu(w, det)?;
        }
        w.write_event(Event::End(BytesEnd::new("rezumat1")))
            .map_err(map_err)?;
    }
    Ok(())
}

fn emit_detaliu(w: &mut Writer<Cursor<Vec<u8>>>, det: &Detaliu) -> AppResult<()> {
    let mut elem = BytesStart::new("detaliu");

    elem.push_attribute(("bun", det.bun.to_string().as_str()));

    // For tp=1: nrLivV/bazaLivV REQUIRED (even if 0)
    if let Some(v) = det.nr_liv_v {
        elem.push_attribute(("nrLivV", v.to_string().as_str()));
    }
    if let Some(v) = det.baza_liv_v {
        elem.push_attribute(("bazaLivV", v.to_string().as_str()));
    }
    // For tp=1: nrAchizC/bazaAchizC/tvaAchizC REQUIRED (even if 0)
    if let Some(v) = det.nr_achiz_c {
        elem.push_attribute(("nrAchizC", v.to_string().as_str()));
    }
    if let Some(v) = det.baza_achiz_c {
        elem.push_attribute(("bazaAchizC", v.to_string().as_str()));
    }
    if let Some(v) = det.tva_achiz_c {
        elem.push_attribute(("tvaAchizC", v.to_string().as_str()));
    }

    w.write_event(Event::Empty(elem)).map_err(map_err)
}

fn emit_rezumat2(w: &mut Writer<Cursor<Vec<u8>>>, r: &Rezumat2) -> AppResult<()> {
    let mut elem = BytesStart::new("rezumat2");

    elem.push_attribute(("cota", r.cota.to_string().as_str()));

    // Rezumat2 has many required fields with specific names from the XSD.
    // The validator checks nrFacturiL/bazaL/tvaL, nrFacturiA/bazaA/tvaA, nrFacturiAI/bazaAI/tvaAI.
    // The XSD also requires the "FSL/FSA/FSAI/BFAI" simplified-invoice fields (cartuș I).
    // Required by checkTag: bazaFSLcod, TVAFSLcod, bazaFSL, TVAFSL, bazaFSA, TVAFSA,
    //   bazaFSAI, TVAFSAI, bazaBFAI, TVABFAI, nrFacturiL, bazaL, tvaL,
    //   nrFacturiA, bazaA, tvaA, nrFacturiAI, bazaAI, tvaAI

    // Facturi simplificate (cartuș I) per cotă — din rândurile numerar introduse manual (0 implicit).
    elem.push_attribute(("bazaFSLcod", r.baza_fsl_cod.to_string().as_str()));
    elem.push_attribute(("TVAFSLcod", r.tva_fsl_cod.to_string().as_str()));
    elem.push_attribute(("bazaFSL", r.baza_fsl.to_string().as_str()));
    elem.push_attribute(("TVAFSL", r.tva_fsl.to_string().as_str()));
    elem.push_attribute(("bazaFSA", r.baza_fsa.to_string().as_str()));
    elem.push_attribute(("TVAFSA", r.tva_fsa.to_string().as_str()));
    elem.push_attribute(("bazaFSAI", r.baza_fsai.to_string().as_str()));
    elem.push_attribute(("TVAFSAI", r.tva_fsai.to_string().as_str()));
    elem.push_attribute(("bazaBFAI", r.baza_bfai.to_string().as_str()));
    elem.push_attribute(("TVABFAI", r.tva_bfai.to_string().as_str()));

    // Op1 aggregates (required by R96-R104)
    elem.push_attribute(("nrFacturiL", r.nr_facturi_l.to_string().as_str()));
    elem.push_attribute(("bazaL", r.baza_l.to_string().as_str()));
    elem.push_attribute(("tvaL", r.tva_l.to_string().as_str()));
    elem.push_attribute(("nrFacturiA", r.nr_facturi_a.to_string().as_str()));
    elem.push_attribute(("bazaA", r.baza_a.to_string().as_str()));
    elem.push_attribute(("tvaA", r.tva_a.to_string().as_str()));
    elem.push_attribute(("nrFacturiAI", r.nr_facturi_ai.to_string().as_str()));
    elem.push_attribute(("bazaAI", r.baza_ai.to_string().as_str()));
    elem.push_attribute(("tvaAI", r.tva_ai.to_string().as_str()));

    // R105-R108: baza_incasari_i1/tva_incasari_i1/baza_incasari_i2/tva_incasari_i2
    // required when cota≠24; emit 0 when no i1/i2 data.
    if r.cota != 24 {
        elem.push_attribute(("baza_incasari_i1", r.baza_incasari_i1.to_string().as_str()));
        elem.push_attribute(("tva_incasari_i1", r.tva_incasari_i1.to_string().as_str()));
        elem.push_attribute(("baza_incasari_i2", r.baza_incasari_i2.to_string().as_str()));
        elem.push_attribute(("tva_incasari_i2", r.tva_incasari_i2.to_string().as_str()));
    }

    // bazaL_PF and tvaL_PF are required by XSD (must be 0 from 2017 onwards per R109/R110)
    elem.push_attribute(("bazaL_PF", "0"));
    elem.push_attribute(("tvaL_PF", "0"));

    w.write_event(Event::Empty(elem)).map_err(map_err)
}

fn emit_op1(w: &mut Writer<Cursor<Vec<u8>>>, op: &Op1) -> AppResult<()> {
    let mut elem = BytesStart::new("op1");

    elem.push_attribute(("tip", op.tip.as_str()));
    elem.push_attribute(("tip_partener", op.tip_partener.to_string().as_str()));
    elem.push_attribute(("cota", op.cota.to_string().as_str()));

    if !op.cui_p.is_empty() {
        elem.push_attribute(("cuiP", op.cui_p.as_str()));
    }

    let den_p = xml_attr(&op.den_p);
    elem.push_attribute(("denP", den_p.as_str()));
    elem.push_attribute(("nrFact", op.nr_fact.to_string().as_str()));
    elem.push_attribute(("baza", op.baza.to_string().as_str()));

    if let Some(tva) = op.tva {
        elem.push_attribute(("tva", tva.to_string().as_str()));
    }

    if op.op11_list.is_empty() {
        // Self-closing
        w.write_event(Event::Empty(elem)).map_err(map_err)?;
    } else {
        // Has op11 children — use Start + children + End
        w.write_event(Event::Start(elem)).map_err(map_err)?;
        for op11 in &op.op11_list {
            let mut e11 = BytesStart::new("op11");
            e11.push_attribute(("nrFactPR", op11.nr_fact_pr.to_string().as_str()));
            e11.push_attribute(("codPR", op11.cod_pr.to_string().as_str()));
            e11.push_attribute(("bazaPR", op11.baza_pr.to_string().as_str()));
            if let Some(tva_pr) = op11.tva_pr {
                e11.push_attribute(("tvaPR", tva_pr.to_string().as_str()));
            }
            w.write_event(Event::Empty(e11)).map_err(map_err)?;
        }
        w.write_event(Event::End(BytesEnd::new("op1")))
            .map_err(map_err)?;
    }

    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anaf_decl::d394::sections::build_sections;
    use crate::anaf_decl::version::resolve;
    use crate::anaf_decl::DeclKind;
    use crate::commands::d394::{D394Partner, D394Report};
    use crate::db::companies::Company;
    use chrono::NaiveDate;

    fn test_company() -> Company {
        Company {
            id: "test-id".to_string(),
            // Valid CUI: 12345674
            cui: "RO12345674".to_string(),
            legal_name: "CLARITO TEST SRL".to_string(),
            trade_name: None,
            registry_number: None,
            vat_payer: true,
            cash_vat: false,
            address: "Calea Victoriei 155".to_string(),
            city: "Bucuresti".to_string(),
            county: "IF".to_string(),
            postal_code: None,
            country: "RO".to_string(),
            email: None,
            phone: Some("0721000000".to_string()),
            iban: None,
            bank_name: None,
            is_active: true,
            spv_enabled: false,
            tax_regime: "micro".into(),
            invoice_series: "F".to_string(),
            last_invoice_number: 0,
            logo_path: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn test_submission() -> D394Submission {
        D394Submission {
            tip_d394: "L".to_string(),
            caen: "6201".to_string(),
            telefon: "0721000000".to_string(),
            den_r: "POPESCU ION".to_string(),
            functie_reprez: "DIRECTOR".to_string(),
            adresa_r: "Calea Victoriei 155, Bucuresti".to_string(),
            tip_intocmit: 0,
            den_intocmit: "POPESCU ION".to_string(),
            // Valid CUI: 12345674
            cif_intocmit: 12345674,
            calitate_intocmit: Some("Reprezentant".to_string()),
            op_efectuate: true,
            ..Default::default()
        }
    }

    fn test_report() -> D394Report {
        D394Report {
            company_cui: "RO12345674".to_string(),
            period_from: "2025-09-01".to_string(),
            period_to: "2025-09-30".to_string(),
            partners: vec![D394Partner {
                // Valid CUI: 98765438
                partner_cui: "RO98765438".to_string(),
                partner_name: "SC CLIENT SRL".to_string(),
                vat_category: "S".to_string(),
                vat_rate: "19".to_string(),
                invoice_count: 3,
                base: "10000.00".to_string(),
                vat: "1900.00".to_string(),
                art331_code: None,
            }],
            total_base: "10000.00".to_string(),
            total_vat: "1900.00".to_string(),
            invoice_count: 3,
            purchase_partners: vec![D394Partner {
                // Valid CUI: 11111110
                partner_cui: "RO11111110".to_string(),
                partner_name: "SC FURNIZOR SRL".to_string(),
                vat_category: "S".to_string(),
                vat_rate: "21".to_string(),
                invoice_count: 2,
                base: "8000.00".to_string(),
                vat: "1680.00".to_string(),
                art331_code: None,
            }],
            total_purchase_base: "8000.00".to_string(),
            total_purchase_vat: "1680.00".to_string(),
            purchase_invoice_count: 2,
            purchase_unparsed_count: 0,
        }
    }

    #[test]
    fn generates_xml_declaration_and_root() {
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();
        let ver = resolve(DeclKind::D394, period).unwrap();
        let report = test_report();
        let sub = test_submission();
        let company = test_company();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        let xml = generate_d394_xml(&doc, &sub, &company, &ver).unwrap();

        assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(xml.contains("<declaratie394 "));
        assert!(xml.contains("xmlns=\"mfp:anaf:dgti:d394:declaratie:v5\""));
        assert!(xml.contains("</declaratie394>"));
    }

    #[test]
    fn company_name_special_chars_are_single_escaped() {
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();
        let ver = resolve(DeclKind::D394, period).unwrap();
        let report = test_report();
        let sub = test_submission();
        let mut company = test_company();
        company.legal_name = "A & B <SRL>".to_string();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        let xml = generate_d394_xml(&doc, &sub, &company, &ver).unwrap();

        // push_attribute escapes once; xml_attr must NOT pre-escape (else &amp;amp;).
        assert!(xml.contains("den=\"A &amp; B &lt;SRL&gt;\""));
        assert!(!xml.contains("&amp;amp;"), "must not double-escape");
    }

    #[test]
    fn required_header_attributes_present() {
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();
        let ver = resolve(DeclKind::D394, period).unwrap();
        let report = test_report();
        let sub = test_submission();
        let company = test_company();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        let xml = generate_d394_xml(&doc, &sub, &company, &ver).unwrap();

        for attr in &[
            "luna=\"9\"",
            "an=\"2025\"",
            "tip_D394=\"L\"",
            "sistemTVA=\"0\"",
            "op_efectuate=\"1\"",
            "cui=\"12345674\"",
            "caen=\"6201\"",
            "telefon=\"0721000000\"",
            "optiune=\"0\"",
            "prsAfiliat=\"0\"",
            "tip_intocmit=\"0\"",
        ] {
            assert!(xml.contains(attr), "missing attr: {attr}\n{xml}");
        }
    }

    #[test]
    fn informatii_element_present_with_required_attrs() {
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();
        let ver = resolve(DeclKind::D394, period).unwrap();
        let report = test_report();
        let sub = test_submission();
        let company = test_company();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        let xml = generate_d394_xml(&doc, &sub, &company, &ver).unwrap();

        assert!(xml.contains("<informatii "), "must have <informatii");
        assert!(xml.contains("nrCui1="), "informatii must have nrCui1");
        assert!(xml.contains("tvaDedAI21="), "must have tvaDedAI21");
        assert!(xml.contains("tvaDedAI19="), "must have tvaDedAI19");
        assert!(xml.contains("solicit="), "must have solicit");
    }

    #[test]
    fn op1_elements_present() {
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();
        let ver = resolve(DeclKind::D394, period).unwrap();
        let report = test_report();
        let sub = test_submission();
        let company = test_company();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        let xml = generate_d394_xml(&doc, &sub, &company, &ver).unwrap();

        assert!(
            xml.contains("<op1 ") || xml.contains("<op1\n"),
            "must have op1"
        );
        assert!(xml.contains("tip=\"L\""), "must have sales tip=L");
        assert!(xml.contains("tip=\"A\""), "must have purchase tip=A");
        assert!(xml.contains("denP="), "op1 must have denP");
        assert!(xml.contains("nrFact="), "op1 must have nrFact");
        assert!(xml.contains("baza="), "op1 must have baza");
    }

    #[test]
    fn rezumat1_elements_present() {
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();
        let ver = resolve(DeclKind::D394, period).unwrap();
        let report = test_report();
        let sub = test_submission();
        let company = test_company();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        let xml = generate_d394_xml(&doc, &sub, &company, &ver).unwrap();

        assert!(xml.contains("<rezumat1 "), "must have rezumat1");
        assert!(
            xml.contains("tip_partener="),
            "rezumat1 must have tip_partener"
        );
        assert!(xml.contains("cota="), "rezumat1 must have cota");
    }

    #[test]
    fn rezumat2_elements_present() {
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();
        let ver = resolve(DeclKind::D394, period).unwrap();
        let report = test_report();
        let sub = test_submission();
        let company = test_company();

        let doc = build_sections(&report, &sub, &company, period).unwrap();
        let xml = generate_d394_xml(&doc, &sub, &company, &ver).unwrap();

        assert!(xml.contains("<rezumat2 "), "must have rezumat2 for cota≠0");
        assert!(xml.contains("nrFacturiL="), "rezumat2 must have nrFacturiL");
    }

    #[test]
    fn empty_report_generates_minimal_valid_xml() {
        let period = NaiveDate::from_ymd_opt(2025, 9, 1).unwrap();
        let ver = resolve(DeclKind::D394, period).unwrap();
        let empty_report = D394Report {
            company_cui: "RO12345674".to_string(),
            period_from: "2025-09-01".to_string(),
            period_to: "2025-09-30".to_string(),
            partners: vec![],
            total_base: "0.00".to_string(),
            total_vat: "0.00".to_string(),
            invoice_count: 0,
            purchase_partners: vec![],
            total_purchase_base: "0.00".to_string(),
            total_purchase_vat: "0.00".to_string(),
            purchase_invoice_count: 0,
            purchase_unparsed_count: 0,
        };
        let sub = test_submission();
        let company = test_company();

        let doc = build_sections(&empty_report, &sub, &company, period).unwrap();
        let xml = generate_d394_xml(&doc, &sub, &company, &ver).unwrap();

        assert!(xml.contains("<declaratie394 "));
        assert!(xml.contains("<informatii "));
        assert!(xml.contains("</declaratie394>"));
        // Empty: op_efectuate=0 (no ops) → totalPlata_A=0
        assert!(!xml.contains("<op1 ") && !xml.contains("<op1\n"));
        assert!(!xml.contains("<rezumat1 "));
    }
}
