//! D112 payroll — the per-employee salary computation that the D112 nominal annex is built from.
//!
//! 2026 rates (verified; the IT/construcții/agri exemptions were removed by OUG 156/2024):
//! CAS (pensie, salariat) 25%; CASS (sănătate, salariat) 10%; impozit pe venit 10% (pe baza după
//! CAS+CASS și deducerea personală); CAM (asigurătorie pentru muncă, angajator) 2,25%. Salariu
//! minim 2026: 4.050 lei (sem. I) / 4.325 lei (de la 1 iulie).
//!
//! This module computes ONE salary state (brut → net + contribuții + cost angajator). The full
//! D112 (evidența nominală a salariaților, stările lunare, exportul XML cu cele două versiuni de
//! schemă din 2026 și notele GL 641/421, 4315, 4316, 444, 646/436) este o extensie ulterioară —
//! acesta este nucleul de calcul reutilizabil.

use rust_decimal::Decimal;
use rust_decimal::RoundingStrategy;
use serde::{Deserialize, Serialize};

/// 2026 contribution + tax rates (percent).
const CAS_PCT: (i64, u32) = (25, 2); // 0.25
const CASS_PCT: (i64, u32) = (10, 2); // 0.10
const INCOME_TAX_PCT: (i64, u32) = (10, 2); // 0.10
pub(crate) const CAM_PCT: (i64, u32) = (225, 4); // 0.0225
/// Contribuția pentru concedii și indemnizații de asigurări sociale de sănătate (CCI, OUG 158/2005
/// art. 4 alin. (2), modificat de Legea 399/2006) — datorată de ANGAJATOR, separat de CAM. 0,85% pe
/// fondul de salarii (venitul supus CASS), în vigoare din 1 ian. 2007. NU a fost inclusă în CAM de
/// reforma 2018 (confirmat 2026 — cota 0,85%, nu 0,75% din forma inițială 2005).
pub(crate) const CONCEDII_PCT: (i64, u32) = (85, 4); // 0.0085

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PayrollInput {
    /// Salariul brut lunar.
    pub gross: Decimal,
    /// Deducerea personală (din tabelul ANAF, în funcție de venit + persoane în întreținere).
    #[serde(default)]
    pub personal_deduction: Decimal,
    /// Suma netaxabilă (art. III OUG 89/2025): 300 lei sem. I / 200 lei sem. II 2026, scutită de
    /// impozit ȘI de CAS/CASS/CAM. Se rezolvă cu [`suma_netaxabila`] (0 dacă nu se aplică). Scăzută
    /// din baza de calcul ÎNAINTE de toate cele patru prelevări.
    #[serde(default)]
    pub non_taxable: Decimal,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PayrollResult {
    pub gross: String,
    pub cas: String,
    pub cass: String,
    pub personal_deduction: String,
    pub taxable_base: String,
    pub income_tax: String,
    pub net: String,
    pub cam: String,
    /// Contribuția pentru concedii și indemnizații 0,85% (angajator, OUG 158/2005). Inclusă în
    /// total_employer_cost.
    pub concedii: String,
    pub total_employer_cost: String,
    /// Suma netaxabilă aplicată efectiv (300/200 lei sau 0).
    pub non_taxable: String,
}

pub(crate) fn pct(d: Decimal, (n, s): (i64, u32)) -> Decimal {
    // Contributions/tax rounded to whole lei with COMMERCIAL rounding (half away from zero), the
    // ANAF convention — e.g. 5.000 × 2,25% = 112,5 → 113 (banker's would give 112).
    (d * Decimal::new(n, s)).round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero)
}
fn fmt(d: Decimal) -> String {
    let d = d.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero);
    let d = if d.is_zero() { Decimal::ZERO } else { d };
    format!("{:.2}", d)
}

/// True dacă salariatul e EXCEPTAT de la baza minimă CAS/CASS part-time conform art. 146 alin. (5^7)
/// Cod fiscal (pentru el baza rămâne venitul realizat). Categoriile (lit. a–e), OG 16/2022:
/// a) elevi/studenți până la 26 ani; b) ucenici până la 18 ani; c) persoane cu dizabilități / care
/// pot lucra < 8h/zi potrivit legii; d) pensionari (limită de vârstă) — flagul `pensionar`;
/// e) venit cumulat din mai multe contracte ≥ salariul minim (procedura OMF 1855/2022).
pub fn exempt_part_time_min_base(pensionar: bool, exceptie_cas_min: &str) -> bool {
    pensionar
        || matches!(
            exceptie_cas_min,
            "elev_student" | "ucenic" | "dizabilitate" | "contracte_multiple"
        )
}

