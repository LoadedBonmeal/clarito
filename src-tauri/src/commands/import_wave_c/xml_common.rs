//! Shared XML parsing helpers for the SAGA-dialect XML format used by both
//! SmartBill "Export pentru Saga" and native SAGA C. XML exports.
//!
//! Both adapters share the same `<Factura>/<Antet>/<Linie>` schema (SmartBill
//! emits the format SAGA imports, so the two adapters call into the same
//! helpers here). Differences between the two sources are handled in the
//! caller adapters.
//!
//! ─── SCHEMA (documented-schema-based; pending real-file verification) ─────────
//!
//! Invoice file (F_*.xml / F_RO<cif>_multiple_<date>.xml / FUR_*.xml):
//!   Root element: UNVERIFIED — likely <Facturi> or bare multiple <Factura>
//!   elements under a single root; see UNVERIFIED list below.
//!
//!   Per invoice:
//!     <Factura>
//!       <Antet>
//!         <FurnizorCIF>   — issuer CIF (ISSUED when == company CIF)
//!         <FurnizorNume>
//!         <FurnizorNrRegCom>
//!         <ClientNume>
//!         <ClientCIF>
//!         <ClientNrRegCom>
//!         <ClientAdresa>
//!         <ClientLocalitate>
//!         <ClientJudet>
//!         <ClientTara>    — country CODE (e.g. "RO"), NOT a name
//!         <FacturaNumar>
//!         <FacturaData>
//!         <FacturaScadenta>
//!         <FacturaTaxareInversa>
//!         <FacturaTVAIncasare>
//!         <FacturaMoneda>
//!         <GUID_factura>  — optional; preferred external_id
//!         <GUID_cod_client> — optional; preferred contact external_id
//!       </Antet>
//!       <Linie>
//!         <Descriere>
//!         <CodArticolFurnizor>
//!         <UM>
//!         <Cantitate>
//!         <Pret>      — WITHOUT VAT (SAGA convention)
//!         <Valoare>   — net line total
//!         <ProcTVA>
//!         <TVA>       — VAT amount on line
//!         <Cont>      — often blank in SmartBill exports
//!         <Gestiune>
//!       </Linie>
//!       ...more <Linie>...
//!     </Factura>
//!
//! Client/partner file (CLI_*.xml):
//!   Root element: UNVERIFIED — likely <Clienti> or <Parteneri>
//!   Per record:
//!     (root element child)
//!       <Cod>
//!       <Denumire>
//!       <Cod_fiscal>
//!       <Reg_com>
//!       <Tara>
//!       <Judet>
//!       <Localitate>
//!       <Adresa>
//!       <Cont_banca>
//!       <Banca>
//!       <Tel>
//!       <Email>
//!       <Guid_cod>    — optional; preferred source_code / external_id
//!
//! Article/product file (ART_*.xml — SAGA native only):
//!   Root element: UNVERIFIED
//!   Per record:
//!     <Cod>
//!     <Denumire>
//!     <UM>
//!     <ProcTVA>
//!     <Pret>
//!     <Serviciu>    — UNVERIFIED element name; "D"/"N" flag
//!     <Guid_cod>    — optional
//!
//! ─── UNVERIFIED ELEMENT NAMES (pending a real export file) ──────────────────
//! * Root/container element for invoice files: guessed as <Facturi>; may be
//!   <Export>, <ImportDate>, or bare document-root with no wrapper.
//! * Root/container element for CLI_*.xml: guessed as <Clienti>; may differ.
//! * Root/container element for ART_*.xml: guessed as <Articole>; may differ.
//! * Per-record element in CLI_*.xml: guessed as <Client>; may be <Partener>.
//! * Per-record element in ART_*.xml: guessed as <Articol>; may differ.
//! * <Serviciu> element in ART_*.xml: guessed by analogy with WinMentor; not
//!   confirmed in the SAGA XML manual.
//! * Whether <GUID_factura> and <GUID_cod_client> appear in SAGA native XML or
//!   only in the SmartBill "Export pentru Saga" variant.
//! * Date format inside <FacturaData>/<FacturaScadenta>: assumed dd.mm.yyyy per
//!   the SAGA / SmartBill convention; verify against a real export.
//! * Encoding declaration: the <?xml ?> PI is honoured via encoding_rs; SAGA
//!   is documented as often ISO-8859-2 or Windows-1250, SmartBill as UTF-8.
//!   Files without an encoding declaration are assumed UTF-8.

use encoding_rs::Encoding;
use quick_xml::events::Event;
use quick_xml::Reader;
use rust_decimal::prelude::FromStr;
use rust_decimal::Decimal;

use crate::db::models::new_id;

use super::{StagedContact, StagedInvoice, StagedLine, StagedProduct};

// ─── Decoded bytes helper ─────────────────────────────────────────────────────

/// Decode raw bytes to a UTF-8 String, respecting the `<?xml encoding="…"?>`
/// declaration when present. Falls back to UTF-8 if no declaration is found or
/// the declared encoding is unknown.
pub fn decode_xml_bytes(raw: &[u8]) -> String {
    // Sniff the encoding declaration from the first 256 bytes.
    let sniff = std::str::from_utf8(&raw[..raw.len().min(256)]).unwrap_or("");
    let enc: &'static Encoding = if let Some(start) = sniff.find("encoding=\"") {
        let tail = &sniff[start + 10..];
        let end = tail.find('"').unwrap_or(tail.len());
        let label = &tail[..end];
        Encoding::for_label(label.as_bytes()).unwrap_or(encoding_rs::UTF_8)
    } else if let Some(start) = sniff.find("encoding='") {
        let tail = &sniff[start + 10..];
        let end = tail.find('\'').unwrap_or(tail.len());
        let label = &tail[..end];
        Encoding::for_label(label.as_bytes()).unwrap_or(encoding_rs::UTF_8)
    } else {
        encoding_rs::UTF_8
    };
    let (cow, _, _) = enc.decode(raw);
    cow.into_owned()
}

