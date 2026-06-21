//! TLS SPKI cert-pinning for ANAF endpoints — safe, optional enforcement.
//!
//! # Architecture
//!
//! A custom [`PinningVerifier`] wraps the platform trust-store verifier
//! (`rustls-platform-verifier`).  For every TLS handshake it:
//!
//! 1. **Delegates chain validation first** to the inner platform verifier — full
//!    trust-path, hostname/SAN, and expiry checks.  If the inner verifier rejects,
//!    the connection is rejected regardless of pins.  The pin check is *additive*.
//! 2. Extracts the SPKI (SubjectPublicKeyInfo) DER bytes of the leaf AND every
//!    presented intermediate certificate.
//! 3. Computes `base64(SHA-256(SPKI_DER))` for each cert (HPKP-style, per
//!    RFC 7469 §2.4).
//! 4. Compares against the active pin set.  Behaviour depends on the **mode**:
//!    - `off` (default) → no pin check; plain platform verifier.
//!    - `report` → WARN on mismatch, accept anyway.
//!    - `enforce` → REJECT on mismatch if ≥ 2 pins configured;
//!      otherwise auto-downgrades to `report` (fail-open).
//!
//! # Configuration (environment variables)
//!
//! | Variable          | Values                        | Default |
//! |-------------------|-------------------------------|---------|
//! | `ANAF_PIN_MODE`   | `off` / `report` / `enforce`  | `off`   |
//! | `ANAF_PIN_DISABLE`| `1`                           | —       |
//! | `ANAF_CERT_PINS`  | comma/space-separated base64  | built-in DigiCert root pin |
//!
//! `ANAF_PIN_DISABLE=1` forces mode `off` regardless of `ANAF_PIN_MODE`.
//!
//! # Built-in default backup pin
//!
//! The DigiCert Global Root G2 SPKI pin is always included in the effective
//! pin set: `i7WTqTvh0OioIruIfFR4kMPnBqrS2rdiVPl/s2uC/CY=`
//!
//! Since the default mode is `off`, this pin has no effect unless an operator
//! opts into `report` or `enforce`.  It is documented here as the stable root
//! backup — the ANAF wildcard leaf rotates annually, but the DigiCert root is
//! stable across rotations.
//!
//! # Shipping restrictions (IMPORTANT — read before enabling enforce)
//!
//! `enforce` **MUST NOT** be enabled by default in production until the SPKI
//! pins of all five ANAF hosts have been individually verified against the live
//! endpoints over at least one certificate-rotation cycle:
//!
//! ```sh
//! openssl s_client -connect api.anaf.ro:443 </dev/null 2>/dev/null \
//!   | openssl x509 -noout -pubkey \
//!   | openssl pkey -pubin -outform der \
//!   | openssl dgst -sha256 -binary \
//!   | base64
//! ```
//!
//! Repeat for: `logincert.anaf.ro`, `webserviced.anaf.ro`,
//! `webserviceapl.anaf.ro`, `webservicesp.anaf.ro`.
//!
//! ANAF may change its CA with no advance notice.  Pins MUST be app-updatable
//! (hot-reloadable or shipped via an update mechanism) before enabling enforce.
//! The shipped default backup pin is the DigiCert Global Root G2.

use std::collections::HashSet;
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{CertificateError, DigitallySignedStruct, Error as TlsError, SignatureScheme};
use sha2::{Digest, Sha256};

// ── Built-in default backup pin ───────────────────────────────────────────────
/// DigiCert Global Root G2 SPKI pin (base64(SHA-256(SubjectPublicKeyInfo DER))).
/// Stable across ANAF leaf rotations; documented default backup.
const DIGICERT_GLOBAL_ROOT_G2_PIN: &str = "i7WTqTvh0OioIruIfFR4kMPnBqrS2rdiVPl/s2uC/CY=";

// ── Mode ──────────────────────────────────────────────────────────────────────

/// Resolved enforcement mode for the pinning verifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinMode {
    /// No pin check at all (plain platform chain validation).  DEFAULT.
    Off,
    /// Warn on mismatch, always accept (report-only).
    Report,
    /// Reject on mismatch — only active when `pin_count >= 2`.
    Enforce,
}

impl fmt::Display for PinMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PinMode::Off => write!(f, "off"),
            PinMode::Report => write!(f, "report"),
            PinMode::Enforce => write!(f, "enforce"),
        }
    }
}

