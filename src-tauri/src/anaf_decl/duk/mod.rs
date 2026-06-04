//! Layer D — real DUKIntegrator validation at runtime, behind a swappable provider.
//! `BundledProvider` resolves the JRE+jars shipped in the app; `EnvProvider` uses
//! `EFACTURA_DUK_JAR` + system `java` (dev/CI). Both produce a `DukRuntime`.

// TODO(remove in P1.T2/T3): used by later tasks
#![allow(dead_code, unused_imports)]

use std::path::{Path, PathBuf};

use crate::anaf_decl::preflight::PreflightIssue;
use crate::anaf_decl::DeclKind;
use crate::error::AppResult;

/// A resolved DUK runtime: the `java` binary + the directory holding DUKIntegrator.jar + lib/.
#[derive(Debug, Clone)]
pub struct DukRuntime {
    pub java: PathBuf,
    pub jar_dir: PathBuf,
}

impl DukRuntime {
    fn duk_jar(&self) -> PathBuf {
        self.jar_dir.join("DUKIntegrator.jar")
    }
}

/// Locates a DUK runtime. Returns `None` when none is available (→ graceful fallback to layer A).
pub trait DukProvider {
    fn resolve(&self) -> Option<DukRuntime>;
}

/// Dev/CI provider: `$EFACTURA_DUK_JAR` points at DUKIntegrator.jar; `java` from PATH.
pub struct EnvProvider;

impl DukProvider for EnvProvider {
    fn resolve(&self) -> Option<DukRuntime> {
        let jar = std::env::var(crate::anaf_decl::validation::DUK_JAR_ENV).ok()?;
        let jar_path = PathBuf::from(&jar);
        if !jar_path.is_file() {
            return None;
        }
        let jar_dir = jar_path.parent()?.to_path_buf();
        let java = std::env::var("EFACTURA_DUK_JAVA")
            .ok()
            .map(PathBuf::from)
            .filter(|p| p.is_file())
            .unwrap_or_else(|| PathBuf::from("java"));
        Some(DukRuntime { java, jar_dir })
    }
}

/// Result of a DUK run, in the same `PreflightIssue` vocabulary as layer A.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DukOutcome {
    /// True only if DUK was available, ran, AND reported no errors.
    pub passed: bool,
    pub errors: Vec<PreflightIssue>,
}

/// Parse DUKIntegrator's textual output (result file or stdout) into issues.
/// Clean marker: output contains "fara erori"/"fără erori". Any line with a DUK
/// error marker becomes an `error` `PreflightIssue` with code "DUK".
pub fn parse_duk_output(raw: &str) -> DukOutcome {
    let mut errors = Vec::new();
    for line in raw.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        let low = l.to_lowercase();
        let looks_error = low.contains("eroare")
            || low.contains("erori")
            || low.contains("nu se incadreaza")
            || low.contains("nu se încadrează")
            || low.contains("invalid")
            || low.contains("atentionare")
            || low.contains("atenționare")
            || low.contains("nu este corect");
        if looks_error && !low.contains("fara erori") && !low.contains("fără erori") {
            errors.push(PreflightIssue {
                severity: "error".to_string(),
                code: "DUK".to_string(),
                message: l.to_string(),
                hint: "Eroare raportată de validatorul oficial ANAF (DUKIntegrator).".to_string(),
            });
        }
    }
    let clean =
        raw.to_lowercase().contains("fara erori") || raw.to_lowercase().contains("fără erori");
    DukOutcome {
        passed: errors.is_empty() && clean,
        errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parser_clean_output_is_passing() {
        let out = parse_duk_output("Validare fara erori fisier: /tmp/x.xml\n");
        assert!(out.passed);
        assert!(out.errors.is_empty());
    }

    #[test]
    fn parser_error_lines_become_issues() {
        let raw = "Atentionari la validare fisier: /tmp/x.xml\n\
                   A: validari globale\n TVA(25) nu se incadreaza in 11% +- marja 1%\n";
        let out = parse_duk_output(raw);
        assert!(!out.passed);
        assert!(!out.errors.is_empty());
        assert_eq!(out.errors[0].code, "DUK");
        assert_eq!(out.errors[0].severity, "error");
        assert!(out
            .errors
            .iter()
            .any(|i| i.message.contains("nu se incadreaza")));
    }

    #[test]
    fn env_provider_resolves_when_jar_and_java_present() {
        let p = EnvProvider;
        match std::env::var("EFACTURA_DUK_JAR") {
            Ok(j) if !j.is_empty() && std::path::Path::new(&j).is_file() => {
                let rt = p.resolve();
                assert!(rt.is_some(), "should resolve when jar present");
                let rt = rt.unwrap();
                assert!(rt.jar_dir.exists());
            }
            _ => assert!(p.resolve().is_none(), "no jar -> None, never panic"),
        }
    }
}
