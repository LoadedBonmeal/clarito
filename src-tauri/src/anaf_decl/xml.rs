//! Shared quick-xml writer helpers for the declaration generators. Mirrors the
//! conformant pattern in `ubl/generator.rs` (Writer + BytesText auto-escaping).

use std::io::Cursor;

use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::Writer;
use rust_decimal::Decimal;

use crate::error::{AppError, AppResult};

pub type XmlWriter = Writer<Cursor<Vec<u8>>>;

fn map_err(e: quick_xml::Error) -> AppError {
    AppError::Other(format!("XML write error: {e}"))
}

/// New writer with the `<?xml version="1.0" encoding="UTF-8"?>` declaration written.
pub fn new_writer() -> AppResult<XmlWriter> {
    let mut w = Writer::new(Cursor::new(Vec::new()));
    w.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(map_err)?;
    Ok(w)
}

/// `<name>text</name>` (text is auto-escaped).
pub fn write_text_elem(w: &mut XmlWriter, name: &str, text: &str) -> AppResult<()> {
    w.write_event(Event::Start(BytesStart::new(name)))
        .map_err(map_err)?;
    w.write_event(Event::Text(BytesText::new(text)))
        .map_err(map_err)?;
    w.write_event(Event::End(BytesEnd::new(name)))
        .map_err(map_err)?;
    Ok(())
}

/// `<name>` decimal formatted to `dp` fractional digits `</name>`. COMMERCIAL rounding (half away
/// from zero) — the ANAF/RO money convention; values are usually pre-rounded upstream, this keeps
/// the safety net consistent with them.
pub fn write_decimal_elem(w: &mut XmlWriter, name: &str, val: &Decimal, dp: u32) -> AppResult<()> {
    let s = val
        .round_dp_with_strategy(dp, rust_decimal::RoundingStrategy::MidpointAwayFromZero)
        .to_string();
    write_text_elem(w, name, &s)
}

/// Open `<name>` (caller writes children, then calls `end_elem`).
pub fn start_elem(w: &mut XmlWriter, name: &str) -> AppResult<()> {
    w.write_event(Event::Start(BytesStart::new(name)))
        .map_err(map_err)
}

/// Close `</name>`.
pub fn end_elem(w: &mut XmlWriter, name: &str) -> AppResult<()> {
    w.write_event(Event::End(BytesEnd::new(name)))
        .map_err(map_err)
}

/// Consume the writer and return the UTF-8 string.
pub fn finish(w: XmlWriter) -> AppResult<String> {
    let bytes = w.into_inner().into_inner();
    String::from_utf8(bytes).map_err(|e| AppError::Other(format!("XML utf8 error: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    #[test]
    fn builds_tiny_doc_with_escaping_and_decimal() {
        let mut w = new_writer().expect("new_writer");
        start_elem(&mut w, "root").expect("start root");
        write_text_elem(&mut w, "a", "x&y").expect("write a");
        write_decimal_elem(&mut w, "b", &Decimal::new(1234, 2), 2).expect("write b");
        end_elem(&mut w, "root").expect("end root");
        let xml = finish(w).expect("finish");

        assert!(
            xml.contains("<a>x&amp;y</a>"),
            "expected escaped ampersand in <a>, got: {xml}"
        );
        assert!(
            xml.contains("<b>12.34</b>"),
            "expected decimal 12.34 in <b>, got: {xml}"
        );
    }
}
