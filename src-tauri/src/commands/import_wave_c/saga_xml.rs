//! SAGA C. native XML adapter — parse-only (Wave C W2).
//!
//! SAGA exports XML files in several categories. This adapter handles:
//!
//!   * **FUR_*.xml** — furnizori (suppliers) partners file
//!   * **CLI_*.xml** — clienti (clients) partners file
//!   * **ART_*.xml** — articole (products/articles) file
//!   * **F_*.xml**   — facturi (invoices) file
//!
//! All share the same SAGA XML dialect as SmartBill's "Export pentru Saga"
//! output, so the shared `xml_common` helpers are reused. The primary differences
//! from the SmartBill adapter:
//!
//!   * File dispatch is by **filename prefix** (FUR_/CLI_/ART_/F_) when
//!     `ImportInput::Files` is used, or by content-sniffing for `::Bytes`.
//!   * FUR_*.xml (suppliers) uses the same `<Cod>/<Denumire>/<Cod_fiscal>/…`
//!     schema as CLI_*.xml — the only difference is the filename prefix.
//!   * ART_*.xml article records are parsed into `StagedProduct` via the
//!     `xml_common::articol_to_staged_product` helper.
//!   * Both ISSUED and RECEIVED invoices may appear in the same F_*.xml file;
//!     direction routing is done per-invoice by comparing FurnizorCIF to
//!     `ctx.company_cui_canonical`.
//!
//! ─── ENCODING ────────────────────────────────────────────────────────────────
//! SAGA XML is documented as often ISO-8859-2 or Windows-1250.
//! `xml_common::decode_xml_bytes` honours the `<?xml encoding="…"?>` PI.
//!
//! ─── UNVERIFIED ELEMENT NAMES ────────────────────────────────────────────────
//! See `xml_common` for the full list. SAGA-specific unverified items:
//! * Root element and per-record element for FUR_*.xml: assumed same as CLI_*.xml
//!   (`<Furnizori>/<Furnizor>` or similar depth-2 structure); the xml_common
//!   depth-2 parser accepts any element name, so this is tolerated gracefully.
//! * Root element and per-record element for ART_*.xml: assumed `<Articole>/<Articol>`.
//! * <Serviciu> element in ART_*.xml: inferred from WinMentor analogy; UNVERIFIED.
//! * Whether SAGA native F_*.xml populates <GUID_factura>/<GUID_cod_client>: UNVERIFIED.
//!   If absent, external_id will be None and W4 uses series+number+date as dedup key.

use crate::error::{AppError, AppResult};

use super::adapter::ImportAdapter;
use super::xml_common::{
    articol_to_staged_product, build_staged_invoice, decode_xml_bytes, parse_art_xml,
    parse_cli_xml, parse_invoice_xml, partener_to_staged_contact,
};
use super::{ImportInput, ParseCtx, SourceKind, StagedData};

const SOURCE: &str = "SAGA_XML";

// ─── Adapter ─────────────────────────────────────────────────────────────────

pub struct SagaXmlAdapter;

impl ImportAdapter for SagaXmlAdapter {
    fn source(&self) -> SourceKind {
        SourceKind::SagaXml
    }

    /// Parse SAGA XML export file(s) into `StagedData`. DB-free.
    ///
    /// `ImportInput::Bytes` — single file content; sniffed by content.
    /// `ImportInput::Files` — multiple files; dispatched by filename prefix.
    fn parse(&self, input: &ImportInput, ctx: &ParseCtx) -> AppResult<StagedData> {
        let mut out = StagedData::empty();
        match input {
            ImportInput::Bytes(bytes) => {
                parse_bytes_sniff(bytes, None, ctx, &mut out)?;
            }
            ImportInput::Files(paths) => {
                for path in paths {
                    let raw = std::fs::read(path).map_err(|e| {
                        AppError::Other(format!(
                            "SAGA XML: nu se poate citi {}: {e}",
                            path.display()
                        ))
                    })?;
                    let filename = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_uppercase();
                    parse_bytes_sniff(&raw, Some(&filename), ctx, &mut out)?;
                }
            }
            ImportInput::RestCreds { .. } => {
                return Err(AppError::Other(
                    "SagaXmlAdapter nu suportă ImportInput::RestCreds.".to_string(),
                ));
            }
        }
        Ok(out)
    }
}