// ─── Low-level SAX helpers ────────────────────────────────────────────────────

/// Return the local name of an element (strips namespace prefix if present).
pub fn local_name(tag: &[u8]) -> String {
    let s = std::str::from_utf8(tag).unwrap_or("");
    // strip namespace prefix "ns:LocalName" → "LocalName"
    if let Some(pos) = s.rfind(':') {
        s[pos + 1..].to_string()
    } else {
        s.to_string()
    }
}

// ─── Parsed intermediate types ────────────────────────────────────────────────
// Both adapters use the same SAX-walk approach as `import_invoice_xml_inner`.

/// Raw header fields extracted from an `<Antet>` block.
#[derive(Debug, Default)]
pub struct Antet {
    pub furnizor_cif: String,
    pub furnizor_nume: String,
    pub furnizor_nr_reg_com: String,
    pub client_cif: String,
    pub client_nume: String,
    pub client_nr_reg_com: String,
    pub client_adresa: String,
    pub client_localitate: String,
    pub client_judet: String,
    /// Country CODE (e.g. "RO") — NOT a name. W4 handles normalisation.
    /// REST path (W3) carries a country NAME instead — noted in both adapters.
    pub client_tara: String,
    pub factura_numar: String,
    pub factura_data: String,
    pub factura_scadenta: String,
    pub factura_taxare_inversa: String,
    pub factura_tva_incasare: String,
    pub factura_moneda: String,
    /// Optional GUID — preferred as `external_id`. May be empty.
    pub guid_factura: String,
    /// Optional GUID for the client record — preferred as partner `source_code`.
    pub guid_cod_client: String,
    // SAGA native sometimes carries a FacturaSerie separate from FacturaNumar.
    // UNVERIFIED: element name "FacturaSerie" is inferred by analogy; confirm.
    pub factura_serie: String,
}

/// Raw line fields extracted from a `<Linie>` block.
#[derive(Debug, Default)]
pub struct Linie {
    pub descriere: String,
    pub cod_articol_furnizor: String,
    pub um: String,
    pub cantitate: String,
    /// Unit price WITHOUT VAT — the SAGA/SmartBill "Export pentru Saga" dialect always emits net
    /// prices, so it is consumed as-is (no VAT back-out path; a VAT-inclusive source is out of scope).
    pub pret: String,
    /// Net line total (Cantitate * Pret).
    pub valoare: String,
    pub proc_tva: String,
    /// VAT amount on the line.
    pub tva: String,
    /// Account code — often blank in SmartBill exports; tolerated as None.
    pub cont: String,
    pub gestiune: String,
}

// ─── Core parser: one XML document → Vec<(Antet, Vec<Linie>)> ─────────────────

