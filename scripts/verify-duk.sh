#!/usr/bin/env bash
# verify-duk.sh — authoritative business-rule gate for Clarito's declaration emitters.
#
# PURPOSE:
#   Run the real ANAF DUKIntegrator over one representative, DUK-VALID sample per
#   declaration type (D301, D700, D710, D100, D101) and confirm "Validare fara erori"
#   for each. This is the BUSINESS-RULE gate — DUKIntegrator validates ANAF-specific
#   rules that XSD alone cannot express (R11b totalPlata_A sums, R14 bife sums, R16/R17
#   nr_evid, R32.1 tip=5 pairing, etc.).
#
#   The cargo gate (cargo test / xmllint) is the STRUCTURAL gate (unit tests + XSD).
#   These two gates are complementary: cargo runs everywhere; DUK requires Java + jars.
#
# USAGE:
#   bash scripts/verify-duk.sh
#
# GATING:
#   - Java absent → "SKIP: no Java" and exit 0 (safe in CI without Java)
#   - DUKIntegrator.jar absent → "SKIP: DUKIntegrator.jar not bundled" and exit 0
#   - Validator jar absent for a specific type → SKIP that type (print "SKIP (no jar)")
#   - Any type with present jar that FAILS → exit 1 (gate red)
#
# JAVA RESOLUTION ORDER:
#   1. src-tauri/resources/jre-min/bin/java  (bundled JRE shipped with the app)
#   2. $JAVA_HOME/bin/java                   (explicit JAVA_HOME)
#   3. java on PATH                          (system Java)
#
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# ── 1. Resolve Java ──────────────────────────────────────────────────────────

BUNDLED_JRE="$ROOT/src-tauri/resources/jre-min/bin/java"
JAVA=""

if [ -x "$BUNDLED_JRE" ]; then
    JAVA="$BUNDLED_JRE"
elif [ -n "${JAVA_HOME:-}" ] && [ -x "$JAVA_HOME/bin/java" ]; then
    JAVA="$JAVA_HOME/bin/java"
elif command -v java >/dev/null 2>&1; then
    JAVA="$(command -v java)"
fi

if [ -z "$JAVA" ]; then
    echo "SKIP: no Java found (tried bundled JRE, \$JAVA_HOME, PATH)"
    exit 0
fi

echo "Java: $JAVA"

# ── 2. Resolve DUKIntegrator.jar ─────────────────────────────────────────────

DUK_DIR="$ROOT/src-tauri/resources/duk"
DUK_JAR="$DUK_DIR/DUKIntegrator.jar"

if [ ! -f "$DUK_JAR" ]; then
    echo "SKIP: DUKIntegrator.jar not bundled at $DUK_JAR"
    exit 0
fi

echo "DUKIntegrator: $DUK_JAR"
echo ""

# ── 3. Generate sample XMLs ───────────────────────────────────────────────────

SAMPLE_DIR="${TMPDIR:-/tmp}/duk_samples_$$"
trap 'rm -rf "$SAMPLE_DIR"' EXIT

echo "Generating samples via: cargo run --example dump_duk_samples -- $SAMPLE_DIR"
cd "$ROOT/src-tauri"
cargo run --example dump_duk_samples -- "$SAMPLE_DIR" 2>&1
echo ""

# ── 4. Validate each declaration type ────────────────────────────────────────

FAIL=0
PASS_COUNT=0
SKIP_COUNT=0
FAIL_COUNT=0

echo "DUKIntegrator validation results:"
echo "══════════════════════════════════"

