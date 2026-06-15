//! D205 — Declarația informativă privind impozitul reținut la sursă, pe beneficiari de venit
//! (OPANAF 179/2022, mod. OPANAF 102/2025). Capitolul DIVIDENDE (`tip_venit` 08). Model `:v3`.
//!
//! Structură pe 3 niveluri, cu TOATE datele ca ATRIBUTE XML și sume în LEI ÎNTREGI (N15):
//! ```text
//!   <declaratie205 …antet… totalPlata_A="…">   — 1 / declarație (luna=12, an de venit)
//!     <sect_II tip_venit="08" nrben Tbaza Timp>  — 1 / tip de venit (dividende)
//!       <benef tip_venit1="08" …/>               — 1 / beneficiar (CNP)
//!   </declaratie205>
//! ```
//! Spre deosebire de emitoarele flat cu elemente-copil (D300/bilanț), D205 folosește emitorul cu
//! ATRIBUTE ([`crate::anaf_decl::xml::start_elem_attrs`] / [`empty_elem_attrs`]). Validatorul oficial
//! este `D205Validator.jar` (`-v D205`) — vezi `src-tauri/D205_EMITTER_DESIGN.md`. Câmpurile/structura
//! reflectă spec-ul verificat la nivel de octet; rularea END-TO-END a DUK pe un XML golden confirmă
//! conformitatea înainte ca gate-ul să devină blocant (regula „verify-first", ca la D112).

use rust_decimal::Decimal;

use crate::anaf_decl::version::resolve;
use crate::anaf_decl::xml::{
    empty_elem_attrs, end_elem, finish, new_writer, start_elem_attrs, trunc,
};
use crate::anaf_decl::{round_lei, DeclKind};
use crate::error::{AppError, AppResult};

/// Antetul D205 (datele plătitorului/declarantului) pentru un an de venit.
pub struct D205Header {
    /// `cif` — codul fiscal al declarantului (CUI, doar cifre, fără „RO").
    pub cif: String,
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
    let nume = trunc(header.nume_declar.trim(), NAME_MAX);
    let prenume = trunc(header.prenume_declar.trim(), NAME_MAX);
    let functie = trunc(header.functie_declar.trim(), NAME_MAX);

    let mut w = new_writer()?;
    start_elem_attrs(
        &mut w,
        sv.root_element, // "declaratie205"
        &[
            ("xmlns", sv.namespace),
            ("version", "1.00"),
            ("luna", "12"), // D205 e anuală: luna de raportare = 12
            ("an", &an),
            ("d_rec", &d_rec),
            ("cif", header.cif.trim()),
            ("den", &den),
            ("nume_declar", &nume),
            ("prenume_declar", &prenume),
            ("functie_declar", &functie),
            ("totalPlata_A", &total_s),
        ],
    )?;

    start_elem_attrs(
        &mut w,
        "sect_II",
        &[
            ("tip_venit", "08"),
            ("nrben", &nrben_s),
            ("Tbaza", &t_baza_s),
            ("Timp", &t_imp_s),
        ],
    )?;

    for b in beneficiaries {
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

    end_elem(&mut w, "sect_II")?;
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
            cif: "13548146".into(),
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
        // Section recap: 1 beneficiary, base 10000, tax 1000.
        assert!(
            xml.contains(r#"<sect_II tip_venit="08" nrben="1" Tbaza="10000" Timp="1000">"#),
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
        // Self-closing benef + closed section + root.
        assert!(
            xml.contains("/>") && xml.contains("</sect_II>") && xml.contains("</declaratie205>"),
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
}
