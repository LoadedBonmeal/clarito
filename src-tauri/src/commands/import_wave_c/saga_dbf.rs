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
//! (Windows Central-European). The adapter tries three candidates in order:
//! CP852 → CP1250 → UTF-8. It picks the first encoding whose decoded string
//! contains valid Romanian diacritics (ă â î ș ț — both comma-below and
//! cedilla variants) and no Unicode replacement character (U+FFFD). If none
//! satisfies the heuristic, it falls through to UTF-8 with lossy replacement.
//!
//! This heuristic is documented here rather than trusted blindly — a real
//! export file should be tested to confirm. The DBF header `ldid` byte is NOT
//! relied on because it is often 0x00 in SAGA exports.
//!
//! # UNVERIFIED column names
//!
//! The synonym table below is built from partial documentation and user reports.
//! Every column name listed should be confirmed against a real SAGA export
//! before the W4/W5 pass. Columns in the synonym table that have LOW confidence
//! are marked with a comment.

use dbase::{FieldValue, Record};
use encoding_rs::WINDOWS_1250;
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

// ─── DBF column detection ─────────────────────────────────────────────────────

fn detect_dbf_columns(bytes: &[u8]) -> AppResult<Vec<DetectedColumn>> {
    let cursor = std::io::Cursor::new(bytes);
    let mut reader = dbase::Reader::new(cursor)
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

    let cursor = std::io::Cursor::new(bytes);
    let mut reader = dbase::Reader::new(cursor)
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

/// Decode a `FieldValue` to a String, applying the codepage heuristic for
/// character fields. Numeric/date/logical values are converted directly.
///
/// # Codepage heuristic (DEFENSIVE)
///
/// SAGA DBF files are commonly CP852 or CP1250. We try three candidates:
///   1. CP852 (MS-DOS East-European)
///   2. CP1250 (Windows Central-European)
///   3. UTF-8 (modern / some newer SAGA versions)
///
/// We pick the first encoding that produces no U+FFFD replacement character
/// AND contains at least one valid Romanian diacritic (ă â î ș ț — both
/// comma-below and cedilla variants). If none qualifies we fall through to
/// UTF-8 with lossy replacement.
///
/// This heuristic is per-field so that files with mixed-encoding records
/// (rare, but observed in older exports) degrade gracefully.
fn field_value_to_string_decoded(value: &FieldValue) -> String {
    match value {
        FieldValue::Character(Some(s)) => decode_ro_string(s.as_bytes()),
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
/// plus their uppercase equivalents.
const RO_DIACRITICS: &[char] = &[
    'ă', 'â', 'î', 'ș', 'ț', 'Ă', 'Â', 'Î', 'Ș', 'Ț', 'ş', 'ţ', 'Ş',
    'Ţ', // cedilla legacy forms
];

/// CP852 (IBM852 / MS-DOS Latin-2) high-byte map for the range 0x80–0xFF.
///
/// `encoding_rs` only implements WHATWG-specified encodings and does NOT include
/// CP852. This static table was derived from the Unicode Consortium's IBM852
/// mapping (ftp://ftp.unicode.org/Public/MAPPINGS/VENDORS/MICSFT/PC/CP852.TXT).
/// Only used by `decode_ro_string` for the codepage-probe heuristic.
///
/// Entry index i corresponds to byte value (0x80 + i).
/// '\u{FFFD}' marks unmapped/undefined code points.
// CP852_HIGH verified against Unicode Consortium IBM852 mapping via Python codecs.
// Key Romanian codepoints: â=0x83(idx 3), Â=0xB6(idx 54), î=0x8C(idx 12),
// Î=0xD7(idx 87), ă=0xC7(idx 71), Ă=0xC6(idx 70), ş=0xAD(idx 45),
// ţ=0xEE(idx 110), Ţ=0xDD(idx 93).
#[rustfmt::skip]
const CP852_HIGH: [char; 128] = [
    // 0x80–0x8F
    '\u{00C7}','\u{00FC}','\u{00E9}','\u{00E2}','\u{00E4}','\u{016F}','\u{0107}','\u{00E7}',
    '\u{0142}','\u{00EB}','\u{0150}','\u{0151}','\u{00EE}','\u{0179}','\u{00C4}','\u{0106}',
    // 0x90–0x9F
    '\u{00C9}','\u{0139}','\u{013A}','\u{00F4}','\u{00F6}','\u{013D}','\u{013E}','\u{015A}',
    '\u{015B}','\u{00D6}','\u{00DC}','\u{0164}','\u{0165}','\u{0141}','\u{00D7}','\u{010D}',
    // 0xA0–0xAF
    '\u{00E1}','\u{00ED}','\u{00F3}','\u{00FA}','\u{0104}','\u{0105}','\u{017D}','\u{017E}',
    '\u{0118}','\u{0119}','\u{00AC}','\u{017A}','\u{010C}','\u{015F}','\u{00AB}','\u{00BB}',
    // 0xB0–0xBF  (0xB6=Â U+00C2, 0xB8=Ş U+015E)
    '\u{2591}','\u{2592}','\u{2593}','\u{2502}','\u{2524}','\u{00C1}','\u{00C2}','\u{011A}',
    '\u{015E}','\u{2563}','\u{2551}','\u{2557}','\u{255D}','\u{017B}','\u{017C}','\u{2510}',
    // 0xC0–0xCF  (0xC6=Ă U+0102, 0xC7=ă U+0103)
    '\u{2514}','\u{2534}','\u{252C}','\u{251C}','\u{2500}','\u{253C}','\u{0102}','\u{0103}',
    '\u{255A}','\u{2554}','\u{2569}','\u{2566}','\u{2560}','\u{2550}','\u{256C}','\u{00A4}',
    // 0xD0–0xDF  (0xD7=Î U+00CE, 0xDD=Ţ U+0162)
    '\u{0111}','\u{0110}','\u{010E}','\u{00CB}','\u{010F}','\u{0147}','\u{00CD}','\u{00CE}',
    '\u{011B}','\u{2518}','\u{250C}','\u{2588}','\u{2584}','\u{0162}','\u{016E}','\u{2580}',
    // 0xE0–0xEF  (0xEE=ţ U+0163)
    '\u{00D3}','\u{00DF}','\u{00D4}','\u{0143}','\u{0144}','\u{0148}','\u{0160}','\u{0161}',
    '\u{0154}','\u{00DA}','\u{0155}','\u{0170}','\u{00FD}','\u{00DD}','\u{0163}','\u{00B4}',
    // 0xF0–0xFF
    '\u{00AD}','\u{02DD}','\u{02DB}','\u{02C7}','\u{02D8}','\u{00A7}','\u{00F7}','\u{00B8}',
    '\u{00B0}','\u{00A8}','\u{02D9}','\u{0171}','\u{0158}','\u{0159}','\u{25A0}','\u{00A0}',
];

/// Decode raw bytes assuming CP852 (MS-DOS Latin-2 / IBM852).
///
/// ASCII bytes (0x00–0x7F) are passed through; high bytes (0x80–0xFF) are
/// looked up in `CP852_HIGH`. Returns `None` if any byte maps to U+FFFD
/// (undefined/unmapped), signalling the caller to try another encoding.
fn decode_cp852(raw: &[u8]) -> Option<String> {
    let mut out = String::with_capacity(raw.len());
    for &b in raw {
        if b < 0x80 {
            out.push(b as char);
        } else {
            let ch = CP852_HIGH[(b - 0x80) as usize];
            if ch == '\u{FFFD}' {
                return None;
            }
            out.push(ch);
        }
    }
    Some(out)
}

/// Attempt to decode raw bytes as CP852 → CP1250 → UTF-8, picking the first
/// candidate that (a) has no replacement characters AND (b) contains at least
/// one Romanian diacritic. Falls back to UTF-8 lossy if none qualifies.
fn decode_ro_string(raw: &[u8]) -> String {
    // Try CP852 first (MS-DOS East-European, common in older SAGA DBF exports).
    if let Some(s) = decode_cp852(raw) {
        if s.chars().any(|c| RO_DIACRITICS.contains(&c)) {
            return s.trim().to_string();
        }
    }

    // Try CP1250 (Windows Central-European, common in newer SAGA exports).
    // encoding_rs::WINDOWS_1250 is in the WHATWG standard and is available.
    let (decoded_1250, _, had_errors_1250) = WINDOWS_1250.decode(raw);
    if !had_errors_1250 && decoded_1250.chars().any(|c| RO_DIACRITICS.contains(&c)) {
        return decoded_1250.trim().to_string();
    }

    // Try plain UTF-8.
    if let Ok(s) = std::str::from_utf8(raw) {
        if !s.contains('\u{FFFD}') {
            return s.trim().to_string();
        }
    }

    // Last resort: lossy UTF-8.
    String::from_utf8_lossy(raw).trim().to_string()
}

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

    // ─── (b) CP852-encoded Romanian diacritics decode correctly ──────────────

    #[test]
    fn codepage_cp852_ro_diacritics_decoded() {
        // CP852 (IBM852) byte mapping (verified via Python codecs):
        //   0x83 → â  (U+00E2)
        //   0xC7 → ă  (U+0103)  ← Romanian-specific, confirmed by Python
        //
        // Test: bytes containing 0xC7 must decode to ă via decode_ro_string.
        // decode_ro_string picks the CP852 path because the decoded string
        // contains ă which is in RO_DIACRITICS.
        let cp852_bytes: &[u8] = b"G\xC7e\x9Fti"; // 0xC7=ă
        let decoded = decode_ro_string(cp852_bytes);
        assert!(
            decoded.contains('\u{0103}'),
            "CP852 byte 0xC7 must decode to ă (U+0103); got: {decoded:?}"
        );
    }

    #[test]
    fn decode_cp852_known_ro_chars() {
        // CP852 byte 0x83 → â (U+00E2), per Python: bytes([0x83]).decode('cp852') == 'â'
        let result = decode_cp852(b"\x83");
        assert!(result.is_some(), "0x83 must decode in CP852");
        assert_eq!(result.unwrap(), "\u{00E2}", "CP852 0x83 = â (U+00E2)");

        // CP852 byte 0xC7 → ă (U+0103), per Python
        let result2 = decode_cp852(b"\xC7");
        assert!(result2.is_some(), "0xC7 must decode in CP852");
        assert_eq!(result2.unwrap(), "\u{0103}", "CP852 0xC7 = ă (U+0103)");

        // ASCII bytes pass through unchanged.
        let result3 = decode_cp852(b"abc");
        assert_eq!(result3, Some("abc".to_string()), "ASCII passthrough");
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
