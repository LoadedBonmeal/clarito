//! Layer D — real DUKIntegrator validation at runtime, behind a swappable provider.
//! `BundledProvider` resolves the JRE+jars shipped in the app; `EnvProvider` uses
//! `EFACTURA_DUK_JAR` + system `java` (dev/CI). Both produce a `DukRuntime`.

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

/// Directory that actually holds the bundled `duk/` + `jre-min/`. Tauri bundles the
/// `resources/duk/**` + `resources/jre-min/**` globs PRESERVING the `resources/` prefix, so in the
/// packaged app they live at `<resource_dir>/resources/duk`, not `<resource_dir>/duk`. Prefer the
/// nested layout when present and fall back to the resource dir itself (covers flat/dev layouts).
pub fn bundled_res_root(resource_dir: &std::path::Path) -> PathBuf {
    let nested = resource_dir.join("resources");
    if nested.join("duk").is_dir() {
        nested
    } else {
        resource_dir.to_path_buf()
    }
}

/// Runtime provider: resolves the jlink JRE + DUK jars bundled as Tauri resources.
pub struct BundledProvider {
    jre_bin: PathBuf,
    jar_dir: PathBuf,
}

impl BundledProvider {
    pub fn new(app: &tauri::AppHandle) -> Self {
        use tauri::Manager;
        let root = bundled_res_root(&app.path().resource_dir().unwrap_or_default());
        let java = if cfg!(windows) {
            "jre-min/bin/java.exe"
        } else {
            "jre-min/bin/java"
        };
        Self {
            jre_bin: root.join(java),
            jar_dir: root.join("duk"),
        }
    }
}

