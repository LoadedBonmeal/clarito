//! Concedii medicale (OUG 158/2005) — registrul certificatelor de concediu medical, sursa blocului
//! D112 `asiguratD` (per certificat) + rollup-ul angajator `angajatorC2` (C2_11 = COUNT, C2_12 = Σ
//! D_16, C2_13 = Σ D_14, C2_14 = Σ D_15, C2_15 = Σ D_20, C2_16 = Σ D_21; recuperarea din FNUASS).
//!
//! Câmpurile derivate determinist (total zile = D_14+D_15; media zilnică = baza/zile_baza) se
//! calculează aici; sumele indemnizațiilor (D_20/D_21) sunt introduse de utilizator (calculul lor
//! din media veniturilor pe 6 luni e o extensie ulterioară). Validarea finală se face în
//! DUKIntegrator înainte de depunere.
//!
//! ## De ce NU emitem încă blocul `asiguratD` în XML-ul D112 (blocaj actualizat 06/2026)
//! Definițiile câmpurilor sunt acum CUNOSCUTE (structura oficială `structura_D112_0126_030226.pdf`,
//! v7): `D_5` = data acordării; `D_9` = cod indemnizație (Nomenclator 9, 01–17/51/91/92); `D_10` =
//! loc prescriere (Nomenclator 8 — dar XSD-ul tipizează `IntInt1_4SType`, deci doar **1–4** sunt
//! acceptate la validare, nu și „5 CEX”); `D_23` = cod boală (3 car., „RM” când D_9=15); `D_16` =
//! D_14+D_15, `D_19` = bază/zile_bază (derivabile). Ce BLOCHEAZĂ emiterea NU mai sunt definițiile,
//! ci IMPOSIBILITATEA DE A VALIDA output-ul înainte de o depunere reală. (a) XSD-ul oficial
//! `d112_10102024.xsd` e malformat — eroare de sintaxă la linia 439 (`name="A_sal1` fără ghilimea
//! de închidere) → nu se încarcă în xmllint → fără validare XSD. (b) Kit-ul DUK livrat NU conține
//! un validator D112 (doar D300/D394/D406) → fără validare de reguli de business. (c) `AsiguratType`
//! folosește `xs:group` (GrupAsiguratGroup) — relația asiguratA↔asiguratD nu e un simplu „sibling”,
//! deci structura trebuie confirmată cu Soft J-ul ANAF.
//! A emite `asiguratD` fără niciun validator ar risca depuneri respinse de ANAF — mai rău decât
//! comportamentul actual sigur (concediile se completează în PDF-ul inteligent ANAF, care le
//! validează). De deblocat când ANAF publică D112Validator/Soft J: atunci capturăm D_5 (modelul îl
//! are deja), D_10 (1–4), D_23 + lărgim selecția D_9, și emitem `asiguratD` + `angajatorC2`.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};
use std::str::FromStr;

