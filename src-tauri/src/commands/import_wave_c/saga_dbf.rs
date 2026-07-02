//! SAGA C. DBF adapter — Wave C W3.  **DEFENSIVE** — column names/order are
//! single-source/version-bound and change between SAGA releases.
//!
//! # Why DEFENSIVE?
//!
//! SAGA does not publish a stable open DBF schema. Column names, ordering, and
//! the exact codepage vary between SAGA versions, firm configurations, and
//! data files (parteneri vs. articole/stoc). This adapter:
//!
//!   1. **Never relies on column order** — always looks up by name.
//!   2. Applies a **synonym table** that maps multiple historical names to one
//!      internal field (`COD_FISCAL`, `CUI`, `CIF`, `CODFISCAL` → `cui`).
//!   3. Accepts a **user-confirmed `ctx.column_map`** override when the W5 UI
//!      presents the detected columns for human review before importing.
//!   4. Emits **warnings for missing/unknown columns** and continues — it never
//!      panics on bad data.
//!
//! # Supported DBF types
//!
//! | SAGA file        | Maps to            | Notes                                 |
//! |------------------|--------------------|---------------------------------------|
//! | Parteneri        | `StagedContact`    | Partner/client list                   |
//! | Articole / Stoc  | `StagedProduct`    | Stock article list                    |
//!
//! Chart-of-accounts: SAGA has **no documented accounts DBF** — accounts are
//! seeded from the standard Romanian plan (OMFP 1802/2014) in Clarito's own
//! `seed_standard` and maintained manually. This adapter does NOT attempt to
//! read or produce `StagedAccount` records. That matches the agreed design.
//!
//! # Codepage detection (defensive heuristic)
//!
//! SAGA DBF files are commonly CP852 (MS-DOS East-European) or CP1250
//! (Windows Central-European). The adapter opens the file under three candidate
//! encodings — CP852, CP1250, strict UTF-8 (`dbase`'s `yore`-backed
//! `Reader::new_with_encoding`, one full re-parse per candidate) — samples up to the
//! first 16 records under each, and scores every candidate by the TOTAL number of
//! valid Romanian diacritics (ă â î ș ț — both comma-below and cedilla variants)
//! across the sample, disqualifying any record containing U+FFFD. The highest score
//! wins; a tie keeps the earlier candidate (CP852, the SAGA MS-DOS default — see
//! `pick_dbf_encoding` for the 0xEE 'ţ'↔'î' ambiguity rationale). If the head sample
//! scores 0 everywhere but the record area contains non-ASCII bytes (ASCII-leading
//! file with diacritics only in later records), the picker re-scores over ALL records
//! before conceding; only a genuinely diacritic-free file falls through to the crate
//! default (`UnicodeLossy`, i.e. UTF-8 with lossy replacement — harmless for ASCII).
//!
//! This heuristic is documented here rather than trusted blindly — a real export
//! file should be tested to confirm (see `pick_dbf_encoding`/`open_dbf_reader`). The
//! DBF header `ldid` code-page byte is NOT relied on because it is often 0x00
//! ("Undefined") in SAGA exports, which even with `yore` enabled resolves to a
//! lossy CP1252 decode rather than the correct CP852/CP1250 one.
//!
//! # UNVERIFIED column names
//!
//! The synonym table below is built from partial documentation and user reports.
//! Every column name listed should be confirmed against a real SAGA export
//! before the W4/W5 pass. Columns in the synonym table that have LOW confidence
//! are marked with a comment.

use dbase::{FieldValue, Record};
use uuid::Uuid;

use crate::error::{AppError, AppResult};

use super::adapter::ImportAdapter;
use super::{
    canonical_cui, DetectedColumn, ImportInput, ParseCtx, SourceKind, StagedContact, StagedData,
    StagedProduct,
};

const SOURCE_CONTACT: &str = "SAGA_DBF_PARTNER";
const SOURCE_PRODUCT: &str = "SAGA_DBF_ARTICOL";

// ─── Internal field tags ─────────────────────────────────────────────────────

/// Internal canonical field names produced after synonym resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContactField {
    Cui,     // CUI/CIF/CodFiscal
    Name,    // Company/partner name
    RegCom,  // Nr. registru comerțului
    Address, // Strada + nr
    City,    // Localitate
    County,  // Județ
    Country, // Țară
    Email,
    Phone,
    IsIndividual, // Persoana fizică flag
    Code,         // Internal SAGA partner code
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProductField {
    Code,      // Cod articol (internal SAGA code)
    Name,      // Denumire
    Unit,      // UM
    VatRate,   // ProcTVA / Cota TVA
    Price,     // Pret vanzare
    IsService, // Serviciu flag
    Barcode,   // Cod bare / EAN
    StockQty,  // Cantitate (if present in the stoc file)
}

// ─── Synonym tables ──────────────────────────────────────────────────────────

/// Return the canonical `ContactField` for a (uppercased) DBF column name, or `None`.
fn contact_synonym(col: &str) -> Option<ContactField> {
    match col {
        // CUI — MEDIUM confidence on CODFISCAL/CIF; COD_FISCAL confirmed in user reports
        "COD_FISCAL" | "CODFISCAL" | "CUI" | "CIF" | "FISCAL" | "COD_FISC" => {
            Some(ContactField::Cui)
        }
        // Name — MEDIUM confidence
        "DENUMIRE" | "NUME" | "DEN" | "DENUMIRE_" | "FIRMA" | "NUMEFIRMA" => {
            Some(ContactField::Name)
        }
        "REG_COM" | "REGCOM" | "NR_REG_COM" | "NR_REG" => Some(ContactField::RegCom),
        "ADRESA" | "STRADA" | "ADR" => Some(ContactField::Address),
        "LOCALITATE" | "ORAS" | "LOCALIT" => Some(ContactField::City),
        "JUDET" | "JUD" | "JUDETE" => Some(ContactField::County),
        "TARA" | "TARA_" | "COUNTRY" => Some(ContactField::Country),
        "EMAIL" | "E_MAIL" | "MAIL" => Some(ContactField::Email),
        "TEL" | "TELEFON" | "PHONE" | "MOBIL" | "FAX" => Some(ContactField::Phone),
        // PF flag — LOW confidence (name varies heavily between SAGA versions)
        "PERSFIZ" | "PERS_FIZ" | "PFIZICA" | "IS_PF" | "PF" => Some(ContactField::IsIndividual),
        // Internal partner code
        "COD" | "COD_PART" | "CODP" | "ID" => Some(ContactField::Code),
        _ => None,
    }
}

