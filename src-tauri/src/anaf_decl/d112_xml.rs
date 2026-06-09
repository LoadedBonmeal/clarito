//! D112 — Declarația 112 XML (schema DecUnica.xsd, namespace
//! `mfp:anaf:dgti:declaratie_unica:declaratie:v1`).
//!
//! Generează structura `declaratieUnica` → `angajator` (rândurile de obligații A + sumarul B +
//! C6) + câte un `asigurat`/`asiguratA` per angajat, pentru cazul STANDARD (salariat cu normă
//! întreagă, nepensionar). Numele atributelor + codurile sunt verbatim din XSD (s-a verificat:
//! angajatorA = A_codOblig/A_codBugetar/A_datorat/A_plata; angajatorB = numere asigurați + fond
//! salarii; asiguratA = A_1 tip asigurat, A_3 tip contract, bazele + sumele CAS/CASS).
//!
//! Este un DRAFT pentru import în aplicația D112 (PDF inteligent ANAF), unde se rulează validatorul
//! (DUKIntegrator) și se completează blocurile speciale (concedii, scutiri, sedii secundare).

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
}

const NS: &str = "mfp:anaf:dgti:declaratie_unica:declaratie:v1";

fn esc(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .flat_map(|c| match c {
            '&' => "&amp;".chars().collect::<Vec<_>>(),
            '<' => "&lt;".chars().collect(),
            '>' => "&gt;".chars().collect(),
            '"' => "&quot;".chars().collect(),
            other => vec![other],
        })
        .collect()
}

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

    // angajatorA — câte un rând per obligație (A_codOblig din Nomenclator 3, verificat în structura
    // D112 0126/2026): poz.01 = 602 impozit pe salarii; poz.02 = 412 CAS; poz.07 = 432 CASS;
    // poz.46 = 480 CAM. (XSD-ul public DecUnica.xsd nu mai e actualizat cu 480; aplicația D112 îl
    // acceptă.) A_codBugetar = codul template "20470101XX".
    let oblig = |cod: &str, suma: i64| {
        format!(
            "    <angajatorA A_codOblig=\"{cod}\" A_codBugetar=\"20470101XX\" \
A_datorat=\"{suma}\" A_deductibil=\"0\" A_plata=\"{suma}\"/>\n"
        )
    };
    let mut ang = String::new();
    ang.push_str(&oblig("602", t_impozit)); // impozit pe veniturile din salarii
    ang.push_str(&oblig("412", t_cas)); // CAS (pensii)
    ang.push_str(&oblig("432", t_cass)); // CASS (sănătate)
    ang.push_str(&oblig("480", t_cam)); // CAM (asiguratorie pentru muncă)
    let total_plata = t_impozit + t_cas + t_cass + t_cam; // totalPlata_A = Σ obligații angajator.
                                                          // angajatorB — numere asigurați + fond de salarii.
    ang.push_str(&format!(
        "    <angajatorB B_cnp=\"{count}\" B_sanatate=\"{count}\" B_pensie=\"{count}\" \
B_brutSalarii=\"{t_gross}\"/>\n"
    ));
    // angajatorC6 — bază + contribuție (sumar) — completat în aplicație; emis 0 pentru validitate.
    ang.push_str("    <angajatorC6 C6_baza=\"0\" C6_ct=\"0\"/>\n");

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
        // Root + namespace + header.
        assert!(
            xml.contains("<declaratieUnica xmlns=\"mfp:anaf:dgti:declaratie_unica:declaratie:v1\"")
        );
        assert!(xml.contains("luna_r=\"6\" an_r=\"2026\""));
        assert!(xml.contains("nume_declar=\"Popescu\""));
        // Employer obligation rows: 602 impozit (2×325=650), 412 CAS (2.500), 432 CASS (1.000),
        // 480 CAM (226) — totals over 2 employees.
        assert!(xml.contains("A_codOblig=\"602\" A_codBugetar=\"20470101XX\" A_datorat=\"650\""));
        assert!(xml.contains("A_codOblig=\"412\" A_codBugetar=\"20470101XX\" A_datorat=\"2500\""));
        assert!(xml.contains("A_codOblig=\"432\" A_codBugetar=\"20470101XX\" A_datorat=\"1000\""));
        assert!(xml.contains("A_codOblig=\"480\" A_codBugetar=\"20470101XX\" A_datorat=\"226\""));
        // totalPlata_A = 650 + 2500 + 1000 + 226 = 4376.
        assert!(xml.contains("totalPlata_A=\"4376\""));
        assert!(xml.contains("B_cnp=\"2\" B_sanatate=\"2\" B_pensie=\"2\" B_brutSalarii=\"10000\""));
        // Two insured persons with asiguratA contributions.
        assert_eq!(xml.matches("<asigurat ").count(), 2);
        assert!(xml.contains("A_1=\"1\" A_2=\"0\" A_3=\"N\" A_4=\"8\""));
        assert!(xml.contains("A_13=\"5000\" A_14=\"1250\"")); // baza CAS + CAS
    }
}
