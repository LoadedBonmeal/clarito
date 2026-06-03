//! Official ANAF declaration generators (D300, D394, SAF-T D406) + the
//! per-period schema-version layer and the DUKIntegrator validation harness.
//!
//! Every generator here emits schema-conformant XML via the `quick_xml::Writer`
//! pattern (see `ubl/generator.rs`), NOT the legacy hand-rolled string builders
//! in `commands/{declarations,d394,saft}.rs`.

pub mod d300;
pub mod d394;
pub mod saft;
pub mod validation;
pub mod version;
pub mod xml;

/// The three official declarations this module targets. `as_duk_type` returns
/// the token DUKIntegrator's `-v` CLI expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclKind {
    D300,
    D394,
    D406,
}

impl DeclKind {
    pub fn as_duk_type(self) -> &'static str {
        match self {
            DeclKind::D300 => "D300",
            DeclKind::D394 => "D394",
            DeclKind::D406 => "D406",
        }
    }
}