/// Return the canonical `ProductField` for a (uppercased) DBF column name, or `None`.
fn product_synonym(col: &str) -> Option<ProductField> {
    match col {
        // Cod articol — MEDIUM confidence
        "COD" | "CODARTICOL" | "COD_ART" | "CODA" | "ID" => Some(ProductField::Code),
        // Name — MEDIUM confidence
        "DENUMIRE" | "DEN" | "NUMEARTICOL" | "DESCRIERE" => Some(ProductField::Name),
        // UM — MEDIUM confidence
        "UM" | "UNITATE" | "UNIT" | "UNMS" => Some(ProductField::Unit),
        // VAT rate — LOW confidence on exact name
        "PROCTVA" | "COT_TVA" | "TVA" | "COTA_TVA" | "PROCENTTVA" => Some(ProductField::VatRate),
        // Price — LOW confidence
        "PRET" | "PRET_VANZ" | "PRETVANZARE" | "PRET_V" => Some(ProductField::Price),
        // Service flag — LOW confidence
        "SERVICIU" | "IS_SERV" | "SERV" | "TIP" => Some(ProductField::IsService),
        // Barcode — LOW confidence
        "COD_BARE" | "CODBARE" | "EAN" | "BARCODE" | "EAN13" => Some(ProductField::Barcode),
        // Stock qty (present in stoc DBF, not always in articole DBF)
        "CANTITATE" | "CANT" | "QTY" | "STOC" | "STOCURI" => Some(ProductField::StockQty),
        _ => None,
    }
}

// ─── Adapter ─────────────────────────────────────────────────────────────────

pub struct SagaDbfAdapter;

impl ImportAdapter for SagaDbfAdapter {
    fn source(&self) -> SourceKind {
        SourceKind::SagaDbf
    }

    /// Detect columns by reading the DBF header, returning names + a sample value.
    ///
    /// This is called by the W5 UI to show the user a column-mapping dialog
    /// before committing the import. The adapter uses it to expose the raw
    /// DBF field names so the user can confirm or override synonym resolution.
    fn detect_columns(&self, input: &ImportInput) -> AppResult<Vec<DetectedColumn>> {
        let bytes = require_bytes(input, "detect_columns")?;
        detect_dbf_columns(bytes)
    }

    /// Parse a SAGA DBF file (parteneri or articole/stoc) into `StagedData`.
    ///
    /// The adapter sniffs the file by looking for contact-field columns first.
    /// If the file contains any recognised contact column (CUI/DENUMIRE/etc.)
    /// it is parsed as a partners DBF. Otherwise it is parsed as an articles DBF.
    ///
    /// `ctx.column_map` can override synonym resolution: if the user confirmed
    /// that e.g. "FIRMA" maps to `cui`, the explicit override wins over the
    /// built-in synonym table.
    fn parse(&self, input: &ImportInput, ctx: &ParseCtx) -> AppResult<StagedData> {
        let bytes = require_bytes(input, "parse")?;
        parse_dbf_bytes(bytes, ctx)
    }
}

fn require_bytes<'a>(input: &'a ImportInput, caller: &str) -> AppResult<&'a [u8]> {
    match input {
        ImportInput::Bytes(b) => Ok(b),
        ImportInput::Files(_) => Err(AppError::Validation(format!(
            "SagaDbfAdapter::{caller}(): transmiteți un singur fișier DBF ca ImportInput::Bytes."
        ))),
        ImportInput::RestCreds { .. } => Err(AppError::Validation(format!(
            "SagaDbfAdapter::{caller}(): nu acceptă RestCreds."
        ))),
    }
}

// ─── DBF codepage detection ──────────────────────────────────────────────────
//
// SAGA DBF files are commonly CP852 (MS-DOS East-European) or CP1250 (Windows
// Central-European); the DBF header's own code-page byte (`ldid`) is NOT relied on
// because it is often 0x00 ("Undefined") in SAGA exports — and even when the `yore`
// feature lets dbase resolve an "Undefined" ldid, it falls back to a LOSSY CP1252
// decode (never errors, but silently mis-decodes CP852/CP1250 bytes as if they were
// CP1252). So this picker re-opens the file under each candidate encoding in turn and
// scores it across a MULTI-RECORD sample (up to `SAMPLE_SIZE` records, or all of them
// if the file is smaller) — NOT just the first record.
//
// Sampling only the first record (the pre-fix heuristic) could misdetect: CP852 and
// CP1250 share byte 0xEE, which decodes as 'î' (U+00EE) under CP1250 but as 'ţ'
// (U+0163, cedilla-legacy) under CP852 — both are valid `RO_DIACRITICS`, so whichever
// candidate is TRIED FIRST (CP852) wins on a single ambiguous record even when the file
// is genuinely CP1250. Scoring across many records fixes this: a genuinely-CP1250 file
// will have far more CP1250-valid (diacritic, no U+FFFD) records than CP852-valid ones,
// because most bytes are NOT 0xEE and decode incompatibly under the wrong codepage,
// producing U+FFFD (which disqualifies that record for that candidate) or simply no
// diacritic hit.
//
// Each candidate's score = the TOTAL number of Romanian-diacritic occurrences
// (ă â î ș ț, both comma-below and cedilla variants) across the sampled records; a
// record containing any U+FFFD contributes nothing for that candidate (mirroring the
// original heuristic's U+FFFD veto, applied per-record). The highest-scoring candidate
// wins; a tie keeps the earlier candidate in the list — CP852 first, the historical
// SAGA MS-DOS default. A CP852↔CP1250 tie can only happen when the text's ONLY
// diacritics sit on the ambiguous 0xEE byte (CP852 'ţ' ↔ CP1250 'î'), which is
// undecidable without a dictionary; real CP1250 ş/ţ/ă bytes (0xBA/0xFE/0xE3) decode as
// non-diacritic junk under CP852, so a genuine CP1250 file wins on score. Falls back to
// UTF-8 lossy (the pre-fix behavior) when no candidate scores at all — e.g. an
// ASCII-only file with no diacritics anywhere in the sample.
const SAMPLE_SIZE: usize = 16;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DbfCodePage {
    Cp852,
    Cp1250,
    Utf8,
}

/// Open a fresh `dbase::Reader` over `bytes` under the given candidate encoding.
fn open_reader_with(
    bytes: &[u8],
    cp: DbfCodePage,
) -> Result<dbase::Reader<std::io::Cursor<&[u8]>>, dbase::Error> {
    let cursor = std::io::Cursor::new(bytes);
    match cp {
        DbfCodePage::Cp852 => {
            dbase::Reader::new_with_encoding(cursor, dbase::yore::code_pages::CP852)
        }
        DbfCodePage::Cp1250 => {
            dbase::Reader::new_with_encoding(cursor, dbase::yore::code_pages::CP1250)
        }
        // Strict UTF-8 (errors on invalid bytes rather than lossy-replacing them), so a
        // genuinely-non-UTF-8 file fails the sample read here and the picker moves on, instead
        // of masking the mismatch behind U+FFFD like UnicodeLossy would. `dbase::Unicode`
        // itself doesn't implement `Into<DynEncoding>` with the `yore` feature on (only
        // `yore::CodePage` impls do), so go through the public `DynEncoding::from_name`
        // constructor instead, which resolves "UTF-8" to the same strict `Unicode` encoding.
        DbfCodePage::Utf8 => {
            let enc = dbase::encoding::DynEncoding::from_name("UTF-8")
                .expect("dbase must resolve the built-in \"UTF-8\" encoding name");
            dbase::Reader::new_with_encoding(cursor, enc)
        }
    }
}

