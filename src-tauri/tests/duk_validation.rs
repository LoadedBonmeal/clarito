//! Integration tests for declaration XML conformance via DUKIntegrator.
//! Skips gracefully when `EFACTURA_DUK_JAR` / Java are absent, so `cargo test`
//! is green everywhere; the gate only bites on machines/CI that vendored the jar.

use efactura_desktop_lib::anaf_decl::validation::duk_available;

#[test]
fn duk_harness_available_or_skips() {
    if !duk_available() {
        eprintln!("SKIP: DUKIntegrator not configured (set EFACTURA_DUK_JAR after running scripts/fetch-validators.sh)");
        return;
    }
    // Generators land in later phases; once they do, this test will generate a
    // golden fixture per declaration and assert `validate_with_duk(...).passed`.
    eprintln!(
        "DUKIntegrator harness available — golden-fixture validation added with the generators."
    );
}
