//! D205 — Declarația informativă privind impozitul reținut la sursă, pe beneficiari de venit
//! (OPANAF 179/2022, mod. OPANAF 102/2025). Capitolul DIVIDENDE (`tip_venit` 08). Model `:v3`.
//!
//! Structură cu TOATE datele ca ATRIBUTE XML și sume în LEI ÎNTREGI (N15). Recapitulația `sect_II` e
//! SELF-CLOSING, iar rândurile `benef` sunt SIBLINGS (copii direcți ai `declaratie205`, DUPĂ sect_II) —
//! NU imbricate. (Structura + numele atributelor au fost CONFIRMATE rulând validatorul oficial pe XML
//! golden până la „Validare fără erori", corectând câteva ipoteze din design — vezi D205_EMITTER_DESIGN.md.)
//! ```text
//!   <declaratie205 …antet… cui adresa totalPlata_A>              — 1 / declarație (luna=12, an de venit)
//!     <sect_II tip_venit="08" nrben Tcastig Tpierd T_VB T_GAR Tbaza Timp/>  — recapitulație (self-closing)
//!     <benef id_inreg="1" tip_venit1="08" …/>                    — 1 / beneficiar (CNP), SIBLING
//!   </declaratie205>
//! ```
//! Folosește emitorul cu ATRIBUTE ([`crate::anaf_decl::xml::start_elem_attrs`] / [`empty_elem_attrs`]),
//! nu pe cel cu elemente-copil (D300/bilanț). Validatorul oficial e `D205Validator.jar` (`-v D205`),
//! inclus în `resources/duk/lib/`; testul `tests::duk_validates_d205` rulează DUK end-to-end pe golden.

use rust_decimal::Decimal;

use crate::anaf_decl::version::resolve;
use crate::anaf_decl::xml::{
    empty_elem_attrs, end_elem, finish, new_writer, start_elem_attrs, trunc,
};
use crate::anaf_decl::{round_lei, DeclKind};
use crate::error::{AppError, AppResult};

/// Antetul D205 (datele plătitorului/declarantului) pentru un an de venit. Numele atributelor sunt cele
/// din validatorul oficial (`D205Validator.jar`, schema curentă v8): declarantul e identificat prin
/// `cui` (NU `cif`), iar `adresa` e obligatorie. NU există atribut `version`.
pub struct D205Header {
    /// `cui` — codul fiscal al declarantului (CUI, doar cifre, fără „RO"). OBLIGATORIU.
    pub cui: String,
    /// `adresa` — adresa declarantului. OBLIGATORIE.
    pub adresa: String,
    /// `den` — denumirea declarantului.
    pub den: String,
    /// `an` — anul de venit raportat (≥ 2025).
    pub an: i32,
    /// `d_rec` — 0 = declarație inițială, 1 = rectificativă.
    pub d_rec: u8,
    pub nume_declar: String,
    pub prenume_declar: String,
    pub functie_declar: String,
}

/// Un beneficiar de dividende (rând `<benef>`, `tip_venit` 08), cu sumele agregate pe anul de venit.
/// Sumele sunt `Decimal` (2 zecimale upstream) și se rotunjesc la lei întregi la emitere (regula
/// A21.46: rotunjirea agregatului, nu per-rând).
#[derive(Debug, Clone)]
pub struct D205Beneficiary {
    /// `cifR` — CNP-ul beneficiarului (validat în amonte, N13 mod-11).
    pub cnp: String,
    /// `den1` — numele beneficiarului.
    pub name: String,
    /// `baza1` — baza de calcul (brutul dividendelor atribuibile beneficiarului).
    pub baza: Decimal,
    /// `imp1` — impozitul reținut.
    pub impozit: Decimal,
    /// `divid_D` — dividende distribuite.
    pub distribuit: Decimal,
    /// `divid_P` — dividende plătite.
    pub platit: Decimal,
    /// `Rezid` — 1 = rezident fiscal RO (D205); nerezidenții merg pe D207 și NU intră aici.
    pub resident: bool,
}

/// Plafonul de lungime pentru câmpurile de denumire (den/den1). Tăiere char-safe (diacritice RO).
const NAME_MAX: usize = 75;
/// Plafonul de lungime pentru adresă.
const ADDR_MAX: usize = 200;

