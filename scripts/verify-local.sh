#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo ""
echo "══════════════════════════════════════"
echo "  efactura-desktop — verificare locală"
echo "══════════════════════════════════════"
echo ""

echo "▶ git status"
git status --short --branch
echo ""

echo "▶ frontend typecheck (tsc)"
pnpm exec tsc --noEmit
echo "  ✓ TypeScript OK"
echo ""

echo "▶ frontend tests (vitest)"
pnpm test
echo ""

echo "▶ frontend build (vite)"
pnpm build
echo "  ✓ Build OK"
echo ""

cd "$ROOT/src-tauri"

echo "▶ rust fmt"
cargo fmt --check
echo "  ✓ fmt OK"
echo ""

echo "▶ rust check"
cargo check 2>&1 | tail -5
echo ""

echo "▶ rust tests"
cargo test --lib
echo ""

# Integration tests live in tests/ and are NOT run by `cargo test --lib`. The SAF-T D406 suite
# validates the generated declaration against the official Ro_SAFT XSD (xmllint) + the bundled DUK
# validator — both local-only (gitignored), so this gate runs here, not in CI.
echo "▶ saft d406 integration gate (XSD + DUK)"
cargo test --test saft_xsd
echo ""

echo "▶ rust clippy (strict)"
cargo clippy --all-targets --all-features -- -D warnings
echo "  ✓ clippy OK"
echo ""

echo "══════════════════════════════════════"
echo "  ✓ Toate verificările au trecut!"
echo "══════════════════════════════════════"
