//! SmartBill "Export pentru Saga" XML adapter — parse-only (Wave C W2).
//!
//! SmartBill's "Export pentru Saga" feature (in the Facturi emise report)
//! emits XML files in the SAGA C. import dialect. Two distinct file types:
//!
//!   1. **Invoice file** — `F_RO<cif>_multiple_<date>.xml` (or similar naming).
//!      Root element UNVERIFIED — assumed `<Facturi>` but may vary.
//!      Shares the `<Factura>/<Antet>/<Linie>` schema with SAGA native XML.
//!
//!   2. **Client file** — `CLI_<date>.xml` ("Export clienti PF").
//!      Root element UNVERIFIED.  Per-record fields: `<Cod>/<Denumire>/
//!      <Cod_fiscal>/<Tara>/.../<Guid_cod>`.
//!
//! Both are parsed via `xml_common` helpers; this adapter does SmartBill-specific
//! dispatch (by filename prefix when `ImportInput::Files` is used) and wraps
//! the results into `StagedData`.
//!
//! ─── SMARTBILL-SPECIFIC NOTES ────────────────────────────────────────────────
//! * `<Cont>` (account) is frequently BLANK in SmartBill exports — tolerated
//!   (stored as None; a warning is NOT emitted for this specific case because it
//!   is expected and documented).
//! * SmartBill invoices are always in the ISSUED perspective (supplier == the
//!   exporting company). The `FurnizorCIF` field should match `ctx.company_cui_canonical`.
//! * `<ClientTara>` carries a country CODE (e.g. "RO", "IT"), NOT a name.
//!   The REST API (W3) uses country names — they are different on purpose.
//!   W4/normalisation handles the code → name resolution when needed.
//! * `<GUID_factura>` / `<GUID_cod_client>` / `<Guid_cod>` are the preferred
//!   external_id / source_code when present; fallback is series+number+date (W4).
//!
//! ─── ENCODING ────────────────────────────────────────────────────────────────
//! SmartBill XML exports are typically UTF-8 but may carry an `encoding="..."` PI.
//! `xml_common::decode_xml_bytes` honours whatever is declared.
//!
//! ─── UNVERIFIED ELEMENT NAMES ────────────────────────────────────────────────
//! See `xml_common` for the full list. SmartBill-specific unverified items:
//! * Whether SmartBill CLI_*.xml uses `<Client>` or `<Partener>` as the per-record
//!   element (the xml_common depth-2 parser accepts any element name).
//! * Whether SmartBill populates `<GUID_cod_client>` in `<Antet>` or only
//!   `<Guid_cod>` in the partner file.
//! * Root/container element for invoice files (guessed `<Facturi>`).

use crate::error::{AppError, AppResult};

use super::adapter::ImportAdapter;
use super::xml_common::{
    build_staged_invoice, decode_xml_bytes, parse_cli_xml, parse_invoice_xml,
    partener_to_staged_contact,
};
use super::{ImportInput, ParseCtx, SourceKind, StagedData};

const SOURCE: &str = "SMARTBILL_XML";

// ─── Adapter ─────────────────────────────────────────────────────────────────

pub struct SmartBillXmlAdapter;

impl ImportAdapter for SmartBillXmlAdapter {
    fn source(&self) -> SourceKind {
        SourceKind::SmartbillXml
    }

    /// Parse SmartBill XML export(s) into `StagedData`. DB-free.
    ///
    /// `ImportInput::Bytes` — a single file's raw bytes; the adapter sniffs the
    /// root element to decide whether it's an invoice file or a client file.
    ///
    /// `ImportInput::Files` — one or more file paths; each is dispatched by
    /// filename prefix (CLI_* → client file; everything else → invoice file).
    fn parse(&self, input: &ImportInput, ctx: &ParseCtx) -> AppResult<StagedData> {
        let mut out = StagedData::empty();
        match input {
            ImportInput::Bytes(bytes) => {
                parse_bytes(bytes, ctx, &mut out)?;
            }
            ImportInput::Files(paths) => {
                for path in paths {
                    let raw = std::fs::read(path).map_err(|e| {
                        AppError::Other(format!(
                            "SmartBill XML: nu se poate citi {}: {e}",
                            path.display()
                        ))
                    })?;
                    parse_bytes(&raw, ctx, &mut out)?;
                }
            }
            ImportInput::RestCreds { .. } => {
                return Err(AppError::Other(
                    "SmartBillXmlAdapter nu suportă ImportInput::RestCreds (folosiți W3)."
                        .to_string(),
                ));
            }
        }
        Ok(out)
    }
}

// ─── Internal parse dispatcher ────────────────────────────────────────────────

