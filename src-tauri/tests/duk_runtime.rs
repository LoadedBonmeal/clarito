//! Skips unless EFACTURA_DUK_JAR (+ java) are configured (dev/CI with /tmp/dukrun).
use efactura_desktop_lib::anaf_decl::duk::{run_duk, EnvProvider};
use efactura_desktop_lib::anaf_decl::DeclKind;
use std::path::Path;

#[test]
fn duk_validates_a_known_good_d300() {
    if std::env::var("EFACTURA_DUK_JAR")
        .map(|v| v.is_empty())
        .unwrap_or(true)
    {
        eprintln!("SKIP duk_runtime: EFACTURA_DUK_JAR not set");
        return;
    }
    let xml = std::env::var("DUK_TEST_D300_XML").unwrap_or_default();
    if xml.is_empty() || !Path::new(&xml).exists() {
        eprintln!("SKIP: set DUK_TEST_D300_XML to a generated valid D300 path");
        return;
    }
    let out = run_duk(&EnvProvider, DeclKind::D300, Path::new(&xml)).expect("run");
    let out = out.expect("runtime available");
    assert!(out.passed, "expected DUK clean, got {:?}", out.errors);
}