/// Parse a SAGA-dialect invoice XML string into a list of (Antet, lines) pairs.
///
/// The function is tolerant: missing elements produce empty strings; unknown
/// elements are silently ignored; `Err` is only returned for truly unreadable XML.
///
/// UNVERIFIED: the exact root/container element is not confirmed from a real file.
/// We treat any element depth containing an `<Antet>` child as an invoice record,
/// regardless of the root tag name — this is robust against root variants.
pub fn parse_invoice_xml(
    xml: &str,
    warnings: &mut Vec<String>,
) -> Result<Vec<(Antet, Vec<Linie>)>, String> {
    let xml = xml.trim_start_matches('\u{FEFF}'); // strip BOM
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut results: Vec<(Antet, Vec<Linie>)> = Vec::new();

    // State machine
    let mut in_factura = false;
    let mut in_antet = false;
    let mut in_linie = false;
    let mut current_tag = String::new();
    let mut current_antet = Antet::default();
    let mut current_linie = Linie::default();
    let mut current_lines: Vec<Linie> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let name = local_name(e.local_name().into_inner());
                match name.as_str() {
                    "Factura" => {
                        in_factura = true;
                        current_antet = Antet::default();
                        current_lines = Vec::new();
                    }
                    "Antet" if in_factura => {
                        in_antet = true;
                    }
                    "Linie" if in_factura => {
                        in_linie = true;
                        current_linie = Linie::default();
                    }
                    _ => {}
                }
                current_tag = name;
            }
            Ok(Event::End(ref e)) => {
                let name = local_name(e.local_name().into_inner());
                match name.as_str() {
                    "Factura" => {
                        if in_factura {
                            results.push((
                                std::mem::take(&mut current_antet),
                                std::mem::take(&mut current_lines),
                            ));
                        }
                        in_factura = false;
                        in_antet = false;
                        in_linie = false;
                    }
                    "Antet" => {
                        in_antet = false;
                    }
                    "Linie" => {
                        if in_linie {
                            current_lines.push(std::mem::take(&mut current_linie));
                        }
                        in_linie = false;
                    }
                    _ => {}
                }
                current_tag.clear();
            }
            Ok(Event::Text(ref e)) => {
                let text = match e.unescape() {
                    Ok(t) => t.trim().to_string(),
                    Err(_) => {
                        buf.clear();
                        continue;
                    }
                };
                if text.is_empty() {
                    buf.clear();
                    continue;
                }
                if in_antet {
                    match current_tag.as_str() {
                        "FurnizorCIF" => current_antet.furnizor_cif = text,
                        "FurnizorNume" => current_antet.furnizor_nume = text,
                        "FurnizorNrRegCom" => current_antet.furnizor_nr_reg_com = text,
                        "ClientCIF" => current_antet.client_cif = text,
                        "ClientNume" => current_antet.client_nume = text,
                        "ClientNrRegCom" => current_antet.client_nr_reg_com = text,
                        "ClientAdresa" => current_antet.client_adresa = text,
                        "ClientLocalitate" => current_antet.client_localitate = text,
                        "ClientJudet" => current_antet.client_judet = text,
                        "ClientTara" => current_antet.client_tara = text,
                        "FacturaNumar" => current_antet.factura_numar = text,
                        "FacturaData" => current_antet.factura_data = text,
                        "FacturaScadenta" => current_antet.factura_scadenta = text,
                        "FacturaTaxareInversa" => current_antet.factura_taxare_inversa = text,
                        "FacturaTVAIncasare" => current_antet.factura_tva_incasare = text,
                        "FacturaMoneda" => current_antet.factura_moneda = text,
                        "GUID_factura" => current_antet.guid_factura = text,
                        "GUID_cod_client" => current_antet.guid_cod_client = text,
                        // UNVERIFIED: "FacturaSerie" — inferred element name for series
                        "FacturaSerie" => current_antet.factura_serie = text,
                        _ => {} // unknown elements silently ignored
                    }
                } else if in_linie {
                    match current_tag.as_str() {
                        "Descriere" => current_linie.descriere = text,
                        "CodArticolFurnizor" => current_linie.cod_articol_furnizor = text,
                        "UM" => current_linie.um = text,
                        "Cantitate" => current_linie.cantitate = text,
                        "Pret" => current_linie.pret = text,
                        "Valoare" => current_linie.valoare = text,
                        "ProcTVA" => current_linie.proc_tva = text,
                        "TVA" => current_linie.tva = text,
                        "Cont" => current_linie.cont = text,
                        "Gestiune" => current_linie.gestiune = text,
                        _ => {}
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(format!("XML parse error: {e}"));
            }
            _ => {}
        }
        buf.clear();
    }

    if results.is_empty() && !xml.contains("<Factura") {
        warnings.push(
            "xml_common: nu s-au găsit elemente <Factura> în fișier — \
             verificați că fișierul este un export SAGA/SmartBill de facturi."
                .to_string(),
        );
    }

    Ok(results)
}

// ─── Partner-file parser ──────────────────────────────────────────────────────

/// Raw fields for one partner/client record from a CLI_*.xml file.
#[derive(Debug, Default)]
pub struct PartenerRecord {
    pub cod: String,
    pub denumire: String,
    pub cod_fiscal: String,
    pub reg_com: String,
    pub tara: String,
    pub judet: String,
    pub localitate: String,
    pub adresa: String,
    pub cont_banca: String,
    pub banca: String,
    pub tel: String,
    pub email: String,
    /// Stable external GUID — preferred `source_code` when present.
    pub guid_cod: String,
}

/// Parse a SAGA-dialect partner/client XML (CLI_*.xml) into raw records.
///
/// UNVERIFIED: the per-record element name is assumed to be `<Client>` or
/// `<Partener>`; we accept any direct child of the root that contains a `<Cod>`
/// or `<Denumire>` child — i.e. depth-2 elements.
///
/// UNVERIFIED: the root/container element name.
pub fn parse_cli_xml(xml: &str, warnings: &mut Vec<String>) -> Result<Vec<PartenerRecord>, String> {
    let xml = xml.trim_start_matches('\u{FEFF}');
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut results: Vec<PartenerRecord> = Vec::new();

    // We use depth-tracking: depth 0 = document, 1 = root, 2 = per-record element.
    let mut depth: u32 = 0;
    let mut in_record = false; // depth == 2
    let mut current: PartenerRecord = PartenerRecord::default();
    let mut current_tag = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                depth += 1;
                let name = local_name(e.local_name().into_inner());
                if depth == 2 {
                    // Start of a per-record element (Client / Partener / etc.)
                    in_record = true;
                    current = PartenerRecord::default();
                }
                current_tag = name;
            }
            Ok(Event::End(ref e)) => {
                let name = local_name(e.local_name().into_inner());
                if depth == 2 && in_record {
                    // End of the per-record element.
                    if !current.denumire.is_empty() || !current.cod_fiscal.is_empty() {
                        results.push(std::mem::take(&mut current));
                    }
                    in_record = false;
                }
                depth = depth.saturating_sub(1);
                let _ = name;
                current_tag.clear();
            }
            Ok(Event::Text(ref e)) => {
                if !in_record {
                    buf.clear();
                    continue;
                }
                let text = match e.unescape() {
                    Ok(t) => t.trim().to_string(),
                    Err(_) => {
                        buf.clear();
                        continue;
                    }
                };
                if text.is_empty() {
                    buf.clear();
                    continue;
                }
                match current_tag.as_str() {
                    "Cod" => current.cod = text,
                    "Denumire" => current.denumire = text,
                    "Cod_fiscal" => current.cod_fiscal = text,
                    "Reg_com" => current.reg_com = text,
                    "Tara" => current.tara = text,
                    "Judet" => current.judet = text,
                    "Localitate" => current.localitate = text,
                    "Adresa" => current.adresa = text,
                    "Cont_banca" => current.cont_banca = text,
                    "Banca" => current.banca = text,
                    "Tel" => current.tel = text,
                    "Email" => current.email = text,
                    "Guid_cod" => current.guid_cod = text,
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(format!("XML CLI parse error: {e}"));
            }
            _ => {}
        }
        buf.clear();
    }

    if results.is_empty() {
        warnings.push(
            "xml_common: nu s-au găsit înregistrări de parteneri în fișierul CLI_*.xml — \
             verificați că fișierul este un export SAGA/SmartBill de clienți."
                .to_string(),
        );
    }

    Ok(results)
}