/// Part-time (contract Pi) minimum CAS/CASS base override — art. 146 alin. (5^6)-(5^9) + art. 168
/// alin. (6^1) Cod fiscal (OG 16/2022), cu derogarea sumei netaxabile (OUG 156/2024). Baza CAS/CASS
/// nu poate fi sub salariul minim ÎNTREG (NU prorata cu fracția de normă orară). 2026: 4.050−300 =
/// 3.750 lei (sem. I) / 4.325−200 = 4.125 lei (de la 1 iulie, HG 146/2026). Diferența de contribuție
/// față de cea pe venitul realizat e suportată de ANGAJATOR. `exempt` (art. 146 (5^7), via
/// [`exempt_part_time_min_base`]) sare peste majorare — baza rămâne venitul realizat.
///
/// Limitare cunoscută: art. 146 (5^6) permite proratarea bazei minime la numărul zilelor lucrătoare
/// din lună în care contractul a fost ACTIV (angajare/încetare la mijlocul lunii). Aplicația nu
/// urmărește încă data încetării / zilele active, deci se folosește baza minimă ÎNTREAGĂ — o
/// supra-declarare conservatoare (protejează baza de pensie a salariatului). Proratarea pe zile e o
/// extensie ulterioară.
///
/// CAM (contribuția asigurătorie pentru muncă, art. 220^4-220^7): NU se ridică la baza minimă —
/// art. 146 alin. (5^6)-(5^9) numește DOAR CAS și CASS, iar baza CAM (art. 220^6) = câștigul brut
/// REALIZAT. D112 nu are un mecanism de „diferență CAM" suportată de angajator (spre deosebire de
/// CAS→4315 / CASS→4316 via 6458). Deci CAM rămâne calculată pe brutul realizat (vezi
/// `compute_payroll`), iar acest helper NU întoarce un `cam_diff`. (Ambiguitate statutară cunoscută
/// — tăcerea legii e interpretată ca excludere; de confirmat cu ANAF/CECCAR dacă apare îndoială.)
///
/// Returnează Some((baza_minimă, cas_diff_angajator, cass_diff_angajator)) când se aplică majorarea.
pub fn part_time_min_base(
    gross: Decimal,
    tip_contract: &str,
    exempt: bool,
    year: i32,
    month: u32,
) -> Option<(Decimal, Decimal, Decimal)> {
    if tip_contract == "N" || exempt || gross <= Decimal::ZERO {
        return None;
    }
    // Baza minimă = salariul minim − suma netaxabilă (NU se prorata cu ore/normă). Derivată din
    // sursa unică (min_wage_lei − carve_out_lei): 2026 H1 = 4.050−300 = 3.750; H2 = 4.325−200 = 4.125.
    let base = min_wage_lei(year, month) - carve_out_lei(year, month);
    if gross >= base {
        return None; // venitul realizat ≥ baza minimă → fără majorare.
    }
    let cas_diff = pct(base, CAS_PCT) - pct(gross, CAS_PCT);
    let cass_diff = pct(base, CASS_PCT) - pct(gross, CASS_PCT);
    Some((base, cas_diff, cass_diff))
}

/// Salariul minim brut pe țară garantat în plată (lei/lună) — SURSĂ UNICĂ, keyed pe (an, lună):
/// 2026 = 4.050 sem. I (HG 1506/2024) / 4.325 sem. II (HG 146/2026, de la 1 iulie 2026). Pentru un
/// an neacoperit avertizează și folosește ultima valoare cunoscută — drift guard: altfel un apel
/// din 2027 ar reutiliza tăcut 4.325. La următorul HG, adăugați aici rândul noului an.
pub(crate) fn min_wage_lei(year: i32, month: u32) -> Decimal {
    match (year, month) {
        (2026, m) if m <= 6 => Decimal::from(4050),
        (2026, _) => Decimal::from(4325),
        _ => {
            tracing::warn!(
                year,
                month,
                "min_wage_lei: an/lună neacoperit(ă) — folosesc 4.325 (2026 sem. II); \
                 actualizați cu noul salariu minim"
            );
            Decimal::from(4325)
        }
    }
}

/// Suma netaxabilă lunară din salariul minim (carve-out art. III OUG 89/2025) — SURSĂ UNICĂ:
/// 300 lei sem. I 2026 / 200 lei sem. II 2026. Drift: pentru un an viitor cade pe valoarea sem. II
/// (200) FĂRĂ warn propriu — [`min_wage_lei`], apelată mereu împreună (per salariat), emite deja
/// avertismentul de an neacoperit; un al doilea/treilea warn ar fi doar zgomot.
pub(crate) fn carve_out_lei(year: i32, month: u32) -> Decimal {
    match (year, month) {
        (2026, m) if m <= 6 => Decimal::from(300),
        (2026, _) => Decimal::from(200),
        _ => {
            tracing::warn!(
                year,
                month,
                "carve_out_lei: an neacoperit — se reutilizează valorile 2026"
            );
            Decimal::from(200)
        }
    }
}

