//! D112 — Declarația 112 XML.
//!
//! Namespace `mfp:anaf:dgti:declaratie_unica:declaratie:v6` + root `declaratieUnica`, VERIFICAT
//! contra XSD-ului D112 OFICIAL `d112_10102024.xsd` (version 1.02, targetNamespace …:v6). ATENȚIE:
//! NU este `…:v1` — acela e din kit-ul OBSOLET «Declarația Unică 2011» (DecUnica.xsd); fișierul D112
//! curent declară `:v6` și cere `angajatorB/@B_sal` (use="required").
//!
//! Generează structura `declaratieUnica` → `angajator` (rândurile de obligații A + sumarul B +
//! C6) + câte un `asigurat`/`asiguratA` per angajat, pentru cazul STANDARD (salariat cu normă
//! întreagă, nepensionar). Numele atributelor + codurile sunt verbatim din XSD: angajatorA =
//! A_codOblig/A_codBugetar/A_datorat/A_deductibil/A_plata; angajatorB = numerele de asigurați
//! (B_cnp/B_sanatate/B_pensie) + B_sal (nr. asigurați cu venituri salariale) + B_brutSalarii;
//! asiguratA = A_1 tip asigurat, A_3 tip contract, bazele + sumele CAS/CASS.
//!
//! Este un DRAFT pentru import în aplicația D112 (PDF inteligent ANAF), unde se rulează validatorul
//! (DUKIntegrator) și se completează blocurile speciale (concedii, scutiri, sedii secundare). Restul
//! atributelor opționale (C/D/E) le completează formularul; emitem subsetul standard obligatoriu.
//!
//! MODEL IULIE 2026: Ordinul comun 605/95/928/2.314/2026 (MO 463/02.06.2026) introduce un nou model
//! D112 pentru veniturile din 07/2026 (prima depunere 25.08.2026). Cercetarea surselor oficiale arată
//! că schimbările sunt la nivel de NOMENCLATOR/instrucțiuni (suma netaxabilă 300→200, relabel tip
//! asigurat 1.11.2/1.11.3, simplificare concedii medicale) — NU câmpuri XML noi; namespace-ul rămâne
//! `:v6`. La 2026-06-13 ANAF NU publicase încă structura/XSD/DUKIntegrator pentru noul model, deci NU
//! se poate emite/valida un model distinct. Calculul H2 (4.325 / bază 200) e deja gestionat în
//! [`crate::anaf_decl::d112::part_time_min_base`]. RE-VALIDAȚI contra artefactelor oficiale (pagina
//! 112.html) când apar, înainte de prima depunere 25.08.2026.

/// Antetul + datele angajatorului.
pub struct D112Header {
    pub luna: u32,
    pub an: i32,
    pub nume_declar: String,
    pub prenume_declar: String,
    pub functie_declar: String,
    pub cif: String,
    /// CAEN (4 cifre).
    pub caen: String,
    pub den: String,
    /// Casa de sănătate (cod județ, ex. "CJ"; "_B" pentru București) — casaAng.
    pub casa: String,
}

/// Un angajat + contribuțiile lunii (sume în lei întregi).
pub struct D112Employee {
    pub cnp: String,
    pub nume: String,
    pub prenume: String,
    /// Data angajării (zz.ll.aaaa) sau gol.
    pub data_ang: String,
    pub gross: i64,
    pub cas: i64,
    pub cass: i64,
    pub impozit: i64,
    pub cam: i64,
    /// Zile lucrate în lună.
    pub zile: u32,
    /// asiguratA: A_1 tip asigurat, A_2 pensionar (0/1), A_3 tip contract, A_4 ore normă.
    pub tip_asigurat: String,
    pub pensionar: bool,
    pub tip_contract: String,
    pub ore_norma: u32,
    /// Baza CAS (A_13) / baza CASS (A_11) — egale cu brutul, sau ajustate la baza minimă part-time.
    pub baza_cas: i64,
    pub baza_cass: i64,
    /// CIF-ul sediului secundar la care e repartizat salariatul (D112 angajatorF2); '' = principal.
    pub sediu_cif: String,
}

const NS: &str = "mfp:anaf:dgti:declaratie_unica:declaratie:v6";

