//! RO e-Transport — UIT declaration model, XML generator (schema v2) + validation.
//!
//! Legal base: OUG 41/2022 (+ OUG 115/2023 extending scope to ALL international transport,
//! OUG 129/2024, OUG 29/2025); high-risk goods list OPANAF 802/2022. In force 2026.
//!
//! Obligation (2026): a UIT is required when the vehicle's max admissible mass ≥ 2.5 t AND the
//! cargo per partner exceeds 500 kg OR 10.000 lei (ex-VAT), for high-risk goods domestically, and
//! for ALL intra-EU / import / export / international transport. The UIT must be obtained BEFORE
//! the transport starts (at most 3 days early) and accompany the goods; valid 5 days (15 intra-EU).
//!
//! Unlike D300/D394 (portal-only), e-Transport HAS an OAuth REST API
//! (POST api.anaf.ro/{prod|test}/ETRANSPORT/ws/v1/upload/ETRANSP/{cif}/2), same logincert OAuth2
//! token as e-Factura. The live upload lives in anaf::client; this module builds + validates the
//! payload (pure, fully testable) and is the input to that call.

use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, Event};
use quick_xml::Writer;
use serde::Deserialize;
use std::io::Cursor;

use crate::error::{AppError, AppResult};

pub const NAMESPACE: &str = "mfp:anaf:dgti:eTransport:declaratie:v2";

/// A transported-goods line (`<bunuriTransportate>`).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Good {
    /// Scope code (101 comercializare, 201 producție, … 9999 = same as operation).
    pub cod_scop_operatiune: String,
    /// NC tariff code (8 digits), optional.
    #[serde(default)]
    pub cod_tarifar: String,
    pub denumire_marfa: String,
    pub cantitate: f64,
    /// UN/ECE unit-of-measure code (e.g. KGM, H87, NIU).
    pub cod_unitate_masura: String,
    #[serde(default)]
    pub greutate_neta: Option<f64>,
    pub greutate_bruta: f64,
    #[serde(default)]
    pub valoare_lei_fara_tva: Option<f64>,
}

/// Commercial partner (`<partenerComercial>`).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Partner {
    /// ISO 3166-1 alpha-2 country code (EL for Greece).
    pub cod_tara: String,
    #[serde(default)]
    pub cod: String,
    pub denumire: String,
}

/// Transport data (`<dateTransport>`).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Transport {
    pub nr_vehicul: String,
    #[serde(default)]
    pub nr_remorca1: String,
    #[serde(default)]
    pub nr_remorca2: String,
    #[serde(default)]
    pub cod_tara_org_transport: String,
    #[serde(default)]
    pub cod_org_transport: String,
    #[serde(default)]
    pub denumire_org_transport: String,
    /// Planned transport date (YYYY-MM-DD).
    pub data_transport: String,
}

/// A route endpoint (`<locStartTraseuRutier>` / `<locFinalTraseuRutier>`): an address, OR a
/// border-crossing point (codPtf), OR a customs office (codBirouVamal).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RouteLoc {
    /// Border crossing point code (1..38) — used instead of an address for some operations.
    #[serde(default)]
    pub cod_ptf: Option<i64>,
    /// Customs office code — used for import (40) / export (50).
    #[serde(default)]
    pub cod_birou_vamal: Option<String>,
    // Address (used when neither codPtf nor codBirouVamal is set):
    #[serde(default)]
    pub cod_judet: Option<i64>,
    #[serde(default)]
    pub denumire_localitate: String,
    #[serde(default)]
    pub denumire_strada: String,
    #[serde(default)]
    pub numar: String,
    #[serde(default)]
    pub cod_postal: String,
    #[serde(default)]
    pub alte_info: String,
}

/// A transport document (`<documenteTransport>`).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TransportDoc {
    /// 10 = CMR, 20 = Factură, 30 = Aviz de însoțire, 9999 = Altele.
    pub tip_document: String,
    #[serde(default)]
    pub numar_document: String,
    #[serde(default)]
    pub data_document: String,
}

