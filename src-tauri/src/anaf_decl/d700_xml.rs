//! D700 — Declarație de înregistrare fiscală / Declarație de mențiuni / Declarație de radiere
//! (OPANAF 15/2026, ediția 0126, pentru persoane juridice și alte entități).
//!
//! **STRUCTURA CORECTĂ PER SPECIFICAȚIE — NESUPUSĂ VALIDĂRII DUK / XSD.**
//! Namespace-ul și versiunea schemei (`D700_NAMESPACE`) sunt marcate ca TODO-verify:
//! acestea TREBUIE verificate față de pachetul oficial Soft J (DUKIntegrator) și față de
//! XSD-ul oficial ANAF înainte de depunerea electronică prin SPV.
//!
//! ## Structura declarației (secțiunile A-D)
//! - **Secțiunea A** (`sect_A`): Date de identificare fiscală (CUI, denumire, adresă, formă
//!   juridică, date reprezentant legal etc.).
//! - **Secțiunea B** (`sect_B`): Vector fiscal — înregistrare/anulare TVA, perioadă TVA
//!   (lunar↔trimestrial), TVA la încasare, micro↔profit, frecvența impozit profit, etc.
//! - **Secțiunea C** (`sect_C`): Sedii secundare și domiciliu fiscal (înregistrare / modificare
//!   / radiere).
//! - **Secțiunea D** (`sect_D`): Radiere — motivul și data radierii.
//!
//! Forma electronică se depune prin SPV (Spațiul Privat Virtual), nu prin PDF.
//!
//! ## IMPORTANT — Validare obligatorie înainte de depunere
//! Înainte de depunerea la ANAF, XML-ul generat TREBUIE validat cu DUKIntegrator împotriva
//! XSD-ului oficial. Obțineți XSD-ul din pachetul Soft J de pe site-ul ANAF (declaratii.anaf.ro).

use serde::{Deserialize, Serialize};

use crate::anaf_decl::xml::{
    empty_elem_attrs, end_elem, finish, new_writer, pretty_print, start_elem, start_elem_attrs,
    trunc, write_text_elem,
};
use crate::error::{AppError, AppResult};

// ── Schema version — TODO: verify against official ANAF XSD + DUKIntegrator ──

/// Namespace D700, ediția 0126 (OPANAF 15/2026).
/// **TODO-verify**: Confirmați versiunea exactă (vN) față de XSD-ul oficial din pachetul
/// Soft J publicat pe declaratii.anaf.ro (ediția 0126).
pub const D700_NAMESPACE: &str = "mfp:anaf:dgti:d700:declaratie:v4";

/// Elementul rădăcină al documentului D700.
pub const D700_ROOT: &str = "declaratie700";

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
    /// Alte mențiuni de vector fiscal — câmp liber (ex. înregistrare impozit construcții, etc.).
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
    /// Data radierii (ISO YYYY-MM-DD).
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
/// **STRUCTURA CORECTĂ PER SPECIFICAȚIE — NESUPUSĂ VALIDĂRII DUK/XSD.**
/// Verificați namespace-ul (`D700_NAMESPACE`) și structura față de XSD-ul oficial ANAF
/// (ediția 0126, OPANAF 15/2026) înainte de depunerea electronică prin SPV.
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

    let mut w = new_writer()?;

    start_elem_attrs(
        &mut w,
        D700_ROOT,
        &[
            ("xmlns", D700_NAMESPACE),
            ("d_rec", &d_rec_s),
            ("cui", a.cui.trim()),
            ("den", &den),
            ("adresa", &adresa),
            ("judet_cod", a.judet_cod.trim()),
            ("forma_juridica", a.forma_juridica.trim()),
        ],
    )?;

    // ── Secțiunea A: date reprezentant legal ──
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

    // ── Secțiunea B: vector fiscal ──
    if input.sect_b.has_data() {
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
    }

    // ── Secțiunea C: sedii secundare + domiciliu fiscal ──
    if input.sect_c.has_data() {
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
    }

    // ── Secțiunea D: radiere ──
    if input.sect_d.has_data() {
        start_elem(&mut w, "sect_D")?;
        if let Some(ref motiv) = input.sect_d.motiv {
            write_text_elem(&mut w, "motiv_radiere", motiv.trim())?;
        }
        if let Some(ref data_r) = input.sect_d.data_radiere {
            write_text_elem(&mut w, "data_radiere", data_r.trim())?;
        }
        end_elem(&mut w, "sect_D")?;
    }

    end_elem(&mut w, D700_ROOT)?;
    Ok(pretty_print(&finish(w)?))
}

