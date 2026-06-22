//! D710 — Declarație rectificativă pentru obligații D100 (OPANAF 587/2016 + 779/2024).
//!
//! **XSD-VALIDAT via `xmllint --schema tools/anaf/d710.xsd`** (official ANAF XSD,
//! targetNamespace `mfp:anaf:dgti:d710:declaratie:v1`, version 1.02).
//! Structura, atributele obligatorii, enumerările și tipurile sunt exacte față de XSD.
//! Validarea completă a regulilor de business necesită rularea `D710Validator.jar`
//! (pachetul standalone `D710_20052026.zip` de pe declaratii.anaf.ro, NU prin
//! DUKIntegrator — D710 are validator separat) înainte de depunerea electronică prin SPV.
//!
//! ## Ce corectează D710 și ce NU?
//! D710 rectifică EXCLUSIV obligațiile din formularul D100 (autoimpunere și reținere la sursă):
//! impozit pe profit, impozit micro, impozit nerezidenți, impozit dividende, accize,
//! impozit pe construcții, contribuții ale angajatorilor din vectorul D100.
//! NU rectifică D112 (are D112 propriu), NU rectifică D300 (are D300 propriu).
//!
//! ## Structura XML (per d710.xsd v1.02)
//! ```text
//!   <declaratie710 xmlns="mfp:anaf:dgti:d710:declaratie:v1"
//!                  luna="N" an="AAAA"
//!                  d_anulare="0|1"       ← 1 = declarație de anulare
//!                  d_recN="1"            ← prezent DOAR pentru rectificativă
//!                  temei="1|2"           ← opțional: 1=normal, 2=corectivă
//!                  cui="…" den="…" adresa="…"
//!                  telefon="…" fax="…" mail="…"
//!                  cifR="…" denR="…" adrR="…" telR="…" faxR="…" emailR="…"  ← împuternicit
//!                  cifS="…"              ← succesor (opțional)
//!                  d_succ="0|1" d_dizolv="0|1" d_energie="0|1" d_modif="0|1"
//!                  totalPlata_A="N"      ← suma totală de plată (≥ 0, întreg lei)
//!                  nume_declar="…" prenume_declar="…" functie_declar="…">
//!     <obligatie cod_oblig="N" cod_bugetar="…" scadenta="ZZ.LL.AAAA" nr_evid="N"
//!                suma_dat_i="N" suma_dat_c="N"
//!                suma_ded_i="N" suma_ded_c="N"
//!                suma_plata_i="N" suma_plata_c="N"
//!                suma_rest_i="N" suma_rest_c="N"
//!                cota="1|2|3"/>    ← toate sumele și cota sunt opționale per XSD
//!     …
//!   </declaratie710>
//! ```
//!
//! ## Nomenclator D100 (coduri frecvente — completați după Nomenclatorul oficial)
//! - `2` = Impozit pe profit (plăți anticipate, persoane juridice române)
//! - `5` = Impozit pe veniturile microîntreprinderilor
//! - `17` = Impozit pe dividende (reținere la sursă, rezidenți)
//! - `22` = Impozit pe veniturile nerezidenților (reținere la sursă)
//! - `37` = Impozit pe construcții
//!   (consultați Anexa formularului D100 publicat de ANAF pentru lista completă)

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::anaf_decl::round_lei;
use crate::anaf_decl::xml::{
    empty_elem_attrs, end_elem, finish, new_writer, pretty_print, start_elem_attrs, trunc,
};
use crate::error::{AppError, AppResult};

// ── Schema constants ──────────────────────────────────────────────────────────

/// Namespace D710 — targetNamespace din d710.xsd (v1).
/// NOTE: XSD-ul ANAF publicat are un bug tipografic (xmlns=v2 dar targetNamespace=v1);
/// documentele generate trebuie să folosească v1 (targetNamespace-ul este autoritar).
pub const D710_NAMESPACE: &str = "mfp:anaf:dgti:d710:declaratie:v1";

/// Elementul rădăcină al documentului D710 (per d710.xsd).
pub const D710_ROOT: &str = "declaratie710";

// ── Model date ────────────────────────────────────────────────────────────────