// ─── Dispatch ────────────────────────────────────────────────────────────────

/// Detect file kind from optional filename prefix, then from content.
fn parse_bytes_sniff(
    raw: &[u8],
    filename_upper: Option<&str>,
    ctx: &ParseCtx,
    out: &mut StagedData,
) -> AppResult<()> {
    let xml = decode_xml_bytes(raw);

    // Prefer filename-based dispatch when we have the name.
    let kind = if let Some(name) = filename_upper {
        if name.starts_with("F_") {
            FileKind::Invoice
        } else if name.starts_with("CLI_") {
            FileKind::Client
        } else if name.starts_with("FUR_") {
            FileKind::Supplier
        } else if name.starts_with("ART_") {
            FileKind::Article
        } else {
            detect_kind_from_content(&xml)
        }
    } else {
        detect_kind_from_content(&xml)
    };

    match kind {
        FileKind::Invoice => parse_invoice_file(&xml, ctx, out),
        FileKind::Client | FileKind::Supplier => parse_partner_file(&xml, ctx, out),
        FileKind::Article => parse_article_file(&xml, ctx, out),
        FileKind::Unknown => {
            out.warnings.push(
                "SAGA XML: conținut nerecunoscut — fișierul nu conține elemente \
                 cunoscute SAGA. Fișier ignorat."
                    .to_string(),
            );
            Ok(())
        }
    }
}

enum FileKind {
    Invoice,
    Client,
    Supplier,
    Article,
    Unknown,
}

fn detect_kind_from_content(xml: &str) -> FileKind {
    if xml.contains("<Factura") {
        FileKind::Invoice
    } else if xml.contains("<Cod_fiscal") {
        FileKind::Client
    } else if xml.contains("<ProcTVA") || xml.contains("<Serviciu") {
        FileKind::Article
    } else if xml.contains("<Denumire") || xml.contains("<Guid_cod") {
        FileKind::Client
    } else {
        FileKind::Unknown
    }
}

// ─── Per-kind parsers ─────────────────────────────────────────────────────────

