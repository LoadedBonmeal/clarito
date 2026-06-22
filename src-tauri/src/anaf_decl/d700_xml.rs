//! D700 — Declarație de înregistrare fiscală / Declarație de mențiuni / Declarație de radiere
//! (OPANAF 15/2026, ediția 0126, pentru persoane juridice și alte entități).
//!
//! **STRUCTURA CORECTĂ PER STRUCTURA_XML_D700_0126 — FĂRĂ XSD PUBLIC.**
//! D700 nu are XSD public descărcabil (numai `D700Validator.jar` din pachetul
//! `D700_20260423.zip` de pe declaratii.anaf.ro, rulat prin DUKIntegrator).
//! Structura elementelor și a bifelor A-G este derivată din documentul
//! `structura_XML_D700_0126` publicat de ANAF.
//!
//! Validare obligatorie înainte de depunere:
//!   `java -jar DUKIntegrator.jar -v D700 <xml> <result>`
//! unde `D700Validator.jar` este inclus în pachetul `D700_20260423.zip`.
//!
//! ## Structura XML (per structura_XML_D700_0126)
//! ```text
//!   <D700 xmlns="mfp:anaf:dgti:d700:declaratie:v4"
//!         an="AAAA" luna="N"
//!         felD="2|3"           ← 2=mențiuni, 3=radiere
//!         dec_inreg="010|013|015|016|020|030|070"
//!         totalPlata_A="N"
//!         nume_decl="…" pren_decl="…" func_decl="…" data_decl="ZZ.LL.AAAA"
//!         cui="…" den="…" adresa="…" judet_cod="…" forma_juridica="…">
//!     <!-- Secțiunea A gated by Bifa_A -->
//!     <Bifa_A valoare="1">
//!       <sect_A>
//!         <reprezentant nume="…" prenume="…" cnp_nif="…" functie="…"
//!                        telefon="…" email="…"/>
//!       </sect_A>
//!     </Bifa_A>
//!     <!-- Secțiunea B gated by Bifa_B — vector fiscal -->
//!     <Bifa_B valoare="1">
//!       <sect_B>
//!         <tva tip="…" data_aplicare="…"/>
//!         <regim_fiscal tip="…" data_aplicare="…"/>
//!         <alta_mentiune>…</alta_mentiune>
//!       </sect_B>
//!     </Bifa_B>
//!     <!-- Secțiunile C-G după același pattern (Bifa_C…Bifa_G) -->
//!     <Bifa_C valoare="1">
//!       <sect_C>
//!         <sediu_secundar tip="…" judet_cod="…" adresa="…"/>
//!         <domiciliu_fiscal>…</domiciliu_fiscal>
//!       </sect_C>
//!     </Bifa_C>
//!     <Bifa_D valoare="1">
//!       <sect_D>
//!         <motiv_radiere>…</motiv_radiere>
//!         <data_radiere>…</data_radiere>
//!       </sect_D>
//!     </Bifa_D>
//!   </D700>
//! ```

use serde::{Deserialize, Serialize};

use crate::anaf_decl::xml::{
    empty_elem_attrs, end_elem, finish, new_writer, pretty_print, start_elem, start_elem_attrs,
    trunc, write_text_elem,
};
use crate::error::{AppError, AppResult};

// ── Schema version ────────────────────────────────────────────────────────────

/// Namespace D700, ediția 0126 (OPANAF 15/2026), versiunea schemei v4.
/// Rădăcina `<D700>` și versiunea v4 sunt corecte per cercetare (OPANAF 15/2026).
/// Nu există XSD public — validați cu `D700Validator.jar` din `D700_20260423.zip`
/// prin DUKIntegrator înainte de depunerea electronică prin SPV.
pub const D700_NAMESPACE: &str = "mfp:anaf:dgti:d700:declaratie:v4";

/// Elementul rădăcină al documentului D700 (verificat: `<D700>` per OPANAF 15/2026 ed. 0126).
pub const D700_ROOT: &str = "D700";

// ── Enumerări vector fiscal ───────────────────────────────────────────────────