/// Parse `ANAF_PIN_MODE` env var into a [`PinMode`].
///
/// Unknown/missing values → `Off` (safe default).
/// `ANAF_PIN_DISABLE=1` → `Off` unconditionally.
pub fn resolve_mode() -> PinMode {
    let disable = std::env::var("ANAF_PIN_DISABLE").as_deref() == Ok("1");
    let mode_str = std::env::var("ANAF_PIN_MODE").unwrap_or_default();
    resolve_mode_from(mode_str.trim(), disable)
}

/// Pure, testable core of mode resolution (no env access).
pub fn resolve_mode_from(mode_str: &str, disable: bool) -> PinMode {
    if disable {
        return PinMode::Off;
    }
    match mode_str.to_ascii_lowercase().as_str() {
        "report" => PinMode::Report,
        "enforce" => PinMode::Enforce,
        _ => PinMode::Off,
    }
}

// ── Pin set ───────────────────────────────────────────────────────────────────

/// Parse configured pins from `ANAF_CERT_PINS` + the built-in default backup pin.
///
/// `ANAF_CERT_PINS` may be comma-or-space-separated base64 strings.
/// Entries that are not valid base64 are silently ignored (logged once).
/// The DigiCert Global Root G2 pin is always included in the result.
pub fn configured_pins() -> Vec<String> {
    let raw = std::env::var("ANAF_CERT_PINS").unwrap_or_default();
    parse_pin_list(&raw)
}

/// Pure, testable pin-list parser.  Input is the raw value of `ANAF_CERT_PINS`
/// (comma-or-space-separated base64 strings).  Always appends the built-in
/// DigiCert Global Root G2 backup pin.
pub fn parse_pin_list(raw: &str) -> Vec<String> {
    let mut pins: Vec<String> = raw
        .split([',', ' '])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .filter(|s| {
            if BASE64.decode(s).is_ok() {
                true
            } else {
                tracing::warn!(pin = %s, "ANAF_CERT_PINS: invalid base64 entry ignored");
                false
            }
        })
        .collect();
    // Always include the built-in DigiCert Global Root G2 backup pin.
    let builtin = DIGICERT_GLOBAL_ROOT_G2_PIN.to_string();
    if !pins.contains(&builtin) {
        pins.push(builtin);
    }
    pins
}

// ── SPKI extraction ───────────────────────────────────────────────────────────

/// Extract the raw SubjectPublicKeyInfo DER bytes from a DER-encoded X.509 certificate.
///
/// X.509 structure (DER):
/// ```text
/// Certificate (SEQUENCE) {
///   TBSCertificate (SEQUENCE) {
///     version        [0] EXPLICIT ... (optional)
///     serialNumber   INTEGER
///     signature      AlgorithmIdentifier (SEQUENCE)
///     issuer         Name (SEQUENCE)
///     validity       Validity (SEQUENCE)
///     subject        Name (SEQUENCE)
///     subjectPublicKeyInfo SubjectPublicKeyInfo (SEQUENCE)  ← return this
///     ...
///   }
///   ...
/// }
/// ```
///
/// Returns `None` on any parse error; the caller treats that as a missing pin.
pub fn extract_spki_der(cert_der: &[u8]) -> Option<Vec<u8>> {
    // Outer SEQUENCE — the Certificate
    let tbs = read_sequence_contents(cert_der)?;
    // First element of Certificate is TBSCertificate (SEQUENCE)
    let tbs_contents = read_sequence_contents(tbs)?;
    let mut rest = tbs_contents;

    // [0] EXPLICIT version — optional, tag byte 0xa0
    if rest.first() == Some(&0xa0) {
        let (_, after) = read_tlv(rest)?;
        rest = after;
    }
    // serialNumber INTEGER (tag 0x02)
    let (_, rest) = read_tlv(rest)?;
    // signature AlgorithmIdentifier (SEQUENCE, tag 0x30)
    let (_, rest) = read_tlv(rest)?;
    // issuer Name (SEQUENCE, tag 0x30)
    let (_, rest) = read_tlv(rest)?;
    // validity Validity (SEQUENCE, tag 0x30)
    let (_, rest) = read_tlv(rest)?;
    // subject Name (SEQUENCE, tag 0x30)
    let (_, rest) = read_tlv(rest)?;
    // subjectPublicKeyInfo (SEQUENCE, tag 0x30) — take the RAW TLV bytes
    let (spki_tlv, _) = split_tlv(rest)?;
    Some(spki_tlv.to_vec())
}