/// Does `record`'s Character fields, taken together, look like a correctly-decoded
/// Romanian string under the candidate encoding it was read with? Mirrors the old
/// `decode_ro_string` per-record scoring heuristic, but applied to an already-decoded
/// `Record` (each candidate encoding decodes at the dbase level, not via a post-hoc
/// byte reinterpretation). A single U+FFFD anywhere in the record disqualifies it
/// (returns `false`) even if another field also contains a valid diacritic.
fn record_ro_score(record: &Record) -> Option<usize> {
    let mut occurrences = 0usize;
    for (_, value) in record.clone().into_iter() {
        if let FieldValue::Character(Some(s)) = &value {
            if s.contains('\u{FFFD}') {
                return None; // a correct code page never lossy-replaces
            }
            occurrences += s.chars().filter(|c| RO_DIACRITICS.contains(c)).count();
        }
    }
    Some(occurrences)
}

/// Score one candidate encoding against up to `SAMPLE_SIZE` records: the TOTAL number
/// of RO-diacritic occurrences across the sampled records (a record containing any
/// U+FFFD contributes nothing). Occurrence counting (not a per-record boolean)
/// discriminates the CP852↔CP1250 byte collisions better: e.g. real CP1250 ş/ţ bytes
/// (0xBA/0xFE) decode as box-drawing junk under CP852, so the wrong candidate scores
/// strictly lower whenever the text has more than the ambiguous î/ţ (0xEE) overlap.
/// Returns `None` if the reader can't even be opened under this encoding.
fn score_candidate(bytes: &[u8], cp: DbfCodePage) -> Option<usize> {
    score_candidate_sampled(bytes, cp, SAMPLE_SIZE)
}

fn score_candidate_sampled(bytes: &[u8], cp: DbfCodePage, sample: usize) -> Option<usize> {
    let mut reader = open_reader_with(bytes, cp).ok()?;
    let score = reader
        .iter_records()
        .take(sample)
        .filter_map(|r| r.ok())
        .filter_map(|record| record_ro_score(&record))
        .sum();
    Some(score)
}

/// Does the RECORD AREA of the DBF (after the header, whose length sits at bytes 8..10
/// little-endian) contain any non-ASCII byte? An ASCII-leading file whose first
/// `SAMPLE_SIZE` records carry no diacritics scores 0 under every candidate, but a
/// high byte later in the file means the lossy UTF-8 fallback would corrupt real
/// diacritics — so the picker must widen its sample instead of giving up.
fn record_area_has_high_bytes(bytes: &[u8]) -> bool {
    let header_len = bytes
        .get(8..10)
        .map(|b| u16::from_le_bytes([b[0], b[1]]) as usize)
        .unwrap_or(0);
    bytes
        .get(header_len..)
        .unwrap_or(bytes)
        .iter()
        .any(|b| *b >= 0x80)
}

/// Pick the best-scoring codepage for this DBF file by sampling up to `SAMPLE_SIZE`
/// records under each candidate in turn (see the module-level comment above for the
/// full rationale). If every candidate scores 0 on the head sample BUT the record area
/// contains non-ASCII bytes (diacritics appearing only after the sampled records),
/// re-scores over ALL records before conceding. Returns `None` only when the file is
/// genuinely diacritic-free (falls back to UTF-8 lossy, the pre-fix default, at the
/// call site — harmless for pure-ASCII data).
fn pick_dbf_encoding(bytes: &[u8]) -> Option<DbfCodePage> {
    pick_dbf_encoding_sampled(bytes, SAMPLE_SIZE).or_else(|| {
        if record_area_has_high_bytes(bytes) {
            pick_dbf_encoding_sampled(bytes, usize::MAX)
        } else {
            None
        }
    })
}

fn pick_dbf_encoding_sampled(bytes: &[u8], sample: usize) -> Option<DbfCodePage> {
    let candidates = [DbfCodePage::Cp852, DbfCodePage::Cp1250, DbfCodePage::Utf8];

    let mut best: Option<(DbfCodePage, usize)> = None;
    for cp in candidates {
        let Some(score) = score_candidate_sampled(bytes, cp, sample) else {
            continue;
        };
        if score == 0 {
            continue;
        }
        best = Some(match best {
            None => (cp, score),
            Some((best_cp, best_score)) => {
                if score > best_score {
                    (cp, score)
                } else {
                    // Tie (or worse) keeps the earlier candidate — CP852 first, the
                    // historical SAGA MS-DOS default. A CP852↔CP1250 tie only happens
                    // when the text's ONLY diacritics sit on the ambiguous 0xEE byte
                    // (CP852 'ţ' ↔ CP1250 'î'), which is undecidable without a
                    // dictionary; any real CP1250 ş/ţ/ă (0xBA/0xFE/0xE3) decodes as
                    // non-diacritic junk under CP852 and wins on score instead.
                    (best_cp, best_score)
                }
            }
        });
    }
    best.map(|(cp, _)| cp)
}

/// Open the real reader for `bytes`, using the best-scoring codepage (see
/// `pick_dbf_encoding`), falling back to the crate default (`UnicodeLossy`, matching
/// the pre-fix behavior) when no candidate's sample scored.
fn open_dbf_reader(bytes: &[u8]) -> Result<dbase::Reader<std::io::Cursor<&[u8]>>, dbase::Error> {
    match pick_dbf_encoding(bytes) {
        Some(cp) => open_reader_with(bytes, cp),
        None => dbase::Reader::new(std::io::Cursor::new(bytes)),
    }
}

// ─── DBF column detection ─────────────────────────────────────────────────────

fn detect_dbf_columns(bytes: &[u8]) -> AppResult<Vec<DetectedColumn>> {
    let mut reader = open_dbf_reader(bytes)
        .map_err(|e| AppError::Other(format!("SAGA DBF: citire header eșuată: {e}")))?;

    let field_names: Vec<String> = reader
        .fields()
        .iter()
        .map(|f| f.name().to_string())
        .collect();

    // Read first record for sample values.
    let first: Option<Record> = reader.iter_records().next().and_then(|r| r.ok());

    let detected: Vec<DetectedColumn> = field_names
        .iter()
        .map(|name| {
            let sample = first
                .as_ref()
                .and_then(|r| r.get(name.as_str()))
                .map(field_value_to_string)
                .unwrap_or_default();
            DetectedColumn {
                name: name.clone(),
                sample,
            }
        })
        .collect();

    Ok(detected)
}

// ─── Main parse logic ─────────────────────────────────────────────────────────