/// Mențiune TVA — tipul modificării înregistrării în scop de TVA.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TvaMentiune {
    /// Înregistrare în scopuri de TVA (art.316).
    Inregistrare,
    /// Anulare înregistrare în scopuri de TVA.
    Anulare,
    /// Schimbare perioadă fiscală: trecere la TVA lunar.
    TrecereaLaLunar,
    /// Schimbare perioadă fiscală: trecere la TVA trimestrial.
    TrecereaLaTrimestrial,
    /// Înregistrare TVA la încasare (art.282).
    TvaLaIncasareInregistrare,
    /// Anulare TVA la încasare.
    TvaLaIncasareAnulare,
}

/// Mențiune regim fiscal — trecere micro/profit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegimFiscalMentiune {
    /// Trecere la impozit pe veniturile microîntreprinderilor.
    TrecereaLaMicro,
    /// Trecere la impozit pe profit.
    TrecereaLaProfit,
    /// Modificare frecvență impozit profit (lunar/trimestrial).
    ModificareFrecventaProfitLunar,
    ModificareFrecventaProfitTrimestrial,
}

/// Mențiune sediu secundar (înregistrare / modificare / radiere).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SediuSecundar {
    /// Codul județului (2 cifre, ex. „01" pentru Alba).
    pub judet_cod: String,
    /// Adresa sediului secundar.
    pub adresa: String,
    /// Tipul mențiunii: "inregistrare", "modificare", "radiere".
    pub tip: String,
}

// ── Model date ────────────────────────────────────────────────────────────────

/// Datele de identificare fiscală (Secțiunea A).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D700SectA {
    /// CUI-ul persoanei impozabile (fără „RO", doar cifre).
    pub cui: String,
    /// Denumirea persoanei juridice.
    pub den: String,
    /// Adresa completă (sediu social).
    pub adresa: String,
    /// Codul județului (2 cifre).
    pub judet_cod: String,
    /// Forma juridică (ex. „SRL", „SA", „PFA").
    pub forma_juridica: String,
    /// Reprezentant legal — nume.
    pub repr_nume: String,
    /// Reprezentant legal — prenume.
    pub repr_prenume: String,
    /// Reprezentant legal — CNP sau NIF (opțional).
    #[serde(default)]
    pub repr_cnp: String,
    /// Funcția reprezentantului legal.
    pub repr_functie: String,
    /// Telefon de contact (opțional).
    #[serde(default)]
    pub telefon: String,
    /// Email de contact (opțional).
    #[serde(default)]
    pub email: String,
}

/// Vectorul fiscal (Secțiunea B) — lista de mențiuni bifate de utilizator.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct D700SectB {
    /// Mențiune TVA (unul sau niciunul).
    #[serde(default)]
    pub tva_mentiune: Option<TvaMentiune>,
    /// Data de la care se aplică mențiunea TVA (ISO YYYY-MM-DD, opțional).
    #[serde(default)]
    pub tva_data: Option<String>,
    /// Mențiune regim fiscal (unul sau niciunul).
    #[serde(default)]
    pub regim_fiscal: Option<RegimFiscalMentiune>,
    /// Data de la care se aplică mențiunea regim fiscal.
    #[serde(default)]
    pub regim_fiscal_data: Option<String>,
    /// Alte mențiuni de vector fiscal — câmp liber.
    #[serde(default)]
    pub alte_mentiuni: Vec<String>,
}

impl D700SectB {
    pub fn has_data(&self) -> bool {
        self.tva_mentiune.is_some() || self.regim_fiscal.is_some() || !self.alte_mentiuni.is_empty()
    }
}

/// Sedii secundare și domiciliu fiscal (Secțiunea C).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct D700SectC {
    /// Lista sediilor secundare cu mențiunile lor.
    #[serde(default)]
    pub sedii_secundare: Vec<SediuSecundar>,
    /// Mențiune schimbare domiciliu fiscal (adresă nouă, opțional).
    #[serde(default)]
    pub domiciliu_fiscal_nou: Option<String>,
}

impl D700SectC {
    pub fn has_data(&self) -> bool {
        !self.sedii_secundare.is_empty() || self.domiciliu_fiscal_nou.is_some()
    }
}