/// Return `(tag, value_bytes, rest_after_tlv)` — skip the TLV, return rest.
fn read_tlv(data: &[u8]) -> Option<(&[u8], &[u8])> {
    let (tlv, rest) = split_tlv(data)?;
    Some((tlv, rest))
}

/// Return `(full_tlv_bytes, bytes_after_tlv)` without consuming contents.
fn split_tlv(data: &[u8]) -> Option<(&[u8], &[u8])> {
    if data.is_empty() {
        return None;
    }
    let (len_bytes, content_len) = der_length(&data[1..])?;
    let total = 1 + len_bytes + content_len;
    if data.len() < total {
        return None;
    }
    Some((&data[..total], &data[total..]))
}

/// Parse a DER length field starting at `data[0]`.
///
/// Returns `(number_of_length_bytes_consumed, content_length)`.
fn der_length(data: &[u8]) -> Option<(usize, usize)> {
    let first = *data.first()?;
    if first < 0x80 {
        // Short form
        Some((1, first as usize))
    } else {
        let n_bytes = (first & 0x7f) as usize;
        if n_bytes == 0 || n_bytes > 4 || data.len() < 1 + n_bytes {
            return None; // Indefinite-length or too large
        }
        let mut len: usize = 0;
        for &b in &data[1..=n_bytes] {
            len = len.checked_shl(8)?.checked_add(b as usize)?;
        }
        Some((1 + n_bytes, len))
    }
}

/// Parse a DER SEQUENCE tag (0x30), return a slice of the SEQUENCE contents.
fn read_sequence_contents(data: &[u8]) -> Option<&[u8]> {
    if data.first() != Some(&0x30) {
        return None;
    }
    let (len_bytes, content_len) = der_length(&data[1..])?;
    let start = 1 + len_bytes;
    let end = start.checked_add(content_len)?;
    if data.len() < end {
        return None;
    }
    Some(&data[start..end])
}

// ── SPKI pin computation ──────────────────────────────────────────────────────

/// Compute `base64(SHA-256(spki_der))` — the HPKP-style pin for one SPKI blob.
pub fn spki_pin(spki_der: &[u8]) -> String {
    BASE64.encode(Sha256::digest(spki_der).as_slice())
}

/// Compute SPKI pins for a leaf cert and all presented intermediates.
///
/// Returns a `Vec<String>` of `base64(SHA-256(SPKI))` — one per cert that
/// could be successfully parsed.  Missing certs / parse failures are skipped.
pub fn compute_chain_pins<'a>(
    end_entity: &CertificateDer<'_>,
    intermediates: &'a [CertificateDer<'a>],
) -> Vec<String> {
    std::iter::once(end_entity.as_ref())
        .chain(intermediates.iter().map(|c| c.as_ref()))
        .filter_map(|cert_der: &[u8]| extract_spki_der(cert_der))
        .map(|spki: Vec<u8>| spki_pin(&spki))
        .collect()
}

// ── Pin-match decision (pure, testable) ──────────────────────────────────────

/// Decision returned by [`pin_decision`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinDecision {
    /// Accept the connection.
    Accept,
    /// Reject the connection (enforce mode, ≥ 2 pins configured, no match).
    Reject,
    /// Accept but log a warning (report mode or enforce with < 2 pins).
    AcceptWithWarn,
}

/// Pure function: given the mode, the effective pin set, and the computed
/// chain pins, return what the verifier should do.
///
/// Rules:
/// - `inner_ok = false` → always `Reject` (chain validation failed first).
/// - `Off` → always `Accept` (no pin check).
/// - `Report` → no match → `AcceptWithWarn`; match → `Accept`.
/// - `Enforce` AND `pin_count >= 2` → no match → `Reject`; match → `Accept`.
/// - `Enforce` AND `pin_count < 2` → auto-downgrade: no match → `AcceptWithWarn`
///   (fail-open to avoid bricking connectivity with a misconfigured pin set).
pub fn pin_decision(
    inner_ok: bool,
    mode: PinMode,
    configured: &[String],
    chain_pins: &[String],
) -> PinDecision {
    if !inner_ok {
        return PinDecision::Reject;
    }
    match mode {
        PinMode::Off => PinDecision::Accept,
        PinMode::Report => {
            if chain_pins.iter().any(|p| configured.contains(p)) {
                PinDecision::Accept
            } else {
                PinDecision::AcceptWithWarn
            }
        }
        PinMode::Enforce => {
            let matched = chain_pins.iter().any(|p| configured.contains(p));
            if matched {
                PinDecision::Accept
            } else if configured.len() >= 2 {
                PinDecision::Reject
            } else {
                // Fail-open: misconfigured (< 2 pins) → report mode
                tracing::warn!(
                    "ANAF_PIN_MODE=enforce but fewer than 2 valid pins are configured — \
                     auto-downgrading to report mode (fail-open) to avoid bricking connectivity"
                );
                PinDecision::AcceptWithWarn
            }
        }
    }
}