// ─── Article/product file parser (SAGA native ART_*.xml) ─────────────────────

/// Raw fields for one article record from an ART_*.xml file.
#[derive(Debug, Default)]
pub struct ArticolRecord {
    pub cod: String,
    pub denumire: String,
    pub um: String,
    pub proc_tva: String,
    pub pret: String,
    /// "D" = serviciu, "N" = stocabil.
    /// UNVERIFIED element name: assumed "Serviciu" by analogy with WinMentor.
    pub serviciu: String,
    /// Optional stable GUID — preferred `source_code`.
    pub guid_cod: String,
}

/// Parse a SAGA-dialect article/product XML (ART_*.xml) into raw records.
///
/// UNVERIFIED: root element name and per-record element name.
pub fn parse_art_xml(xml: &str, warnings: &mut Vec<String>) -> Result<Vec<ArticolRecord>, String> {
    let xml = xml.trim_start_matches('\u{FEFF}');
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();
    let mut results: Vec<ArticolRecord> = Vec::new();

    let mut depth: u32 = 0;
    let mut in_record = false;
    let mut current: ArticolRecord = ArticolRecord::default();
    let mut current_tag = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                depth += 1;
                let name = local_name(e.local_name().into_inner());
                if depth == 2 {
                    in_record = true;
                    current = ArticolRecord::default();
                }
                current_tag = name;
            }
            Ok(Event::End(_)) => {
                if depth == 2 && in_record {
                    if !current.denumire.is_empty() || !current.cod.is_empty() {
                        results.push(std::mem::take(&mut current));
                    }
                    in_record = false;
                }
                depth = depth.saturating_sub(1);
                current_tag.clear();
            }
            Ok(Event::Text(ref e)) => {
                if !in_record {
                    buf.clear();
                    continue;
                }
                let text = match e.unescape() {
                    Ok(t) => t.trim().to_string(),
                    Err(_) => {
                        buf.clear();
                        continue;
                    }
                };
                if text.is_empty() {
                    buf.clear();
                    continue;
                }
                match current_tag.as_str() {
                    "Cod" => current.cod = text,
                    "Denumire" => current.denumire = text,
                    "UM" => current.um = text,
                    "ProcTVA" => current.proc_tva = text,
                    "Pret" => current.pret = text,
                    // UNVERIFIED element name "Serviciu"
                    "Serviciu" => current.serviciu = text,
                    "Guid_cod" => current.guid_cod = text,
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(format!("XML ART parse error: {e}"));
            }
            _ => {}
        }
        buf.clear();
    }

    if results.is_empty() {
        warnings.push(
            "xml_common: nu s-au găsit înregistrări de articole în fișierul ART_*.xml.".to_string(),
        );
    }

    Ok(results)
}

// ─── Conversion helpers ───────────────────────────────────────────────────────

/// Parse a decimal string (tolerating comma as thousands separator or
/// Romanian decimal comma). Returns None + logs a warning on failure.
pub fn parse_decimal(s: &str, field: &str, warnings: &mut Vec<String>) -> Option<Decimal> {
    // Normalise the thousands/decimal separators. When BOTH ',' and '.' appear, the RIGHTMOST is
    // the decimal separator and the other is the thousands grouping — this disambiguates European
    // "1.234,56" (→1234.56) AND US "1,234.56" (→1234.56) without corrupting either. With a single
    // separator we treat it as the decimal (',' → '.'), which is correct for both the European
    // comma-decimal and the dot-decimal SmartBill-REST convention.
    let normalised = if s.contains(',') && s.contains('.') {
        if s.rfind(',') > s.rfind('.') {
            // comma is the decimal mark: drop the '.' thousands, comma → '.'
            s.replace('.', "").replace(',', ".")
        } else {
            // dot is the decimal mark: drop the ',' thousands
            s.replace(',', "")
        }
    } else {
        s.replace(',', ".")
    };
    match Decimal::from_str(normalised.trim()) {
        Ok(d) => Some(d),
        Err(_) => {
            if !s.is_empty() {
                warnings.push(format!(
                    "xml_common: nu pot parsa '{s}' ca număr pentru câmpul '{field}'"
                ));
            }
            None
        }
    }
}

/// Format a Decimal to a 2-decimal-place String (staging TEXT convention).
pub fn fmt_decimal(d: Decimal) -> String {
    d.round_dp_with_strategy(2, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
        .to_string()
}

/// Determine invoice direction: "ISSUED" if the issuer CIF canonicalises to
/// `company_cui_canonical`, else "RECEIVED".
pub fn invoice_direction(issuer_cif: &str, company_cui_canonical: &str) -> &'static str {
    let canon = super::canonical_cui(issuer_cif);
    if canon == company_cui_canonical {
        "ISSUED"
    } else {
        "RECEIVED"
    }
}

