//! D100 XML emitter — declarație privind obligațiile de plată la bugetul de stat.
//! Namespace: mfp:anaf:dgti:d100:declaratie:v2 (XSD: tools/anaf/d100_24022022.xsd).
//! DUK: java -jar DUKIntegrator.jar -v D100 <xml> <result> via lib/D100Validator.jar.
//!
//! ## DUK business rules implemented here
//! - **R11b**: totalPlata_A = Σ(suma_dat + suma_ded + suma_plata + suma_rest) across ALL obligations
//!   (all non-null sums are added together, not just suma_plata).
//! - **R16**: nr_evid must be exactly 23 characters. If 0 is passed, the value is auto-computed
//!   using the same algorithm as D710 (`compute_nr_evid_d710`).
//! - **Rcota / R17**: For cod_oblig=121 (impozit pe veniturile microîntreprinderilor), the `cota`
//!   attribute must be present and equal to 1 (cota=1 means 1%; cota=3 means 3%). Other obligations
//!   that require a cota must also set it explicitly. The DUK R17 rule rejects documents where
//!   `cota` is missing when cod_oblig=121.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::anaf_decl::d710_xml::compute_nr_evid_d710;
use crate::anaf_decl::round_lei;
use crate::error::AppResult;

// ── Cota logic ────────────────────────────────────────────────────────────────

/// Returnează `cota` obligatorie per regulile DUK D100, sau `None` dacă nu se aplică.
///
/// - cod_oblig 121 (impozit pe veniturile microîntreprinderilor): cota = 1 (1%) obligatorie per
///   DUK Rcota + R17. Valoarea `1` corespunde cotei de 1%; valoarea `3` ar corespunde cotei de 3%
///   (reglementare istorică), dar cota curentă este 1.
/// - Toate celelalte coduri: nu impun cota prin regulile DUK (poate fi None sau specificată manual).
pub fn required_cota_for_cod(cod_oblig: u32) -> Option<u8> {
    match cod_oblig {
        121 => Some(1), // impozit micro — DUK Rcota + R17: cota must be 1
        _ => None,
    }
}

// ── Model date ────────────────────────────────────────────────────────────────

/// O obligație de plată din D100.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct D100Obligatie {
    /// Codul obligației din nomenclatorul D100 (ex: 121 = micro 1%, 102 = impozit profit Q).
    pub cod_oblig: u32,
    /// Codul bugetar (ex: "20470101").
    pub cod_bugetar: String,
    /// Scadența în formatul DD.MM.YYYY.
    pub scadenta: String,
    /// Numărul de evidență a plății (23 caractere). Dacă este 0, se calculează automat
    /// folosind algoritmul DUK R16 (identic cu D710).
    pub nr_evid: u64,
    /// Suma datorată (lei, întreg ≥ 0).
    pub suma_dat: Option<Decimal>,
    /// Suma dedusă (lei, întreg ≥ 0).
    pub suma_ded: Option<Decimal>,
    /// Suma de plată (lei, întreg ≥ 0).
    pub suma_plata: Option<Decimal>,
    /// Suma de restituit (lei, întreg ≥ 0).
    pub suma_rest: Option<Decimal>,
    /// Cota aplicabilă (1 sau 3, per Int_listaCoteSType din XSD).
    /// Dacă este None, se completează automat pe baza cod_oblig (ex: 121 → 1).
    #[serde(default)]
    pub cota: Option<u8>,
    /// Suma de reducere (lei, întreg ≥ 0, opțional).
    #[serde(default)]
    pub suma_redu: Option<Decimal>,
}

/// Header-ul declarației D100.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct D100Header {
    /// Luna perioadei de raportare (1-12; pentru Q1 = 3, Q2 = 6 etc.).
    pub luna: u32,
    /// Anul perioadei de raportare.
    pub an: i32,
    /// 0 = declarație normală, 1 = declarație de anulare.
    pub d_anulare: u8,
    /// CUI-ul firmei (doar cifre).
    pub cui: String,
    /// Denumirea firmei.
    pub den: String,
    /// Adresa firmei.
    pub adresa: String,
    /// Telefon (opțional).
    pub telefon: Option<String>,
    /// Fax (opțional).
    pub fax: Option<String>,
    /// Email (opțional) — atribut XML `mai` (per XSD d100).
    pub email: Option<String>,
    /// Numele declarantului.
    pub nume_declar: String,
    /// Prenumele declarantului.
    pub prenume_declar: String,
    /// Funcția declarantului.
    pub functie_declar: String,
    /// Lista obligațiilor de plată.
    pub obligatii: Vec<D100Obligatie>,
}

// ── Helper: parsează scadența "ZZ.LL.AAAA" → (luna, an) ──────────────────────

fn parse_scadenta(s: &str) -> Option<(u32, u32)> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() == 3 {
        let mm = parts[1].parse::<u32>().ok()?;
        let yy = parts[2].parse::<u32>().ok()? % 100;
        Some((mm, yy))
    } else {
        None
    }
}

