//! Self-enforcing guards for invariants that MUST NOT change silently (audit Note 2).
//!
//! The Tauri bundle identifier and the build.rs license-salt derivation are load-bearing: changing
//! the identifier breaks update/installer continuity, and changing the salt seed/mask invalidates
//! every license key already issued. These were previously protected only by "verified via git
//! diff" — now any change trips the gate (`cargo test --lib`) instead.

#[cfg(test)]
mod tests {
    /// The app bundle identifier is wired into installers, updater feeds and the license tie. It is
    /// frozen — see audit Note 2.
    #[test]
    fn bundle_identifier_is_frozen() {
        let conf = include_str!("../tauri.conf.json");
        assert!(
            conf.contains("\"identifier\": \"com.lucaris.efactura\""),
            "tauri.conf.json bundle identifier must stay com.lucaris.efactura — changing it breaks \
             updater continuity and the license tie."
        );
    }

    /// The license-secret obfuscation in build.rs XOR-cycles the secrets against a salt derived from
    /// these two literals. Changing either invalidates every license already issued, so they are
    /// frozen — see audit Note 2 / SEC-05.
    #[test]
    fn license_salt_derivation_is_frozen() {
        let build = include_str!("../build.rs");
        assert!(
            build.contains("RoFactura-build-salt-2026"),
            "build.rs salt seed must stay 'RoFactura-build-salt-2026' — changing it invalidates all \
             issued license keys."
        );
        assert!(
            build.contains("RoFactura-salt-mask-v1"),
            "build.rs salt mask must stay 'RoFactura-salt-mask-v1' — changing it invalidates all \
             issued license keys."
        );
    }
}