/// True-ish flag values used in SAGA/SmartBill XML.
pub fn is_true_flag(s: &str) -> bool {
    matches!(
        s.trim().to_uppercase().as_str(),
        "D" | "DA" | "TRUE" | "1" | "YES"
    )
}

// ─── Staged-struct builders ───────────────────────────────────────────────────

/// Convert a `PartenerRecord` into a `StagedContact`.
pub fn partener_to_staged_contact(rec: &PartenerRecord, source: &str) -> StagedContact {
    use super::canonical_cui;

    let cui_raw = if rec.cod_fiscal.is_empty() {
        None
    } else {
        Some(rec.cod_fiscal.clone())
    };
    let cui_canonical = cui_raw.as_deref().map(canonical_cui);
    // Use GUID when present; fall back to Cod (internal nomenclator code).
    let source_code = if !rec.guid_cod.is_empty() {
        Some(rec.guid_cod.clone())
    } else if !rec.cod.is_empty() {
        Some(rec.cod.clone())
    } else {
        None
    };
    let dedup_key = cui_canonical.clone().filter(|s| !s.is_empty());

    let raw_json = serde_json::json!({
        "cod": rec.cod,
        "denumire": rec.denumire,
        "cod_fiscal": rec.cod_fiscal,
        "reg_com": rec.reg_com,
        "tara": rec.tara,
        "judet": rec.judet,
        "localitate": rec.localitate,
        "adresa": rec.adresa,
        "tel": rec.tel,
        "email": rec.email,
        "guid_cod": rec.guid_cod,
    })
    .to_string();

    StagedContact {
        id: new_id(),
        source: source.to_string(),
        raw_json,
        source_code,
        contact_type: None,
        cui_raw,
        cui_canonical,
        legal_name: if rec.denumire.is_empty() {
            None
        } else {
            Some(rec.denumire.clone())
        },
        vat_payer: None,
        is_individual: None,
        address: if rec.adresa.is_empty() {
            None
        } else {
            Some(rec.adresa.clone())
        },
        city: if rec.localitate.is_empty() {
            None
        } else {
            Some(rec.localitate.clone())
        },
        county: if rec.judet.is_empty() {
            None
        } else {
            Some(rec.judet.clone())
        },
        // Country code passed through as-is (W4 handles normalisation).
        // NOTE: the REST path (W3) carries a country NAME here instead.
        country: if rec.tara.is_empty() {
            None
        } else {
            Some(rec.tara.clone())
        },
        email: if rec.email.is_empty() {
            None
        } else {
            Some(rec.email.clone())
        },
        phone: if rec.tel.is_empty() {
            None
        } else {
            Some(rec.tel.clone())
        },
        dedup_key,
    }
}

/// Convert an `ArticolRecord` into a `StagedProduct`.
pub fn articol_to_staged_product(rec: &ArticolRecord, source: &str) -> StagedProduct {
    // Use GUID as preferred source_code; fall back to Cod.
    let source_code = if !rec.guid_cod.is_empty() {
        Some(rec.guid_cod.clone())
    } else if !rec.cod.is_empty() {
        Some(rec.cod.clone())
    } else {
        None
    };

    let raw_json = serde_json::json!({
        "cod": rec.cod,
        "denumire": rec.denumire,
        "um": rec.um,
        "proc_tva": rec.proc_tva,
        "pret": rec.pret,
        "serviciu": rec.serviciu,
        "guid_cod": rec.guid_cod,
    })
    .to_string();

    StagedProduct {
        id: new_id(),
        source: source.to_string(),
        raw_json,
        source_code: source_code.clone(),
        name: if rec.denumire.is_empty() {
            None
        } else {
            Some(rec.denumire.clone())
        },
        unit: if rec.um.is_empty() {
            None
        } else {
            Some(rec.um.clone())
        },
        unit_price: if rec.pret.is_empty() {
            None
        } else {
            Some(rec.pret.clone())
        },
        vat_rate: if rec.proc_tva.is_empty() {
            None
        } else {
            Some(rec.proc_tva.clone())
        },
        vat_category: None,
        code: if rec.cod.is_empty() {
            None
        } else {
            Some(rec.cod.clone())
        },
        barcode: None,
        stock_qty: None,
        is_service: if rec.serviciu.is_empty() {
            None
        } else {
            Some(is_true_flag(&rec.serviciu))
        },
        dedup_key: source_code.clone(),
    }
}