fn parse_dbf_bytes(bytes: &[u8], ctx: &ParseCtx) -> AppResult<StagedData> {
    let mut out = StagedData::empty();

    let mut reader = open_dbf_reader(bytes)
        .map_err(|e| AppError::Other(format!("SAGA DBF: nu se poate deschide fișierul: {e}")))?;

    // Collect all field names from the header.
    let field_names: Vec<String> = reader
        .fields()
        .iter()
        .map(|f| f.name().to_string())
        .collect();

    if field_names.is_empty() {
        out.warnings.push("SAGA DBF: fișier fără coloane.".into());
        return Ok(out);
    }

    // Build column→internal-field maps (contact + product) applying synonym
    // resolution and respecting ctx.column_map overrides.
    let contact_map = build_contact_map(&field_names, ctx);
    let product_map = build_product_map(&field_names, ctx);

    // Determine the file type: prefer contact if any contact column is found.
    let is_contact_file = !contact_map.is_empty();

    // Read all records up front so we can move the reader.
    let records: Vec<Record> = reader
        .iter_records()
        .filter_map(|r| {
            r.map_err(|e| {
                out.warnings.push(format!("SAGA DBF: rând necitibil: {e}"));
            })
            .ok()
        })
        .collect();

    if is_contact_file {
        parse_as_contacts(records, &contact_map, &mut out);
    } else if !product_map.is_empty() {
        parse_as_products(records, &product_map, &mut out);
    } else {
        out.warnings.push(
            "SAGA DBF: nicio coloană recunoscută — fișierul nu este un DBF de parteneri sau \
             articole SAGA, sau coloanele necesită un column_map explicit."
                .into(),
        );
    }

    Ok(out)
}

// ─── Column-map builders ──────────────────────────────────────────────────────

/// Maps DBF column name → ContactField, applying ctx overrides first.
fn build_contact_map(
    field_names: &[String],
    ctx: &ParseCtx,
) -> std::collections::HashMap<String, ContactField> {
    let mut map = std::collections::HashMap::new();
    for name in field_names {
        let upper = name.to_uppercase();
        // ctx.column_map override wins.
        if let Some(cm) = ctx.column_map {
            if let Some(internal) = cm.get(name).or_else(|| cm.get(&upper)) {
                if let Some(field) = contact_field_from_str(internal) {
                    map.insert(name.clone(), field);
                    continue;
                }
            }
        }
        if let Some(field) = contact_synonym(&upper) {
            map.insert(name.clone(), field);
        }
    }
    map
}

/// Maps DBF column name → ProductField, applying ctx overrides first.
fn build_product_map(
    field_names: &[String],
    ctx: &ParseCtx,
) -> std::collections::HashMap<String, ProductField> {
    let mut map = std::collections::HashMap::new();
    for name in field_names {
        let upper = name.to_uppercase();
        if let Some(cm) = ctx.column_map {
            if let Some(internal) = cm.get(name).or_else(|| cm.get(&upper)) {
                if let Some(field) = product_field_from_str(internal) {
                    map.insert(name.clone(), field);
                    continue;
                }
            }
        }
        if let Some(field) = product_synonym(&upper) {
            map.insert(name.clone(), field);
        }
    }
    map
}

fn contact_field_from_str(s: &str) -> Option<ContactField> {
    match s.to_uppercase().as_str() {
        "CUI" | "CIF" | "COD_FISCAL" => Some(ContactField::Cui),
        "NAME" | "DENUMIRE" => Some(ContactField::Name),
        "REG_COM" => Some(ContactField::RegCom),
        "ADDRESS" | "ADRESA" => Some(ContactField::Address),
        "CITY" | "LOCALITATE" => Some(ContactField::City),
        "COUNTY" | "JUDET" => Some(ContactField::County),
        "COUNTRY" | "TARA" => Some(ContactField::Country),
        "EMAIL" => Some(ContactField::Email),
        "PHONE" | "TEL" => Some(ContactField::Phone),
        "IS_INDIVIDUAL" | "PERSFIZ" => Some(ContactField::IsIndividual),
        "CODE" | "COD" => Some(ContactField::Code),
        _ => None,
    }
}

fn product_field_from_str(s: &str) -> Option<ProductField> {
    match s.to_uppercase().as_str() {
        "CODE" | "COD" => Some(ProductField::Code),
        "NAME" | "DENUMIRE" => Some(ProductField::Name),
        "UNIT" | "UM" => Some(ProductField::Unit),
        "VAT_RATE" | "PROCTVA" => Some(ProductField::VatRate),
        "PRICE" | "PRET" => Some(ProductField::Price),
        "IS_SERVICE" | "SERVICIU" => Some(ProductField::IsService),
        "BARCODE" | "COD_BARE" => Some(ProductField::Barcode),
        "STOCK_QTY" | "CANTITATE" => Some(ProductField::StockQty),
        _ => None,
    }
}

// ─── Contact (parteneri) parser ───────────────────────────────────────────────

fn parse_as_contacts(
    records: Vec<Record>,
    contact_map: &std::collections::HashMap<String, ContactField>,
    out: &mut StagedData,
) {
    // Warn about expected-but-absent fields.
    let has_cui = contact_map.values().any(|f| *f == ContactField::Cui);
    let has_name = contact_map.values().any(|f| *f == ContactField::Name);
    if !has_cui {
        out.warnings.push(
            "SAGA DBF parteneri: câmpul CUI/CIF nu a fost găsit (COD_FISCAL/CUI/CIF). \
             Deduplicarea după CUI va fi indisponibilă."
                .into(),
        );
    }
    if !has_name {
        out.warnings.push(
            "SAGA DBF parteneri: câmpul DENUMIRE nu a fost găsit. \
             Contactele importate nu vor avea nume."
                .into(),
        );
    }

    for record in records {
        let mut contact = StagedContact {
            id: Uuid::now_v7().to_string(),
            source: SOURCE_CONTACT.to_string(),
            raw_json: record_to_json(&record),
            source_code: None,
            contact_type: Some("COMPANY".to_string()),
            cui_raw: None,
            cui_canonical: None,
            legal_name: None,
            vat_payer: None,
            is_individual: None,
            address: None,
            city: None,
            county: None,
            country: None,
            email: None,
            phone: None,
            dedup_key: None,
        };

        for (col, field) in contact_map {
            let raw = match record.get(col.as_str()) {
                Some(v) => field_value_to_string_decoded(v),
                None => continue,
            };
            if raw.is_empty() {
                continue;
            }
            match field {
                ContactField::Cui => {
                    let canon = canonical_cui(&raw);
                    contact.cui_raw = Some(raw.clone());
                    contact.cui_canonical = Some(canon.clone());
                    contact.dedup_key = if canon.is_empty() { None } else { Some(canon) };
                }
                ContactField::Name => {
                    contact.legal_name = Some(raw);
                }
                ContactField::RegCom => {
                    // Store in source_code as the closest available slot;
                    // W4 can persist it into the contact's reg_com field.
                    if contact.source_code.is_none() {
                        contact.source_code = Some(raw);
                    }
                }
                ContactField::Address => contact.address = Some(raw),
                ContactField::City => contact.city = Some(raw),
                ContactField::County => contact.county = Some(raw),
                ContactField::Country => contact.country = Some(raw),
                ContactField::Email => contact.email = Some(raw),
                ContactField::Phone => contact.phone = Some(raw),
                ContactField::IsIndividual => {
                    // Flag values: "D", "Da", "1", "true", "T" → true
                    let is_pf = matches!(
                        raw.to_uppercase().as_str(),
                        "D" | "DA" | "1" | "TRUE" | "T" | "YES"
                    );
                    contact.is_individual = Some(is_pf);
                    if is_pf {
                        contact.contact_type = Some("INDIVIDUAL".to_string());
                    }
                }
                ContactField::Code => contact.source_code = Some(raw),
            }
        }

        out.contacts.push(contact);
    }
}