use crate::db::models::{new_id, now_unix};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MedicalLeave {
    pub id: String,
    pub company_id: String,
    pub employee_id: String,
    pub period_ym: String,
    pub serie: String,
    pub numar: String,
    pub cod_indemnizatie: String,
    pub data_acordare: String,
    pub data_inceput: String,
    pub data_sfarsit: String,
    pub zile_angajator: i64,
    pub zile_fnuass: i64,
    pub baza_calcul: String,
    pub zile_baza: i64,
    pub suma_angajator: String,
    pub suma_fnuass: String,
    pub procent: i64,
    /// D_10 — locul de prescriere a certificatului (Nomenclator 8). Live XSD types it
    /// `IntInt1_4SType`, so the valid domain is 1–4 (1 medic familie, 2 spital, 3 ambulatoriu, 4 CAS).
    pub loc_prescriere: i64,
    /// D_23 — codul de boală (diagnostic) de pe certificat, max 3 caractere; „RM" pentru risc
    /// maternal (D_9=15). NOTE: structura v7 = D_23, OPANAF 299/2025 = D_22 — confirmat la emitere.
    pub cod_boala: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MedicalLeaveInput {
    pub company_id: String,
    pub employee_id: String,
    pub period_ym: String,
    #[serde(default)]
    pub serie: Option<String>,
    #[serde(default)]
    pub numar: Option<String>,
    #[serde(default)]
    pub cod_indemnizatie: Option<String>,
    #[serde(default)]
    pub data_acordare: Option<String>,
    #[serde(default)]
    pub data_inceput: Option<String>,
    #[serde(default)]
    pub data_sfarsit: Option<String>,
    #[serde(default)]
    pub zile_angajator: Option<i64>,
    #[serde(default)]
    pub zile_fnuass: Option<i64>,
    #[serde(default)]
    pub baza_calcul: Option<String>,
    #[serde(default)]
    pub zile_baza: Option<i64>,
    #[serde(default)]
    pub suma_angajator: Option<String>,
    #[serde(default)]
    pub suma_fnuass: Option<String>,
    #[serde(default)]
    pub procent: Option<i64>,
    #[serde(default)]
    pub loc_prescriere: Option<i64>,
    #[serde(default)]
    pub cod_boala: Option<String>,
}

const COLS: &str = "id, company_id, employee_id, period_ym, serie, numar, cod_indemnizatie, \
                    data_acordare, data_inceput, data_sfarsit, zile_angajator, zile_fnuass, \
                    baza_calcul, zile_baza, suma_angajator, suma_fnuass, procent, \
                    loc_prescriere, cod_boala, created_at";

fn money(label: &str, s: &str) -> AppResult<String> {
    let d = Decimal::from_str(s.trim()).map_err(|_| {
        AppError::Validation(format!("{label} invalid — folosiți formatul 123.45."))
    })?;
    if d.is_sign_negative() {
        return Err(AppError::Validation(format!(
            "{label} nu poate fi negativ."
        )));
    }
    Ok(d.to_string())
}

/// `true` for a well-formed ISO calendar date `YYYY-MM-DD` (month 1–12, day 1–31).
/// ISO strings also compare lexicographically = chronologically, so `<=` on the raw
/// strings is a valid ordering check once both are validated here.
fn valid_iso_date(s: &str) -> bool {
    let p: Vec<&str> = s.split('-').collect();
    if p.len() != 3 || p[0].len() != 4 || p[1].len() != 2 || p[2].len() != 2 {
        return false;
    }
    if !p.iter().all(|seg| seg.bytes().all(|b| b.is_ascii_digit())) {
        return false;
    }
    let (m, d) = (
        p[1].parse::<u32>().unwrap_or(0),
        p[2].parse::<u32>().unwrap_or(0),
    );
    (1..=12).contains(&m) && (1..=31).contains(&d)
}

/// Baseline data-quality validation for a medical-leave certificate (OUG 158/2005).
/// Rejects the obviously-unusable rows that would otherwise reach the D112 export /
/// DUKIntegrator as garbage. Does NOT enforce the deferred `asiguratD` rules (see the
/// module-level note on the `D_5`/`D_10`/`D_23` blocker).
fn validate_leave(input: &MedicalLeaveInput) -> AppResult<()> {
    let serie = input.serie.as_deref().unwrap_or("").trim();
    let numar = input.numar.as_deref().unwrap_or("").trim();
    if serie.is_empty() || numar.is_empty() {
        return Err(AppError::Validation(
            "Seria și numărul certificatului medical sunt obligatorii.".into(),
        ));
    }
    let inceput = input.data_inceput.as_deref().unwrap_or("").trim();
    let sfarsit = input.data_sfarsit.as_deref().unwrap_or("").trim();
    if !valid_iso_date(inceput) || !valid_iso_date(sfarsit) {
        return Err(AppError::Validation(
            "Datele de început și sfârșit ale concediului sunt obligatorii și trebuie să fie valide."
                .into(),
        ));
    }
    if sfarsit < inceput {
        return Err(AppError::Validation(
            "Data de sfârșit nu poate fi înainte de data de început.".into(),
        ));
    }
    // data_acordare is optional in the UI; validate ordering only when present.
    let acordare = input.data_acordare.as_deref().unwrap_or("").trim();
    if !acordare.is_empty() && (!valid_iso_date(acordare) || acordare > inceput) {
        return Err(AppError::Validation(
            "Data acordării trebuie să fie validă și cel mult egală cu data de început.".into(),
        ));
    }
    let total_zile =
        input.zile_angajator.unwrap_or(0).max(0) + input.zile_fnuass.unwrap_or(0).max(0);
    if total_zile < 1 {
        return Err(AppError::Validation(
            "Certificatul trebuie să acopere cel puțin o zi (angajator sau FNUASS).".into(),
        ));
    }
    // media zilnică (D_19) = bază / zile_bază; o bază nenulă cu zile_bază = 0 ar fi împărțire la 0.
    let baza_nonzero = input
        .baza_calcul
        .as_deref()
        .and_then(|s| Decimal::from_str(s.trim()).ok())
        .is_some_and(|d| !d.is_zero());
    if baza_nonzero && input.zile_baza.unwrap_or(0) < 1 {
        return Err(AppError::Validation(
            "Numărul de zile pentru baza de calcul trebuie să fie cel puțin 1.".into(),
        ));
    }
    // D_10 (loc prescriere): XSD `IntInt1_4SType` → domeniul valid e 1–4. Întoarcem eroare doar
    // pentru o valoare explicit invalidă (None → default 1 la create).
    if let Some(loc) = input.loc_prescriere {
        if !(1..=4).contains(&loc) {
            return Err(AppError::Validation(
                "Locul de prescriere (D_10) trebuie să fie 1–4 (1 medic, 2 spital, 3 ambulatoriu, 4 CAS)."
                    .into(),
            ));
        }
    }
    // D_23 (cod boală): max 3 caractere. „RM" e impus automat pentru D_9=15 (risc maternal) la create.
    if let Some(cod) = input.cod_boala.as_deref() {
        if cod.trim().chars().count() > 3 {
            return Err(AppError::Validation(
                "Codul de boală (D_23) are maximum 3 caractere.".into(),
            ));
        }
    }
    Ok(())
}

/// All medical-leave certificates for a company in a reporting month ('YYYY-MM').
pub async fn list(
    pool: &SqlitePool,
    company_id: &str,
    period_ym: &str,
) -> AppResult<Vec<MedicalLeave>> {
    let q = format!(
        "SELECT {COLS} FROM medical_leaves WHERE company_id=?1 AND period_ym=?2 \
         ORDER BY employee_id, data_inceput"
    );
    Ok(sqlx::query_as::<_, MedicalLeave>(&q)
        .bind(company_id)
        .bind(period_ym)
        .fetch_all(pool)
        .await?)
}

pub async fn create(pool: &SqlitePool, input: MedicalLeaveInput) -> AppResult<MedicalLeave> {
    validate_leave(&input)?;
    let baza = money(
        "Baza de calcul",
        input.baza_calcul.as_deref().unwrap_or("0"),
    )?;
    let s_ang = money(
        "Indemnizația angajator",
        input.suma_angajator.as_deref().unwrap_or("0"),
    )?;
    let s_fnuass = money(
        "Indemnizația FNUASS",
        input.suma_fnuass.as_deref().unwrap_or("0"),
    )?;
    let id = new_id();
    let cod_indemnizatie = input.cod_indemnizatie.as_deref().unwrap_or("01");
    // D_23: forced to "RM" for risc maternal (D_9=15); otherwise the entered diagnosis code.
    let cod_boala = if cod_indemnizatie == "15" {
        "RM".to_string()
    } else {
        input.cod_boala.as_deref().unwrap_or("").trim().to_string()
    };
    sqlx::query(&format!(
        "INSERT INTO medical_leaves ({COLS}) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)"
    ))
    .bind(&id)
    .bind(&input.company_id)
    .bind(&input.employee_id)
    .bind(&input.period_ym)
    .bind(input.serie.as_deref().unwrap_or("").trim())
    .bind(input.numar.as_deref().unwrap_or("").trim())
    .bind(cod_indemnizatie)
    .bind(input.data_acordare.as_deref().unwrap_or(""))
    .bind(input.data_inceput.as_deref().unwrap_or(""))
    .bind(input.data_sfarsit.as_deref().unwrap_or(""))
    .bind(input.zile_angajator.unwrap_or(0).max(0))
    .bind(input.zile_fnuass.unwrap_or(0).max(0))
    .bind(&baza)
    .bind(input.zile_baza.unwrap_or(0).max(0))
    .bind(&s_ang)
    .bind(&s_fnuass)
    .bind(input.procent.unwrap_or(75))
    .bind(input.loc_prescriere.unwrap_or(1).clamp(1, 4))
    .bind(&cod_boala)
    .bind(now_unix())
    .execute(pool)
    .await?;
    list(pool, &input.company_id, &input.period_ym)
        .await?
        .into_iter()
        .find(|m| m.id == id)
        .ok_or(AppError::NotFound)
}

pub async fn delete(pool: &SqlitePool, id: &str, company_id: &str) -> AppResult<()> {
    sqlx::query("DELETE FROM medical_leaves WHERE id=?1 AND company_id=?2")
        .bind(id)
        .bind(company_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query(
            "INSERT INTO companies (id, cui, legal_name, address, city, county, country) \
             VALUES ('co','RO1','T','S','C','CJ','RO')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO employees (id, company_id, cnp, full_name, gross_salary, personal_deduction) \
             VALUES ('e1','co','1900101410011','Ion','5000','0')",
        )
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    #[tokio::test]
    async fn create_list_delete_roundtrip() {
        let pool = pool().await;
        let m = create(
            &pool,
            MedicalLeaveInput {
                company_id: "co".into(),
                employee_id: "e1".into(),
                period_ym: "2026-06".into(),
                serie: Some("AB".into()),
                numar: Some("123".into()),
                cod_indemnizatie: Some("01".into()),
                data_acordare: Some("2026-06-01".into()),
                data_inceput: Some("2026-06-02".into()),
                data_sfarsit: Some("2026-06-06".into()),
                zile_angajator: Some(5),
                zile_fnuass: Some(0),
                baza_calcul: Some("6000".into()),
                zile_baza: Some(21),
                suma_angajator: Some("1071.43".into()),
                suma_fnuass: Some("0".into()),
                procent: Some(75),
                loc_prescriere: Some(1),
                cod_boala: Some("01".into()),
            },
        )
        .await
        .unwrap();
        assert_eq!(m.serie, "AB");
        let all = list(&pool, "co", "2026-06").await.unwrap();
        assert_eq!(all.len(), 1);
        delete(&pool, &m.id, "co").await.unwrap();
        assert!(list(&pool, "co", "2026-06").await.unwrap().is_empty());
    }

    /// A complete, valid certificate input — the baseline the validation tests mutate.
    fn valid_input() -> MedicalLeaveInput {
        MedicalLeaveInput {
            company_id: "co".into(),
            employee_id: "e1".into(),
            period_ym: "2026-06".into(),
            serie: Some("AB".into()),
            numar: Some("123".into()),
            cod_indemnizatie: Some("01".into()),
            data_acordare: Some("2026-06-01".into()),
            data_inceput: Some("2026-06-02".into()),
            data_sfarsit: Some("2026-06-06".into()),
            zile_angajator: Some(5),
            zile_fnuass: Some(0),
            baza_calcul: Some("6000".into()),
            zile_baza: Some(21),
            suma_angajator: Some("1071.43".into()),
            suma_fnuass: Some("0".into()),
            procent: Some(75),
            loc_prescriere: Some(1),
            cod_boala: Some("01".into()),
        }
    }

    #[tokio::test]
    async fn rejects_negative_amount() {
        let pool = pool().await;
        let r = create(
            &pool,
            MedicalLeaveInput {
                suma_angajator: Some("-5".into()),
                ..valid_input()
            },
        )
        .await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn d10_d23_capture_and_rules() {
        let pool = pool().await;
        // D_9=15 (risc maternal) forces D_23 = "RM" regardless of the entered cod_boala.
        let m = create(
            &pool,
            MedicalLeaveInput {
                cod_indemnizatie: Some("15".into()),
                cod_boala: Some("xyz".into()),
                loc_prescriere: Some(2),
                ..valid_input()
            },
        )
        .await
        .unwrap();
        assert_eq!(m.cod_boala, "RM");
        assert_eq!(m.loc_prescriere, 2);
        // D_10 out of the XSD's 1..=4 domain → rejected.
        assert!(create(
            &pool,
            MedicalLeaveInput {
                loc_prescriere: Some(5),
                ..valid_input()
            }
        )
        .await
        .is_err());
        // D_23 longer than 3 chars → rejected.
        assert!(create(
            &pool,
            MedicalLeaveInput {
                cod_boala: Some("ABCD".into()),
                ..valid_input()
            }
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn rejects_missing_serie_or_numar() {
        let pool = pool().await;
        assert!(create(
            &pool,
            MedicalLeaveInput {
                serie: Some("  ".into()),
                ..valid_input()
            }
        )
        .await
        .is_err());
        assert!(create(
            &pool,
            MedicalLeaveInput {
                numar: None,
                ..valid_input()
            }
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn rejects_end_before_start_and_zero_days() {
        let pool = pool().await;
        // sfârșit < început
        assert!(create(
            &pool,
            MedicalLeaveInput {
                data_inceput: Some("2026-06-10".into()),
                data_sfarsit: Some("2026-06-05".into()),
                ..valid_input()
            }
        )
        .await
        .is_err());
        // zero zile (angajator + FNUASS)
        assert!(create(
            &pool,
            MedicalLeaveInput {
                zile_angajator: Some(0),
                zile_fnuass: Some(0),
                ..valid_input()
            }
        )
        .await
        .is_err());
        // bază nenulă fără zile_bază → ar fi împărțire la 0 pentru media zilnică
        assert!(create(
            &pool,
            MedicalLeaveInput {
                baza_calcul: Some("6000".into()),
                zile_baza: Some(0),
                ..valid_input()
            }
        )
        .await
        .is_err());
    }

    #[test]
    fn iso_date_helper() {
        assert!(valid_iso_date("2026-06-02"));
        assert!(!valid_iso_date("2026-13-02")); // luna 13
        assert!(!valid_iso_date("2026-06-32")); // ziua 32
        assert!(!valid_iso_date("2026-6-2")); // segmente nepadate
        assert!(!valid_iso_date("02.06.2026")); // format zz.ll.aaaa, nu ISO
        assert!(!valid_iso_date(""));
    }
}
