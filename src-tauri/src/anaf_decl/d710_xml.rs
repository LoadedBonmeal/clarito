//! D710 — Declarație rectificativă pentru obligații D100 (OPANAF 587/2016 + 779/2024).
//!
//! **XSD-VALIDAT via `xmllint --schema tools/anaf/d710.xsd`** (XSD vendored patched la
//! targetNamespace `mfp:anaf:dgti:d710:declaratie:v2`, version 1.02).
//! **DUK-VALIDAT prin DUKIntegrator** (`java -jar DUKIntegrator.jar -v D710 <xml> <result>`),
//! același mecanism overlay ca D301 și D700.
//! NOTĂ: XSD-ul ANAF oficial are `targetNamespace=v1` dar DUKIntegrator cere `v2`;
//! XSD-ul vendored este patched de `scripts/fetch-validators.sh` să accepte `v2`.
//! Atributele `<obligatie>` folosesc uppercase `_I`/`_C` (cerință DUK v2+, nu lowercase ca în XSD v1).
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
//!                suma_dat_I="N" suma_dat_C="N"
//!                suma_ded_I="N" suma_ded_C="N"
//!                suma_plata_I="N" suma_plata_C="N"
//!                suma_rest_I="N" suma_rest_C="N"
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

/// Namespace D710 — cerut de `D710Validator.jar` prin DUKIntegrator (`-v D710`).
/// NOTE: XSD-ul ANAF publicat are un bug tipografic: XSD-ul oficial are `targetNamespace=v1`
/// dar validatorul DUK respinge documentele cu `v1` și cere `v2`. XSD-ul vendored
/// (`tools/anaf/d710.xsd`) este patched de `scripts/fetch-validators.sh` să accepte `v2`.
/// Documentele generate trebuie să folosească `v2` (cerința DUKIntegrator este autoritativă).
pub const D710_NAMESPACE: &str = "mfp:anaf:dgti:d710:declaratie:v2";

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
/// toate sumele (`suma_dat_I/c`, `suma_ded_I/c`, `suma_plata_I/c`, `suma_rest_I/c`)
/// și `cota` sunt OPȚIONALE. Sumele sunt IntPoz15SType (întreg ≥ 0, lei întregi).
///
/// Semantica perechilor I/C (inițial/corect):
/// - `_i` = valoarea INIȚIAL declarată în D100 original.
/// - `_c` = valoarea CORECTĂ (totalul corect, NU diferența față de inițial).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(non_snake_case)] // Attribute names must match DUKIntegrator v2 protocol (uppercase I/C)
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
    pub suma_dat_I: Option<Decimal>,
    /// Suma datorată corectă (C), lei întregi (opțional per XSD).
    #[serde(default)]
    pub suma_dat_C: Option<Decimal>,
    /// Suma deductibilă inițial (I), lei întregi (opțional).
    #[serde(default)]
    pub suma_ded_I: Option<Decimal>,
    /// Suma deductibilă corectă (C), lei întregi (opțional).
    #[serde(default)]
    pub suma_ded_C: Option<Decimal>,
    /// Suma de plată inițial (I), lei întregi (opțional).
    #[serde(default)]
    pub suma_plata_I: Option<Decimal>,
    /// Suma de plată corectă (C), lei întregi (opțional).
    #[serde(default)]
    pub suma_plata_C: Option<Decimal>,
    /// Suma restantă inițial (I), lei întregi (opțional).
    #[serde(default)]
    pub suma_rest_I: Option<Decimal>,
    /// Suma restantă corectă (C), lei întregi (opțional).
    #[serde(default)]
    pub suma_rest_C: Option<Decimal>,
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