/// Radiere (Secțiunea D).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct D700SectD {
    /// Motivul radierii (câmp liber).
    #[serde(default)]
    pub motiv: Option<String>,
    /// Data radierii (ISO YYYY-MM-DD sau ZZ.LL.AAAA).
    #[serde(default)]
    pub data_radiere: Option<String>,
}

impl D700SectD {
    pub fn has_data(&self) -> bool {
        self.motiv.is_some() || self.data_radiere.is_some()
    }
}

/// Datele complete ale declarației D700.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D700Input {
    /// 0 = declarație inițială, 1 = rectificativă (mențiuni).
    #[serde(default)]
    pub d_rec: u8,
    /// Luna depunerii (1-12). Opțional — dacă absent, se omite din header.
    #[serde(default)]
    pub luna: Option<u32>,
    /// Anul depunerii. Opțional — dacă absent, se omite din header.
    #[serde(default)]
    pub an: Option<i32>,
    /// felD: tipul declarației — 2 = mențiuni, 3 = radiere.
    /// Derivat automat: 3 dacă sect_D are date, altfel 2.
    #[serde(default)]
    pub fel_d: Option<u8>,
    /// dec_inreg: codul vectorului de înregistrare (010/013/015/016/020/030/070).
    /// Opțional — emis doar dacă este setat.
    #[serde(default)]
    pub dec_inreg: Option<String>,
    /// Data declarației (ZZ.LL.AAAA). Opțional.
    #[serde(default)]
    pub data_decl: Option<String>,
    /// Suma totală de plată (lei). Tipic 0 pentru D700 (vector fiscal, nu bani).
    #[serde(default)]
    pub total_plata_a: i64,
    /// Declarant — nume.
    #[serde(default)]
    pub nume_decl: Option<String>,
    /// Declarant — prenume.
    #[serde(default)]
    pub pren_decl: Option<String>,
    /// Declarant — funcție.
    #[serde(default)]
    pub func_decl: Option<String>,
    /// Secțiunea A — date identificare.
    pub sect_a: D700SectA,
    /// Secțiunea B — vector fiscal.
    #[serde(default)]
    pub sect_b: D700SectB,
    /// Secțiunea C — sedii + domiciliu.
    #[serde(default)]
    pub sect_c: D700SectC,
    /// Secțiunea D — radiere.
    #[serde(default)]
    pub sect_d: D700SectD,
}

// ── Emitorul XML ──────────────────────────────────────────────────────────────

