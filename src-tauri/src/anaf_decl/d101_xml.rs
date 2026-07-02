//! D101 XML emitter — declarație privind impozitul pe profit.
//!
//! ## Namespace (PERIOD-DEPENDENT, confirmed via live DUKIntegrator D101Validator.jar, 2026-06):
//!   ≤2023  → mfp:anaf:dgti:d101:declaratie:v9
//!   ≥2024  → mfp:anaf:dgti:d101:declaratie:v10
//!
//! ## Atribute obligatorii identificate prin inginerie inversă DUK (v8 = 2024-12+):
//! - `Data_S` / `Data_I` — capital D și I/S, formatul DD.MM.YYYY
//! - `d_rec=2, d_recN=1` — DUK v8 impune acestea pentru declarația anuală (R2a)
//! - `d_recN` — interval fix [1,1]; împreună cu `d_rec=2` satisface R2a
//!
//! ## W8-3 — semantica `d_rec` în dicționarul v10 (an ≥ 2024), VERIFICATĂ cu DUK-ul real
//! (`tests/d101_drec_probe.rs`, D101Validator.jar, 2026-07):
//!   - `d_recN` are interval FIX [1,1] (0 și 2 sunt respinse: „nu se incadreaza in intervalul cerut");
//!   - regula R2a: „d_rec(x) =2 daca d_recN (1) =1" → d_rec=0 și d_rec=1 sunt RESPINSE,
//!     d_rec=3 e în afara intervalului; SINGURA combinație validă e `d_rec=2, d_recN=1`.
//! Deci pentru an ≥ 2024 atributul XML `d_rec` este o CONSTANTĂ STRUCTURALĂ a dicționarului și
//! NU mai codifică statutul de rectificativă. Rectificativa NU se poate semnala prin acest câmp;
//! evidența „originală vs. rectificativă" trebuie purtată separat (vezi
//! `commands::d101::D101ExportParams::is_rectificative`).
//! - `d_prof=0` — 0..2, obligatoriu
//! - `d_reg=0`  — 0..1, obligatoriu
//! - `temei=1`  — 1..2, obligatoriu
//! - `d_grup=1` — fix 1, obligatoriu pentru an ≥ 2022
//! - `cod_obligatie` — din lista ["102","103","104","105"] (102 = impozit pe profit anual)
//!   * R10.2: `trim_micro` TREBUIE absent dacă cod_obligatie="102"
//! - `Data_S` — data de sfârșit a perioadei (ex: "31.12.2025")
//! - `d_alte, d_anulare, d_succ, d_reglem` — 0..1, obligatorii
//!
//! NOTE: XSD vendored (d101_20250214.xsd) = v3 — nu coincide cu namespace-ul DUK. XSD-ul este
//! depășit; autoritatea de validare este DUKIntegrator (D101Validator.jar).
//!
//! DUK: java -jar DUKIntegrator.jar -v D101 <xml> <result>

use serde::{Deserialize, Serialize};

use crate::error::AppResult;