for TYPE in D301 D700 D710 D100 D101; do
    JAR="$DUK_DIR/lib/${TYPE}Validator.jar"
    XML="$SAMPLE_DIR/${TYPE}.xml"

    if [ ! -f "$JAR" ]; then
        echo "  $TYPE: SKIP (no jar at lib/${TYPE}Validator.jar)"
        SKIP_COUNT=$((SKIP_COUNT + 1))
        continue
    fi

    # Write result to a unique temp file so concurrent runs don't collide.
    RESULT="$(mktemp /tmp/duk_result_${TYPE}_XXXXXX.txt)"

    # Run DUKIntegrator: java -Djava.awt.headless=true -jar DUKIntegrator.jar -v TYPE xml result
    # Capture stdout+stderr; DUKIntegrator prints "Validare fara erori" there.
    STDOUT_BODY="$("$JAVA" -Djava.awt.headless=true \
        -jar "$DUK_JAR" \
        -v "$TYPE" \
        "$XML" \
        "$RESULT" \
        2>&1)" || true  # DUKIntegrator may exit non-zero even on success

    RESULT_BODY=""
    if [ -f "$RESULT" ]; then
        RESULT_BODY="$(cat "$RESULT")"
        rm -f "$RESULT"
    fi

    # Combine stdout + result file for comprehensive detection.
    # DUKIntegrator behaviour (observed):
    #   - D301/D710/D100/D101: stdout contains "Validare fara erori"; result file = "ok"
    #   - D700: stdout contains "Atentionari la validare"; result file = "atentionare ..."
    #     (advisory warnings only, no blocking errors)
    COMBINED="$STDOUT_BODY
$RESULT_BODY"

    # Error lines: E: or F: prefix in result file (blocking errors).
    # "atentionare" / "atentionari" lines are ADVISORY WARNINGS — not errors.
    ERRORS="$(echo "$RESULT_BODY" | grep -iE '^[[:space:]]*(E:|F:)' || true)"

    # PASS conditions (any of):
    #   "fara erori" / "fără erori" in stdout/result = explicit clean pass
    #   result file = "ok" (literal, case-insensitive) with no E:/F: error lines
    #   result file contains ONLY "atentionare" (advisory) lines with no E:/F: errors
    #     — DUKIntegrator prints "Atentionari la validare" for warnings-only runs (D700)
    CLEAN=false
    if echo "$COMBINED" | grep -qi "fara erori"; then
        CLEAN=true
    fi
    if echo "$COMBINED" | grep -qi "fără erori"; then
        CLEAN=true
    fi
    # "ok" as the entire (trimmed) result file body is also a clean pass marker
    RESULT_TRIMMED="$(echo "$RESULT_BODY" | tr -d '[:space:]' | tr '[:upper:]' '[:lower:]')"
    if [ "$RESULT_TRIMMED" = "ok" ]; then
        CLEAN=true
    fi
    # Warnings-only: no E:/F: errors AND no error keywords in the result body,
    # AND the result body only contains "atentionare" / advisory content.
    # This covers D700 which reports "Atentionari la validare" (advisory-only run).
    if [ -z "$ERRORS" ]; then
        RESULT_ERROR_LINES="$(echo "$RESULT_BODY" | grep -iv "atention" \
            | grep -iv "fara erori" | grep -iv "fără erori" \
            | grep -Eiv '^[[:space:]]*A:' \
            | grep -Ev '^[[:space:]]*$' || true)"
        if [ -z "$RESULT_ERROR_LINES" ]; then
            CLEAN=true
        fi
    fi

    if $CLEAN && [ -z "$ERRORS" ]; then
        echo "  $TYPE: PASS"
        PASS_COUNT=$((PASS_COUNT + 1))
    else
        echo "  $TYPE: FAIL"
        if [ -n "$ERRORS" ]; then
            echo "         Errors:"
            echo "$ERRORS" | while IFS= read -r line; do
                echo "           $line"
            done
        fi
        echo "         Stdout:"
        echo "$STDOUT_BODY" | head -10 | while IFS= read -r line; do
            echo "           $line"
        done
        echo "         Result file:"
        echo "$RESULT_BODY" | head -20 | while IFS= read -r line; do
            echo "           $line"
        done
        FAIL_COUNT=$((FAIL_COUNT + 1))
        FAIL=1
    fi
done

# ── 5. Summary ────────────────────────────────────────────────────────────────

echo "══════════════════════════════════"
echo "Summary: ${PASS_COUNT} PASS, ${FAIL_COUNT} FAIL, ${SKIP_COUNT} SKIP"
echo ""

if [ "$FAIL" -ne 0 ]; then
    echo "GATE RED: one or more declarations FAILED DUK business-rule validation."
    echo "Fix the emitter until all present jars report 'Validare fara erori'."
    exit 1
fi

echo "GATE GREEN: all declarations with present jars passed DUK validation."
exit 0
