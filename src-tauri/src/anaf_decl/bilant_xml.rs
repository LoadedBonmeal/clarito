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
use rust_decimal::Decimal;
use std::collections::HashMap;

/// Resolve the bilanț size form applying the OMFP 1802/2014 pct. 13 alin. (2) two-consecutive-years
/// rule: a single year in which the criteria point to a different category than the one established
/// last year (`prior_year`) does NOT switch the category — the entity keeps `prior_year` until the
/// breach persists a second consecutive year (which the user confirms via `form_override`).
/// `form_override` always wins; an absent/invalid `prior_year` falls back to `current`.
pub fn resolve_size_form(
    current: &str,
    form_override: Option<&str>,
    prior_year: Option<&str>,
) -> String {
    let valid = |f: &str| matches!(f, "UU" | "BS" | "BL");
    // Defensive: an out-of-range `current` falls back to the smallest form (micro/UU) rather than
    // emitting a garbage code; in practice the caller always passes a computed UU/BS/BL.
    let current = if valid(current) { current } else { "UU" };
    if let Some(o) = form_override.filter(|f| valid(f)) {
        return o.to_string();
    }
    match prior_year.filter(|f| valid(f)) {
        Some(p) if p != current => p.to_string(), // one-year change → sticky to prior year
        _ => current.to_string(),
    }
}

