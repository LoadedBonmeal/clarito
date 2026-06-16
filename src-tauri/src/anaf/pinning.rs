//! Report-only TLS observability for ANAF endpoints.
//!
//! Standard TLS validation is UNCHANGED — reqwest still verifies the full certificate chain against
//! the platform roots, and an invalid cert is still rejected as before. This module ONLY *observes*
//! the peer leaf certificate (enabled via `ClientBuilder::tls_info(true)`) to:
//!   1. capture the real ANAF leaf-cert SHA-256 fingerprints (logged once per unique cert) so a
//!      future ENFORCEMENT phase can pin verified values, and
//!   2. emit a WARN — never a block — when the observed fingerprint doesn't match an optional
//!      configured pin list (`ANAF_CERT_PINS`, comma-separated SHA-256 hex). Empty list ⇒ pure logging.
//!
//! It NEVER rejects a connection (report-only). Enabling hard enforcement is a deliberate future
//! change once the real pins are captured from these logs.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use sha2::{Digest, Sha256};

/// Configured pins (lower-case SHA-256 hex of the leaf cert DER) from env `ANAF_CERT_PINS`.
/// Empty ⇒ observability only (no mismatch warnings).
fn configured_pins() -> &'static Vec<String> {
    static PINS: OnceLock<Vec<String>> = OnceLock::new();
    PINS.get_or_init(|| {
        std::env::var("ANAF_CERT_PINS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect()
    })
}

/// Lower-case hex of a byte slice (no new dependency; same style as `oauth::random_bytes_hex`).
fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 fingerprint of the peer leaf certificate DER — `None` if `tls_info` is off or no TLS info.
fn leaf_fingerprint(resp: &reqwest::Response) -> Option<String> {
    let info = resp.extensions().get::<reqwest::tls::TlsInfo>()?;
    let der = info.peer_certificate()?;
    Some(to_hex(&Sha256::digest(der)))
}

/// True the FIRST time a `(host, fingerprint)` is seen — so we log once per unique cert and stay
/// quiet afterwards, while a cert CHANGE (new fingerprint) surfaces as a fresh log line.
fn first_sighting(host: &str, fp: &str) -> bool {
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let seen = SEEN.get_or_init(|| Mutex::new(HashSet::new()));
    seen.lock()
        .map(|mut s| s.insert(format!("{host}|{fp}")))
        .unwrap_or(true)
}

/// Observe (log + optional report-only warn) the ANAF endpoint's TLS leaf certificate. NEVER blocks.
pub fn observe_cert(resp: &reqwest::Response) {
    let Some(fp) = leaf_fingerprint(resp) else {
        return;
    };
    let host = resp.url().host_str().unwrap_or("?").to_string();
    if first_sighting(&host, &fp) {
        tracing::info!(host = %host, sha256 = %fp, "ANAF TLS leaf certificate observed");
        let pins = configured_pins();
        if !pins.is_empty() && !pins.contains(&fp) {
            tracing::warn!(
                host = %host, sha256 = %fp,
                "ANAF TLS certificate does NOT match any configured pin (ANAF_CERT_PINS) — possible \
                 MITM or certificate rotation (report-only: the connection was NOT blocked)"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::to_hex;
    use sha2::{Digest, Sha256};

    #[test]
    fn fingerprint_hex_is_stable_64_chars() {
        // SHA-256 of a known DER-ish blob → 64 lower-case hex chars, deterministic.
        let fp = to_hex(&Sha256::digest(b"\x30\x82test-cert"));
        assert_eq!(fp.len(), 64);
        assert_eq!(fp, to_hex(&Sha256::digest(b"\x30\x82test-cert"))); // stable
        assert!(fp
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }
}
