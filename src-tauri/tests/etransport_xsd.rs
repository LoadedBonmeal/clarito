//! Integration test: generate an e-Transport v2 XML and validate it against the official ANAF XSD
//! (`schema_ETR_v2.xsd`, targetNamespace `mfp:anaf:dgti:eTransport:declaratie:v2`) via `xmllint`.
//!
//! Skips gracefully when the XSD or xmllint are absent so the standard `cargo test` gate stays green
//! everywhere. On a machine that has both, this is the proof that the generated e-Transport notification
//! is structurally conformant with the official schema.

use std::path::Path;

use efactura_desktop_lib::anaf_decl::etransport::{
    generate_etransport_xml, validate_etransport, EtransportDeclaration, Good, Partner, RouteLoc,
    Transport, TransportDoc,
};
use efactura_desktop_lib::anaf_decl::validation::{validate_with_xsd, xmllint_available};

/// A complete, valid national-transport (codTipOperatiune=30) notification with all required fields.
fn full_declaration() -> EtransportDeclaration {
    EtransportDeclaration {
        cod_declarant: "12345674".into(), // valid CUI checksum
        ref_declarant: "REF-001".into(),
        cod_tip_operatiune: "30".into(), // transport pe teritoriul național
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
            cod: "12345674".into(),
            denumire: "Client SRL".into(),
        },
        transport: Transport {
            nr_vehicul: "B100ABC".into(),
            cod_tara_org_transport: "RO".into(),
            denumire_org_transport: "Transportator SRL".into(),
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
            denumire_strada: "Str. B".into(),
            numar: "2".into(),
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
fn etransport_validates_against_official_xsd() {
    let xsd_path = Path::new("tools/anaf/schema_ETR_v2.xsd");
    if !xsd_path.exists() {
        eprintln!("SKIP etransport_xsd: official XSD not vendored at {xsd_path:?}");
        return;
    }
    if !xmllint_available() {
        eprintln!("SKIP etransport_xsd: xmllint not available");
        return;
    }

    let decl = full_declaration();
    let val_errs = validate_etransport(&decl);
    assert!(
        val_errs.is_empty(),
        "fully-populated declaration must pass local validation, got: {val_errs:?}"
    );

    let xml = generate_etransport_xml(&decl).expect("generate_etransport_xml");
    eprintln!("Generated e-Transport XML ({} bytes):\n{xml}", xml.len());
    // The attributes the UI historically failed to send — required by the official XSD.
    for needle in [
        "denumireStrada=\"Str. A\"",
        "denumireStrada=\"Str. B\"",
        "codTaraOrgTransport=\"RO\"",
        "denumireOrgTransport=\"Transportator SRL\"",
    ] {
        assert!(
            xml.contains(needle),
            "XML must contain {needle}, got: {xml}"
        );
    }

    let tmp = std::env::temp_dir().join("etransport_xsd_test.xml");
    std::fs::write(&tmp, xml.as_bytes()).expect("write temp XML");
    let result = validate_with_xsd(xsd_path, &tmp).expect("validate_with_xsd (xmllint)");
    let _ = std::fs::remove_file(&tmp);

    assert!(
        result.passed,
        "e-Transport XML failed official XSD validation. Errors:\n{}",
        result.errors.join("\n")
    );
}
