//! Dividende repartizate + impozit pe dividende (Legea 141/2025): **16%** pentru dividende DISTRIBUITE
//! de la 01.01.2026; **10%** tranzitoriu pentru distribuiri anterioare SAU pentru dividende din
//! situații financiare interimare întocmite în 2025 (chiar dacă plata e în 2026). Fiecare înregistrare
//! postează nota contabilă **117 / 457 / 446** (idempotent) și expune obligația pentru Declarația 100,
//! scadentă pe 25 a lunii următoare PLĂȚII (ori 25 ianuarie pentru dividende distribuite, neplătite).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::collections::BTreeMap;
use std::str::FromStr;

use crate::db::invoices::round2;
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
    // DIV-01: cota e bracketată pe data DISTRIBUIRII (comparare lexicografică ISO = cronologică), ca o
    // distribuire retroactivă (corecție/back-date) să primească cota anului ei, nu fallback-ul 10%:
    // 16% de la 01.01.2026 (Legea 141/2025); 10% în 2025 (OUG 156/2024); 8% în 2023-2024 (Legea
    // 142/2022); 5% până în 2022 inclusiv.
    match distribution_date {
        d if d >= "2026-01-01" => 16,
        d if d >= "2025-01-01" => 10,
        d if d >= "2023-01-01" => 8,
        _ => 5,
    }
}

/// Termenul de plată/declarare a impozitului pe dividende (Cod fiscal art. 43(2)/97(7)/224(4)): 25 a
/// lunii următoare celei în care s-a făcut PLATA; pentru dividende distribuite dar NEPLĂTITE până la
/// finalul anului, 25 ianuarie a anului următor anului distribuirii. Întoarce ISO `YYYY-MM-DD`.
///
/// FIX 4 (audit wave 3, P2): art. 97(7) plafonează termenul la 25 ianuarie anul următor
/// distribuirii — indiferent de CÂND se face efectiv plata ulterioară. Dacă plata are loc
/// într-un an calendaristic ULTERIOR anului distribuirii (dividend distribuit dar neplătit la
/// 31.12, apoi plătit mai târziu), termenul rămâne plafonat la 25 ianuarie (distribution_year+1)
/// — NU se recalculează pe baza lunii plății efective (ceea ce ar amâna nelegal scadența).
pub fn dividend_tax_deadline(distribution_date: &str, payment_date: Option<&str>) -> String {
    use chrono::Datelike;
    let distribution_year = distribution_date
        .get(0..4)
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0);
    let unpaid_by_year_end_deadline = format!("{:04}-01-25", distribution_year + 1);

    if let Some(pd) = payment_date.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(pd, "%Y-%m-%d") {
            // Plata într-un an ULTERIOR distribuirii ⇒ dividendul era deja neplătit la 31.12 al
            // anului distribuirii, deci termenul legal (25 ian. anul următor distribuirii) a
            // trecut deja — plafonăm aici, nu-l "amânăm" pe baza lunii plății reale.
            if d.year() > distribution_year {
                return unpaid_by_year_end_deadline;
            }
            let (y, m) = if d.month() == 12 {
                (d.year() + 1, 1)
            } else {
                (d.year(), d.month() + 1)
            };
            return format!("{y:04}-{m:02}-25");
        }
    }
    unpaid_by_year_end_deadline
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
    /// `Stat_R` D207 — codul de țară (2 litere, nomenclator ANAF) al beneficiarului NEREZIDENT.
    /// Relevant doar când `beneficiary_resident = false`; cerut la exportul D207.
    pub beneficiary_country: Option<String>,
    /// `cifS` D207 — codul fiscal din străinătate (NIF) al beneficiarului nerezident (opțional).
    pub beneficiary_foreign_tax_id: Option<String>,
    /// Scutire art. 43 alin. (4) Cod fiscal: dividendele plătite unei persoane juridice ROMÂNE care
    /// deține ≥10% din capital de ≥1 an sunt SCUTITE de impozit. Utilizatorul ATESTĂ condiția de
    /// participație; valabil doar pentru beneficiar rezident de tip PJ. Când e setat: cotă 0,
    /// impozit 0, nota GL fără rândul 446 (457 creditat cu brutul întreg) și distribuirea NU intră
    /// în obligația D100 cod 150.
    pub exempt_art43_4: bool,
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
    /// D207 (nerezidenți): țara de rezidență (Stat_R) + codul fiscal străin (cifS), opționale la creare.
    #[serde(default)]
    pub beneficiary_country: Option<String>,
    #[serde(default)]
    pub beneficiary_foreign_tax_id: Option<String>,
    /// Scutire art. 43 alin. (4) — vezi [`Dividend::exempt_art43_4`]. Acceptată DOAR pentru
    /// beneficiar rezident de tip PJ (altfel Validation error). Afectează sumele + GL, deci nu e
    /// editabilă in-place (ca brutul/impozitul): delete + recreate.
    #[serde(default)]
    pub exempt_art43_4: bool,
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
    #[serde(default)]
    pub beneficiary_country: Option<String>,
    #[serde(default)]
    pub beneficiary_foreign_tax_id: Option<String>,
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
        beneficiary_country: r.get("beneficiary_country"),
        beneficiary_foreign_tax_id: r.get("beneficiary_foreign_tax_id"),
        exempt_art43_4: r.get::<i64, _>("exempt_art43_4") != 0,
        note: r.get("note"),
        tax_deadline,
    }
}