fn parse_invoice_file(xml: &str, ctx: &ParseCtx, out: &mut StagedData) -> AppResult<()> {
    let facturi = parse_invoice_xml(xml, &mut out.warnings).map_err(AppError::Other)?;

    for (antet, lines) in &facturi {
        let inv = build_staged_invoice(
            antet,
            lines,
            SOURCE,
            ctx.company_cui_canonical,
            &mut out.warnings,
        );

        // Extract partner from invoice header (direction-aware: partner = OTHER party).
        let (
            partner_cif,
            partner_name,
            partner_adresa,
            partner_localitate,
            partner_judet,
            partner_tara,
            partner_reg_com,
        ) = if inv.direction == "ISSUED" {
            (
                antet.client_cif.clone(),
                antet.client_nume.clone(),
                antet.client_adresa.clone(),
                antet.client_localitate.clone(),
                antet.client_judet.clone(),
                antet.client_tara.clone(),
                antet.client_nr_reg_com.clone(),
            )
        } else {
            (
                antet.furnizor_cif.clone(),
                antet.furnizor_nume.clone(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                antet.furnizor_nr_reg_com.clone(),
            )
        };

        if !partner_cif.is_empty() || !partner_name.is_empty() {
            use super::xml_common::PartenerRecord;
            let rec = PartenerRecord {
                cod: String::new(),
                denumire: partner_name,
                cod_fiscal: partner_cif,
                reg_com: partner_reg_com,
                tara: partner_tara,
                judet: partner_judet,
                localitate: partner_localitate,
                adresa: partner_adresa,
                cont_banca: String::new(),
                banca: String::new(),
                tel: String::new(),
                email: String::new(),
                guid_cod: antet.guid_cod_client.clone(),
            };
            let contact = partener_to_staged_contact(&rec, SOURCE);
            let existing = out.contacts.iter().any(|c| {
                c.cui_canonical.is_some()
                    && c.cui_canonical == contact.cui_canonical
                    && contact.cui_canonical.is_some()
            });
            if !existing {
                out.contacts.push(contact);
            }
        }

        out.invoices.push(inv);
    }

    Ok(())
}

fn parse_partner_file(xml: &str, _ctx: &ParseCtx, out: &mut StagedData) -> AppResult<()> {
    let records = parse_cli_xml(xml, &mut out.warnings).map_err(AppError::Other)?;
    for rec in &records {
        let contact = partener_to_staged_contact(rec, SOURCE);
        let existing = out.contacts.iter().any(|c| {
            c.cui_canonical.is_some()
                && c.cui_canonical == contact.cui_canonical
                && contact.cui_canonical.is_some()
        });
        if !existing {
            out.contacts.push(contact);
        }
    }
    Ok(())
}

fn parse_article_file(xml: &str, _ctx: &ParseCtx, out: &mut StagedData) -> AppResult<()> {
    let records = parse_art_xml(xml, &mut out.warnings).map_err(AppError::Other)?;
    for rec in &records {
        let product = articol_to_staged_product(rec, SOURCE);
        // Dedup by source_code (GUID or Cod)
        let existing = out
            .products
            .iter()
            .any(|p| p.source_code.is_some() && p.source_code == product.source_code);
        if !existing {
            out.products.push(product);
        }
    }
    Ok(())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::import_wave_c::{ImportInput, ParseCtx};

    fn ctx<'a>(company_cui: &'a str) -> ParseCtx<'a> {
        ParseCtx {
            company_cui_canonical: company_cui,
            column_map: None,
        }
    }

    /// Documented-schema-based fixture: one ISSUED + one RECEIVED invoice.
    /// Pending real-file verification against a live SAGA export.
    const INVOICES_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Facturi>
  <!-- ISSUED: FurnizorCIF == company CUI -->
  <Factura>
    <Antet>
      <FurnizorCIF>RO12345678</FurnizorCIF>
      <FurnizorNume>Firma Noastra SRL</FurnizorNume>
      <ClientCIF>RO87654321</ClientCIF>
      <ClientNume>Client SA</ClientNume>
      <ClientTara>RO</ClientTara>
      <FacturaNumar>2001</FacturaNumar>
      <FacturaData>10.06.2026</FacturaData>
      <FacturaMoneda>RON</FacturaMoneda>
      <GUID_factura>guid-iesire-001</GUID_factura>
    </Antet>
    <Linie>
      <Descriere>Marfa A</Descriere>
      <Cantitate>5</Cantitate>
      <Pret>200.00</Pret>
      <Valoare>1000.00</Valoare>
      <ProcTVA>19</ProcTVA>
      <TVA>190.00</TVA>
    </Linie>
  </Factura>
  <!-- RECEIVED: FurnizorCIF != company CUI -->
  <Factura>
    <Antet>
      <FurnizorCIF>RO55555555</FurnizorCIF>
      <FurnizorNume>Furnizor Extern SRL</FurnizorNume>
      <ClientCIF>RO12345678</ClientCIF>
      <ClientNume>Firma Noastra SRL</ClientNume>
      <FacturaNumar>3001</FacturaNumar>
      <FacturaData>11.06.2026</FacturaData>
      <FacturaMoneda>RON</FacturaMoneda>
    </Antet>
    <Linie>
      <Descriere>Servicii externe</Descriere>
      <Cantitate>1</Cantitate>
      <Pret>500.00</Pret>
      <Valoare>500.00</Valoare>
      <ProcTVA>19</ProcTVA>
      <TVA>95.00</TVA>
    </Linie>
  </Factura>