/// D112 employer obligations — each is `(A_codOblig, A_codBugetar)` taken verbatim from Nomenclator 3
/// of the in-force *structura D112_A7.2.6 (v7), luna 01/2026* (structura_D112_0126_030226.pdf).
///
/// IMPORTANT: do NOT validate these against the `DecUnica*.xsd` files under `static.anaf.ro/.../
/// declunica/` — those are the OBSOLETE 2011 "Declarația Unică" kit (a different declaration; its
/// `CodObligSType` stops at 449 and lacks 480). The authoritative obligation list for D112 is
/// Nomenclator 3 in the structura, validated by the live DUKIntegrator. The `XX`/`X` placeholders in
/// the budget codes are resolved automatically by the PDF-inteligent on selection.
pub mod oblig {
    /// poz. 01 — Impozit pe veniturile din salarii și asimilate salariilor (cod bugetar 5503XXXXXX).
    pub const IMPOZIT: (&str, &str) = ("602", "5503XXXXXX");
    /// poz. 02 — CAS (pensii) datorată de asigurat (cod bugetar 5503XXXXXX).
    pub const CAS: (&str, &str) = ("412", "5503XXXXXX");
    /// poz. 07 — CASS (sănătate) datorată de asigurat (cod bugetar 5503XXXXXX).
    pub const CASS: (&str, &str) = ("432", "5503XXXXXX");
    /// poz. 46 — CAM (contribuția asigurătorie pentru muncă), angajator (cod bugetar 20470300XX).
    pub const CAM: (&str, &str) = ("480", "20470300XX");
}

use crate::anaf_decl::xml_esc as esc;