/// Plafonul brutului realizat (inclusiv) până la care se acordă carve-out-ul: 4.300 sem. I /
/// 4.600 sem. II 2026. Valoare legiferată DISTINCTĂ (nu = salariul minim + carve-out), keyed pe lună.
/// Aceeași convenție de drift ca [`carve_out_lei`] — warn-ul de an neacoperit e în [`min_wage_lei`].
fn carve_out_gross_ceiling(year: i32, month: u32) -> Decimal {
    match (year, month) {
        (2026, m) if m <= 6 => Decimal::from(4300),
        (2026, _) => Decimal::from(4600),
        _ => {
            tracing::warn!(
                year,
                month,
                "carve_out_gross_ceiling: an neacoperit — se reutilizează valorile 2026"
            );
            Decimal::from(4600)
        }
    }
}

/// Suma netaxabilă din salariul minim — art. III OUG 89/2025 (continuă OUG 156/2024 art. LXVI).
/// 300 lei/lună sem. I 2026 / 200 lei/lună sem. II 2026, scutită de impozit pe venit ȘI de
/// CAS/CASS/CAM (derogare art. 78/139(1)/140/157(1)/220^4(1) Cod fiscal).
///
/// Condiții CUMULATIVE: (a) salariat cu normă întreagă pe CIM (tip_contract "N"); (b) salariul de
/// bază contractual = salariul minim brut în vigoare (4.050 sem. I / 4.325 sem. II); (c) venitul brut
/// realizat (fără tichete/vouchere) ≤ 4.300 sem. I / 4.600 sem. II inclusiv; (d) angajatorul nu a
/// diminuat salariul de bază între 01.01.2026 și 31.12.2026.
///
/// `beneficiar` este ATESTAREA contabilului că (b)+(d) sunt îndeplinite (aplicația nu modelează
/// salariul de bază contractual separat de brut, nici istoricul diminuărilor). Aici aplicăm automat
/// (a) normă întreagă + (c) plafonul brut; restul țin de flag. Întoarce 0 dacă nu se aplică.
///
/// Limitare cunoscută: nu se prorata pe zile pentru luni parțiale (angajare/încetare la mijlocul
/// lunii) — se aplică suma întreagă (conservator), aliniat cu [`part_time_min_base`].
pub fn suma_netaxabila(
    beneficiar: bool,
    tip_contract: &str,
    gross: Decimal,
    year: i32,
    month: u32,
) -> Decimal {
    if !beneficiar || tip_contract != "N" || gross <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    // Sumă + plafon din sursa unică keyed pe (an, lună): 300/4.300 sem. I, 200/4.600 sem. II 2026.
    let amount = carve_out_lei(year, month);
    let ceiling = carve_out_gross_ceiling(year, month);
    if gross > ceiling {
        return Decimal::ZERO; // peste plafonul brut → întreaga sumă netaxabilă se pierde
    }
    amount.min(gross)
}

/// Compute one monthly salary state from the gross + personal deduction (2026 rates).
/// `input.non_taxable` (resolved by [`suma_netaxabila`]) is carved out of the base BEFORE CAS, CASS,
/// CAM and income tax (art. III OUG 89/2025).
pub fn compute_payroll(input: &PayrollInput) -> PayrollResult {
    let z = Decimal::ZERO;
    let gross = input.gross.max(z);
    let non_taxable = input.non_taxable.max(z).min(gross);
    // Contribution base = gross − suma netaxabilă; CAS/CASS/CAM all computed on it.
    let contrib_base = (gross - non_taxable).max(z);
    let cas = pct(contrib_base, CAS_PCT);
    let cass = pct(contrib_base, CASS_PCT);
    let after_contrib = gross - cas - cass;
    let deduction = input.personal_deduction.max(z).min(after_contrib.max(z));
    // Income-tax base = venit net − deducere personală − suma netaxabilă (Baza_impozit, FGO/Cod fiscal).
    let taxable_base = (after_contrib - deduction - non_taxable).max(z);
    let income_tax = pct(taxable_base, INCOME_TAX_PCT);
    let net = gross - cas - cass - income_tax;
    let cam = pct(contrib_base, CAM_PCT);
    // CCI 0,85% (angajator) pe aceeași bază ca CAM (venitul supus CASS). Plafonul art. 6(8) (12×
    // salariul minim/asigurat ≈ 48.600 lei/lună) practic nu se atinge → neaplicat aici.
    let concedii = pct(contrib_base, CONCEDII_PCT);
    let total_employer_cost = gross + cam + concedii;

    PayrollResult {
        gross: fmt(gross),
        cas: fmt(cas),
        cass: fmt(cass),
        personal_deduction: fmt(deduction),
        taxable_base: fmt(taxable_base),
        income_tax: fmt(income_tax),
        net: fmt(net),
        cam: fmt(cam),
        concedii: fmt(concedii),
        total_employer_cost: fmt(total_employer_cost),
        non_taxable: fmt(non_taxable),
    }
}

