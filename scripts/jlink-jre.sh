#!/usr/bin/env bash
# Produce a minimal JRE for DUKIntegrator into ./src-tauri/resources/jre-min
# Usage: JAVA_HOME=<jdk> scripts/jlink-jre.sh
#
# Module set derived via:
#   jdeps --multi-release 17 --ignore-missing-deps --list-deps \
#         DUKIntegrator.jar lib/*.jar
# Result: java.base, java.datatransfer (via java.desktop), java.desktop,
#         java.logging, java.naming, java.sql, java.xml
# Added: java.management (logging infra), jdk.crypto.ec (ECDSA/TLS for ANAF endpoints)
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/src-tauri/resources/jre-min"
MODULES="java.base,java.desktop,java.logging,java.xml,java.naming,java.management,java.sql,jdk.crypto.ec"
rm -rf "$OUT"
"${JAVA_HOME}/bin/jlink" --add-modules "$MODULES" \
  --strip-debug --no-man-pages --no-header-files --compress=2 --output "$OUT"
# jlink creates read-only legal/ files (444) — make them user-writable so that
# tauri_build::build() can re-copy them on subsequent `cargo check/build` runs
# without hitting "Permission denied (os error 13)".
chmod -R u+w "$OUT"
# Windows jlink emits bin/java.exe; macOS/Linux emit bin/java. Pick whichever exists
# so the smoke check works on every runner (under set -e a missing binary still fails).
if [ -x "$OUT/bin/java.exe" ]; then
  "$OUT/bin/java.exe" -version
else
  "$OUT/bin/java" -version
fi
