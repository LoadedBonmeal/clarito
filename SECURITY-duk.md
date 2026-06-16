# Security note — bundled DUK validator (Bouncy Castle 1.45)

**Status: accepted low risk — no code/binary change.** (Audit 2026, verified by code + threat-model review.)

## What was flagged
The bundled ANAF DUK validator kit (`src-tauri/resources/duk/`) ships **Bouncy Castle 1.45**
(`lib/bcprov-jdk15-145.jar`, `lib/bcmail-jdk15-145.jar`, ~2012) with known CVEs — e.g.
CVE-2013-1624 (AES-CBC timing oracle), the BKS keystore hash-collision, and an ECDH timing leak.
Both JARs are declared in `DUKIntegrator.jar`'s `MANIFEST.MF` `Class-Path`.

## Why it is low risk here (not exploitable in our usage)
DUK is invoked **validation-only**: `java -jar DUKIntegrator.jar -v <TYPE> <xml> <result>`
(see `src-tauri/src/anaf_decl/validation.rs`). In that path:
- **No symmetric decryption** of attacker-controlled data → CVE-2013-1624 (CBC timing) unreachable.
- **No untrusted keystore is loaded** (only ANAF's own bundled schemas) → BKS collision unreachable.
- **No ECDH key agreement** is performed → ECDH timing leak unreachable.
- BC code is only touched via iText's OCSP/TSA adapters, which run **only for the `-p`/`-s`
  PDF-signing modes — Clarito uses `-v` exclusively**.

The validator runs **locally, offline, on the user's OWN generated XML** — no network attack
surface, no attacker-supplied cryptographic material, no timing-oracle feasibility (one-shot
validation). The bundled JRE (OpenJDK **17.0.19**) is current.

## Why we don't change it
- It is **ANAF's closed validator bundle** — we cannot patch the Bouncy Castle version inside it.
- `bcprov`/`bcmail` are on `DUKIntegrator.jar`'s `Class-Path`; **removing them risks breaking
  validation** (`ClassNotFoundException`), so they are intentionally left in place.

## Update path (when a newer official bundle ships)
`scripts/fetch-validators.sh` re-fetches the kit from the official ANAF DUKIntegrator page
(<https://static.anaf.ro/static/DUKIntegrator/DUKIntegrator.htm>). On the next refresh, check
whether the new bundle ships Bouncy Castle ≥ 1.52 (CVE-2013-1624 is fixed in 1.48+) and, if so,
re-bundle and re-run the DUK validation tests. If we ever add PDF-signing (`-p`/`-s`), require a
bundle with a modern Bouncy Castle first.