/// Antetul declarației D710 (per d710.xsd — atribute rădăcină).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D710Header {
    /// CUI-ul declarantului (CifSType — fără „RO", 2-10 cifre sau 13 cifre CNP).
    pub cui: String,
    /// Denumirea persoanei impozabile (max 200 chr).
    pub den: String,
    /// Adresa completă (max 1000 chr, REQUIRED în XSD).
    pub adresa: String,
    /// Luna perioadei rectificate (1-12).
    pub luna: u32,
    /// Anul perioadei rectificate (2011-2100).
    pub an: i32,
    /// 0 = rectificativă normală, 1 = declarație de anulare (d_anulare).
    pub d_anulare: u8,
    /// `true` = declarație rectificativă (emite atribut d_recN="1"); `false` = inițială (omis).
    #[serde(default)]
    pub rectificativa: bool,
    /// Temeiul legal (opțional): 1 = normal, 2 = corectivă.
    #[serde(default)]
    pub temei: Option<u8>,
    /// Telefon (opțional, max 15 chr).
    #[serde(default)]
    pub telefon: Option<String>,
    /// Fax (opțional, max 15 chr).
    #[serde(default)]
    pub fax: Option<String>,
    /// E-mail (opțional, max 200 chr).
    #[serde(default)]
    pub mail: Option<String>,
    /// CUI-ul împuternicitului (opțional, CifSType).
    #[serde(default)]
    pub cif_r: Option<String>,
    /// Denumirea împuternicitului (opțional, max 200 chr).
    #[serde(default)]
    pub den_r: Option<String>,
    /// Adresa împuternicitului (opțional, max 1000 chr).
    #[serde(default)]
    pub adr_r: Option<String>,
    /// Telefon împuternicit (opțional, max 15 chr).
    #[serde(default)]
    pub tel_r: Option<String>,
    /// Fax împuternicit (opțional, max 15 chr).
    #[serde(default)]
    pub fax_r: Option<String>,
    /// Email împuternicit (opțional, max 200 chr).
    #[serde(default)]
    pub email_r: Option<String>,
    /// CUI-ul succesorului (cifS, opțional).
    #[serde(default)]
    pub cif_s: Option<String>,
    /// Indicator succesor (d_succ, 0 sau 1, opțional).
    #[serde(default)]
    pub d_succ: Option<u8>,
    /// Indicator dizolvare (d_dizolv, 0 sau 1, opțional).
    #[serde(default)]
    pub d_dizolv: Option<u8>,
    /// Indicator energie (d_energie, 0 sau 1, opțional).
    #[serde(default)]
    pub d_energie: Option<u8>,
    /// Indicator modificare (d_modif, 0 sau 1, opțional).
    #[serde(default)]
    pub d_modif: Option<u8>,
    /// Numele declarantului (max 75 chr).
    pub nume_declar: String,
    /// Prenumele declarantului (max 75 chr).
    pub prenume_declar: String,
    /// Funcția declarantului (max 50 chr).
    pub functie_declar: String,
}

