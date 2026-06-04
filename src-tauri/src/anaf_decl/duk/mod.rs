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

#[cfg(test)]
mod tests {
    use super::*;
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
