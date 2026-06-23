//! Dump one representative, DUK-VALID sample XML per declaration to `<out_dir>/`:
//!   D301.xml  D700.xml  D710.xml  D100.xml  D101.xml
//!
//! These samples use the SAME inputs as the green XSD/unit tests, so they are
//! guaranteed to pass both structural (xmllint) and business-rule (DUKIntegrator)
//! validation. The `scripts/verify-duk.sh` script drives this binary, then runs
//! the ANAF DUKIntegrator over each sample to confirm "Validare fara erori".
//!
//! Usage:
//!   cd src-tauri && cargo run --example dump_duk_samples -- /tmp/duk_samples
//!
//! The example is compiled by `cargo build --examples` / `cargo clippy --all-targets`
//! and must stay clippy-clean (no `allow` suppressions).

use efactura_desktop_lib::anaf_decl::d100_xml::{build_d100_xml, D100Header, D100Obligatie};
use efactura_desktop_lib::anaf_decl::d101_xml::{build_d101_xml, D101Header};
use efactura_desktop_lib::anaf_decl::d301_xml::{
    build_d301_xml, D301Data, D301Header, D301Sectiune,
};
use efactura_desktop_lib::anaf_decl::d700_xml::{build_d700_xml, D700Input};
use efactura_desktop_lib::anaf_decl::d710_xml::{
    build_d710_xml, D710Header, D710Input, D710Obligation,
};
use rust_decimal::Decimal;
use std::path::PathBuf;
use std::str::FromStr;

fn d(s: &str) -> Decimal {
    Decimal::from_str(s).expect("valid decimal literal")
}

// ── D301 sample (same inputs as d301_xsd.rs "all_sections" + "header") ─────

fn d301_header() -> D301Header {
    D301Header {
        cif: "12345674".into(),
        denumire: "Test SRL".into(),
        adresa: "Str. Exemplu nr. 1, București, Sector 1".into(),
        telefon: "0721000000".into(),
        fax: "".into(),
        email: "test@test.ro".into(),
        banca: "Banca Comerciala Romana".into(),
        cont: "RO49AAAA1B31007593840000".into(),
        pers_inreg: 1,
        nr_evid: 0, // auto-computed per DUK R16
        luna: 5,
        an: 2026,
        d_rec: 0,
        temei: 2, // DUK R5b: d_rec=0 → temei must be 2
        nume_declarant: "Popescu".into(),
        prenume_declarant: "Ion".into(),
        functia_declarant: "Administrator".into(),
    }
}

fn d301_data() -> D301Data {
    D301Data {
        sectiuni: vec![
            D301Sectiune {
                tip_operatie: 1,
                nr_doc: "FAC-001".into(),
                data_doc: "10.05.2026".into(),
                val_valuta: d("5000.00"),
                tip_valuta: "RON".into(),
                curs_valutar: d("1.0000"),
                baza: d("5000.00"),
                tva: d("950.00"),
            },
            D301Sectiune {
                tip_operatie: 2,
                nr_doc: "MT-001".into(),
                data_doc: "15.05.2026".into(),
                val_valuta: d("10000.00"),
                tip_valuta: "EUR".into(),
                curs_valutar: d("5.0200"),
                baza: d("50200.00"),
                tva: d("9538.00"),
            },
            D301Sectiune {
                tip_operatie: 3,
                nr_doc: "ACC-001".into(),
                data_doc: "18.05.2026".into(),
                val_valuta: d("2000.00"),
                tip_valuta: "EUR".into(),
                curs_valutar: d("5.0200"),
                baza: d("10040.00"),
                tva: d("1907.60"),
            },
            // tip_operatie=4 (intra-EU service, main row)
            D301Sectiune {
                tip_operatie: 4,
                nr_doc: "SRV-EU-001".into(),
                data_doc: "20.05.2026".into(),
                val_valuta: d("3000.00"),
                tip_valuta: "EUR".into(),
                curs_valutar: d("5.0200"),
                baza: d("15060.00"),
                tva: d("2861.40"),
            },
            // tip_operatie=4 (paired, art.307(3))
            D301Sectiune {
                tip_operatie: 4,
                nr_doc: "SRV-NEU-001".into(),
                data_doc: "22.05.2026".into(),
                val_valuta: d("1500.00"),
                tip_valuta: "USD".into(),
                curs_valutar: d("4.6300"),
                baza: d("6945.00"),
                tva: d("1319.55"),
            },
            // tip_operatie=5: DUK R32.1 — MUST be an exact duplicate of a tip=4 row
            D301Sectiune {
                tip_operatie: 5,
                nr_doc: "SRV-NEU-001".into(),
                data_doc: "22.05.2026".into(),
                val_valuta: d("1500.00"),
                tip_valuta: "USD".into(),
                curs_valutar: d("4.6300"),
                baza: d("6945.00"),
                tva: d("1319.55"),
            },
        ],
    }
}

