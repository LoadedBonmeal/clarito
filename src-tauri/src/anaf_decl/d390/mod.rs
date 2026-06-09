//! D390 Declarație recapitulativă (VIES) — ANAF-schema XML generator (OPANAF 705/2020, v3).
//!
//! Structure (`mfp:anaf:dgti:d390:declaratie:v3`):
//!   `<declaratie390 luna an d_rec cui den adresa telefon totalPlata_A nume_declar …>`
//!     `<rezumat nr_pag nrOPI bazaL bazaT bazaA bazaP bazaS bazaR total_baza/>`  (1)
//!     `<operatie tip tara codO denO baza/>`  (1-n)  — one per (tara+codO+denO+tip)
//!
//! Operation codes (`tip`): L = livrări intracomunitare de bunuri, T = livrări triunghiulare,
//! A = achiziții intracomunitare de bunuri, P = prestări intracomunitare de servicii,
//! S = achiziții intracomunitare de servicii, R = livrări în regimul agricultorilor.
//! codO is mandatory for L/T/P/R, may be absent for A/S.
//!
//! App-data mapping: outbound sales lines with vat_category 'K' → L (goods) / P (services by
//! the line's revenue_kind); inbound received lines 'K' → A (goods) / S (services by
//! received_invoices.intra_eu_kind). T / R are not modelled (rare) and are omitted.

pub mod generator;

use serde::{Deserialize, Serialize};

/// One aggregated D390 operation row (sum over a period per partner + type).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct D390Op {
    /// Operation type: L/T/A/P/S/R.
    pub tip: String,
    /// Partner country code (2 letters), e.g. "DE".
    pub tara: String,
    /// Partner VAT id WITHOUT the country prefix.
    pub cod_o: String,
    /// Partner name.
    pub den_o: String,
    /// Taxable base in RON (whole lei), no VAT.
    pub baza: i64,
}

/// Submission metadata not derivable from the operations or the company record.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct D390Submission {
    /// Declarație rectificativă (true = 1).
    #[serde(default)]
    pub d_rec: bool,
    /// Declarant: nume / prenume / funcție.
    #[serde(default)]
    pub nume_declar: String,
    #[serde(default)]
    pub prenume_declar: String,
    #[serde(default)]
    pub functie_declar: String,
}

/// The full D390 document: period + the aggregated operation rows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct D390Doc {
    pub luna: u32,
    pub an: i32,
    pub operations: Vec<D390Op>,
    /// Count of intra-EU ('K') operations skipped because the partner VAT id was missing or
    /// not a valid EU code — surfaced so the user can fix the data (else VIES under-reporting).
    pub dropped: i64,
}