</Facturi>"#;

    const ART_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Articole>
  <Articol>
    <Cod>ART001</Cod>
    <Denumire>Produs Stoc</Denumire>
    <UM>buc</UM>
    <ProcTVA>19</ProcTVA>
    <Pret>100.00</Pret>
    <Serviciu>N</Serviciu>
    <Guid_cod>guid-art-001</Guid_cod>
  </Articol>
  <Articol>
    <Cod>SRV001</Cod>
    <Denumire>Serviciu Consulting</Denumire>
    <UM>ora</UM>
    <ProcTVA>19</ProcTVA>
    <Pret>200.00</Pret>
    <Serviciu>D</Serviciu>
  </Articol>
</Articole>"#;

    const CLI_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Clienti>
  <Client>
    <Cod>CLI001</Cod>
    <Denumire>Client Curent SRL</Denumire>
    <Cod_fiscal>RO33333333</Cod_fiscal>
    <Tara>RO</Tara>
    <Localitate>Timisoara</Localitate>
    <Guid_cod>guid-cli-001</Guid_cod>
  </Client>
</Clienti>"#;

    const FUR_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Furnizori>
  <Furnizor>
    <Cod>FUR001</Cod>
    <Denumire>Furnizor Principal SRL</Denumire>
    <Cod_fiscal>RO44444444</Cod_fiscal>
    <Tara>RO</Tara>
    <Localitate>Brasov</Localitate>
    <Guid_cod>guid-fur-001</Guid_cod>
  </Furnizor>