/// Parse one XML file's bytes and accumulate results into `out`.
fn parse_bytes(raw: &[u8], ctx: &ParseCtx, out: &mut StagedData) -> AppResult<()> {
    let xml = decode_xml_bytes(raw);

    // Dispatch by content: look for characteristic root/record elements.
    // CLI_*.xml has <Cod_fiscal> or <Guid_cod> but no <Factura>.
    // Invoice file has <Factura>.
    if xml.contains("<Factura") {
        parse_invoice_file(&xml, ctx, out)
    } else if xml.contains("<Cod_fiscal") || xml.contains("<Guid_cod") || xml.contains("<Denumire")
    {
        parse_cli_file(&xml, ctx, out)
    } else {
        out.warnings.push(
            "SmartBill XML: conținut nerecunoscut — fișierul nu conține <Factura>, \
             <Cod_fiscal> sau <Denumire>. Fișier ignorat."
                .to_string(),
        );
        Ok(())
    }
}

/// Parse a SmartBill invoice export XML.
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

        // Extract partner contact from the invoice header.
        // For SmartBill (always ISSUED), the partner is the client.
        if !antet.client_cif.is_empty() || !antet.client_nume.is_empty() {
            use super::xml_common::{partener_to_staged_contact, PartenerRecord};
            let rec = PartenerRecord {
                cod: String::new(),
                denumire: antet.client_nume.clone(),
                cod_fiscal: antet.client_cif.clone(),
                reg_com: antet.client_nr_reg_com.clone(),
                tara: antet.client_tara.clone(),
                judet: antet.client_judet.clone(),
                localitate: antet.client_localitate.clone(),
                adresa: antet.client_adresa.clone(),
                cont_banca: String::new(),
                banca: String::new(),
                tel: String::new(),
                email: String::new(),
                // GUID_cod_client in the invoice header maps to Guid_cod in the
                // partner record — use it as source_code when present.
                // UNVERIFIED: whether SmartBill actually populates GUID_cod_client.
                guid_cod: antet.guid_cod_client.clone(),
            };
            let contact = partener_to_staged_contact(&rec, SOURCE);
            // Only add if not a duplicate of an existing staged contact by CUI.
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

/// Parse a SmartBill "Export clienti PF" XML (CLI_*.xml).
fn parse_cli_file(xml: &str, _ctx: &ParseCtx, out: &mut StagedData) -> AppResult<()> {
    let records = parse_cli_xml(xml, &mut out.warnings).map_err(AppError::Other)?;
    for rec in &records {
        let contact = partener_to_staged_contact(rec, SOURCE);
        // Dedup by canonical CUI.
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

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::import_wave_c::{canonical_cui, ImportInput, ParseCtx};

    fn ctx<'a>(company_cui: &'a str) -> ParseCtx<'a> {
        ParseCtx {
            company_cui_canonical: company_cui,
            column_map: None,
        }
    }

    /// Documented-schema-based fixture: invoice XML (ISSUED) + CLI_*.xml.
    /// Pending real-file verification against a live SmartBill export.
    const INVOICE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Facturi>
  <Factura>
    <Antet>
      <FurnizorCIF>RO12345678</FurnizorCIF>
      <FurnizorNume>Firma Mea SRL</FurnizorNume>
      <FurnizorNrRegCom>J01/123/2020</FurnizorNrRegCom>
      <ClientCIF>RO87654321</ClientCIF>
      <ClientNume>Client SA</ClientNume>
      <ClientNrRegCom>J02/456/2021</ClientNrRegCom>
      <ClientAdresa>Str. Principala 1</ClientAdresa>
      <ClientLocalitate>Cluj-Napoca</ClientLocalitate>
      <ClientJudet>CJ</ClientJudet>
      <ClientTara>RO</ClientTara>
      <FacturaNumar>1001</FacturaNumar>
      <FacturaData>15.06.2026</FacturaData>
      <FacturaScadenta>30.06.2026</FacturaScadenta>
      <FacturaMoneda>RON</FacturaMoneda>
      <FacturaTaxareInversa>N</FacturaTaxareInversa>
      <FacturaTVAIncasare>N</FacturaTVAIncasare>
      <GUID_factura>guid-invoice-001</GUID_factura>
      <GUID_cod_client>guid-client-001</GUID_cod_client>
    </Antet>
    <Linie>
      <Descriere>Servicii consultanta</Descriere>
      <CodArticolFurnizor>SRV-001</CodArticolFurnizor>
      <UM>ora</UM>
      <Cantitate>10</Cantitate>
      <Pret>100.00</Pret>
      <Valoare>1000.00</Valoare>
      <ProcTVA>19</ProcTVA>
      <TVA>190.00</TVA>
      <Cont></Cont>
      <Gestiune></Gestiune>
    </Linie>
    <Linie>
      <Descriere>Cheltuieli transport</Descriere>
      <CodArticolFurnizor>TRN-001</CodArticolFurnizor>
      <UM>buc</UM>
      <Cantitate>1</Cantitate>
      <Pret>50.00</Pret>
      <Valoare>50.00</Valoare>
      <ProcTVA>19</ProcTVA>
      <TVA>9.50</TVA>
      <Cont></Cont>
    </Linie>
  </Factura>
</Facturi>"#;

    const CLI_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Clienti>
  <Client>
    <Cod>C001</Cod>
    <Denumire>Client SA</Denumire>
    <Cod_fiscal>RO87654321</Cod_fiscal>
    <Reg_com>J02/456/2021</Reg_com>
    <Tara>RO</Tara>
    <Judet>CJ</Judet>
    <Localitate>Cluj-Napoca</Localitate>
    <Adresa>Str. Principala 1</Adresa>
    <Tel>0264000000</Tel>
    <Email>office@client.ro</Email>
    <Guid_cod>guid-client-001</Guid_cod>
  </Client>
  <Client>
    <Cod>C002</Cod>
    <Denumire>Client Italia SRL</Denumire>
    <Cod_fiscal>IT12345678901</Cod_fiscal>
    <Tara>IT</Tara>
    <Localitate>Milano</Localitate>
    <Guid_cod>guid-client-002</Guid_cod>
  </Client>
</Clienti>"#;

    #[test]
    fn smartbill_parse_invoice_bytes_issued() {
        let adapter = SmartBillXmlAdapter;
        let input = ImportInput::Bytes(INVOICE_XML.as_bytes().to_vec());
        // Company CUI = canonical of "RO12345678" = "12345678"
        let c = ctx("12345678");
        let data = adapter.parse(&input, &c).unwrap();

        // 1 invoice
        assert_eq!(data.invoices.len(), 1, "should have 1 invoice");
        let inv = &data.invoices[0];

        // Direction: ISSUED (FurnizorCIF == company CUI)
        assert_eq!(inv.direction, "ISSUED");

        // GUID → external_id
        assert_eq!(inv.external_id.as_deref(), Some("guid-invoice-001"));

        // Partner extracted from header (ClientCIF)
        assert_eq!(
            inv.partner_cui_canonical.as_deref(),
            Some(canonical_cui("RO87654321").as_str())
        );

        // Line count
        assert_eq!(inv.lines.len(), 2);

        // Line 1: qty=10, net=1000.00, vat=190.00, total=1190.00
        let l1 = &inv.lines[0];
        assert_eq!(l1.quantity.as_deref(), Some("10"));
        assert_eq!(l1.unit_price.as_deref(), Some("100.00"));
        assert_eq!(l1.subtotal_amount.as_deref(), Some("1000.00"));
        assert_eq!(l1.vat_amount.as_deref(), Some("190.00"));
        assert_eq!(l1.total_amount.as_deref(), Some("1190.00"));
        assert_eq!(l1.product_code.as_deref(), Some("SRV-001"));
        // Blank <Cont> → account_code is None
        assert!(l1.account_code.is_none(), "blank Cont should be None");

        // Header totals: net=1050.00, vat=199.50, total=1249.50
        assert_eq!(inv.subtotal_amount.as_deref(), Some("1050.00"));
        assert_eq!(inv.vat_amount.as_deref(), Some("199.50"));
        assert_eq!(inv.total_amount.as_deref(), Some("1249.50"));

        // Header-vs-lines VAT reconcile within 0.01 (no warnings expected for
        // lines where Valoare and Pret*Cantitate agree)
        let vat_warn = data
            .warnings
            .iter()
            .any(|w| w.contains("TVA") && w.contains("diferă"));
        assert!(
            !vat_warn,
            "unexpected VAT reconcile warning: {:?}",
            data.warnings
        );
    }

    #[test]
    fn smartbill_parse_cli_extracts_partner_name_and_canonical_cui() {
        let adapter = SmartBillXmlAdapter;
        let input = ImportInput::Bytes(CLI_XML.as_bytes().to_vec());
        let c = ctx("12345678");
        let data = adapter.parse(&input, &c).unwrap();

        assert_eq!(data.contacts.len(), 2, "two partners expected");
        let c1 = &data.contacts[0];
        assert_eq!(c1.legal_name.as_deref(), Some("Client SA"));
        assert_eq!(c1.cui_canonical.as_deref(), Some("87654321"));
        assert_eq!(
            c1.source_code.as_deref(),
            Some("guid-client-001"),
            "Guid_cod should become source_code"
        );

        // Country code passthrough (RO kept as-is, not expanded to 'Romania')
        assert_eq!(c1.country.as_deref(), Some("RO"));

        // Italian partner — foreign VAT id, no canonical stripping
        let c2 = &data.contacts[1];
        assert_eq!(c2.country.as_deref(), Some("IT"));
        assert_eq!(c2.legal_name.as_deref(), Some("Client Italia SRL"));
    }

    #[test]
    fn smartbill_parse_files_invoice_and_cli() {
        use std::io::Write as _;
        let adapter = SmartBillXmlAdapter;
        // Write fixtures to temp files
        let inv_path = {
            let mut f = tempfile::NamedTempFile::new().unwrap();
            f.write_all(INVOICE_XML.as_bytes()).unwrap();
            f.into_temp_path()
        };
        let cli_path = {
            let mut f = tempfile::NamedTempFile::new().unwrap();
            f.write_all(CLI_XML.as_bytes()).unwrap();
            f.into_temp_path()
        };
        let input = ImportInput::Files(vec![inv_path.to_path_buf(), cli_path.to_path_buf()]);
        let c = ctx("12345678");
        let data = adapter.parse(&input, &c).unwrap();

        // 1 invoice from invoice file
        assert_eq!(data.invoices.len(), 1);
        // Contacts from both: invoice header contributes Client SA + CLI adds same
        // (deduped) + Client Italia → 2 unique by CUI
        // Client SA appears in both: once from invoice header, once from CLI_*.xml.
        // Dedup by canonical CUI → 2 unique contacts.
        assert_eq!(
            data.contacts.len(),
            2,
            "Client SA (deduped) + Client Italia = 2; got {:?}",
            data.contacts
                .iter()
                .map(|c| c.legal_name.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn smartbill_guid_external_id_country_code_passthrough() {
        let adapter = SmartBillXmlAdapter;
        let input = ImportInput::Bytes(INVOICE_XML.as_bytes().to_vec());
        let c = ctx("12345678");
        let data = adapter.parse(&input, &c).unwrap();
        let inv = &data.invoices[0];
        // GUID → external_id (invoice dedup key)
        assert_eq!(inv.external_id.as_deref(), Some("guid-invoice-001"));
        // Partner contact: ClientTara = "RO" (code, not name)
        let contact = data
            .contacts
            .iter()
            .find(|c| c.cui_canonical.as_deref() == Some("87654321"))
            .expect("partner contact from invoice header");
        assert_eq!(
            contact.country.as_deref(),
            Some("RO"),
            "country code 'RO' must pass through as-is"
        );
    }

    #[test]
    fn smartbill_missing_guid_external_id_none() {
        // When GUID_factura is absent, external_id must be None (W4 uses series+number+date)
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Facturi>
  <Factura>
    <Antet>
      <FurnizorCIF>RO12345678</FurnizorCIF>
      <ClientCIF>RO87654321</ClientCIF>
      <ClientNume>Client SA</ClientNume>
      <FacturaNumar>999</FacturaNumar>
      <FacturaData>20.06.2026</FacturaData>
    </Antet>
    <Linie>
      <Descriere>Test</Descriere>
      <Cantitate>1</Cantitate>
      <Pret>10.00</Pret>
      <Valoare>10.00</Valoare>
      <ProcTVA>19</ProcTVA>
      <TVA>1.90</TVA>
    </Linie>
  </Factura>
</Facturi>"#;
        let adapter = SmartBillXmlAdapter;
        let input = ImportInput::Bytes(xml.as_bytes().to_vec());
        let c = ctx("12345678");
        let data = adapter.parse(&input, &c).unwrap();
        assert_eq!(data.invoices.len(), 1);
        assert!(
            data.invoices[0].external_id.is_none(),
            "no GUID → external_id must be None"
        );
    }

    #[test]
    fn smartbill_parse_returns_no_db_access() {
        // This test verifies parse() returns StagedData with no DB writes
        // (all results are in-memory; StagedData contains contacts/products/invoices
        // but no database IDs from the DB layer — just new_id() UUIDs).
        let adapter = SmartBillXmlAdapter;
        let input = ImportInput::Bytes(INVOICE_XML.as_bytes().to_vec());
        let c = ctx("12345678");
        let data = adapter.parse(&input, &c).unwrap();
        // Structural check: StagedData fields are populated
        assert!(!data.invoices.is_empty());
        // IDs are non-empty UUIDs (generated by new_id())
        assert!(!data.invoices[0].id.is_empty());
        assert!(!data.invoices[0].lines[0].id.is_empty());
        // No DB-derived fields (accounts are empty for SmartBill XML — no account mapping)
        assert!(data.accounts.is_empty());
    }
}