// ─── Product (articole/stoc) parser ──────────────────────────────────────────

fn parse_as_products(
    records: Vec<Record>,
    product_map: &std::collections::HashMap<String, ProductField>,
    out: &mut StagedData,
) {
    let has_name = product_map.values().any(|f| *f == ProductField::Name);
    if !has_name {
        out.warnings.push(
            "SAGA DBF articole: câmpul DENUMIRE nu a fost găsit. \
             Produsele importate nu vor avea denumire."
                .into(),
        );
    }

    for record in records {
        let mut product = StagedProduct {
            id: Uuid::now_v7().to_string(),
            source: SOURCE_PRODUCT.to_string(),
            raw_json: record_to_json(&record),
            source_code: None,
            name: None,
            unit: None,
            unit_price: None,
            vat_rate: None,
            vat_category: None,
            code: None,
            barcode: None,
            stock_qty: None,
            is_service: None,
            dedup_key: None,
        };

        for (col, field) in product_map {
            let raw = match record.get(col.as_str()) {
                Some(v) => field_value_to_string_decoded(v),
                None => continue,
            };
            if raw.is_empty() {
                continue;
            }
            match field {
                ProductField::Code => {
                    product.source_code = Some(raw.clone());
                    product.code = Some(raw.clone());
                    product.dedup_key = Some(raw);
                }
                ProductField::Name => product.name = Some(raw),
                ProductField::Unit => product.unit = Some(raw),
                ProductField::VatRate => product.vat_rate = Some(raw),
                ProductField::Price => product.unit_price = Some(raw),
                ProductField::IsService => {
                    product.is_service = Some(matches!(
                        raw.to_uppercase().as_str(),
                        "D" | "DA" | "1" | "TRUE" | "T" | "YES"
                    ));
                }
                ProductField::Barcode => product.barcode = Some(raw),
                ProductField::StockQty => product.stock_qty = Some(raw),
            }
        }

        out.products.push(product);
    }
}

// ─── Codepage-aware field decoding ───────────────────────────────────────────

/// Decode a `FieldValue` to a String. Numeric/date/logical values are converted
/// directly; Character values are just trimmed, because by this point `dbase`
/// itself has ALREADY decoded the field's raw bytes using the file-wide codepage
/// chosen by `pick_dbf_encoding`/`open_dbf_reader` (CP852, CP1250, or strict UTF-8 —
/// whichever scored best on the file's first record), so the string is correct RO
/// text already.
///
/// FIXED (previously a KNOWN LIMITATION, found by the pre-publication audit):
/// `dbase::Reader::new` (no explicit encoding) used the crate default `UnicodeLossy`
/// (= `String::from_utf8_lossy`), which replaced non-UTF-8 CP852/CP1250 diacritic bytes
/// with U+FFFD BEFORE any Rust-side heuristic could see the original bytes — so ă/â/î/ș/ț
/// were unrecoverably lost for CP852/CP1250 files. Fixed by enabling dbase's `yore`
/// feature (Cargo.toml) and opening the reader with `Reader::new_with_encoding` under a
/// per-file-detected code page (see `open_dbf_reader`), instead of ever going through
/// `UnicodeLossy` for a file that scores as CP852/CP1250. The old per-record post-hoc
/// `decode_ro_string`/`decode_cp852` byte-reinterpretation was removed — encoding is now
/// resolved once, at reader-open time; the RO-diacritic scoring lives in
/// `record_looks_like_ro`/`pick_dbf_encoding`.
fn field_value_to_string_decoded(value: &FieldValue) -> String {
    match value {
        FieldValue::Character(Some(s)) => s.trim().to_string(),
        other => field_value_to_string(other),
    }
}

/// Raw field-value to String (no extra encoding heuristic).
fn field_value_to_string(value: &FieldValue) -> String {
    match value {
        FieldValue::Character(Some(s)) => s.trim().to_string(),
        FieldValue::Character(None) => String::new(),
        FieldValue::Numeric(Some(n)) => n.to_string(),
        FieldValue::Numeric(None) => String::new(),
        FieldValue::Float(Some(f)) => f.to_string(),
        FieldValue::Float(None) => String::new(),
        FieldValue::Logical(Some(b)) => if *b { "true" } else { "false" }.to_string(),
        FieldValue::Logical(None) => String::new(),
        FieldValue::Date(Some(d)) => format!("{d}"),
        FieldValue::Date(None) => String::new(),
        FieldValue::Integer(n) => n.to_string(),
        FieldValue::Double(f) => f.to_string(),
        FieldValue::Memo(m) => m.trim().to_string(),
        FieldValue::Currency(c) => c.to_string(),
        FieldValue::DateTime(dt) => format!("{dt:?}"),
    }
}

