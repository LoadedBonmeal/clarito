//! Embedded font bytes shared between PDF generators.
//!
//! `include_bytes!` embeds each font file exactly once at compile time.
//! Both `ubl::pdf` and `commands::receipts` reference these statics so the
//! linker deduplicates the bytes in the final binary (~825 KB saved).

/// Liberation Sans Regular — supports Romanian diacritics (ș, ț, ă, â, î).
pub static FONT_REGULAR_BYTES: &[u8] =
    include_bytes!("../../resources/fonts/LiberationSans-Regular.ttf");

/// Liberation Sans Bold — used for headings and totals.
pub static FONT_BOLD_BYTES: &[u8] = include_bytes!("../../resources/fonts/LiberationSans-Bold.ttf");