// ── Dedup warn tracker ────────────────────────────────────────────────────────

/// Return `true` the FIRST time `(host, pins_joined)` is seen — log once only.
fn first_warn_sighting(host: &str, chain_pins: &[String]) -> bool {
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let key = format!("{host}|{}", chain_pins.join(","));
    let seen = SEEN.get_or_init(|| Mutex::new(HashSet::new()));
    seen.lock().map(|mut s| s.insert(key)).unwrap_or(true)
}

// ── PinningVerifier ───────────────────────────────────────────────────────────

/// A `rustls` [`ServerCertVerifier`] that:
///
/// 1. Delegates to the platform verifier (full chain validation, hostname, expiry).
/// 2. Optionally enforces SPKI pins depending on [`PinMode`].
///
/// Created via [`build_pinning_verifier`].  In `off` mode this is a thin
/// passthrough and the behaviour is identical to the plain platform verifier.
pub struct PinningVerifier {
    inner: Arc<dyn ServerCertVerifier>,
    mode: PinMode,
    pins: Vec<String>,
}

impl fmt::Debug for PinningVerifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PinningVerifier")
            .field("mode", &self.mode)
            .field("pin_count", &self.pins.len())
            .finish()
    }
}

impl ServerCertVerifier for PinningVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        // 1. Always run chain validation first.  Never bypass it.
        let chain_result = self.inner.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        );
        let inner_ok = chain_result.is_ok();

        // 2. Off mode — just forward the inner result.
        if self.mode == PinMode::Off {
            return chain_result;
        }

        // 3. Compute SPKI pins from the full presented chain.
        let chain_pins = compute_chain_pins(end_entity, intermediates);
        let host = match server_name {
            ServerName::DnsName(n) => n.as_ref().to_string(),
            _ => "<non-dns>".to_string(),
        };

        // 4. Decision.
        let decision = pin_decision(inner_ok, self.mode, &self.pins, &chain_pins);

        match decision {
            PinDecision::Accept => {
                // Either chain + pin both ok, or mode==off (handled above).
                chain_result
            }
            PinDecision::AcceptWithWarn => {
                if first_warn_sighting(&host, &chain_pins) {
                    tracing::warn!(
                        host = %host,
                        computed_pins = ?chain_pins,
                        configured_pins = ?self.pins,
                        mode = %self.mode,
                        "ANAF TLS: no SPKI pin match — possible certificate rotation or \
                         misconfigured pins (connection NOT blocked)"
                    );
                }
                // Accept: return the chain result (Ok or the inner error if any)
                chain_result
            }
            PinDecision::Reject => {
                if !inner_ok {
                    // Chain validation failed (expired, hostname mismatch, untrusted
                    // root, etc.).  Propagate the ORIGINAL inner error — do NOT
                    // relabel it as PinRejected, which would obscure the true cause
                    // in logs and diagnostics.  Security behaviour is unchanged:
                    // the connection is still rejected.
                    return chain_result;
                }
                // enforce mode, ≥ 2 pins configured, inner chain OK, but no pin matched.
                // This is a genuine SPKI-pin mismatch — emit PinRejected.
                tracing::error!(
                    host = %host,
                    computed_pins = ?chain_pins,
                    configured_pins = ?self.pins,
                    "ANAF TLS: SPKI pin enforcement — no pin match, connection REJECTED \
                     (ANAF_PIN_MODE=enforce); set ANAF_PIN_DISABLE=1 to bypass"
                );
                Err(TlsError::InvalidCertificate(CertificateError::Other(
                    rustls::OtherError(Arc::new(PinRejected {
                        host: host.clone(),
                        computed_pins: chain_pins.clone(),
                    })),
                )))
            }
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

// ── Distinct enforce-rejection error ─────────────────────────────────────────

/// Distinct error emitted when `enforce` mode rejects a connection due to no
/// SPKI pin match.  Distinguishable from generic TLS/network failures in logs.
#[derive(Debug, Clone)]
pub struct PinRejected {
    pub host: String,
    pub computed_pins: Vec<String>,
}

impl fmt::Display for PinRejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ANAF_PIN_ENFORCE: no SPKI pin matched for host '{}' (computed: {:?}). \
             Set ANAF_PIN_DISABLE=1 to bypass; verify pins per docs/ANAF_INTEGRATION_TESTING.md.",
            self.host, self.computed_pins
        )
    }
}