</Furnizori>"#;

    #[test]
    fn saga_direction_routing_issued_and_received() {
        let adapter = SagaXmlAdapter;
        let input = ImportInput::Bytes(INVOICES_XML.as_bytes().to_vec());
        let c = ctx("12345678"); // company CUI
        let data = adapter.parse(&input, &c).unwrap();

        assert_eq!(data.invoices.len(), 2, "two invoices expected");

        // First invoice: FurnizorCIF == company → ISSUED
        let issued = data
            .invoices
            .iter()
            .find(|i| i.direction == "ISSUED")
            .expect("should have an ISSUED invoice");
        assert_eq!(issued.direction, "ISSUED");
        assert_eq!(issued.external_id.as_deref(), Some("guid-iesire-001"));
        // Partner on ISSUED = client
        assert_eq!(issued.partner_cui_canonical.as_deref(), Some("87654321"));

        // Second invoice: FurnizorCIF != company → RECEIVED
        let received = data
            .invoices
            .iter()
            .find(|i| i.direction == "RECEIVED")
            .expect("should have a RECEIVED invoice");
        assert_eq!(received.direction, "RECEIVED");
        // Partner on RECEIVED = supplier
        assert_eq!(received.partner_cui_canonical.as_deref(), Some("55555555"));
        // No GUID → external_id None
        assert!(received.external_id.is_none());
    }

    #[test]
    fn saga_art_xml_product_extraction() {
        let adapter = SagaXmlAdapter;
        let input = ImportInput::Bytes(ART_XML.as_bytes().to_vec());
        let c = ctx("12345678");
        let data = adapter.parse(&input, &c).unwrap();

        assert_eq!(data.products.len(), 2, "two products expected");

        let p1 = data
            .products
            .iter()
            .find(|p| p.code.as_deref() == Some("ART001"))
            .expect("ART001 product");
        assert_eq!(p1.name.as_deref(), Some("Produs Stoc"));
        assert_eq!(p1.unit.as_deref(), Some("buc"));
        assert_eq!(p1.vat_rate.as_deref(), Some("19"));
        assert_eq!(p1.is_service, Some(false));
        // GUID → source_code
        assert_eq!(p1.source_code.as_deref(), Some("guid-art-001"));

        let p2 = data
            .products
            .iter()
            .find(|p| p.code.as_deref() == Some("SRV001"))
            .expect("SRV001 product");
        assert_eq!(p2.is_service, Some(true));
        // No GUID → source_code falls back to Cod
        assert_eq!(p2.source_code.as_deref(), Some("SRV001"));
    }

    #[test]
    fn saga_missing_guid_external_id_none() {
        // The RECEIVED invoice in INVOICES_XML has no GUID_factura → external_id = None
        let adapter = SagaXmlAdapter;
        let input = ImportInput::Bytes(INVOICES_XML.as_bytes().to_vec());
        let c = ctx("12345678");
        let data = adapter.parse(&input, &c).unwrap();

        let received = data
            .invoices
            .iter()
            .find(|i| i.direction == "RECEIVED")
            .unwrap();
        assert!(
            received.external_id.is_none(),
            "no GUID_factura → external_id must be None (W4 uses series+number+date)"
        );
    }

    #[test]
    fn saga_blank_cont_tolerated_no_error() {
        // <Cont> blank on a line must produce account_code = None, no error, only
        // possibly a warning (tolerated per spec).
        // Using the INVOICES_XML fixture which has no <Cont> at all (absent = blank).
        let adapter = SagaXmlAdapter;
        let input = ImportInput::Bytes(INVOICES_XML.as_bytes().to_vec());
        let c = ctx("12345678");
        let data = adapter.parse(&input, &c).unwrap();
        for inv in &data.invoices {
            for line in &inv.lines {
                assert!(
                    line.account_code.is_none(),
                    "absent <Cont> must map to None, not error"
                );
            }
        }
    }

    #[test]
    fn saga_files_dispatch_by_prefix() {
        use std::io::Write as _;
        let adapter = SagaXmlAdapter;

        // Write fixtures to temp files with correct prefixes
        let fur_path = {
            let mut f = tempfile::Builder::new()
                .prefix("FUR_")
                .suffix(".xml")
                .tempfile()
                .unwrap();
            f.write_all(FUR_XML.as_bytes()).unwrap();
            f.into_temp_path()
        };
        let cli_path = {
            let mut f = tempfile::Builder::new()
                .prefix("CLI_")
                .suffix(".xml")
                .tempfile()
                .unwrap();
            f.write_all(CLI_XML.as_bytes()).unwrap();
            f.into_temp_path()
        };
        let art_path = {
            let mut f = tempfile::Builder::new()
                .prefix("ART_")
                .suffix(".xml")
                .tempfile()
                .unwrap();
            f.write_all(ART_XML.as_bytes()).unwrap();
            f.into_temp_path()
        };
        let inv_path = {
            let mut f = tempfile::Builder::new()
                .prefix("F_")
                .suffix(".xml")
                .tempfile()
                .unwrap();
            f.write_all(INVOICES_XML.as_bytes()).unwrap();
            f.into_temp_path()
        };

        let input = ImportInput::Files(vec![
            fur_path.to_path_buf(),
            cli_path.to_path_buf(),
            art_path.to_path_buf(),
            inv_path.to_path_buf(),
        ]);

        let c = ctx("12345678");
        let data = adapter.parse(&input, &c).unwrap();

        // 2 invoices from F_*.xml
        assert_eq!(data.invoices.len(), 2, "two invoices from F_*.xml");
        // 2 products from ART_*.xml
        assert_eq!(data.products.len(), 2, "two products from ART_*.xml");
        // Contacts: FUR_*(1 furnizor) + CLI_*(1 client) + invoice headers (2 partners)
        // Deduped by CUI:
        //   - RO33333333 from CLI
        //   - RO44444444 from FUR
        //   - RO87654321 from invoice ISSUED header (Client SA)
        //   - RO55555555 from invoice RECEIVED header (Furnizor Extern)
        // = 4 unique contacts
        assert_eq!(
            data.contacts.len(),
            4,
            "4 unique contacts; got {:?}",
            data.contacts
                .iter()
                .map(|c| c.cui_canonical.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn saga_parse_returns_pure_staged_data_no_db() {
        // Confirms parse() is DB-free: StagedData is purely in-memory,
        // with new_id() UUIDs on each record.
        let adapter = SagaXmlAdapter;
        let input = ImportInput::Bytes(INVOICES_XML.as_bytes().to_vec());
        let c = ctx("12345678");
        let data = adapter.parse(&input, &c).unwrap();
        assert!(!data.invoices.is_empty());
        assert!(!data.invoices[0].id.is_empty());
        assert!(!data.invoices[0].lines[0].id.is_empty());
        // StagedData has no accounts for raw invoice XML (no account mapping)
        assert!(data.accounts.is_empty());
    }
}
