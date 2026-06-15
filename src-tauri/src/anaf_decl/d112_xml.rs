//! D112 — Declarația 112 XML (model `:v7`).
//!
//! Namespace `mfp:anaf:dgti:declaratie_unica:declaratie:v7` + root `declaratieUnica`. CONFIRMAT
//! rulând validatorul OFICIAL ANAF `D112Validator.jar` (`-v D112`, build 209/Apr-2026): modelul curent
//! cere `:v7` (vechiul `d112_10102024.xsd` declara `:v6` — respins de validator). Cazul STANDARD
//! (salariați full-time, condiții normale, fără concediu medical) trece CURAT — `Validare fără erori`,
//! zero atenționări — vezi testul `dump_standard_d112` (`-v D112`). Deltele `:v7` față de `:v6`,
//! confirmate de validator + structura:
//!   • root `d_rec` (0 inițială / 1 rectificativă);
//!   • `angajator/@datCAM` (1 dacă se datorează CAM);
//!   • `angajatorA/@A_scutit` (A_plata = A_datorat − A_deductibil − A_scutit);
//!   • obligația CAM (480) = ROUND(Σ baza × 2,25%) pe baza AGREGATĂ (regula A21.46), NU Σ rotunjirilor
//!     per-salariat (altfel diferență de 1 leu); aceeași valoare la `angajatorC4/@C4_ct`;
//!   • `angajatorC1` (rollup CAS) + `angajatorC4` (rollup CAM) OBLIGATORII înainte de `asigurat`;
//!   • `angajatorC6` ELIMINAT (nu există în :v7);
//!   • `asigurat/@casaSn` + `asigurat/@Timp_E3` (= impozitul reținut al asiguratului);
//!   • `asiguratA`: `A_sal1` (bază contract) + `A_sal2` (brut realizat) + `A_5` = baza CAM (nu brutul)
//!     + `A_6` (ore lucrate) + `A_9` (baza șomaj); `A_20` ELIMINAT (impozitul nu e per-asigurat aici);
//!   • per-asigurat tip A: `asiguratE1` (rezumat funcția de bază) + `asiguratE3` (rând detaliat
//!     venit/impozit, E3_3=1 funcția de bază, E3_4='P' perioada curentă).
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
    /// E3_14 / E1_6 — venit bază de calcul al impozitului (taxable_base = brut − CAS − CASS − deduceri).
    pub baza_impozit: i64,
    /// E3_12 / E1_4 — deducere personală totală a lunii (de bază; suplimentara = 0 momentan).
    pub deducere: i64,
    /// CIF-ul sediului secundar la care e repartizat salariatul (D112 angajatorF2); '' = principal.
    pub sediu_cif: String,
    /// Certificatele de concediu medical ale lunii (OUG 158/2005). GOL ⇒ calea standard `asiguratA`
    /// (regression-safe). Cu ≥1 certificat ⇒ calea B (`asiguratB1/B2/B3/B4` + `asiguratD`/certificat),
    /// căci „Secțiunea D nu poate exista concomitent cu secțiunea A” (structura, poz 3502).
    pub med_leaves: Vec<D112MedicalLeave>,
}