// ─── Calcul salarial cu concediu medical (OUG 158/2005) ──────────────────────

/// Un certificat de concediu medical redus la inputurile de calcul salarial. Tratamentul fiscal
/// (`cass_due`, `taxable`) e derivat din codul de indemnizație (Nomenclator 9) prin
/// [`cm_indemn_treatment`].
#[derive(Debug, Clone)]
pub struct LeaveCert {
    /// Indemnizația brută suportată de angajator (D_20).
    pub indemn_employer: Decimal,
    /// Indemnizația brută suportată din FNUASS (D_21).
    pub indemn_fnuass: Decimal,
    /// Zile lucrătoare de concediu (D_14 + D_15) — scad din zilele lucrate ale lunii.
    pub leave_working_days: u32,
    /// CASS 10% se datorează pe indemnizație? (codurile 01/07/10; restul = scutit — structura D112
    /// CMscutit însumează indemnizațiile pentru coduri ∉{01,07,10}).
    pub cass_due: bool,
    /// Indemnizația e impozabilă (impozit 10%)?
    pub taxable: bool,
}

/// Inputul calculului salarial al unei luni CU concediu medical.
#[derive(Debug, Clone)]
pub struct LeavePayrollInput {
    /// Salariul de bază lunar (full); se proratează la zilele lucrate.
    pub gross: Decimal,
    pub personal_deduction: Decimal,
    /// Suma netaxabilă (carve-out 300/200), aplicată pe brutul lucrat.
    pub non_taxable: Decimal,
    /// Zile lucrătoare în lună (Luni-Vineri).
    pub working_days: u32,
    pub certs: Vec<LeaveCert>,
}

/// Rezultatul calculului salarial al unei luni cu concediu medical.
#[derive(Debug, Clone)]
pub struct LeavePayrollResult {
    /// Zile efectiv lucrate (= zile lucrătoare − zile de concediu).
    pub worked_days: u32,
    /// Salariul brut lucrat (proratat la zilele lucrate).
    pub worked_gross: Decimal,
    /// Baza CAS/CASS pe partea LUCRATĂ (brut lucrat − suma netaxabilă) — intră în asiguratB2/B4 ale D112.
    pub worked_base: Decimal,
    pub indemn_employer: Decimal,
    pub indemn_fnuass: Decimal,
    pub indemn_total: Decimal,
    /// CAS 25% pe (baza lucrată + toată indemnizația).
    pub cas: Decimal,
    /// CASS 10% pe (baza lucrată + indemnizația ne-scutită).
    pub cass: Decimal,
    /// CAM 2,25% DOAR pe baza lucrată (indemnizația nu e supusă CAM).
    pub cam: Decimal,
    /// CCI 0,85% (angajator) DOAR pe baza lucrată (ca și CAM).
    pub concedii: Decimal,
    /// Impozit 10% pe (venit lucrat + indemnizație impozabilă) − contribuții − deducere − sumă netaxabilă.
    pub income_tax: Decimal,
    pub taxable_base: Decimal,
    /// Net total = (brut lucrat + indemnizație) − CAS − CASS − impozit.
    pub net: Decimal,
}

/// Tratamentul contribuțiilor/impozitului pe indemnizația de concediu medical, după codul de
/// indemnizație (Nomenclator 9, OUG 158/2005). Întoarce `(cass_due, taxable)`.
///
/// CASS: datorată (10%) DOAR pentru codurile 01/07/10 (OUG 115/2023, de la 01.01.2024; ANAF
/// „Precizări CASS concedii medicale”); structura D112 scutește de CASS indemnizațiile pentru codurile
/// ∉{01,07,10} (regula CMscutit din asiguratB4). CAS 25% se aplică TUTUROR codurilor (Cod fiscal art.
/// 139 (1) lit. o; B4_7 = bază lucrată + ΣB3_7). Impozit: indemnizația de incapacitate temporară (cod
/// 01 ș.a.) e impozabilă (10%); NEIMPOZABILE sunt indemnizațiile de maternitate (08), creșterea/
/// îngrijirea copilului (09/91/92) și risc maternal (15) — Cod fiscal art. 62 lit. c). Implicit =
/// impozabil.
pub fn cm_indemn_treatment(cod_indemn: &str) -> (bool, bool) {
    let cass_due = matches!(cod_indemn, "01" | "07" | "10");
    // Coduri de indemnizație NEIMPOZABILE (maternitate / îngrijire copil / risc maternal). NU includ
    // codul 16 (incapacitate temporară) — acela e impozabil.
    let tax_exempt = matches!(cod_indemn, "08" | "09" | "15" | "91" | "92");
    (cass_due, !tax_exempt)
}