const SELECT: &str =
    "SELECT id, company_id, distribution_date, payment_date, gross_amount, tax_rate, \
     tax_amount, net_amount, interim_2025, shareholder, beneficiary_cnp, beneficiary_resident, \
     beneficiary_type, beneficiary_country, beneficiary_foreign_tax_id, exempt_art43_4, note \
     FROM dividends";

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
    // Tip beneficiar: "PJ" doar dacă e cerut explicit; orice altceva → "PF" (implicit, cazul uzual).
    let ben_type = if input.beneficiary_type.as_deref() == Some(BEN_PJ) {
        BEN_PJ
    } else {
        BEN_PF
    };
    // W8-1: scutirea art. 43 alin. (4) se aplică DOAR dividendelor către o persoană juridică
    // ROMÂNĂ (rezidentă) cu participație ≥10% deținută ≥1 an (condiție atestată de utilizator).
    // PF-urile (art. 97) și nerezidenții (art. 224 / D207) nu au această scutire pe acest drum.
    if input.exempt_art43_4 && (ben_type != BEN_PJ || !input.beneficiary_resident) {
        return Err(AppError::Validation(
            "Scutirea art. 43 alin. (4) se aplică doar dividendelor către o persoană juridică \
             rezidentă (participație ≥10% deținută ≥1 an)."
                .into(),
        ));
    }
    let rate = if input.exempt_art43_4 {
        0 // scutit art. 43(4): cotă 0, impozit 0
    } else {
        dividend_tax_rate(date, input.interim_2025)
    };
    let tax = round2(gross * Decimal::new(rate, 2));
    let net = gross - tax; // ambele 2dp → diferența e exactă, deci nota e echilibrată
                           // D207 (nerezidenți): țara (Stat_R) + codul fiscal străin (cifS), opționale la creare.
    let ben_country = input
        .beneficiary_country
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let ben_foreign = input
        .beneficiary_foreign_tax_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let id = new_id();
    sqlx::query(
        "INSERT INTO dividends (id, company_id, distribution_date, payment_date, gross_amount, \
         tax_rate, tax_amount, net_amount, interim_2025, shareholder, beneficiary_cnp, \
         beneficiary_resident, beneficiary_type, beneficiary_country, beneficiary_foreign_tax_id, \
         exempt_art43_4, note, created_at) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
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
    .bind(ben_country)
    .bind(ben_foreign)
    .bind(input.exempt_art43_4 as i64)
    .bind(input.note.as_deref())
    .bind(now_unix())
    .execute(pool)
    .await?;

    // Nota contabilă: D 117 (rezultat reportat) brut; C 457 (dividende de plată) net; C 446 (impozit
    // pe dividende) impozit. Σdebit (brut) = Σcredit (net + impozit). Idempotent per dividend id.
    // W8-1: la scutirea art. 43(4) NU există impozit reținut → nota are DOAR 117 / 457 (457 creditat
    // cu întregul brut, net = brut); rândul 446 e absent, nu un rând cu 0.
    let desc = if input.exempt_art43_4 {
        format!("Repartizare dividende {date} (scutit art. 43 alin. (4) C.fisc.)")
    } else {
        format!("Repartizare dividende {date} (impozit {rate}%)")
    };
    let lines_exempt = [("117", gross, Decimal::ZERO), ("457", Decimal::ZERO, net)];
    let lines_taxed = [
        ("117", gross, Decimal::ZERO),
        ("457", Decimal::ZERO, net),
        ("446", Decimal::ZERO, tax),
    ];
    let lines: &[(&str, Decimal, Decimal)] = if input.exempt_art43_4 {
        &lines_exempt
    } else {
        &lines_taxed
    };
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
            partner_cui: None,
        },
        lines,
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
    let ben_country = upd
        .beneficiary_country
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let ben_foreign = upd
        .beneficiary_foreign_tax_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let res = sqlx::query(
        "UPDATE dividends SET payment_date = ?3, shareholder = ?4, beneficiary_cnp = ?5, \
         beneficiary_resident = ?6, beneficiary_type = ?7, note = ?8, \
         beneficiary_country = ?9, beneficiary_foreign_tax_id = ?10 \
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
    .bind(ben_country)
    .bind(ben_foreign)
    .execute(pool)
    .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound); // cross-company / id inexistent
    }
    get(pool, &upd.id, &upd.company_id).await
}

pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    // Period-lock guard (OMFP 2634/2015 immutability; QA follow-up to the v0.7.4 audit):
    // deleting a dividend drops its DIVIDEND journal (117/457/446); if that month is FILED
    // (locked), declared figures would change silently. Refuse when ANY month is locked.
    let months: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT substr(transaction_date,1,7) FROM gl_journal \
         WHERE company_id=?1 AND source_type='DIVIDEND' AND source_id=?2",
    )
    .bind(company_id)
    .bind(id)
    .fetch_all(pool)
    .await?;
    for ym in &months {
        if crate::db::period_locks::is_period_locked(pool, company_id, ym).await? {
            return Err(AppError::Validation(format!(
                "Perioada {ym} este blocată (declarație depusă) — dividendul nu poate fi șters. \
                 Deblocați perioada pentru a înregistra o corecție."
            )));
        }
    }

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
/// din Declarația 100. `period_ym` = `YYYY-MM`. Filtrează NUMAI rezidenți (ca și
/// `dividend_obligations_in_months`) — impozitul pe nerezidenți se declară prin D207, nu D100.
pub async fn dividend_tax_due_in_period(
    pool: &SqlitePool,
    company_id: &str,
    period_ym: &str,
) -> AppResult<Decimal> {
    let mut total = Decimal::ZERO;
    for d in list(pool, company_id).await? {
        // W8-1: scutirea art. 43(4) nu generează impozit (tax_amount e deja 0) — o excludem
        // explicit ca filtrul să rămână corect chiar dacă suma ar fi vreodată nenulă.
        if d.tax_deadline.starts_with(period_ym) && d.beneficiary_resident && !d.exempt_art43_4 {
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
/// W8-2 — Obligația D100 pentru impozitul pe dividende reținut NEREZIDENȚILOR (art. 224 Cod fiscal):
/// cod_oblig 631 („Impozit pe veniturile din dividende obținute din România de persoane nerezidente").
/// VERIFICAT în nomenclatorul îmbarcat al DUK D100Validator (Parameters: cod 631, aceeași tabelă din
/// care provin 604/150). Rând pur INFORMATIV, ca și 604/150: impozitul se declară lunar pe D100
/// cod 631, iar anual informativ prin D207.
pub const D100_DIVIDEND_NONREZ_COD: &str = "631";
pub const D100_DIVIDEND_NONREZ_LABEL: &str = "Impozit dividende nerezidenți (D207)";

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
/// scadență, tip beneficiar): rânduri SEPARATE pentru PF (cod 604, art. 97), PJ (cod 150, art. 43) și
/// — W8-2 — NEREZIDENȚI (cod 631, art. 224, raportare anuală prin D207), fiindcă sunt creanțe
/// distincte în Nomenclator și se declară pe poziții diferite. O linie per (lună, tip) cu cel puțin o
/// distribuire. Scutirile art. 43(4) (W8-1) nu produc rânduri. Datele ISO se compară lexicografic.
pub async fn dividend_obligations_in_months(
    pool: &SqlitePool,
    company_id: &str,
    months: &[String],
) -> AppResult<Vec<DividendObligation>> {
    /// Bucket-ul de agregare: PF/PJ rezidente (604/150) + nerezidenți (631, W8-2).
    #[derive(PartialEq, Clone, Copy)]
    enum Bucket {
        ResidentPf,
        ResidentPj,
        NonResident,
    }
    let all = list(pool, company_id).await?;
    let mut out = Vec::new();
    for ym in months {
        let (y, m) = ym.split_once('-').unwrap_or(("", ""));
        // Ordine stabilă: PF, PJ, apoi nerezidenți (W8-2 — rând informativ separat, cod 631 + D207).
        for (bucket, cod, label) in [
            (
                Bucket::ResidentPf,
                D100_DIVIDEND_PF_COD,
                D100_DIVIDEND_PF_LABEL,
            ),
            (
                Bucket::ResidentPj,
                D100_DIVIDEND_PJ_COD,
                D100_DIVIDEND_PJ_LABEL,
            ),
            (
                Bucket::NonResident,
                D100_DIVIDEND_NONREZ_COD,
                D100_DIVIDEND_NONREZ_LABEL,
            ),
        ] {
            let mut sum = Decimal::ZERO;
            let mut count = 0u32;
            for d in &all {
                // 604/150 = doar rezidenți; impozitul reținut nerezidenților merge pe rândul lui
                // (cod 631) + raportarea anuală D207 — nu pe 604/150.
                // W8-1: distribuirile scutite art. 43(4) NU au impozit de declarat → nu produc
                // (nici măcar cu suma 0) un rând de obligație cod 150.
                let in_bucket = match bucket {
                    Bucket::ResidentPf => {
                        d.beneficiary_resident && !d.exempt_art43_4 && d.beneficiary_type != BEN_PJ
                    }
                    Bucket::ResidentPj => {
                        d.beneficiary_resident && !d.exempt_art43_4 && d.beneficiary_type == BEN_PJ
                    }
                    Bucket::NonResident => !d.beneficiary_resident,
                };
                if d.tax_deadline.starts_with(ym.as_str()) && in_bucket {
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

/// Agregă dividendele NEREZIDENTE ale anului de venit `year` în rânduri D207 (`<benef>`), grupate pe
/// (țară de rezidență, identitate). Identitatea = `cifS` (cod fiscal străin), altfel `cifR` (cod RO),
/// altfel numele. `tip_venit1 = "01"` (dividende impozabile, art. 223), `Act_N = 1` (Cod fiscal) —
/// cazul intern uzual; varianta cu convenție (22 / Act_N 2) se va selecta când modelul o va captura.
/// Eroare dacă un nerezident nu are nume (`den1`) sau țară (`Stat_R`). Funcție PURĂ (testabilă).
pub fn d207_beneficiaries_for_year(
    dividends: &[Dividend],
    year: i32,
) -> AppResult<Vec<crate::anaf_decl::d207_xml::D207Benef>> {
    use crate::anaf_decl::d207_xml::D207Benef;

    let year_str = format!("{year:04}");
    struct Acc {
        name: String,
        stat_r: String,
        cif_r: Option<String>,
        cif_s: Option<String>,
        baza: Decimal,
        imp: Decimal,
    }
    let mut by: BTreeMap<(String, String), Acc> = BTreeMap::new();
    for d in dividends
        .iter()
        .filter(|d| !d.beneficiary_resident && d.distribution_date.starts_with(&year_str))
    {
        let name = d
            .shareholder
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                AppError::Validation(
                    "D207: un beneficiar nerezident nu are nume (den1) — completați-l.".into(),
                )
            })?
            .to_string();
        let stat_r = d
            .beneficiary_country
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                AppError::Validation(format!(
                    "D207: beneficiarul nerezident „{name}” nu are țara de rezidență (Stat_R) — \
                     completați codul de țară (ex. DE, FR, NL)."
                ))
            })?
            .to_uppercase();
        let cif_s = d
            .beneficiary_foreign_tax_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let cif_r = d
            .beneficiary_cnp
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let ident = cif_s
            .clone()
            .or_else(|| cif_r.clone())
            .unwrap_or_else(|| name.clone());
        let acc = by.entry((stat_r.clone(), ident)).or_insert_with(|| Acc {
            name: name.clone(),
            stat_r: stat_r.clone(),
            cif_r: cif_r.clone(),
            cif_s: cif_s.clone(),
            baza: Decimal::ZERO,
            imp: Decimal::ZERO,
        });
        acc.baza += Decimal::from_str(d.gross_amount.trim()).unwrap_or(Decimal::ZERO);
        acc.imp += Decimal::from_str(d.tax_amount.trim()).unwrap_or(Decimal::ZERO);
    }
    Ok(by
        .into_values()
        .map(|a| D207Benef {
            tip_venit: "01".into(),
            name: a.name,
            stat_r: a.stat_r,
            cif_r: a.cif_r,
            cif_s: a.cif_s,
            baza: a.baza,
            impozit: a.imp,
            impozit_suportat: Decimal::ZERO,
            act_n: 1,
        })
        .collect())
}

/// Agregă dividendele REZIDENTE cu data distribuirii în anul de venit `year` în rânduri D205 (un
/// `<benef>` per CNP). Nerezidenții sunt EXCLUȘI (se raportează separat în D207). `baza1`/`divid_D` =
/// brutul; `imp1` = impozitul; `divid_P` (dividende plătite = NET) e derivat la emitere ca baza −
/// impozit (OPANAF 154/2024 — sumele plătite asociatului sunt NETE). Întoarce Err dacă
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
        let entry = by_cnp
            .entry(cnp.clone())
            .or_insert_with(|| D205Beneficiary {
                cnp: cnp.clone(),
                name: String::new(),
                baza: Decimal::ZERO,
                impozit: Decimal::ZERO,
                distribuit: Decimal::ZERO,
                resident: true,
            });
        entry.baza += gross;
        entry.impozit += tax;
        // divid_D = brut distribuit. divid_P (dividende plătite = NET) e derivat la emitere ca
        // baza − impozit (OPANAF 154/2024), deci nu îl mai acumulăm aici pe bază de dată-plată.
        entry.distribuit += gross;
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
    fn rate_brackets_pre_2025_distributions_dx() {
        // DIV-01: o distribuire retroactivă (corecție/back-date) primește cota anului ei, nu 10% fallback.
        assert_eq!(dividend_tax_rate("2025-01-01", false), 10); // 2025 → 10% (OUG 156/2024)
        assert_eq!(dividend_tax_rate("2024-12-31", false), 8); // 2023-2024 → 8% (Legea 142/2022)
        assert_eq!(dividend_tax_rate("2023-01-01", false), 8);
        assert_eq!(dividend_tax_rate("2022-12-31", false), 5); // ≤2022 → 5%
        assert_eq!(dividend_tax_rate("2020-06-01", false), 5);
        // Tranzitoriul interim-2025 rămâne 10% indiferent de data distribuirii.
        assert_eq!(dividend_tax_rate("2022-01-01", true), 10);
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

    /// FIX 4 (audit wave 3, P2): plata într-un an calendaristic ULTERIOR distribuirii trebuie
    /// să rămână plafonată la 25 ianuarie (distribution_year+1) — art. 97(7) — NU amânată pe
    /// baza lunii plății efective. Un dividend distribuit în 2025 și plătit abia în martie 2027
    /// era deja restant la 25 ianuarie 2026; termenul NU devine 25 aprilie 2027.
    #[test]
    fn deadline_capped_at_25_jan_when_payment_crosses_into_a_later_year() {
        // Distribuit 2025, plătit martie 2027 (2 ani mai târziu) → plafonat la 25 ian. 2026,
        // NU 25 aprilie 2027 (ceea ce ar rezulta din "25 a lunii următoare plății" fără plafon).
        assert_eq!(
            dividend_tax_deadline("2025-06-15", Some("2027-03-10")),
            "2026-01-25"
        );
        // Distribuit 2025, plătit ianuarie 2026 (imediat anul următor, dar TOT ulterior anului
        // distribuirii) → tot plafonat la 25 ian. 2026 (nu 25 februarie 2026).
        assert_eq!(
            dividend_tax_deadline("2025-11-01", Some("2026-01-15")),
            "2026-01-25"
        );
        // Distribuit 2026, plătit decembrie 2026 (SAME an ca distribuirea) → comportament
        // existent, neschimbat: 25 ianuarie 2027 (25 a lunii următoare lui decembrie).
        assert_eq!(
            dividend_tax_deadline("2026-03-01", Some("2026-12-20")),
            "2027-01-25"
        );
    }

    /// FIX 4: cazul "distribuit dar neplătit la sfârșitul anului" (payment_date=None sau gol)
    /// rămâne plafonat la 25 ianuarie anul următor distribuirii, indiferent de anul distribuirii.
    #[test]
    fn deadline_unpaid_by_year_end_always_25_jan_next_year() {
        assert_eq!(dividend_tax_deadline("2025-01-01", None), "2026-01-25");
        assert_eq!(dividend_tax_deadline("2025-01-01", Some("")), "2026-01-25");
        assert_eq!(
            dividend_tax_deadline("2025-01-01", Some("   ")),
            "2026-01-25"
        );
        // An diferit — plafonul urmează anul distribuirii, nu anul curent.
        assert_eq!(dividend_tax_deadline("2027-06-01", None), "2028-01-25");
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
                beneficiary_country: None,
                beneficiary_foreign_tax_id: None,
                exempt_art43_4: false,
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
                beneficiary_country: None,
                beneficiary_foreign_tax_id: None,
                exempt_art43_4: false,
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
                beneficiary_country: None,
                beneficiary_foreign_tax_id: None,
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
                beneficiary_country: None,
                beneficiary_foreign_tax_id: None,
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
                beneficiary_country: None,
                beneficiary_foreign_tax_id: None,
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
                    beneficiary_country: None,
                    beneficiary_foreign_tax_id: None,
                    exempt_art43_4: false,
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
                beneficiary_country: None,
                beneficiary_foreign_tax_id: None,
                exempt_art43_4: false,
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
                    beneficiary_country: None,
                    beneficiary_foreign_tax_id: None,
                    exempt_art43_4: false,
                    note: None,
                },
            )
            .await
            .unwrap();
        }
        // Nerezident (PF) plătit aceeași lună → NU intră în 604/150; W8-2: primește propriul rând
        // informativ (cod 631, D207).
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
                beneficiary_country: None,
                beneficiary_foreign_tax_id: None,
                exempt_art43_4: false,
                note: None,
            },
        )
        .await
        .unwrap();
        let obls = dividend_obligations_in_months(&pool, "co-1", &["2026-08".to_string()])
            .await
            .unwrap();
        // Trei creanțe distincte (PF, PJ, apoi nerezident). PF/PJ câte 10000 × 16% = 1600 —
        // nerezidentul NU e amestecat în 604 (altfel PF ar fi 1600 + 1440 = 3040); W8-2: el
        // primește propriul rând informativ cod 631 (9000 × 16% = 1440).
        assert_eq!(obls.len(), 3, "PF (604) + PJ (150) + nerezident (631)");
        assert_eq!(obls[0].cod_oblig, "604");
        assert!(obls[0].label.contains("persoanelor fizice"));
        assert_eq!(obls[0].amount, "1600.00");
        assert_eq!(obls[1].cod_oblig, "150");
        assert!(obls[1].label.contains("persoanelor juridice"));
        assert_eq!(obls[1].amount, "1600.00");
        assert_eq!(obls[2].cod_oblig, "631");
        assert!(obls[2].label.contains("nerezidenți"));
        assert_eq!(obls[2].amount, "1440.00");
        assert_eq!(obls[2].count, 1);
    }

    /// W8-1: scutirea art. 43 alin. (4) — dividend către PJ rezidentă cu participație atestată:
    /// cotă 0, impozit 0, net = brut, nota GL DOAR 117/457 (fără rând 446, nu un 446 cu 0), iar
    /// distribuirea NU produce rând de obligație cod 150. PJ-ul NE-scutit rămâne neschimbat.
    #[tokio::test]
    async fn exempt_art43_4_zero_tax_no_446_and_no_obligation_row() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co-1','12345678','Test SRL','Str 1','Bucuresti','B','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mk = |exempt: bool, gross: &str| DividendInput {
            company_id: "co-1".into(),
            distribution_date: "2026-07-01".into(),
            payment_date: Some("2026-07-10".into()),
            gross_amount: gross.into(),
            interim_2025: false,
            shareholder: Some("Holding SRL".into()),
            beneficiary_cnp: None,
            beneficiary_resident: true,
            beneficiary_type: Some(BEN_PJ.into()),
            beneficiary_country: None,
            beneficiary_foreign_tax_id: None,
            exempt_art43_4: exempt,
            note: None,
        };
        // Scutit: cotă 0, impozit 0, net = brut.
        let ex = create(&pool, mk(true, "10000")).await.unwrap();
        assert_eq!(ex.tax_rate, 0);
        assert_eq!(
            Decimal::from_str(&ex.tax_amount).unwrap(),
            Decimal::ZERO,
            "impozit 0 la scutire (got {})",
            ex.tax_amount
        );
        assert_eq!(
            Decimal::from_str(&ex.net_amount).unwrap(),
            Decimal::from(10000),
            "net = brut la scutire (got {})",
            ex.net_amount
        );
        assert!(ex.exempt_art43_4);

        // Nota GL: DOAR 2 linii (117 debit brut / 457 credit brut) — rândul 446 e ABSENT.
        let rows = sqlx::query(
            "SELECT e.account_code AS a, CAST(e.debit AS REAL) AS d, CAST(e.credit AS REAL) AS c \
             FROM gl_entry e JOIN gl_journal j ON e.journal_pk = j.id \
             WHERE j.company_id='co-1' AND j.source_type='DIVIDEND' AND j.source_id=?1 \
             ORDER BY e.account_code",
        )
        .bind(&ex.id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(rows.len(), 2, "nota scutită are DOAR 117/457 (fără 446)");
        let a0: String = rows[0].get("a");
        let a1: String = rows[1].get("a");
        assert_eq!((a0.as_str(), a1.as_str()), ("117", "457"));
        let (d0, c1): (f64, f64) = (rows[0].get("d"), rows[1].get("c"));
        assert!((d0 - 10000.0).abs() < 0.005, "117 debitat cu brutul");
        assert!(
            (c1 - 10000.0).abs() < 0.005,
            "457 creditat cu ÎNTREGUL brut (fără reținere)"
        );

        // Fără rând de obligație cod 150 (și nici 604/631) — nu există impozit de declarat.
        let obls = dividend_obligations_in_months(&pool, "co-1", &["2026-08".to_string()])
            .await
            .unwrap();
        assert!(
            obls.is_empty(),
            "distribuirea scutită art. 43(4) nu produce obligații D100: {obls:?}"
        );
        // ... și nu intră nici în totalul impozitului scadent al perioadei.
        assert_eq!(
            dividend_tax_due_in_period(&pool, "co-1", "2026-08")
                .await
                .unwrap(),
            Decimal::ZERO
        );

        // PJ NE-scutit, aceleași date: comportament neschimbat (16%, rând cod 150).
        let tx = create(&pool, mk(false, "5000")).await.unwrap();
        assert_eq!(tx.tax_rate, 16);
        assert_eq!(tx.tax_amount, "800.00");
        assert!(!tx.exempt_art43_4);
        let obls = dividend_obligations_in_months(&pool, "co-1", &["2026-08".to_string()])
            .await
            .unwrap();
        assert_eq!(obls.len(), 1, "doar PJ-ul ne-scutit produce obligație");
        assert_eq!(obls[0].cod_oblig, "150");
        assert_eq!(obls[0].amount, "800.00");
        assert_eq!(obls[0].count, 1, "scutitul nu e numărat nici cu suma 0");
    }

    /// W8-1: scutirea art. 43(4) e refuzată pentru PF (art. 97) și pentru nerezidenți (art. 224).
    #[tokio::test]
    async fn exempt_art43_4_rejected_for_pf_and_nonresident() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co-1','12345678','Test SRL','Str 1','Bucuresti','B','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let mk = |ty: &str, resident: bool| DividendInput {
            company_id: "co-1".into(),
            distribution_date: "2026-07-01".into(),
            payment_date: None,
            gross_amount: "1000".into(),
            interim_2025: false,
            shareholder: Some("X".into()),
            beneficiary_cnp: None,
            beneficiary_resident: resident,
            beneficiary_type: Some(ty.into()),
            beneficiary_country: None,
            beneficiary_foreign_tax_id: None,
            exempt_art43_4: true,
            note: None,
        };
        for (ty, resident) in [(BEN_PF, true), (BEN_PJ, false), (BEN_PF, false)] {
            match create(&pool, mk(ty, resident)).await.unwrap_err() {
                AppError::Validation(m) => {
                    assert!(m.contains("art. 43"), "got: {m}")
                }
                other => {
                    panic!("expected Validation for ({ty}, resident={resident}), got {other:?}")
                }
            }
        }
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
            beneficiary_country: None,
            beneficiary_foreign_tax_id: None,
            exempt_art43_4: false,
            note: None,
            tax_deadline: "2026-01-25".into(),
        }
    }

    /// A non-resident dividend row for the D207 router tests.
    fn mk_nonresident(
        name: &str,
        country: Option<&str>,
        dist_date: &str,
        gross: &str,
        tax: &str,
    ) -> Dividend {
        Dividend {
            id: "x".into(),
            company_id: "co-1".into(),
            distribution_date: dist_date.into(),
            payment_date: None,
            gross_amount: gross.into(),
            tax_rate: 16,
            tax_amount: tax.into(),
            net_amount: "0".into(),
            interim_2025: false,
            shareholder: Some(name.into()),
            beneficiary_cnp: None,
            beneficiary_resident: false,
            beneficiary_type: BEN_PF.into(),
            beneficiary_country: country.map(|s| s.into()),
            beneficiary_foreign_tax_id: None,
            exempt_art43_4: false,
            note: None,
            tax_deadline: "2026-05-25".into(),
        }
    }

    #[test]
    fn d207_router_filters_non_residents_and_requires_country() {
        // Nerezident FĂRĂ țară → export D207 blocat (Stat_R e obligatoriu).
        let no_country = vec![mk_nonresident(
            "Müller GmbH",
            None,
            "2026-04-10",
            "10000",
            "1600",
        )];
        assert!(d207_beneficiaries_for_year(&no_country, 2026).is_err());

        // Cu țară → câte un rând benef per beneficiar, sumele agregate.
        let ok = vec![
            mk_nonresident("Müller GmbH", Some("DE"), "2026-04-10", "10000", "1600"),
            mk_nonresident("Müller GmbH", Some("de"), "2026-09-10", "2000", "320"), // același benef, sumat
            mk_nonresident("Dupont SA", Some("FR"), "2026-06-01", "5000", "800"),
        ];
        let benefs = d207_beneficiaries_for_year(&ok, 2026).unwrap();
        assert_eq!(benefs.len(), 2, "doi beneficiari distincți (DE + FR)");
        let de = benefs.iter().find(|b| b.stat_r == "DE").unwrap();
        assert_eq!(de.name, "Müller GmbH");
        assert_eq!(de.baza, Decimal::from_str("12000").unwrap()); // 10000+2000 agregat
        assert_eq!(de.tip_venit, "01");
        assert_eq!(de.act_n, 1);

        // Rezidenții sunt EXCLUȘI din D207 (merg în D205).
        let mut res = mk_nonresident("Ionescu", Some("RO"), "2026-03-01", "3000", "480");
        res.beneficiary_resident = true;
        assert!(d207_beneficiaries_for_year(&[res], 2026)
            .unwrap()
            .is_empty());
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
        assert_eq!(bens[1].cnp, "1960101410019");
        assert_eq!(bens[1].baza, Decimal::from_str("4000").unwrap());
        assert_eq!(bens[1].impozit, Decimal::from_str("640").unwrap());
        assert_eq!(bens[1].distribuit, Decimal::from_str("4000").unwrap());

        // divid_P (dividende plătite) = NET = baza − impozit la emitere (OPANAF 154/2024), NU brutul.
        let header = crate::anaf_decl::d205_xml::D205Header {
            cui: "13548146".into(),
            adresa: "Str. Exemplu 1".into(),
            den: "Demo SRL".into(),
            an: 2025,
            d_rec: 0,
            nume_declar: "A".into(),
            prenume_declar: "B".into(),
            functie_declar: "Administrator".into(),
        };
        let xml = crate::anaf_decl::d205_xml::build_d205_xml(&header, &bens).unwrap();
        // divid_D rămâne brut (15000 / 4000); divid_P = NET (15000−2400=12600 / 4000−640=3360).
        assert!(
            xml.contains(r#"divid_D="15000""#) && xml.contains(r#"divid_P="12600""#),
            "{xml}"
        );
        assert!(
            xml.contains(r#"divid_D="4000""#) && xml.contains(r#"divid_P="3360""#),
            "{xml}"
        );
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

    /// DIV-03: dividend_tax_due_in_period trebuie să excludă nerezidenții (ca dividend_obligations_in_months).
    #[test]
    fn tax_due_in_period_excludes_nonresident() {
        // Construim manual dividende (fără DB) folosind structura Dividend.
        let make = |resident: bool, deadline: &str, tax: &str| -> Dividend {
            Dividend {
                id: uuid::Uuid::now_v7().to_string(),
                company_id: "co-1".into(),
                distribution_date: "2026-06-01".into(),
                payment_date: Some("2026-06-15".into()),
                gross_amount: "10000".into(),
                tax_rate: 16,
                tax_amount: tax.into(),
                net_amount: "8400".into(),
                interim_2025: false,
                shareholder: None,
                beneficiary_cnp: None,
                beneficiary_resident: resident,
                beneficiary_type: "PF".into(),
                beneficiary_country: None,
                beneficiary_foreign_tax_id: None,
                exempt_art43_4: false,
                note: None,
                tax_deadline: deadline.into(),
            }
        };
        let resident = make(true, "2026-07", "1600");
        let nonresident = make(false, "2026-07", "1000");
        // Simulăm logica filtrului direct.
        let period = "2026-07";
        let total: Decimal = [&resident, &nonresident]
            .iter()
            .filter(|d| d.tax_deadline.starts_with(period) && d.beneficiary_resident)
            .map(|d| Decimal::from_str(d.tax_amount.trim()).unwrap_or(Decimal::ZERO))
            .sum();
        assert_eq!(
            total,
            Decimal::from_str("1600").unwrap(),
            "nerezidenții nu trebuie incluși"
        );
    }
}