impl DukProvider for BundledProvider {
    fn resolve(&self) -> Option<DukRuntime> {
        if self.jre_bin.is_file() && self.jar_dir.join("DUKIntegrator.jar").is_file() {
            Some(DukRuntime {
                java: self.jre_bin.clone(),
                jar_dir: self.jar_dir.clone(),
            })
        } else {
            None // not bundled (e.g. dev) → graceful fallback
        }
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

/// Run DUK against `xml` using the runtime from `provider`. Returns `None` when no
/// runtime is available (caller falls back to layer A). Never panics.
///
/// Routing:
/// - D710 → `run_standalone_validator(java, lib/D710Validator.jar, xml)` (no `-v`, no result-file)
/// - All others → `run_java_validator(java, DUKIntegrator.jar, -v <TYPE>, xml, result)` (overlay path)
///
/// For standalone validators the specific jar (`lib/<TYPE>Validator.jar`) is probed first; if absent
/// the function returns `None` (graceful fallback — same pattern as D205).
pub fn run_duk(
    provider: &dyn DukProvider,
    decl: DeclKind,
    xml: &Path,
) -> AppResult<Option<DukOutcome>> {
    let Some(rt) = provider.resolve() else {
        return Ok(None);
    };

    if decl.is_standalone_validator() {
        // Standalone path: the specific validator jar is the sole entry point.
        let standalone_jar = rt
            .jar_dir
            .join("lib")
            .join(format!("{}Validator.jar", decl.as_duk_type()));
        if !standalone_jar.is_file() {
            // Jar absent → graceful skip (same as the D205 jar-probe pattern).
            return Ok(None);
        }
        let raw =
            crate::anaf_decl::validation::run_standalone_validator(&rt.java, &standalone_jar, xml)?;
        return Ok(Some(parse_duk_output(&raw)));
    }

    let raw = crate::anaf_decl::validation::run_java_validator(&rt.java, &rt.duk_jar(), decl, xml)?;
    Ok(Some(parse_duk_output(&raw)))
}

/// Parse DUKIntegrator's textual output (result file or stdout) into issues.
/// Clean marker: output contains "fara erori"/"fără erori". CRITICAL: ANAF
/// distinguishes ERORI (blocking) from ATENȚIONĂRI (advisory warnings) — D112
/// in particular emits many legitimate warnings (hours-vs-days, base-calc) that
/// must NOT fail validation. We classify each line by severity and only
/// error-severity issues set `passed = false`; warnings are surfaced but pass.
pub fn parse_duk_output(raw: &str) -> DukOutcome {
    let mut errors = Vec::new();
    for line in raw.lines() {
        let l = line.trim();
        if l.is_empty()
            || l.to_lowercase().contains("fara erori")
            || l.to_lowercase().contains("fără erori")
        {
            continue;
        }
        let low = l.to_lowercase();
        // Advisory warning (atenționare/atenționări) — must NOT be treated as a blocking error.
        // Match the stem across ASCII + both ț encodings (U+021B comma, U+0163 cedilla).
        let is_warning = low.contains("atention")
            || low.contains("aten\u{021b}ion")
            || low.contains("aten\u{0163}ion");
        let is_error = !is_warning
            && (low.contains("eroare")
                || low.contains("erori")
                || low.contains("nu se incadreaza")
                || low.contains("nu se încadrează")
                || low.contains("invalid")
                || low.contains("nu este corect"));
        if is_error {
            errors.push(PreflightIssue {
                severity: "error".to_string(),
                code: "DUK".to_string(),
                message: l.to_string(),
                hint: "Eroare raportată de validatorul oficial ANAF (DUKIntegrator).".to_string(),
            });
        } else if is_warning {
            errors.push(PreflightIssue {
                severity: "warning".to_string(),
                code: "DUK".to_string(),
                message: l.to_string(),
                hint: "Atenționare de la validatorul oficial ANAF (informativă, nu blochează depunerea).".to_string(),
            });
        }
    }
    let lower = raw.to_lowercase();
    let clean =
        lower.contains("fara erori") || lower.contains("fără erori") || lower.trim() == "ok";
    // Only ERROR-severity issues block; warnings are advisory.
    let has_errors = errors.iter().any(|e| e.severity == "error");
    DukOutcome {
        passed: !has_errors && clean,
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
        assert!(out
            .errors
            .iter()
            .any(|i| i.severity == "error" && i.message.contains("nu se incadreaza")));
    }

    #[test]
    fn parser_warnings_do_not_fail_validation() {
        // ANAF "atenționare" lines are advisory — surfaced as warnings but the file PASSES.
        let raw = "Validare fara erori fisier: /tmp/x.xml\n\
                   Atentionare: A_6 (ore lucrate) difera de A_8 * A_4\n";
        let out = parse_duk_output(raw);
        assert!(out.passed, "warnings alone must not fail validation");
        assert!(out.errors.iter().any(|i| i.severity == "warning"));
        assert!(!out.errors.iter().any(|i| i.severity == "error"));
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

    /// D710 routes to the standalone path (is_standalone_validator = true).
    /// When lib/D710Validator.jar is absent (sandbox), run_duk returns None gracefully —
    /// just like the D205 jar-probe pattern.
    #[test]
    fn d710_run_duk_skips_gracefully_when_jar_absent() {
        // NoopProvider always resolves to a DukRuntime pointing at a nonexistent jar_dir.
        struct NoopProvider {
            jar_dir: std::path::PathBuf,
        }
        impl DukProvider for NoopProvider {
            fn resolve(&self) -> Option<DukRuntime> {
                Some(DukRuntime {
                    java: std::path::PathBuf::from("java"),
                    jar_dir: self.jar_dir.clone(),
                })
            }
        }
        // Use a temp dir that exists but has NO D710Validator.jar inside lib/.
        let tmp = std::env::temp_dir().join("duk_d710_noop_test");
        let provider = NoopProvider {
            jar_dir: tmp.clone(),
        };
        let xml_path = std::env::temp_dir().join("d710_dummy.xml");
        // The file doesn't need to exist — the jar probe fires first.
        let result = run_duk(&provider, crate::anaf_decl::DeclKind::D710, &xml_path);
        // Should return Ok(None) because lib/D710Validator.jar is absent.
        assert!(result.is_ok(), "run_duk must not error when jar absent");
        assert!(
            result.unwrap().is_none(),
            "run_duk must return None (graceful skip) when D710Validator.jar is absent"
        );
    }

    /// D301 and D700 use the DUKIntegrator overlay path (is_standalone_validator = false).
    /// When the DukRuntime is absent (provider returns None), run_duk returns Ok(None).
    #[test]
    fn d301_d700_run_duk_return_none_when_runtime_absent() {
        struct NoneProvider;
        impl DukProvider for NoneProvider {
            fn resolve(&self) -> Option<DukRuntime> {
                None
            }
        }
        let p = NoneProvider;
        let dummy = std::path::Path::new("/nonexistent.xml");
        let r301 = run_duk(&p, crate::anaf_decl::DeclKind::D301, dummy);
        let r700 = run_duk(&p, crate::anaf_decl::DeclKind::D700, dummy);
        assert!(
            r301.is_ok() && r301.unwrap().is_none(),
            "D301: None when no runtime"
        );
        assert!(
            r700.is_ok() && r700.unwrap().is_none(),
            "D700: None when no runtime"
        );
    }
}
