//! D394 Declarație informativă — ANAF-schema-conformant XML generator (v5).
//!
//! The schema has a hierarchical structure:
//!   `<declaratie394 ...header attrs...>`
//!     `<informatii .../>` (required, one only)
//!     `<rezumat1 .../>*` (optional; one per distinct (tip_partener, cota) in op1)
//!     `<rezumat2 .../>?` (optional; cash-register/PF summary — skipped, no data)
//!     `<op1 .../>*` (optional; one per partner × operation group)
//!
//! Namespace: `mfp:anaf:dgti:d394:declaratie:v5`
//! Root element: `declaratie394`
//!
//! Usage:
//! ```no_run
//! use efactura_desktop_lib::anaf_decl::d394::{D394Submission, sections, generator};
//! ```

pub mod generator;
pub mod sections;

use serde::{Deserialize, Serialize};

/// Submission-level metadata not derivable from `D394Report` or the company
/// record — supplied by the user/caller before export.
///
/// Fields with defaults use `#[serde(default)]` or `Default`. The Tauri command
/// accepts this struct as JSON; the frontend only needs to supply fields that
/// differ from the defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D394Submission {
    // ── Declaration type / fiscal regime ─────────────────────────────────────
    /// Tipul D394: L=lunar, T=trimestrial, S=semestrial, A=anual.
    pub tip_d394: String,
    /// Sistem TVA la încasare (0=standard, 1=la încasare).
    #[serde(default)]
    pub sistem_tva: bool,
    /// Operațiuni efectuate cu persoane afiliate (0/1).
    #[serde(default)]
    pub op_efectuate: bool,

    // ── Company metadata (not derivable from Company struct) ──────────────────
    /// Codul CAEN principal (4 cifre, din enumerarea XSD).
    pub caen: String,
    /// Telefon (max 15 chars).
    pub telefon: String,

    // ── Representative (mandatory in XSD; use company itself if no separate rep) ─
    /// Denumirea reprezentantului (max 200 chars).
    pub den_r: String,
    /// Funcția reprezentantului (max 100 chars).
    pub functie_reprez: String,
    /// Adresa reprezentantului (max 1000 chars).
    pub adresa_r: String,

    // ── Preparer (cel care a întocmit declarația) ─────────────────────────────
    /// 0=persoana proprie, 1=consultant.
    #[serde(default)]
    pub tip_intocmit: i32,
    /// Denumire persoana care a întocmit (max 75 chars).
    pub den_intocmit: String,
    /// CIF persoana care a întocmit (IntPoz13SType: 0–9999999999999).
    #[serde(default)]
    pub cif_intocmit: i64,
    /// Calitatea celui care a întocmit declarația (XSD `calitate_intocmit`, Str75, optional).
    /// Emis doar când `tip_intocmit == 0` (preparer este persoana proprie / reprezentant).
    /// DUK business rule: required when tip_intocmit=0.
    /// Exemplu: "Reprezentant", "Director", "Administrator".
    #[serde(default)]
    pub calitate_intocmit: Option<String>,

    // ── Other flags ───────────────────────────────────────────────────────────
    /// Opțiune regim special (0/1).
    #[serde(default)]
    pub optiune: bool,
    /// Persoane afiliate (0/1).
    #[serde(default)]
    pub prs_afiliat: bool,

    // ── Summary: solicit (InformatiiType solicit attr) ────────────────────────
    /// Solicită rambursare (0/1); reflected in informatii.solicit.
    #[serde(default)]
    pub solicit: bool,

    // ── Cartuș G (încasări AMEF) + facturi simplificate — introduse manual pe cotă ─
    /// Numărul total de bonuri fiscale Î1 (AMEF). Informativ; fără reconciliere DUK.
    #[serde(default)]
    pub nr_bf_i1: i64,
    /// Totaluri pe cotă pentru încasări numerar + facturi simplificate (cartuș G/I). Sumele-total
    /// `incasari_i1`/`incasari_i2` se CALCULEAZĂ din aceste rânduri (regula DUK), nu se introduc.
    #[serde(default)]
    pub cash_rows: Vec<D394CashRow>,
}

/// Un rând per cotă TVA cu totalurile (lei întregi) pentru încasări numerar și facturi simplificate
/// declarate manual în D394 (cartuș G + I). Bonurile pentru care s-a emis factură NU se includ (se
/// evită dubla raportare); facturile simplificate sunt cele ≤ 100 EUR (art. 319 Cod fiscal).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D394CashRow {
    /// Cota TVA (2026: 21 / 11 / 9).
    pub cota: i64,
    /// Î1 — încasări prin AMEF (casa de marcat), bază + TVA.
    #[serde(default)]
    pub baza_i1: i64,
    #[serde(default)]
    pub tva_i1: i64,
    /// Î2 — încasări din activități exceptate de la AMEF (OUG 28/1999), bază + TVA.
    #[serde(default)]
    pub baza_i2: i64,
    #[serde(default)]
    pub tva_i2: i64,
    /// Facturi simplificate emise FĂRĂ codul beneficiarului (FSL), bază + TVA.
    #[serde(default)]
    pub baza_fsl: i64,
    #[serde(default)]
    pub tva_fsl: i64,
    /// Facturi simplificate emise CU codul beneficiarului (FSLcod), bază + TVA.
    #[serde(default)]
    pub baza_fsl_cod: i64,
    #[serde(default)]
    pub tva_fsl_cod: i64,
    /// Facturi simplificate primite — achiziții (FSA), bază + TVA.
    #[serde(default)]
    pub baza_fsa: i64,
    #[serde(default)]
    pub tva_fsa: i64,
    /// Facturi simplificate primite — achiziții intracomunitare (FSAI), bază + TVA.
    #[serde(default)]
    pub baza_fsai: i64,
    #[serde(default)]
    pub tva_fsai: i64,
    /// Bonuri fiscale — achiziții intracomunitare (BFAI), bază + TVA.
    #[serde(default)]
    pub baza_bfai: i64,
    #[serde(default)]
    pub tva_bfai: i64,
}

impl Default for D394Submission {
    fn default() -> Self {
        Self {
            tip_d394: "L".to_string(),
            sistem_tva: false,
            op_efectuate: false,
            caen: "6201".to_string(),
            telefon: "0000000".to_string(),
            den_r: String::new(),
            functie_reprez: "DIRECTOR".to_string(),
            adresa_r: String::new(),
            tip_intocmit: 0,
            den_intocmit: String::new(),
            cif_intocmit: 0,
            calitate_intocmit: Some("Reprezentant".to_string()),
            optiune: false,
            prs_afiliat: false,
            solicit: false,
            nr_bf_i1: 0,
            cash_rows: Vec::new(),
        }
    }
}