impl std::error::Error for PinRejected {}

// ── Client builder ────────────────────────────────────────────────────────────

/// Build a `reqwest::Client` with the ANAF pinning verifier installed.
///
/// The verifier mode is resolved once from env at call time.  In `off` mode
/// (the default) this returns a client identical in behaviour to one built
/// without this function — no BRICK RISK.
///
/// Wire all ANAF host clients through this function so pinning covers every
/// ANAF TLS handshake (e-Factura, OAuth, SPVWS2, e-Transport, e-TVA).
pub fn build_pinned_client(timeout_secs: u64) -> reqwest::Client {
    let mode = resolve_mode();
    let pins = configured_pins();

    // Build the rustls ClientConfig with a PinningVerifier.
    let crypto = Arc::new(rustls::crypto::ring::default_provider());
    let inner_verifier = match rustls_platform_verifier::Verifier::new(crypto.clone()) {
        Ok(v) => Arc::new(v) as Arc<dyn ServerCertVerifier>,
        Err(e) => {
            tracing::error!(
                err = %e,
                "Failed to build platform TLS verifier; falling back to plain reqwest client"
            );
            // Safety valve: return a plain client so the app isn't bricked.
            return plain_client(timeout_secs);
        }
    };

    let verifier = Arc::new(PinningVerifier {
        inner: inner_verifier,
        mode,
        pins: pins.clone(),
    });

    tracing::info!(
        mode = %mode,
        pin_count = pins.len(),
        "ANAF TLS pinning verifier initialised"
    );

    // Build a rustls ClientConfig with our custom verifier.
    let tls_config = match rustls::ClientConfig::builder_with_provider(crypto)
        .with_safe_default_protocol_versions()
    {
        Err(e) => {
            tracing::error!(
                err = %e,
                "Failed to set TLS protocol versions; falling back to plain reqwest client"
            );
            return plain_client(timeout_secs);
        }
        Ok(builder) => builder
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth(),
    };

    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .use_preconfigured_tls(tls_config)
        .build()
        .unwrap_or_else(|e| {
            tracing::error!(
                err = %e,
                "Failed to build pinned reqwest client; falling back to plain client"
            );
            reqwest::Client::new()
        })
}

/// Fallback: plain reqwest client (no custom verifier, TLS info captured for legacy observe_cert).
fn plain_client(timeout_secs: u64) -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .tls_info(true)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

// ── Legacy observe_cert (preserved for call sites not yet migrated) ───────────

/// Original report-only TLS observability — kept for backward compatibility.
///
/// This is called ONLY when the client was NOT built via [`build_pinned_client`]
/// (i.e. a plain `reqwest::Client` with `.tls_info(true)`).  Once all clients
/// go through `build_pinned_client`, this becomes a no-op shim.
pub fn observe_cert(resp: &reqwest::Response) {
    let Some(fp) = leaf_fingerprint_hex(resp) else {
        return;
    };
    let host = resp.url().host_str().unwrap_or("?").to_string();
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let seen = SEEN.get_or_init(|| Mutex::new(HashSet::new()));
    let is_new = seen
        .lock()
        .map(|mut s| s.insert(format!("{host}|{fp}")))
        .unwrap_or(true);
    if is_new {
        tracing::info!(host = %host, sha256 = %fp, "ANAF TLS leaf certificate observed (legacy)");
    }
}

