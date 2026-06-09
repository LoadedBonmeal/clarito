//! Bilanț — official ANAF XML export (S1005 «UU» micro / S1003 «BS» small), OMFP 1802/2014.
//!
//! Generates a `<Bilant1005>` / `<Bilant1003>` XML carrying the GL-derived F10 (balance sheet)
//! and F20 (P&L) blocks, which the accountant imports into ANAF's PDF inteligent ("Import fișier
//! XML generat cu alte aplicații") and there completes the F30 «Date informative» + the header
//! fields the app doesn't hold (tax-office codes, preparer, audit flags) before signing.
//!
//! Field codes are verbatim from the published XSD (s1005/s1003): `F10_<rrr><c>` / `F20_<rrr><c>`
//! where `rrr` = 3-digit official row and `c` = column (1 = sold la începutul exercițiului, 2 = la
//! sfârșit; for F20, 1 = exercițiul precedent, 2 = curent). VALUES ARE WHOLE LEI (IntPoz/Neg15).
//!
//! Scope: the principal F10/F20 rows are mapped from the trial balance + P&L; rows for items a
//! typical micro/small company doesn't have are emitted as 0. The exhaustive sub-line breakdown
//! and F30 are completed in the ANAF app. This is the integration the bilanț filing path expects.

use crate::db::gl::{ProfitLoss, TrialBalance};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use std::collections::HashMap;

