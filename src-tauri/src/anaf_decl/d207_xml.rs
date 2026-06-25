//! Generator XML pentru **Declarația 207** (D207 — informativă privind impozitul reținut la sursă pe
//! beneficiari de venit **NEREZIDENȚI**, OPANAF 179/2022; capitolul DIVIDENDE). Este sibling-ul D205
//! pentru nerezidenți: rezidenții → D205, nerezidenții → D207 (`beneficiary_resident = false`).
//!
//! Schema OFICIALĂ (vendorată în `tools/anaf/d207.xsd`, version 1.02): namespace
//! `mfp:anaf:dgti:d207:declaratie:v2`. NU există un `D207Validator.jar` dedicat (DUKIntegrator nu
//! acoperă D207) — validarea e prin XSD. Testul `tests/d207_xsd.rs` rulează `xmllint` pe XML-ul generat.
//!
//! Structură (din `d207.xsd`):
//! ```text
//! <declaratie207 luna="12" an d_rec cui den adresa nume/prenume/functie_declar totalPlata_A …>
//!   <sect_II tip_venit nrben Tscutit Tbaza Timp Timps/>          — 1 / tip_venit, ÎNAINTEA rândurilor
//!   <benef id_inreg tip_venit1 den1 Stat_R cifR? cifS? baza1 imp1 imps1 Act_N/>  — 1 / beneficiar
//! </declaratie207>
//! ```
//! `totalPlata_A = Σ_sect (nrben + Tscutit + Tbaza + Timp + Timps)` (sumă de control, lei întregi).

use std::collections::BTreeMap;

use rust_decimal::Decimal;

use crate::anaf_decl::round_lei;
use crate::anaf_decl::xml::{
    empty_elem_attrs, end_elem, finish, new_writer, start_elem_attrs, trunc,
};
use crate::error::{AppError, AppResult};

const NS: &str = "mfp:anaf:dgti:d207:declaratie:v2";
const ROOT: &str = "declaratie207";

/// Coduri `tip_venit` SCUTITE (schema: `baza1` ține venitul brut scutit, iar `imp1`/`imps1` = 0; suma
/// intră în `Tscutit`, nu în `Tbaza`). Pentru dividende: 14 (art.229) / 20 (convenții). Restul → impozabile.
fn is_exempt(tip_venit: &str) -> bool {
    matches!(
        tip_venit,
        "12" | "13" | "14" | "15" | "16" | "17" | "18" | "19" | "20" | "21"
    )
}

/// Doar cifre (pattern `CifSType` = `[1-9]\d{12}|[1-9]\d{1,9}`): elimină „RO" + orice non-cifră.
fn digits_only(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_digit()).collect()
}

/// Antetul D207 (plătitorul/declarantul). `cui` = doar cifre (fără „RO").
pub struct D207Header {
    pub cui: String,
    pub den: String,
    pub adresa: String,
    /// Anul de venit raportat (`an`, ≥ 2018 per schemă).
    pub an: i32,
    /// `d_rec` — 0 = inițială, 1 = rectificativă.
    pub d_rec: u8,
    pub nume_declar: String,
    pub prenume_declar: String,
    pub functie_declar: String,
}

/// Un beneficiar **nerezident** (rând `<benef>`). Sumele sunt `Decimal`; se rotunjesc la lei la emitere.
#[derive(Debug, Clone)]
pub struct D207Benef {
    /// `tip_venit1` — cod natură venit din nomenclatorul schemei (ex. "01" dividende impozabile,
    /// "22" dividende cu convenție de evitare a dublei impuneri, "14"/"20" dividende scutite).
    pub tip_venit: String,
    /// `den1` — numele/denumirea beneficiarului nerezident.
    pub name: String,
    /// `Stat_R` — codul de țară (2 litere) al statului de rezidență, din nomenclatorul ANAF al schemei.
    pub stat_r: String,
    /// `cifR` — codul fiscal RO al nerezidentului, dacă i s-a atribuit unul (opțional, doar cifre).
    pub cif_r: Option<String>,
    /// `cifS` — codul fiscal din străinătate (NIF emis de statul de rezidență), opțional.
    pub cif_s: Option<String>,
    /// `baza1` — baza impozabilă (sau venitul brut scutit pentru codurile scutite).
    pub baza: Decimal,
    /// `imp1` — impozit reținut la sursă.
    pub impozit: Decimal,
    /// `imps1` — impozit suportat de plătitorul de venit.
    pub impozit_suportat: Decimal,
    /// `Act_N` — temei legal: 1 = Codul fiscal (L227/2015); 2 = convenție de evitare a dublei impuneri.
    pub act_n: u8,
}

