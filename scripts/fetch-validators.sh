#!/usr/bin/env bash
# fetch-validators.sh — download the ANAF DUKIntegrator jar + XSD schema bundles
#
# USAGE:
#   bash scripts/fetch-validators.sh
#
# After running, export the jar path so `cargo test` can find it:
#   export EFACTURA_DUK_JAR="$PWD/src-tauri/tools/dukintegrator/DUKIntegrator.jar"
#
# NOTE: ANAF publishes new validator versions roughly twice a year (usually when
# a new declaration schema is mandated). When that happens:
#   1. Check the DUKIntegrator index page for the latest filename:
#      https://static.anaf.ro/static/DUKIntegrator/DUKIntegrator.htm
#   2. Update DUK_JAR_URL below.
#   3. Update XSD bundle URLs for any changed declarations.
#   4. Re-vendor the XSD and add / update golden fixtures in Phase 2+.
#
# This script is NOT auto-run by the build — run it manually on dev machines
# and on CI agents that need the full validation gate. The downloaded artifacts
# live under src-tauri/tools/ which is git-ignored.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TOOLS_DIR="$REPO_ROOT/src-tauri/tools/dukintegrator"

mkdir -p "$TOOLS_DIR"

echo ""
echo "══════════════════════════════════════════════════════"
echo "  efactura-desktop — fetch ANAF validators"
echo "  Target: $TOOLS_DIR"
echo "══════════════════════════════════════════════════════"
echo ""

# ── DUKIntegrator jar ────────────────────────────────────────────────────────
# Index page (check here for the latest version):
#   https://static.anaf.ro/static/DUKIntegrator/DUKIntegrator.htm
#
# The filename pattern is DUKIntegrator_<VERSION>.zip or DUKIntegrator.jar
# depending on the release. Verify the exact name on the index page before
# updating this URL.
DUK_JAR_URL="https://static.anaf.ro/static/DUKIntegrator/DUKIntegrator.jar"
DUK_JAR_PATH="$TOOLS_DIR/DUKIntegrator.jar"

echo "▶ Downloading DUKIntegrator jar ..."
echo "  URL: $DUK_JAR_URL"
curl -fL --progress-bar -o "$DUK_JAR_PATH" "$DUK_JAR_URL"
echo "  Saved: $DUK_JAR_PATH"
echo ""

# ── SAF-T D406 XSD schema ────────────────────────────────────────────────────
# Check the SAF-T download directory for the latest XSD filename:
#   https://static.anaf.ro/static/10/Anaf/Informatii_R/
#
# Example filename (as of 2023-07):
#   Ro_SAFT_Schema_v248_20230731.xsd
# Validator ZIP bundles (pattern):
#   duk_SAFT_an_luna_<YYYYMM>.zip  (released alongside monthly schema updates)
#
# Update SAFT_XSD_URL when ANAF publishes a new schema.
SAFT_XSD_URL="https://static.anaf.ro/static/10/Anaf/Informatii_R/Ro_SAFT_Schema_v248_20230731.xsd"
SAFT_XSD_PATH="$TOOLS_DIR/Ro_SAFT_Schema_v248.xsd"

echo "▶ Downloading SAF-T D406 XSD schema ..."
echo "  URL: $SAFT_XSD_URL"
curl -fL --progress-bar -o "$SAFT_XSD_PATH" "$SAFT_XSD_URL" || {
    echo "  WARNING: SAF-T XSD download failed (URL may have changed — check ANAF index)."
    echo "  Continuing without it; D406 golden-fixture tests will be skipped until vendored."
}
echo ""

# ── e-Transport XSD schema (v2) ──────────────────────────────────────────────
# The OFFICIAL XSD is published only on https://etransport.mfinante.gov.ro/informatii-tehnice,
# which blocks non-browser downloads (curl/wget get a connection reset). For reproducibility we
# pull the same schema (schema_ETR_v2.xsd, targetNamespace mfp:anaf:dgti:eTransport:declaratie:v2,
# version 1.02) from a public mirror that vendors it next to ANAF's official Schematron v2.0.2.
# Replace with the canonical MF file the moment it can be fetched headlessly (e.g. via a browser).
# Used by tests/etransport_xsd.rs (skips gracefully when absent).
ETRANSPORT_XSD_URL="https://raw.githubusercontent.com/stornoro/storno/main/backend/resources/etransport/schema_ETR_v2.xsd"
ETRANSPORT_XSD_PATH="$TOOLS_DIR/schema_ETR_v2.xsd"