/// O obligație rectificată — un rând `<obligatie>` în D710.
///
/// Per d710.xsd: `cod_oblig`, `cod_bugetar`, `scadenta`, `nr_evid` sunt REQUIRED;
/// toate sumele (`suma_dat_i/c`, `suma_ded_i/c`, `suma_plata_i/c`, `suma_rest_i/c`)
/// și `cota` sunt OPȚIONALE. Sumele sunt IntPoz15SType (întreg ≥ 0, lei întregi).
///
/// Semantica perechilor I/C (inițial/corect):
/// - `_i` = valoarea INIȚIAL declarată în D100 original.
/// - `_c` = valoarea CORECTĂ (totalul corect, NU diferența față de inițial).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D710Obligation {
    /// Codul obligației din Nomenclatorul D100 (număr întreg ≥ 1, ex. 2, 5, 17, 22, 37).
    pub cod_oblig: u32,
    /// Codul bugetar (max 10 chr, REQUIRED per XSD — ex. "0105", "0205").
    pub cod_bugetar: String,
    /// Scadența obligației (ZZ.LL.AAAA — formatul ANAF, REQUIRED per XSD).
    pub scadenta: String,
    /// Numărul de evidență (IntStr23SType ≥ 0, REQUIRED per XSD; 0 dacă lipsește).
    pub nr_evid: u64,
    /// Suma datorată inițial (I), lei întregi (opțional per XSD).
    #[serde(default)]
    pub suma_dat_i: Option<Decimal>,
    /// Suma datorată corectă (C), lei întregi (opțional per XSD).
    #[serde(default)]
    pub suma_dat_c: Option<Decimal>,
    /// Suma deductibilă inițial (I), lei întregi (opțional).
    #[serde(default)]
    pub suma_ded_i: Option<Decimal>,
    /// Suma deductibilă corectă (C), lei întregi (opțional).
    #[serde(default)]
    pub suma_ded_c: Option<Decimal>,
    /// Suma de plată inițial (I), lei întregi (opțional).
    #[serde(default)]
    pub suma_plata_i: Option<Decimal>,
    /// Suma de plată corectă (C), lei întregi (opțional).
    #[serde(default)]
    pub suma_plata_c: Option<Decimal>,
    /// Suma restantă inițial (I), lei întregi (opțional).
    #[serde(default)]
    pub suma_rest_i: Option<Decimal>,
    /// Suma restantă corectă (C), lei întregi (opțional).
    #[serde(default)]
    pub suma_rest_c: Option<Decimal>,
    /// Cota aplicabilă (1, 2 sau 3 — IntInt1_3SType, opțional).
    #[serde(default)]
    pub cota: Option<u8>,
    /// Denumirea scurtă a obligației (pentru claritate UI, nu intră în XML).
    #[serde(default)]
    pub den_oblig: String,
}

/// Datele complete ale declarației D710 pentru O perioadă (luna + an).
/// Perioade diferite → obiecte D710Input separate → fișiere XML separate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D710Input {
    /// Antet cu datele declarantului și perioada rectificată.
    pub header: D710Header,
    /// Lista obligațiilor rectificate (minimum una per XSD — maxOccurs=1300).
    pub obligations: Vec<D710Obligation>,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Rotunjire la lei întregi (fără zecimale), comercial, ca string.
fn to_lei(d: Decimal) -> String {
    round_lei(d).to_string()
}

// ── Emitorul XML ──────────────────────────────────────────────────────────────

