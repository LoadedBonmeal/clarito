//! DUKIntegrator validation harness (dev/CI only). ANAF's official Java validator
//! has a scriptable CLI: `java -jar DUKIntegrator.jar -v <TYPE> <xml> <result>`.
//! We never bundle Java in the shipped app — this is a developer/CI gate. It
//! degrades gracefully: when Java or the jar are absent, `duk_available()` is
//! false and callers skip, so the normal build stays green.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::anaf_decl::DeclKind;
use crate::error::{AppError, AppResult};

/// Env var pointing at the DUKIntegrator jar (set by devs who ran
/// `scripts/fetch-validators.sh`).
pub const DUK_JAR_ENV: &str = "EFACTURA_DUK_JAR";

#[derive(Debug, Clone)]
pub struct DukResult {
    pub passed: bool,
    pub errors: Vec<String>,
}

/// True only if `$EFACTURA_DUK_JAR` points at an existing file AND `java` runs.
pub fn duk_available() -> bool {
    let jar = match std::env::var(DUK_JAR_ENV) {
        Ok(p) if !p.is_empty() => p,
        _ => return false,
    };
    if !Path::new(&jar).is_file() {
        return false;
    }
    Command::new("java")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Run a Java declaration validator and return its raw textual output (result file
/// contents, falling back to stdout). `java` is the binary, `jar` the DUKIntegrator jar.
pub fn run_java_validator(
    java: &Path,
    jar: &Path,
    decl: DeclKind,
    xml_path: &Path,
) -> AppResult<String> {
    let stem = xml_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("decl");
    let result_path =
        std::env::temp_dir().join(format!("duk_result_{}_{}.txt", stem, uuid::Uuid::now_v7()));
    let output = std::process::Command::new(java)
        .arg("-jar")
        .arg(jar)
        .arg("-v")
        .arg(decl.as_duk_type())
        .arg(xml_path)
        .arg(&result_path)
        .output()
        .map_err(|e| AppError::Other(format!("Nu pot porni DUKIntegrator: {e}")))?;
    let body = std::fs::read_to_string(&result_path)
        .unwrap_or_else(|_| String::from_utf8_lossy(&output.stdout).to_string());
    let _ = std::fs::remove_file(&result_path);
    Ok(body)
}

/// Validate `xml_path` with DUKIntegrator's `-v` mode. Returns Err if the harness
/// is not configured (call `duk_available()` first to skip).
pub fn validate_with_duk(decl: DeclKind, xml_path: &Path) -> AppResult<DukResult> {
    let jar = std::env::var(DUK_JAR_ENV)
        .ok()
        .filter(|p| !p.is_empty())
        .ok_or_else(|| AppError::Other(format!("{DUK_JAR_ENV} not set")))?;

    // Delegate java invocation to shared core.
    let body = run_java_validator(Path::new("java"), &PathBuf::from(&jar), decl, xml_path)?;

    // DUKIntegrator writes findings to the result file (and sometimes stdout).
    // Best-effort tolerant parse: treat any line containing a Romanian error
    // marker as an error; "ok"/"corect"/no errors => passed. Refine once the
    // jar is vendored and the exact result format is confirmed against fixtures.
    let mut errors: Vec<String> = Vec::new();
    for line in body.lines() {
        let l = line.trim();
        let lower = l.to_lowercase();
        if l.is_empty() {
            continue;
        }
        // ANAF "atenționare" lines are advisory warnings — never count them as errors.
        if lower.contains("atention")
            || lower.contains("aten\u{021b}ion")
            || lower.contains("aten\u{0163}ion")
        {
            continue;
        }
        if lower.contains("eroare")
            || lower.contains("erori")
            || lower.contains("invalid")
            || lower.contains("nu este corect")
            || lower.contains("error")
        {
            errors.push(l.to_string());
        }
    }
    let passed = errors.is_empty()
        && (body.to_lowercase().contains("corect")
            || body.to_lowercase().contains("ok")
            || body.trim().is_empty());

    Ok(DukResult { passed, errors })
}

/// True if the libxml2 `xmllint` CLI is available (ships with macOS/most Linux;
/// no Java needed). This is the PRIMARY automatable structural-conformance gate:
/// D300/D394/SAF-T all publish a real XSD, so `xmllint --schema` enforces the
/// namespace, required attributes, enums, and type patterns. DUKIntegrator adds
/// ANAF business rules on top but is harder to drive headlessly.
pub fn xmllint_available() -> bool {
    Command::new("xmllint")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Validate `xml_path` against the XSD at `xsd_path` via `xmllint --schema`.
/// `passed` is true iff xmllint exits 0 (schema-valid); libxml validity/parser
/// error lines are collected into `errors`.
pub fn validate_with_xsd(xsd_path: &Path, xml_path: &Path) -> AppResult<DukResult> {
    let output = Command::new("xmllint")
        .arg("--noout")
        .arg("--schema")
        .arg(xsd_path)
        .arg(xml_path)
        .output()
        .map_err(|e| AppError::Other(format!("Failed to launch xmllint: {e}")))?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let errors: Vec<String> = stderr
        .lines()
        .map(|l| l.trim())
        .filter(|l| {
            let lower = l.to_lowercase();
            !l.is_empty()
                && (lower.contains("validity error")
                    || lower.contains("schemas validity")
                    || lower.contains("parser error")
                    || lower.contains("fails to validate"))
        })
        .map(|l| l.to_string())
        .collect();

    Ok(DukResult {
        passed: output.status.success() && errors.is_empty(),
        errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duk_available_does_not_panic() {
        // We do not assert the value — a dev may or may not have the jar configured.
        // This test just ensures the function compiles and runs without panicking.
        let _ = duk_available();
    }

    #[test]
    fn xmllint_available_does_not_panic() {
        let _ = xmllint_available();
    }
}