// ── Emitorul XML ──────────────────────────────────────────────────────────────

/// Construiește XML-ul D100 conform OPANAF 57/2026.
///
/// ## Reguli DUK implementate
/// - **R11b**: totalPlata_A = Σ(suma_dat + suma_ded + suma_plata + suma_rest) — TOATE câmpurile sumă.
/// - **R16**: nr_evid 23 de cifre (calculat automat dacă nr_evid=0).
/// - **Rcota / R17**: cota=1 obligatorie pentru cod_oblig=121 (micro 1%).
pub fn build_d100_xml(h: &D100Header) -> AppResult<String> {
    use crate::anaf_decl::xml_esc;

    // DUK R11b: totalPlata_A = Σ(suma_dat + suma_ded + suma_plata + suma_rest) pentru TOATE
    // obligațiile. ATENȚIE: nu doar suma_plata, ci SUMA tuturor câmpurilor sumă non-nule.
    let total_plata: i64 = h
        .obligatii
        .iter()
        .map(|o| {
            let mut s = 0i64;
            if let Some(v) = o.suma_dat {
                s += round_lei(v);
            }
            if let Some(v) = o.suma_ded {
                s += round_lei(v);
            }
            if let Some(v) = o.suma_plata {
                s += round_lei(v);
            }
            if let Some(v) = o.suma_rest {
                s += round_lei(v);
            }
            s
        })
        .sum::<i64>()
        .max(0);

    let mut children = String::new();
    for ob in &h.obligatii {
        // DUK R16: nr_evid trebuie să aibă lungimea de 23 cifre.
        // Dacă nr_evid == 0, calculăm automat folosind algoritmul D710 (identic).
        let nr_evid_s = if ob.nr_evid == 0 {
            let (scad_mm, scad_yy) = parse_scadenta(ob.scadenta.trim())
                .unwrap_or((h.luna % 12 + 1, (h.an % 100) as u32));
            compute_nr_evid_d710(h.luna, h.an, ob.cod_oblig, scad_mm, scad_yy, 0)
        } else {
            format!("{:023}", ob.nr_evid)
        };

        // DUK Rcota + R17: cota este obligatorie pentru anumite cod_oblig (ex: 121 → 1).
        // Dacă cota este None dar cod_oblig impune una, o calculăm automat.
        let effective_cota = ob.cota.or_else(|| required_cota_for_cod(ob.cod_oblig));

        let mut attrs = format!(
            r#" cod_oblig="{}" cod_bugetar="{}" scadenta="{}" nr_evid="{}""#,
            ob.cod_oblig,
            xml_esc(&ob.cod_bugetar),
            xml_esc(&ob.scadenta),
            nr_evid_s,
        );
        if let Some(v) = ob.suma_dat {
            attrs.push_str(&format!(r#" suma_dat="{}""#, round_lei(v)));
        }
        if let Some(v) = ob.suma_ded {
            attrs.push_str(&format!(r#" suma_ded="{}""#, round_lei(v)));
        }
        if let Some(v) = ob.suma_plata {
            attrs.push_str(&format!(r#" suma_plata="{}""#, round_lei(v)));
        }
        if let Some(v) = ob.suma_rest {
            attrs.push_str(&format!(r#" suma_rest="{}""#, round_lei(v)));
        }
        if let Some(c) = effective_cota {
            attrs.push_str(&format!(r#" cota="{}""#, c));
        }
        if let Some(v) = ob.suma_redu {
            attrs.push_str(&format!(r#" suma_redu="{}""#, round_lei(v)));
        }
        children.push_str(&format!("  <obligatie{}/>\n", attrs));
    }

    let mut root_attrs = format!(
        r#"xmlns="mfp:anaf:dgti:d100:declaratie:v2" luna="{}" an="{}" d_anulare="{}" cui="{}" den="{}" adresa="{}" totalPlata_A="{}" nume_declar="{}" prenume_declar="{}" functie_declar="{}""#,
        h.luna,
        h.an,
        h.d_anulare,
        xml_esc(&h.cui),
        xml_esc(&h.den),
        xml_esc(&h.adresa),
        total_plata,
        xml_esc(&h.nume_declar),
        xml_esc(&h.prenume_declar),
        xml_esc(&h.functie_declar),
    );
    if let Some(t) = &h.telefon {
        if !t.is_empty() {
            root_attrs.push_str(&format!(r#" telefon="{}""#, xml_esc(t)));
        }
    }
    if let Some(f) = &h.fax {
        if !f.is_empty() {
            root_attrs.push_str(&format!(r#" fax="{}""#, xml_esc(f)));
        }
    }
    if let Some(m) = &h.email {
        if !m.is_empty() {
            // XSD attribute is named "mai" (not "email") per d100_24022022.xsd
            root_attrs.push_str(&format!(r#" mai="{}""#, xml_esc(m)));
        }
    }

    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<declaratie100 {root_attrs}>\n{children}</declaratie100>\n"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn header() -> D100Header {
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
                nr_evid: 0,
                suma_dat: Some(d("1000")),
                suma_ded: None,
                suma_plata: Some(d("1000")),
                suma_rest: None,
                cota: None, // auto-filled to 1 for cod_oblig=121
                suma_redu: None,
            }],
        }
    }

    #[test]
    fn total_plata_sums_all_fields_r11b() {
        // R11b: totalPlata_A = Σ(suma_dat + suma_ded + suma_plata + suma_rest)
        // For this header: suma_dat=1000 + suma_plata=1000 = 2000
        let xml = build_d100_xml(&header()).expect("build_d100_xml");
        assert!(
            xml.contains(r#"totalPlata_A="2000""#),
            "R11b: totalPlata_A must be Σ(all sum fields), got: {xml}"
        );
        assert!(xml.contains(r#"suma_plata="1000""#));
        assert!(xml.contains(r#"xmlns="mfp:anaf:dgti:d100:declaratie:v2""#));
        assert!(xml.contains("<declaratie100 "));
        assert!(xml.contains("</declaratie100>"));
    }

    #[test]
    fn nr_evid_auto_computed_23_chars_r16() {
        // R16: nr_evid must be exactly 23 digits when nr_evid=0 (auto-computed via D710 algorithm)
        let xml = build_d100_xml(&header()).expect("build_d100_xml");
        // Find the nr_evid attribute value
        let start = xml.find(r#"nr_evid=""#).expect("nr_evid attr") + 9;
        let end = xml[start..].find('"').expect("closing quote") + start;
        let nr_evid_val = &xml[start..end];
        assert_eq!(
            nr_evid_val.len(),
            23,
            "R16: nr_evid must be 23 chars, got {:?} ({})",
            nr_evid_val,
            nr_evid_val.len()
        );
        assert!(
            nr_evid_val.chars().all(|c| c.is_ascii_digit()),
            "nr_evid must be all digits: {nr_evid_val}"
        );
    }

    #[test]
    fn cota_1_auto_emitted_for_cod_121_rcota() {
        // Rcota + R17: cota=1 required for cod_oblig=121 (micro), auto-filled when cota=None
        let xml = build_d100_xml(&header()).expect("build_d100_xml");
        assert!(
            xml.contains(r#"cota="1""#),
            "Rcota: cota=1 must be auto-emitted for cod_oblig=121: {xml}"
        );
    }

    #[test]
    fn explicit_cota_overrides_auto() {
        let mut h = header();
        h.obligatii[0].cota = Some(3); // manual override
        let xml = build_d100_xml(&h).expect("build_d100_xml");
        assert!(
            xml.contains(r#"cota="3""#),
            "explicit cota=3 must appear: {xml}"
        );
    }

    #[test]
    fn multiple_obligatii_sum_all_fields_correctly() {
        let mut h = header();
        // first obligation: suma_dat=1000 + suma_plata=1000 = 2000
        h.obligatii.push(D100Obligatie {
            cod_oblig: 102,
            cod_bugetar: "20030101".into(),
            scadenta: "25.04.2026".into(),
            nr_evid: 0,
            suma_dat: Some(d("2000")),
            suma_ded: Some(d("500")),
            suma_plata: Some(d("1500")),
            suma_rest: None,
            cota: None,
            suma_redu: None,
        });
        // total: 1000 + 1000 + 2000 + 500 + 1500 = 6000
        let xml = build_d100_xml(&h).expect("build_d100_xml multi");
        assert!(
            xml.contains(r#"totalPlata_A="6000""#),
            "R11b multi: expected 6000, got: {xml}"
        );
    }

    #[test]
    fn xml_esc_applied_to_den() {
        let mut h = header();
        h.den = "Test & SRL <>&".into();
        let xml = build_d100_xml(&h).expect("build_d100_xml esc");
        assert!(xml.contains("Test &amp; SRL &lt;&gt;&amp;"));
        assert!(!xml.contains("Test & SRL"));
    }

    #[test]
    fn nr_evid_explicit_zero_padded_to_23() {
        let mut h = header();
        h.obligatii[0].nr_evid = 42;
        let xml = build_d100_xml(&h).expect("build_d100_xml");
        assert!(
            xml.contains(r#"nr_evid="00000000000000000000042""#),
            "nr_evid explicit must be zero-padded to 23: {xml}"
        );
    }

    #[test]
    fn required_cota_cod_121_is_1() {
        assert_eq!(required_cota_for_cod(121), Some(1));
    }

    #[test]
    fn required_cota_other_codes_is_none() {
        assert_eq!(required_cota_for_cod(102), None);
        assert_eq!(required_cota_for_cod(103), None);
        assert_eq!(required_cota_for_cod(150), None);
    }
}
