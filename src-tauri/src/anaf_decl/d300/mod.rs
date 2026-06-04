//! D300 Decont TVA — ANAF-schema-conformant XML generator (v12, period ≥ 2025-08).
//!
//! The schema is a single flat `<declaratie300 ...>` element with all data as
//! attributes. Namespace: `mfp:anaf:dgti:d300:declaratie:v12`.
//!
//! Usage:
//! ```no_run
//! use efactura_desktop_lib::anaf_decl::d300::{D300Submission, generator, rows};
//! ```

pub mod generator;
pub mod rows;

use serde::{Deserialize, Serialize};

/// Submission-level metadata not derivable from the computed `D300Report` or
/// the company record — supplied by the user/caller before export.
///
/// Fields with defaults are set via `Default`. The Tauri command accepts this
/// struct as a JSON payload; the frontend only needs to supply the fields that
/// differ from the defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D300Submission {
    // ── Declarant person ─────────────────────────────────────────────────────
    /// Numele declarantului (max 75 chars).
    pub nume_declar: String,
    /// Prenumele declarantului (max 75 chars).
    pub prenume_declar: String,
    /// Funcția declarantului (max 50 chars).
    pub functie_declar: String,

    // ── Company / banking ────────────────────────────────────────────────────
    /// Codul CAEN (4 cifre, din enum-ul XSD).
    pub caen: String,
    /// Denumirea băncii (max 50 chars).
    pub banca: String,
    /// Contul bancar IBAN (max 50 chars).
    pub cont: String,

    // ── Declaration type / legal basis ───────────────────────────────────────
    /// Tipul decontului: L=lunar, T=trimestrial, S=semestrial, A=anual.
    pub tip_decont: String,
    /// Temeiul legal pentru depunere: 0 (standard) sau 2 (alt temei).
    #[serde(default)]
    pub temei: i32,
    /// Dacă este depus prin reprezentant fiscal (0/1).
    #[serde(default)]
    pub depus_reprezentant: bool,

    // ── Special regime flags ─────────────────────────────────────────────────
    /// Bifă operațiuni interne (0/1).
    #[serde(default)]
    pub bifa_interne: bool,
    /// Bifă cereale (D/N).
    #[serde(default)]
    pub bifa_cereale: bool,
    /// Bifă mobile (D/N).
    #[serde(default)]
    pub bifa_mob: bool,
    /// Bifă dispozitive (D/N).
    #[serde(default)]
    pub bifa_disp: bool,
    /// Bifă construcții (D/N).
    #[serde(default)]
    pub bifa_cons: bool,

    // ── Refund / pro-rata ────────────────────────────────────────────────────
    /// Solicită rambursare TVA (D/N).
    #[serde(default)]
    pub solicit_ramb: bool,
    /// Nr. din Registrul persoanelor impozabile (integer string, default "0").
    #[serde(default = "default_nr_evid")]
    pub nr_evid: String,
    /// Pro-rata TVA (0.0 – 100.0, default 100.0 = nu se aplică pro-rata).
    #[serde(default = "default_pro_rata")]
    pub pro_rata: f64,

    // ── Regularizări cote vechi (Wave 8) ─────────────────────────────────────
    /// Override baza R16_1 (regularizări taxă colectată, cote vechi 19%/5%).
    /// `None` = use auto-computed value from `D300Report.reg_colectata_baza`.
    #[serde(default)]
    pub reg_colectata_baza: Option<i64>,
    /// Override TVA R16_2 (regularizări taxă colectată, cote vechi 19%/5%).
    #[serde(default)]
    pub reg_colectata_tva: Option<i64>,
    /// Override baza R30_1 (regularizări taxă dedusă, cote vechi 19%/9%/5%).
    #[serde(default)]
    pub reg_dedusa_baza: Option<i64>,
    /// Override TVA R30_2 (regularizări taxă dedusă, cote vechi 19%/9%/5%).
    #[serde(default)]
    pub reg_dedusa_tva: Option<i64>,
}

fn default_nr_evid() -> String {
    "0".to_string()
}

fn default_pro_rata() -> f64 {
    100.0
}

impl Default for D300Submission {
    fn default() -> Self {
        Self {
            nume_declar: String::new(),
            prenume_declar: String::new(),
            functie_declar: String::new(),
            caen: "6201".to_string(), // Activități de realizare a soft-ului la comandă
            banca: String::new(),
            cont: String::new(),
            tip_decont: "L".to_string(),
            temei: 0,
            depus_reprezentant: false,
            bifa_interne: false,
            bifa_cereale: false,
            bifa_mob: false,
            bifa_disp: false,
            bifa_cons: false,
            solicit_ramb: false,
            nr_evid: default_nr_evid(),
            pro_rata: default_pro_rata(),
            reg_colectata_baza: None,
            reg_colectata_tva: None,
            reg_dedusa_baza: None,
            reg_dedusa_tva: None,
        }
    }
}