/// Calculează Numărul de Evidență a Plății D710 (23 cifre) conform algoritmului DUK R16.
///
/// Format (decodat din `v10/Obligatie.class` din `D710Validator.jar`):
/// - pos 0-1:   `"10"` — prefix fix
/// - pos 2-4:   `cod_oblig` zero-padded la 3 cifre
/// - pos 5-6:   `"01"` — câmp fix (verificat prin concat [0:2]+[5:7]+[17:21] = "10010000")
/// - pos 7-10:  `MMYY` — luna (zero-pad 2) + ultimele 2 cifre ale anului
/// - pos 11-16: `"25"` + `scad_MM` (zero-pad 2) + `scad_YY` (2 cifre) — data scadenței
/// - pos 17-20: `"0000"` — câmp fix
/// - pos 21-22: suma cifrelor [0..20] mod 100, 2 cifre
///
/// Dacă `nr_evid_override > 0`, returnează acel număr formatat pe 23 cifre fără calcul.
pub fn compute_nr_evid_d710(
    luna: u32,
    an: i32,
    cod_oblig: u32,
    scad_mm: u32,
    scad_yy: u32,
    nr_evid_override: u64,
) -> String {
    if nr_evid_override > 0 {
        return format!("{:023}", nr_evid_override);
    }

    let cod_s = format!("{:03}", cod_oblig);
    let mm = format!("{:02}", luna);
    let yy = format!("{:02}", an.abs() % 100);
    let scad_mm_s = format!("{:02}", scad_mm);
    let scad_yy_s = format!("{:02}", scad_yy % 100);

    // Build 21-char base: "10" + COD(3) + "01" + MMYY(4) + "25" + scadMM(2) + scadYY(2) + "0000"(4)
    let base = format!("10{cod_s}01{mm}{yy}25{scad_mm_s}{scad_yy_s}0000");
    debug_assert_eq!(base.len(), 21, "D710 nr_evid base must be 21 chars");

    let sum: u32 = base.chars().map(|c| c.to_digit(10).unwrap_or(0)).sum();
    let ctrl = sum % 100;
    format!("{base}{:02}", ctrl)
}

/// Parsează data scadenței "ZZ.LL.AAAA" și returnează (luna, an) sau None.
fn parse_scadenta(scadenta: &str) -> Option<(u32, u32)> {
    let parts: Vec<&str> = scadenta.split('.').collect();
    if parts.len() == 3 {
        let mm = parts[1].parse::<u32>().ok()?;
        let yy = parts[2].parse::<u32>().ok()? % 100;
        Some((mm, yy))
    } else {
        None
    }
}

// ── Emitorul XML ──────────────────────────────────────────────────────────────

