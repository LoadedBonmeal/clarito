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

# ── D301 XSD schema ──────────────────────────────────────────────────────────
# Official ANAF XSD (d301_20200130.xsd, targetNamespace mfp:anaf:dgti:d301:declaratie:v1,
# version 1.02). Used by tests/d301_xsd.rs (skips gracefully when absent).
# Full business-rule validation requires D301Validator.jar from D301_20201022.zip:
#   https://static.anaf.ro/static/10/Anaf/Declaratii_R/AplicatiiDec/D301_20201022.zip
# Run via DUKIntegrator: java -jar DUKIntegrator.jar -v D301 <xml> <result>
D301_XSD_URL="https://static.anaf.ro/static/10/Anaf/Declaratii_R/AplicatiiDec/d301_20200130.xsd"
D301_XSD_PATH="$REPO_ROOT/src-tauri/tools/anaf/d301.xsd"
mkdir -p "$(dirname "$D301_XSD_PATH")"
echo "▶ Downloading D301 v1 XSD schema ..."
echo "  URL: $D301_XSD_URL"
curl -fL --progress-bar -o "$D301_XSD_PATH" "$D301_XSD_URL" || {
    echo "  WARNING: D301 XSD download failed; the d301_xsd test will skip until vendored."
}
echo ""

# ── D710 XSD schema ──────────────────────────────────────────────────────────
# Official ANAF XSD (d710_20012025.xsd). NOTE: the published XSD has a typo —
# it declares `targetNamespace=v1` but DUKIntegrator (`-v D710`) requires documents
# to use namespace `v2`. We patch the vendored XSD to use v2 everywhere so that
# xmllint validates v2 documents correctly.
# Used by tests/d710_xsd.rs (skips gracefully when absent).
# Full business-rule validation requires D710Validator.jar via DUKIntegrator:
#   https://static.anaf.ro/static/10/Anaf/Declaratii_R/AplicatiiDec/D710_20052026.zip
# Run via DUKIntegrator: java -jar DUKIntegrator.jar -v D710 <xml> <result>
# D700 full business-rule validation requires D700Validator.jar from D700_20260423.zip:
#   https://static.anaf.ro/static/10/Anaf/Declaratii_R/AplicatiiDec/D700_20260423.zip
# Run via DUKIntegrator: java -jar DUKIntegrator.jar -v D700 <xml> <result>
D710_XSD_URL="https://static.anaf.ro/static/10/Anaf/Declaratii_R/AplicatiiDec/d710_20012025.xsd"
D710_XSD_PATH="$REPO_ROOT/src-tauri/tools/anaf/d710.xsd"
mkdir -p "$(dirname "$D710_XSD_PATH")"
echo "▶ Downloading D710 v1 XSD schema ..."
echo "  URL: $D710_XSD_URL"
curl -fL --progress-bar -o "$D710_XSD_PATH" "$D710_XSD_URL" || {
    echo "  WARNING: D710 XSD download failed; the d710_xsd test will skip until vendored."
}
# Patch 1: targetNamespace AND xmlns from v1 to v2 (DUK requires v2 namespace in documents;
# the vendored XSD must match so xmllint validates v2 documents correctly).
# Patch 2: attribute names suma_dat_i/c → suma_dat_I/C (DUK v2 requires uppercase I/C suffixes;
# the published XSD uses lowercase which is the v1 convention).
if [ -f "$D710_XSD_PATH" ]; then
    sed -i '' 's|targetNamespace="mfp:anaf:dgti:d710:declaratie:v1"|targetNamespace="mfp:anaf:dgti:d710:declaratie:v2"|g' "$D710_XSD_PATH" 2>/dev/null || \
    sed -i 's|targetNamespace="mfp:anaf:dgti:d710:declaratie:v1"|targetNamespace="mfp:anaf:dgti:d710:declaratie:v2"|g' "$D710_XSD_PATH" 2>/dev/null || true
    sed -i '' 's|xmlns="mfp:anaf:dgti:d710:declaratie:v1"|xmlns="mfp:anaf:dgti:d710:declaratie:v2"|g' "$D710_XSD_PATH" 2>/dev/null || \
    sed -i 's|xmlns="mfp:anaf:dgti:d710:declaratie:v1"|xmlns="mfp:anaf:dgti:d710:declaratie:v2"|g' "$D710_XSD_PATH" 2>/dev/null || true
    # Patch attribute name case: v1 has lowercase _i/_c; DUK v2 requires uppercase _I/_C.
    for pair in "suma_dat_i:suma_dat_I" "suma_dat_c:suma_dat_C" \
                "suma_ded_i:suma_ded_I" "suma_ded_c:suma_ded_C" \
                "suma_plata_i:suma_plata_I" "suma_plata_c:suma_plata_C" \
                "suma_rest_i:suma_rest_I" "suma_rest_c:suma_rest_C"; do
        old="${pair%%:*}"
        new="${pair##*:}"
        sed -i '' "s|name=\"$old\"|name=\"$new\"|g" "$D710_XSD_PATH" 2>/dev/null || \
        sed -i "s|name=\"$old\"|name=\"$new\"|g" "$D710_XSD_PATH" 2>/dev/null || true
    done
    echo "  Patched D710 XSD: namespace v1→v2 + attribute names lowercase→uppercase I/C (DUK v2 requirement)."
fi
echo ""

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