/// A full e-Transport notification (the UIT declaration).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EtransportDeclaration {
    /// Declarant CUI/CIF (digits).
    pub cod_declarant: String,
    #[serde(default)]
    pub ref_declarant: String,
    /// Operation type: 10 AIC / 12 LHI / 14 / 20 LIC / 22 / 24 / 30 național / 40 import /
    /// 50 export / 60 / 70.
    pub cod_tip_operatiune: String,
    pub goods: Vec<Good>,
    pub partner: Partner,
    pub transport: Transport,
    pub loc_start: RouteLoc,
    pub loc_final: RouteLoc,
    pub documents: Vec<TransportDoc>,
}

/// Validate a declaration; returns the list of human-readable problems (empty = valid).
pub fn validate_etransport(d: &EtransportDeclaration) -> Vec<String> {
    let mut errs = Vec::new();
    if d.cod_declarant.trim().is_empty() {
        errs.push("Lipsește codul declarantului (CUI).".into());
    }
    const VALID_OP_TYPES: &[&str] = &[
        "10", "12", "14", "20", "22", "24", "30", "40", "50", "60", "70",
    ];
    let op = d.cod_tip_operatiune.trim();
    if op.is_empty() {
        errs.push("Lipsește tipul operațiunii (codTipOperatiune).".into());
    } else if !VALID_OP_TYPES.contains(&op) {
        errs.push(format!(
            "Tip operațiune invalid: '{op}' (permise: {}).",
            VALID_OP_TYPES.join("/")
        ));
    }
    if d.goods.is_empty() {
        errs.push("Cel puțin o linie de marfă este obligatorie.".into());
    }
    for (i, g) in d.goods.iter().enumerate() {
        let n = i + 1;
        if g.denumire_marfa.trim().is_empty() {
            errs.push(format!("Marfa {n}: denumirea este obligatorie."));
        }
        if g.cantitate <= 0.0 {
            errs.push(format!("Marfa {n}: cantitatea trebuie să fie > 0."));
        }
        if g.cod_unitate_masura.trim().is_empty() {
            errs.push(format!("Marfa {n}: unitatea de măsură este obligatorie."));
        }
        if g.greutate_bruta <= 0.0 {
            errs.push(format!("Marfa {n}: greutatea brută trebuie să fie > 0."));
        }
        if g.cod_scop_operatiune.trim().is_empty() {
            errs.push(format!(
                "Marfa {n}: codul scop operațiune este obligatoriu."
            ));
        }
    }
    if d.partner.denumire.trim().is_empty() {
        errs.push("Lipsește denumirea partenerului comercial.".into());
    }
    if d.partner.cod_tara.trim().is_empty() {
        errs.push("Lipsește codul de țară al partenerului.".into());
    }
    if d.transport.nr_vehicul.trim().is_empty() {
        errs.push("Lipsește numărul de înmatriculare al vehiculului.".into());
    }
    if d.transport.data_transport.trim().is_empty() {
        errs.push("Lipsește data transportului.".into());
    }
    if d.documents.is_empty() {
        errs.push("Cel puțin un document de transport este obligatoriu.".into());
    }
    if !route_complete(&d.loc_start) {
        errs.push(
            "Locul de plecare incomplet: completați județul + localitatea, sau un punct de \
             frontieră / birou vamal."
                .into(),
        );
    }
    if !route_complete(&d.loc_final) {
        errs.push(
            "Locul de sosire incomplet: completați județul + localitatea, sau un punct de \
             frontieră / birou vamal."
                .into(),
        );
    }
    errs
}

fn dec_attr(v: f64) -> String {
    format!("{v:.4}")
}

/// Truncate + strip control chars for an attribute value (quick-xml escapes &<>'" itself).
fn clean(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect()
}

/// CUI digits-only (strip an "RO" prefix) — codDeclarant must be digits and must match the CIF
/// in the upload URL (which is also RO-stripped).
fn strip_ro(cui: &str) -> String {
    let s = cui.trim();
    s.strip_prefix("RO")
        .or_else(|| s.strip_prefix("ro"))
        .unwrap_or(s)
        .trim()
        .to_string()
}

