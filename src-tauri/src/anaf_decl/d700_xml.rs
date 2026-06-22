//! D700 — Declarație de înregistrare fiscală / Declarație de mențiuni / Declarație de radiere
//! (OPANAF 15/2026, ediția 0126, pentru persoane juridice și alte entități).
//!
//! **STRUCTURA CORECTĂ PER D700Validator.jar (DUK business-rule validation) — FĂRĂ XSD PUBLIC.**
//! D700 nu are XSD public descărcabil. Structura corectă este derivată prin testare directă
//! cu `D700Validator.jar` din pachetul `D700_20260423.zip` prin DUKIntegrator.
//!
//! Validare obligatorie înainte de depunere:
//!   `java -jar DUKIntegrator.jar -v D700 <xml> <result>`
//!
//! ## Structura XML v4 CORECTĂ (confirmată DUK GREEN — toate bifele sunt atribute root INTEGER)
//! ```text
//!   <D700 xmlns="mfp:anaf:dgti:d700:declaratie:v4"
//!         an="AAAA" luna="N"
//!         felD="2"             ← 2=mențiuni (radiere viitoare = alt felD)
//!         dec_inreg="010"      ← codul vectorului de înregistrare
//!         totalPlata_A="N"     ← Σ(Bife active) per R14 DUK
//!         cif="NNN"            ← CUI fără "RO"
//!         den="…"              ← denumire persoană juridică
//!         nume_decl="…" pren_decl="…" func_decl="…"
//!         Bifa_A="1"           ← Secțiunea A (date identificare)
//!         Bifa_B="1"           ← Secțiunea B (vector fiscal)
//!         Bifa_C="1"           ← Secțiunea C (sedii secundare)
//!         Bifa_D="1"           ← Secțiunea D (radiere)
//!         Bifa_F="1"           ← Secțiunea F (alte mențiuni)
//!         Bifa_G="1"           ← Secțiunea G
//!         Bifa_3b="1"          ← TVA trimestrial (schimbare perioadă fiscală)
//!         Bifa_B3="1"          ← TVA la încasare art.282
//!         Bifa_B11="1"         ← TVA la încasare anulare
//!         Bifa11_3b="1"        ← Trecere TVA trimestrial (dată specficică)
//!         Data_3b="ZZ.LL.AAAA" ← Data schimbare TVA la trimestrial>
//!   </D700>
//! ```
//!
//! ## ATENȚIE: structura corectă v4 NU are elemente copil (Bifa_A, sect_A, reprezentant etc.)!
//! Structura element-based (`<Bifa_A valoare="1"><sect_A>`) este GREȘITĂ pentru v4
//! și este respinsă de DUKIntegrator cu erori de structură.
//! Toate bifele și datele sunt atribute pe elementul rădăcină `<D700>`.
//!
//! ## Atribute INVALIDE în v4 (respinse de DUK)
//! - `d_rec` (nu există în v4)
//! - `adresa` (nu există ca atribut root în v4)
//! - `judet_cod` (nu există ca atribut root în v4)
//! - `forma_juridica` (nu există ca atribut root în v4)

use serde::{Deserialize, Serialize};

use crate::anaf_decl::xml::{end_elem, finish, new_writer, pretty_print, start_elem_attrs, trunc};
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