echo "▶ Downloading e-Transport v2 XSD schema ..."
echo "  URL: $ETRANSPORT_XSD_URL"
curl -fL --progress-bar -o "$ETRANSPORT_XSD_PATH" "$ETRANSPORT_XSD_URL" || {
    echo "  WARNING: e-Transport XSD download failed; the etransport_xsd test will skip until vendored."
}
echo ""

# ── D207 XSD schema (v2) — informativă impozit reținut la sursă, beneficiari NEREZIDENȚI ─────────
# Official ANAF schema (d207_20025020.xsd, targetNamespace mfp:anaf:dgti:d207:declaratie:v2, version
# 1.02) — fetches headlessly from static.anaf.ro. D207 has NO DUKIntegrator jar, so this XSD is the
# authoritative validator. Used by tests/d207_xsd.rs (skips gracefully when absent).
D207_XSD_URL="https://static.anaf.ro/static/10/Anaf/Declaratii_R/AplicatiiDec/d207_20025020.xsd"
D207_XSD_PATH="$REPO_ROOT/src-tauri/tools/anaf/d207.xsd"
mkdir -p "$(dirname "$D207_XSD_PATH")"
echo "▶ Downloading D207 v2 XSD schema ..."
echo "  URL: $D207_XSD_URL"
curl -fL --progress-bar -o "$D207_XSD_PATH" "$D207_XSD_URL" || {
    echo "  WARNING: D207 XSD download failed; the d207_xsd test will skip until vendored."
}
echo ""

# ── D300 / D394 assistance programs ─────────────────────────────────────────
# ANAF distributes declaration-specific validators as part of the assistance
# program ZIPs. Reference pages (check for latest download links):
#   D394: https://static.anaf.ro/static/10/Anaf/Declaratii_R/394.html
#   D300: https://static.anaf.ro/static/10/Anaf/Declaratii_R/descarcare_declaratii.htm
#
# The DUKIntegrator jar above typically covers all three (D300/D394/D406).
# Uncomment the lines below if ANAF splits the validators into separate JARs.
#
# D394_XSD_URL="https://static.anaf.ro/static/10/Anaf/Declaratii_R/D394_v4.xsd"
# curl -fL --progress-bar -o "$TOOLS_DIR/D394_v4.xsd" "$D394_XSD_URL"
#
# D300_XSD_URL="https://static.anaf.ro/static/10/Anaf/Declaratii_R/D300_v12.xsd"
# curl -fL --progress-bar -o "$TOOLS_DIR/D300_v12.xsd" "$D300_XSD_URL"

# ── D112 validator (medical-leave / asiguratD emission gate) ─────────────────
# D112 is NOT covered by the generic DUKIntegrator.jar above — it ships a dedicated
# business-rule validator. Index: https://static.anaf.ro/static/10/Anaf/Declaratii_R/112.html
# Manifest (authoritative latest version, survives ANAF's ~biannual bumps):
#   https://static.anaf.ro/static/10/Anaf/update5/versiuni.xml   (lists D112_<ver>/D112Validator.jar)
# As of 2026-04: D112_209/D112Validator.jar (~12 MB, J26.0.3, schema v6 / 01-2026).
# Java-6 bytecode → runs on the bundled JRE 17 via the same `-v D112` harness as D300/D394/D406
# (run_java_validator in src-tauri/src/anaf_decl/validation.rs). REQUIRED for the asiguratD
# emission gate: build the XML, then `java -jar D112Validator.jar -v D112 <xml> <result>` must
# return "fără erori" before a clean medical-leave declaration can be claimed.
D112_VALIDATOR_URL="https://static.anaf.ro/static/10/Anaf/update5/D112_209/D112Validator.jar"
D112_VALIDATOR_PATH="$TOOLS_DIR/D112Validator.jar"

echo "▶ Downloading D112 validator (asiguratD/concedii gate) ..."
echo "  URL: $D112_VALIDATOR_URL"
curl -fL --progress-bar -o "$D112_VALIDATOR_PATH" "$D112_VALIDATOR_URL" || {
    echo "  WARNING: D112Validator.jar download failed — check the version in update5/versiuni.xml."
    echo "  The D112 asiguratD emission test stays skipped until the validator is vendored."
}
echo ""

# ── Done ─────────────────────────────────────────────────────────────────────
echo "══════════════════════════════════════════════════════"
echo "  Validators fetched. To enable the DUKIntegrator gate:"
echo ""
echo "  export EFACTURA_DUK_JAR=\"$TOOLS_DIR/DUKIntegrator.jar\""
echo ""
echo "  Then run: cargo test --test duk_validation"
echo "══════════════════════════════════════════════════════"