/// Construiește XML-ul D700 (declarație de înregistrare/mențiuni/radiere — vector fiscal).
///
/// Structura este conformă cu `structura_XML_D700_0126` publicat de ANAF (OPANAF 15/2026).
/// **Nu există XSD public** — validați cu `D700Validator.jar` din pachetul
/// `D700_20260423.zip` prin DUKIntegrator:
/// `java -jar DUKIntegrator.jar -v D700 <xml> <result>`
///
/// ## Structura emisă
/// - Root `<D700>` cu atribute: `xmlns`, `an`, `luna`, `felD`, `dec_inreg`, `totalPlata_A`,
///   `nume_decl`, `pren_decl`, `func_decl`, `data_decl`, `cui`, `den`, `adresa`,
///   `judet_cod`, `forma_juridica`.
/// - Secțiunile A-D sunt emise condițional, fiecare gated de un element `<Bifa_X valoare="1">`.
///
/// # Erori
/// Returnează eroare dacă nicio secțiune B/C/D nu are date (o D700 fără mențiuni e invalidă).
pub fn build_d700_xml(input: &D700Input) -> AppResult<String> {
    if !input.sect_b.has_data() && !input.sect_c.has_data() && !input.sect_d.has_data() {
        return Err(AppError::Validation(
            "D700: nicio mențiune selectată (secțiunile B, C, D sunt goale). \
             Selectați cel puțin o mențiune."
                .into(),
        ));
    }

    let a = &input.sect_a;
    let d_rec_s = input.d_rec.to_string();
    let den = trunc(a.den.trim(), 200);
    let adresa = trunc(a.adresa.trim(), 200);
    let repr_nume = trunc(a.repr_nume.trim(), 75);
    let repr_prenume = trunc(a.repr_prenume.trim(), 75);
    let repr_functie = trunc(a.repr_functie.trim(), 75);

    // felD: 3 = radiere (sect_D populated), 2 = mențiuni (default).
    let fel_d = input
        .fel_d
        .unwrap_or_else(|| if input.sect_d.has_data() { 3 } else { 2 });
    let fel_d_s = fel_d.to_string();

    let total_plata_s = input.total_plata_a.to_string();

    // Build root attributes.
    let mut attrs: Vec<(&str, String)> = vec![("xmlns", D700_NAMESPACE.into()), ("d_rec", d_rec_s)];

    if let Some(an) = input.an {
        attrs.push(("an", an.to_string()));
    }
    if let Some(luna) = input.luna {
        attrs.push(("luna", luna.to_string()));
    }

    attrs.push(("felD", fel_d_s));

    if let Some(ref di) = input.dec_inreg {
        attrs.push(("dec_inreg", di.trim().into()));
    }

    attrs.push(("totalPlata_A", total_plata_s));

    if let Some(ref nm) = input.nume_decl {
        attrs.push(("nume_decl", trunc(nm.trim(), 75)));
    }
    if let Some(ref pr) = input.pren_decl {
        attrs.push(("pren_decl", trunc(pr.trim(), 75)));
    }
    if let Some(ref fc) = input.func_decl {
        attrs.push(("func_decl", trunc(fc.trim(), 50)));
    }
    if let Some(ref dd) = input.data_decl {
        attrs.push(("data_decl", dd.trim().into()));
    }

    attrs.push(("cui", a.cui.trim().into()));
    attrs.push(("den", den));
    attrs.push(("adresa", adresa));
    attrs.push(("judet_cod", a.judet_cod.trim().into()));
    attrs.push(("forma_juridica", a.forma_juridica.trim().into()));

    let attr_refs: Vec<(&str, &str)> = attrs.iter().map(|(k, v)| (*k, v.as_str())).collect();

    let mut w = new_writer()?;
    start_elem_attrs(&mut w, D700_ROOT, &attr_refs)?;

    // ── Secțiunea A: date reprezentant legal (Bifa_A) ──
    // Secțiunea A este întotdeauna emisă — conține datele de identificare ale reprezentantului.
    start_elem_attrs(&mut w, "Bifa_A", &[("valoare", "1")])?;
    start_elem(&mut w, "sect_A")?;
    empty_elem_attrs(
        &mut w,
        "reprezentant",
        &[
            ("nume", &repr_nume),
            ("prenume", &repr_prenume),
            ("cnp_nif", a.repr_cnp.trim()),
            ("functie", &repr_functie),
            ("telefon", a.telefon.trim()),
            ("email", a.email.trim()),
        ],
    )?;
    end_elem(&mut w, "sect_A")?;
    end_elem(&mut w, "Bifa_A")?;

    // ── Secțiunea B: vector fiscal (Bifa_B) ──
    if input.sect_b.has_data() {
        start_elem_attrs(&mut w, "Bifa_B", &[("valoare", "1")])?;
        start_elem(&mut w, "sect_B")?;

        if let Some(ref tv) = input.sect_b.tva_mentiune {
            let tip = tva_mentiune_cod(tv);
            let data = input.sect_b.tva_data.as_deref().unwrap_or("");
            empty_elem_attrs(&mut w, "tva", &[("tip", tip), ("data_aplicare", data)])?;
        }

        if let Some(ref rf) = input.sect_b.regim_fiscal {
            let tip = regim_fiscal_cod(rf);
            let data = input.sect_b.regim_fiscal_data.as_deref().unwrap_or("");
            empty_elem_attrs(
                &mut w,
                "regim_fiscal",
                &[("tip", tip), ("data_aplicare", data)],
            )?;
        }

        for mentiune in &input.sect_b.alte_mentiuni {
            write_text_elem(&mut w, "alta_mentiune", mentiune.trim())?;
        }

        end_elem(&mut w, "sect_B")?;
        end_elem(&mut w, "Bifa_B")?;
    }

    // ── Secțiunea C: sedii secundare + domiciliu fiscal (Bifa_C) ──
    if input.sect_c.has_data() {
        start_elem_attrs(&mut w, "Bifa_C", &[("valoare", "1")])?;
        start_elem(&mut w, "sect_C")?;

        for sediu in &input.sect_c.sedii_secundare {
            let adresa_s = trunc(sediu.adresa.trim(), 200);
            empty_elem_attrs(
                &mut w,
                "sediu_secundar",
                &[
                    ("tip", sediu.tip.trim()),
                    ("judet_cod", sediu.judet_cod.trim()),
                    ("adresa", &adresa_s),
                ],
            )?;
        }

        if let Some(ref df) = input.sect_c.domiciliu_fiscal_nou {
            let df_s = trunc(df.trim(), 200);
            write_text_elem(&mut w, "domiciliu_fiscal", &df_s)?;
        }

        end_elem(&mut w, "sect_C")?;
        end_elem(&mut w, "Bifa_C")?;
    }

    // ── Secțiunea D: radiere (Bifa_D) ──
    if input.sect_d.has_data() {
        start_elem_attrs(&mut w, "Bifa_D", &[("valoare", "1")])?;
        start_elem(&mut w, "sect_D")?;
        if let Some(ref motiv) = input.sect_d.motiv {
            write_text_elem(&mut w, "motiv_radiere", motiv.trim())?;
        }
        if let Some(ref data_r) = input.sect_d.data_radiere {
            write_text_elem(&mut w, "data_radiere", data_r.trim())?;
        }
        end_elem(&mut w, "sect_D")?;
        end_elem(&mut w, "Bifa_D")?;
    }

    end_elem(&mut w, D700_ROOT)?;
    Ok(pretty_print(&finish(w)?))
}