// ── D700 sample (same inputs as d700_xml.rs `input_ab`) ──────────────────────

fn d700_input() -> D700Input {
    D700Input {
        luna: Some(6),
        an: Some(2026),
        fel_d: None,
        dec_inreg: Some("010".into()),
        total_plata_a: 0, // auto-computed
        cif: "12345674".into(),
        den: "Test SRL".into(),
        nume_decl: Some("Popescu".into()),
        pren_decl: Some("Ion".into()),
        func_decl: Some("Administrator".into()),
        bifa_a: true,
        bifa_b: true,
        bifa_c: false,
        bifa_d: false,
        bifa_f: false,
        bifa_g: false,
        bifa_3b: false,
        bifa_b3: false,
        bifa_b11: false,
        bifa11_3b: false,
        data_3b: None,
        bifa_b8: true,    // DUK R51: Bifa_B sub-bifa (alte obligatii sect. B)
        bifa_8b: Some(1), // DUK R125: Bifa_8b=1 (required when Bifa_B8=1)
        sect_a: None,
        sect_b: None,
        sect_c: None,
        sect_d: None,
    }
}

// ── D710 sample (same inputs as d710_xsd.rs "two_obligations" + header(3,2026)) ─

fn d710_header() -> D710Header {
    D710Header {
        cui: "12345674".into(),
        den: "Test SRL".into(),
        adresa: "Str. Exemplu nr. 1, București, Sector 1".into(),
        luna: 3,
        an: 2026,
        d_anulare: 0,
        rectificativa: false,
        temei: None,
        telefon: None,
        fax: None,
        mail: None,
        cif_r: None,
        den_r: None,
        adr_r: None,
        tel_r: None,
        fax_r: None,
        email_r: None,
        cif_s: None,
        d_succ: None,
        d_dizolv: None,
        d_energie: None,
        d_modif: None,
        nume_declar: "Popescu".into(),
        prenume_declar: "Ion".into(),
        functie_declar: "Administrator".into(),
    }
}

fn d710_obligations() -> Vec<D710Obligation> {
    vec![
        D710Obligation {
            cod_oblig: 103,
            cod_bugetar: "20470101".into(),
            scadenta: "25.04.2026".into(),
            nr_evid: 0, // auto-computed per compute_nr_evid_d710
            suma_dat_I: Some(d("8000")),
            suma_dat_C: Some(d("10000")),
            suma_plata_I: Some(d("8000")),
            suma_plata_C: Some(d("10000")),
            ..Default::default()
        },
        D710Obligation {
            cod_oblig: 121,
            cod_bugetar: "20470101".into(),
            scadenta: "25.04.2026".into(),
            nr_evid: 0, // auto-computed per compute_nr_evid_d710
            suma_dat_I: Some(d("1800")),
            suma_dat_C: Some(d("2000")),
            suma_plata_I: Some(d("1800")),
            suma_plata_C: Some(d("2000")),
            cota: Some(1), // DUK R17: cod_oblig=121 (micro) requires cota impozitare
            ..Default::default()
        },
    ]
}

// ── D100 sample (same inputs as d100_xsd.rs `test_header`) ──────────────────

