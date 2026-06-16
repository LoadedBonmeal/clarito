//! Dividende repartizate + impozit pe dividende (Legea 141/2025): **16%** pentru dividende DISTRIBUITE
//! de la 01.01.2026; **10%** tranzitoriu pentru distribuiri anterioare SAU pentru dividende din
//! situații financiare interimare întocmite în 2025 (chiar dacă plata e în 2026). Fiecare înregistrare
//! postează nota contabilă **117 / 457 / 446** (idempotent) și expune obligația pentru Declarația 100,
//! scadentă pe 25 a lunii următoare PLĂȚII (ori 25 ianuarie pentru dividende distribuite, neplătite).

use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::collections::BTreeMap;
use std::str::FromStr;

use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

/// Cota de impozit pe dividende pentru o distribuire (Legea 141/2025 art. II pct.1 + art. VII):
/// 16% pentru dividende DISTRIBUITE de la 01.01.2026; 10% pentru distribuiri anterioare sau pentru
/// dividende din situații interimare 2025 (`interim_2025`). `distribution_date` = ISO `YYYY-MM-DD`
/// (compararea lexicografică a datelor ISO = cronologică, vezi `db::concedii`).
pub fn dividend_tax_rate(distribution_date: &str, interim_2025: bool) -> i64 {
    if interim_2025 {
        return 10;
    }
    if distribution_date >= "2026-01-01" {
        16
    } else {
        10
    }
}

/// Termenul de plată/declarare a impozitului pe dividende (Cod fiscal art. 43(2)/97(7)/224(4)): 25 a
/// lunii următoare celei în care s-a făcut PLATA; pentru dividende distribuite dar NEPLĂTITE până la
/// finalul anului, 25 ianuarie a anului următor anului distribuirii. Întoarce ISO `YYYY-MM-DD`.
pub fn dividend_tax_deadline(distribution_date: &str, payment_date: Option<&str>) -> String {
    use chrono::Datelike;
    if let Some(pd) = payment_date.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(pd, "%Y-%m-%d") {
            let (y, m) = if d.month() == 12 {
                (d.year() + 1, 1)
            } else {
                (d.year(), d.month() + 1)
            };
            return format!("{y:04}-{m:02}-25");
        }
    }
    let year = distribution_date
        .get(0..4)
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0);
    format!("{:04}-01-25", year + 1)
}

fn round2(d: Decimal) -> Decimal {
    d.round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dividend {
    pub id: String,
    pub company_id: String,
    pub distribution_date: String,
    pub payment_date: Option<String>,
    pub gross_amount: String,
    pub tax_rate: i64,
    pub tax_amount: String,
    pub net_amount: String,
    pub interim_2025: bool,
    /// Numele beneficiarului (text liber) — refolosit ca `den1` în D205.
    pub shareholder: Option<String>,
    /// CNP-ul beneficiarului (D205 `cifR`, N13 mod-11). Opțional la înregistrare; cerut la exportul D205.
    pub beneficiary_cnp: Option<String>,
    /// Rezident fiscal RO (D205 `Rezid`; 1 = rezident → D205, 2 = nerezident → D207). Capitolul
    /// dividende raportează DOAR rezidenți (filtru în `d205_beneficiaries_for_year`), deci Rezid e
    /// mereu "1"; ramura "2" e rezervată/neutilizată (validatorul D205 interzice Rezid=2 la
    /// tip_venit 08). Implicit true.
    pub beneficiary_resident: bool,
    /// Tipul beneficiarului: "PF" (persoană fizică, art. 97 → D100 cod 604, intră în D205) sau
    /// "PJ" (persoană juridică, art. 43 → D100 cod 150, EXCLUS din D205). Implicit "PF".
    pub beneficiary_type: String,
    pub note: Option<String>,
    /// Termenul de plată al impozitului (derivat, nu stocat).
    pub tax_deadline: String,
}

/// Persoană fizică (default). Distribuie obligația D100 pe cod 604 (art. 97) și intră în D205.
pub const BEN_PF: &str = "PF";
/// Persoană juridică. Distribuie obligația D100 pe cod 150 (art. 43); exclusă din D205.
pub const BEN_PJ: &str = "PJ";

fn default_resident() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DividendInput {
    pub company_id: String,
    pub distribution_date: String,
    pub payment_date: Option<String>,
    pub gross_amount: String,
    #[serde(default)]
    pub interim_2025: bool,
    pub shareholder: Option<String>,
    #[serde(default)]
    pub beneficiary_cnp: Option<String>,
    #[serde(default = "default_resident")]
    pub beneficiary_resident: bool,
    /// "PF" (implicit) sau "PJ" — vezi [`Dividend::beneficiary_type`].
    #[serde(default)]
    pub beneficiary_type: Option<String>,
    pub note: Option<String>,
}

/// DIV-01: câmpurile de IDENTITATE beneficiar (+ data plății / nota) editabile in-place. NU include
/// sumele (brut/impozit) — acelea postează nota 117/457/446, deci rămân imuabile pe acest drum
/// (pentru a le schimba: delete + recreate, ca să se re-posteze GL-ul). Permite corectarea unui CNP
/// lipsă/greșit fără a pierde înregistrarea (altfel exportul D205 al anului ar rămâne blocat).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DividendBeneficiaryUpdate {
    pub id: String,
    pub company_id: String,
    pub payment_date: Option<String>,
    pub shareholder: Option<String>,
    #[serde(default)]
    pub beneficiary_cnp: Option<String>,
    #[serde(default = "default_resident")]
    pub beneficiary_resident: bool,
    #[serde(default)]
    pub beneficiary_type: Option<String>,
    pub note: Option<String>,
}