/// Romanian diacritics in both comma-below (correct) and cedilla (legacy) forms,
/// plus their uppercase equivalents. Used by `record_looks_like_ro` to score each
/// codepage candidate in `pick_dbf_encoding`.
const RO_DIACRITICS: &[char] = &[
    'ă', 'â', 'î', 'ș', 'ț', 'Ă', 'Â', 'Î', 'Ș', 'Ț', 'ş', 'ţ', 'Ş',
    'Ţ', // cedilla legacy forms
];

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn record_to_json(record: &Record) -> String {
    let mut map = serde_json::Map::new();
    // Record implements IntoIterator (consuming); clone to preserve for later use.
    for (name, value) in record.clone().into_iter() {
        let v = match &value {
            FieldValue::Numeric(Some(n)) => serde_json::json!(n),
            FieldValue::Float(Some(f)) => serde_json::json!(f),
            FieldValue::Integer(i) => serde_json::json!(i),
            FieldValue::Double(d) => serde_json::json!(d),
            FieldValue::Logical(Some(b)) => serde_json::json!(b),
            _ => serde_json::json!(field_value_to_string(&value)),
        };
        map.insert(name, v);
    }
    serde_json::to_string(&map).unwrap_or_else(|_| "{}".to_string())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use dbase::TableWriterBuilder;

    use super::*;

    // ─── Helper: write a character-only DBF to a Vec<u8> ─────────────────────
    //
    // Uses dbase::TableWriterBuilder which requires Write + Seek.
    // Cursor<Vec<u8>> satisfies both.

    fn write_dbf(fields: &[&str], rows: Vec<Vec<(&str, &str)>>) -> Vec<u8> {
        let mut builder = TableWriterBuilder::new();
        for name in fields {
            builder = builder.add_character_field(
                dbase::FieldName::try_from(*name).expect("valid field name"),
                64,
            );
        }
        let mut cursor = Cursor::new(Vec::<u8>::new());
        let mut writer = builder.build_with_dest(&mut cursor);

        for row in rows {
            let mut record = Record::default();
            for (name, val) in row {
                record.insert(
                    name.to_string(),
                    FieldValue::Character(Some(val.to_string())),
                );
            }
            writer.write_record(&record).expect("write row");
        }
        // finalize is called in Drop; call explicitly to flush header.
        writer.finalize().expect("finalize");
        drop(writer);
        cursor.into_inner()
    }

    /// Same as `write_dbf`, but writes Character fields under an explicit encoding —
    /// i.e. what a real MS-DOS/Windows-era SAGA export actually looks like on disk,
    /// instead of dbase's `UnicodeLossy` (UTF-8) writer default.
    pub(super) fn write_dbf_with_encoding<E: dbase::Encoding + 'static>(
        encoding: E,
        fields: &[&str],
        rows: Vec<Vec<(&str, &str)>>,
    ) -> Vec<u8> {
        let mut builder = TableWriterBuilder::with_encoding(encoding);
        for name in fields {
            builder = builder.add_character_field(
                dbase::FieldName::try_from(*name).expect("valid field name"),
                64,
            );
        }
        let mut cursor = Cursor::new(Vec::<u8>::new());
        let mut writer = builder.build_with_dest(&mut cursor);

        for row in rows {
            let mut record = Record::default();
            for (name, val) in row {
                record.insert(
                    name.to_string(),
                    FieldValue::Character(Some(val.to_string())),
                );
            }
            writer.write_record(&record).expect("write row");
        }
        writer.finalize().expect("finalize");
        drop(writer);
        cursor.into_inner()
    }

    fn write_dbf_cp852(fields: &[&str], rows: Vec<Vec<(&str, &str)>>) -> Vec<u8> {
        write_dbf_with_encoding(dbase::yore::code_pages::CP852, fields, rows)
    }

    fn write_dbf_cp1250(fields: &[&str], rows: Vec<Vec<(&str, &str)>>) -> Vec<u8> {
        write_dbf_with_encoding(dbase::yore::code_pages::CP1250, fields, rows)
    }

    fn ctx_no_map() -> ParseCtx<'static> {
        ParseCtx {
            company_cui_canonical: "12345678",
            column_map: None,
        }
    }

    // ─── (a) Reordered columns + synonym (CIF instead of COD_FISCAL) ──────────

    #[test]
    fn parse_parteneri_reordered_cols_and_synonym() {
        // DBF has TELEFON, DENUMIRE, CIF (synonym), ADRESA in non-standard order.
        // Also includes an extra unknown column (NR_INTERN) that must be ignored.
        let fields = ["TELEFON", "DENUMIRE", "CIF", "ADRESA", "NR_INTERN"];
        let rows = vec![vec![
            ("TELEFON", "0722111222"),
            ("DENUMIRE", "SC Test SRL"),
            ("CIF", "RO12345678"),
            ("ADRESA", "Str. Libertatii nr. 1"),
            ("NR_INTERN", "ZZZ-999"),
        ]];
        let bytes = write_dbf(&fields, rows);

        let adapter = SagaDbfAdapter;
        let input = ImportInput::Bytes(bytes);
        let ctx = ctx_no_map();
        let data = adapter.parse(&input, &ctx).unwrap();

        assert_eq!(data.contacts.len(), 1, "should produce one contact");
        let c = &data.contacts[0];
        assert_eq!(c.legal_name.as_deref(), Some("SC Test SRL"), "name");
        assert_eq!(
            c.cui_canonical.as_deref(),
            Some("12345678"),
            "canonical CUI"
        );
        assert_eq!(c.phone.as_deref(), Some("0722111222"), "phone");
        assert_eq!(
            c.address.as_deref(),
            Some("Str. Libertatii nr. 1"),
            "address"
        );
    }

    // ─── (b) CP852-encoded Romanian diacritics survive the full parse pipeline ─
    //
    // End-to-end regression for the diacritics fix: a DBF whose Character fields are
    // genuinely CP852-encoded bytes (built via `write_dbf_cp852`, i.e. what a real
    // MS-DOS-era SAGA export looks like on disk — NOT dbase's default UTF-8 writer)
    // must come out the OTHER end of `SagaDbfAdapter::parse` with the correct
    // Romanian text, not U+FFFD replacement characters. This exercises the real
    // production path (`open_dbf_reader` → `pick_dbf_encoding` → `parse_as_contacts`
    // → `field_value_to_string_decoded`), unlike the old unit tests this replaces,
    // which only tested the superseded post-hoc byte-reinterpretation helpers
    // directly and never caught that `dbase::Reader::new`'s default `UnicodeLossy`
    // destroyed the original bytes before those helpers ever ran.
    #[test]
    fn cp852_encoded_dbf_diacritics_survive_full_parse() {
        // CP852 has no comma-below ș/ț codepoints (those are post-Unicode); real
        // MS-DOS-era Romanian text used the cedilla ş/ţ approximations, which
        // `RO_DIACRITICS` already treats as valid Romanian diacritics.
        let fields = ["CIF", "DENUMIRE", "LOCALITATE"];
        let rows = vec![vec![
            ("CIF", "RO12345678"),
            ("DENUMIRE", "SC Constanţa Trading SRL"),
            ("LOCALITATE", "Iaşi"),
        ]];
        let bytes = write_dbf_cp852(&fields, rows);

        // Sanity check: the raw bytes on disk are NOT valid UTF-8 (genuinely CP852).
        assert!(
            std::str::from_utf8(&bytes).is_err(),
            "fixture must contain real non-UTF-8 CP852 bytes, not ASCII/UTF-8 text"
        );

        let adapter = SagaDbfAdapter;
        let input = ImportInput::Bytes(bytes);
        let ctx = ctx_no_map();
        let data = adapter.parse(&input, &ctx).unwrap();

        assert_eq!(data.contacts.len(), 1, "should produce one contact");
        let c = &data.contacts[0];
        assert_eq!(
            c.legal_name.as_deref(),
            Some("SC Constanţa Trading SRL"),
            "CP852 'ţ' must decode to U+0163, not U+FFFD"
        );
        assert_eq!(
            c.city.as_deref(),
            Some("Iaşi"),
            "CP852 'ş' must decode to U+015F, not U+FFFD"
        );
        assert!(
            !c.legal_name.as_deref().unwrap_or("").contains('\u{FFFD}'),
            "no replacement characters in legal_name"
        );
        assert!(
            !c.city.as_deref().unwrap_or("").contains('\u{FFFD}'),
            "no replacement characters in city"
        );
    }

    #[test]
    fn pick_dbf_encoding_selects_cp852_for_cp852_file() {
        let fields = ["DENUMIRE"];
        let rows = vec![vec![("DENUMIRE", "Constanţa")]];
        let bytes = write_dbf_cp852(&fields, rows);

        let chosen = pick_dbf_encoding(&bytes);
        assert!(
            matches!(chosen, Some(DbfCodePage::Cp852)),
            "a genuinely CP852-encoded file must be detected as CP852"
        );
    }

    /// ASCII-leading file: the first SAMPLE_SIZE records carry no diacritics, the first
    /// one appears at record SAMPLE_SIZE+4. The head sample scores 0 everywhere, but the
    /// record area contains high bytes — the picker must widen to a full scan and still
    /// detect CP852 instead of falling back to lossy UTF-8 (which would corrupt 'ţ').
    #[test]
    fn pick_dbf_encoding_widens_sample_for_ascii_leading_file() {
        let fields = ["DENUMIRE"];
        let mut rows: Vec<Vec<(&str, &str)>> = (0..SAMPLE_SIZE + 3)
            .map(|_| vec![("DENUMIRE", "SC ASCII ONLY SRL")])
            .collect();
        rows.push(vec![("DENUMIRE", "Constanţa")]);
        let bytes = write_dbf_cp852(&fields, rows);

        assert!(
            record_area_has_high_bytes(&bytes),
            "fixture must contain a non-ASCII byte past the head sample"
        );
        let chosen = pick_dbf_encoding(&bytes);
        assert!(
            matches!(chosen, Some(DbfCodePage::Cp852)),
            "ASCII-leading CP852 file must widen the sample, not fall back lossy: got {chosen:?}"
        );
    }

    /// Pure-ASCII file: no high bytes anywhere → the picker returns None and the caller
    /// falls back to the crate default, which is harmless for ASCII-only data.
    #[test]
    fn pick_dbf_encoding_ascii_only_file_still_falls_back() {
        let fields = ["DENUMIRE"];
        let rows = vec![vec![("DENUMIRE", "SC ASCII ONLY SRL")]];
        let bytes = write_dbf_cp852(&fields, rows);

        assert!(!record_area_has_high_bytes(&bytes));
        assert!(pick_dbf_encoding(&bytes).is_none());
    }

    #[test]
    fn cp1250_encoded_dbf_diacritics_survive_full_parse() {
        // Neither CP852 nor CP1250 (real Windows-1250, confirmed against Python's stdlib
        // `cp1250` codec) has the MODERN comma-below ș/ț/Ș/Ț codepoints — those postdate
        // the legacy code pages. A real Windows-era SAGA export uses the cedilla ş/ţ/Ş/Ţ
        // approximations instead, same as a real MS-DOS/CP852 export would.
        let fields = ["CIF", "DENUMIRE", "LOCALITATE"];
        let rows = vec![vec![
            ("CIF", "RO87654321"),
            ("DENUMIRE", "SC Ploieşti Distribuţie SRL"),
            ("LOCALITATE", "Constanţa"),
        ]];
        let bytes = write_dbf_cp1250(&fields, rows);

        assert!(
            std::str::from_utf8(&bytes).is_err(),
            "fixture must contain real non-UTF-8 CP1250 bytes, not ASCII/UTF-8 text"
        );

        let chosen = pick_dbf_encoding(&bytes);
        assert!(
            matches!(chosen, Some(DbfCodePage::Cp1250)),
            "a genuinely CP1250-encoded file must be detected as CP1250: got {chosen:?}"
        );

        let adapter = SagaDbfAdapter;
        let input = ImportInput::Bytes(bytes);
        let ctx = ctx_no_map();
        let data = adapter.parse(&input, &ctx).unwrap();

        assert_eq!(data.contacts.len(), 1, "should produce one contact");
        let c = &data.contacts[0];
        assert_eq!(
            c.legal_name.as_deref(),
            Some("SC Ploieşti Distribuţie SRL"),
            "CP1250 'ş'/'ţ' must decode correctly, not U+FFFD"
        );
        assert_eq!(
            c.city.as_deref(),
            Some("Constanţa"),
            "CP1250 'ţ' must decode correctly, not U+FFFD"
        );
    }

    #[test]
    fn pick_dbf_encoding_prefers_cp1250_across_multiple_records_despite_0xee_collision() {
        // Regression for the single-record picker's mis-detection bug: CP852 and CP1250
        // share byte 0xEE, which decodes as 'ţ' (U+0163) under CP852 but as 'î' (U+00EE)
        // under CP1250 — both valid `RO_DIACRITICS`, so a picker that only samples the
        // FIRST record can pick CP852 for a genuinely-CP1250 file whenever that first
        // record happens to contain an 0xEE byte (CP852 is tried first in the candidate
        // list). Here the first record is exactly that kind of ambiguous single-diacritic
        // record ("Rîu" — only the collision byte, no other diacritic), while the
        // following records are UNAMBIGUOUSLY CP1250-only (ș/ț-adjacent cedilla forms in
        // positions CP852 does not share). The file-wide, multi-record score must prefer
        // CP1250 because it wins on the majority of sampled records, even though a
        // first-record-only heuristic would tie or favor CP852.
        let fields = ["DENUMIRE"];
        let rows = vec![
            vec![("DENUMIRE", "Rîu")], // ambiguous: 'î' only exists via the 0xEE collision
            vec![("DENUMIRE", "Ploieşti")],
            vec![("DENUMIRE", "Constanţa")],
            vec![("DENUMIRE", "Bacău")],
            vec![("DENUMIRE", "Craiova Distribuţie")],
        ];
        let bytes = write_dbf_cp1250(&fields, rows);

        let chosen = pick_dbf_encoding(&bytes);
        assert!(
            matches!(chosen, Some(DbfCodePage::Cp1250)),
            "multi-record scoring must prefer CP1250 for a genuinely CP1250 file even \
             when the first record is 0xEE-ambiguous: got {chosen:?}"
        );
    }

    #[test]
    fn pick_dbf_encoding_breaks_genuine_tie_in_favor_of_cp852() {
        // A file whose only Character content is the single 0xEE byte scores IDENTICALLY
        // under CP852 (-> 'ţ') and CP1250 (-> 'î'): both are valid RO_DIACRITICS, neither
        // produces U+FFFD — a genuine, dictionary-undecidable tie. The tie keeps the
        // earlier candidate: CP852, the historical SAGA MS-DOS default (this also keeps a
        // genuine CP852 file containing only 'ţ' — e.g. "Constanţa" — decoding correctly).
        // A real CP1250 file is NOT affected: its ş/ţ/ă bytes (0xBA/0xFE/0xE3) decode as
        // non-diacritic junk under CP852, so CP1250 wins on score, not on the tie-break.
        let fields = ["DENUMIRE"];
        // Written under CP1250 so the on-disk byte truly is 0xEE either way (both
        // candidate encodings map the ASCII portion identically; only 0xEE differs).
        let rows = vec![vec![("DENUMIRE", "î")]];
        let bytes = write_dbf_cp1250(&fields, rows);

        // Sanity: both candidates must actually score 1 (a genuine tie), not just one.
        assert_eq!(score_candidate(&bytes, DbfCodePage::Cp852), Some(1));
        assert_eq!(score_candidate(&bytes, DbfCodePage::Cp1250), Some(1));

        let chosen = pick_dbf_encoding(&bytes);
        assert!(
            matches!(chosen, Some(DbfCodePage::Cp852)),
            "a genuine CP852/CP1250 tie must keep the CP852 default: got {chosen:?}"
        );
    }

    #[test]
    fn pick_dbf_encoding_falls_back_to_none_for_plain_ascii() {
        // A plain-ASCII file has no diacritics to score on — no candidate wins,
        // and `open_dbf_reader` falls back to the crate default (UnicodeLossy),
        // which is correct and lossless for pure ASCII anyway.
        let fields = ["DENUMIRE"];
        let rows = vec![vec![("DENUMIRE", "Test SRL")]];
        let bytes = write_dbf(&fields, rows);

        assert_eq!(
            pick_dbf_encoding(&bytes),
            None,
            "plain ASCII has no diacritic signal, so no candidate should win"
        );
    }

    // ─── (c) detect_columns returns header names ──────────────────────────────

    #[test]
    fn detect_columns_returns_header_names() {
        let fields = ["COD_FISCAL", "DENUMIRE", "JUDET"];
        let rows = vec![vec![
            ("COD_FISCAL", "RO999"),
            ("DENUMIRE", "Test"),
            ("JUDET", "Ilfov"),
        ]];
        let bytes = write_dbf(&fields, rows);

        let adapter = SagaDbfAdapter;
        let input = ImportInput::Bytes(bytes);
        let detected = adapter.detect_columns(&input).unwrap();
        let names: Vec<_> = detected.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"COD_FISCAL"), "COD_FISCAL in detected");
        assert!(names.contains(&"DENUMIRE"), "DENUMIRE in detected");
        assert!(names.contains(&"JUDET"), "JUDET in detected");
    }

    // ─── (d) missing required column → warning, no panic ─────────────────────

    #[test]
    fn missing_required_column_emits_warning_no_panic() {
        // DBF with CIF but no DENUMIRE — must not panic; must emit a warning.
        let fields = ["CIF", "ADRESA"];
        let rows = vec![vec![("CIF", "RO111222"), ("ADRESA", "Str. Test")]];
        let bytes = write_dbf(&fields, rows);

        let adapter = SagaDbfAdapter;
        let input = ImportInput::Bytes(bytes);
        let ctx = ctx_no_map();
        let data = adapter.parse(&input, &ctx).unwrap(); // must not panic or error

        assert_eq!(data.contacts.len(), 1);
        assert!(
            data.contacts[0].legal_name.is_none(),
            "name should be absent"
        );
        // Warning about missing DENUMIRE expected.
        assert!(
            data.warnings.iter().any(|w| w.contains("DENUMIRE")),
            "expected DENUMIRE warning; got: {:?}",
            data.warnings
        );
    }

    // ─── (e) ctx.column_map override is honoured ─────────────────────────────

    #[test]
    fn column_map_override_honoured() {
        // DBF has a non-standard column "FIRMA" that is not in the default synonym
        // table. A column_map { "FIRMA" -> "DENUMIRE" } must resolve it to Name.
        let fields = ["COD_FISCAL", "FIRMA"];
        let rows = vec![vec![("COD_FISCAL", "RO77777"), ("FIRMA", "Override SRL")]];
        let bytes = write_dbf(&fields, rows);

        let mut column_map = super::super::ColumnMap::new();
        column_map.insert("FIRMA".to_string(), "DENUMIRE".to_string());
        let ctx = ParseCtx {
            company_cui_canonical: "12345678",
            column_map: Some(&column_map),
        };

        let adapter = SagaDbfAdapter;
        let input = ImportInput::Bytes(bytes);
        let data = adapter.parse(&input, &ctx).unwrap();

        assert_eq!(data.contacts.len(), 1);
        assert_eq!(
            data.contacts[0].legal_name.as_deref(),
            Some("Override SRL"),
            "column_map override must resolve FIRMA → name"
        );
    }

    // ─── product DBF (articole without CUI columns) ───────────────────────────

    #[test]
    fn parse_articole_dbf_takes_product_path() {
        // Product-only columns: CODARTICOL/NUMEARTICOL/UM/PROCTVA/PRET resolve ONLY via
        // product_synonym (none is a contact synonym), so contact_map is empty and the adapter
        // takes the product path automatically — no column_map needed. Columns are also REORDERED
        // (name before code) to prove name-based, not ordinal, mapping.
        let fields = ["NUMEARTICOL", "CODARTICOL", "UM", "PROCTVA", "PRET"];
        let rows = vec![
            vec![
                ("NUMEARTICOL", "Beton C20/25"),
                ("CODARTICOL", "ART-001"),
                ("UM", "MC"),
                ("PROCTVA", "19"),
                ("PRET", "350.00"),
            ],
            vec![
                ("NUMEARTICOL", "Ciment Portland"),
                ("CODARTICOL", "ART-002"),
                ("UM", "KG"),
                ("PROCTVA", "19"),
                ("PRET", "1.25"),
            ],
        ];
        let bytes = write_dbf(&fields, rows);
        let ctx = ParseCtx {
            company_cui_canonical: "12345678",
            column_map: None,
        };
        let data = SagaDbfAdapter
            .parse(&ImportInput::Bytes(bytes), &ctx)
            .unwrap();

        assert!(
            data.contacts.is_empty(),
            "a product DBF must NOT be parsed as contacts"
        );
        assert_eq!(data.products.len(), 2, "both article rows parsed");
        let p0 = &data.products[0];
        assert_eq!(p0.code.as_deref(), Some("ART-001"));
        assert_eq!(p0.name.as_deref(), Some("Beton C20/25"));
        assert_eq!(p0.unit.as_deref(), Some("MC"));
        assert_eq!(p0.vat_rate.as_deref(), Some("19"));
        assert_eq!(data.products[1].code.as_deref(), Some("ART-002"));
    }

    // ─── detect_columns sample values ────────────────────────────────────────

    #[test]
    fn detect_columns_includes_sample_values() {
        let fields = ["COD_FISCAL", "DENUMIRE"];
        let rows = vec![vec![("COD_FISCAL", "RO123"), ("DENUMIRE", "Firma Test")]];
        let bytes = write_dbf(&fields, rows);

        let adapter = SagaDbfAdapter;
        let input = ImportInput::Bytes(bytes);
        let detected = adapter.detect_columns(&input).unwrap();

        let cui_col = detected.iter().find(|d| d.name == "COD_FISCAL");
        assert!(cui_col.is_some(), "COD_FISCAL must be detected");
        assert_eq!(
            cui_col.unwrap().sample,
            "RO123",
            "sample must match first row value"
        );
    }
}