/// Is a route endpoint complete per the schema: a full address (codJudet + localitate) OR a
/// border-crossing point (codPtf) OR a customs office (codBirouVamal).
fn route_complete(loc: &RouteLoc) -> bool {
    loc.cod_ptf.is_some()
        || loc
            .cod_birou_vamal
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty())
        || (loc.cod_judet.is_some() && !loc.denumire_localitate.trim().is_empty())
}

fn write_route(
    w: &mut Writer<Cursor<Vec<u8>>>,
    tag: &str,
    loc: &RouteLoc,
) -> Result<(), quick_xml::Error> {
    let mut e = BytesStart::new(tag);
    if let Some(ptf) = loc.cod_ptf {
        e.push_attribute(("codPtf", ptf.to_string().as_str()));
        w.write_event(Event::Empty(e))?;
    } else if let Some(ref bv) = loc.cod_birou_vamal {
        e.push_attribute(("codBirouVamal", clean(bv, 20).as_str()));
        w.write_event(Event::Empty(e))?;
    } else {
        // address form: <locStart…><locatie codJudet … denumireLocalitate …/></locStart…>
        w.write_event(Event::Start(e))?;
        let mut l = BytesStart::new("locatie");
        if let Some(j) = loc.cod_judet {
            l.push_attribute(("codJudet", j.to_string().as_str()));
        }
        if !loc.denumire_localitate.is_empty() {
            l.push_attribute((
                "denumireLocalitate",
                clean(&loc.denumire_localitate, 200).as_str(),
            ));
        }
        if !loc.denumire_strada.is_empty() {
            l.push_attribute(("denumireStrada", clean(&loc.denumire_strada, 200).as_str()));
        }
        if !loc.numar.is_empty() {
            l.push_attribute(("numar", clean(&loc.numar, 50).as_str()));
        }
        if !loc.cod_postal.is_empty() {
            l.push_attribute(("codPostal", clean(&loc.cod_postal, 10).as_str()));
        }
        if !loc.alte_info.is_empty() {
            l.push_attribute(("alteInfo", clean(&loc.alte_info, 200).as_str()));
        }
        w.write_event(Event::Empty(l))?;
        w.write_event(Event::End(BytesEnd::new(tag)))?;
    }
    Ok(())
}