fn row_to_dividend(r: &sqlx::sqlite::SqliteRow) -> Dividend {
    let distribution_date: String = r.get("distribution_date");
    let payment_date: Option<String> = r.get("payment_date");
    let tax_deadline = dividend_tax_deadline(&distribution_date, payment_date.as_deref());
    Dividend {
        id: r.get("id"),
        company_id: r.get("company_id"),
        distribution_date,
        payment_date,
        gross_amount: r.get("gross_amount"),
        tax_rate: r.get("tax_rate"),
        tax_amount: r.get("tax_amount"),
        net_amount: r.get("net_amount"),
        interim_2025: r.get::<i64, _>("interim_2025") != 0,
        shareholder: r.get("shareholder"),
        beneficiary_cnp: r.get("beneficiary_cnp"),
        beneficiary_resident: r.get::<i64, _>("beneficiary_resident") != 0,
        beneficiary_type: r.get("beneficiary_type"),
        note: r.get("note"),
        tax_deadline,
    }
}

const SELECT: &str =
    "SELECT id, company_id, distribution_date, payment_date, gross_amount, tax_rate, \
     tax_amount, net_amount, interim_2025, shareholder, beneficiary_cnp, beneficiary_resident, \
     beneficiary_type, note FROM dividends";

pub async fn list(pool: &SqlitePool, company_id: &str) -> AppResult<Vec<Dividend>> {
    let rows = sqlx::query(&format!(
        "{SELECT} WHERE company_id = ?1 ORDER BY distribution_date DESC, created_at DESC"
    ))
    .bind(company_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(row_to_dividend).collect())
}

pub async fn get(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<Dividend> {
    let row = sqlx::query(&format!("{SELECT} WHERE id = ?1 AND company_id = ?2"))
        .bind(id)
        .bind(company_id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(row_to_dividend(&row))
}

pub async fn create(pool: &SqlitePool, input: DividendInput) -> AppResult<Dividend> {
    let date = input.distribution_date.trim();
    if date.len() != 10 || chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
        return Err(AppError::Validation(
            "Data distribuirii trebuie să fie o dată calendaristică validă (AAAA-LL-ZZ).".into(),
        ));
    }
    let gross = round2(
        Decimal::from_str(input.gross_amount.trim())
            .map_err(|_| AppError::Validation("Sumă brută dividende invalidă.".into()))?,
    );
    if gross <= Decimal::ZERO {
        return Err(AppError::Validation(
            "Suma brută a dividendelor trebuie să fie > 0.".into(),
        ));
    }
    // CNP beneficiar — opțional la înregistrare (regression-safe), dar dacă e prezent trebuie să fie
    // valid (D205 `cifR`, N13 mod-11; ANAF respinge un CNP invalid). Cerut la exportul D205.
    let ben_cnp = input
        .beneficiary_cnp
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(cnp) = ben_cnp {
        if !crate::anaf_decl::valid_cnp(cnp) {
            return Err(AppError::Validation(
                "CNP beneficiar invalid — 13 cifre cu cifra de control corectă.".into(),
            ));
        }
    }
    let rate = dividend_tax_rate(date, input.interim_2025);
    let tax = round2(gross * Decimal::new(rate, 2));
    let net = gross - tax; // ambele 2dp → diferența e exactă, deci nota e echilibrată

    // Tip beneficiar: "PJ" doar dacă e cerut explicit; orice altceva → "PF" (implicit, cazul uzual).
    let ben_type = if input.beneficiary_type.as_deref() == Some(BEN_PJ) {
        BEN_PJ
    } else {
        BEN_PF
    };

    let id = new_id();
    sqlx::query(
        "INSERT INTO dividends (id, company_id, distribution_date, payment_date, gross_amount, \
         tax_rate, tax_amount, net_amount, interim_2025, shareholder, beneficiary_cnp, \
         beneficiary_resident, beneficiary_type, note, created_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
    )
    .bind(&id)
    .bind(&input.company_id)
    .bind(date)
    .bind(
        input
            .payment_date
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty()),
    )
    .bind(gross.to_string())
    .bind(rate)
    .bind(tax.to_string())
    .bind(net.to_string())
    .bind(input.interim_2025 as i64)
    .bind(input.shareholder.as_deref())
    .bind(ben_cnp)
    .bind(input.beneficiary_resident as i64)
    .bind(ben_type)
    .bind(input.note.as_deref())
    .bind(now_unix())
    .execute(pool)
    .await?;

    // Nota contabilă: D 117 (rezultat reportat) brut; C 457 (dividende de plată) net; C 446 (impozit
    // pe dividende) impozit. Σdebit (brut) = Σcredit (net + impozit). Idempotent per dividend id.
    let desc = format!("Repartizare dividende {date} (impozit {rate}%)");
    crate::db::gl::post_manual_journal(
        pool,
        &crate::db::gl::ManualJournal {
            company_id: &input.company_id,
            journal_id: "DIVERSE",
            journal_type: "DIVIDEND",
            source_type: "DIVIDEND",
            source_id: &id,
            date,
            description: &desc,
        },
        &[
            ("117", gross, Decimal::ZERO),
            ("457", Decimal::ZERO, net),
            ("446", Decimal::ZERO, tax),
        ],
    )
    .await?;

    get(pool, &id, &input.company_id).await
}