/// Header D101 (atribute obligatorii per DUK v8 + namespace period-dependent).
/// Namespace emis depinde de `an`: ≤2023 → v9, ≥2024 → v10.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct D101Header {
    /// Luna de început (1 pentru exerciciu fiscal standard ianuarie–decembrie).
    pub luna_i: u32,
    /// Luna de raportare (12 = declarație anuală).
    pub luna: u32,
    /// Anul de raportare.
    pub an: i32,
    /// Anul de început al exercițiului fiscal.
    pub an_i: i32,
    /// Atributul XML `d_rec`. ATENȚIE (W8-3, verificat cu DUK-ul real — vezi doc-ul modulului):
    /// - an ≥ 2024 (dicționar v10): câmpul e o CONSTANTĂ STRUCTURALĂ — emitem mereu `2`
    ///   (singura valoare acceptată, R2a + d_recN fix 1), indiferent de valoarea de aici.
    ///   NU codifică „rectificativă"; folosiți `D101ExportParams::is_rectificative` pentru evidență.
    /// - an ≤ 2023 (v9): emis ca atare, cu semantica istorică (0=normală, 1=inițială,
    ///   2=rectificativă).
    pub d_rec: u8,
    /// 0 = nu se anulează.
    pub d_anulare: u8,
    /// 0 = nu e succesor.
    pub d_succ: u8,
    /// 0 = nu alte situații.
    pub d_alte: u8,
    /// 0 = nu reglementare.
    pub d_reglem: u8,
    /// Data de început a exercițiului (DD.MM.YYYY) — emis ca `Data_I` în XML.
    pub data_i: String,
    /// Data de sfârșit a exercițiului (DD.MM.YYYY) — emis ca `Data_S` în XML.
    pub data_s: String,
    /// Codul obligației: "102"=profit anual, "103"/"104"="105"=trimestriale.
    /// DUK v8 acceptă doar: "102", "103", "104", "105".
    /// R10.2: când cod_obligatie="102", `trim_micro` TREBUIE absent.
    pub cod_obligatie: String,
    /// Scadența în format DDMMYY (ex: "250625" = 25 iunie 2025).
    pub scadenta: String,
    /// Codul bugetar (ex: "20470101").
    pub cod_bug: String,
    /// Numărul de evidență (string, 23 cifre; "0" dacă necunoscut — se validează DUK R18).
    pub nr_evid: String,
    /// Total de plată (lei, întreg) = impozit datorat – plăți anticipate.
    pub total_plata_a: i64,
    /// CIF (cifre, fără RO) — al declarantului.
    pub cif: String,
    /// Cod CAEN (4 cifre).
    pub caen: String,
    /// Denumirea firmei.
    pub denumire: String,
    /// Adresa firmei.
    pub adresa: String,
    /// Telefon (opțional).
    pub telefon: Option<String>,
    /// Fax (opțional).
    pub fax: Option<String>,
    /// Email (opțional).
    pub email: Option<String>,
    /// Numele declarantului.
    pub nume_declar: String,
    /// Prenumele declarantului.
    pub prenume_declar: String,
    /// Funcția declarantului.
    pub functie_declar: String,
    // Câmpurile P1-P56 (rândurile din declarație) — opționale, completate de contabil.
    // Câmpurile de bază (profit/pierdere):
    /// rd.1 venituri din exploatare
    pub p1: Option<i64>,
    /// rd.2 cheltuieli de exploatare
    pub p2: Option<i64>,
    /// rd.3 rezultat din exploatare (P1 - P2)
    pub p3: Option<i64>,
    /// rd.4 venituri financiare
    pub p4: Option<i64>,
    /// rd.5 cheltuieli financiare
    pub p5: Option<i64>,
    /// rd.6 rezultat financiar (P4 - P5) [R37]
    pub p6: Option<i64>,
    /// rd.7 rezultat brut (P3 + P6) [R38]
    pub p7: Option<i64>,
    /// rd.8 pierdere recuperată
    pub p8: Option<i64>,
    /// rd.9 baza impozabilă [R45: P10 = P7 + P8 - P9]
    pub p9: Option<i64>,
    /// rd.10 impozit pe profit (16% din P9)
    pub p10: Option<i64>,
    /// rd.11 sponsorizare deductibilă
    pub p11: Option<i64>,
    /// rd.12 impozit datorat (P10 - P11)
    pub p12: Option<i64>,
    /// rd.13 plăți anticipate
    pub p13: Option<i64>,
    /// rd.14 impozit de plată (P12 - P13)
    pub p14: Option<i64>,
    /// rd.15 impozit de recuperat
    pub p15: Option<i64>,
}

/// Returnează namespace-ul corect D101 pentru un anumit an de raportare.
/// Confirmat prin DUKIntegrator D101Validator.jar (iunie 2026):
///   ≤2023 → v9, ≥2024 → v10
pub fn d101_namespace_for_year(an: i32) -> &'static str {
    if an >= 2024 {
        "mfp:anaf:dgti:d101:declaratie:v10"
    } else {
        "mfp:anaf:dgti:d101:declaratie:v9"
    }
}