/// Un certificat de concediu medical (OUG 158/2005) → blocul `asiguratD` (per certificat) + rollup-ul
/// angajator `angajatorC2`. Sumele/zilele sunt deja calculate (introduse în registrul concedii).
#[derive(Clone)]
pub struct D112MedicalLeave {
    /// D_1 seria, D_2 numărul certificatului.
    pub serie: String,
    pub numar: String,
    /// D_9 cod indemnizație (Nomenclator 9: „01" boală obișnuită, „07" carantină, „08" sarcină/lăuzie,
    /// „09" îngrijire copil, „15" risc maternal, „17" oncologic …).
    pub cod_indemn: String,
    /// D_5 data acordării, D_6 data început, D_7 data sfârșit valabilitate (zz.ll.aaaa).
    pub data_acordare: String,
    pub data_inceput: String,
    pub data_sfarsit: String,
    /// D_14 zile suportate de angajator, D_15 zile suportate din FNUASS (D_16 = D_14 + D_15).
    pub zile_ang: i64,
    pub zile_fnuass: i64,
    /// D_17 baza (Σ venituri ultimele 6 luni), D_18 nr. zile aferente (D_19 = ROUND(D_17/D_18, 2)).
    pub baza_calcul: i64,
    pub zile_baza: i64,
    /// D_20 indemnizația suportată de angajator, D_21 indemnizația suportată din FNUASS.
    pub suma_ang: i64,
    pub suma_fnuass: i64,
    /// D_28 procent (55/65/75) — doar pentru D_9 = „01"; 0 ⇒ se omite.
    pub procent: i64,
    /// D_10 loc prescriere (1-4, Nomenclator 8), D_23 cod boală (max 3 car.; „RM" pt. D_9=„15").
    pub loc_prescriere: i64,
    pub cod_boala: String,
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

use crate::anaf_decl::d112::cm_indemn_treatment;
use crate::anaf_decl::xml_esc as esc;

/// Construiește XML-ul D112 pentru o lună. `employees` sunt deja salariații activi cu contribuțiile
/// calculate (ratele 2026). Antetul A_codBugetar folosește codul bugetar standard al CAS/CASS/CAM.
pub fn generate_d112_xml(h: &D112Header, employees: &[D112Employee]) -> String {
    let count = employees.len() as i64;
    let tot = |f: fn(&D112Employee) -> i64| employees.iter().map(f).sum::<i64>();
    // Totaluri pe asigurat (lucrat + indemnizație de concediu medical). `month_totals` e SURSA UNICĂ
    // pentru obligațiile angajator (412 CAS / 432 CASS / angajatorC1) ȘI pentru detaliul de impozit
    // E1/E3 — pe luna cu CM, CAS/CASS se aplică pe baza lucrată + indemnizație (vezi `emit_leave_blocks`).
    // Fără concediu ⇒ valorile pe brutul lucrat (regression-safe, cazul standard neschimbat).
    let mt: Vec<MonthTotals> = employees.iter().map(month_totals).collect();
    let t_gross = tot(|e| e.gross);
    let t_impozit = tot(|e| e.impozit);
    let t_cas: i64 = mt.iter().map(|m| m.cas).sum();
    let t_cass: i64 = mt.iter().map(|m| m.cass).sum();
    // angajatorC1: C1_11 (total venit realizat conditii normale) = Σ baza CAS LUCRATĂ (ΣA_13 + ΣB2_5),
    // FĂRĂ indemnizație; baza CAS a indemnizației de CM (Σ B3_7) merge separat în C1_12 (regula A29/A36.2).
    let t_baza_cas: i64 = tot(|e| e.baza_cas);
    let t_b3_7: i64 = employees
        .iter()
        .flat_map(|e| &e.med_leaves)
        .map(|l| l.suma_ang + l.suma_fnuass)
        .sum();
    // CAM angajator (480) = ROUND(Σ baza_cam × 2,25%) pe baza AGREGATĂ (regula A21.46/A91b — NU Σ
    // rotunjirilor per-salariat; altfel diferență de 1 leu: 2×113=226 vs round(10000×2,25%)=225).
    // Indemnizația de CM NU e supusă CAM ⇒ baza CAM rămâne partea lucrată. Aceeași valoare la 480,
    // totalPlata_A, datCAM și C4_ct.
    let t_baza_cam = tot(|e| e.baza_cam);
    let t_cam = (t_baza_cam * 225 + 5000) / 10000;

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
    // NOTE: `angajatorC6` (emis sub :v6) NU există în :v7. Eliminat.
    // :v7 cere sumarele de reconciliere a contribuțiilor ÎNAINTE de blocurile `asigurat`, în ordine:
    // angajatorC1 (detaliu CAS pe condiții de muncă) → C2 (recuperări indemnizații FNUASS) → C4 (CAM).
    // Caz standard (condiții NORMALE, fără accidente / construcții): C1_11 = C1_T1 = Σ baza CAS (lucrat
    // + indemnizație CM), restul 0, C1_T3 = 0 (CacasN = 0%); C4_baza = Σ A_5 (baza CAM), C4_ct = Σ CAM.
    // C2 se emite DOAR când există concediu medical (altfel 0 ocurențe). C3 (accidente) / C5 (CAM
    // construcții) = 0 ocurențe în cazul standard.
    // C1_11 = C1_T1 = Σ baza CAS lucrată (condiții normale); C1_12 = C1_T2 = Σ B3_7 (baza CAS a
    // indemnizației OUG158); restul (deosebite/speciale/scutiri) = 0; C1_T3 = 0 (CacasN = 0%).
    ang.push_str(&format!(
        "    <angajatorC1 C1_11=\"{t_baza_cas}\" C1_12=\"{t_b3_7}\" C1_13=\"0\" C1_21=\"0\" \
C1_22=\"0\" C1_23=\"0\" C1_31=\"0\" C1_32=\"0\" C1_33=\"0\" C1_T1=\"{t_baza_cas}\" \
C1_T2=\"{t_b3_7}\" C1_T=\"0\" C1_T3=\"0\" C1_5=\"0\"/>\n"
    ));
    ang.push_str(&emit_angajator_c2(employees));
    // C4_ct = ROUND(C4_baza × 2,25%) = t_cam (aceeași regulă de rotunjire agregată ca obligația 480).
    ang.push_str(&format!(
        "    <angajatorC4 C4_baza=\"{t_baza_cam}\" C4_ct=\"{t_cam}\"/>\n"
    ));

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

    // asigurat* — câte unul per salariat. Fără concediu medical ⇒ blocul `asiguratA` (caz standard);
    // cu ≥1 certificat ⇒ calea B (`asiguratB1/B2/B3/B4` + `asiguratD`/certificat), căci secțiunile A și
    // B/D se exclud reciproc (structura poz 2496/3502). Detaliul de impozit `asiguratE1/E3` se emite în
    // AMBELE cazuri (e independent de ruta A/B). Vezi `emit_leave_blocks` pentru calea B.
    let mut asig = String::new();
    for (i, e) in employees.iter().enumerate() {
        let id = i + 1;
        let data = if e.data_ang.is_empty() {
            String::new()
        } else {
            format!(" dataAng=\"{}\"", esc(&e.data_ang))
        };
        // Totalurile lunii (lucrat + indemnizație CM) — precalculate în `mt`. Indemnizația de CM e
        // impozabilă (10%) și intră în baza CAS/CASS conform regulilor B4/CMscutit; e.cas/e.cass/
        // e.baza_* sunt pe partea LUCRATĂ, iar e.impozit/e.baza_impozit sunt TOTALELE lunii.
        let m = &mt[i];
        let t_indemn: i64 = e
            .med_leaves
            .iter()
            .map(|l| l.suma_ang + l.suma_fnuass)
            .sum();
        let contrib_block = if e.med_leaves.is_empty() {
            // calea A — asiguratA (:v7: A_5 = baza CAM (nu brutul), A_sal1/A_sal2, A_6/A_9; fără A_20).
            let a6 = e.ore_norma * e.zile;
            format!(
                "    <asiguratA A_1=\"{a1}\" A_sal1=\"{sal1}\" A_sal2=\"{sal2}\" A_2=\"{a2}\" \
A_3=\"{a3}\" A_4=\"{a4}\" A_5=\"{cam_base}\" A_6=\"{a6}\" A_7=\"0\" A_8=\"{zile}\" A_9=\"{a9}\" \
A_11=\"{baza_cass}\" A_12=\"{cass}\" A_13=\"{baza_cas}\" A_14=\"{cas}\"/>\n",
                a1 = esc(&e.tip_asigurat),
                sal1 = e.sal_contract,
                sal2 = e.gross,
                a2 = if e.pensionar { 1 } else { 0 },
                a3 = esc(&e.tip_contract),
                a4 = e.ore_norma,
                cam_base = e.baza_cam,
                a9 = e.baza_cas,
                zile = e.zile,
                baza_cass = e.baza_cass,
                cass = e.cass,
                baza_cas = e.baza_cas,
                cas = e.cas,
            )
        } else {
            emit_leave_blocks(e)
        };
        // asiguratE1/E3 (detaliu impozit pe venit) — comun ambelor căi. Pe luna cu CM, venitul brut
        // (E3_8) = brut lucrat + indemnizație; contribuțiile (E3_9) = CAS + CASS totale ale lunii.
        // E3_1 = secțiunea de contribuții ('A' fără CM, 'B' cu CM) — regula S122.2 cere ca tipul din E3
        // să corespundă secțiunii prezente (A/B/C); E3_2 = tipul asiguratului (= A_1 / B1_1).
        let (e3_1, e3_2) = if e.med_leaves.is_empty() {
            ("A", e.tip_asigurat.as_str())
        } else {
            ("B", "1") // calea B emite B1_1 = 1 (salariat)
        };
        let e3_8 = e.gross + t_indemn;
        let e3_9 = m.cas + m.cass;
        let net = e3_8 - m.cas - m.cass - e.impozit; // E3_16 suma încasată
        asig.push_str(&format!(
            "  <asigurat idAsig=\"{id}\" cnpAsig=\"{cnp}\" numeAsig=\"{nume}\" \
prenAsig=\"{pren}\"{data} casaSn=\"{casa}\" asigCI=\"1\" asigSO=\"1\" Timp_E3=\"{impozit}\">\n\
{contrib_block}\
    <asiguratE1 E1_1=\"{e3_8}\" E1_2=\"{e3_9}\" E1_3=\"0\" E1_4=\"{ded}\" E1_41=\"{ded}\" \
E1_42=\"0\" E1_421=\"0\" E1_422=\"0\" E1_5=\"0\" E1_6=\"{base}\" E1_7=\"{impozit}\"/>\n\
    <asiguratE3 E3_1=\"{e3_1}\" E3_2=\"{e3_2}\" E3_3=\"1\" E3_4=\"P\" E3_8=\"{e3_8}\" \
E3_9=\"{e3_9}\" E3_12=\"{ded}\" E3_121=\"{ded}\" E3_122=\"0\" E3_14=\"{base}\" E3_15=\"{impozit}\" \
E3_16=\"{net}\" E3_19=\"0\" E3_23=\"0\"/>\n\
  </asigurat>\n",
            cnp = esc(&e.cnp),
            nume = esc(&e.nume),
            pren = esc(&e.prenume),
            casa = esc(&h.casa),
            impozit = e.impozit,
            ded = e.deducere,
            base = e.baza_impozit,
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

/// ROUND(base × pct%) cu rotunjire comercială la leu întreg (half-up) — ex. 5000 × 2,25% → 113.
fn round_pct(base: i64, pct_num: i64, pct_den: i64) -> i64 {
    (base * pct_num + pct_den / 2) / pct_den
}

/// ROUND(num/den, 2) ca string „n.dd" (N(6.2) din structura). Den 0 ⇒ „0".
fn round2(num: i64, den: i64) -> String {
    if den == 0 {
        return "0".to_string();
    }
    let centi = (num * 100 + den / 2) / den;
    format!("{}.{:02}", centi / 100, centi % 100)
}

/// Contribuțiile + bazele TOTALE ale lunii pentru un asigurat (lucrat + indemnizație de concediu).
pub struct MonthTotals {
    pub cas: i64,
    pub cass: i64,
    pub baza_cas: i64,
    pub baza_cass: i64,
}

/// Calculează totalurile lunii. Fără concediu ⇒ valorile pe brutul lucrat (cazul standard, neschimbat).
/// Cu ≥1 certificat ⇒ baza CAS/CASS = baza lucrată + indemnizația de CM (B4_7 / B4_5 din structura):
/// indemnizația e supusă CAS 25% și CASS 10% (mai puțin codurile scutite de CASS — CMscutit, coduri
/// ∉ {01,07,10}). Indemnizația NU e supusă CAM. Sursă unică pentru obligațiile angajator + E1/E3.
fn month_totals(e: &D112Employee) -> MonthTotals {
    if e.med_leaves.is_empty() {
        return MonthTotals {
            cas: e.cas,
            cass: e.cass,
            baza_cas: e.baza_cas,
            baza_cass: e.baza_cass,
        };
    }
    let indemn: i64 = e
        .med_leaves
        .iter()
        .map(|l| l.suma_ang + l.suma_fnuass)
        .sum();
    // [P4 / DRY] Use cm_indemn_treatment (canonical source in d112.rs) instead of repeating the
    // {01,07,10} literal set. cass_due == false ⇒ CASS-exempt indemnity.
    let cm_scutit: i64 = e
        .med_leaves
        .iter()
        .filter(|l| !cm_indemn_treatment(l.cod_indemn.as_str()).0)
        .map(|l| l.suma_ang + l.suma_fnuass)
        .sum();
    let baza_cas = e.baza_cas + indemn;
    let baza_cass = e.baza_cass + (indemn - cm_scutit);
    MonthTotals {
        cas: round_pct(baza_cas, 25, 100),
        cass: round_pct(baza_cass, 10, 100),
        baza_cas,
        baza_cass,
    }
}

/// Calea B (concediu medical) a unui asigurat: `asiguratB1` (identitate contract + timp lucrat) +
/// `asiguratB2` (detaliu venit lucrat, dacă a lucrat) + `asiguratB3` (rollup indemnizații) +
/// `asiguratB4` (agregat CAS/CASS/CAM) + câte un `asiguratD` per certificat. Reconcilierile sunt cele
/// din structura D112 v7: B3_6=ΣD_16, B3_12=ΣD_20, B3_13=ΣD_21, B3_7=B3_12+B3_13; B4_7=baza CAS,
/// B4_8=ROUND(B4_7×25%), B4_5=baza CASS, B4_6=ROUND(B4_5×10%); D_16=D_14+D_15, D_19=ROUND(D_17/D_18,2).
fn emit_leave_blocks(e: &D112Employee) -> String {
    let m = month_totals(e);
    let b3_6: i64 = e
        .med_leaves
        .iter()
        .map(|l| l.zile_ang + l.zile_fnuass)
        .sum();
    let b3_12: i64 = e.med_leaves.iter().map(|l| l.suma_ang).sum();
    let b3_13: i64 = e.med_leaves.iter().map(|l| l.suma_fnuass).sum();
    let b3_7 = b3_12 + b3_13; // baza CAS indemnizație (B3_7S = 0 pentru salariatul normal)
    let b1_6 = e.ore_norma * e.zile; // ore lucrate efectiv
    let b1_10 = e.baza_cas; // baza șomaj = brutul lucrat (indemnizația nu e supusă șomajului)

    // B1 — identitate contract + timp lucrat (B1_1 = 1 salariat; satisface poarta D, structura 3526).
    let mut s = format!(
        "    <asiguratB1 B1_1=\"1\" B1_sal1=\"{sal1}\" B1_sal2=\"{gross}\" B1_2=\"{pens}\" \
B1_3=\"{contract}\" B1_4=\"{ore}\" B1_5=\"{cam}\" B1_6=\"{b1_6}\" B1_7=\"0\" B1_10=\"{b1_10}\" \
B1_15=\"{zile}\"/>\n",
        sal1 = e.sal_contract,
        gross = e.gross,
        pens = if e.pensionar { 1 } else { 0 },
        contract = esc(&e.tip_contract),
        ore = e.ore_norma,
        cam = e.baza_cam,
        zile = e.zile,
    );
    // B2 — detaliu venit lucrat (condiții normale). Doar dacă a existat timp lucrat în lună.
    if e.zile > 0 {
        s.push_str(&format!(
            "    <asiguratB2 B2_2=\"{zile}\" B2_5=\"{baza_cas}\" B2_5S=\"0\" B2_5C=\"0\" \
B2_6S=\"0\" B2_6C=\"0\" B2_7S=\"0\" B2_7C=\"0\"/>\n",
            zile = e.zile,
            baza_cas = e.baza_cas,
        ));
    }
    // B3 — rollup indemnizații. B3_1 (zile indemnizații condiții normale) = B3_6 (regula V88:
    // B3_1+B3_2+B3_3 = B3_6+B3_8; condiții normale ⇒ totul în B3_1).
    s.push_str(&format!(
        "    <asiguratB3 B3_1=\"{b3_6}\" B3_6=\"{b3_6}\" B3_7=\"{b3_7}\" B3_12=\"{b3_12}\" \
B3_13=\"{b3_13}\"/>\n"
    ));
    // B4 — agregat CAS/CASS/CAM; câmpurile „…P/…S/…C/…D" sunt DA (=0 pentru full-time normă întreagă).
    s.push_str(&format!(
        "    <asiguratB4 B4_1=\"{zile}\" B4_3=\"{b1_10}\" B4_5=\"{b4_5}\" B4_6=\"{b4_6}\" \
B4_7=\"{b4_7}\" B4_8=\"{b4_8}\" B4_7P=\"0\" B4_8P=\"0\" B4_5P=\"0\" B4_6P=\"0\" B4_7S=\"0\" \
B4_7C=\"0\" B4_8D=\"0\" B4_6D=\"0\" B4_14=\"{cam}\"/>\n",
        zile = e.zile,
        b4_5 = m.baza_cass,
        b4_6 = m.cass,
        b4_7 = m.baza_cas,
        b4_8 = m.cas,
        cam = e.baza_cam,
    ));
    // asiguratD — câte unul per certificat.
    for l in &e.med_leaves {
        let d16 = l.zile_ang + l.zile_fnuass;
        let d19 = round2(l.baza_calcul, l.zile_baza);
        // D_28 (procent) doar pentru boala obișnuită (D_9 = 01); altfel se omite.
        let d28 = if l.cod_indemn == "01" && l.procent > 0 {
            format!(" D_28=\"{}\"", l.procent)
        } else {
            String::new()
        };
        s.push_str(&format!(
            "    <asiguratD D_1=\"{serie}\" D_2=\"{numar}\" D_5=\"{acord}\" D_6=\"{inc}\" \
D_7=\"{sf}\" D_9=\"{cod}\" D_10=\"{loc}\" D_14=\"{za}\" D_15=\"{zf}\" D_16=\"{d16}\" \
D_17=\"{baza}\" D_18=\"{zb}\" D_19=\"{d19}\" D_20=\"{sa}\" D_21=\"{sfn}\" D_23=\"{boala}\"{d28}/>\n",
            serie = esc(&l.serie),
            numar = esc(&l.numar),
            acord = esc(&l.data_acordare),
            inc = esc(&l.data_inceput),
            sf = esc(&l.data_sfarsit),
            cod = esc(&l.cod_indemn),
            loc = l.loc_prescriere,
            za = l.zile_ang,
            zf = l.zile_fnuass,
            baza = l.baza_calcul,
            zb = l.zile_baza,
            sa = l.suma_ang,
            sfn = l.suma_fnuass,
            boala = esc(&l.cod_boala),
        ));
    }
    s
}

/// angajatorC2 — rollup-ul angajator al recuperărilor de indemnizații din FNUASS (peste TOȚI asigurații
/// cu concediu medical). Se emite doar dacă există certificate. Rd1 = incapacitate temporară de muncă
/// (COUNT + Σ pe D_16/D_14/D_15/D_20/D_21); sub-rândurile pe procent (Rd1.2/1.3/1.4 = D_9„01" cu
/// D_28 55/65/75); totaluri Rd6-8 (C2_T6 = Σ recuperat FNUASS, C2_10 = C2_T6, C2_140 = C2_10).
fn emit_angajator_c2(employees: &[D112Employee]) -> String {
    let leaves: Vec<&D112MedicalLeave> =
        employees.iter().flat_map(|e| e.med_leaves.iter()).collect();
    if leaves.is_empty() {
        return String::new();
    }
    // (COUNT, Σ zile total, Σ zile ang, Σ zile FNUASS, Σ sumă ang, Σ sumă FNUASS) peste un filtru.
    let agg = |f: &dyn Fn(&D112MedicalLeave) -> bool| -> (i64, i64, i64, i64, i64, i64) {
        let sel = leaves.iter().filter(|l| f(l));
        sel.fold((0, 0, 0, 0, 0, 0), |a, l| {
            (
                a.0 + 1,
                a.1 + l.zile_ang + l.zile_fnuass,
                a.2 + l.zile_ang,
                a.3 + l.zile_fnuass,
                a.4 + l.suma_ang,
                a.5 + l.suma_fnuass,
            )
        })
    };
    // Rd1 — incapacitate temporară de muncă (boală obișnuită + coduri asimilate).
    let inc = |l: &D112MedicalLeave| {
        matches!(
            l.cod_indemn.as_str(),
            "01" | "02" | "03" | "04" | "05" | "51" | "06" | "12" | "13" | "14" | "16"
        )
    };
    let r1 = agg(&inc);
    let mut s = format!(
        "    <angajatorC2 C2_11=\"{}\" C2_12=\"{}\" C2_13=\"{}\" C2_14=\"{}\" C2_15=\"{}\" \
C2_16=\"{}\"",
        r1.0, r1.1, r1.2, r1.3, r1.4, r1.5
    );
    // Sub-rândurile pe procent pentru boală obișnuită (D_9 = „01"): 55% / 65% / 75%
    // (scala graduală OUG 91/2025, de la 01.01.2026; procentul este introdus de utilizator).
    for (pct, lo) in [(55u8, "121"), (65, "131"), (75, "141")] {
        let r = agg(&|l| l.cod_indemn == "01" && l.procent == pct as i64);
        if r.0 > 0 {
            let n: i64 = lo.parse().unwrap();
            s.push_str(&format!(
                " C2_{}=\"{}\" C2_{}=\"{}\" C2_{}=\"{}\" C2_{}=\"{}\" C2_{}=\"{}\" C2_{}=\"{}\"",
                n,
                r.0,
                n + 1,
                r.1,
                n + 2,
                r.2,
                n + 3,
                r.3,
                n + 4,
                r.4,
                n + 5,
                r.5
            ));
        }
    }
    // Totaluri FNUASS recuperat: C2_T6 = Σ indemnizații FNUASS, C2_10 = C2_T6, C2_140 = C2_10.
    let t_fnuass: i64 = leaves.iter().map(|l| l.suma_fnuass).sum();
    s.push_str(&format!(
        " C2_T6=\"{t_fnuass}\" C2_10=\"{t_fnuass}\" C2_140=\"{t_fnuass}\"/>\n"
    ));
    s
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
            baza_impozit: 3250, // 5000 − 1250 CAS − 500 CASS (deducere 0 la acest venit)
            deducere: 0,
            sediu_cif: "".into(),
            med_leaves: vec![],
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
        // The exported/previewed D112 is pretty_print(generate_d112_xml(..)) — assert that the saved
        // form is the canonical professional format (UTF-8 prolog + LF + 2-space), like every decl.
        crate::anaf_decl::xml::assert_canonical_xml(&crate::anaf_decl::xml::pretty_print(&xml));
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
        // 480 CAM (225) — totals over 2 employees. CAM = ROUND(Σ baza × 2,25%) = round(10000×2,25%)
        // = 225 (NU 2×113=226: regula ANAF A21.46 rotunjește baza agregată). Budget codes per
        // structura: 5503XXXXXX for impozit/CAS/CASS, 20470300XX for CAM.
        assert!(xml.contains("A_codOblig=\"602\" A_codBugetar=\"5503XXXXXX\" A_datorat=\"650\""));
        assert!(xml.contains("A_codOblig=\"412\" A_codBugetar=\"5503XXXXXX\" A_datorat=\"2500\""));
        assert!(xml.contains("A_codOblig=\"432\" A_codBugetar=\"5503XXXXXX\" A_datorat=\"1000\""));
        assert!(xml.contains("A_codOblig=\"480\" A_codBugetar=\"20470300XX\" A_datorat=\"225\""));
        // totalPlata_A = 650 + 2500 + 1000 + 225 = 4375.
        assert!(xml.contains("totalPlata_A=\"4375\""));
        // C4 reconciliere CAM: C4_baza = Σ baza_cam (10000), C4_ct = obligația 480 (225).
        assert!(xml.contains("<angajatorC4 C4_baza=\"10000\" C4_ct=\"225\"/>"));
        // angajatorB includes the required B_sal (= count for the all-salaried standard case).
        assert!(xml.contains(
            "B_cnp=\"2\" B_sanatate=\"2\" B_pensie=\"2\" B_sal=\"2\" B_brutSalarii=\"10000\""
        ));
        // Two insured persons with asiguratA contributions; :v7 attrs A_scutit / casaSn / A_sal1 /
        // Timp_E3 present, A_20 gone.
        assert_eq!(xml.matches("<asigurat ").count(), 2);
        assert!(xml.contains("A_deductibil=\"0\" A_scutit=\"0\" A_plata=\"650\""));
        assert!(xml.contains("casaSn=\"CJ\" asigCI=\"1\" asigSO=\"1\" Timp_E3=\"325\""));
        assert!(
            xml.contains("A_1=\"1\" A_sal1=\"5000\" A_sal2=\"5000\" A_2=\"0\" A_3=\"N\" A_4=\"8\"")
        );
        assert!(xml.contains("A_13=\"5000\" A_14=\"1250\"")); // baza CAS + CAS
        assert!(!xml.contains("A_20=")); // per-employee impozit field does not exist in :v7
                                         // :v7 per-asigurat income-tax detail: asiguratE1 (function-of-base summary) + asiguratE3
                                         // (one detail row, E3_3=1 funcția de bază, E3_4='P', E3_8 brut, E3_14 bază, E3_15 impozit).
        assert!(xml.contains("<asiguratE1 E1_1=\"5000\" E1_2=\"1750\""));
        assert!(xml.contains("E1_6=\"3250\" E1_7=\"325\"/>"));
        assert!(xml.contains(
            "<asiguratE3 E3_1=\"A\" E3_2=\"1\" E3_3=\"1\" E3_4=\"P\" E3_8=\"5000\" E3_9=\"1750\""
        ));
        assert!(
            xml.contains("E3_14=\"3250\" E3_15=\"325\" E3_16=\"2925\" E3_19=\"0\" E3_23=\"0\"/>")
        );
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

    /// Un salariat cu lună mixtă: 16 zile lucrate (brut 4000) plus un certificat de concediu medical
    /// de boală obișnuită (D_9=01) de 5 zile suportate de angajator. Indemnizația (508) e supusă CAS
    /// 25%, CASS 10% și impozit 10% (NU CAM). month_totals: baza CAS/CASS = 4000+508 = 4508 ⇒ CAS
    /// 1127, CASS 451; brut total E3_8 = 4508; impozit total 293 pe baza 2930.
    fn leave_emp(cnp: &str, nume: &str) -> D112Employee {
        D112Employee {
            cnp: cnp.into(),
            nume: nume.into(),
            prenume: "Ion".into(),
            data_ang: "01.01.2024".into(),
            gross: 4000, // brut LUCRAT (16 zile)
            cas: 1000,
            cass: 400,
            impozit: 293, // TOTAL (lucrat + indemnizație)
            cam: 90,
            zile: 16, // zile lucrate
            tip_asigurat: "1".into(),
            pensionar: false,
            tip_contract: "N".into(),
            ore_norma: 8,
            baza_cas: 4000, // baza LUCRATĂ
            baza_cass: 4000,
            baza_cam: 4000,
            sal_contract: 5000,
            baza_impozit: 2930, // 4508 brut − 1578 (CAS+CASS) − 0 deducere
            deducere: 0,
            sediu_cif: "".into(),
            med_leaves: vec![D112MedicalLeave {
                serie: "AB".into(),
                numar: "1234567".into(),
                cod_indemn: "01".into(),
                // CM inițial: data acordării (D_5) ≥ data început (D_6) — regula S95.2.
                data_acordare: "06.06.2026".into(),
                data_inceput: "06.06.2026".into(),
                data_sfarsit: "10.06.2026".into(),
                zile_ang: 5,
                zile_fnuass: 0,
                baza_calcul: 24000,
                zile_baza: 130,
                suma_ang: 508,
                suma_fnuass: 0,
                procent: 55,
                loc_prescriere: 1,
                cod_boala: "A09".into(),
            }],
        }
    }

    #[test]
    fn d112_leave_emits_b_path_not_a() {
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
        let xml = generate_d112_xml(&h, &[leave_emp("1", "A")]);
        // Calea B: B1/B2/B3/B4 + asiguratD, fără asiguratA (se exclud reciproc).
        assert!(xml.contains("<asiguratB1 B1_1=\"1\""));
        assert!(xml.contains("<asiguratB2 "));
        assert!(xml.contains(
            "<asiguratB3 B3_1=\"5\" B3_6=\"5\" B3_7=\"508\" B3_12=\"508\" B3_13=\"0\"/>"
        ));
        // Calea B ⇒ E3_1='B' (regula S122.2: tipul E3 = secțiunea de contribuții prezentă).
        assert!(xml.contains("E3_1=\"B\" E3_2=\"1\""));
        // B4: baza CAS/CASS = 4000 + 508 = 4508; CAS 1127, CASS 451; CAM doar pe partea lucrată (4000).
        assert!(xml.contains("B4_5=\"4508\" B4_6=\"451\" B4_7=\"4508\" B4_8=\"1127\""));
        assert!(xml.contains("B4_14=\"4000\""));
        assert!(!xml.contains("<asiguratA "));
        // asiguratD: D_16 = D_14+D_15 = 5; D_19 = ROUND(24000/130,2) = 184.62; D_28 = 55 (boală 01).
        assert!(xml.contains("D_1=\"AB\" D_2=\"1234567\""));
        assert!(xml.contains("D_14=\"5\" D_15=\"0\" D_16=\"5\""));
        assert!(xml.contains("D_19=\"184.62\""));
        assert!(xml.contains("D_23=\"A09\" D_28=\"55\""));
        // angajatorC2 rollup: COUNT=1, Σ zile=5, Σ sumă ang=508; sub-rând 55% prezent.
        assert!(xml.contains("<angajatorC2 C2_11=\"1\" C2_12=\"5\""));
        assert!(xml.contains("C2_121=\"1\""));
        // Obligația angajator 412 CAS = 1127, 432 CASS = 451 (pe baza lucrat + indemnizație).
        assert!(xml.contains("A_codOblig=\"412\" A_codBugetar=\"5503XXXXXX\" A_datorat=\"1127\""));
        assert!(xml.contains("A_codOblig=\"432\" A_codBugetar=\"5503XXXXXX\" A_datorat=\"451\""));
        // Detaliu impozit comun: E3_8 = 4508 (brut + indemnizație), Timp_E3 = 293.
        assert!(xml.contains("Timp_E3=\"293\""));
        assert!(xml.contains("E3_8=\"4508\""));
    }

    /// Dev helper (opt-in): write a WITH-MEDICAL-LEAVE D112 (calea B) for the real ANAF D112Validator.
    ///   cargo test --lib anaf_decl::d112_xml::tests::dump_leave_d112 -- --ignored --nocapture
    #[test]
    #[ignore]
    fn dump_leave_d112() {
        let h = D112Header {
            luna: 6,
            an: 2026,
            d_rec: 0,
            nume_declar: "Popescu".into(),
            prenume_declar: "Maria".into(),
            functie_declar: "Administrator".into(),
            cif: "13548146".into(),
            caen: "6201".into(),
            den: "Test SRL".into(),
            casa: "CJ".into(),
        };
        // Un salariat standard + unul cu concediu medical (acoperă ambele căi în aceeași declarație).
        let xml = generate_d112_xml(
            &h,
            &[
                emp("1960101410019", "Popescu"),
                leave_emp("1900101410011", "Ionescu"),
            ],
        );
        let path = std::env::temp_dir().join("d112_leave.xml");
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