/// DIV-01: edit a dividend's beneficiary identity (CNP, name, resident, type) + payment_date/note in
/// place — the D205/D100-relevant fields, none of which touch the financial amounts or the 117/457/446
/// GL note. Company-scoped; CNP re-validated (mod-11) when present. Returns the refreshed record.
pub async fn update_beneficiary(
    pool: &SqlitePool,
    upd: DividendBeneficiaryUpdate,
) -> AppResult<Dividend> {
    let ben_cnp = upd
        .beneficiary_cnp
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(cnp) = ben_cnp {
        if !crate::anaf_decl::valid_cnp(cnp) {
            return Err(AppError::Validation(
                "CNP beneficiar invalid — 13 cifre cu cifra de control corectă.".into(),
            ));
        }
    }
    let ben_type = if upd.beneficiary_type.as_deref() == Some(BEN_PJ) {
        BEN_PJ
    } else {
        BEN_PF
    };
    let res = sqlx::query(
        "UPDATE dividends SET payment_date = ?3, shareholder = ?4, beneficiary_cnp = ?5, \
         beneficiary_resident = ?6, beneficiary_type = ?7, note = ?8 \
         WHERE id = ?1 AND company_id = ?2",
    )
    .bind(&upd.id)
    .bind(&upd.company_id)
    .bind(
        upd.payment_date
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty()),
    )
    .bind(upd.shareholder.as_deref())
    .bind(ben_cnp)
    .bind(upd.beneficiary_resident as i64)
    .bind(ben_type)
    .bind(upd.note.as_deref())
    .execute(pool)
    .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound); // cross-company / id inexistent
    }
    get(pool, &upd.id, &upd.company_id).await
}

pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    let res = sqlx::query("DELETE FROM dividends WHERE id = ?1 AND company_id = ?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound); // cross-company / id inexistent
    }
    // Șterge și nota contabilă asociată.
    sqlx::query(
        "DELETE FROM gl_journal WHERE company_id = ?1 AND source_type = 'DIVIDEND' AND source_id = ?2",
    )
    .bind(company_id)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Total impozit pe dividende cu termenul de declarare/plată într-o perioadă (lună), pentru obligația
/// din Declarația 100. `period_ym` = `YYYY-MM`.
pub async fn dividend_tax_due_in_period(
    pool: &SqlitePool,
    company_id: &str,
    period_ym: &str,
) -> AppResult<Decimal> {
    let mut total = Decimal::ZERO;
    for d in list(pool, company_id).await? {
        if d.tax_deadline.starts_with(period_ym) {
            total += Decimal::from_str(d.tax_amount.trim()).unwrap_or(Decimal::ZERO);
        }
    }
    Ok(total)
}

/// Obligația D100 pentru dividende către PERSOANE FIZICE (art. 97 Cod fiscal): Nomenclator poziția 6,
/// cod_oblig 604. Codul se selectează în SPV după această poziție.
pub const D100_DIVIDEND_PF_COD: &str = "604";
pub const D100_DIVIDEND_PF_LABEL: &str =
    "Impozit pe veniturile din dividende distribuite persoanelor fizice (art. 97 C.fisc.)";
/// Obligația D100 pentru dividende către PERSOANE JURIDICE (art. 43 Cod fiscal): Nomenclator poziția 4,
/// cod_oblig 150.
pub const D100_DIVIDEND_PJ_COD: &str = "150";
pub const D100_DIVIDEND_PJ_LABEL: &str =
    "Impozit pe dividende distribuite persoanelor juridice (art. 43 C.fisc.)";

