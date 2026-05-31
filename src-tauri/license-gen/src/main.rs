//! License key generator CLI.
//!
//! Mints valid license keys using the same KEY_HMAC_SECRET embedded in the
//! release binary. Run after a customer pays:
//!
//!     cargo run --bin license-gen -- \
//!         --tier SOLO --email customer@x.com --expires-days 365
//!
//! Output (stdout): a single line with the key — pipeable to clipboard etc.
//! Output (stderr): human-readable metadata (tier, email, expiry, build hint).
//!
//! Keys are bound to the build's KEY_HMAC_SECRET. A version bump that
//! changes the build.rs salt invalidates all previously-issued keys.

use clap::Parser;
use efactura_desktop_lib::{key_checksum, validate_license_key};
use rand::Rng;
use std::process;

#[derive(Parser, Debug)]
#[command(
    name = "license-gen",
    about = "Generates a valid RoFactura license key for a paying customer.",
    long_about = None
)]
struct Args {
    /// License tier (only SOLO is meaningful today; reserved for future tiers).
    #[arg(long, default_value = "SOLO")]
    tier: String,

    /// Customer email — embedded in the metadata (not in the key itself).
    #[arg(long)]
    email: String,

    /// Days until the activation expires (server-side activation overwrites).
    #[arg(long, default_value_t = 365)]
    expires_days: i64,

    /// Optional machine_id binding hint (24 hex chars). For documentation only;
    /// the running app computes machine_id at activation time.
    #[arg(long)]
    machine_id: Option<String>,

    /// Print only the key (no stderr metadata). Useful for scripts.
    #[arg(long)]
    quiet: bool,
}

const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

fn rand_segment(rng: &mut impl Rng) -> String {
    (0..4)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect()
}

fn generate_key() -> String {
    let mut rng = rand::thread_rng();
    let s1 = rand_segment(&mut rng);
    let s2 = rand_segment(&mut rng);
    let s3 = rand_segment(&mut rng);
    let payload = format!("{s1}-{s2}-{s3}");
    let checksum = &key_checksum(payload.as_bytes())[..8];
    format!("{payload}-{}", checksum.to_uppercase())
}

fn main() {
    let args = Args::parse();

    if !args.email.contains('@') {
        eprintln!("error: --email must contain '@'");
        process::exit(2);
    }

    let key = generate_key();

    // Round-trip sanity check — would crash here if we regress the algorithm.
    if !validate_license_key(&key) {
        eprintln!("internal error: generated key failed self-validation: {key}");
        process::exit(1);
    }

    // stdout = the key alone (pipeable)
    println!("{key}");

    if !args.quiet {
        eprintln!();
        eprintln!("RoFactura license-gen");
        eprintln!("  tier           : {}", args.tier);
        eprintln!("  email          : {}", args.email);
        eprintln!(
            "  expires        : {} days from activation",
            args.expires_days
        );
        if let Some(mid) = args.machine_id.as_ref() {
            eprintln!("  machine_id hint: {mid}");
        }
        eprintln!("  build version  : {}", env!("CARGO_PKG_VERSION"));
        eprintln!();
        eprintln!(
            "Send this key to the customer. It will activate against build v{}.",
            env!("CARGO_PKG_VERSION")
        );
        eprintln!("If you ship a new version with a different salt, previously-issued keys");
        eprintln!("will be invalidated and you must regenerate.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_keys_pass_validation() {
        for _ in 0..50 {
            let key = generate_key();
            assert!(validate_license_key(&key), "generated key rejected: {key}");
        }
    }

    #[test]
    fn key_format_matches_spec() {
        let key = generate_key();
        let parts: Vec<&str> = key.split('-').collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0].len(), 4);
        assert_eq!(parts[1].len(), 4);
        assert_eq!(parts[2].len(), 4);
        assert_eq!(parts[3].len(), 8);
        for p in &parts[..3] {
            for c in p.chars() {
                assert!(
                    c.is_ascii_uppercase() || c.is_ascii_digit(),
                    "char {c} in {p}"
                );
            }
        }
        for c in parts[3].chars() {
            assert!(
                c.is_ascii_hexdigit() && (c.is_ascii_digit() || c.is_ascii_uppercase()),
                "checksum char {c} in {}",
                parts[3]
            );
        }
    }
}
