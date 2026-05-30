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
pnpm build --silent
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

echo "▶ rust clippy (strict)"
cargo clippy --all-targets --all-features -- -D warnings
echo "  ✓ clippy OK"
echo ""

echo "══════════════════════════════════════"
echo "  ✓ Toate verificările au trecut!"
echo "══════════════════════════════════════"