/// Round a Decimal RON value to whole lei (i64), commercial rounding.
fn lei(d: Decimal) -> i64 {
    d.round_dp_with_strategy(0, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
        .to_i64()
        .unwrap_or(0)
}

/// Net (debit-positive) balance of an account at the START of the period (opening) and END
/// (closing) from a trial-balance row. Returns (opening_net_debit, closing_net_debit).
fn net(tb: &TrialBalance, code_pred: impl Fn(&str) -> bool) -> (Decimal, Decimal) {
    let p = |s: &str| s.parse::<Decimal>().unwrap_or(Decimal::ZERO);
    let mut open = Decimal::ZERO;
    let mut close = Decimal::ZERO;
    for r in &tb.rows {
        if code_pred(&r.account_code) {
            open += p(&r.opening_debit) - p(&r.opening_credit);
            close += p(&r.closing_debit) - p(&r.closing_credit);
        }
    }
    (open, close)
}

/// Map the trial balance to the F10 (balance-sheet) field → whole-lei value. Both columns
/// (1 = opening, 2 = closing). Assets are debit-positive; equity/liabilities credit-positive.
pub fn compute_f10(tb: &TrialBalance) -> HashMap<String, i64> {
    let mut f = HashMap::new();
    // Asset rows: value = net debit. starts_with helpers.
    let sw = |code: &str, p: &str| code.starts_with(p);
    let mut put = |row: &str, open: Decimal, close: Decimal, credit_side: bool| {
        let (o, c) = if credit_side {
            (-open, -close)
        } else {
            (open, close)
        };
        f.insert(format!("F10_{row}1"), lei(o));
        f.insert(format!("F10_{row}2"), lei(c));
    };

    // rd.01 Imobilizări necorporale = 20x − 280 − 290 (+ 4094). rd.02 corporale = 21x/23x −281−291.
    let (o1, c1) = net(tb, |x| {
        (sw(x, "20") || x == "4094") && !sw(x, "280") && !sw(x, "290")
            || sw(x, "280")
            || sw(x, "290")
    });
    put("001", o1, c1, false);
    let (o2, c2) = net(tb, |x| {
        sw(x, "21") || sw(x, "23") || sw(x, "281") || sw(x, "291")
    });
    put("002", o2, c2, false);
    // rd.03 Imobilizări financiare = 26x/267 − 296.
    let (o3, c3) = net(tb, |x| sw(x, "26") || sw(x, "267") || sw(x, "296"));
    put("003", o3, c3, false);
    // rd.04 total imobilizate.
    put("004", o1 + o2 + o3, c1 + c2 + c3, false);

    // rd.05 Stocuri = class 3 − 39x. rd.06 Creanțe = class-4 debit. rd.07 Investiții = 50x/59x.
    // rd.08 Casa = 51x/53x/54x debit (excl. 50x). All asset side (debit-positive).
    let (o5, c5) = net(tb, |x| sw(x, "3"));
    put("005", o5, c5, false);
    let (o6, c6) = class4_debit(tb);
    put("006", o6, c6, false);
    let (o7, c7) = net(tb, |x| sw(x, "50") || sw(x, "59"));
    put("007", o7, c7, false);
    // rd.08 Casa și conturi la bănci = 511x/512x (bank) + 53x (cash) + 54x (treasury).
    let (o8, c8) = net(tb, |x| {
        sw(x, "511") || sw(x, "512") || sw(x, "53") || sw(x, "54")
    });
    put("008", o8, c8, false);
    put("009", o5 + o6 + o7 + o8, c5 + c6 + c7 + c8, false);

    // rd.10 Cheltuieli în avans = 471.
    let (o10, c10) = net(tb, |x| x == "471" || sw(x, "476"));
    put("010", o10, c10, false);

    // rd.13 Datorii ≤1 an + rd.16 >1 an (no maturity split → all in ≤1 an): class-4 credit + 16x
    // + 519. rd.17 Provizioane = 15x. rd.18 Venituri în avans = 472/475.
    let (o13, c13) = current_liabilities(tb);
    put("013", o13, c13, true); // credit side
    let (o16, c16) = net(tb, |x| sw(x, "16"));
    put("016", o16, c16, true);
    let (o17, c17) = net(tb, |x| sw(x, "15"));
    put("017", o17, c17, true);
    let (o18, c18) = net(tb, |x| x == "472" || sw(x, "475"));
    put("018", o18, c18, true);

    // J. Capitaluri proprii: rd.26 Capital vărsat (1012), rd.32 Rezerve (106), rd.38 Rezultat
    // reportat (117), rd.42 Rezultat exercițiu (121), rd.49 TOTAL capitaluri proprii.
    let (o_cap, c_cap) = net(tb, |x| sw(x, "101"));
    put("026", o_cap, c_cap, true);
    let (o_rez, c_rez) = net(tb, |x| sw(x, "106") || sw(x, "104") || sw(x, "105"));
    put("032", o_rez, c_rez, true);
    let (o_rep, c_rep) = net(tb, |x| sw(x, "117"));
    put("038", o_rep, c_rep, true);
    // Rezultatul exercițiului (credit-positive) = 121 credit balance + the not-yet-closed 6/7
    // result (revenue − expense). `net()` is debit-positive, so credit/result = its negation.
    let (o_121, c_121) = net(tb, |x| x == "121");
    let (o_67, c_67) = net(tb, |x| sw(x, "7") || sw(x, "6"));
    let result_open = (-o_121) + (-o_67);
    let result_close = (-c_121) + (-c_67);
    f.insert("F10_0421".into(), lei(result_open));
    f.insert("F10_0422".into(), lei(result_close));
    // rd.49 total capitaluri proprii = capital + rezerve + reportat + rezultat.
    let cap_open = (-o_cap) + (-o_rez) + (-o_rep) + result_open;
    let cap_close = (-c_cap) + (-c_rez) + (-c_rep) + result_close;
    f.insert("F10_0491".into(), lei(cap_open));
    f.insert("F10_0492".into(), lei(cap_close));

    // F10_0011 (rd.01 opening, imobilizări necorporale) is IntPoz15 (≥0) in the schema — clamp a
    // negative (over-amortized intangible at opening) to 0 so the XML validates.
    if let Some(v) = f.get_mut("F10_0011") {
        if *v < 0 {
            *v = 0;
        }
    }

    f
}

/// "Settlement" accounts whose debit balance is a creanță and credit balance a datorie:
/// class-4 (excl. avans 471/472/475) + interest 518x + short-term bank credit 519x (incl. their
/// 4-digit analytics 5191/5198 — the common overdraft accounts that must not be dropped).
fn is_settlement(code: &str) -> bool {
    (code.starts_with('4') && code != "471" && code != "472" && !code.starts_with("475"))
        || code.starts_with("518")
        || code.starts_with("519")
}

/// Settlement accounts with a DEBIT balance → creanțe (rd.06), per column.
fn class4_debit(tb: &TrialBalance) -> (Decimal, Decimal) {
    let p = |s: &str| s.parse::<Decimal>().unwrap_or(Decimal::ZERO);
    let (mut o, mut c) = (Decimal::ZERO, Decimal::ZERO);
    for r in &tb.rows {
        if is_settlement(&r.account_code) {
            let od = p(&r.opening_debit) - p(&r.opening_credit);
            let cd = p(&r.closing_debit) - p(&r.closing_credit);
            if od > Decimal::ZERO {
                o += od;
            }
            if cd > Decimal::ZERO {
                c += cd;
            }
        }
    }
    (o, c)
}

/// Settlement accounts with a CREDIT balance → datorii curente (rd.13), per column.
fn current_liabilities(tb: &TrialBalance) -> (Decimal, Decimal) {
    let p = |s: &str| s.parse::<Decimal>().unwrap_or(Decimal::ZERO);
    let (mut o, mut c) = (Decimal::ZERO, Decimal::ZERO);
    for r in &tb.rows {
        if is_settlement(&r.account_code) {
            let oc = p(&r.opening_credit) - p(&r.opening_debit);
            let cc = p(&r.closing_credit) - p(&r.closing_debit);
            if oc > Decimal::ZERO {
                o += oc;
            }
            if cc > Decimal::ZERO {
                c += cc;
            }
        }
    }
    // Stored as credit-positive; the caller's `put(.., true)` negates net-debit, so pass as
    // net-debit = -credit to round-trip correctly.
    (-o, -c)
}

/// Map the P&L to the F20 fields (col 2 = current year, col 1 = prior). For the MICRO form (UU,
/// s1005) the simplified 9-row layout (cifra de afaceri rd.01, venituri totale rd.07, cheltuieli
/// totale rd.08, rezultat rd.09). For the SMALL form (BS, s1003) the row codes differ (full 70-row
/// F20), so we emit only the confirmed cifra de afaceri (F20_001) — the full P&L is completed in
/// the ANAF app after import.
pub fn compute_f20(
    pnl: &ProfitLoss,
    prior: Option<&ProfitLoss>,
    micro: bool,
) -> HashMap<String, i64> {
    let p = |s: &str| s.parse::<Decimal>().unwrap_or(Decimal::ZERO);
    let mut f = HashMap::new();
    let mut col = |row: &str, cur: Decimal, pri: Decimal| {
        f.insert(format!("F20_{row}1"), lei(pri));
        f.insert(format!("F20_{row}2"), lei(cur));
    };
    let prv = |get: &dyn Fn(&ProfitLoss) -> Decimal| prior.map(get).unwrap_or(Decimal::ZERO);
    let op_rev = |x: &ProfitLoss| p(&x.operating_revenue);

    col("001", op_rev(pnl), prv(&op_rev)); // cifra de afaceri netă (rd.01) — same code in both forms
    if micro {
        let tot_rev = |x: &ProfitLoss| p(&x.total_revenue);
        let tot_exp = |x: &ProfitLoss| p(&x.total_expense);
        let tax = |x: &ProfitLoss| p(&x.income_tax);
        let net_res = |x: &ProfitLoss| p(&x.net_result);
        col("007", tot_rev(pnl), prv(&tot_rev)); // venituri totale (micro rd.07)
        col(
            "008",
            tot_exp(pnl) + tax(pnl),
            prv(&|x| tot_exp(x) + tax(x)),
        ); // cheltuieli totale
        col("009", net_res(pnl), prv(&net_res)); // rezultat net (micro rd.09)
    }
    f
}

/// XML-escape (attribute value).
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

/// Map a 2-letter county auto-code to its ANAF județ code (Int_listaCodJud, 1-42; 40 = București).
pub fn county_code(county: &str) -> u8 {
    match county.trim().to_uppercase().as_str() {
        "AB" => 1,
        "AR" => 2,
        "AG" => 3,
        "BC" => 4,
        "BH" => 5,
        "BN" => 6,
        "BT" => 7,
        "BV" => 8,
        "BR" => 9,
        "BZ" => 10,
        "CS" => 11,
        "CJ" => 12,
        "CT" => 13,
        "CV" => 14,
        "DB" => 15,
        "DJ" => 16,
        "GL" => 17,
        "GJ" => 18,
        "HR" => 19,
        "HD" => 20,
        "IL" => 21,
        "IS" => 22,
        "IF" => 23,
        "MM" => 24,
        "MH" => 25,
        "MS" => 26,
        "NT" => 27,
        "OT" => 28,
        "PH" => 29,
        "SM" => 30,
        "SJ" => 31,
        "SB" => 32,
        "SV" => 33,
        "TR" => 34,
        "TM" => 35,
        "TL" => 36,
        "VS" => 37,
        "VL" => 38,
        "VN" => 39,
        "B" => 40,
        "CL" => 41,
        "GR" => 42,
        _ => 40, // default București
    }
}

/// Header values the generator needs. The CAEN must be a valid code (required by the XSD); the
/// tax-office sub-code (codJJ) and ownership form (codPP) default to common values the accountant
/// verifies in the ANAF app after import; F30 «Date informative» is completed there.
pub struct BilantHeader {
    pub year: i32,
    pub cui: String,
    pub den: String,
    pub adresa: String,
    pub reg_com: String,
    /// Valid 4-digit CAEN code (Str_coduriCaen) — required by the schema.
    pub caen: String,
    /// 2-letter county auto-code (mapped to codTT).
    pub county: String,
    pub nume_admin: String,
}

/// Build the bilanț XML for the entity-size `form`: "UU" → `<Bilant1005>` (s1005:v14,
/// microîntreprindere); "BS" → `<Bilant1003>` (s1003:v15, entitate mică); "BL" → `<Bilant1002>`
/// (s1002:v15, entitate mare/mijlocie). UU + BS share the prescurtat F10 (rd.1-49); the developed
/// F10 (rd.1-103) of the BL form is a different row layout — its F10 is completed in the ANAF app.
pub fn generate_bilant_xml(
    h: &BilantHeader,
    f10: &HashMap<String, i64>,
    f20: &HashMap<String, i64>,
    form: &str,
) -> String {
    let total_plata = f10.get("F10_0492").copied().unwrap_or(0); // control sum = total capitaluri.
    let cod_tt = county_code(&h.county);
    let an_caen = 2025; // Str_coduriCaen2024_2025 / IntInt2024_2025.
                        // bifa_art27 (IntInt0_0, must be 0) is required by ALL three schemas (s1005/s1003/s1002), so it
                        // is emitted unconditionally below.
    let (root, ns, tip) = match form {
        "BL" => ("Bilant1002", "mfp:anaf:dgti:s1002:declaratie:v15", "BL"),
        "BS" => ("Bilant1003", "mfp:anaf:dgti:s1003:declaratie:v15", "BS"),
        _ => ("Bilant1005", "mfp:anaf:dgti:s1005:declaratie:v14", "UU"),
    };

    let attrs = |m: &HashMap<String, i64>| {
        let mut keys: Vec<_> = m.keys().cloned().collect();
        keys.sort();
        keys.into_iter()
            .map(|k| format!("{k}=\"{}\"", m[&k]))
            .collect::<Vec<_>>()
            .join(" ")
    };

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<{root} xmlns=\"{ns}\" \
luna=\"12\" an=\"{an}\" cui=\"{cui}\" den=\"{den}\" adresa=\"{adr}\" \
caen=\"{caen}\" caenE=\"{caen}\" AN_CAEN=\"{an_caen}\" regCom=\"{reg}\" \
bifa_aprob=\"1\" bifaMC=\"1\" bifaDD=\"0\" bifaGG=\"0\" bifaAA=\"0\" bifa_art27=\"0\" \
tipBIL=\"{tip}\" interes_public=\"0\" codTT=\"{tt}\" codJJ=\"1\" codPP=\"11\" \
nume_admin=\"{adm}\" nume_intocmit=\"{adm}\" calit_intocmit=\"11\" \
totalPlata_A=\"{tp}\">\n\
  <F10 {f10}/>\n\
  <F20 {f20}/>\n\
  <F30/>\n\
</{root}>\n",
        root = root,
        ns = ns,
        tip = tip,
        an = h.year,
        an_caen = an_caen,
        cui = esc(&h.cui),
        den = esc(&h.den),
        adr = esc(&h.adresa),
        caen = esc(&h.caen),
        reg = esc(&h.reg_com),
        tt = cod_tt,
        adm = esc(&h.nume_admin),
        tp = total_plata,
        f10 = attrs(f10),
        f20 = attrs(f20),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tb_row(
        code: &str,
        od: &str,
        oc: &str,
        cd: &str,
        cc: &str,
    ) -> crate::db::gl::TrialBalanceRow {
        crate::db::gl::TrialBalanceRow {
            account_code: code.into(),
            account_name: code.into(),
            opening_debit: od.into(),
            opening_credit: oc.into(),
            period_debit: "0".into(),
            period_credit: "0".into(),
            total_debit: "0".into(),
            total_credit: "0".into(),
            closing_debit: cd.into(),
            closing_credit: cc.into(),
        }
    }

    fn tb(rows: Vec<crate::db::gl::TrialBalanceRow>) -> TrialBalance {
        TrialBalance {
            rows,
            total_opening_debit: "0".into(),
            total_opening_credit: "0".into(),
            total_period_debit: "0".into(),
            total_period_credit: "0".into(),
            total_total_debit: "0".into(),
            total_total_credit: "0".into(),
            total_closing_debit: "0".into(),
            total_closing_credit: "0".into(),
            balanced: true,
        }
    }

    #[test]
    fn f10_maps_principal_rows_in_whole_lei() {
        let t = tb(vec![
            tb_row("2131", "0", "0", "50000", "0"),
            tb_row("2813", "0", "0", "0", "10000"),
            tb_row("371", "0", "0", "20000", "0"),
            tb_row("4111", "0", "0", "30000", "0"),
            tb_row("5121", "0", "0", "15000", "0"),
            tb_row("101", "0", "0", "0", "50000"),
            tb_row("401", "0", "0", "0", "25000"),
            tb_row("121", "0", "0", "0", "40000"),
        ]);
        let f = compute_f10(&t);
        assert_eq!(f["F10_0022"], 40000); // imobilizări corporale net 50.000 − 10.000
        assert_eq!(f["F10_0052"], 20000); // stocuri
        assert_eq!(f["F10_0062"], 30000); // creanțe
        assert_eq!(f["F10_0082"], 15000); // casa
        assert_eq!(f["F10_0132"], 25000); // datorii curente (401)
                                          // total capitaluri proprii = 50.000 capital + 40.000 rezultat = 90.000.
        assert_eq!(f["F10_0492"], 90000);
    }

    #[test]
    fn xml_has_root_and_blocks() {
        let h = BilantHeader {
            year: 2026,
            cui: "12345678".into(),
            den: "Test SRL".into(),
            adresa: "Str 1".into(),
            reg_com: "J40/1/2020".into(),
            caen: "6201".into(),
            county: "CJ".into(),
            nume_admin: "Ion Popescu".into(),
        };
        let mut f10 = HashMap::new();
        f10.insert("F10_0492".to_string(), 90000i64);
        let f20 = HashMap::new();
        let xml = generate_bilant_xml(&h, &f10, &f20, "UU");
        assert!(xml.contains("<Bilant1005"));
        assert!(xml.contains("mfp:anaf:dgti:s1005:declaratie:v14"));
        assert!(xml.contains("tipBIL=\"UU\""));
        assert!(xml.contains("bifa_aprob=\"1\"")); // IntInt1_1 — must be exactly 1 (XSD-validated)
        assert!(xml.contains("bifa_art27=\"0\""));
        assert!(xml.contains("calit_intocmit=\"11\""));
        assert!(xml.contains("codTT=\"12\"")); // CJ → 12
        assert!(xml.contains("caen=\"6201\""));
        assert!(xml.contains("AN_CAEN=\"2025\""));
        assert!(xml.contains("totalPlata_A=\"90000\""));
        assert!(xml.contains("F10_0492=\"90000\""));

        // Small-entity form → Bilant1003 / BS / s1003 namespace; bifa_art27 required (IntInt0_0).
        let bs = generate_bilant_xml(&h, &f10, &f20, "BS");
        assert!(bs.contains("<Bilant1003"));
        assert!(bs.contains("mfp:anaf:dgti:s1003:declaratie:v15"));
        assert!(bs.contains("tipBIL=\"BS\""));
        assert!(bs.contains("bifa_art27=\"0\""));

        // Large-entity form → Bilant1002 / BL / s1002 namespace; bifa_art27 required.
        let bl = generate_bilant_xml(&h, &f10, &f20, "BL");
        assert!(bl.contains("<Bilant1002"));
        assert!(bl.contains("mfp:anaf:dgti:s1002:declaratie:v15"));
        assert!(bl.contains("tipBIL=\"BL\""));
        assert!(bl.contains("bifa_art27=\"0\""));
    }

    #[test]
    fn balanced_tb_yields_balanced_f10() {
        // A genuinely balanced trial balance (Σ net-debit = 0).
        let t = tb(vec![
            tb_row("2131", "0", "0", "50000", "0"),
            tb_row("2813", "0", "0", "0", "10000"),
            tb_row("371", "0", "0", "20000", "0"),
            tb_row("4111", "0", "0", "30000", "0"),
            tb_row("5121", "0", "0", "40000", "0"),
            tb_row("101", "0", "0", "0", "25000"),
            tb_row("401", "0", "0", "0", "25000"),
            tb_row("5191", "0", "0", "0", "40000"), // overdraft — must NOT be dropped
            tb_row("121", "0", "0", "0", "40000"),
        ]);
        let f = compute_f10(&t);
        // Active = rd.04 + rd.09 + rd.10.
        let active = f["F10_0042"] + f["F10_0092"] + f["F10_0102"];
        // Pasiv = capitaluri (rd.49) + datorii (rd.13 + rd.16) + provizioane (rd.17) + ven. avans.
        let pasiv = f["F10_0492"] + f["F10_0132"] + f["F10_0162"] + f["F10_0172"] + f["F10_0182"];
        assert_eq!(
            active, pasiv,
            "Active = Capitaluri + Datorii (5191 not dropped)"
        );
    }
}