/// O linie informativă de obligație de impozit pe dividende pentru Declarația 100: codul de creanță
/// (cod_oblig), denumirea oficială, suma reținută și scadența (25 a lunii). D100 NU emite XML (se depune
/// prin PDF inteligent + SPV), deci rândul e pur INFORMATIV — semnalează contribuabilului obligația de a
/// declara impozitul la termen, pe poziția corectă din Nomenclator (604 PF / 150 PJ).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DividendObligation {
    /// Codul de creanță din Nomenclator (`<cod_oblig>`): "604" (PF) sau "150" (PJ).
    pub cod_oblig: String,
    pub label: String,
    /// Suma impozitului reținut (lei, 2 zecimale — suma exactă din notele 446).
    pub amount: String,
    /// Scadența declarării/plății (zz.ll.aaaa) — 25 a lunii următoare plății.
    pub deadline: String,
    /// Numărul de distribuiri agregate în această obligație.
    pub count: u32,
}

/// Obligațiile de impozit pe dividende cu scadența în lunile date (rânduri informative pentru D100).
/// `months` = liste de `YYYY-MM` (de regulă cele 3 luni ale trimestrului afișat). Grupează pe (lună de
/// scadență, tip beneficiar): rânduri SEPARATE pentru PF (cod 604, art. 97) și PJ (cod 150, art. 43),
/// fiindcă sunt creanțe distincte în Nomenclator și se declară pe poziții diferite. O linie per
/// (lună, tip) cu impozit > 0. Datele ISO se compară lexicografic (cf. restul modulului).
pub async fn dividend_obligations_in_months(
    pool: &SqlitePool,
    company_id: &str,
    months: &[String],
) -> AppResult<Vec<DividendObligation>> {
    let all = list(pool, company_id).await?;
    let mut out = Vec::new();
    for ym in months {
        let (y, m) = ym.split_once('-').unwrap_or(("", ""));
        // PJ înaintea PF e indiferent; emitem PF apoi PJ pentru o ordine stabilă.
        for (is_pj, cod, label) in [
            (false, D100_DIVIDEND_PF_COD, D100_DIVIDEND_PF_LABEL),
            (true, D100_DIVIDEND_PJ_COD, D100_DIVIDEND_PJ_LABEL),
        ] {
            let mut sum = Decimal::ZERO;
            let mut count = 0u32;
            for d in &all {
                // Doar rezidenți: impozitul pe dividende către nerezidenți se declară pe altă
                // obligație (impozit venituri nerezidenți, cod 631) + D207, nu pe 604/150.
                if d.tax_deadline.starts_with(ym.as_str())
                    && d.beneficiary_resident
                    && (d.beneficiary_type == BEN_PJ) == is_pj
                {
                    sum += Decimal::from_str(d.tax_amount.trim()).unwrap_or(Decimal::ZERO);
                    count += 1;
                }
            }
            if count > 0 {
                out.push(DividendObligation {
                    cod_oblig: cod.to_string(),
                    label: label.to_string(),
                    amount: round2(sum).to_string(),
                    deadline: format!("25.{m}.{y}"),
                    count,
                });
            }
        }
    }
    Ok(out)
}