/// Build the e-Transport XML (`<eTransport>` v2) from a declaration. Caller should validate first.
pub fn generate_etransport_xml(d: &EtransportDeclaration) -> AppResult<String> {
    let map = |e: quick_xml::Error| AppError::Other(format!("XML write error: {e}"));
    let mut w = Writer::new(Cursor::new(Vec::<u8>::new()));
    w.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(map)?;

    let mut root = BytesStart::new("eTransport");
    root.push_attribute(("xmlns", NAMESPACE));
    root.push_attribute(("xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance"));
    root.push_attribute((
        "codDeclarant",
        clean(&strip_ro(&d.cod_declarant), 13).as_str(),
    ));
    if !d.ref_declarant.is_empty() {
        root.push_attribute(("refDeclarant", clean(&d.ref_declarant, 50).as_str()));
    }
    w.write_event(Event::Start(root)).map_err(map)?;

    let mut notif = BytesStart::new("notificare");
    notif.push_attribute(("codTipOperatiune", clean(&d.cod_tip_operatiune, 4).as_str()));
    w.write_event(Event::Start(notif)).map_err(map)?;

    for g in &d.goods {
        let mut e = BytesStart::new("bunuriTransportate");
        e.push_attribute((
            "codScopOperatiune",
            clean(&g.cod_scop_operatiune, 6).as_str(),
        ));
        if !g.cod_tarifar.is_empty() {
            e.push_attribute(("codTarifar", clean(&g.cod_tarifar, 8).as_str()));
        }
        e.push_attribute(("denumireMarfa", clean(&g.denumire_marfa, 500).as_str()));
        e.push_attribute(("cantitate", dec_attr(g.cantitate).as_str()));
        e.push_attribute(("codUnitateMasura", clean(&g.cod_unitate_masura, 3).as_str()));
        if let Some(n) = g.greutate_neta {
            e.push_attribute(("greutateNeta", dec_attr(n).as_str()));
        }
        e.push_attribute(("greutateBruta", dec_attr(g.greutate_bruta).as_str()));
        if let Some(v) = g.valoare_lei_fara_tva {
            e.push_attribute(("valoareLeiFaraTva", format!("{v:.2}").as_str()));
        }
        w.write_event(Event::Empty(e)).map_err(map)?;
    }

    let mut p = BytesStart::new("partenerComercial");
    p.push_attribute(("codTara", clean(&d.partner.cod_tara, 2).as_str()));
    if !d.partner.cod.is_empty() {
        p.push_attribute(("cod", clean(&d.partner.cod, 30).as_str()));
    }
    p.push_attribute(("denumire", clean(&d.partner.denumire, 200).as_str()));
    w.write_event(Event::Empty(p)).map_err(map)?;

    let t = &d.transport;
    let mut dt = BytesStart::new("dateTransport");
    dt.push_attribute(("nrVehicul", clean(&t.nr_vehicul, 20).as_str()));
    if !t.nr_remorca1.is_empty() {
        dt.push_attribute(("nrRemorca1", clean(&t.nr_remorca1, 20).as_str()));
    }
    if !t.nr_remorca2.is_empty() {
        dt.push_attribute(("nrRemorca2", clean(&t.nr_remorca2, 20).as_str()));
    }
    if !t.cod_tara_org_transport.is_empty() {
        dt.push_attribute((
            "codTaraOrgTransport",
            clean(&t.cod_tara_org_transport, 2).as_str(),
        ));
    }
    if !t.cod_org_transport.is_empty() {
        dt.push_attribute(("codOrgTransport", clean(&t.cod_org_transport, 30).as_str()));
    }
    if !t.denumire_org_transport.is_empty() {
        dt.push_attribute((
            "denumireOrgTransport",
            clean(&t.denumire_org_transport, 200).as_str(),
        ));
    }
    dt.push_attribute(("dataTransport", clean(&t.data_transport, 10).as_str()));
    w.write_event(Event::Empty(dt)).map_err(map)?;

    write_route(&mut w, "locStartTraseuRutier", &d.loc_start).map_err(map)?;
    write_route(&mut w, "locFinalTraseuRutier", &d.loc_final).map_err(map)?;

    for doc in &d.documents {
        let mut e = BytesStart::new("documenteTransport");
        e.push_attribute(("tipDocument", clean(&doc.tip_document, 4).as_str()));
        if !doc.numar_document.is_empty() {
            e.push_attribute(("numarDocument", clean(&doc.numar_document, 50).as_str()));
        }
        if !doc.data_document.is_empty() {
            e.push_attribute(("dataDocument", clean(&doc.data_document, 10).as_str()));
        }
        w.write_event(Event::Empty(e)).map_err(map)?;
    }

    w.write_event(Event::End(BytesEnd::new("notificare")))
        .map_err(map)?;
    w.write_event(Event::End(BytesEnd::new("eTransport")))
        .map_err(map)?;
    let bytes = w.into_inner().into_inner();
    String::from_utf8(bytes).map_err(|e| AppError::Other(format!("UTF-8: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> EtransportDeclaration {
        EtransportDeclaration {
            cod_declarant: "12345678".into(),
            ref_declarant: "REF1".into(),
            cod_tip_operatiune: "30".into(),
            goods: vec![Good {
                cod_scop_operatiune: "101".into(),
                cod_tarifar: "07020000".into(),
                denumire_marfa: "Roșii".into(),
                cantitate: 1000.0,
                cod_unitate_masura: "KGM".into(),
                greutate_neta: Some(1000.0),
                greutate_bruta: 1050.0,
                valoare_lei_fara_tva: Some(5000.0),
            }],
            partner: Partner {
                cod_tara: "RO".into(),
                cod: "RO9999".into(),
                denumire: "Client SRL".into(),
            },
            transport: Transport {
                nr_vehicul: "B100ABC".into(),
                data_transport: "2026-06-10".into(),
                ..Default::default()
            },
            loc_start: RouteLoc {
                cod_judet: Some(40),
                denumire_localitate: "București".into(),
                denumire_strada: "Str. A".into(),
                numar: "1".into(),
                ..Default::default()
            },
            loc_final: RouteLoc {
                cod_judet: Some(12),
                denumire_localitate: "Cluj-Napoca".into(),
                ..Default::default()
            },
            documents: vec![TransportDoc {
                tip_document: "20".into(),
                numar_document: "F123".into(),
                data_document: "2026-06-09".into(),
            }],
        }
    }

    #[test]
    fn valid_declaration_passes() {
        assert!(validate_etransport(&sample()).is_empty());
    }

    #[test]
    fn validation_catches_missing_required_fields() {
        let mut d = sample();
        d.cod_declarant = "".into();
        d.transport.nr_vehicul = "".into();
        d.goods[0].greutate_bruta = 0.0;
        d.documents.clear();
        let errs = validate_etransport(&d);
        assert!(errs.iter().any(|e| e.contains("declarantului")));
        assert!(errs.iter().any(|e| e.contains("vehiculului")));
        assert!(errs.iter().any(|e| e.contains("greutatea brută")));
        assert!(errs.iter().any(|e| e.contains("document")));
    }

    #[test]
    fn generates_v2_xml() {
        let xml = generate_etransport_xml(&sample()).unwrap();
        assert!(xml.contains(&format!("xmlns=\"{NAMESPACE}\"")));
        assert!(xml.contains("<eTransport "));
        assert!(xml.contains("codDeclarant=\"12345678\""));
        assert!(xml.contains("<notificare codTipOperatiune=\"30\""));
        assert!(xml.contains("denumireMarfa=\"Roșii\""));
        assert!(xml.contains("greutateBruta=\"1050.0000\""));
        assert!(xml.contains("codUnitateMasura=\"KGM\""));
        assert!(xml.contains("<partenerComercial codTara=\"RO\""));
        assert!(xml.contains("nrVehicul=\"B100ABC\""));
        // route start as address (locatie), final as address.
        assert!(xml.contains("<locStartTraseuRutier><locatie codJudet=\"40\""));
        assert!(xml.contains("tipDocument=\"20\""));
        assert!(xml.contains("</eTransport>"));
    }

    #[test]
    fn strips_ro_prefix_from_declarant_to_match_url_cif() {
        let mut d = sample();
        d.cod_declarant = "RO12345678".into();
        let xml = generate_etransport_xml(&d).unwrap();
        assert!(xml.contains("codDeclarant=\"12345678\""));
        assert!(xml.starts_with("<?xml"));
    }

    #[test]
    fn validation_rejects_incomplete_route_endpoints() {
        let mut d = sample();
        d.loc_start = RouteLoc {
            denumire_localitate: "București".into(), // localitate without codJudet → incomplete
            ..Default::default()
        };
        let errs = validate_etransport(&d);
        assert!(errs.iter().any(|e| e.contains("Locul de plecare")));
        // a border point alone is complete:
        d.loc_start = RouteLoc {
            cod_ptf: Some(4),
            ..Default::default()
        };
        assert!(!validate_etransport(&d)
            .iter()
            .any(|e| e.contains("Locul de plecare")));
    }

    #[test]
    fn route_endpoint_can_be_a_border_point() {
        let mut d = sample();
        d.cod_tip_operatiune = "10".into();
        d.loc_start = RouteLoc {
            cod_ptf: Some(4),
            ..Default::default()
        };
        let xml = generate_etransport_xml(&d).unwrap();
        assert!(xml.contains("<locStartTraseuRutier codPtf=\"4\""));
    }
}