/// Construiește XML-ul D112 pentru o lună. `employees` sunt deja salariații activi cu contribuțiile
/// calculate (ratele 2026). Antetul A_codBugetar folosește codul bugetar standard al CAS/CASS/CAM.
pub fn generate_d112_xml(h: &D112Header, employees: &[D112Employee]) -> String {
    let count = employees.len() as i64;
    let tot = |f: fn(&D112Employee) -> i64| employees.iter().map(f).sum::<i64>();
    let (t_gross, t_cas, t_cass, t_cam, t_impozit) = (
        tot(|e| e.gross),
        tot(|e| e.cas),
        tot(|e| e.cass),
        tot(|e| e.cam),
        tot(|e| e.impozit),
    );

    // angajatorA — câte un rând per obligație, cu (A_codOblig, A_codBugetar) din Nomenclator 3 al
    // structurii D112_A7.2.6 v7 (01/2026). Vezi modulul `oblig` pentru sursă + de ce NU se validează
    // contra DecUnica.xsd (formularul obsolet 2011 «Declarația Unică»).
    let oblig = |(cod, buget): (&str, &str), suma: i64| {
        format!(
            "    <angajatorA A_codOblig=\"{cod}\" A_codBugetar=\"{buget}\" \
A_datorat=\"{suma}\" A_deductibil=\"0\" A_plata=\"{suma}\"/>\n"
        )
    };
    let mut ang = String::new();
    ang.push_str(&oblig(oblig::IMPOZIT, t_impozit)); // 602 impozit pe veniturile din salarii
    ang.push_str(&oblig(oblig::CAS, t_cas)); // 412 CAS (pensii)
    ang.push_str(&oblig(oblig::CASS, t_cass)); // 432 CASS (sănătate)
    ang.push_str(&oblig(oblig::CAM, t_cam)); // 480 CAM (asigurătorie pentru muncă)
    let total_plata = t_impozit + t_cas + t_cass + t_cam; // totalPlata_A = Σ obligații angajator.
                                                          // angajatorB — numere asigurați + fond de salarii.
                                                          // B_sal (use="required" în d112_10102024.xsd) = nr. asigurați cu venituri de natură salarială;
                                                          // în cazul standard (toți salariați) = numărul de asigurați, ca B_cnp/B_sanatate/B_pensie.
    ang.push_str(&format!(
        "    <angajatorB B_cnp=\"{count}\" B_sanatate=\"{count}\" B_pensie=\"{count}\" \
B_sal=\"{count}\" B_brutSalarii=\"{t_gross}\"/>\n"
    ));
    // angajatorC6 — bază + contribuție (sumar) — completat în aplicație; emis 0 pentru validitate.
    ang.push_str("    <angajatorC6 C6_baza=\"0\" C6_ct=\"0\"/>\n");

    // Sedii secundare (angajatorF1 sediu principal + angajatorF2 per sediu): impozitul pe salarii se
    // repartizează după CIF-ul sediului fiecărui salariat. Se emite DOAR dacă există sedii secundare.
    // F*_deplata = F*_suma − F*_suma_ded − F*_suma_scut (deduceri/scutiri = 0 în acest caz de bază).
    let mut by_sediu: std::collections::BTreeMap<&str, i64> = std::collections::BTreeMap::new();
    for e in employees {
        *by_sediu.entry(e.sediu_cif.trim()).or_default() += e.impozit;
    }
    let has_sedii = by_sediu.keys().any(|c| !c.is_empty());
    if has_sedii {
        let head = by_sediu.get("").copied().unwrap_or(0);
        ang.push_str(&format!(
            "    <angajatorF1 F1_suma=\"{head}\" F1_suma_ded=\"0\" F1_suma_scut=\"0\" \
F1_deplata=\"{head}\"/>\n"
        ));
        let mut idx = 0;
        for (cif, suma) in by_sediu.iter().filter(|(c, _)| !c.is_empty()) {
            idx += 1;
            ang.push_str(&format!(
                "    <angajatorF2 F2_cif=\"{cif}\" F2_id=\"{idx}\" F2_suma=\"{suma}\" \
F2_suma_ded=\"0\" F2_suma_scut=\"0\" F2_deplata=\"{suma}\"/>\n",
                cif = esc(cif)
            ));
        }
    }

    // asigurat* — câte unul per salariat, cu blocul de contribuții asiguratA (caz standard).
    let mut asig = String::new();
    for (i, e) in employees.iter().enumerate() {
        let id = i + 1;
        let data = if e.data_ang.is_empty() {
            String::new()
        } else {
            format!(" dataAng=\"{}\"", esc(&e.data_ang))
        };
        asig.push_str(&format!(
            "  <asigurat idAsig=\"{id}\" cnpAsig=\"{cnp}\" numeAsig=\"{nume}\" \
prenAsig=\"{pren}\"{data} asigCI=\"1\" asigSO=\"1\">\n\
    <asiguratA A_1=\"{a1}\" A_2=\"{a2}\" A_3=\"{a3}\" A_4=\"{a4}\" A_5=\"{gross}\" A_8=\"{zile}\" \
A_11=\"{baza_cass}\" A_12=\"{cass}\" A_13=\"{baza_cas}\" A_14=\"{cas}\" A_20=\"{impozit}\"/>\n\
  </asigurat>\n",
            cnp = esc(&e.cnp),
            nume = esc(&e.nume),
            pren = esc(&e.prenume),
            a1 = esc(&e.tip_asigurat),
            a2 = if e.pensionar { 1 } else { 0 },
            a3 = esc(&e.tip_contract),
            a4 = e.ore_norma,
            gross = e.gross,
            zile = e.zile,
            baza_cass = e.baza_cass,
            cass = e.cass,
            baza_cas = e.baza_cas,
            cas = e.cas,
            impozit = e.impozit,
        ));
    }

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<declaratieUnica xmlns=\"{NS}\" luna_r=\"{luna}\" an_r=\"{an}\" \
nume_declar=\"{nd}\" prenume_declar=\"{pd}\" functie_declar=\"{fd}\">\n\
  <angajator cif=\"{cif}\" caen=\"{caen}\" den=\"{den}\" casaAng=\"{casa}\" \
totalPlata_A=\"{tp}\">\n\
{ang}\
  </angajator>\n\
{asig}\
</declaratieUnica>\n",
        tp = total_plata,
        luna = h.luna,
        an = h.an,
        nd = esc(&h.nume_declar),
        pd = esc(&h.prenume_declar),
        fd = esc(&h.functie_declar),
        cif = esc(&h.cif),
        caen = esc(&h.caen),
        den = esc(&h.den),
        casa = esc(&h.casa),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emp(cnp: &str, nume: &str) -> D112Employee {
        D112Employee {
            cnp: cnp.into(),
            nume: nume.into(),
            prenume: "Ion".into(),
            data_ang: "01.01.2024".into(),
            gross: 5000,
            cas: 1250,
            cass: 500,
            impozit: 325,
            cam: 113,
            zile: 21,
            tip_asigurat: "1".into(),
            pensionar: false,
            tip_contract: "N".into(),
            ore_norma: 8,
            baza_cas: 5000,
            baza_cass: 5000,
            sediu_cif: "".into(),
        }
    }

    #[test]
    fn d112_has_structure_and_totals() {
        let h = D112Header {
            luna: 6,
            an: 2026,
            nume_declar: "Popescu".into(),
            prenume_declar: "Maria".into(),
            functie_declar: "Administrator".into(),
            cif: "12345678".into(),
            caen: "6201".into(),
            den: "Test SRL".into(),
            casa: "CJ".into(),
        };
        let xml = generate_d112_xml(&h, &[emp("1", "A"), emp("2", "B")]);
        // Root + namespace + header. Namespace is :v6 (official d112_10102024.xsd), NOT the obsolete
        // DecUnica :v1.
        assert!(
            xml.contains("<declaratieUnica xmlns=\"mfp:anaf:dgti:declaratie_unica:declaratie:v6\"")
        );
        assert!(xml.contains("luna_r=\"6\" an_r=\"2026\""));
        assert!(xml.contains("nume_declar=\"Popescu\""));
        // Employer obligation rows: 602 impozit (2×325=650), 412 CAS (2.500), 432 CASS (1.000),
        // 480 CAM (226) — totals over 2 employees. Budget codes per structura: 5503XXXXXX for
        // impozit/CAS/CASS, 20470300XX for CAM.
        assert!(xml.contains("A_codOblig=\"602\" A_codBugetar=\"5503XXXXXX\" A_datorat=\"650\""));
        assert!(xml.contains("A_codOblig=\"412\" A_codBugetar=\"5503XXXXXX\" A_datorat=\"2500\""));
        assert!(xml.contains("A_codOblig=\"432\" A_codBugetar=\"5503XXXXXX\" A_datorat=\"1000\""));
        assert!(xml.contains("A_codOblig=\"480\" A_codBugetar=\"20470300XX\" A_datorat=\"226\""));
        // totalPlata_A = 650 + 2500 + 1000 + 226 = 4376.
        assert!(xml.contains("totalPlata_A=\"4376\""));
        // angajatorB includes the required B_sal (= count for the all-salaried standard case).
        assert!(xml.contains(
            "B_cnp=\"2\" B_sanatate=\"2\" B_pensie=\"2\" B_sal=\"2\" B_brutSalarii=\"10000\""
        ));
        // Two insured persons with asiguratA contributions.
        assert_eq!(xml.matches("<asigurat ").count(), 2);
        assert!(xml.contains("A_1=\"1\" A_2=\"0\" A_3=\"N\" A_4=\"8\""));
        assert!(xml.contains("A_13=\"5000\" A_14=\"1250\"")); // baza CAS + CAS
    }

    #[test]
    fn sedii_secundare_split_impozit_into_f1_f2() {
        // Două sedii: angajatul A la sediu secundar CIF 99, B la sediu principal. Impozit 325 fiecare.
        let mut a = emp("1", "A");
        a.sediu_cif = "99".into();
        let b = emp("2", "B"); // sediu principal ('')
        let h = D112Header {
            luna: 6,
            an: 2026,
            nume_declar: "X".into(),
            prenume_declar: "-".into(),
            functie_declar: "Adm".into(),
            cif: "12345678".into(),
            caen: "6201".into(),
            den: "T".into(),
            casa: "CJ".into(),
        };
        let xml = generate_d112_xml(&h, &[a, b]);
        // F1 (sediu principal) = 325 (angajatul B); F2 pentru CIF 99 = 325 (angajatul A).
        assert!(xml.contains("<angajatorF1 F1_suma=\"325\" F1_suma_ded=\"0\" F1_suma_scut=\"0\" F1_deplata=\"325\"/>"));
        assert!(xml.contains("<angajatorF2 F2_cif=\"99\" F2_id=\"1\" F2_suma=\"325\" F2_suma_ded=\"0\" F2_suma_scut=\"0\" F2_deplata=\"325\"/>"));
    }

    #[test]
    fn no_sedii_omits_f1_f2() {
        let h = D112Header {
            luna: 6,
            an: 2026,
            nume_declar: "X".into(),
            prenume_declar: "-".into(),
            functie_declar: "Adm".into(),
            cif: "12345678".into(),
            caen: "6201".into(),
            den: "T".into(),
            casa: "CJ".into(),
        };
        let xml = generate_d112_xml(&h, &[emp("1", "A")]); // toți la sediu principal
        assert!(!xml.contains("angajatorF1"));
        assert!(!xml.contains("angajatorF2"));
    }

    #[test]
    fn obligation_codes_match_structura_2026() {
        // Lock (codOblig, codBugetar) against drift (audit Note 1): structura D112 v7 (01/2026) maps
        // impozit→(602,5503XXXXXX), CAS→(412,5503XXXXXX), CASS→(432,5503XXXXXX), CAM→(480,20470300XX).
        // The "stops-at-449" DecUnica.xsd is the obsolete 2011 Declarația Unică form, not D112.
        assert_eq!(oblig::IMPOZIT, ("602", "5503XXXXXX"));
        assert_eq!(oblig::CAS, ("412", "5503XXXXXX"));
        assert_eq!(oblig::CASS, ("432", "5503XXXXXX"));
        assert_eq!(oblig::CAM, ("480", "20470300XX"));
    }
}