/// Build a `StagedInvoice` + its `StagedLine`s from a parsed `(Antet, Vec<Linie>)`.
///
/// `source` = source string for staging rows.
/// `company_cui_canonical` = canonical CUI of the importing company (for direction).
/// `warnings` = mutable warnings accumulator.
pub fn build_staged_invoice(
    antet: &Antet,
    lines: &[Linie],
    source: &str,
    company_cui_canonical: &str,
    warnings: &mut Vec<String>,
) -> StagedInvoice {
    use super::canonical_cui;

    let direction = invoice_direction(&antet.furnizor_cif, company_cui_canonical);

    // The partner is the OTHER party: if ISSUED → partner is the client;
    // if RECEIVED → partner is the supplier (furnizor).
    let (partner_name, partner_cif_raw) = if direction == "ISSUED" {
        (antet.client_nume.clone(), antet.client_cif.clone())
    } else {
        (antet.furnizor_nume.clone(), antet.furnizor_cif.clone())
    };
    let partner_cui_canonical = if partner_cif_raw.is_empty() {
        None
    } else {
        let c = canonical_cui(&partner_cif_raw);
        if c.is_empty() {
            None
        } else {
            Some(c)
        }
    };

    // External id: GUID_factura preferred; absent → None (W4 uses series+number+date)
    let external_id = if !antet.guid_factura.is_empty() {
        Some(antet.guid_factura.clone())
    } else {
        None
    };

    // Series + number: SAGA sometimes embeds the series inside FacturaNumar as
    // a prefix (e.g. "FACT0001"); we store the raw value and let W4 split.
    // If FacturaSerie is populated we use it directly.
    let series = if !antet.factura_serie.is_empty() {
        Some(antet.factura_serie.clone())
    } else {
        None
    };
    let number = if !antet.factura_numar.is_empty() {
        Some(antet.factura_numar.clone())
    } else {
        None
    };
    let full_number = match (&series, &number) {
        (Some(s), Some(n)) => Some(format!("{s}{n}")),
        (None, Some(n)) => Some(n.clone()),
        _ => None,
    };

    // Currency — default RON when absent
    let currency = if !antet.factura_moneda.is_empty() {
        Some(antet.factura_moneda.clone())
    } else {
        Some("RON".to_string())
    };

    let reverse_charge = if !antet.factura_taxare_inversa.is_empty() {
        Some(is_true_flag(&antet.factura_taxare_inversa))
    } else {
        None
    };
    let cash_vat = if !antet.factura_tva_incasare.is_empty() {
        Some(is_true_flag(&antet.factura_tva_incasare))
    } else {
        None
    };

    // Build lines and accumulate header totals from lines.
    let mut total_net = Decimal::ZERO;
    let mut total_vat = Decimal::ZERO;
    let mut staged_lines: Vec<StagedLine> = Vec::new();

    for (pos, linie) in lines.iter().enumerate() {
        // If Cont is blank, log a warning but don't fail.
        if linie.cont.is_empty() {
            // This is expected for SmartBill exports — note silently.
            // Only emit a warning if there's no Cont AND no account can be inferred.
        }

        // Parse amounts — SAGA prices are WITHOUT VAT.
        let cantitate = parse_decimal(&linie.cantitate, "Cantitate", warnings);
        let pret = parse_decimal(&linie.pret, "Pret", warnings);
        let valoare_parsed = parse_decimal(&linie.valoare, "Valoare", warnings);
        let proc_tva = parse_decimal(&linie.proc_tva, "ProcTVA", warnings);
        let tva_parsed = parse_decimal(&linie.tva, "TVA", warnings);

        // Compute net = Cantitate * Pret (prefer the source Valoare when present
        // and consistent, fall back to computed).
        let net_line = if let (Some(qty), Some(price)) = (cantitate, pret) {
            let computed = qty * price;
            if let Some(src_val) = valoare_parsed {
                // Accept source value; warn if > 0.01 discrepancy.
                let diff = (computed - src_val).abs();
                if diff > Decimal::new(1, 2) {
                    warnings.push(format!(
                        "Linie {}: Valoare sursă ({}) diferă de Cantitate*Pret ({}) cu {}",
                        pos + 1,
                        fmt_decimal(src_val),
                        fmt_decimal(computed),
                        fmt_decimal(diff)
                    ));
                }
                src_val
            } else {
                computed
            }
        } else if let Some(src_val) = valoare_parsed {
            src_val
        } else {
            warnings.push(format!(
                "Linie {}: nu s-a putut determina valoarea netă (Cantitate/Pret/Valoare lipsă sau invalide)",
                pos + 1
            ));
            Decimal::ZERO
        };

        // VAT amount: prefer source TVA; fall back to computed.
        let vat_rate_dec = proc_tva.unwrap_or(Decimal::ZERO);
        let computed_vat = net_line * vat_rate_dec / Decimal::ONE_HUNDRED;
        let vat_line = if let Some(src_tva) = tva_parsed {
            let diff = (computed_vat - src_tva).abs();
            if diff > Decimal::new(1, 2) {
                warnings.push(format!(
                    "Linie {}: TVA sursă ({}) diferă de calculat ({}) cu {}",
                    pos + 1,
                    fmt_decimal(src_tva),
                    fmt_decimal(computed_vat),
                    fmt_decimal(diff)
                ));
            }
            src_tva
        } else {
            computed_vat
        };

        let total_line = net_line + vat_line;
        total_net += net_line;
        total_vat += vat_line;

        let account_code = if linie.cont.is_empty() {
            None
        } else {
            Some(linie.cont.clone())
        };

        staged_lines.push(StagedLine {
            id: new_id(),
            position: (pos + 1) as i32,
            name: if linie.descriere.is_empty() {
                None
            } else {
                Some(linie.descriere.clone())
            },
            description: None,
            product_code: if linie.cod_articol_furnizor.is_empty() {
                None
            } else {
                Some(linie.cod_articol_furnizor.clone())
            },
            quantity: cantitate.map(fmt_decimal),
            unit: if linie.um.is_empty() {
                None
            } else {
                Some(linie.um.clone())
            },
            unit_price: pret.map(fmt_decimal),
            vat_rate: proc_tva.map(fmt_decimal),
            vat_category: None,
            subtotal_amount: Some(fmt_decimal(net_line)),
            vat_amount: Some(fmt_decimal(vat_line)),
            total_amount: Some(fmt_decimal(total_line)),
            account_code,
            warehouse: if linie.gestiune.is_empty() {
                None
            } else {
                Some(linie.gestiune.clone())
            },
        });
    }

    let total_gross = total_net + total_vat;

    let raw_json = serde_json::json!({
        "furnizor_cif": antet.furnizor_cif,
        "furnizor_nume": antet.furnizor_nume,
        "client_cif": antet.client_cif,
        "client_nume": antet.client_nume,
        "factura_numar": antet.factura_numar,
        "factura_data": antet.factura_data,
        "guid_factura": antet.guid_factura,
    })
    .to_string();

    let dedup_key = match (&full_number, &antet.factura_data) {
        (Some(n), d) if !d.is_empty() => Some(format!("{n}|{d}")),
        _ => None,
    };

    StagedInvoice {
        id: new_id(),
        source: source.to_string(),
        raw_json,
        direction: direction.to_string(),
        external_id,
        partner_cui_canonical,
        partner_name: if partner_name.is_empty() {
            None
        } else {
            Some(partner_name)
        },
        series,
        number,
        full_number,
        issue_date: if antet.factura_data.is_empty() {
            None
        } else {
            Some(antet.factura_data.clone())
        },
        due_date: if antet.factura_scadenta.is_empty() {
            None
        } else {
            Some(antet.factura_scadenta.clone())
        },
        currency,
        exchange_rate: None,
        reverse_charge,
        cash_vat,
        subtotal_amount: Some(fmt_decimal(total_net)),
        vat_amount: Some(fmt_decimal(total_vat)),
        total_amount: Some(fmt_decimal(total_gross)),
        dedup_key,
        lines: staged_lines,
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_decimal_dot_format() {
        let mut w = vec![];
        let d = parse_decimal("1234.56", "test", &mut w).unwrap();
        assert_eq!(d, Decimal::new(123456, 2));
        assert!(w.is_empty());
    }

    #[test]
    fn parse_decimal_european_format() {
        let mut w = vec![];
        // "1.234,56" → 1234.56
        let d = parse_decimal("1.234,56", "test", &mut w).unwrap();
        assert_eq!(d, Decimal::new(123456, 2));
    }

    #[test]
    fn parse_decimal_us_format_not_corrupted() {
        // Both-separator disambiguation: rightmost is the decimal. US "1,234.56" must NOT become
        // 1.23456 (the old naive replace bug). European "1.234,56" stays 1234.56.
        let mut w = vec![];
        assert_eq!(
            parse_decimal("1,234.56", "test", &mut w).unwrap(),
            Decimal::new(123456, 2)
        );
        assert_eq!(
            parse_decimal("1.234,56", "test", &mut w).unwrap(),
            Decimal::new(123456, 2)
        );
        assert!(
            w.is_empty(),
            "both well-formed numbers should parse without a warning"
        );
    }

    #[test]
    fn parse_decimal_empty_no_warning() {
        let mut w = vec![];
        let d = parse_decimal("", "test", &mut w);
        assert!(d.is_none());
        assert!(w.is_empty(), "empty string should not emit a warning");
    }

    #[test]
    fn invoice_direction_issued() {
        // Company CUI == issuer CIF → ISSUED
        assert_eq!(invoice_direction("RO12345678", "12345678"), "ISSUED");
    }

    #[test]
    fn invoice_direction_received() {
        assert_eq!(invoice_direction("RO99999999", "12345678"), "RECEIVED");
    }

    #[test]
    fn is_true_flag_variants() {
        assert!(is_true_flag("D"));
        assert!(is_true_flag("Da"));
        assert!(is_true_flag("TRUE"));
        assert!(!is_true_flag("N"));
        assert!(!is_true_flag("NU"));
    }

    #[test]
    fn decode_xml_bytes_utf8() {
        let xml = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><root/>";
        let s = decode_xml_bytes(xml);
        assert!(s.contains("<root/>"));
    }

    /// Minimal invoice XML fixture — documented-schema-based, pending real-file verification.
    #[test]
    fn parse_invoice_xml_basic() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Facturi>
  <Factura>
    <Antet>
      <FurnizorCIF>RO12345678</FurnizorCIF>
      <FurnizorNume>Test SRL</FurnizorNume>
      <ClientCIF>RO87654321</ClientCIF>
      <ClientNume>Client SA</ClientNume>
      <ClientTara>RO</ClientTara>
      <FacturaNumar>1</FacturaNumar>
      <FacturaData>01.06.2026</FacturaData>
      <FacturaMoneda>RON</FacturaMoneda>
      <GUID_factura>abc-123</GUID_factura>
    </Antet>
    <Linie>
      <Descriere>Servicii</Descriere>
      <Cantitate>2</Cantitate>
      <Pret>100.00</Pret>
      <Valoare>200.00</Valoare>
      <ProcTVA>19</ProcTVA>
      <TVA>38.00</TVA>
    </Linie>
  </Factura>
</Facturi>"#;
        let mut w = vec![];
        let invoices = parse_invoice_xml(xml, &mut w).unwrap();
        assert_eq!(invoices.len(), 1);
        let (antet, lines) = &invoices[0];
        assert_eq!(antet.furnizor_cif, "RO12345678");
        assert_eq!(antet.client_cif, "RO87654321");
        assert_eq!(antet.guid_factura, "abc-123");
        assert_eq!(antet.client_tara, "RO");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].cantitate, "2");
        assert_eq!(lines[0].pret, "100.00");
        assert_eq!(lines[0].tva, "38.00");
    }

    /// CLI XML fixture — documented-schema-based, pending real-file verification.
    #[test]
    fn parse_cli_xml_basic() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Clienti>
  <Client>
    <Cod>C001</Cod>
    <Denumire>Client SRL</Denumire>
    <Cod_fiscal>RO11111111</Cod_fiscal>
    <Tara>RO</Tara>
    <Localitate>Cluj-Napoca</Localitate>
    <Email>office@client.ro</Email>
    <Guid_cod>guid-c001</Guid_cod>
  </Client>