/// Round a Decimal RON value to whole lei (i64), commercial rounding (shared helper).
fn lei(d: Decimal) -> i64 {
    crate::anaf_decl::round_lei(d)
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

/// Period turnover (rulaj) of accounts matching `pred`: (debit_turnover, credit_turnover). Used for
/// the developed F20 (cont de profit și pierdere).
fn turn(tb: &TrialBalance, pred: impl Fn(&str) -> bool) -> (Decimal, Decimal) {
    let p = |s: &str| s.parse::<Decimal>().unwrap_or(Decimal::ZERO);
    let (mut d, mut c) = (Decimal::ZERO, Decimal::ZERO);
    for r in &tb.rows {
        if pred(&r.account_code) {
            d += p(&r.period_debit);
            c += p(&r.period_credit);
        }
    }
    (d, c)
}

/// Developed F10 (bilanț dezvoltat, rd.1-103) for the LARGE form S1002/BL. Maps account prefixes to
/// the developed rows (assets debit-positive, capital/liabilities credit-positive), then derives the
/// official TOTAL rows from the structura formulas so the XSD «corelație» cross-checks pass.
pub fn compute_f10_developed(tb: &TrialBalance) -> HashMap<String, i64> {
    let mut f: HashMap<String, i64> = HashMap::new();
    let sw = |code: &str, p: &str| code.starts_with(p);
    // Asset row (debit-positive net). credit_side=false.
    let put = |f: &mut HashMap<String, i64>, row: &str, o: Decimal, c: Decimal, credit: bool| {
        let (o, c) = if credit { (-o, -c) } else { (o, c) };
        f.insert(format!("F10_{row}1"), lei(o));
        f.insert(format!("F10_{row}2"), lei(c));
    };

    // ── A.I Imobilizări necorporale (rd.01-07) — net of 280x/290x amortization/depreciation ──
    let (o, c) = net(tb, |x| sw(x, "201") || sw(x, "2801"));
    put(&mut f, "001", o, c, false);
    let (o, c) = net(tb, |x| sw(x, "203") || sw(x, "2803") || sw(x, "2903"));
    put(&mut f, "002", o, c, false);
    let (o, c) = net(tb, |x| {
        sw(x, "205")
            || sw(x, "208")
            || sw(x, "2805")
            || sw(x, "2808")
            || sw(x, "2905")
            || sw(x, "2908")
    });
    put(&mut f, "003", o, c, false);
    let (o, c) = net(tb, |x| sw(x, "2071") || sw(x, "2807"));
    put(&mut f, "004", o, c, false);
    let (o, c) = net(tb, |x| sw(x, "206") || sw(x, "2806") || sw(x, "2906"));
    put(&mut f, "005", o, c, false);
    let (o, c) = net(tb, |x| x == "4094");
    put(&mut f, "006", o, c, false);

    // ── A.II Imobilizări corporale (rd.08-17) ──
    let (o, c) = net(tb, |x| {
        sw(x, "211")
            || sw(x, "212")
            || sw(x, "2811")
            || sw(x, "2812")
            || sw(x, "2911")
            || sw(x, "2912")
    });
    put(&mut f, "008", o, c, false);
    let (o, c) = net(tb, |x| {
        sw(x, "213") || sw(x, "223") || sw(x, "2813") || sw(x, "2913")
    });
    put(&mut f, "009", o, c, false);
    let (o, c) = net(tb, |x| {
        sw(x, "214") || sw(x, "224") || sw(x, "2814") || sw(x, "2914")
    });
    put(&mut f, "010", o, c, false);
    let (o, c) = net(tb, |x| sw(x, "215") || sw(x, "2815") || sw(x, "2915"));
    put(&mut f, "011", o, c, false);
    let (o, c) = net(tb, |x| sw(x, "231") || sw(x, "2931"));
    put(&mut f, "012", o, c, false);
    let (o, c) = net(tb, |x| sw(x, "235") || sw(x, "2935"));
    put(&mut f, "013", o, c, false);
    let (o, c) = net(tb, |x| sw(x, "216") || sw(x, "2816") || sw(x, "2916"));
    put(&mut f, "014", o, c, false);
    let (o, c) = net(tb, |x| {
        sw(x, "217") || sw(x, "227") || sw(x, "2817") || sw(x, "2917")
    });
    put(&mut f, "015", o, c, false);
    let (o, c) = net(tb, |x| x == "4093");
    put(&mut f, "016", o, c, false);

    // ── A.III Imobilizări financiare (rd.18-23) ──
    let (o, c) = net(tb, |x| sw(x, "261") || sw(x, "2961"));
    put(&mut f, "018", o, c, false);
    let (o, c) = net(tb, |x| {
        sw(x, "263") || sw(x, "265") || sw(x, "266") || sw(x, "267") || sw(x, "296")
    });
    put(&mut f, "022", o, c, false);

    // ── B.I Stocuri (rd.26-30) ── (lump by class-3 less adjustments + 409x advances)
    let (o, c) = net(tb, |x| {
        (sw(x, "30")
            || sw(x, "32")
            || sw(x, "33")
            || sw(x, "34")
            || sw(x, "35")
            || sw(x, "36")
            || sw(x, "37")
            || sw(x, "38"))
            && !sw(x, "39")
    });
    let (oa, ca) = net(tb, |x| sw(x, "39")); // adjustments (credit) reduce stocks
    put(&mut f, "028", o + oa, c + ca, false);
    let (o, c) = net(tb, |x| x == "4091");
    put(&mut f, "029", o, c, false);

    // ── B.II Creanțe (rd.31-35, 301) ── class-4 debit + 5187 + 409x
    let (o, c) = net(tb, |x| {
        (sw(x, "4")
            && !sw(x, "401")
            && !sw(x, "403")
            && !sw(x, "404")
            && !sw(x, "405")
            && !sw(x, "408")
            && !sw(x, "419")
            && !sw(x, "421")
            && !sw(x, "423")
            && !sw(x, "424")
            && !sw(x, "426")
            && !sw(x, "427")
            && !sw(x, "428")
            && !sw(x, "431")
            && !sw(x, "437")
            && !sw(x, "438")
            && !sw(x, "440")
            && !sw(x, "441")
            && !sw(x, "442")
            && !sw(x, "444")
            && !sw(x, "446")
            && !sw(x, "447")
            && !sw(x, "448")
            && !sw(x, "455")
            && !sw(x, "457")
            && !sw(x, "458")
            && !sw(x, "462")
            && !sw(x, "466")
            && !sw(x, "467")
            && !sw(x, "472")
            && !sw(x, "475")
            && !sw(x, "478")
            && !sw(x, "409"))
            || sw(x, "5187")
    });
    // Only keep the net DEBIT part (receivables); credit balances belong to datorii.
    put(
        &mut f,
        "034",
        o.max(Decimal::ZERO),
        c.max(Decimal::ZERO),
        false,
    );

    // ── B.III Investiții pe termen scurt (rd.37-39) + Casa (rd.40) ──
    let (o, c) = net(tb, |x| sw(x, "50") || sw(x, "59"));
    put(&mut f, "037", o, c, false);
    let (o, c) = net(tb, |x| {
        sw(x, "511") || sw(x, "512") || sw(x, "53") || sw(x, "54") || sw(x, "508")
    });
    put(&mut f, "040", o, c, false);

    // ── C Cheltuieli în avans (rd.42/43) ──
    let (o, c) = net(tb, |x| x == "471");
    put(&mut f, "043", o, c, false);

    // ── D Datorii ≤ 1 an (rd.45-53) — credit-positive ──
    let (o, c) = net(tb, |x| sw(x, "401") || sw(x, "404") || sw(x, "408"));
    put(&mut f, "048", o, c, true);
    let (o, c) = net(tb, |x| sw(x, "403") || sw(x, "405"));
    put(&mut f, "049", o, c, true);
    let (o, c) = net(tb, |x| sw(x, "16") || sw(x, "519"));
    put(&mut f, "046", o, c, true);
    // Other current settlement debts (the big residual bucket rd.52): 42x salarii/contributii, 43x,
    // 44x (incl. 441 impozit pe profit datorat + 4423 TVA de plată), 455/456/457/458, 462, 466, 473,
    // 509, 518x/519x interest+overdraft.
    let (o, c) = net(tb, |x| {
        sw(x, "42")
            || sw(x, "43")
            || sw(x, "44")
            || sw(x, "455")
            || sw(x, "456")
            || sw(x, "457")
            || sw(x, "458")
            || sw(x, "462")
            || sw(x, "466")
            || sw(x, "473")
            || sw(x, "509")
            || sw(x, "5186")
            || sw(x, "5191")
            || sw(x, "5198")
    });
    // Keep only the CREDIT (liability) part: net is debit-positive, so credit balances are negative;
    // min(0) selects them, and put(credit=true) negates back to the positive liability figure.
    // (Debit balances of these accounts are receivables, mapped under rd.34 — emit 0 here.)
    put(
        &mut f,
        "052",
        o.min(Decimal::ZERO),
        c.min(Decimal::ZERO),
        true,
    );

    // ── H Provizioane (rd.65-68) — credit-positive ──
    let (o, c) = net(tb, |x| sw(x, "15"));
    put(&mut f, "067", o, c, true);

    // ── I Venituri în avans (rd.69-79) ──
    let (o, c) = net(tb, |x| x == "472" || sw(x, "475") || sw(x, "478"));
    put(&mut f, "072", o, c, true);

    // ── J Capitaluri (rd.80-103) — sold credit, IntPoz≥0 ──
    let (o, c) = net(tb, |x| sw(x, "1012"));
    put(&mut f, "080", o, c, true);
    let (o, c) = net(tb, |x| sw(x, "1011"));
    put(&mut f, "081", o, c, true);
    let (o, c) = net(tb, |x| sw(x, "104"));
    put(&mut f, "086", o, c, true);
    let (o, c) = net(tb, |x| sw(x, "105"));
    put(&mut f, "087", o, c, true);
    let (o, c) = net(tb, |x| sw(x, "106"));
    put(&mut f, "090", o, c, true);
    // Rezultat reportat 117: sold C → rd.95; sold D → rd.96.
    let (o117, c117) = net(tb, |x| sw(x, "117"));
    put(
        &mut f,
        "095",
        o117.min(Decimal::ZERO),
        c117.min(Decimal::ZERO),
        true,
    ); // -net = credit part
    put(
        &mut f,
        "096",
        o117.max(Decimal::ZERO),
        c117.max(Decimal::ZERO),
        false,
    ); // debit part
       // Rezultatul exercițiului = 121 sold + not-yet-closed 6/7 result.
    let (o121, c121) = net(tb, |x| x == "121");
    let (o67, c67) = net(tb, |x| sw(x, "7") || sw(x, "6"));
    let (res_o, res_c) = ((-o121) + (-o67), (-c121) + (-c67)); // credit-positive result
    f.insert("F10_0971".into(), lei(res_o.max(Decimal::ZERO)));
    f.insert("F10_0972".into(), lei(res_c.max(Decimal::ZERO)));
    f.insert("F10_0981".into(), lei((-res_o).max(Decimal::ZERO)));
    f.insert("F10_0982".into(), lei((-res_c).max(Decimal::ZERO)));

    // ── Derived TOTAL rows (structura formulas) ──
    let g = |f: &HashMap<String, i64>, row: &str, col: u8| {
        *f.get(&format!("F10_{row}{col}")).unwrap_or(&0)
    };
    let sum = |f: &HashMap<String, i64>, rows: &[&str], col: u8| {
        rows.iter().map(|r| g(f, r, col)).sum::<i64>()
    };
    for col in [1u8, 2] {
        let r007 = sum(&f, &["001", "002", "003", "004", "005", "006"], col);
        let r017 = sum(
            &f,
            &[
                "008", "009", "010", "011", "012", "013", "014", "015", "016",
            ],
            col,
        );
        let r024 = sum(&f, &["018", "019", "020", "021", "022", "023"], col);
        f.insert(format!("F10_007{col}"), r007);
        f.insert(format!("F10_017{col}"), r017);
        f.insert(format!("F10_024{col}"), r024);
        f.insert(format!("F10_025{col}"), r007 + r017 + r024);
        let r030 = sum(&f, &["026", "027", "028", "029"], col);
        let r036 = sum(&f, &["031", "032", "033", "034", "035", "301"], col);
        let r039 = sum(&f, &["037", "038"], col);
        f.insert(format!("F10_030{col}"), r030);
        f.insert(format!("F10_036{col}"), r036);
        f.insert(format!("F10_039{col}"), r039);
        f.insert(
            format!("F10_041{col}"),
            r030 + r036 + r039 + g(&f, "040", col),
        );
        let r053 = sum(
            &f,
            &["045", "046", "047", "048", "049", "050", "051", "052"],
            col,
        );
        f.insert(format!("F10_053{col}"), r053);
        let r042 = g(&f, "043", col) + g(&f, "044", col);
        f.insert(format!("F10_042{col}"), r042);
        let r041 = r030 + r036 + r039 + g(&f, "040", col);
        let r054 = r041 + g(&f, "043", col)
            - r053
            - g(&f, "070", col)
            - g(&f, "073", col)
            - g(&f, "076", col);
        f.insert(format!("F10_054{col}"), r054);
        f.insert(
            format!("F10_055{col}"),
            r025_value(&f, col) + g(&f, "044", col) + r054,
        );
        f.insert(
            format!("F10_068{col}"),
            sum(&f, &["065", "066", "067"], col),
        );
        f.insert(
            format!("F10_079{col}"),
            sum(&f, &["069", "072", "075", "078"], col),
        );
        let r085 = sum(&f, &["080", "081", "082", "083", "084"], col);
        let r091 = sum(&f, &["088", "089", "090"], col);
        f.insert(format!("F10_085{col}"), r085);
        f.insert(format!("F10_091{col}"), r091);
        let r100 = r085 + g(&f, "086", col) + g(&f, "087", col) + r091 - g(&f, "092", col)
            + g(&f, "093", col)
            - g(&f, "094", col)
            + g(&f, "095", col)
            - g(&f, "096", col)
            + g(&f, "097", col)
            - g(&f, "098", col)
            - g(&f, "099", col);
        f.insert(format!("F10_100{col}"), r100);
        f.insert(
            format!("F10_103{col}"),
            r100 + g(&f, "101", col) + g(&f, "102", col),
        );
    }

    // IntPoz clamp for the capital + amounts rows that the schema declares ≥0.
    for row in [
        "080", "081", "082", "083", "084", "085", "088", "089", "090", "091", "095", "097", "100",
        "103",
    ] {
        for col in [1u8, 2] {
            if let Some(v) = f.get_mut(&format!("F10_{row}{col}")) {
                if *v < 0 {
                    *v = 0;
                }
            }
        }
    }
    f
}

fn r025_value(f: &HashMap<String, i64>, col: u8) -> i64 {
    *f.get(&format!("F10_025{col}")).unwrap_or(&0)
}

/// Full developed F20 (cont de profit și pierdere, rd.1-70). col 1 = exercițiul precedent (from the
/// prior trial balance, if supplied), col 2 = curent. Values from period turnover.
pub fn compute_f20_full(cur: &TrialBalance, prior: Option<&TrialBalance>) -> HashMap<String, i64> {
    let mut f = HashMap::new();
    // For one TB, returns the full-F20 row → value map (credit turnover for revenue, debit for exp).
    let rows_for = |tb: &TrialBalance| -> HashMap<String, i64> {
        let sw = |c: &str, p: &str| c.starts_with(p);
        let cr = |pred: &dyn Fn(&str) -> bool| turn(tb, pred).1; // credit turnover (revenue)
        let db = |pred: &dyn Fn(&str) -> bool| turn(tb, pred).0; // debit turnover (expense)
        let mut m: HashMap<String, i64> = HashMap::new();
        // Revenue
        let ca = cr(&|x| sw(x, "70")); // 70x net sales (≈ cifra de afaceri)
        m.insert("001".into(), lei(ca));
        m.insert("016".into(), lei(ca)); // venituri exploatare total (simplified to revenue)
                                         // Expense by nature
        let exp_mat = db(&|x| sw(x, "60") && !sw(x, "607") && !sw(x, "609"));
        let exp_marf = db(&|x| sw(x, "607"));
        let exp_pers = db(&|x| sw(x, "64"));
        let exp_amort = db(&|x| sw(x, "681"));
        let exp_alte = db(&|x| sw(x, "61") || sw(x, "62") || sw(x, "65"));
        let exp_total = exp_mat + exp_marf + exp_pers + exp_amort + exp_alte;
        m.insert("042".into(), lei(exp_total)); // cheltuieli exploatare total
        let rez_expl = ca - exp_total;
        m.insert("043".into(), lei(rez_expl.max(Decimal::ZERO)));
        m.insert("044".into(), lei((-rez_expl).max(Decimal::ZERO)));
        // Financial
        let ven_fin = cr(&|x| sw(x, "76"));
        let chelt_fin = db(&|x| sw(x, "66"));
        m.insert("052".into(), lei(ven_fin));
        m.insert("059".into(), lei(chelt_fin));
        let ven_tot = ca + ven_fin;
        let chelt_tot = exp_total + chelt_fin;
        m.insert("062".into(), lei(ven_tot));
        m.insert("063".into(), lei(chelt_tot));
        let rez_brut = ven_tot - chelt_tot;
        m.insert("064".into(), lei(rez_brut.max(Decimal::ZERO)));
        m.insert("065".into(), lei((-rez_brut).max(Decimal::ZERO)));
        let imp = db(&|x| sw(x, "691") || sw(x, "698"));
        m.insert("066".into(), lei(imp));
        let rez_net = rez_brut - imp;
        m.insert("069".into(), lei(rez_net.max(Decimal::ZERO)));
        m.insert("070".into(), lei((-rez_net).max(Decimal::ZERO)));
        m
    };
    let cur_rows = rows_for(cur);
    let prior_rows = prior.map(rows_for).unwrap_or_default();
    let all_keys: std::collections::BTreeSet<String> =
        cur_rows.keys().chain(prior_rows.keys()).cloned().collect();
    for k in all_keys {
        f.insert(format!("F20_{k}1"), *prior_rows.get(&k).unwrap_or(&0));
        f.insert(format!("F20_{k}2"), *cur_rows.get(&k).unwrap_or(&0));
    }
    f
}

/// "Settlement" accounts whose debit balance is a creanță and credit balance a datorie:
/// class-4 (excl. avans 471/472/475 AND 4093/4094 avansuri imobilizări, which belong in rd.01/02 of
/// the F10, not creanțe) + interest 518x + short-term bank credit 519x (incl. their 4-digit
/// analytics 5191/5198 — the common overdraft accounts that must not be dropped).
fn is_settlement(code: &str) -> bool {
    (code.starts_with('4')
        && code != "471"
        && code != "472"
        && !code.starts_with("475")
        && !code.starts_with("4093")
        && !code.starts_with("4094"))
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
    // Cifra de afaceri netă (rd.01) = clasa 70x — NU operating_revenue (care include 71x/72x/74x/75x).
    let ca = |x: &ProfitLoss| p(&x.cifra_afaceri);

    col("001", ca(pnl), prv(&ca)); // cifra de afaceri netă (rd.01) — same code in both forms
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
use crate::anaf_decl::xml_esc as esc;

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
    // Control sum (totalPlata_A) = total capitaluri proprii. The prescurtat F10 (UU/BS) carries it
    // in F10_0492 (rd.49); the DEVELOPED F10 (BL, entitate mare/mijlocie) never inserts F10_0492 —
    // its total is rd.100 → F10_1002. Hard-coding F10_0492 made EVERY BL bilanț emit
    // totalPlata_A="0" (control-sum mismatch → ANAF reject). Pick the key by form.
    let cap_key = if form == "BL" { "F10_1002" } else { "F10_0492" };
    let total_plata = f10.get(cap_key).copied().unwrap_or_else(|| {
        tracing::warn!(
            form,
            cap_key,
            "bilanț: total capitaluri proprii lipsește din F10 — totalPlata_A=0"
        );
        0
    });
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

    // Pretty-print so the F10/F20/F30 children are 2-space indented like every other declaration.
    // (The `\` source line-continuations below strip the literal leading spaces, so the raw string
    // would otherwise emit children at column 0 — the canonical formatter re-indents them; the
    // inter-element whitespace stays XSD-ignorable / DUK-safe.)
    let raw = format!(
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
    );
    crate::anaf_decl::xml::pretty_print(&raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_form_two_consecutive_years_rule() {
        // No prior year → use the current-year computation.
        assert_eq!(resolve_size_form("BS", None, None), "BS");
        // Stable: current == prior → keep it.
        assert_eq!(resolve_size_form("UU", None, Some("UU")), "UU");
        // One-year change (criteria say BS, but last year was UU) → STICKY to UU (pct. 13(2)).
        assert_eq!(resolve_size_form("BS", None, Some("UU")), "UU");
        // Second year confirmed → the user forces the new form via override.
        assert_eq!(resolve_size_form("BS", Some("BS"), Some("UU")), "BS");
        // Override always wins; invalid prior is ignored.
        assert_eq!(resolve_size_form("BL", None, Some("ZZ")), "BL");
        // Defensive: an invalid `current` falls back to UU.
        assert_eq!(resolve_size_form("ZZ", None, None), "UU");
    }

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
        // Same canonical professional format as every other declaration (UTF-8 prolog + LF + 2-space).
        crate::anaf_decl::xml::assert_canonical_xml(&xml);
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
    fn developed_bl_generates_and_writes_for_xmllint() {
        let t = tb(vec![
            tb_row("2131", "0", "0", "50000", "0"),
            tb_row("2813", "0", "0", "0", "10000"),
            tb_row("371", "0", "0", "20000", "0"),
            tb_row("4111", "0", "0", "30000", "0"),
            tb_row("5121", "0", "0", "15000", "0"),
            tb_row("1012", "0", "0", "0", "50000"),
            tb_row("401", "0", "0", "0", "25000"),
            tb_row("421", "0", "0", "0", "5850"), // salarii datorate → rd.052 (datorii curente)
            tb_row("4423", "0", "0", "0", "2000"), // TVA de plată → rd.052
            tb_row("121", "0", "0", "0", "30000"),
        ]);
        let f10 = compute_f10_developed(&t);
        // rd.09 (213 corporale) net = 50.000 − 10.000 = 40.000; total imob corporale rd.017 = 40.000.
        assert_eq!(f10["F10_0092"], 40000);
        assert_eq!(f10["F10_0172"], 40000);
        // rd.025 active imobilizate total = 40.000 (only corporale).
        assert_eq!(f10["F10_0252"], 40000);
        // rd.052 (datorii curente diverse) = 421 5.850 + 4423 2.000 = 7.850 (sign-fixed; was 0).
        assert_eq!(f10["F10_0522"], 7850);
        let f20 = compute_f20_full(&t, None);
        let h = BilantHeader {
            year: 2026,
            cui: "12345678".into(),
            den: "Mare SRL".into(),
            adresa: "Str 1".into(),
            reg_com: "J40/1/2020".into(),
            caen: "6201".into(),
            county: "CJ".into(),
            nume_admin: "Ion Popescu".into(),
        };
        let xml = generate_bilant_xml(&h, &f10, &f20, "BL");
        assert!(xml.contains("<Bilant1002"));
        assert!(xml.contains("F10_0172=\"40000\""));
        // The BL control sum must come from the developed total capitaluri (rd.100 → F10_1002),
        // NOT the prescurtat F10_0492 (absent in the developed layout → would emit "0").
        let cap = f10["F10_1002"];
        assert_ne!(cap, 0, "developed BL total capitaluri must be non-zero");
        assert!(
            xml.contains(&format!("totalPlata_A=\"{cap}\"")),
            "BL totalPlata_A must equal F10_1002 ({cap}), not 0"
        );
        // Write for external `xmllint --schema s1002.xsd` validation (CROSS-01: portable temp dir).
        let _ = std::fs::write(std::env::temp_dir().join("bl-generated.xml"), &xml);
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
