//! D112 — Declarația 112 XML (model `:v7`).
//!
//! Namespace `mfp:anaf:dgti:declaratie_unica:declaratie:v7` + root `declaratieUnica`. CONFIRMAT
//! rulând validatorul OFICIAL ANAF `D112Validator.jar` (`-v D112`, build 209/Apr-2026): modelul curent
//! cere `:v7` (vechiul `d112_10102024.xsd` declara `:v6` — respins de validator). Deltele `:v7` față de
//! `:v6` sunt CONFIRMATE de validator + structura: root `d_rec`; `angajator/@datCAM`; `angajatorA/@A_scutit`
//! (A_plata = A_datorat − A_deductibil − A_scutit); `angajatorC6` ELIMINAT (nu există în :v7);
//! `asigurat/@casaSn`; `asiguratA/@A_sal1`; `asiguratA/@A_5` = baza CAM (nu brutul); `A_20` ELIMINAT
//! (impozitul nu e per-asigurat, doar în agregatul angajatorA(602)).
//!
//! ⚠️ WIP — caz STANDARD încă NU trece curat de `-v D112`: validatorul cere SECȚIUNILE DE SUMAR
//! `angajatorC1..C5` (rollup-uri contribuții) ÎNAINTE de blocurile `asigurat`. Atributele de mai sus
//! sunt corecte; structura `angajator` (secțiunile C-rollup obligatorii) e pasul rămas — se completează
//! iterativ contra `-v D112` (vezi `~/.claude/plans/robust-crunching-ocean.md`). NU este încă depunabil.
//!
//! `asigurat` e SIBLING al `angajator` (ambele copii ai `declaratieUnica`) — confirmat de structură
//! (`</angajator>` apoi `<asigurat>`). asiguratA = A_1 tip asigurat, A_3 tip contract, bazele CAS/CASS.
//!
//! Pentru veniturile din 07/2026 (Ordin 605/95/928/2.314/2026): rate/nomenclator (300→200, tip-asigurat
//! 1.11.2/1.11.3) deja gestionate în [`crate::anaf_decl::d112`]; namespace-ul `:v7` acoperă și H2.

/// Antetul + datele angajatorului.
pub struct D112Header {
    pub luna: u32,
    pub an: i32,
    /// d_rec (poz 3, DA): 0 = declarație inițială, 1 = rectificativă.
    pub d_rec: u8,
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
    /// A_5 — baza CAM (= baza CAS/CASS pentru salariatul normal; 0 dacă nu se datorează CAM).
    pub baza_cam: i64,
    /// A_sal1 — salariul de bază brut din contract (poz 14a, :v7).
    pub sal_contract: i64,
    /// CIF-ul sediului secundar la care e repartizat salariatul (D112 angajatorF2); '' = principal.
    pub sediu_cif: String,
}

