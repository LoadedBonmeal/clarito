//! W8-3 VERIFY-FIRST probe: what does the REAL DUK (D101Validator.jar) accept for `d_rec`
//! on an an>=2024 (v10 dictionary) D101? Runs the bundled DUK on the emitted XML with the
//! d_rec / d_recN attributes patched to each candidate value and prints the verdicts.
//! Skips gracefully (like the saft_xsd.rs LocalBundle pattern) when the bundle is absent.
//!
//! Run: cargo test --test d101_drec_probe -- --nocapture

use std::path::PathBuf;

use efactura_desktop_lib::anaf_decl::d101_xml::{build_d101_xml, D101Header};
use efactura_desktop_lib::anaf_decl::duk::{run_duk, DukProvider, DukRuntime};
use efactura_desktop_lib::anaf_decl::DeclKind;

struct LocalBundle {
    java: PathBuf,
    jar_dir: PathBuf,
}
impl DukProvider for LocalBundle {
    fn resolve(&self) -> Option<DukRuntime> {
        if self.java.is_file()
            && self.jar_dir.join("DUKIntegrator.jar").is_file()
            && self.jar_dir.join("lib/D101Validator.jar").is_file()
        {
            Some(DukRuntime {
                java: self.java.clone(),
                jar_dir: self.jar_dir.clone(),
            })
        } else {
            None
        }
    }
}

fn bundle() -> LocalBundle {
    let res = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
    LocalBundle {
        java: res.join(if cfg!(windows) {
            "jre-min/bin/java.exe"
        } else {
            "jre-min/bin/java"
        }),
        jar_dir: res.join("duk"),
    }
}

fn header_2025() -> D101Header {
    D101Header {
        luna_i: 1,
        luna: 12,
        an: 2025,
        an_i: 2025,
        d_rec: 0,
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

#[test]
fn probe_d101_v10_d_rec_semantics() {
    let b = bundle();
    if b.resolve().is_none() {
        eprintln!("SKIP: bundled jre-min / D101Validator.jar not present");
        return;
    }
    let xml = build_d101_xml(&header_2025()).expect("build_d101_xml");
    assert!(
        xml.contains(r#"d_rec="2""#) && xml.contains(r#"d_recN="1""#),
        "baseline emitter output changed; probe patching would be wrong:\n{xml}"
    );

    // Patch (d_rec, d_recN) to each candidate and ask the real DUK.
    let candidates: &[(&str, &str)] = &[
        ("0", "1"),
        ("1", "1"),
        ("2", "1"),
        ("3", "1"),
        ("0", "0"),
        ("2", "0"),
        ("2", "2"),
    ];
    for (d_rec, d_rec_n) in candidates {
        let patched = xml
            .replace(r#"d_rec="2""#, &format!(r#"d_rec="{d_rec}""#))
            .replace(r#"d_recN="1""#, &format!(r#"d_recN="{d_rec_n}""#));
        let tmp = std::env::temp_dir().join(format!("d101_probe_drec{d_rec}_n{d_rec_n}.xml"));
        std::fs::write(&tmp, patched.as_bytes()).expect("write probe XML");
        let outcome = run_duk(&bundle(), DeclKind::D101, &tmp)
            .expect("run_duk must not fail")
            .expect("bundle resolved above");
        let _ = std::fs::remove_file(&tmp);
        eprintln!(
            "d_rec={d_rec} d_recN={d_rec_n} → passed={} errors={:?}",
            outcome.passed,
            outcome
                .errors
                .iter()
                .map(|e| e.message.clone())
                .collect::<Vec<_>>()
        );
    }
}