/// Datele complete ale declarației D700 (structura v4, FLAT — toate bifele sunt atribute root).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D700Input {
    /// Luna depunerii (1-12). Opțional — dacă absent, se omite din header.
    #[serde(default)]
    pub luna: Option<u32>,
    /// Anul depunerii. Opțional — dacă absent, se omite din header.
    #[serde(default)]
    pub an: Option<i32>,
    /// felD: tipul declarației — 2 = mențiuni.
    /// Derivat automat dacă absent (2 dacă are bife, altfel eroare).
    #[serde(default)]
    pub fel_d: Option<u8>,
    /// dec_inreg: codul vectorului de înregistrare (010/013/015/016/020/030/070).
    /// Opțional — emis doar dacă este setat.
    #[serde(default)]
    pub dec_inreg: Option<String>,
    /// Suma totală de plată (lei). Per R14 DUK: Σ(Bife active A+B+C+D+F+G).
    /// Calculat automat dacă zero.
    #[serde(default)]
    pub total_plata_a: i64,
    /// CUI-ul persoanei impozabile (fără „RO", 2-10 cifre).
    #[serde(default)]
    pub cif: String,
    /// Denumirea persoanei juridice (max 200 chr).
    #[serde(default)]
    pub den: String,
    /// Declarant — nume (max 75 chr).
    #[serde(default)]
    pub nume_decl: Option<String>,
    /// Declarant — prenume (max 75 chr).
    #[serde(default)]
    pub pren_decl: Option<String>,
    /// Declarant — funcție (max 50 chr).
    #[serde(default)]
    pub func_decl: Option<String>,
    /// Bifa_A = 1 → Secțiunea A activă (date identificare reprezentant).
    #[serde(default)]
    pub bifa_a: bool,
    /// Bifa_B = 1 → Secțiunea B activă (vector fiscal TVA, regim fiscal, etc.)
    #[serde(default)]
    pub bifa_b: bool,
    /// Bifa_C = 1 → Secțiunea C activă (sedii secundare, domiciliu fiscal)
    #[serde(default)]
    pub bifa_c: bool,
    /// Bifa_D = 1 → Secțiunea D activă (radiere)
    #[serde(default)]
    pub bifa_d: bool,
    /// Bifa_F = 1 → Secțiunea F activă (alte mențiuni)
    #[serde(default)]
    pub bifa_f: bool,
    /// Bifa_G = 1 → Secțiunea G activă
    #[serde(default)]
    pub bifa_g: bool,
    /// Bifa_3b = 1 → TVA trimestrial (schimbare perioadă fiscală la trimestrial)
    #[serde(default)]
    pub bifa_3b: bool,
    /// Bifa_B3 = 1 → TVA la încasare art.282 (activare)
    #[serde(default)]
    pub bifa_b3: bool,
    /// Bifa_B11 = 1 → TVA la încasare art.282 (anulare)
    #[serde(default)]
    pub bifa_b11: bool,
    /// Bifa11_3b = 1 → Trecere la TVA trimestrial cu dată specifică (Data_3b)
    #[serde(default)]
    pub bifa11_3b: bool,
    /// Data_3b: data trecerii la TVA trimestrial (ZZ.LL.AAAA). Requries bifa11_3b=true.
    #[serde(default)]
    pub data_3b: Option<String>,
    /// Bifa_B8 = 1 → sub-bifă B8 din vectorul fiscal (alte obligații secțiunea B).
    /// DUK R51: Bifa_B=1 necesită cel puțin una din Bifa_B1..Bifa_B8=1.
    /// DUK R125: Bifa_B8=1 necesită Bifa_8b ∈ {1,2,3,4,5,6}.
    #[serde(default)]
    pub bifa_b8: bool,
    /// Bifa_8b: tipul obligației B8 (1-6, requries bifa_b8=true).
    #[serde(default)]
    pub bifa_8b: Option<u8>,
    /// DEPRECATED: sect_a kept for backward compat (CUI/den migrated to root cif/den).
    /// Populați câmpurile `cif`, `den`, `numeDecl`, `prenDecl`, `funcDecl` direct.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sect_a: Option<D700SectA>,
    /// DEPRECATED: sect_b kept for backward compat (TVA bife → bifa_3b/bifa_b3/bifa_b11/bifa_b).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sect_b: Option<D700SectB>,
    /// DEPRECATED: sect_c kept for backward compat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sect_c: Option<D700SectC>,
    /// DEPRECATED: sect_d kept for backward compat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sect_d: Option<D700SectD>,
}

// ── Emitorul XML ──────────────────────────────────────────────────────────────