/// Codifică mențiunea TVA ca atribut `tip` (per structura_XML_D700_0126 + OPANAF 15/2026).
fn tva_mentiune_cod(m: &TvaMentiune) -> &'static str {
    match m {
        TvaMentiune::Inregistrare => "inregistrare",
        TvaMentiune::Anulare => "anulare",
        TvaMentiune::TrecereaLaLunar => "lunar",
        TvaMentiune::TrecereaLaTrimestrial => "trimestrial",
        TvaMentiune::TvaLaIncasareInregistrare => "tva_incasare_inreg",
        TvaMentiune::TvaLaIncasareAnulare => "tva_incasare_anulare",
    }
}

/// Codifică mențiunea de regim fiscal ca atribut `tip`.
fn regim_fiscal_cod(m: &RegimFiscalMentiune) -> &'static str {
    match m {
        RegimFiscalMentiune::TrecereaLaMicro => "micro",
        RegimFiscalMentiune::TrecereaLaProfit => "profit",
        RegimFiscalMentiune::ModificareFrecventaProfitLunar => "profit_lunar",
        RegimFiscalMentiune::ModificareFrecventaProfitTrimestrial => "profit_trimestrial",
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sect_a() -> D700SectA {
        D700SectA {
            cui: "12345674".into(),
            den: "Test SRL".into(),
            adresa: "Str. Test 1, București, Sector 1".into(),
            judet_cod: "40".into(),
            forma_juridica: "SRL".into(),
            repr_nume: "Popescu".into(),
            repr_prenume: "Ion".into(),
            repr_cnp: "1900101410019".into(),
            repr_functie: "Administrator".into(),
            telefon: "0721000000".into(),
            email: "test@test.ro".into(),
        }
    }

    // Structural tests — NOT DUK/XSD validation (no public XSD exists for D700).
    // These verify: well-formed XML, root element is <D700>, correct namespace v4,
    // Bifa-gated sections (Bifa_A always; Bifa_B when sect_B populated;
    // Bifa_C when sect_C populated; Bifa_D when sect_D populated),
    // felD derived (2=mențiuni, 3=radiere), header fields present.
    // Full validation requires D700Validator.jar from D700_20260423.zip via DUKIntegrator.

    /// D700 root MUST be `<D700>` (per OPANAF 15/2026, ed. 0126).
    /// NOT `<declaratie700>` — that was the previous guessed name.
    #[test]
    fn root_is_d700_not_declaratie700() {
        let input = D700Input {
            d_rec: 0,
            luna: Some(6),
            an: Some(2026),
            fel_d: None,
            dec_inreg: None,
            data_decl: None,
            total_plata_a: 0,
            nume_decl: None,
            pren_decl: None,
            func_decl: None,
            sect_a: sect_a(),
            sect_b: D700SectB {
                tva_mentiune: Some(TvaMentiune::Inregistrare),
                ..D700SectB::default()
            },
            sect_c: D700SectC::default(),
            sect_d: D700SectD::default(),
        };
        let xml = build_d700_xml(&input).unwrap();
        assert!(
            xml.contains("<D700 ") || xml.contains("<D700>"),
            "root must be <D700>: {xml}"
        );
        assert!(
            !xml.contains("<declaratie700"),
            "old guessed root <declaratie700> must NOT appear: {xml}"
        );
        assert!(
            xml.contains("</D700>"),
            "root close tag must be </D700>: {xml}"
        );
        assert!(
            xml.contains(r#"xmlns="mfp:anaf:dgti:d700:declaratie:v4""#),
            "namespace v4 must be present: {xml}"
        );
    }

    #[test]
    fn no_sections_returns_error() {
        let input = D700Input {
            d_rec: 0,
            luna: None,
            an: None,
            fel_d: None,
            dec_inreg: None,
            data_decl: None,
            total_plata_a: 0,
            nume_decl: None,
            pren_decl: None,
            func_decl: None,
            sect_a: sect_a(),
            sect_b: D700SectB::default(),
            sect_c: D700SectC::default(),
            sect_d: D700SectD::default(),
        };
        assert!(build_d700_xml(&input).is_err(), "empty D700 should fail");
    }

    #[test]
    fn bifa_sections_gated_correctly() {
        // sect_B only → Bifa_A + Bifa_B present; Bifa_C + Bifa_D absent.
        let input = D700Input {
            d_rec: 0,
            luna: Some(6),
            an: Some(2026),
            fel_d: None,
            dec_inreg: Some("010".into()),
            data_decl: Some("20.06.2026".into()),
            total_plata_a: 0,
            nume_decl: Some("Popescu".into()),
            pren_decl: Some("Ion".into()),
            func_decl: Some("Administrator".into()),
            sect_a: sect_a(),
            sect_b: D700SectB {
                tva_mentiune: Some(TvaMentiune::TrecereaLaTrimestrial),
                tva_data: Some("2026-07-01".into()),
                ..D700SectB::default()
            },
            sect_c: D700SectC::default(),
            sect_d: D700SectD::default(),
        };
        let xml = build_d700_xml(&input).unwrap();

        // Root + namespace
        assert!(
            xml.contains(&format!(r#"xmlns="{D700_NAMESPACE}""#)),
            "namespace: {xml}"
        );
        assert!(xml.contains(&format!("<{D700_ROOT}")), "root: {xml}");

        // Header attrs
        assert!(xml.contains(r#"luna="6""#), "luna: {xml}");
        assert!(xml.contains(r#"an="2026""#), "an: {xml}");
        assert!(xml.contains(r#"felD="2""#), "felD=2 for mențiuni: {xml}");
        assert!(xml.contains(r#"dec_inreg="010""#), "dec_inreg: {xml}");
        assert!(xml.contains(r#"totalPlata_A="0""#), "totalPlata_A: {xml}");
        assert!(xml.contains(r#"nume_decl="Popescu""#), "nume_decl: {xml}");
        assert!(
            xml.contains(r#"data_decl="20.06.2026""#),
            "data_decl: {xml}"
        );
        assert!(xml.contains(r#"cui="12345674""#), "cui: {xml}");

        // Bifa_A always present
        assert!(xml.contains(r#"<Bifa_A valoare="1">"#), "Bifa_A: {xml}");
        assert!(xml.contains("<sect_A>"), "sect_A: {xml}");
        assert!(xml.contains(r#"nume="Popescu""#), "repr_nume: {xml}");

        // Bifa_B present (sect_B has TVA mențiune)
        assert!(xml.contains(r#"<Bifa_B valoare="1">"#), "Bifa_B: {xml}");
        assert!(xml.contains("<sect_B>"), "sect_B: {xml}");
        assert!(xml.contains(r#"tip="trimestrial""#), "TVA cod: {xml}");

        // Bifa_C + Bifa_D absent
        assert!(!xml.contains("<Bifa_C"), "Bifa_C should be absent: {xml}");
        assert!(!xml.contains("<Bifa_D"), "Bifa_D should be absent: {xml}");
    }

    #[test]
    fn fel_d_is_3_for_radiere() {
        let input = D700Input {
            d_rec: 0,
            luna: None,
            an: None,
            fel_d: None, // auto-derived
            dec_inreg: None,
            data_decl: None,
            total_plata_a: 0,
            nume_decl: None,
            pren_decl: None,
            func_decl: None,
            sect_a: sect_a(),
            sect_b: D700SectB::default(),
            sect_c: D700SectC::default(),
            sect_d: D700SectD {
                motiv: Some("Dizolvare voluntară".into()),
                data_radiere: Some("2026-12-31".into()),
            },
        };
        let xml = build_d700_xml(&input).unwrap();
        assert!(xml.contains(r#"felD="3""#), "felD=3 for radiere: {xml}");
        assert!(xml.contains(r#"<Bifa_D valoare="1">"#), "Bifa_D: {xml}");
        assert!(xml.contains("<sect_D>"), "sect_D: {xml}");
        assert!(xml.contains("Dizolvare"), "motiv: {xml}");
    }

    #[test]
    fn secondary_office_and_domiciliu_in_bifa_c() {
        let input = D700Input {
            d_rec: 0,
            luna: None,
            an: None,
            fel_d: None,
            dec_inreg: None,
            data_decl: None,
            total_plata_a: 0,
            nume_decl: None,
            pren_decl: None,
            func_decl: None,
            sect_a: sect_a(),
            sect_b: D700SectB {
                tva_mentiune: Some(TvaMentiune::Inregistrare),
                ..D700SectB::default()
            },
            sect_c: D700SectC {
                sedii_secundare: vec![SediuSecundar {
                    judet_cod: "01".into(),
                    adresa: "Str. Secundar 1, Alba Iulia".into(),
                    tip: "inregistrare".into(),
                }],
                domiciliu_fiscal_nou: Some("Str. Nouă 5, Cluj-Napoca, CJ".into()),
            },
            sect_d: D700SectD::default(),
        };
        let xml = build_d700_xml(&input).unwrap();
        assert!(xml.contains(r#"<Bifa_C valoare="1">"#), "Bifa_C: {xml}");
        assert!(xml.contains("<sect_C>"), "sect_C: {xml}");
        assert!(xml.contains(r#"judet_cod="01""#), "sediu judet: {xml}");
        assert!(
            xml.contains("<domiciliu_fiscal>"),
            "domiciliu_fiscal: {xml}"
        );
        assert!(xml.contains("Cluj-Napoca"), "domiciliu value: {xml}");
    }

    #[test]
    fn xml_is_well_formed() {
        let input = D700Input {
            d_rec: 0,
            luna: Some(5),
            an: Some(2026),
            fel_d: None,
            dec_inreg: None,
            data_decl: None,
            total_plata_a: 0,
            nume_decl: None,
            pren_decl: None,
            func_decl: None,
            sect_a: sect_a(),
            sect_b: D700SectB {
                tva_mentiune: Some(TvaMentiune::Inregistrare),
                ..D700SectB::default()
            },
            sect_c: D700SectC::default(),
            sect_d: D700SectD::default(),
        };
        let xml = build_d700_xml(&input).unwrap();
        assert!(
            xml.contains("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"),
            "prolog: {xml}"
        );
        assert!(
            xml.contains(&format!("</{D700_ROOT}>")),
            "root close: {xml}"
        );
    }
}