pub fn build_d101_xml(h: &D101Header) -> AppResult<String> {
    use crate::anaf_decl::xml_esc;

    let ns = d101_namespace_for_year(h.an);

    // DUK v10 (an ≥ 2024) requires d_recN=1 (fixed range [1,1]) together with d_rec=2 (R2a).
    // W8-3: verified against the REAL DUK (tests/d101_drec_probe.rs) — d_rec ∈ {0,1,3} and
    // d_recN ∈ {0,2} are all rejected; (2,1) is the ONLY valid pair. The attribute is therefore
    // a structural constant for ≥2024 and carries NO original-vs-rectificative meaning; the
    // caller's d_rec is honored verbatim only for ≤2023 (v9 semantics).
    let d_rec_val = if h.an >= 2024 { 2 } else { h.d_rec };
    let d_rec_n = 1u8; // fixed [1,1]

    // d_grup=1 required for an>=2022 (fixed value [1,1]).
    let d_grup = if h.an >= 2022 { "1" } else { "" };

    let mut attrs = format!(
        r#"xmlns="{ns}" d_recN="{d_rec_n}" d_prof="0" d_reg="0" temei="1"{d_grup_attr} luna_i="{}" luna="{}" an="{}" an_i="{}" d_rec="{d_rec_val}" d_anulare="{}" d_succ="{}" d_alte="{}" d_reglem="{}" Data_I="{}" Data_S="{}" cod_obligatie="{}" scadenta="{}" cod_bug="{}" nr_evid="{}" totalPlata_A="{}" cif="{}" caen="{}" denumire="{}" adresa="{}" nume_declar="{}" prenume_declar="{}" functie_declar="{}""#,
        h.luna_i,
        h.luna,
        h.an,
        h.an_i,
        h.d_anulare,
        h.d_succ,
        h.d_alte,
        h.d_reglem,
        xml_esc(&h.data_i),
        xml_esc(&h.data_s),
        xml_esc(&h.cod_obligatie),
        xml_esc(&h.scadenta),
        xml_esc(&h.cod_bug),
        xml_esc(&h.nr_evid),
        h.total_plata_a,
        xml_esc(&h.cif),
        xml_esc(&h.caen),
        xml_esc(&h.denumire),
        xml_esc(&h.adresa),
        xml_esc(&h.nume_declar),
        xml_esc(&h.prenume_declar),
        xml_esc(&h.functie_declar),
        d_grup_attr = if d_grup.is_empty() {
            String::new()
        } else {
            format!(r#" d_grup="{d_grup}""#)
        },
    );
    if let Some(t) = &h.telefon {
        if !t.is_empty() {
            attrs.push_str(&format!(r#" telefon="{}""#, xml_esc(t)));
        }
    }
    if let Some(f) = &h.fax {
        if !f.is_empty() {
            attrs.push_str(&format!(r#" fax="{}""#, xml_esc(f)));
        }
    }
    if let Some(m) = &h.email {
        if !m.is_empty() {
            attrs.push_str(&format!(r#" email="{}""#, xml_esc(m)));
        }
    }
    // P fields (only emit non-None)
    let p_fields: &[(Option<i64>, &str)] = &[
        (h.p1, "P1"),
        (h.p2, "P2"),
        (h.p3, "P3"),
        (h.p4, "P4"),
        (h.p5, "P5"),
        (h.p6, "P6"),
        (h.p7, "P7"),
        (h.p8, "P8"),
        (h.p9, "P9"),
        (h.p10, "P10"),
        (h.p11, "P11"),
        (h.p12, "P12"),
        (h.p13, "P13"),
        (h.p14, "P14"),
        (h.p15, "P15"),
    ];
    for (val, name) in p_fields {
        if let Some(v) = val {
            attrs.push_str(&format!(r#" {name}="{v}""#));
        }
    }

    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<declaratie101 {}/>\n",
        attrs
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> D101Header {
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
    fn build_d101_emits_namespace_and_root() {
        let xml = build_d101_xml(&header()).expect("build_d101_xml");
        // header() uses an=2025 → DUK-confirmed namespace is v10 for ≥2024
        assert!(xml.contains(r#"xmlns="mfp:anaf:dgti:d101:declaratie:v10""#));
        assert!(xml.contains("<declaratie101 "));
        assert!(xml.ends_with("/>\n"));
    }

    #[test]
    fn d101_namespace_period_routing() {
        assert_eq!(
            d101_namespace_for_year(2023),
            "mfp:anaf:dgti:d101:declaratie:v9"
        );
        assert_eq!(
            d101_namespace_for_year(2022),
            "mfp:anaf:dgti:d101:declaratie:v9"
        );
        assert_eq!(
            d101_namespace_for_year(2024),
            "mfp:anaf:dgti:d101:declaratie:v10"
        );
        assert_eq!(
            d101_namespace_for_year(2025),
            "mfp:anaf:dgti:d101:declaratie:v10"
        );
        assert_eq!(
            d101_namespace_for_year(2026),
            "mfp:anaf:dgti:d101:declaratie:v10"
        );
    }

    #[test]
    fn build_d101_emits_duk_required_attributes() {
        let xml = build_d101_xml(&header()).expect("build_d101_xml");
        // DUK v8: d_recN=1 and d_rec=2 are required (R2a rule)
        assert!(xml.contains(r#"d_recN="1""#), "d_recN=1 required by DUK v8");
        assert!(
            xml.contains(r#"d_rec="2""#),
            "d_rec=2 required for an>=2024 (R2a)"
        );
        // Data_I and Data_S must use capital letters
        assert!(
            xml.contains(r#"Data_I="01.01.2025""#),
            "Data_I (capital) required"
        );
        assert!(
            xml.contains(r#"Data_S="31.12.2025""#),
            "Data_S (capital) required"
        );
        // Other required DUK v8 attributes
        assert!(xml.contains(r#"d_prof="0""#));
        assert!(xml.contains(r#"d_reg="0""#));
        assert!(xml.contains(r#"temei="1""#));
        assert!(
            xml.contains(r#"d_grup="1""#),
            "d_grup=1 required for an>=2022"
        );
        // cod_obligatie=102 (annual profit tax)
        assert!(xml.contains(r#"cod_obligatie="102""#));
        // trim_micro must NOT be present for cod=102 (R10.2)
        assert!(
            !xml.contains("trim_micro"),
            "trim_micro must be absent for cod_obligatie=102"
        );
    }

    #[test]
    fn build_d101_emits_total_plata() {
        let xml = build_d101_xml(&header()).expect("build_d101_xml");
        assert!(xml.contains(r#"totalPlata_A="0""#));
    }

    /// W8-3 (DUK-verificat, vezi tests/d101_drec_probe.rs): pentru an ≥ 2024 dicționarul v10
    /// acceptă DOAR d_rec=2 (+ d_recN=1) — atributul e o constantă structurală, emisă indiferent
    /// de d_rec-ul apelantului. Pentru an ≤ 2023 (v9) d_rec-ul apelantului e emis ca atare.
    #[test]
    fn d_rec_structural_constant_for_2024_plus_verbatim_before() {
        for caller_d_rec in [0u8, 1, 2] {
            let mut h = header();
            h.d_rec = caller_d_rec;
            let xml = build_d101_xml(&h).expect("build_d101_xml");
            assert!(
                xml.contains(r#"d_rec="2""#) && xml.contains(r#"d_recN="1""#),
                "an=2025 (v10): singura pereche validă e d_rec=2/d_recN=1, \
                 indiferent de d_rec-ul apelantului ({caller_d_rec}); got:\n{xml}"
            );
        }
        // an ≤ 2023 (v9): valoarea apelantului e onorată.
        let mut h = header();
        h.an = 2023;
        h.an_i = 2023;
        h.data_i = "01.01.2023".into();
        h.data_s = "31.12.2023".into();
        h.d_rec = 1;
        let xml = build_d101_xml(&h).expect("build_d101_xml");
        assert!(
            xml.contains(r#"d_rec="1""#),
            "an=2023 (v9): d_rec-ul apelantului trebuie emis ca atare; got:\n{xml}"
        );
    }
}