# ── D301 validator (DUKIntegrator overlay) ───────────────────────────────────
# D301Validator.jar is shipped inside D301_20201022.zip (ANAF AplicatiiDec index).
# Invoked via DUKIntegrator overlay: java -jar DUKIntegrator.jar -v D301 <xml> <result>
# The extracted jar is placed in dukintegrator/lib/ so the bundled runtime can find it
# at resources/duk/lib/D301Validator.jar (Tauri resource layout).
# Official ZIP: https://static.anaf.ro/static/10/Anaf/Declaratii_R/AplicatiiDec/D301_20201022.zip
# NOTE: the SHIPPED jars come from the release duk.zip secret (consistent with D300/D112 jars);
# this script is for DEV/CI use only (src-tauri/tools/ and resources/duk are gitignored).
D301_LIB_DIR="$TOOLS_DIR/lib"
mkdir -p "$D301_LIB_DIR"
D301_ZIP_URL="https://static.anaf.ro/static/10/Anaf/Declaratii_R/AplicatiiDec/D301_20201022.zip"
D301_ZIP_PATH="$TOOLS_DIR/D301_20201022.zip"
echo "▶ Downloading D301 validator package ..."
echo "  URL: $D301_ZIP_URL"
curl -fL --progress-bar -o "$D301_ZIP_PATH" "$D301_ZIP_URL" || {
    echo "  WARNING: D301_20201022.zip download failed — D301 DUK gate will skip gracefully without jar."
}
if [ -f "$D301_ZIP_PATH" ]; then
    # Extract only D301Validator.jar (may be at root or in a subdirectory).
    unzip -p "$D301_ZIP_PATH" "D301Validator.jar" > "$D301_LIB_DIR/D301Validator.jar" 2>/dev/null || \
    unzip -p "$D301_ZIP_PATH" "*/D301Validator.jar" > "$D301_LIB_DIR/D301Validator.jar" 2>/dev/null || \
    unzip -j "$D301_ZIP_PATH" "*D301Validator.jar" -d "$D301_LIB_DIR" 2>/dev/null || \
    echo "  WARNING: Could not extract D301Validator.jar from zip — extract manually."
    rm -f "$D301_ZIP_PATH"
fi
echo ""

# ── D700 validator (DUKIntegrator overlay) ───────────────────────────────────
# D700Validator.jar is shipped inside D700_20260423.zip (ANAF AplicatiiDec index).
# Invoked via DUKIntegrator overlay: java -jar DUKIntegrator.jar -v D700 <xml> <result>
# Official ZIP: https://static.anaf.ro/static/10/Anaf/Declaratii_R/AplicatiiDec/D700_20260423.zip
D700_ZIP_URL="https://static.anaf.ro/static/10/Anaf/Declaratii_R/AplicatiiDec/D700_20260423.zip"
D700_ZIP_PATH="$TOOLS_DIR/D700_20260423.zip"
echo "▶ Downloading D700 validator package ..."
echo "  URL: $D700_ZIP_URL"
curl -fL --progress-bar -o "$D700_ZIP_PATH" "$D700_ZIP_URL" || {
    echo "  WARNING: D700_20260423.zip download failed — D700 DUK gate will skip gracefully without jar."
}
if [ -f "$D700_ZIP_PATH" ]; then
    unzip -p "$D700_ZIP_PATH" "D700Validator.jar" > "$D301_LIB_DIR/D700Validator.jar" 2>/dev/null || \
    unzip -p "$D700_ZIP_PATH" "*/D700Validator.jar" > "$D301_LIB_DIR/D700Validator.jar" 2>/dev/null || \
    unzip -j "$D700_ZIP_PATH" "*D700Validator.jar" -d "$D301_LIB_DIR" 2>/dev/null || \
    echo "  WARNING: Could not extract D700Validator.jar from zip — extract manually."
    rm -f "$D700_ZIP_PATH"
fi
echo ""

# ── D710 validator (STANDALONE — NOT through DUKIntegrator) ──────────────────
# D710Validator.jar is shipped inside D710_20052026.zip (ANAF AplicatiiDec index).
# STANDALONE invocation: java -jar D710Validator.jar <xml>  (no -v token, no result-file)
# Output goes to STDOUT; same parse_duk_output markers as DUKIntegrator.
# Official ZIP: https://static.anaf.ro/static/10/Anaf/Declaratii_R/AplicatiiDec/D710_20052026.zip
D710_ZIP_URL="https://static.anaf.ro/static/10/Anaf/Declaratii_R/AplicatiiDec/D710_20052026.zip"
D710_ZIP_PATH="$TOOLS_DIR/D710_20052026.zip"
echo "▶ Downloading D710 validator package (STANDALONE) ..."
echo "  URL: $D710_ZIP_URL"
curl -fL --progress-bar -o "$D710_ZIP_PATH" "$D710_ZIP_URL" || {
    echo "  WARNING: D710_20052026.zip download failed — D710 DUK gate will skip gracefully without jar."
}
if [ -f "$D710_ZIP_PATH" ]; then
    unzip -p "$D710_ZIP_PATH" "D710Validator.jar" > "$D301_LIB_DIR/D710Validator.jar" 2>/dev/null || \
    unzip -p "$D710_ZIP_PATH" "*/D710Validator.jar" > "$D301_LIB_DIR/D710Validator.jar" 2>/dev/null || \
    unzip -j "$D710_ZIP_PATH" "*D710Validator.jar" -d "$D301_LIB_DIR" 2>/dev/null || \
    echo "  WARNING: Could not extract D710Validator.jar from zip — extract manually."
    rm -f "$D710_ZIP_PATH"
fi
echo ""

# ── Done ─────────────────────────────────────────────────────────────────────
echo "══════════════════════════════════════════════════════"
echo "  Validators fetched. To enable the DUKIntegrator gate:"
echo ""
echo "  export EFACTURA_DUK_JAR=\"$TOOLS_DIR/DUKIntegrator.jar\""
echo ""
echo "  Then run: cargo test --test duk_validation"
echo "══════════════════════════════════════════════════════"