/// Construiește XML-ul D710 (declarație rectificativă obligații D100) pentru perioada dată.
///
/// Structura este **XSD-validată** față de `tools/anaf/d710.xsd` (patched la v2, version 1.02)
/// și **DUK-validată** prin DUKIntegrator (`-v D710`).
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

    // GUARDRAIL: suma_dat_C (suma datorată CORECTĂ) este obligatorie per regulile de business
    // D710 — fără suma corectată, declarația nu are sens fiscal (rectifică fără a indica corectul).
    // Nota: sumele (C) = totalul corect, NU diferența față de inițial.
    for (i, o) in input.obligations.iter().enumerate() {
        if o.suma_dat_C.is_none()
            && o.suma_plata_C.is_none()
            && o.suma_ded_C.is_none()
            && o.suma_rest_C.is_none()
        {
            return Err(AppError::Validation(format!(
                "D710: obligația {} (cod_oblig={}) nu are nicio sumă corectă (C) completată. \
                 Introduceți cel puțin suma datorată corectă (suma_dat_C) — aceasta reprezintă \
                 totalul corect, NU diferența față de suma inițial declarată.",
                i + 1,
                o.cod_oblig
            )));
        }
    }

    // DUK R11b: totalPlata_A = Σ(TOATE câmpurile sumă non-nule pentru TOATE obligațiile).
    // DUK calculează: Σ(suma_dat_I + suma_dat_C + suma_ded_I + suma_ded_C +
    //                   suma_plata_I + suma_plata_C + suma_rest_I + suma_rest_C)
    // pentru fiecare obligație, indiferent de câmpul prezent.
    let total_plata_a: i64 = input
        .obligations
        .iter()
        .map(|o| {
            let mut s = 0i64;
            if let Some(v) = o.suma_dat_I {
                s += round_lei(v);
            }
            if let Some(v) = o.suma_dat_C {
                s += round_lei(v);
            }
            if let Some(v) = o.suma_ded_I {
                s += round_lei(v);
            }
            if let Some(v) = o.suma_ded_C {
                s += round_lei(v);
            }
            if let Some(v) = o.suma_plata_I {
                s += round_lei(v);
            }
            if let Some(v) = o.suma_plata_C {
                s += round_lei(v);
            }
            if let Some(v) = o.suma_rest_I {
                s += round_lei(v);
            }
            if let Some(v) = o.suma_rest_C {
                s += round_lei(v);
            }
            s
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
        // nr_evid: dacă 0, calculează automat din luna/an/cod_oblig/scadenta (DUK R16).
        let nr_evid_s = if o.nr_evid == 0 {
            let (scad_mm, scad_yy) =
                parse_scadenta(o.scadenta.trim()).unwrap_or((hdr.luna + 1, (hdr.an % 100) as u32));
            compute_nr_evid_d710(hdr.luna, hdr.an, o.cod_oblig, scad_mm, scad_yy, 0)
        } else {
            format!("{:023}", o.nr_evid)
        };

        let mut oattrs: Vec<(&str, String)> = vec![
            ("cod_oblig", cod_s),
            ("cod_bugetar", cod_bug),
            ("scadenta", o.scadenta.trim().into()),
            ("nr_evid", nr_evid_s),
        ];

        if let Some(v) = o.suma_dat_I {
            oattrs.push(("suma_dat_I", to_lei(v)));
        }
        if let Some(v) = o.suma_dat_C {
            oattrs.push(("suma_dat_C", to_lei(v)));
        }
        if let Some(v) = o.suma_ded_I {
            oattrs.push(("suma_ded_I", to_lei(v)));
        }
        if let Some(v) = o.suma_ded_C {
            oattrs.push(("suma_ded_C", to_lei(v)));
        }
        if let Some(v) = o.suma_plata_I {
            oattrs.push(("suma_plata_I", to_lei(v)));
        }
        if let Some(v) = o.suma_plata_C {
            oattrs.push(("suma_plata_C", to_lei(v)));
        }
        if let Some(v) = o.suma_rest_I {
            oattrs.push(("suma_rest_I", to_lei(v)));
        }
        if let Some(v) = o.suma_rest_C {
            oattrs.push(("suma_rest_C", to_lei(v)));
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
            suma_dat_I: None,
            suma_dat_C: None,
            suma_ded_I: None,
            suma_ded_C: None,
            suma_plata_I: Some(d(plata_i)),
            suma_plata_C: Some(d(plata_c)),
            suma_rest_I: None,
            suma_rest_C: None,
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
    fn root_is_declaratie710_v2_namespace() {
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
        // Namespace MUST be v2 (required by DUKIntegrator `-v D710`; XSD is patched to v2 by fetch-validators.sh)
        assert!(
            xml.contains(r#"xmlns="mfp:anaf:dgti:d710:declaratie:v2""#),
            "namespace must be v2 (DUK requirement): {xml}"
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
        // nr_evid is auto-computed (23 chars) when 0 (DUK R16)
        assert!(xml.contains(r#"nr_evid=""#), "nr_evid attr missing: {xml}");
        // Optional sum attributes
        assert!(
            xml.contains(r#"suma_plata_I="8000""#),
            "suma_plata_I attr: {xml}"
        );
        assert!(
            xml.contains(r#"suma_plata_C="10000""#),
            "suma_plata_C attr: {xml}"
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
        // DUK R11b: totalPlata_A = Σ(ALL non-null sums) = suma_plata_I(1800) + suma_plata_C(2000)
        assert!(
            xml.contains(r#"totalPlata_A="3800""#),
            "totalPlata_A (R11b: all sums): {xml}"
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

        // DUK R11b: totalPlata_A = Σ(ALL non-null sums)
        // = plata_I(1800) + plata_C(2000) + plata_I(1400) + plata_C(1600) = 6800
        assert!(
            xml.contains(r#"totalPlata_A="6800""#),
            "totalPlata_A (R11b: Σ all sums): {xml}"
        );
        assert!(
            xml.contains(r#"suma_plata_I="1800""#),
            "plata_i micro: {xml}"
        );
        assert!(
            xml.contains(r#"suma_plata_C="2000""#),
            "plata_c micro: {xml}"
        );
        assert!(
            xml.contains(r#"suma_plata_I="1400""#),
            "plata_i dividende: {xml}"
        );
        assert!(
            xml.contains(r#"suma_plata_C="1600""#),
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
                suma_plata_I: Some(d("8888.50")), // → 8889
                suma_plata_C: Some(d("9999.50")), // → 10000
                ..Default::default()
            }],
        };
        let xml = build_d710_xml(&input).unwrap();
        assert!(xml.contains(r#"suma_plata_I="8889""#), "rounding I: {xml}");
        assert!(xml.contains(r#"suma_plata_C="10000""#), "rounding C: {xml}");
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
                suma_dat_I: Some(d("1000")),
                suma_dat_C: Some(d("1100")),
                suma_ded_I: Some(d("100")),
                suma_ded_C: Some(d("110")),
                suma_plata_I: Some(d("900")),
                suma_plata_C: Some(d("990")),
                suma_rest_I: Some(d("50")),
                suma_rest_C: Some(d("55")),
                cota: Some(1),
                den_oblig: "Test".into(),
            }],
        };
        let xml = build_d710_xml(&input).unwrap();
        // nr_evid is always 23-char (override non-zero → zero-padded to 23 digits)
        assert!(
            xml.contains(r#"nr_evid="00000000000000000000042""#),
            "nr_evid (23-char): {xml}"
        );
        assert!(xml.contains(r#"suma_dat_I="1000""#), "suma_dat_I: {xml}");
        assert!(xml.contains(r#"suma_dat_C="1100""#), "suma_dat_C: {xml}");
        assert!(xml.contains(r#"suma_ded_I="100""#), "suma_ded_I: {xml}");
        assert!(xml.contains(r#"suma_ded_C="110""#), "suma_ded_C: {xml}");
        assert!(xml.contains(r#"suma_plata_I="900""#), "suma_plata_I: {xml}");
        assert!(xml.contains(r#"suma_plata_C="990""#), "suma_plata_C: {xml}");
        assert!(xml.contains(r#"suma_rest_I="50""#), "suma_rest_I: {xml}");
        assert!(xml.contains(r#"suma_rest_C="55""#), "suma_rest_C: {xml}");
        assert!(xml.contains(r#"cota="1""#), "cota: {xml}");
    }

    // ── GUARDRAIL tests: suma_dat_C required ─────────────────────────────────

    /// GUARDRAIL: obligation with no corrected (C) amounts is rejected.
    /// suma_dat_C is required — D710 without a corrected amount has no fiscal meaning.
    #[test]
    fn suma_dat_corrected_missing_all_c_fields_rejected() {
        let input = D710Input {
            header: header(5, 2026),
            obligations: vec![D710Obligation {
                cod_oblig: 5,
                cod_bugetar: "0205".into(),
                scadenta: "25.06.2026".into(),
                nr_evid: 0,
                suma_dat_I: Some(d("1000")), // (I) present
                suma_dat_C: None,            // (C) missing
                suma_ded_I: None,
                suma_ded_C: None, // (C) missing
                suma_plata_I: None,
                suma_plata_C: None, // (C) missing
                suma_rest_I: None,
                suma_rest_C: None, // (C) missing
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

    /// GUARDRAIL: obligation with suma_dat_C set is accepted (C present).
    #[test]
    fn suma_dat_corrected_present_accepted() {
        let input = D710Input {
            header: header(5, 2026),
            obligations: vec![D710Obligation {
                cod_oblig: 5,
                cod_bugetar: "0205".into(),
                scadenta: "25.06.2026".into(),
                nr_evid: 0,
                suma_dat_I: Some(d("1000")),
                suma_dat_C: Some(d("1200")), // (C) required — TOTAL correct, not diff
                suma_ded_I: None,
                suma_ded_C: None,
                suma_plata_I: None,
                suma_plata_C: None,
                suma_rest_I: None,
                suma_rest_C: None,
                cota: None,
                den_oblig: "Impozit micro".into(),
            }],
        };
        let result = build_d710_xml(&input);
        assert!(
            result.is_ok(),
            "D710 obligation with suma_dat_C must be accepted: {:?}",
            result
        );
        let xml = result.unwrap();
        assert!(
            xml.contains(r#"suma_dat_C="1200""#),
            "suma_dat_C in XML: {xml}"
        );
        assert!(
            xml.contains(r#"suma_dat_I="1000""#),
            "suma_dat_I in XML: {xml}"
        );
    }

    /// GUARDRAIL: obligation with only suma_plata_C (no suma_dat_C) is also accepted
    /// because at least one corrected (C) field is present.
    #[test]
    fn suma_plata_corrected_only_accepted() {
        // oblig_simple uses suma_plata_C (not suma_dat_C) — this must succeed.
        let input = D710Input {
            header: header(5, 2026),
            obligations: vec![oblig_simple(17, "0405", "25.06.2026", "500", "600")],
        };
        let result = build_d710_xml(&input);
        assert!(
            result.is_ok(),
            "D710 with suma_plata_C but no suma_dat_C must be accepted: {:?}",
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
                    suma_dat_I: Some(d("5000")),
                    suma_dat_C: None, // missing C
                    suma_ded_I: None,
                    suma_ded_C: None,
                    suma_plata_I: None,
                    suma_plata_C: None,
                    suma_rest_I: None,
                    suma_rest_C: None,
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