/// Calcul salarial cu concediu medical (OUG 158/2005). Salariul de bază se proratează la zilele
/// lucrate (worked_days/working_days). Indemnizația de CM e supusă CAS 25% + CASS 10% (doar codurile
/// `cass_due`) — confirmat de reconcilierea B4 din structura D112 v7 (B4_7 = baza lucrată + indemnizație,
/// B4_8 = ROUND(B4_7×25%)) — dar NU CAM (baza CAM = doar partea lucrată). Impozitul 10% se aplică pe
/// (venit lucrat + indemnizație impozabilă) − contribuții − deducere − sumă netaxabilă.
///
/// Proprietate de consistență: cu `certs` gol, rezultatul coincide cu [`compute_payroll`] (lună întreagă
/// lucrată) — vezi testul `leave_empty_certs_equals_compute_payroll`.
pub fn compute_payroll_with_leave(input: &LeavePayrollInput) -> LeavePayrollResult {
    let z = Decimal::ZERO;
    let wd = input.working_days.max(1);
    let leave_days: u32 = input.certs.iter().map(|c| c.leave_working_days).sum();
    let worked_days = wd.saturating_sub(leave_days);
    // Salariul lucrat = salariul de bază × zile lucrate / zile lucrătoare, rotunjit la LEU ÎNTREG
    // (contribuțiile RO se declară în lei întregi; păstrează D112 = GL exact, ca pe calea fără concediu).
    let worked_gross = (input.gross.max(z) * Decimal::from(worked_days) / Decimal::from(wd))
        .round_dp_with_strategy(0, RoundingStrategy::MidpointAwayFromZero);
    let non_taxable = input.non_taxable.max(z).min(worked_gross);
    let worked_base = (worked_gross - non_taxable).max(z);

    let indemn_employer: Decimal = input.certs.iter().map(|c| c.indemn_employer.max(z)).sum();
    let indemn_fnuass: Decimal = input.certs.iter().map(|c| c.indemn_fnuass.max(z)).sum();
    let indemn_total = indemn_employer + indemn_fnuass;
    let indemn_cass_base: Decimal = input
        .certs
        .iter()
        .filter(|c| c.cass_due)
        .map(|c| (c.indemn_employer + c.indemn_fnuass).max(z))
        .sum();
    let indemn_taxable: Decimal = input
        .certs
        .iter()
        .filter(|c| c.taxable)
        .map(|c| (c.indemn_employer + c.indemn_fnuass).max(z))
        .sum();

    // CAS 25% pe (bază lucrată + toată indemnizația); CASS 10% pe (bază lucrată + indemnizația ne-scutită);
    // CAM 2,25% DOAR pe baza lucrată.
    let cas = pct(worked_base + indemn_total, CAS_PCT);
    let cass = pct(worked_base + indemn_cass_base, CASS_PCT);
    let cam = pct(worked_base, CAM_PCT);
    let concedii = pct(worked_base, CONCEDII_PCT); // CCI 0,85% pe baza lucrată (ca și CAM)

    // Impozit 10% pe (venit lucrat + indemnizație impozabilă) − CAS − CASS − deducere − sumă netaxabilă.
    let after_contrib = (worked_gross + indemn_taxable) - cas - cass;
    let deduction = input.personal_deduction.max(z).min(after_contrib.max(z));
    let taxable_base = (after_contrib - deduction - non_taxable).max(z);
    let income_tax = pct(taxable_base, INCOME_TAX_PCT);
    let net = (worked_gross + indemn_total) - cas - cass - income_tax;

    LeavePayrollResult {
        worked_days,
        worked_gross,
        worked_base,
        indemn_employer,
        indemn_fnuass,
        indemn_total,
        cas,
        cass,
        cam,
        concedii,
        income_tax,
        taxable_base,
        net,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    fn d(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    #[test]
    fn part_time_min_base_full_minimum_not_prorated() {
        // Part-time P1, gross 3.000, H1 (month 3): baza = salariul minim ÎNTREG 3.750 (NU prorata).
        // cas_diff = 938 − 750 = 188 (pct(3750,25%)=937.5→938); cass_diff = 375 − 300 = 75.
        let r = part_time_min_base(d("3000"), "P1", false, 2026, 3);
        assert_eq!(r, Some((d("3750"), d("188"), d("75"))));
        // H2 (month 8): baza 4.125.
        assert_eq!(
            part_time_min_base(d("3000"), "P1", false, 2026, 8)
                .unwrap()
                .0,
            d("4125")
        );
        // Full-time N → fără majorare.
        assert_eq!(part_time_min_base(d("3000"), "N", false, 2026, 3), None);
        // Exceptat (art. 146 (5^7)) → baza rămâne venitul realizat.
        assert_eq!(part_time_min_base(d("3000"), "P1", true, 2026, 3), None);
        // Venit ≥ baza minimă → fără majorare.
        assert_eq!(part_time_min_base(d("4000"), "P1", false, 2026, 3), None);
    }

    #[test]
    fn art146_5_7_exemption_categories() {
        // Pensionar (lit. d) + cele 4 categorii cu cod → exceptat; restul → neexceptat.
        assert!(exempt_part_time_min_base(true, ""));
        assert!(exempt_part_time_min_base(false, "elev_student")); // lit. a
        assert!(exempt_part_time_min_base(false, "ucenic")); // lit. b
        assert!(exempt_part_time_min_base(false, "dizabilitate")); // lit. c
        assert!(exempt_part_time_min_base(false, "contracte_multiple")); // lit. e
        assert!(!exempt_part_time_min_base(false, ""));
        assert!(!exempt_part_time_min_base(false, "altceva"));
    }

    #[test]
    fn payroll_2026_rates_gross_to_net() {
        // Gross 5.000, no personal deduction.
        // CAS 25% = 1.250; CASS 10% = 500; base = 5.000 − 1.250 − 500 = 3.250; impozit 10% = 325.
        // Net = 5.000 − 1.250 − 500 − 325 = 2.925. CAM 2,25% = 113; CCI 0,85% = 43 (5000 × 0.0085 =
        // 42.5 → 43). Cost angajator = 5.000 + 113 + 43 = 5.156.
        let r = compute_payroll(&PayrollInput {
            gross: d("5000"),
            personal_deduction: d("0"),
            non_taxable: d("0"),
        });
        assert_eq!(r.cas, "1250.00");
        assert_eq!(r.cass, "500.00");
        assert_eq!(r.taxable_base, "3250.00");
        assert_eq!(r.income_tax, "325.00");
        assert_eq!(r.net, "2925.00");
        assert_eq!(r.cam, "113.00"); // 5000 × 0.0225 = 112.5 → 113
        assert_eq!(r.concedii, "43.00"); // CCI 0,85%: 5000 × 0.0085 = 42.5 → 43
        assert_eq!(r.total_employer_cost, "5156.00");
    }

    #[test]
    fn concedii_cci_rate_single_source_of_truth() {
        // Drift guard: rata CCI = 0,85% (OUG 158/2005 art. 4(2)), separată de CAM.
        assert_eq!(CONCEDII_PCT, (85, 4));
        // Carve-out exempt de CASS ⇒ și de CCI: brut 4.050 cu 300 netaxabil ⇒ baza 3.750.
        let r = compute_payroll(&PayrollInput {
            gross: d("4050"),
            personal_deduction: d("0"),
            non_taxable: d("300"),
        });
        assert_eq!(r.concedii, "32.00"); // 3750 × 0.0085 = 31.875 → 32
    }

    #[test]
    fn leave_empty_certs_equals_compute_payroll() {
        // Proprietate de regresie: fără concediu, calculul cu concediu == compute_payroll (lună întreagă).
        let base = compute_payroll(&PayrollInput {
            gross: d("5000"),
            personal_deduction: d("150"),
            non_taxable: d("0"),
        });
        let lv = compute_payroll_with_leave(&LeavePayrollInput {
            gross: d("5000"),
            personal_deduction: d("150"),
            non_taxable: d("0"),
            working_days: 21,
            certs: vec![],
        });
        assert_eq!(lv.worked_days, 21);
        assert_eq!(fmt(lv.worked_gross), base.gross);
        assert_eq!(fmt(lv.cas), base.cas);
        assert_eq!(fmt(lv.cass), base.cass);
        assert_eq!(fmt(lv.cam), base.cam);
        assert_eq!(fmt(lv.income_tax), base.income_tax);
        assert_eq!(fmt(lv.net), base.net);
        assert_eq!(fmt(lv.taxable_base), base.taxable_base);
    }

    #[test]
    fn leave_common_illness_prorates_and_taxes_indemnity() {
        // Brut 5.250, 21 zile lucrătoare, 5 zile concediu boală obișnuită (cod 01) ⇒ 16 zile lucrate,
        // brut lucrat = 5.250 × 16/21 = 4.000. Indemnizație 508 (suportată de angajator). CAS 25% +
        // CASS 10% pe (4.000 + 508) = 4.508; impozit 10%. Numerele coincid cu fixtura emitterului D112.
        let (cass_due, taxable) = cm_indemn_treatment("01");
        assert!(cass_due && taxable);
        let r = compute_payroll_with_leave(&LeavePayrollInput {
            gross: d("5250"),
            personal_deduction: d("0"),
            non_taxable: d("0"),
            working_days: 21,
            certs: vec![LeaveCert {
                indemn_employer: d("508"),
                indemn_fnuass: d("0"),
                leave_working_days: 5,
                cass_due,
                taxable,
            }],
        });
        assert_eq!(r.worked_days, 16);
        assert_eq!(fmt(r.worked_gross), "4000.00");
        assert_eq!(fmt(r.worked_base), "4000.00");
        assert_eq!(fmt(r.cas), "1127.00"); // round(4508 × 25%)
        assert_eq!(fmt(r.cass), "451.00"); // round(4508 × 10%) = 450.8 → 451
        assert_eq!(fmt(r.cam), "90.00"); // round(4000 × 2.25%)
        assert_eq!(fmt(r.income_tax), "293.00"); // round(2930 × 10%)
        assert_eq!(fmt(r.taxable_base), "2930.00");
        assert_eq!(fmt(r.net), "2637.00"); // 4508 − 1127 − 451 − 293
    }

    #[test]
    fn cm_treatment_per_code() {
        assert_eq!(cm_indemn_treatment("01"), (true, true)); // boală obișnuită: CASS + impozit
        assert_eq!(cm_indemn_treatment("07"), (true, true)); // carantină
        assert_eq!(cm_indemn_treatment("10"), (true, true)); // reducere temporară activitate
        assert_eq!(cm_indemn_treatment("08"), (false, false)); // sarcină/lăuzie: scutit ambele
        assert_eq!(cm_indemn_treatment("09"), (false, false)); // îngrijire copil bolnav: scutit
        assert_eq!(cm_indemn_treatment("15"), (false, false)); // risc maternal: scutit
        assert_eq!(cm_indemn_treatment("16"), (false, true)); // incapacitate: fără CASS, dar impozabil
    }

    #[test]
    fn leave_maternity_indemnity_untaxed_and_cass_exempt() {
        // Concediu maternitate (cod 08) toată luna: indemnizația NU e impozabilă și NU e supusă CASS;
        // CAS 25% se aplică totuși (B4_7 include indemnizația). Fără zile lucrate ⇒ brut lucrat 0.
        let (cass_due, taxable) = cm_indemn_treatment("08");
        let r = compute_payroll_with_leave(&LeavePayrollInput {
            gross: d("5000"),
            personal_deduction: d("0"),
            non_taxable: d("0"),
            working_days: 21,
            certs: vec![LeaveCert {
                indemn_employer: d("4000"),
                indemn_fnuass: d("0"),
                leave_working_days: 21,
                cass_due,
                taxable,
            }],
        });
        assert_eq!(r.worked_days, 0);
        assert_eq!(fmt(r.worked_gross), "0.00");
        assert_eq!(fmt(r.cas), "1000.00"); // 25% × 4000 (CAS se aplică pe indemnizație)
        assert_eq!(fmt(r.cass), "0.00"); // scutit de CASS
        assert_eq!(fmt(r.cam), "0.00"); // fără parte lucrată
        assert_eq!(fmt(r.income_tax), "0.00"); // neimpozabil
    }

    #[test]
    fn suma_netaxabila_gating() {
        // Sem. I (≤6): 300 lei for a full-time beneficiary; sem. II (≥7): 200 lei.
        assert_eq!(suma_netaxabila(true, "N", d("4050"), 2026, 3), d("300"));
        assert_eq!(suma_netaxabila(true, "N", d("4325"), 2026, 8), d("200"));
        // Not a beneficiary → 0.
        assert_eq!(suma_netaxabila(false, "N", d("4050"), 2026, 3), d("0"));
        // Part-time (Pi) → 0 (measure is full-time only).
        assert_eq!(suma_netaxabila(true, "P1", d("4050"), 2026, 3), d("0"));
        // Exactly AT the ceiling is INCLUSIVE (≤ 4.300 H1 / 4.600 H2) — boundary lock (TEST-01).
        assert_eq!(suma_netaxabila(true, "N", d("4300"), 2026, 3), d("300"));
        assert_eq!(suma_netaxabila(true, "N", d("4600"), 2026, 8), d("200"));
        // Just OVER the ceiling → whole benefit lost.
        assert_eq!(suma_netaxabila(true, "N", d("4301"), 2026, 3), d("0"));
        assert_eq!(suma_netaxabila(true, "N", d("4500"), 2026, 8), d("200")); // 4500 ≤ 4600 H2
        assert_eq!(suma_netaxabila(true, "N", d("4601"), 2026, 8), d("0"));
    }

    #[test]
    fn min_wage_single_source_of_truth_and_drift_guard() {
        // Wave D: one source for the min wage + carve-out, keyed on (year, month).
        assert_eq!(min_wage_lei(2026, 3), d("4050"));
        assert_eq!(min_wage_lei(2026, 7), d("4325"));
        assert_eq!(carve_out_lei(2026, 3), d("300"));
        assert_eq!(carve_out_lei(2026, 8), d("200"));
        // part_time_min_base now DERIVES 3.750 / 4.125 from min_wage − carve_out (not magic numbers).
        assert_eq!(
            part_time_min_base(d("1000"), "P1", false, 2026, 3)
                .unwrap()
                .0,
            d("3750")
        );
        assert_eq!(
            part_time_min_base(d("1000"), "P1", false, 2026, 8)
                .unwrap()
                .0,
            d("4125")
        );
        // Drift guard: an uncovered future year falls back to the last known value (+ a tracing::warn)
        // instead of silently mis-deriving — the reminder to add the next HG row.
        assert_eq!(min_wage_lei(2027, 3), d("4325"));
    }

    #[test]
    fn cam_stays_on_realized_gross_for_part_time_min_base() {
        // CALC-01/02 lock: for a part-time employee whose CAS/CASS base is lifted to the minimum
        // (art. 146 (5^6)), CAM is NOT lifted — it stays on the REALIZED gross (art. 220^6). The
        // helper returns only (base, cas_diff, cass_diff) — no CAM component — and CAM = pct(gross).
        let (base, cas_diff, cass_diff) =
            part_time_min_base(d("2000"), "P1", false, 2026, 3).unwrap();
        assert_eq!(base, d("3750")); // CAS/CASS base lifted to 3.750 (H1)
        assert!(cas_diff > d("0") && cass_diff > d("0")); // employer bears the CAS/CASS top-up
                                                          // CAM is computed on the realized 2.000 (= 45), NOT on the lifted 3.750 (which would be 84).
        let r = compute_payroll(&PayrollInput {
            gross: d("2000"),
            personal_deduction: d("0"),
            non_taxable: d("0"), // carve-out is full-time-only; part-timers get 0
        });
        assert_eq!(r.cam, "45.00"); // 2000 × 2.25% = 45 — on realized gross, not the minimum base
    }

    #[test]
    fn carveout_reduces_all_four_levies() {
        // Full-time min-wage beneficiary, H1: gross 4.050, carve-out 300 → base 3.750.
        // CAS 25%·3750 = 938 (937.5→938); CASS 10%·3750 = 375; CAM 2.25%·3750 = 84 (84.375→84).
        // venit net = 4050 − 938 − 375 = 2737; with deducere 807: base = 2737 − 807 − 300 = 1630;
        // impozit 10% = 163; net = 2737 − 163 = 2574.
        let r = compute_payroll(&PayrollInput {
            gross: d("4050"),
            personal_deduction: d("807"),
            non_taxable: d("300"),
        });
        assert_eq!(r.non_taxable, "300.00");
        assert_eq!(r.cas, "938.00");
        assert_eq!(r.cass, "375.00");
        assert_eq!(r.cam, "84.00"); // on the reduced base 3.750, NOT 4.050
        assert_eq!(r.taxable_base, "1630.00");
        assert_eq!(r.income_tax, "163.00");
        assert_eq!(r.net, "2574.00");
        // Same gross WITHOUT the carve-out over-declares: CAS on full 4.050 = 1013 (> 938).
        let no = compute_payroll(&PayrollInput {
            gross: d("4050"),
            personal_deduction: d("807"),
            non_taxable: d("0"),
        });
        // Without the carve-out CAS is on the full 4.050 (1013 > 938) and tax is higher (183 > 163) —
        // i.e. the missing carve-out OVER-declares. (Compare numerically, not as strings.)
        assert_eq!(no.cas, "1013.00");
        assert_eq!(no.income_tax, "183.00");
    }

    #[test]
    fn personal_deduction_reduces_the_income_tax_base() {
        // Gross 4.050 (min wage H1), deduction 700.
        // CAS 1.013 (4050×0.25=1012.5→1013); CASS 405; after = 4050−1013−405 = 2632.
        // base = 2632 − 700 = 1932; impozit 10% = 193. Net = 2632 − 193 = 2439.
        let r = compute_payroll(&PayrollInput {
            gross: d("4050"),
            personal_deduction: d("700"),
            non_taxable: d("0"),
        });
        assert_eq!(r.cas, "1013.00");
        assert_eq!(r.cass, "405.00");
        assert_eq!(r.taxable_base, "1932.00");
        assert_eq!(r.income_tax, "193.00");
        assert_eq!(r.net, "2439.00");
    }
}
