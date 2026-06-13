//! Launch-time check: are the app's BUNDLED declaration form versions still current
//! vs the versions ANAF currently requires (manifest published on our CDN)? If a form
//! is behind, the UI nudges the user to update the app (which ships matching DUK jars
//! + generator). Network failure is non-fatal → empty result (no banner).

use serde::Serialize;

/// Bundled form versions — MUST be bumped together with the generator + DUK jars.
const BUNDLED: &[(&str, &str)] = &[
    ("D300", "v12"),
    ("D394", "v5"),
    ("D406", "v1"),
    ("D112", "v6"),
];

const MANIFEST_URL: &str = "https://releases.lucaris.ro/efactura/anaf-forms.json";

/// One stale form: the app bundles `bundled` but the manifest reports `latest`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormStaleness {
    pub form: String,
    pub bundled: String,
    pub latest: String,
}

/// Manifest shape: { "D300": "v12", "D394": "v5", "D406": "v1" }
type Manifest = std::collections::HashMap<String, String>;

/// Pure comparison: any bundled form whose manifest value DIFFERS is stale.
pub fn compute_staleness(bundled: &[(&str, &str)], manifest: &Manifest) -> Vec<FormStaleness> {
    let mut out = Vec::new();
    for (form, bv) in bundled {
        if let Some(latest) = manifest.get(*form) {
            if latest != bv {
                out.push(FormStaleness {
                    form: form.to_string(),
                    bundled: bv.to_string(),
                    latest: latest.clone(),
                });
            }
        }
    }
    out
}

/// Fetch the manifest (5s timeout) and compute staleness. Errors/timeouts → empty (graceful).
pub async fn check() -> Vec<FormStaleness> {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let manifest: Manifest = match client.get(MANIFEST_URL).send().await {
        Ok(r) => match r.json().await {
            Ok(m) => m,
            Err(_) => return Vec::new(),
        },
        Err(_) => return Vec::new(),
    };
    compute_staleness(BUNDLED, &manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn mf(pairs: &[(&str, &str)]) -> Manifest {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }
    #[test]
    fn no_staleness_when_equal() {
        let m = mf(&[
            ("D300", "v12"),
            ("D394", "v5"),
            ("D406", "v1"),
            ("D112", "v6"),
        ]);
        assert!(compute_staleness(BUNDLED, &m).is_empty());
    }
    #[test]
    fn flags_newer_form() {
        let m = mf(&[("D300", "v13"), ("D394", "v5"), ("D406", "v1")]);
        let s = compute_staleness(BUNDLED, &m);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].form, "D300");
        assert_eq!(s[0].latest, "v13");
        assert_eq!(s[0].bundled, "v12");
    }
    #[test]
    fn empty_manifest_no_staleness() {
        assert!(compute_staleness(BUNDLED, &mf(&[])).is_empty());
    }
}