/// Construiește XML-ul D205 (`:v3`) pentru anul `header.an`, capitolul dividende. Toți beneficiarii sunt
/// de tip 08. Sumele se rotunjesc la lei întregi ([`round_lei`]). `totalPlata_A = Σ_sect (nrben + Tbaza
/// + Timp)` — pentru o singură secțiune (08): `nrben + Tbaza + Timp`. Eroare dacă lista e goală.
pub fn build_d205_xml(header: &D205Header, beneficiaries: &[D205Beneficiary]) -> AppResult<String> {
    if beneficiaries.is_empty() {
        return Err(AppError::Validation(
            "D205: nu există beneficiari de dividende cu CNP pentru anul selectat.".into(),
        ));
    }
    // Namespace + root din registrul de versiuni (an de venit → 31 dec al anului).
    let period = chrono::NaiveDate::from_ymd_opt(header.an, 12, 31)
        .ok_or_else(|| AppError::Validation("An D205 invalid.".into()))?;
    let sv = resolve(DeclKind::D205, period)?;

    // Agregatele secțiunii (dividende), în lei întregi.
    let nrben = beneficiaries.len() as i64;
    let t_baza: i64 = beneficiaries.iter().map(|b| round_lei(b.baza)).sum();
    let t_imp: i64 = beneficiaries.iter().map(|b| round_lei(b.impozit)).sum();
    let total_plata_a = nrben + t_baza + t_imp;

    let an = header.an.to_string();
    let d_rec = header.d_rec.to_string();
    let nrben_s = nrben.to_string();
    let t_baza_s = t_baza.to_string();
    let t_imp_s = t_imp.to_string();
    let total_s = total_plata_a.to_string();
    let den = trunc(header.den.trim(), NAME_MAX);
    let adresa = trunc(header.adresa.trim(), ADDR_MAX);
    let nume = trunc(header.nume_declar.trim(), NAME_MAX);
    let prenume = trunc(header.prenume_declar.trim(), NAME_MAX);
    let functie = trunc(header.functie_declar.trim(), NAME_MAX);

    let mut w = new_writer()?;
    start_elem_attrs(
        &mut w,
        sv.root_element, // "declaratie205"
        &[
            ("xmlns", sv.namespace),
            ("luna", "12"), // D205 e anuală: luna de raportare = 12
            ("an", &an),
            ("d_rec", &d_rec),
            ("cui", header.cui.trim()), // declarantul e identificat prin `cui` (NU `cif`)
            ("adresa", &adresa),
            ("den", &den),
            ("nume_declar", &nume),
            ("prenume_declar", &prenume),
            ("functie_declar", &functie),
            ("totalPlata_A", &total_s),
        ],
    )?;

    // Recapitulația secțiunii (SELF-CLOSING) vine ÎNAINTEA rândurilor `benef`. Pentru dividende, doar
    // Tbaza/Timp au valori; restul totalurilor de tip de venit (câștig/pierdere/venit-brut/garanție)
    // sunt 0, dar atributele sunt OBLIGATORII (schema v8).
    empty_elem_attrs(
        &mut w,
        "sect_II",
        &[
            ("tip_venit", "08"),
            ("nrben", &nrben_s),
            ("Tcastig", "0"),
            ("Tpierd", "0"),
            ("T_VB", "0"),
            ("T_GAR", "0"),
            ("Tbaza", &t_baza_s),
            ("Timp", &t_imp_s),
        ],
    )?;

    // Rândurile `benef` sunt copii DIRECȚI ai `declaratie205` (SIBLINGS ai recapitulației sect_II) —
    // fiecare se auto-identifică prin `tip_venit1`. Vin DUPĂ recapitulația secțiunii. `id_inreg` e
    // numărul de înregistrare secvențial (1-based, cheia unică a rândului — OBLIGATORIU).
    for (i, b) in beneficiaries.iter().enumerate() {
        let id_inreg = (i + 1).to_string();
        let baza = round_lei(b.baza).to_string();
        let imp = round_lei(b.impozit).to_string();
        let divd = round_lei(b.distribuit).to_string();
        let divp = round_lei(b.platit).to_string();
        let name = trunc(b.name.trim(), NAME_MAX);
        let rezid = if b.resident { "1" } else { "2" };
        empty_elem_attrs(
            &mut w,
            "benef",
            &[
                ("id_inreg", &id_inreg),
                ("tip_venit1", "08"),
                ("tip_plata", "2"), // 2 = plată finală/definitivă
                ("Rezid", rezid),
                ("cifR", b.cnp.trim()),
                ("den1", &name),
                ("baza1", &baza),
                ("imp1", &imp),
                ("divid_D", &divd),
                ("divid_P", &divp),
            ],
        )?;
    }

    end_elem(&mut w, sv.root_element)?;
    finish(w)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn header() -> D205Header {
        D205Header {
            cui: "13548146".into(),
            adresa: "Str. Exemplu 1, București".into(),
            den: "Andrei Consulting SRL".into(),
            an: 2025,
            d_rec: 0,
            nume_declar: "Popescu".into(),
            prenume_declar: "Maria".into(),
            functie_declar: "Administrator".into(),
        }
    }

    fn benef(cnp: &str, baza: &str, imp: &str) -> D205Beneficiary {
        D205Beneficiary {
            cnp: cnp.into(),
            name: "Ion Gheorghe".into(),
            baza: d(baza),
            impozit: d(imp),
            distribuit: d(baza),
            platit: d(baza),
            resident: true,
        }
    }

    #[test]
    fn empty_beneficiaries_errors() {
        assert!(build_d205_xml(&header(), &[]).is_err());
    }

    #[test]
    fn builds_one_beneficiary_with_correct_attrs() {
        let xml = build_d205_xml(&header(), &[benef("1900101410011", "10000", "1000")]).unwrap();
        // Root + namespace + annual marker.
        assert!(
            xml.contains(r#"xmlns="mfp:anaf:dgti:d205:declaratie:v3""#),
            "{xml}"
        );
        assert!(
            xml.contains(r#"<declaratie205 "#) && xml.contains(r#"luna="12""#),
            "{xml}"
        );
        assert!(
            xml.contains(r#"an="2025""#) && xml.contains(r#"d_rec="0""#),
            "{xml}"
        );
        // Declarant identified by `cui` + `adresa` (NOT `cif`); no `version` attribute on the root
        // element (the `version="1.0"` in the XML prolog is the XML declaration, not a D205 attr).
        assert!(
            xml.contains(r#"cui="13548146""#) && xml.contains(r#"adresa="#),
            "{xml}"
        );
        let root_start = xml.find("<declaratie205").unwrap();
        let root_open = &xml[root_start..root_start + xml[root_start..].find('>').unwrap()];
        assert!(
            !root_open.contains("cif=") && !root_open.contains("version="),
            "{root_open}"
        );
        // Section recap: 1 beneficiary, base 10000, tax 1000; the other type-totals are 0 (required).
        assert!(
            xml.contains(r#"nrben="1""#)
                && xml.contains(r#"Tbaza="10000""#)
                && xml.contains(r#"Timp="1000""#),
            "{xml}"
        );
        assert!(
            xml.contains(r#"Tcastig="0""#)
                && xml.contains(r#"Tpierd="0""#)
                && xml.contains(r#"T_VB="0""#)
                && xml.contains(r#"T_GAR="0""#),
            "{xml}"
        );
        // Beneficiary row — dividend codes + whole-lei money.
        assert!(
            xml.contains(r#"tip_venit1="08""#) && xml.contains(r#"tip_plata="2""#),
            "{xml}"
        );
        assert!(
            xml.contains(r#"Rezid="1""#) && xml.contains(r#"cifR="1900101410011""#),
            "{xml}"
        );
        assert!(
            xml.contains(r#"baza1="10000""#) && xml.contains(r#"imp1="1000""#),
            "{xml}"
        );
        assert!(
            xml.contains(r#"divid_D="10000""#) && xml.contains(r#"divid_P="10000""#),
            "{xml}"
        );
        // totalPlata_A = nrben(1) + Tbaza(10000) + Timp(1000) = 11001.
        assert!(xml.contains(r#"totalPlata_A="11001""#), "{xml}");
        // sect_II recap is SELF-CLOSING and precedes the benef sibling rows; root closes.
        let sect_pos = xml.find("<sect_II ").unwrap();
        let benef_pos = xml.find("<benef ").unwrap();
        assert!(
            sect_pos < benef_pos,
            "sect_II recap must precede benef rows: {xml}"
        );
        assert!(
            !xml.contains("</sect_II>"),
            "sect_II must be self-closing: {xml}"
        );
        assert!(
            xml.contains("/></declaratie205>")
                || xml.contains("/>\n</declaratie205>")
                || xml.contains("</declaratie205>"),
            "{xml}"
        );
    }

    #[test]
    fn aggregates_section_totals_over_beneficiaries() {
        let xml = build_d205_xml(
            &header(),
            &[
                benef("1900101410011", "10000", "1600"),
                benef("1960101410019", "5000", "800"),
            ],
        )
        .unwrap();
        // nrben=2, Tbaza=15000, Timp=2400, totalPlata_A=2+15000+2400=17402.
        assert!(
            xml.contains(r#"nrben="2""#) && xml.contains(r#"Tbaza="15000""#),
            "{xml}"
        );
        assert!(xml.contains(r#"Timp="2400""#), "{xml}");
        assert!(xml.contains(r#"totalPlata_A="17402""#), "{xml}");
        assert_eq!(xml.matches("<benef ").count(), 2);
    }

    /// Dev helper (opt-in): dump a golden D205 to a temp file for the real `-v D205` validator run.
    ///   cargo test --lib anaf_decl::d205_xml::tests::dump_d205 -- --ignored --nocapture
    #[test]
    #[ignore]
    fn dump_d205() {
        let xml = build_d205_xml(
            &header(),
            &[
                benef("1900101410011", "10000", "1600"),
                benef("1960101410019", "5000", "800"),
            ],
        )
        .unwrap();
        let path = std::env::temp_dir().join("d205_golden.xml");
        std::fs::write(&path, &xml).unwrap();
        eprintln!("WROTE {}", path.display());
    }

    /// END-TO-END DUK gate (opt-in, like the D112 one): build a golden D205 and run the REAL bundled
    /// ANAF validator `D205Validator.jar` (`-v D205`) on it via `run_duk`, asserting it PASSES. This is
    /// the verify-first proof that the emitter is schema-conformant. `#[ignore]` because it spawns the
    /// 349 KB Java validator + jlink JRE; runs on demand:
    ///   cargo test --lib anaf_decl::d205_xml::tests::duk_validates_d205 -- --ignored --nocapture
    /// Graceful: if the bundled resources are absent (stripped checkout), it skips, never panics.
    #[test]
    #[ignore]
    fn duk_validates_d205() {
        use crate::anaf_decl::duk::{run_duk, DukProvider, DukRuntime};
        use std::path::PathBuf;

        let xml = build_d205_xml(
            &header(),
            &[
                benef("1900101410011", "10000", "1600"),
                benef("1960101410019", "5000", "800"),
            ],
        )
        .unwrap();
        let tmp = std::env::temp_dir().join("d205_duk_gate_test.xml");
        std::fs::write(&tmp, &xml).unwrap();

        let res = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
        let java = res.join(if cfg!(windows) {
            "jre-min/bin/java.exe"
        } else {
            "jre-min/bin/java"
        });
        let jar_dir = res.join("duk");

        struct LocalBundle {
            java: PathBuf,
            jar_dir: PathBuf,
        }
        impl DukProvider for LocalBundle {
            fn resolve(&self) -> Option<DukRuntime> {
                // Require D205Validator.jar specifically — skip gracefully if it isn't bundled.
                if self.java.is_file()
                    && self.jar_dir.join("DUKIntegrator.jar").is_file()
                    && self.jar_dir.join("lib/D205Validator.jar").is_file()
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
        let provider = LocalBundle { java, jar_dir };

        match run_duk(&provider, DeclKind::D205, &tmp).unwrap() {
            Some(outcome) => assert!(
                outcome.passed,
                "D205Validator reported errors on a standard D205: {:?}",
                outcome.errors
            ),
            None => eprintln!("SKIP: bundled D205 DUK runtime not present — nothing validated"),
        }
        let _ = std::fs::remove_file(&tmp);
    }
}