fn d100_header() -> D100Header {
    D100Header {
        luna: 3,
        an: 2026,
        d_anulare: 0,
        cui: "12345674".into(),
        den: "Test SRL".into(),
        adresa: "Str. Exemplu nr. 1, Bucuresti".into(),
        telefon: None,
        fax: None,
        email: None,
        nume_declar: "Popescu".into(),
        prenume_declar: "Ion".into(),
        functie_declar: "Administrator".into(),
        obligatii: vec![D100Obligatie {
            cod_oblig: 121,
            cod_bugetar: "20470101".into(),
            scadenta: "25.04.2026".into(),
            nr_evid: 0, // auto-computed to 23-char via D710 algorithm
            suma_dat: Some(d("1000")),
            suma_ded: None,
            suma_plata: Some(d("1000")),
            suma_rest: None,
            cota: None, // auto-filled to 1 for cod_oblig=121 (DUK Rcota)
            suma_redu: None,
        }],
    }
}

// ── D101 sample (same inputs as d101_xsd.rs `test_header`) ──────────────────

fn d101_header() -> D101Header {
    D101Header {
        luna_i: 1,
        luna: 12,
        an: 2025,
        an_i: 2025,
        d_rec: 0, // overridden to 2 for an>=2024 by build_d101_xml (DUK R2a)
        d_anulare: 0,
        d_succ: 0,
        d_alte: 0,
        d_reglem: 0,
        data_i: "01.01.2025".into(),
        data_s: "31.12.2025".into(),
        cod_obligatie: "102".into(),
        scadenta: "250625".into(),
        cod_bug: "20470101".into(),
        nr_evid: "10102011225250625000035".into(),
        total_plata_a: 0,
        cif: "12345674".into(),
        caen: "6201".into(),
        denumire: "Test SRL".into(),
        adresa: "Str. Exemplu nr. 1, Bucuresti".into(),
        telefon: None,
        fax: None,
        email: None,
        nume_declar: "Popescu".into(),
        prenume_declar: "Ion".into(),
        functie_declar: "Administrator".into(),
        p1: None,
        p2: None,
        p3: None,
        p4: None,
        p5: None,
        p6: None,
        p7: None,
        p8: None,
        p9: None,
        p10: None,
        p11: None,
        p12: None,
        p13: None,
        p14: None,
        p15: None,
    }
}

// ── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: dump_duk_samples <out_dir>");
        std::process::exit(1);
    }
    let out_dir = PathBuf::from(&args[1]);
    std::fs::create_dir_all(&out_dir).expect("create out_dir");

    // D301
    let d301_xml = build_d301_xml(&d301_header(), &d301_data()).expect("build_d301_xml");
    let p = out_dir.join("D301.xml");
    std::fs::write(&p, d301_xml.as_bytes()).expect("write D301.xml");
    println!("Written: {}", p.display());

    // D700
    let d700_xml = build_d700_xml(&d700_input()).expect("build_d700_xml");
    let p = out_dir.join("D700.xml");
    std::fs::write(&p, d700_xml.as_bytes()).expect("write D700.xml");
    println!("Written: {}", p.display());

    // D710
    let d710_xml = build_d710_xml(&D710Input {
        header: d710_header(),
        obligations: d710_obligations(),
    })
    .expect("build_d710_xml");
    let p = out_dir.join("D710.xml");
    std::fs::write(&p, d710_xml.as_bytes()).expect("write D710.xml");
    println!("Written: {}", p.display());

    // D100
    let d100_xml = build_d100_xml(&d100_header()).expect("build_d100_xml");
    let p = out_dir.join("D100.xml");
    std::fs::write(&p, d100_xml.as_bytes()).expect("write D100.xml");
    println!("Written: {}", p.display());

    // D101
    let d101_xml = build_d101_xml(&d101_header()).expect("build_d101_xml");
    let p = out_dir.join("D101.xml");
    std::fs::write(&p, d101_xml.as_bytes()).expect("write D101.xml");
    println!("Written: {}", p.display());

    println!("All 5 samples written to {}", out_dir.display());
}