/// Construiește XML-ul D207 (`:v2`) pentru anul `header.an`. Grupează beneficiarii pe `tip_venit` într-o
/// recapitulație `sect_II`, apoi emite rândurile `benef`. Eroare dacă lista e goală.
pub fn build_d207_xml(header: &D207Header, benefs: &[D207Benef]) -> AppResult<String> {
    if benefs.is_empty() {
        return Err(AppError::Validation(
            "D207: nu există beneficiari nerezidenți pentru anul selectat.".into(),
        ));
    }

    // ── Agregate per tip_venit (sect_II). Scutit → Tscutit; impozabil → Tbaza/Timp/Timps. ──
    #[derive(Default)]
    struct Agg {
        nrben: i64,
        t_scutit: i64,
        t_baza: i64,
        t_imp: i64,
        t_imps: i64,
    }
    let mut by_tip: BTreeMap<String, Agg> = BTreeMap::new();
    for b in benefs {
        let baza = round_lei(b.baza);
        let a = by_tip.entry(b.tip_venit.clone()).or_default();
        a.nrben += 1;
        if is_exempt(&b.tip_venit) {
            a.t_scutit += baza;
        } else {
            a.t_baza += baza;
            a.t_imp += round_lei(b.impozit);
            a.t_imps += round_lei(b.impozit_suportat);
        }
    }
    let total_plata_a: i64 = by_tip
        .values()
        .map(|a| a.nrben + a.t_scutit + a.t_baza + a.t_imp + a.t_imps)
        .sum();

    let an = header.an.to_string();
    let d_rec = header.d_rec.to_string();
    let total_s = total_plata_a.to_string();
    let cui = digits_only(&header.cui);
    // The declarant CUI is mandatory — `digits_only` would otherwise silently emit an empty
    // `cui=""` attribute that the ANAF DUK validator rejects.
    if cui.is_empty() {
        return Err(AppError::Validation(
            "CUI-ul declarantului este obligatoriu pentru D207.".to_string(),
        ));
    }
    let den = trunc(header.den.trim(), 200);
    let adresa = trunc(header.adresa.trim(), 1000);
    let nume = trunc(header.nume_declar.trim(), 75);
    let prenume = trunc(header.prenume_declar.trim(), 75);
    let functie = trunc(header.functie_declar.trim(), 50);

    let mut w = new_writer()?;
    start_elem_attrs(
        &mut w,
        ROOT,
        &[
            ("xmlns", NS),
            ("luna", "12"), // D207 e anuală: luna de raportare = 12 (fixat în schemă)
            ("an", &an),
            ("d_rec", &d_rec),
            ("nume_declar", &nume),
            ("prenume_declar", &prenume),
            ("functie_declar", &functie),
            ("cui", &cui),
            ("den", &den),
            ("adresa", &adresa),
            ("totalPlata_A", &total_s),
        ],
    )?;

    // sect_II (recapitulații), ÎNAINTEA rândurilor benef — una per tip_venit (max 21 per schemă).
    for (tip, a) in &by_tip {
        empty_elem_attrs(
            &mut w,
            "sect_II",
            &[
                ("tip_venit", tip),
                ("nrben", &a.nrben.to_string()),
                ("Tscutit", &a.t_scutit.to_string()),
                ("Tbaza", &a.t_baza.to_string()),
                ("Timp", &a.t_imp.to_string()),
                ("Timps", &a.t_imps.to_string()),
            ],
        )?;
    }

    // Rândurile benef. cifR/cifS sunt opționale (emise doar când au valoare).
    for (i, b) in benefs.iter().enumerate() {
        let id_inreg = (i + 1).to_string();
        let exempt = is_exempt(&b.tip_venit);
        let baza = round_lei(b.baza).to_string();
        let imp = if exempt {
            "0".to_string()
        } else {
            round_lei(b.impozit).to_string()
        };
        let imps = if exempt {
            "0".to_string()
        } else {
            round_lei(b.impozit_suportat).to_string()
        };
        let name = trunc(b.name.trim(), 100);
        let act_n = if b.act_n == 2 { "2" } else { "1" };
        let cif_r: Option<String> = b
            .cif_r
            .as_deref()
            .map(digits_only)
            .filter(|s| !s.is_empty());
        let cif_s: Option<&str> = b.cif_s.as_deref().map(str::trim).filter(|s| !s.is_empty());

        let mut attrs: Vec<(&str, &str)> = vec![
            ("id_inreg", &id_inreg),
            ("tip_venit1", &b.tip_venit),
            ("den1", &name),
            ("Stat_R", b.stat_r.trim()),
        ];
        if let Some(ref cr) = cif_r {
            attrs.push(("cifR", cr.as_str()));
        }
        if let Some(cs) = cif_s {
            attrs.push(("cifS", cs));
        }
        attrs.push(("baza1", &baza));
        attrs.push(("imp1", &imp));
        attrs.push(("imps1", &imps));
        attrs.push(("Act_N", act_n));
        empty_elem_attrs(&mut w, "benef", &attrs)?;
    }

    end_elem(&mut w, ROOT)?;
    Ok(crate::anaf_decl::xml::pretty_print(&finish(w)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn header() -> D207Header {
        D207Header {
            cui: "RO12345678".into(),
            den: "Plătitor SRL".into(),
            adresa: "Str. Exemplu 1, București".into(),
            an: 2025,
            d_rec: 0,
            nume_declar: "Popescu".into(),
            prenume_declar: "Ion".into(),
            functie_declar: "Administrator".into(),
        }
    }

    #[test]
    fn emits_root_sect_and_benef_with_correct_totals() {
        let benefs = vec![
            D207Benef {
                tip_venit: "01".into(), // dividende impozabile (art.223)
                name: "Müller GmbH".into(),
                stat_r: "DE".into(),
                cif_r: None,
                cif_s: Some("DE811234567".into()),
                baza: d("10000.00"),
                impozit: d("1000.00"), // 10% (an de venit 2025)
                impozit_suportat: d("0"),
                act_n: 1,
            },
            D207Benef {
                tip_venit: "22".into(), // dividende cu convenție
                name: "Dupont SA".into(),
                stat_r: "FR".into(),
                cif_r: None,
                cif_s: Some("FR12345678901".into()),
                baza: d("5000.00"),
                impozit: d("500.00"),
                impozit_suportat: d("0"),
                act_n: 2,
            },
        ];
        let xml = build_d207_xml(&header(), &benefs).unwrap();
        assert!(xml.contains("<declaratie207 "));
        assert!(xml.contains(&format!("xmlns=\"{NS}\"")));
        assert!(xml.contains("luna=\"12\""));
        assert!(xml.contains("an=\"2025\""));
        assert!(xml.contains("cui=\"12345678\""), "RO prefix stripped"); // digits-only
                                                                         // două recapitulații (01 + 22), fiecare cu nrben=1.
        assert!(xml.contains("tip_venit=\"01\""));
        assert!(xml.contains("tip_venit=\"22\""));
        // rânduri benef cu țara + codul fiscal străin.
        assert!(xml.contains("Stat_R=\"DE\""));
        assert!(xml.contains("cifS=\"DE811234567\""));
        assert!(xml.contains("baza1=\"10000\""));
        assert!(xml.contains("imp1=\"1000\""));
        assert!(xml.contains("Act_N=\"2\""));
        // totalPlata_A = Σ(nrben + Tbaza + Timp) = (1+10000+1000) + (1+5000+500) = 16502.
        assert!(xml.contains("totalPlata_A=\"16502\""));
        assert!(xml.contains("</declaratie207>"));
    }

    #[test]
    fn exempt_dividend_goes_to_tscutit_with_zero_tax() {
        // Cod 14 = dividende scutite (art.229) → baza1 în Tscutit; imp1/imps1 = 0.
        let benefs = vec![D207Benef {
            tip_venit: "14".into(),
            name: "EU Parent BV".into(),
            stat_r: "NL".into(),
            cif_r: None,
            cif_s: None,
            baza: d("20000.00"),
            impozit: d("0"),
            impozit_suportat: d("0"),
            act_n: 1,
        }];
        let xml = build_d207_xml(&header(), &benefs).unwrap();
        assert!(xml.contains("Tscutit=\"20000\""));
        assert!(xml.contains("Tbaza=\"0\""));
        assert!(xml.contains("imp1=\"0\""));
        // totalPlata_A = nrben(1) + Tscutit(20000) = 20001.
        assert!(xml.contains("totalPlata_A=\"20001\""));
    }

    #[test]
    fn empty_list_is_rejected() {
        assert!(build_d207_xml(&header(), &[]).is_err());
    }
}