/// Construiește XML-ul D700 (declarație de înregistrare/mențiuni/radiere — vector fiscal).
///
/// **Structura v4 FLAT** — toate bifele sunt atribute INTEGER pe elementul rădăcină `<D700>`.
/// Nu există elemente copil (structura veche cu `<Bifa_A valoare="1"><sect_A>` era GREȘITĂ).
///
/// Validare obligatorie înainte de depunere:
/// `java -jar DUKIntegrator.jar -v D700 <xml> <result>`
///
/// ## Atribute root generate
/// - Obligatorii: `xmlns`, `an`, `luna`, `felD`, `totalPlata_A`, `cif`, `den`
/// - Opționale identificare: `dec_inreg`, `numeDecl`, `prenDecl`, `funcDecl`
/// - Bife active (0 omis, 1 emis): `Bifa_A`, `Bifa_B`, `Bifa_C`, `Bifa_D`, `Bifa_F`, `Bifa_G`
/// - Bife speciale TVA: `Bifa_3b`, `Bifa_B3`, `Bifa_B11`, `Bifa11_3b`, `Data_3b`
///
/// ## R14 DUK: totalPlata_A = Σ(Bife active A+B+C+D+F+G)
/// Calculat automat dacă `input.total_plata_a == 0`.
///
/// # Erori
/// Returnează eroare dacă nicio bifă nu este activată (D700 fără mențiuni e invalidă).
pub fn build_d700_xml(input: &D700Input) -> AppResult<String> {
    // Migrate deprecated sect_* fields to flat booleans if used.
    let bifa_a = input.bifa_a || input.sect_a.is_some();
    let bifa_b = input.bifa_b || input.sect_b.as_ref().map(|s| s.has_data()).unwrap_or(false);
    let bifa_c = input.bifa_c || input.sect_c.as_ref().map(|s| s.has_data()).unwrap_or(false);
    let bifa_d = input.bifa_d || input.sect_d.as_ref().map(|s| s.has_data()).unwrap_or(false);
    let bifa_f = input.bifa_f;
    let bifa_g = input.bifa_g;

    if !bifa_a && !bifa_b && !bifa_c && !bifa_d && !bifa_f && !bifa_g {
        return Err(AppError::Validation(
            "D700: nicio bifă activă (A, B, C, D, F, G). Selectați cel puțin o secțiune.".into(),
        ));
    }

    // DUK R51 GUARDRAIL: Bifa_B=1 necesită cel puțin una din sub-bifele B1..B8.
    // Sub-bifele B3 și B11 sunt mapate de bifa_b3/bifa_b11; B8 de bifa_b8.
    if bifa_b && !input.bifa_b3 && !input.bifa_b11 && !input.bifa_b8 {
        return Err(AppError::Validation(
            "D700 R51: Bifa_B=1 necesită cel puțin o sub-bifă activă (Bifa_B3=TVA la încasare, \
             Bifa_B11=anulare TVA la încasare, sau Bifa_B8=altă obligație din secțiunea B). \
             Selectați sub-bifa corespunzătoare mențiunii."
                .into(),
        ));
    }

    // R14: totalPlata_A = Σ(Bife active A+B+C+D+F+G).
    let bife_sum: i64 = [bifa_a, bifa_b, bifa_c, bifa_d, bifa_f, bifa_g]
        .iter()
        .filter(|&&b| b)
        .count() as i64;
    let total_plata = if input.total_plata_a == 0 {
        bife_sum
    } else {
        input.total_plata_a
    };

    // felD: 2 = mențiuni (default).
    let fel_d = input.fel_d.unwrap_or(2);

    // CIF/den: prefer flat fields, fallback to deprecated sect_a.
    let cif = if !input.cif.is_empty() {
        input.cif.trim().to_string()
    } else {
        input
            .sect_a
            .as_ref()
            .map(|a| a.cui.trim().to_string())
            .unwrap_or_default()
    };
    let den_val = if !input.den.is_empty() {
        trunc(input.den.trim(), 200)
    } else {
        input
            .sect_a
            .as_ref()
            .map(|a| trunc(a.den.trim(), 200))
            .unwrap_or_default()
    };
    let nm = input.nume_decl.as_deref().unwrap_or("").trim().to_string();
    let pr = input.pren_decl.as_deref().unwrap_or("").trim().to_string();
    let fc = input.func_decl.as_deref().unwrap_or("").trim().to_string();

    // Stringify values.
    let an_s = input.an.map(|v| v.to_string()).unwrap_or_default();
    let luna_s = input.luna.map(|v| v.to_string()).unwrap_or_default();
    let fel_d_s = fel_d.to_string();
    let total_s = total_plata.to_string();
    let bifa_a_s = if bifa_a { "1" } else { "0" };
    let bifa_b_s = if bifa_b { "1" } else { "0" };
    let bifa_c_s = if bifa_c { "1" } else { "0" };
    let bifa_d_s = if bifa_d { "1" } else { "0" };
    let bifa_f_s = if bifa_f { "1" } else { "0" };
    let bifa_g_s = if bifa_g { "1" } else { "0" };
    let bifa_3b_s = if input.bifa_3b { "1" } else { "0" };
    let bifa_b3_s = if input.bifa_b3 { "1" } else { "0" };
    let bifa_b11_s = if input.bifa_b11 { "1" } else { "0" };
    let bifa11_3b_s = if input.bifa11_3b { "1" } else { "0" };
    let bifa_b8_s = if input.bifa_b8 { "1" } else { "0" };
    let bifa_8b_s = input.bifa_8b.map(|v| v.to_string());

    let nm_trunc = trunc(&nm, 75);
    let pr_trunc = trunc(&pr, 75);
    let fc_trunc = trunc(&fc, 50);

    // Build root attributes in the correct v4 order (confirmed DUK GREEN).
    let mut attrs: Vec<(&str, &str)> = vec![("xmlns", D700_NAMESPACE)];
    if !an_s.is_empty() {
        attrs.push(("an", &an_s));
    }
    if !luna_s.is_empty() {
        attrs.push(("luna", &luna_s));
    }
    attrs.push(("felD", &fel_d_s));
    if let Some(ref di) = input.dec_inreg {
        attrs.push(("dec_inreg", di.trim()));
    }
    attrs.push(("totalPlata_A", &total_s));
    attrs.push(("cif", &cif));
    attrs.push(("den", &den_val));
    if !nm_trunc.is_empty() {
        attrs.push(("nume_decl", &nm_trunc));
    }
    if !pr_trunc.is_empty() {
        attrs.push(("pren_decl", &pr_trunc));
    }
    if !fc_trunc.is_empty() {
        attrs.push(("func_decl", &fc_trunc));
    }
    // Bife (emit all active ones; omit 0-value bife per DUK advisory-only convention).
    if bifa_a {
        attrs.push(("Bifa_A", bifa_a_s));
    }
    if bifa_b {
        attrs.push(("Bifa_B", bifa_b_s));
    }
    if bifa_c {
        attrs.push(("Bifa_C", bifa_c_s));
    }
    if bifa_d {
        attrs.push(("Bifa_D", bifa_d_s));
    }
    if bifa_f {
        attrs.push(("Bifa_F", bifa_f_s));
    }
    if bifa_g {
        attrs.push(("Bifa_G", bifa_g_s));
    }
    // TVA period-change bife (optional).
    if input.bifa_3b {
        attrs.push(("Bifa_3b", bifa_3b_s));
    }
    if input.bifa_b3 {
        attrs.push(("Bifa_B3", bifa_b3_s));
    }
    if input.bifa_b11 {
        attrs.push(("Bifa_B11", bifa_b11_s));
    }
    if input.bifa11_3b {
        attrs.push(("Bifa11_3b", bifa11_3b_s));
        if let Some(ref d3b) = input.data_3b {
            attrs.push(("Data_3b", d3b.trim()));
        }
    }
    if input.bifa_b8 {
        attrs.push(("Bifa_B8", bifa_b8_s));
        if let Some(ref v8b) = bifa_8b_s {
            attrs.push(("Bifa_8b", v8b.as_str()));
        }
    }

    let mut w = new_writer()?;
    start_elem_attrs(&mut w, D700_ROOT, &attrs)?;
    end_elem(&mut w, D700_ROOT)?;
    Ok(pretty_print(&finish(w)?))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: D700Input minimal with Bifa_A and Bifa_B active.
    /// DUK R51: Bifa_B=1 needs at least one sub-bifa (B3/B11/B8).
    /// Uses bifa_b8=true + bifa_8b=Some(1) as the minimal valid sub-bifa for section B.
    fn input_ab() -> D700Input {
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

    // Structural tests — NOT DUK/XSD validation (no public XSD exists for D700).
    // These verify: well-formed XML, root element is <D700>, correct namespace v4,
    // flat Bifa_X attributes (NOT element-based), header fields present.
    // Full validation requires D700Validator.jar from D700_20260423.zip via DUKIntegrator.

    /// D700 root MUST be `<D700>` (per OPANAF 15/2026, ed. 0126) with v4 namespace.
    #[test]
    fn root_is_d700_v4_namespace() {
        let xml = build_d700_xml(&input_ab()).unwrap();
        assert!(
            xml.contains("<D700 ") || xml.contains("<D700>"),
            "root must be <D700>: {xml}"
        );
        assert!(
            !xml.contains("<declaratie700"),
            "old guessed root must NOT appear: {xml}"
        );
        assert!(xml.contains("</D700>"), "root close tag: {xml}");
        assert!(
            xml.contains(r#"xmlns="mfp:anaf:dgti:d700:declaratie:v4""#),
            "namespace v4: {xml}"
        );
    }

    /// D700 v4 uses FLAT ATTRIBUTES — NO child elements (the old structure was wrong).
    #[test]
    fn bifa_flags_are_root_attributes_not_elements() {
        let xml = build_d700_xml(&input_ab()).unwrap();
        // v4 correct: Bifa_A and Bifa_B as root integer attributes
        assert!(
            xml.contains(r#"Bifa_A="1""#),
            "Bifa_A must be a root attr: {xml}"
        );
        assert!(
            xml.contains(r#"Bifa_B="1""#),
            "Bifa_B must be a root attr: {xml}"
        );
        // MUST NOT contain old element-based structure
        assert!(
            !xml.contains("<Bifa_A"),
            "old element <Bifa_A> must NOT appear in v4: {xml}"
        );
        assert!(
            !xml.contains("<sect_A"),
            "old element <sect_A> must NOT appear in v4: {xml}"
        );
        assert!(
            !xml.contains("<Bifa_B"),
            "old element <Bifa_B> must NOT appear in v4: {xml}"
        );
    }

    #[test]
    fn no_bife_returns_error() {
        let input = D700Input {
            luna: None,
            an: None,
            fel_d: None,
            dec_inreg: None,
            total_plata_a: 0,
            cif: "12345674".into(),
            den: "Test SRL".into(),
            nume_decl: None,
            pren_decl: None,
            func_decl: None,
            bifa_a: false,
            bifa_b: false,
            bifa_c: false,
            bifa_d: false,
            bifa_f: false,
            bifa_g: false,
            bifa_3b: false,
            bifa_b3: false,
            bifa_b11: false,
            bifa11_3b: false,
            data_3b: None,
            bifa_b8: false,
            bifa_8b: None,
            sect_a: None,
            sect_b: None,
            sect_c: None,
            sect_d: None,
        };
        assert!(
            build_d700_xml(&input).is_err(),
            "D700 without bife must fail"
        );
    }

    #[test]
    fn header_attrs_present_and_correct() {
        let xml = build_d700_xml(&input_ab()).unwrap();
        assert!(xml.contains(r#"luna="6""#), "luna: {xml}");
        assert!(xml.contains(r#"an="2026""#), "an: {xml}");
        assert!(xml.contains(r#"felD="2""#), "felD=2: {xml}");
        assert!(xml.contains(r#"dec_inreg="010""#), "dec_inreg: {xml}");
        assert!(xml.contains(r#"cif="12345674""#), "cif: {xml}");
        assert!(xml.contains(r#"den="Test SRL""#), "den: {xml}");
        assert!(xml.contains(r#"nume_decl="Popescu""#), "nume_decl: {xml}");
        assert!(xml.contains(r#"pren_decl="Ion""#), "pren_decl: {xml}");
        assert!(
            xml.contains(r#"func_decl="Administrator""#),
            "func_decl: {xml}"
        );
        // Invalid v4 attrs must NOT appear
        assert!(!xml.contains("d_rec"), "d_rec is INVALID in v4: {xml}");
        assert!(
            !xml.contains("judet_cod"),
            "judet_cod is INVALID in v4: {xml}"
        );
        assert!(
            !xml.contains("forma_juridica"),
            "forma_juridica is INVALID in v4: {xml}"
        );
    }

    #[test]
    fn total_plata_a_auto_computed_as_sum_of_bife() {
        // Bifa_A + Bifa_B = 2 active bife → R14: totalPlata_A = 2
        let xml = build_d700_xml(&input_ab()).unwrap();
        assert!(
            xml.contains(r#"totalPlata_A="2""#),
            "totalPlata_A must be 2 (Bifa_A + Bifa_B): {xml}"
        );
    }

    #[test]
    fn total_plata_a_override_respected() {
        let mut input = input_ab();
        input.total_plata_a = 5; // explicit override
        let xml = build_d700_xml(&input).unwrap();
        assert!(
            xml.contains(r#"totalPlata_A="5""#),
            "explicit totalPlata_A must be respected: {xml}"
        );
    }

    #[test]
    fn tva_trimestrial_bife_emitted() {
        // TVA period change: bifa_b=true + bifa_3b=true + bifa_b3=true (DUK R51 satisfied by Bifa_B3)
        // + bifa11_3b=true + data_3b
        let input = D700Input {
            luna: Some(6),
            an: Some(2026),
            fel_d: None,
            dec_inreg: Some("010".into()),
            total_plata_a: 0,
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
            bifa_3b: true,
            bifa_b3: true, // Satisfies DUK R51 (Bifa_B3 is a sub-bifa of B)
            bifa_b11: false,
            bifa11_3b: true,
            data_3b: Some("01.07.2026".into()),
            bifa_b8: false,
            bifa_8b: None,
            sect_a: None,
            sect_b: None,
            sect_c: None,
            sect_d: None,
        };
        let xml = build_d700_xml(&input).unwrap();
        assert!(xml.contains(r#"Bifa_3b="1""#), "Bifa_3b: {xml}");
        assert!(xml.contains(r#"Bifa_B3="1""#), "Bifa_B3: {xml}");
        assert!(xml.contains(r#"Bifa11_3b="1""#), "Bifa11_3b: {xml}");
        assert!(xml.contains(r#"Data_3b="01.07.2026""#), "Data_3b: {xml}");
    }

    #[test]
    fn xml_is_well_formed_and_has_prolog() {
        let xml = build_d700_xml(&input_ab()).unwrap();
        assert!(
            xml.contains("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"),
            "prolog: {xml}"
        );
        assert!(xml.contains("</D700>"), "close tag: {xml}");
    }

    /// Migration: D700Input with deprecated sect_b (has tva_mentiune) triggers bifa_b=true.
    /// Note: DUK R51 requires a sub-bifa when Bifa_B=1; use bifa_b8=true + bifa_8b=Some(1).
    #[test]
    fn deprecated_sect_b_migrates_to_bifa_b() {
        let input = D700Input {
            luna: Some(6),
            an: Some(2026),
            fel_d: None,
            dec_inreg: None,
            total_plata_a: 0,
            cif: "12345674".into(),
            den: "Test SRL".into(),
            nume_decl: None,
            pren_decl: None,
            func_decl: None,
            bifa_a: false,
            bifa_b: false, // NOT set here (migrated from sect_b)
            bifa_c: false,
            bifa_d: false,
            bifa_f: false,
            bifa_g: false,
            bifa_3b: false,
            bifa_b3: false,
            bifa_b11: false,
            bifa11_3b: false,
            data_3b: None,
            bifa_b8: true,    // DUK R51: sub-bifa for Bifa_B (required)
            bifa_8b: Some(1), // DUK R125: required when Bifa_B8=1
            sect_a: None,
            sect_b: Some(D700SectB {
                tva_mentiune: Some(TvaMentiune::Inregistrare),
                ..D700SectB::default()
            }),
            sect_c: None,
            sect_d: None,
        };
        let xml = build_d700_xml(&input).unwrap();
        // sect_b has tva_mentiune → bifa_b should be activated
        assert!(
            xml.contains(r#"Bifa_B="1""#),
            "deprecated sect_b must activate Bifa_B: {xml}"
        );
    }
}