/// Agregă dividendele REZIDENTE cu data distribuirii în anul de venit `year` în rânduri D205 (un
/// `<benef>` per CNP). Nerezidenții sunt EXCLUȘI (se raportează separat în D207). `baza1`/`divid_D` =
/// brutul; `divid_P` = brutul plătit (doar cu dată de plată); `imp1` = impozitul. Întoarce Err dacă
/// vreun dividend rezident NU are CNP (o D205 incompletă = declarație greșită). Sortare deterministă pe
/// CNP (BTreeMap). Funcție PURĂ (testabilă) — folosită de `commands::dividends::export_d205_official`.
pub fn d205_beneficiaries_for_year(
    dividends: &[Dividend],
    year: i32,
) -> AppResult<Vec<crate::anaf_decl::d205_xml::D205Beneficiary>> {
    use crate::anaf_decl::d205_xml::D205Beneficiary;

    // D205 raportează DOAR persoane fizice rezidente: nerezidenții → D207, persoanele juridice (art. 43)
    // nu se raportează în D205 (impozitul lor e pe altă obligație D100, cod 150).
    let year_str = format!("{year:04}");
    let residents: Vec<&Dividend> = dividends
        .iter()
        .filter(|d| {
            d.distribution_date.starts_with(&year_str)
                && d.beneficiary_resident
                && d.beneficiary_type != BEN_PJ
        })
        .collect();

    let missing = residents
        .iter()
        .filter(|d| {
            d.beneficiary_cnp
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
        })
        .count();
    if missing > 0 {
        return Err(AppError::Validation(format!(
            "D205 {year}: {missing} dividende rezidente fără CNP beneficiar — completați CNP-ul \
             înainte de export (nerezidenții se raportează separat în D207)."
        )));
    }

    let mut by_cnp: BTreeMap<String, D205Beneficiary> = BTreeMap::new();
    for d in &residents {
        let cnp = d
            .beneficiary_cnp
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_string();
        let gross = Decimal::from_str(d.gross_amount.trim()).unwrap_or_default();
        let tax = Decimal::from_str(d.tax_amount.trim()).unwrap_or_default();
        let paid = if d
            .payment_date
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_some()
        {
            gross
        } else {
            Decimal::ZERO
        };
        let entry = by_cnp
            .entry(cnp.clone())
            .or_insert_with(|| D205Beneficiary {
                cnp: cnp.clone(),
                name: String::new(),
                baza: Decimal::ZERO,
                impozit: Decimal::ZERO,
                distribuit: Decimal::ZERO,
                platit: Decimal::ZERO,
                resident: true,
            });
        entry.baza += gross;
        entry.impozit += tax;
        entry.distribuit += gross;
        entry.platit += paid;
        if entry.name.is_empty() {
            if let Some(n) = d.shareholder.as_deref() {
                entry.name = n.trim().to_string();
            }
        }
    }
    // den1 (numele beneficiarului) e câmp obligatoriu în D205 — un nume gol produce o declarație
    // respinsă/neconformă. Cerem completarea înainte de export (ca și la CNP).
    let nameless: Vec<&str> = by_cnp
        .values()
        .filter(|b| b.name.trim().is_empty())
        .map(|b| b.cnp.as_str())
        .collect();
    if !nameless.is_empty() {
        return Err(AppError::Validation(format!(
            "D205 {year}: {} beneficiar(i) fără nume — completați numele beneficiarului (câmp \
             obligatoriu) înainte de export. CNP: {}.",
            nameless.len(),
            nameless.join(", ")
        )));
    }
    Ok(by_cnp.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_16_from_2026_else_10() {
        assert_eq!(dividend_tax_rate("2026-01-01", false), 16);
        assert_eq!(dividend_tax_rate("2026-06-15", false), 16);
        assert_eq!(dividend_tax_rate("2025-12-31", false), 10); // distribuit în 2025 → 10%
                                                                // Tranzitoriu: situații interimare 2025, chiar dacă distribuirea/plata e în 2026 → 10%.
        assert_eq!(dividend_tax_rate("2026-03-10", true), 10);
    }

    #[test]
    fn tax_amounts_round_and_balance() {
        let gross = round2(Decimal::from_str("10000").unwrap());
        let tax = round2(gross * Decimal::new(16, 2));
        let net = gross - tax;
        assert_eq!(tax, Decimal::from_str("1600.00").unwrap());
        assert_eq!(net, Decimal::from_str("8400.00").unwrap());
        assert_eq!(net + tax, gross); // nota 117/457/446 e echilibrată
    }

    #[test]
    fn deadline_25th_of_month_after_payment_or_jan() {
        // Plătit în iunie 2026 → 25 iulie 2026.
        assert_eq!(
            dividend_tax_deadline("2026-06-01", Some("2026-06-20")),
            "2026-07-25"
        );
        // Plătit în decembrie → 25 ianuarie anul următor.
        assert_eq!(
            dividend_tax_deadline("2026-12-05", Some("2026-12-30")),
            "2027-01-25"
        );
        // Distribuit dar neplătit → 25 ianuarie a anului următor anului distribuirii.
        assert_eq!(dividend_tax_deadline("2026-09-10", None), "2027-01-25");
        assert_eq!(dividend_tax_deadline("2026-09-10", Some("")), "2027-01-25");
    }

    #[tokio::test]
    async fn create_posts_balanced_117_457_446_and_delete_clears_it() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        // gl_journal.company_id are FK către companies → seed o firmă.
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co-1','12345678','Test SRL','Str 1','Bucuresti','B','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let d = create(
            &pool,
            DividendInput {
                company_id: "co-1".into(),
                distribution_date: "2026-03-15".into(),
                payment_date: Some("2026-03-20".into()),
                gross_amount: "10000".into(),
                interim_2025: false,
                shareholder: Some("Asociat A".into()),
                beneficiary_cnp: Some("1960101410019".into()),
                beneficiary_resident: true,
                beneficiary_type: None,
                note: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(d.tax_rate, 16);
        assert_eq!(d.tax_amount, "1600.00");
        assert_eq!(d.net_amount, "8400.00");
        assert_eq!(d.tax_deadline, "2026-04-25"); // plătit în martie → 25 aprilie
        assert_eq!(d.beneficiary_cnp.as_deref(), Some("1960101410019")); // CNP round-trips
        assert!(d.beneficiary_resident);

        // Nota GL: 3 linii (117/457/446), echilibrată, total debit = brutul.
        let row = sqlx::query(
            "SELECT COALESCE(SUM(CAST(e.debit AS REAL)),0) AS d, \
                    COALESCE(SUM(CAST(e.credit AS REAL)),0) AS c, COUNT(*) AS n \
             FROM gl_entry e JOIN gl_journal j ON e.journal_pk = j.id \
             WHERE j.company_id='co-1' AND j.source_type='DIVIDEND' AND j.source_id=?1",
        )
        .bind(&d.id)
        .fetch_one(&pool)
        .await
        .unwrap();
        let (sd, sc, n): (f64, f64, i64) = (row.get("d"), row.get("c"), row.get("n"));
        assert_eq!(n, 3, "nota are 3 linii (117/457/446)");
        assert!((sd - sc).abs() < 0.005, "nota GL trebuie echilibrată");
        assert!((sd - 10000.0).abs() < 0.005, "debit total = brutul");

        // Ștergerea înlătură și nota contabilă.
        delete(&pool, &d.id, "co-1").await.unwrap();
        let after: i64 =
            sqlx::query("SELECT COUNT(*) AS n FROM gl_journal WHERE source_type='DIVIDEND'")
                .fetch_one(&pool)
                .await
                .unwrap()
                .get("n");
        assert_eq!(after, 0, "ștergerea dividendului curăță nota GL");
    }

    #[tokio::test]
    async fn update_beneficiary_sets_cnp_in_place_and_unblocks_d205() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co-1','12345678','Test SRL','Str 1','Bucuresti','B','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();
        // DIV-01: dividend rezident creat FĂRĂ CNP — se înregistrează, dar blochează exportul D205.
        let d = create(
            &pool,
            DividendInput {
                company_id: "co-1".into(),
                distribution_date: "2026-03-15".into(),
                payment_date: Some("2026-03-20".into()),
                gross_amount: "10000".into(),
                interim_2025: false,
                shareholder: None,
                beneficiary_cnp: None,
                beneficiary_resident: true,
                beneficiary_type: None,
                note: None,
            },
        )
        .await
        .unwrap();
        assert!(d.beneficiary_cnp.is_none());
        assert!(
            d205_beneficiaries_for_year(&list(&pool, "co-1").await.unwrap(), 2026).is_err(),
            "D205 2026 e blocat cât timp rezidentul nu are CNP"
        );

        // CNP invalid (cifră de control greșită) e respins de update.
        assert!(update_beneficiary(
            &pool,
            DividendBeneficiaryUpdate {
                id: d.id.clone(),
                company_id: "co-1".into(),
                payment_date: Some("2026-03-20".into()),
                shareholder: Some("Asociat A".into()),
                beneficiary_cnp: Some("1960101410018".into()),
                beneficiary_resident: true,
                beneficiary_type: None,
                note: None,
            },
        )
        .await
        .is_err());

        // Corectare in-place: CNP valid + nume.
        let upd = update_beneficiary(
            &pool,
            DividendBeneficiaryUpdate {
                id: d.id.clone(),
                company_id: "co-1".into(),
                payment_date: Some("2026-03-20".into()),
                shareholder: Some("Asociat A".into()),
                beneficiary_cnp: Some("1960101410019".into()),
                beneficiary_resident: true,
                beneficiary_type: None,
                note: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(upd.beneficiary_cnp.as_deref(), Some("1960101410019"));
        assert_eq!(upd.shareholder.as_deref(), Some("Asociat A"));
        // Sumele rămân imuabile pe acest drum (brut/impozit postează GL).
        assert_eq!(upd.gross_amount, d.gross_amount);
        assert_eq!(upd.tax_amount, d.tax_amount);

        // D205 2026 acum TRECE (un beneficiar).
        let bens = d205_beneficiaries_for_year(&list(&pool, "co-1").await.unwrap(), 2026).unwrap();
        assert_eq!(bens.len(), 1);

        // Nota GL e NESCHIMBATĂ: tot 3 linii, debit total = brutul.
        let row = sqlx::query(
            "SELECT COUNT(*) AS n, COALESCE(SUM(CAST(e.debit AS REAL)),0) AS d \
             FROM gl_entry e JOIN gl_journal j ON e.journal_pk = j.id \
             WHERE j.company_id='co-1' AND j.source_type='DIVIDEND' AND j.source_id=?1",
        )
        .bind(&d.id)
        .fetch_one(&pool)
        .await
        .unwrap();
        let (n, sd): (i64, f64) = (row.get("n"), row.get("d"));
        assert_eq!(n, 3);
        assert!((sd - 10000.0).abs() < 0.005);

        // Cross-company: id corect, firmă greșită → NotFound (izolare).
        assert!(update_beneficiary(
            &pool,
            DividendBeneficiaryUpdate {
                id: d.id.clone(),
                company_id: "other-co".into(),
                payment_date: None,
                shareholder: None,
                beneficiary_cnp: Some("1960101410019".into()),
                beneficiary_resident: true,
                beneficiary_type: None,
                note: None,
            },
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn obligations_grouped_by_deadline_month_for_quarter() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co-1','12345678','Test SRL','Str 1','Bucuresti','B','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Două distribuiri plătite în iunie → ambele scadente 25.07 (deci aceeași lună de scadență,
        // agregate într-o singură obligație), plus una plătită în iulie → scadentă 25.08.
        for pay in ["2026-06-10", "2026-06-20"] {
            create(
                &pool,
                DividendInput {
                    company_id: "co-1".into(),
                    distribution_date: "2026-06-01".into(),
                    payment_date: Some(pay.into()),
                    gross_amount: "10000".into(),
                    interim_2025: false,
                    shareholder: Some("Asociat".into()),
                    beneficiary_cnp: None,
                    beneficiary_resident: true,
                    beneficiary_type: None,
                    note: None,
                },
            )
            .await
            .unwrap();
        }
        create(
            &pool,
            DividendInput {
                company_id: "co-1".into(),
                distribution_date: "2026-07-01".into(),
                payment_date: Some("2026-07-15".into()),
                gross_amount: "5000".into(),
                interim_2025: false,
                shareholder: Some("Asociat".into()),
                beneficiary_cnp: None,
                beneficiary_resident: true,
                beneficiary_type: None,
                note: None,
            },
        )
        .await
        .unwrap();

        // Trimestrul III (iul-aug-sep): lunile de scadență 2026-07, 2026-08, 2026-09.
        let months = vec![
            "2026-07".to_string(),
            "2026-08".to_string(),
            "2026-09".to_string(),
        ];
        let obls = dividend_obligations_in_months(&pool, "co-1", &months)
            .await
            .unwrap();
        assert_eq!(
            obls.len(),
            2,
            "două luni de scadență cu impozit (iul + aug)"
        );
        // 25.07: 2 × (10000 × 16%) = 2 × 1600 = 3200, count 2.
        assert_eq!(obls[0].deadline, "25.07.2026");
        assert_eq!(obls[0].amount, "3200.00");
        assert_eq!(obls[0].count, 2);
        assert!(obls[0].label.contains("dividende"));
        // 25.08: 5000 × 16% = 800, count 1.
        assert_eq!(obls[1].deadline, "25.08.2026");
        assert_eq!(obls[1].amount, "800.00");
        assert_eq!(obls[1].count, 1);
        // Toate sunt PF (implicit) → cod 604.
        assert!(obls.iter().all(|o| o.cod_oblig == "604"));
    }

    #[tokio::test]
    async fn obligations_split_pf_604_and_pj_150() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co-1','12345678','Test SRL','Str 1','Bucuresti','B','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();
        // PF + PJ, ambele plătite în iulie → scadență 25.08, dar pe obligații DISTINCTE.
        for ty in [BEN_PF, BEN_PJ] {
            create(
                &pool,
                DividendInput {
                    company_id: "co-1".into(),
                    distribution_date: "2026-07-01".into(),
                    payment_date: Some("2026-07-10".into()),
                    gross_amount: "10000".into(),
                    interim_2025: false,
                    shareholder: Some("Beneficiar".into()),
                    beneficiary_cnp: None,
                    beneficiary_resident: true,
                    beneficiary_type: Some(ty.into()),
                    note: None,
                },
            )
            .await
            .unwrap();
        }
        // Nerezident (PF) plătit aceeași lună → NU intră în obligațiile 604/150 (merge pe cod 631/D207).
        create(
            &pool,
            DividendInput {
                company_id: "co-1".into(),
                distribution_date: "2026-07-01".into(),
                payment_date: Some("2026-07-10".into()),
                gross_amount: "9000".into(),
                interim_2025: false,
                shareholder: Some("Nerezident".into()),
                beneficiary_cnp: None,
                beneficiary_resident: false,
                beneficiary_type: Some(BEN_PF.into()),
                note: None,
            },
        )
        .await
        .unwrap();
        let obls = dividend_obligations_in_months(&pool, "co-1", &["2026-08".to_string()])
            .await
            .unwrap();
        // Două creanțe distincte (PF înaintea PJ), fiecare 10000 × 16% = 1600. Nerezidentul e exclus
        // (altfel PF ar fi 1600 + 1440 = 3040).
        assert_eq!(obls.len(), 2, "PF (604) + PJ (150); nerezidentul exclus");
        assert_eq!(obls[0].cod_oblig, "604");
        assert!(obls[0].label.contains("persoanelor fizice"));
        assert_eq!(obls[0].amount, "1600.00");
        assert_eq!(obls[1].cod_oblig, "150");
        assert!(obls[1].label.contains("persoanelor juridice"));
        assert_eq!(obls[1].amount, "1600.00");
    }

    // ── D205 aggregation (pure) ──────────────────────────────────────────────
    fn mk_div(
        cnp: Option<&str>,
        resident: bool,
        dist_date: &str,
        pay_date: Option<&str>,
        gross: &str,
        tax: &str,
    ) -> Dividend {
        Dividend {
            id: "x".into(),
            company_id: "co-1".into(),
            distribution_date: dist_date.into(),
            payment_date: pay_date.map(|s| s.into()),
            gross_amount: gross.into(),
            tax_rate: 16,
            tax_amount: tax.into(),
            net_amount: "0".into(),
            interim_2025: false,
            shareholder: Some("Ion Gheorghe".into()),
            beneficiary_cnp: cnp.map(|s| s.into()),
            beneficiary_resident: resident,
            beneficiary_type: BEN_PF.into(),
            note: None,
            tax_deadline: "2026-01-25".into(),
        }
    }

    #[test]
    fn d205_aggregates_by_cnp_excludes_nonresidents_and_other_years() {
        let divs = vec![
            mk_div(
                Some("1900101410011"),
                true,
                "2025-03-01",
                Some("2025-03-10"),
                "10000",
                "1600",
            ),
            mk_div(
                Some("1900101410011"),
                true,
                "2025-06-01",
                Some("2025-06-10"),
                "5000",
                "800",
            ), // same CNP → merge
            mk_div(
                Some("1960101410019"),
                true,
                "2025-07-01",
                None,
                "4000",
                "640",
            ), // diff CNP, UNPAID
            mk_div(
                Some("1800101410010"),
                false,
                "2025-08-01",
                Some("2025-08-10"),
                "9000",
                "1440",
            ), // non-resident → D207
            mk_div(
                Some("1900101410011"),
                true,
                "2024-12-01",
                Some("2024-12-10"),
                "999",
                "159",
            ), // 2024 → excluded
        ];
        let bens = d205_beneficiaries_for_year(&divs, 2025).unwrap();
        assert_eq!(bens.len(), 2, "two resident CNPs in 2025 (sorted by CNP)");
        // 1900… first (BTreeMap order): merged 10000+5000.
        assert_eq!(bens[0].cnp, "1900101410011");
        assert_eq!(bens[0].baza, Decimal::from_str("15000").unwrap());
        assert_eq!(bens[0].impozit, Decimal::from_str("2400").unwrap());
        assert_eq!(bens[0].distribuit, Decimal::from_str("15000").unwrap());
        assert_eq!(bens[0].platit, Decimal::from_str("15000").unwrap()); // both paid
                                                                         // 1960… UNPAID → divid_P = 0 but baza/distribuit still the gross.
        assert_eq!(bens[1].cnp, "1960101410019");
        assert_eq!(bens[1].baza, Decimal::from_str("4000").unwrap());
        assert_eq!(bens[1].distribuit, Decimal::from_str("4000").unwrap());
        assert_eq!(bens[1].platit, Decimal::ZERO);
    }

    #[test]
    fn d205_resident_without_cnp_is_blocked() {
        let divs = vec![
            mk_div(
                Some("1900101410011"),
                true,
                "2025-03-01",
                Some("2025-03-10"),
                "10000",
                "1600",
            ),
            mk_div(None, true, "2025-04-01", Some("2025-04-10"), "5000", "800"), // resident, NO CNP
        ];
        match d205_beneficiaries_for_year(&divs, 2025).unwrap_err() {
            AppError::Validation(m) => assert!(m.contains("fără CNP"), "got: {m}"),
            other => panic!("expected Validation, got {other:?}"),
        }
    }

    #[test]
    fn d205_resident_without_name_is_blocked() {
        // Rezident cu CNP dar FĂRĂ nume (den1) → blocat (câmp obligatoriu ANAF), nu emis cu nume gol.
        let mut d = mk_div(
            Some("1900101410011"),
            true,
            "2025-03-01",
            Some("2025-03-10"),
            "10000",
            "1600",
        );
        d.shareholder = None;
        match d205_beneficiaries_for_year(&[d], 2025).unwrap_err() {
            AppError::Validation(m) => assert!(m.contains("fără nume"), "got: {m}"),
            other => panic!("expected Validation, got {other:?}"),
        }
    }

    #[test]
    fn d205_empty_when_no_residents_in_year() {
        // A non-resident (→ D207) + a resident in the wrong year → no D205 rows, NO error.
        let divs = vec![
            mk_div(
                Some("1800101410010"),
                false,
                "2025-08-01",
                Some("2025-08-10"),
                "9000",
                "1440",
            ),
            mk_div(
                Some("1900101410011"),
                true,
                "2024-01-01",
                Some("2024-01-10"),
                "1000",
                "160",
            ),
        ];
        assert!(d205_beneficiaries_for_year(&divs, 2025).unwrap().is_empty());
    }

    #[test]
    fn d205_excludes_legal_person_beneficiaries() {
        // PF rezident → în D205; PJ rezidentă (art. 43) → EXCLUSĂ (impozitul ei e pe D100 cod 150).
        let pf = mk_div(
            Some("1900101410011"),
            true,
            "2025-03-01",
            Some("2025-03-10"),
            "10000",
            "1600",
        );
        let mut pj = mk_div(
            Some("1900101410011"),
            true,
            "2025-04-01",
            Some("2025-04-10"),
            "5000",
            "800",
        );
        pj.beneficiary_type = BEN_PJ.into();
        let bens = d205_beneficiaries_for_year(&[pf, pj], 2025).unwrap();
        assert_eq!(bens.len(), 1, "doar PF intră în D205");
        assert_eq!(bens[0].distribuit, Decimal::from(10000)); // PJ (5000) exclusă
    }
}