</Clienti>"#;
        let mut w = vec![];
        let records = parse_cli_xml(xml, &mut w).unwrap();
        assert_eq!(records.len(), 1);
        let r = &records[0];
        assert_eq!(r.cod_fiscal, "RO11111111");
        assert_eq!(r.denumire, "Client SRL");
        assert_eq!(r.guid_cod, "guid-c001");
        assert_eq!(r.tara, "RO");
    }

    /// ART XML fixture — documented-schema-based, pending real-file verification.
    #[test]
    fn parse_art_xml_basic() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Articole>
  <Articol>
    <Cod>ART001</Cod>
    <Denumire>Produs Test</Denumire>
    <UM>buc</UM>
    <ProcTVA>19</ProcTVA>
    <Pret>50.00</Pret>
    <Serviciu>N</Serviciu>
    <Guid_cod>guid-art001</Guid_cod>
  </Articol>
</Articole>"#;
        let mut w = vec![];
        let records = parse_art_xml(xml, &mut w).unwrap();
        assert_eq!(records.len(), 1);
        let r = &records[0];
        assert_eq!(r.cod, "ART001");
        assert_eq!(r.um, "buc");
        assert_eq!(r.serviciu, "N");
        assert_eq!(r.guid_cod, "guid-art001");
    }

    #[test]
    fn build_staged_invoice_amounts_and_direction() {
        let antet = Antet {
            furnizor_cif: "RO12345678".into(),
            furnizor_nume: "Test SRL".into(),
            client_cif: "RO87654321".into(),
            client_nume: "Client SA".into(),
            client_tara: "RO".into(),
            factura_numar: "1".into(),
            factura_data: "01.06.2026".into(),
            factura_moneda: "RON".into(),
            guid_factura: "abc-123".into(),
            ..Default::default()
        };
        let line = Linie {
            descriere: "Servicii".into(),
            cantitate: "2".into(),
            pret: "100.00".into(),
            valoare: "200.00".into(),
            proc_tva: "19".into(),
            tva: "38.00".into(),
            ..Default::default()
        };
        let mut w = vec![];
        // Company CUI = "12345678" → FurnizorCIF (RO12345678) → ISSUED
        let inv = build_staged_invoice(&antet, &[line], "SMARTBILL_XML", "12345678", &mut w);
        assert_eq!(inv.direction, "ISSUED");
        assert_eq!(inv.external_id, Some("abc-123".into()));
        // Net: 200.00, VAT: 38.00, Total: 238.00
        assert_eq!(inv.subtotal_amount.as_deref(), Some("200.00"));
        assert_eq!(inv.vat_amount.as_deref(), Some("38.00"));
        assert_eq!(inv.total_amount.as_deref(), Some("238.00"));
        assert_eq!(inv.lines.len(), 1);
        assert!(w.is_empty(), "no warnings expected: {w:?}");
    }

    #[test]
    fn build_staged_invoice_warns_on_amount_divergence() {
        // Computed net = 2*100 = 200 and VAT = 200*19% = 38, but the source claims 250 / 50 → both
        // diverge by > 0.01. The reconcile guard must WARN (never Err) so a human reviews the line.
        let antet = Antet {
            furnizor_cif: "RO12345678".into(),
            client_cif: "RO87654321".into(),
            factura_numar: "2".into(),
            ..Default::default()
        };
        let line = Linie {
            descriere: "Servicii".into(),
            cantitate: "2".into(),
            pret: "100.00".into(),
            valoare: "250.00".into(),
            proc_tva: "19".into(),
            tva: "50.00".into(),
            ..Default::default()
        };
        let mut w = vec![];
        let inv = build_staged_invoice(&antet, &[line], "SAGA_XML", "12345678", &mut w);
        assert_eq!(inv.direction, "ISSUED");
        assert!(
            !w.is_empty(),
            "divergent source amounts (250 vs computed 200) must emit a reconcile warning"
        );
    }

    #[test]
    fn build_staged_invoice_received_direction() {
        let antet = Antet {
            furnizor_cif: "RO99999999".into(),
            furnizor_nume: "Furnizor SRL".into(),
            client_cif: "RO12345678".into(),
            client_nume: "My Company".into(),
            factura_numar: "10".into(),
            factura_data: "01.06.2026".into(),
            ..Default::default()
        };
        let mut w = vec![];
        let inv = build_staged_invoice(&antet, &[], "SAGA_XML", "12345678", &mut w);
        assert_eq!(inv.direction, "RECEIVED");
        // Partner should be the supplier
        assert_eq!(inv.partner_name.as_deref(), Some("Furnizor SRL"));
    }
}