/// SHA-256 hex of the peer LEAF cert DER from `TlsInfo` (leaf only, not SPKI).
fn leaf_fingerprint_hex(resp: &reqwest::Response) -> Option<String> {
    let info = resp.extensions().get::<reqwest::tls::TlsInfo>()?;
    let der = info.peer_certificate()?;
    Some(to_hex(&Sha256::digest(der)))
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Pin parsing (pure fn — no env mutation, no parallel-test races) ──────

    #[test]
    fn pin_parsing_valid_base64_accepted() {
        // Two valid base64 strings (SHA-256 digests in base64 are 44 chars with padding)
        let pin_a = BASE64.encode([0xAAu8; 32]);
        let pin_b = BASE64.encode([0xBBu8; 32]);
        let pins = parse_pin_list(&format!("{},{}", pin_a, pin_b));
        assert!(pins.contains(&pin_a), "pin_a must be present");
        assert!(pins.contains(&pin_b), "pin_b must be present");
        assert!(
            pins.contains(&DIGICERT_GLOBAL_ROOT_G2_PIN.to_string()),
            "built-in root pin always present"
        );
    }

    #[test]
    fn pin_parsing_invalid_base64_ignored() {
        let pins = parse_pin_list("!!not-base64!!");
        // Invalid entry is ignored; only the built-in remains.
        assert_eq!(pins, vec![DIGICERT_GLOBAL_ROOT_G2_PIN.to_string()]);
    }

    #[test]
    fn pin_parsing_space_separated() {
        let pin_a = BASE64.encode([0xCCu8; 32]);
        let pin_b = BASE64.encode([0xDDu8; 32]);
        let pins = parse_pin_list(&format!("{} {}", pin_a, pin_b));
        assert!(pins.contains(&pin_a));
        assert!(pins.contains(&pin_b));
    }

    #[test]
    fn pin_parsing_empty_env_only_builtin() {
        let pins = parse_pin_list("");
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0], DIGICERT_GLOBAL_ROOT_G2_PIN);
    }

    // ── Mode resolution (pure fn — no env mutation, no parallel-test races) ──

    #[test]
    fn mode_default_is_off() {
        assert_eq!(resolve_mode_from("", false), PinMode::Off);
        assert_eq!(resolve_mode_from("unknown", false), PinMode::Off);
        assert_eq!(resolve_mode_from("  ", false), PinMode::Off);
    }

    #[test]
    fn mode_report() {
        assert_eq!(resolve_mode_from("report", false), PinMode::Report);
        assert_eq!(resolve_mode_from("REPORT", false), PinMode::Report);
    }

    #[test]
    fn mode_enforce_two_pins() {
        assert_eq!(resolve_mode_from("enforce", false), PinMode::Enforce);
        assert_eq!(resolve_mode_from("ENFORCE", false), PinMode::Enforce);
    }

    #[test]
    fn mode_disable_overrides_enforce() {
        // ANAF_PIN_DISABLE=1 → Off regardless of mode string.
        assert_eq!(resolve_mode_from("enforce", true), PinMode::Off);
        assert_eq!(resolve_mode_from("report", true), PinMode::Off);
        assert_eq!(resolve_mode_from("", true), PinMode::Off);
    }

    #[test]
    fn mode_unknown_value_defaults_to_off() {
        assert_eq!(resolve_mode_from("strict", false), PinMode::Off);
        assert_eq!(resolve_mode_from("1", false), PinMode::Off);
    }

    // ── SPKI pin computation ──────────────────────────────────────────────────

    #[test]
    fn spki_pin_known_input() {
        // base64(SHA256(b"hello")) — deterministic reference value.
        let expected = BASE64.encode(sha2::Sha256::digest(b"hello"));
        assert_eq!(spki_pin(b"hello"), expected);
    }

    #[test]
    fn spki_pin_sha256_base64_length() {
        // SHA-256 is 32 bytes → 44 base64 chars (with padding).
        let pin = spki_pin(b"any SPKI DER bytes here");
        assert_eq!(pin.len(), 44);
    }

    // ── DER SPKI extraction ───────────────────────────────────────────────────

    /// Build a minimal, valid DER-encoded X.509 v3 self-signed certificate
    /// with a known SPKI, sufficient for parse testing.  The cert is NOT
    /// cryptographically valid (signatures are zeroed) — purely structural.
    fn make_test_cert_der() -> (Vec<u8>, Vec<u8>) {
        // We build a minimal TBSCertificate by hand in DER.
        // Fields: version [0] INTEGER 2, serialNumber INTEGER 1,
        //   signature AlgId (SHA256WithRSA OID), issuer/subject (minimal
        //   SEQUENCE), validity (two GeneralizedTime), SPKI (known bytes).

        // SPKI: a minimal RSA SPKI (algorithm + public key blob) — 8 bytes of zeros
        // wrapped in a SEQUENCE.
        let known_spki_content: &[u8] = &[
            0x30, 0x06, // SEQUENCE, 6 bytes
            0x30, 0x02, 0x05, 0x00, // AlgorithmIdentifier NULL
            0x03, 0x00, // BIT STRING, 0 bytes
        ];
        // Make a minimal Name (one empty SEQUENCE-OF-SET)
        let empty_name: &[u8] = &[0x30, 0x00]; // SEQUENCE {}
                                               // Validity: two UTCTime "700101000000Z"
        let utctime: &[u8] = b"\x17\x0d700101000000Z";
        let validity = seq(&[utctime, utctime]);
        // AlgorithmIdentifier for signature
        let alg_id: &[u8] = &[0x30, 0x02, 0x05, 0x00]; // SEQUENCE { NULL }
                                                       // serialNumber INTEGER 1
        let serial: &[u8] = &[0x02, 0x01, 0x01];
        // version [0] EXPLICIT INTEGER 2 (v3)
        let version: &[u8] = &[0xa0, 0x03, 0x02, 0x01, 0x02];

        let tbs_inner: Vec<u8> = [
            version,
            serial,
            alg_id,
            empty_name, // issuer
            &validity,
            empty_name, // subject
            known_spki_content,
        ]
        .concat();

        let tbs = seq_wrap(&tbs_inner);

        // Full Certificate: TBSCertificate + signatureAlgorithm + signatureValue
        let sig_alg: &[u8] = &[0x30, 0x02, 0x05, 0x00];
        let sig_val: &[u8] = &[0x03, 0x01, 0x00]; // BIT STRING 0 bytes
        let cert_inner: Vec<u8> = [tbs.as_slice(), sig_alg, sig_val].concat();
        let cert_der = seq_wrap(&cert_inner);

        (cert_der, known_spki_content.to_vec())
    }

    fn seq(parts: &[&[u8]]) -> Vec<u8> {
        let inner: Vec<u8> = parts.concat();
        seq_wrap(&inner)
    }

    fn seq_wrap(inner: &[u8]) -> Vec<u8> {
        let mut v = vec![0x30u8];
        encode_der_len(&mut v, inner.len());
        v.extend_from_slice(inner);
        v
    }

    fn encode_der_len(out: &mut Vec<u8>, len: usize) {
        if len < 0x80 {
            out.push(len as u8);
        } else if len < 0x100 {
            out.extend_from_slice(&[0x81, len as u8]);
        } else {
            out.extend_from_slice(&[0x82, (len >> 8) as u8, (len & 0xff) as u8]);
        }
    }

    #[test]
    fn extract_spki_der_roundtrip() {
        let (cert_der, expected_spki) = make_test_cert_der();
        let extracted = extract_spki_der(&cert_der).expect("SPKI extraction must succeed");
        assert_eq!(
            extracted, expected_spki,
            "Extracted SPKI must match the known SPKI bytes"
        );
    }

    #[test]
    fn extract_spki_der_bad_input_returns_none() {
        assert!(extract_spki_der(&[]).is_none(), "empty → None");
        assert!(extract_spki_der(&[0xFF; 10]).is_none(), "garbage → None");
        assert!(
            extract_spki_der(&[0x30, 0x01, 0x00]).is_none(),
            "truncated → None"
        );
    }

    /// Regression guard: feed the REAL DigiCert Global Root G2 certificate DER
    /// through `extract_spki_der` and assert that the resulting SPKI pin matches
    /// the built-in constant.  This locks in the correctness of the hand-rolled
    /// DER walker against a known, public root certificate across future refactors.
    ///
    /// The fixture is the official DigiCert Global Root G2 (SHA-256 SPKI pin:
    /// `i7WTqTvh0OioIruIfFR4kMPnBqrS2rdiVPl/s2uC/CY=`, stable until 2038).
    #[test]
    fn extract_spki_der_real_digicert_global_root_g2_matches_builtin_pin() {
        // The DER is the public DigiCert Global Root G2 root certificate (914 bytes,
        // SHA-256 SPKI pin verified independently via `openssl x509 … | openssl pkey …
        // | openssl dgst -sha256 -binary | base64`).
        let cert_der: &[u8] = include_bytes!("digicert_global_root_g2.der");

        let spki = extract_spki_der(cert_der)
            .expect("extract_spki_der must succeed on a real X.509 certificate");

        let computed_pin = spki_pin(&spki);
        assert_eq!(
            computed_pin, DIGICERT_GLOBAL_ROOT_G2_PIN,
            "SPKI pin computed from DigiCert Global Root G2 DER must match the built-in constant"
        );
    }

    // ── Pin-decision pure function ────────────────────────────────────────────

    #[test]
    fn decision_off_always_accept() {
        let pins = vec!["pinA".to_string(), "pinB".to_string()];
        assert_eq!(
            pin_decision(true, PinMode::Off, &pins, &["other".to_string()]),
            PinDecision::Accept
        );
        // Even when inner failed, off mode will Accept — but inner_ok=false always Rejects.
        // (Off mode short-circuits before checking inner_ok for Accept, but reject happens first)
        assert_eq!(
            pin_decision(false, PinMode::Off, &pins, &[]),
            PinDecision::Reject,
            "inner failure must always reject"
        );
    }

    #[test]
    fn decision_report_match_accepts() {
        let pins = vec!["pinA".to_string(), "pinB".to_string()];
        let chain = vec!["pinA".to_string()];
        assert_eq!(
            pin_decision(true, PinMode::Report, &pins, &chain),
            PinDecision::Accept
        );
    }

    #[test]
    fn decision_report_no_match_warn_accept() {
        let pins = vec!["pinA".to_string(), "pinB".to_string()];
        let chain = vec!["other".to_string()];
        assert_eq!(
            pin_decision(true, PinMode::Report, &pins, &chain),
            PinDecision::AcceptWithWarn
        );
    }

    #[test]
    fn decision_enforce_one_pin_downgrades_to_warn() {
        // Only 1 pin configured → enforce auto-downgrades to report (fail-open).
        let pins = vec!["pinA".to_string()];
        let chain = vec!["other".to_string()];
        assert_eq!(
            pin_decision(true, PinMode::Enforce, &pins, &chain),
            PinDecision::AcceptWithWarn,
            "enforce with <2 pins must fail-open (AcceptWithWarn)"
        );
    }

    #[test]
    fn decision_enforce_two_pins_no_match_rejects() {
        let pins = vec!["pinA".to_string(), "pinB".to_string()];
        let chain = vec!["other".to_string()];
        assert_eq!(
            pin_decision(true, PinMode::Enforce, &pins, &chain),
            PinDecision::Reject
        );
    }

    #[test]
    fn decision_enforce_two_pins_match_accepts() {
        let pins = vec!["pinA".to_string(), "pinB".to_string()];
        let chain = vec!["pinB".to_string()];
        assert_eq!(
            pin_decision(true, PinMode::Enforce, &pins, &chain),
            PinDecision::Accept
        );
    }

    #[test]
    fn decision_chain_failure_always_rejects_regardless_of_pins() {
        // inner_ok=false must reject even in off mode (the inner error propagates).
        let pins = vec!["pinA".to_string(), "pinB".to_string()];
        let chain = vec!["pinA".to_string()];
        assert_eq!(
            pin_decision(false, PinMode::Report, &pins, &chain),
            PinDecision::Reject,
            "chain failure → reject in report mode"
        );
        assert_eq!(
            pin_decision(false, PinMode::Enforce, &pins, &chain),
            PinDecision::Reject,
            "chain failure → reject in enforce mode"
        );
        assert_eq!(
            pin_decision(false, PinMode::Off, &pins, &chain),
            PinDecision::Reject,
            "chain failure → reject in off mode"
        );
    }

    // ── SHA-256 + base64 helper ───────────────────────────────────────────────

    #[test]
    fn spki_pin_stable_and_deterministic() {
        let input = b"\x30\x82test-spki";
        let p1 = spki_pin(input);
        let p2 = spki_pin(input);
        assert_eq!(p1, p2, "pin must be deterministic");
        assert!(BASE64.decode(&p1).is_ok(), "pin must be valid base64");
    }

    #[test]
    fn hex_helper_64_chars_lowercase() {
        use sha2::Sha256;
        let fp = to_hex(&Sha256::digest(b"\x30\x82test-cert"));
        assert_eq!(fp.len(), 64);
        assert!(fp
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }
}