// Namespace CONFIRMED against the LIVE ANAF D112Validator (`-v D112`): the current model requires
// `:v7`. The older `d112_10102024.xsd` declared `:v6`, but ANAF bumped the schema — emitting `:v6`
// is rejected ("Valoarea corecta este …:v7"). The :v7 deltas (datCAM, A_scutit, casaSn, A_sal1; A_20
// and angajatorC6 removed) are likewise validator-confirmed against the structura.
const NS: &str = "mfp:anaf:dgti:declaratie_unica:declaratie:v7";

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
    // :v7: A_scutit (poz 22a, DA) e obligatoriu pe fiecare rând. Ordine: A_codOblig, A_codBugetar,
    // A_datorat, A_deductibil, A_scutit, A_plata; A_plata = A_datorat − A_deductibil − A_scutit
    // (numeric neschimbat în cazul standard: A_deductibil = A_scutit = 0).
    let oblig = |(cod, buget): (&str, &str), suma: i64| {
        format!(
            "    <angajatorA A_codOblig=\"{cod}\" A_codBugetar=\"{buget}\" \
A_datorat=\"{suma}\" A_deductibil=\"0\" A_scutit=\"0\" A_plata=\"{suma}\"/>\n"
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
    // NOTE: `angajatorC6` (emis sub :v6) NU există în :v7 — validatorul îl respinge. Eliminat.
    // angajatorC1-C5/D/E/F/G sunt sumare opționale (0-1) — în cazul standard (condiții normale,
    // fără concedii) nu se emit. CHEIA :v7: `<asigurat>` e COPIL al `<angajator>` (NU sibling) —
    // confirmat de validator ("sectiune obligatorie lipsa inainte de sfarsitul angajator" dispare
    // când asigurat e mutat înăuntru).

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
        // :v7: + casaSn (poz 9, DA) = casa de sănătate a angajatorului; A_5 = baza CAM (NU brutul);
        // A_sal1 (poz 14a) = salariul de bază din contract; A_20 NU există (impozitul e doar în
        // agregatul angajatorA(602)) — eliminat.
        asig.push_str(&format!(
            "  <asigurat idAsig=\"{id}\" cnpAsig=\"{cnp}\" numeAsig=\"{nume}\" \
prenAsig=\"{pren}\"{data} casaSn=\"{casa}\" asigCI=\"1\" asigSO=\"1\">\n\
    <asiguratA A_1=\"{a1}\" A_sal1=\"{sal1}\" A_2=\"{a2}\" A_3=\"{a3}\" A_4=\"{a4}\" \
A_5=\"{cam_base}\" A_8=\"{zile}\" A_11=\"{baza_cass}\" A_12=\"{cass}\" A_13=\"{baza_cas}\" \
A_14=\"{cas}\"/>\n\
  </asigurat>\n",
            cnp = esc(&e.cnp),
            nume = esc(&e.nume),
            pren = esc(&e.prenume),
            casa = esc(&h.casa),
            a1 = esc(&e.tip_asigurat),
            sal1 = e.sal_contract,
            a2 = if e.pensionar { 1 } else { 0 },
            a3 = esc(&e.tip_contract),
            a4 = e.ore_norma,
            cam_base = e.baza_cam,
            zile = e.zile,
            baza_cass = e.baza_cass,
            cass = e.cass,
            baza_cas = e.baza_cas,
            cas = e.cas,
        ));
    }

    // :v7: datCAM (poz 18a, DA) la nivel de angajator = 1 dacă se datorează CAM (orice CAM > 0).
    let datcam = if t_cam > 0 { 1 } else { 0 };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<declaratieUnica xmlns=\"{NS}\" luna_r=\"{luna}\" an_r=\"{an}\" d_rec=\"{drec}\" \
nume_declar=\"{nd}\" prenume_declar=\"{pd}\" functie_declar=\"{fd}\">\n\
  <angajator cif=\"{cif}\" caen=\"{caen}\" den=\"{den}\" casaAng=\"{casa}\" \
datCAM=\"{datcam}\" totalPlata_A=\"{tp}\">\n\
{ang}\
  </angajator>\n\
{asig}\
</declaratieUnica>\n",
        tp = total_plata,
        drec = h.d_rec,
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
            baza_cam: 5000,
            sal_contract: 5000,
            sediu_cif: "".into(),
        }
    }

    #[test]
    fn d112_has_structure_and_totals() {
        let h = D112Header {
            luna: 6,
            an: 2026,
            d_rec: 0,
            nume_declar: "Popescu".into(),
            prenume_declar: "Maria".into(),
            functie_declar: "Administrator".into(),
            cif: "12345678".into(),
            caen: "6201".into(),
            den: "Test SRL".into(),
            casa: "CJ".into(),
        };
        let xml = generate_d112_xml(&h, &[emp("1", "A"), emp("2", "B")]);
        // Root + namespace + header. Namespace is :v7 (live ANAF D112Validator), NOT :v6/:v1.
        assert!(
            xml.contains("<declaratieUnica xmlns=\"mfp:anaf:dgti:declaratie_unica:declaratie:v7\"")
        );
        assert!(xml.contains("luna_r=\"6\" an_r=\"2026\" d_rec=\"0\""));
        assert!(xml.contains("nume_declar=\"Popescu\""));
        // angajator carries datCAM (:v7); angajatorC6 is gone.
        assert!(xml.contains("datCAM=\"1\""));
        assert!(!xml.contains("angajatorC6"));
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
        // Two insured persons with asiguratA contributions; :v7 attrs A_scutit / casaSn / A_sal1
        // present, A_20 gone.
        assert_eq!(xml.matches("<asigurat ").count(), 2);
        assert!(xml.contains("A_deductibil=\"0\" A_scutit=\"0\" A_plata=\"650\""));
        assert!(xml.contains("casaSn=\"CJ\""));
        assert!(xml.contains("A_1=\"1\" A_sal1=\"5000\" A_2=\"0\" A_3=\"N\" A_4=\"8\""));
        assert!(xml.contains("A_13=\"5000\" A_14=\"1250\"")); // baza CAS + CAS
        assert!(!xml.contains("A_20=")); // per-employee impozit field does not exist in :v7
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
            d_rec: 0,
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

    /// Dev helper (opt-in): write the standard D112 XML to a file for the real ANAF D112Validator.
    ///   cargo test --lib anaf_decl::d112_xml::tests::dump_standard_d112 -- --ignored --nocapture
    #[test]
    #[ignore]
    fn dump_standard_d112() {
        let h = D112Header {
            luna: 6,
            an: 2026,
            d_rec: 0,
            nume_declar: "Popescu".into(),
            prenume_declar: "Maria".into(),
            functie_declar: "Administrator".into(),
            cif: "13548146".into(), // valid mod-11 CUI for the validator
            caen: "6201".into(),
            den: "Test SRL".into(),
            casa: "CJ".into(),
        };
        let xml = generate_d112_xml(
            &h,
            &[
                emp("1960101410019", "Popescu"),
                emp("1900101410011", "Ionescu"),
            ],
        );
        let path = std::env::temp_dir().join("d112_std.xml");
        std::fs::write(&path, &xml).unwrap();
        eprintln!("WROTE {}", path.display());
    }

    #[test]
    fn no_sedii_omits_f1_f2() {
        let h = D112Header {
            luna: 6,
            an: 2026,
            d_rec: 0,
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