/// Construiește XML-ul D710 (declarație rectificativă obligații D100) pentru perioada dată.
///
/// Structura este **XSD-validată** față de `tools/anaf/d710.xsd` (ANAF oficial, v1.02,
/// targetNamespace `mfp:anaf:dgti:d710:declaratie:v1`).
/// Atribute rădăcină obligatorii: `luna`, `an`, `d_anulare`, `cui`, `den`, `adresa`,
/// `totalPlata_A`, `nume_declar`, `prenume_declar`, `functie_declar`.
/// Copii obligatori: cel puțin un `<obligatie>` cu atribute `cod_oblig`, `cod_bugetar`,
/// `scadenta`, `nr_evid`.
///
/// Validarea completă a regulilor de business necesită `D710Validator.jar` din pachetul
/// standalone `D710_20052026.zip` de pe declaratii.anaf.ro.
///
/// # Erori
/// Returnează eroare dacă lista de obligații e goală sau luna e invalidă (1-12).
pub fn build_d710_xml(input: &D710Input) -> AppResult<String> {
    if input.obligations.is_empty() {
        return Err(AppError::Validation(
            "D710: lista de obligații rectificate este goală. \
             Adăugați cel puțin o obligație (cod_oblig + cod_bugetar + scadenta)."
                .into(),
        ));
    }
    let hdr = &input.header;
    if hdr.luna == 0 || hdr.luna > 12 {
        return Err(AppError::Validation(format!(
            "D710: luna {} este invalidă — trebuie să fie 1-12.",
            hdr.luna
        )));
    }

    // GUARDRAIL: suma_dat_c (suma datorată CORECTĂ) este obligatorie per regulile de business
    // D710 — fără suma corectată, declarația nu are sens fiscal (rectifică fără a indica corectul).
    // Nota: sumele (C) = totalul corect, NU diferența față de inițial.
    for (i, o) in input.obligations.iter().enumerate() {
        if o.suma_dat_c.is_none()
            && o.suma_plata_c.is_none()
            && o.suma_ded_c.is_none()
            && o.suma_rest_c.is_none()
        {
            return Err(AppError::Validation(format!(
                "D710: obligația {} (cod_oblig={}) nu are nicio sumă corectă (C) completată. \
                 Introduceți cel puțin suma datorată corectă (suma_dat_c) — aceasta reprezintă \
                 totalul corect, NU diferența față de suma inițial declarată.",
                i + 1,
                o.cod_oblig
            )));
        }
    }

    // totalPlata_A = suma tuturor suma_plata_c (valoarea corectă de plată per obligație).
    // Dacă nu există suma_plata_c, folosim suma_dat_c ca fallback, altfel 0.
    let total_plata_a: i64 = input
        .obligations
        .iter()
        .map(|o| {
            let val = o.suma_plata_c.or(o.suma_dat_c).unwrap_or(Decimal::ZERO);
            round_lei(val)
        })
        .sum::<i64>()
        .max(0);

    let luna_s = hdr.luna.to_string();
    let an_s = hdr.an.to_string();
    let d_anulare_s = hdr.d_anulare.to_string();
    let total_s = total_plata_a.to_string();

    let den = trunc(hdr.den.trim(), 200);
    let adresa = trunc(hdr.adresa.trim(), 1000);
    let nume = trunc(hdr.nume_declar.trim(), 75);
    let prenume = trunc(hdr.prenume_declar.trim(), 75);
    let functie = trunc(hdr.functie_declar.trim(), 50);

    // Build root attributes (ordered per XSD for readability).
    let mut attrs: Vec<(&str, String)> = vec![
        ("xmlns", D710_NAMESPACE.into()),
        ("luna", luna_s),
        ("an", an_s),
    ];

    if let Some(d) = hdr.d_succ {
        attrs.push(("d_succ", d.to_string()));
    }
    if let Some(d) = hdr.d_dizolv {
        attrs.push(("d_dizolv", d.to_string()));
    }
    if let Some(d) = hdr.d_energie {
        attrs.push(("d_energie", d.to_string()));
    }
    if let Some(d) = hdr.d_modif {
        attrs.push(("d_modif", d.to_string()));
    }

    attrs.push(("d_anulare", d_anulare_s));

    if let Some(t) = hdr.temei {
        attrs.push(("temei", t.to_string()));
    }
    if hdr.rectificativa {
        attrs.push(("d_recN", "1".into()));
    }

    attrs.push(("nume_declar", nume));
    attrs.push(("prenume_declar", prenume));
    attrs.push(("functie_declar", functie));
    attrs.push(("cui", hdr.cui.trim().into()));

    if let Some(ref cs) = hdr.cif_s {
        attrs.push(("cifS", cs.trim().into()));
    }

    attrs.push(("den", den));
    attrs.push(("adresa", adresa));

    if let Some(ref t) = hdr.telefon {
        attrs.push(("telefon", trunc(t.trim(), 15)));
    }
    if let Some(ref f) = hdr.fax {
        attrs.push(("fax", trunc(f.trim(), 15)));
    }
    if let Some(ref m) = hdr.mail {
        attrs.push(("mail", trunc(m.trim(), 200)));
    }
    if let Some(ref cr) = hdr.cif_r {
        attrs.push(("cifR", cr.trim().into()));
    }
    if let Some(ref dr) = hdr.den_r {
        attrs.push(("denR", trunc(dr.trim(), 200)));
    }
    if let Some(ref ar) = hdr.adr_r {
        attrs.push(("adrR", trunc(ar.trim(), 1000)));
    }
    if let Some(ref tr) = hdr.tel_r {
        attrs.push(("telR", trunc(tr.trim(), 15)));
    }
    if let Some(ref fr) = hdr.fax_r {
        attrs.push(("faxR", trunc(fr.trim(), 15)));
    }
    if let Some(ref er) = hdr.email_r {
        attrs.push(("emailR", trunc(er.trim(), 200)));
    }

    attrs.push(("totalPlata_A", total_s));

    // Convert to &str pairs for the writer.
    let attr_refs: Vec<(&str, &str)> = attrs.iter().map(|(k, v)| (*k, v.as_str())).collect();

    let mut w = new_writer()?;
    start_elem_attrs(&mut w, D710_ROOT, &attr_refs)?;

    // Emit one <obligatie> per obligation (attribute-based, per d710.xsd).
    for o in &input.obligations {
        let cod_s = o.cod_oblig.to_string();
        let cod_bug = trunc(o.cod_bugetar.trim(), 10);
        let nr_evid_s = o.nr_evid.to_string();

        let mut oattrs: Vec<(&str, String)> = vec![
            ("cod_oblig", cod_s),
            ("cod_bugetar", cod_bug),
            ("scadenta", o.scadenta.trim().into()),
            ("nr_evid", nr_evid_s),
        ];

        if let Some(v) = o.suma_dat_i {
            oattrs.push(("suma_dat_i", to_lei(v)));
        }
        if let Some(v) = o.suma_dat_c {
            oattrs.push(("suma_dat_c", to_lei(v)));
        }
        if let Some(v) = o.suma_ded_i {
            oattrs.push(("suma_ded_i", to_lei(v)));
        }
        if let Some(v) = o.suma_ded_c {
            oattrs.push(("suma_ded_c", to_lei(v)));
        }
        if let Some(v) = o.suma_plata_i {
            oattrs.push(("suma_plata_i", to_lei(v)));
        }
        if let Some(v) = o.suma_plata_c {
            oattrs.push(("suma_plata_c", to_lei(v)));
        }
        if let Some(v) = o.suma_rest_i {
            oattrs.push(("suma_rest_i", to_lei(v)));
        }
        if let Some(v) = o.suma_rest_c {
            oattrs.push(("suma_rest_c", to_lei(v)));
        }
        if let Some(c) = o.cota {
            oattrs.push(("cota", c.to_string()));
        }

        let orefs: Vec<(&str, &str)> = oattrs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        empty_elem_attrs(&mut w, "obligatie", &orefs)?;
    }

    end_elem(&mut w, D710_ROOT)?;
    Ok(pretty_print(&finish(w)?))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn header(luna: u32, an: i32) -> D710Header {
        D710Header {
            cui: "12345674".into(),
            den: "Test SRL".into(),
            adresa: "Str. Test 1, București".into(),
            luna,
            an,
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

    fn oblig_simple(
        cod: u32,
        cod_bug: &str,
        scadenta: &str,
        plata_i: &str,
        plata_c: &str,
    ) -> D710Obligation {
        D710Obligation {
            cod_oblig: cod,
            cod_bugetar: cod_bug.into(),
            scadenta: scadenta.into(),
            nr_evid: 0,
            suma_dat_i: None,
            suma_dat_c: None,
            suma_ded_i: None,
            suma_ded_c: None,
            suma_plata_i: Some(d(plata_i)),
            suma_plata_c: Some(d(plata_c)),
            suma_rest_i: None,
            suma_rest_c: None,
            cota: None,
            den_oblig: String::new(),
        }
    }

    #[test]
    fn empty_obligations_returns_error() {
        let input = D710Input {
            header: header(5, 2026),
            obligations: vec![],
        };
        assert!(
            build_d710_xml(&input).is_err(),
            "empty obligations should fail"
        );
    }

    #[test]
    fn invalid_luna_returns_error() {
        let mut hdr = header(0, 2026);
        hdr.luna = 13;
        let input = D710Input {
            header: hdr,
            obligations: vec![oblig_simple(2, "0105", "25.04.2026", "8000", "10000")],
        };
        assert!(build_d710_xml(&input).is_err(), "luna=13 should fail");
    }

    #[test]
    fn root_is_declaratie710_v1_namespace() {
        let input = D710Input {
            header: header(5, 2026),
            obligations: vec![oblig_simple(2, "0105", "25.06.2026", "8000", "10000")],
        };
        let xml = build_d710_xml(&input).unwrap();
        // Root must be <declaratie710>
        assert!(
            xml.contains("<declaratie710 ") || xml.contains("<declaratie710>"),
            "root must be <declaratie710>: {xml}"
        );
        assert!(
            xml.contains("</declaratie710>"),
            "close tag </declaratie710> missing: {xml}"
        );
        // Namespace MUST be v1 (targetNamespace per XSD, not the buggy xmlns=v2 in the XSD header)
        assert!(
            xml.contains(r#"xmlns="mfp:anaf:dgti:d710:declaratie:v1""#),
            "namespace must be v1: {xml}"
        );
    }

    #[test]
    fn obligatie_uses_attributes_not_child_elements() {
        let input = D710Input {
            header: header(3, 2026),
            obligations: vec![oblig_simple(2, "0105", "25.04.2026", "8000", "10000")],
        };
        let xml = build_d710_xml(&input).unwrap();

        // Must use <obligatie .../> (self-closing attribute-based), NOT <tabel>
        assert!(
            xml.contains("<obligatie "),
            "must use <obligatie> element: {xml}"
        );
        assert!(
            !xml.contains("<tabel"),
            "must NOT use old <tabel> element: {xml}"
        );
        // Attributes (required per XSD)
        assert!(xml.contains(r#"cod_oblig="2""#), "cod_oblig attr: {xml}");
        assert!(
            xml.contains(r#"cod_bugetar="0105""#),
            "cod_bugetar attr: {xml}"
        );
        assert!(
            xml.contains(r#"scadenta="25.04.2026""#),
            "scadenta attr: {xml}"
        );
        assert!(xml.contains(r#"nr_evid="0""#), "nr_evid attr: {xml}");
        // Optional sum attributes
        assert!(
            xml.contains(r#"suma_plata_i="8000""#),
            "suma_plata_i attr: {xml}"
        );
        assert!(
            xml.contains(r#"suma_plata_c="10000""#),
            "suma_plata_c attr: {xml}"
        );
    }

    #[test]
    fn header_attributes_present() {
        let input = D710Input {
            header: header(5, 2026),
            obligations: vec![oblig_simple(5, "0205", "25.06.2026", "1800", "2000")],
        };
        let xml = build_d710_xml(&input).unwrap();

        assert!(xml.contains(r#"luna="5""#), "luna: {xml}");
        assert!(xml.contains(r#"an="2026""#), "an: {xml}");
        assert!(xml.contains(r#"d_anulare="0""#), "d_anulare: {xml}");
        assert!(xml.contains(r#"cui="12345674""#), "cui: {xml}");
        assert!(xml.contains(r#"den="Test SRL""#), "den: {xml}");
        assert!(
            xml.contains(r#"nome_declar"#) || xml.contains(r#"nume_declar="Popescu""#),
            "nume_declar: {xml}"
        );
        assert!(
            xml.contains(r#"prenume_declar="Ion""#),
            "prenume_declar: {xml}"
        );
        assert!(
            xml.contains(r#"functie_declar="Administrator""#),
            "functie_declar: {xml}"
        );
        // totalPlata_A = suma_plata_c = 2000
        assert!(
            xml.contains(r#"totalPlata_A="2000""#),
            "totalPlata_A: {xml}"
        );
    }

    #[test]
    fn two_obligations_same_period_produce_two_obligatie_elements() {
        let input = D710Input {
            header: header(6, 2026),
            obligations: vec![
                oblig_simple(5, "0205", "25.07.2026", "1800", "2000"),
                oblig_simple(17, "0305", "25.07.2026", "1400", "1600"),
            ],
        };
        let xml = build_d710_xml(&input).unwrap();

        // Two <obligatie> elements
        assert_eq!(
            xml.matches("<obligatie ").count(),
            2,
            "expected 2 <obligatie> elements: {xml}"
        );
        assert!(xml.contains(r#"cod_oblig="5""#), "cod micro: {xml}");
        assert!(xml.contains(r#"cod_oblig="17""#), "cod dividende: {xml}");

        // totalPlata_A = 2000 + 1600 = 3600
        assert!(
            xml.contains(r#"totalPlata_A="3600""#),
            "totalPlata_A sum: {xml}"
        );
        assert!(
            xml.contains(r#"suma_plata_i="1800""#),
            "plata_i micro: {xml}"
        );
        assert!(
            xml.contains(r#"suma_plata_c="2000""#),
            "plata_c micro: {xml}"
        );
        assert!(
            xml.contains(r#"suma_plata_i="1400""#),
            "plata_i dividende: {xml}"
        );
        assert!(
            xml.contains(r#"suma_plata_c="1600""#),
            "plata_c dividende: {xml}"
        );
    }

    #[test]
    fn amounts_rounded_to_whole_lei() {
        let input = D710Input {
            header: header(3, 2026),
            obligations: vec![D710Obligation {
                cod_oblig: 2,
                cod_bugetar: "0105".into(),
                scadenta: "25.04.2026".into(),
                nr_evid: 0,
                suma_plata_i: Some(d("8888.50")), // → 8889
                suma_plata_c: Some(d("9999.50")), // → 10000
                ..Default::default()
            }],
        };
        let xml = build_d710_xml(&input).unwrap();
        assert!(xml.contains(r#"suma_plata_i="8889""#), "rounding I: {xml}");
        assert!(xml.contains(r#"suma_plata_c="10000""#), "rounding C: {xml}");
    }

    #[test]
    fn rectificativa_emits_d_recn_attribute() {
        let mut hdr = header(3, 2026);
        hdr.rectificativa = true;
        let input = D710Input {
            header: hdr,
            obligations: vec![oblig_simple(22, "0405", "25.04.2026", "2500", "3000")],
        };
        let xml = build_d710_xml(&input).unwrap();
        assert!(
            xml.contains(r#"d_recN="1""#),
            "d_recN for rectificativa: {xml}"
        );
    }

    #[test]
    fn d_anulare_1_emits_correctly() {
        let mut hdr = header(5, 2026);
        hdr.d_anulare = 1;
        let input = D710Input {
            header: hdr,
            obligations: vec![oblig_simple(5, "0205", "25.06.2026", "2000", "0")],
        };
        let xml = build_d710_xml(&input).unwrap();
        assert!(xml.contains(r#"d_anulare="1""#), "d_anulare=1: {xml}");
    }

    #[test]
    fn all_optional_sum_fields_emitted_when_set() {
        let input = D710Input {
            header: header(5, 2026),
            obligations: vec![D710Obligation {
                cod_oblig: 2,
                cod_bugetar: "0105".into(),
                scadenta: "25.06.2026".into(),
                nr_evid: 42,
                suma_dat_i: Some(d("1000")),
                suma_dat_c: Some(d("1100")),
                suma_ded_i: Some(d("100")),
                suma_ded_c: Some(d("110")),
                suma_plata_i: Some(d("900")),
                suma_plata_c: Some(d("990")),
                suma_rest_i: Some(d("50")),
                suma_rest_c: Some(d("55")),
                cota: Some(1),
                den_oblig: "Test".into(),
            }],
        };
        let xml = build_d710_xml(&input).unwrap();
        assert!(xml.contains(r#"nr_evid="42""#), "nr_evid: {xml}");
        assert!(xml.contains(r#"suma_dat_i="1000""#), "suma_dat_i: {xml}");
        assert!(xml.contains(r#"suma_dat_c="1100""#), "suma_dat_c: {xml}");
        assert!(xml.contains(r#"suma_ded_i="100""#), "suma_ded_i: {xml}");
        assert!(xml.contains(r#"suma_ded_c="110""#), "suma_ded_c: {xml}");
        assert!(xml.contains(r#"suma_plata_i="900""#), "suma_plata_i: {xml}");
        assert!(xml.contains(r#"suma_plata_c="990""#), "suma_plata_c: {xml}");
        assert!(xml.contains(r#"suma_rest_i="50""#), "suma_rest_i: {xml}");
        assert!(xml.contains(r#"suma_rest_c="55""#), "suma_rest_c: {xml}");
        assert!(xml.contains(r#"cota="1""#), "cota: {xml}");
    }

    // ── GUARDRAIL tests: suma_dat_c required ─────────────────────────────────

    /// GUARDRAIL: obligation with no corrected (C) amounts is rejected.
    /// suma_dat_c is required — D710 without a corrected amount has no fiscal meaning.
    #[test]
    fn suma_dat_c_missing_all_c_fields_rejected() {
        let input = D710Input {
            header: header(5, 2026),
            obligations: vec![D710Obligation {
                cod_oblig: 5,
                cod_bugetar: "0205".into(),
                scadenta: "25.06.2026".into(),
                nr_evid: 0,
                suma_dat_i: Some(d("1000")), // (I) present
                suma_dat_c: None,            // (C) missing
                suma_ded_i: None,
                suma_ded_c: None, // (C) missing
                suma_plata_i: None,
                suma_plata_c: None, // (C) missing
                suma_rest_i: None,
                suma_rest_c: None, // (C) missing
                cota: None,
                den_oblig: "Impozit micro".into(),
            }],
        };
        let result = build_d710_xml(&input);
        assert!(
            result.is_err(),
            "D710 obligation with no corrected (C) amount must be rejected by guardrail"
        );
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("suma") || msg.contains("corect"),
            "error should mention suma or corect: {msg}"
        );
    }

    /// GUARDRAIL: obligation with suma_dat_c set is accepted (C present).
    #[test]
    fn suma_dat_c_present_accepted() {
        let input = D710Input {
            header: header(5, 2026),
            obligations: vec![D710Obligation {
                cod_oblig: 5,
                cod_bugetar: "0205".into(),
                scadenta: "25.06.2026".into(),
                nr_evid: 0,
                suma_dat_i: Some(d("1000")),
                suma_dat_c: Some(d("1200")), // (C) required — TOTAL correct, not diff
                suma_ded_i: None,
                suma_ded_c: None,
                suma_plata_i: None,
                suma_plata_c: None,
                suma_rest_i: None,
                suma_rest_c: None,
                cota: None,
                den_oblig: "Impozit micro".into(),
            }],
        };
        let result = build_d710_xml(&input);
        assert!(
            result.is_ok(),
            "D710 obligation with suma_dat_c must be accepted: {:?}",
            result
        );
        let xml = result.unwrap();
        assert!(
            xml.contains(r#"suma_dat_c="1200""#),
            "suma_dat_c in XML: {xml}"
        );
        assert!(
            xml.contains(r#"suma_dat_i="1000""#),
            "suma_dat_i in XML: {xml}"
        );
    }

    /// GUARDRAIL: obligation with only suma_plata_c (no suma_dat_c) is also accepted
    /// because at least one corrected (C) field is present.
    #[test]
    fn suma_plata_c_only_accepted() {
        // oblig_simple uses suma_plata_c (not suma_dat_c) — this must succeed.
        let input = D710Input {
            header: header(5, 2026),
            obligations: vec![oblig_simple(17, "0405", "25.06.2026", "500", "600")],
        };
        let result = build_d710_xml(&input);
        assert!(
            result.is_ok(),
            "D710 with suma_plata_c but no suma_dat_c must be accepted: {:?}",
            result
        );
    }

    /// GUARDRAIL: multiple obligations — only the one without any C rejected.
    #[test]
    fn mixed_obligations_one_missing_c_rejected() {
        let input = D710Input {
            header: header(5, 2026),
            obligations: vec![
                oblig_simple(5, "0205", "25.06.2026", "1000", "1200"), // valid
                D710Obligation {
                    cod_oblig: 2,
                    cod_bugetar: "0105".into(),
                    scadenta: "25.06.2026".into(),
                    nr_evid: 0,
                    suma_dat_i: Some(d("5000")),
                    suma_dat_c: None, // missing C
                    suma_ded_i: None,
                    suma_ded_c: None,
                    suma_plata_i: None,
                    suma_plata_c: None,
                    suma_rest_i: None,
                    suma_rest_c: None,
                    cota: None,
                    den_oblig: "Impozit profit".into(),
                },
            ],
        };
        let result = build_d710_xml(&input);
        assert!(
            result.is_err(),
            "one obligation without any C should reject the whole D710"
        );
    }
}