/// Codifică mențiunea TVA ca atribut `tip` (valori conform structurii OPANAF 15/2026 — TODO-verify).
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

    /// Structural tests — NOT DUK/XSD validation (no official XSD bundled).
    /// These verify: well-formed XML, correct namespace, section B present with TVA mention,
    /// sections C/D absent when empty. DUK validation requires the official XSD from ANAF.

    #[test]
    fn no_sections_returns_error() {
        let input = D700Input {
            d_rec: 0,
            sect_a: sect_a(),
            sect_b: D700SectB::default(),
            sect_c: D700SectC::default(),
            sect_d: D700SectD::default(),
        };
        assert!(build_d700_xml(&input).is_err(), "empty D700 should fail");
    }

    #[test]
    fn tva_period_change_mentiune_emits_sect_b() {
        // D700 with a TVA period change (monthly → quarterly) in section B.
        let input = D700Input {
            d_rec: 0,
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
            "namespace missing: {xml}"
        );
        assert!(
            xml.contains(&format!("<{D700_ROOT}")),
            "root missing: {xml}"
        );
        assert!(xml.contains(r#"cui="12345674""#), "cui missing: {xml}");

        // Secțiunea A — reprezentant
        assert!(xml.contains("<sect_A>"), "sect_A missing: {xml}");
        assert!(
            xml.contains(r#"nume="Popescu""#),
            "repr_nume missing: {xml}"
        );

        // Secțiunea B — mențiune TVA → trimestrial
        assert!(xml.contains("<sect_B>"), "sect_B missing: {xml}");
        assert!(
            xml.contains(r#"tip="trimestrial""#),
            "TVA period cod missing: {xml}"
        );
        assert!(
            xml.contains(r#"data_aplicare="2026-07-01""#),
            "TVA data missing: {xml}"
        );

        // Secțiunile C și D — absente
        assert!(!xml.contains("<sect_C>"), "sect_C should be absent: {xml}");
        assert!(!xml.contains("<sect_D>"), "sect_D should be absent: {xml}");

        // XML bine-format
        assert!(
            xml.contains("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"),
            "prolog: {xml}"
        );
        assert!(
            xml.contains(&format!("</{D700_ROOT}>")),
            "root close: {xml}"
        );
    }

    #[test]
    fn secondary_office_and_domiciliu_in_sect_c() {
        let input = D700Input {
            d_rec: 0,
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
        assert!(xml.contains("<sect_C>"), "sect_C missing: {xml}");
        assert!(xml.contains(r#"judet_cod="01""#), "sediu judet: {xml}");
        assert!(
            xml.contains("<domiciliu_fiscal>"),
            "domiciliu_fiscal: {xml}"
        );
        assert!(xml.contains("Cluj-Napoca"), "domiciliu value: {xml}");
    }

    #[test]
    fn radiere_in_sect_d() {
        let input = D700Input {
            d_rec: 0,
            sect_a: sect_a(),
            sect_b: D700SectB::default(),
            sect_c: D700SectC::default(),
            sect_d: D700SectD {
                motiv: Some("Dizolvare și lichidare conform Legii 31/1990.".into()),
                data_radiere: Some("2026-12-31".into()),
            },
        };
        let xml = build_d700_xml(&input).unwrap();
        assert!(xml.contains("<sect_D>"), "sect_D missing: {xml}");
        assert!(xml.contains("Dizolvare"), "motiv missing: {xml}");
        assert!(xml.contains("2026-12-31"), "data_radiere missing: {xml}");
    }
}
