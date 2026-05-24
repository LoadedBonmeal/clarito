# RoFactura — Security Audit, Feature Completeness & Build Setup

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close all security exploits, verify full feature completeness, update UI to latest design, optimize for macOS + Windows, and ship production-ready installers for both platforms.

**Architecture:** Tauri 2.0 desktop app — Rust backend (SQLite/sqlx, keyring, sha2) + React 19/TypeScript frontend. License system uses OS keychain + SHA-256 integrity fingerprints + anti-clock-rollback. All monetary math uses `rust_decimal::Decimal`. Custom CSS design system in `src/styles/design.css`.

**Tech Stack:** Tauri 2.0, Rust 1.82+, React 19, TypeScript, sqlx 0.8, printpdf 0.7, Liberation Sans fonts, TanStack Query/Router/Virtual, GitHub Actions

**Critical rule: AppState field is `.db` (NOT `.pool`). Money math ONLY with `rust_decimal::Decimal`. ANAF tokens NEVER logged.**

---

## Audit Findings Summary

| # | Issue | Severity | Task |
|---|-------|----------|------|
| S1 | `activate_license` accepts **any** key — zero validation | 🔴 CRITICAL exploit | T1 |
| S2 | Paid (SOLO) licenses skip SHA-256 integrity fingerprint | 🔴 CRITICAL exploit | T1 |
| S3 | `check_license_validity` skips machine_id binding for non-TRIAL | 🔴 HIGH exploit | T1 |
| S4 | CSP is `null` — webview allows any external script/resource | 🔴 HIGH | T2 |
| S5 | Dev seed left commented out after onboarding demo | 🟠 MEDIUM | T3 |
| S6 | No GitHub Actions CI — no automated macOS+Windows builds | 🟠 HIGH | T4 |
| S7 | No code signing config — updater pubkey empty, no entitlements | 🟠 HIGH | T5 |
| S8 | No macOS DMG customization or Hardened Runtime entitlements | 🟡 MEDIUM | T5 |
| S9 | No Windows NSIS branding / install path config | 🟡 MEDIUM | T6 |
| F1 | Feature completeness: all 8 gap-fill tasks present — verify wiring | ✅ verify | T3 |
| U1 | UI: re-enable dev seed, verify onboarding wizard end-to-end | 🟡 MEDIUM | T3 |

---

## File Map

| File | Action | Task |
|------|--------|------|
| `src-tauri/src/commands/license.rs` | Modify — add HMAC key validation + fingerprint for SOLO | T1 |
| `src-tauri/src/commands/license.rs` | Modify — check machine_id for all tiers | T1 |
| `src-tauri/src/lib.rs` | Modify — restore dev seed + add CSP | T2+T3 |
| `src-tauri/tauri.conf.json` | Modify — add CSP policy | T2 |
| `src-tauri/capabilities/default.json` | Modify — tighten permissions | T2 |
| `src-tauri/entitlements.plist` | Create — macOS Hardened Runtime | T5 |
| `src-tauri/tauri.conf.json` | Modify — macOS signing + DMG + Windows NSIS | T5+T6 |
| `.github/workflows/build.yml` | Create — CI for macOS universal + Windows x64 | T4 |
| `.github/workflows/release.yml` | Create — release workflow with artifacts | T4 |

---

## Task 1: Fix License Security Exploits (CRITICAL)

**Files:**
- Modify: `src-tauri/src/commands/license.rs`
- Modify: `src-tauri/src/db/license.rs` (add fingerprint for SOLO)

### What's broken

`activate_license` accepts any string as a key and immediately issues a full 1-year SOLO license. A user can type `XXXX-XXXX-XXXX-XXXX` and bypass the paywall. Additionally, SOLO licenses have no integrity fingerprint — a user can extend `expires_at` in SQLite and the validity check won't notice.

### Fix plan

Add three layers:
1. **Key format validation** — keys must match `[A-Z0-9]{4}-[A-Z0-9]{4}-[A-Z0-9]{4}-[A-Z0-9]{4}`
2. **HMAC-SHA256 checksum embedded in key** — last 8 chars are hex(HMAC_SHA256(first 12 chars, SECRET)[0..4]). This is offline-verifiable without a server and means random keys fail ~99.998% of the time.
3. **Fingerprint for SOLO licenses too** — same SHA-256 integrity fingerprint applied after activation

- [ ] **Step 1.1 — Add HMAC key validation to `license.rs`**

Open `src-tauri/src/commands/license.rs` and add these constants and functions BEFORE the `activate_license` command:

```rust
// ─── License Key Validation ─────────────────────────────────────────────────

/// Format: XXXX-XXXX-XXXX-XXXX (A-Z, 0-9 only, 16 data chars + 3 dashes)
/// Last 4 chars (segment 4) = first 4 hex chars of HMAC-SHA256(segments1-3, KEY_SECRET).
/// This allows offline key validation without a server.
const KEY_SECRET: &[u8] = b"RoF@ctura#Key!HMAC2026\xb2\x7f\xd4\x91\xc3\x0a";

/// Returns `true` if the key format is valid AND the HMAC checksum passes.
fn validate_license_key(key: &str) -> bool {
    // Format check: 4 groups of 4 uppercase alphanumeric chars separated by '-'
    let parts: Vec<&str> = key.split('-').collect();
    if parts.len() != 4 {
        return false;
    }
    for part in &parts {
        if part.len() != 4 || !part.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) {
            return false;
        }
    }

    // HMAC checksum: compute HMAC-SHA256 of "XXXX-XXXX-XXXX" (first 3 segments)
    // Expected checksum = first 4 hex chars of HMAC output
    let payload = format!("{}-{}-{}", parts[0], parts[1], parts[2]);
    let expected_check = hmac_key_checksum(payload.as_bytes());

    // parts[3] must equal the first 4 chars of the hex-encoded checksum
    parts[3].eq_ignore_ascii_case(&expected_check[..4])
}

/// Computes first 4 hex chars of HMAC-SHA256(data, KEY_SECRET).
fn hmac_key_checksum(data: &[u8]) -> String {
    use sha2::Sha256;
    use sha2::digest::Mac;

    // Use SHA-256 as a simple keyed hash (HMAC-SHA256 proper would need hmac crate).
    // We use SHA-256(KEY_SECRET || 0x00 || data) as a simplified HMAC for embedded validation.
    let mut h = Sha256::new();
    h.update(KEY_SECRET);
    h.update(b"\x00");
    h.update(data);
    let result = h.finalize();
    format!("{:x}", result)
}
```

- [ ] **Step 1.2 — Update `activate_license` to validate key + fingerprint**

Replace the existing `activate_license` function body:

```rust
#[tauri::command]
pub async fn activate_license(
    state: State<'_, AppState>,
    key: String,
    email: String,
) -> AppResult<License> {
    let pool = &state.db;
    let key_upper = key.trim().to_uppercase();

    // 1. Validate key format + HMAC checksum (offline — no server needed)
    if !validate_license_key(&key_upper) {
        return Err(AppError::Validation(
            "Cheia de licență este invalidă. Verificați că ați introdus corect \
             cheia primită prin email (format: XXXX-XXXX-XXXX-XXXX)."
                .into(),
        ));
    }

    // 2. Validate email
    if email.trim().is_empty() || !email.contains('@') {
        return Err(AppError::Validation(
            "Adresa de email este obligatorie pentru activarea licenței.".into(),
        ));
    }

    let mid = machine_id();
    let one_year = chrono::Utc::now().timestamp() + 365 * 86_400;

    let lic = license::activate(pool, &key_upper, "SOLO", one_year, &email.trim(), &mid).await?;

    // 3. Apply integrity fingerprint to SOLO license (same as TRIAL)
    //    Prevents manual extension of expires_at in SQLite
    let fp = compute_fingerprint(&email.trim().to_lowercase(), &mid, lic.expires_at, "SOLO");
    set_setting(pool, FP_SETTINGS_KEY, &fp).await;

    // 4. Update last_seen (anti-rollback)
    let now = chrono::Utc::now().timestamp();
    set_setting(pool, LAST_SEEN_KEY, &now.to_string()).await;

    Ok(lic)
}
```

- [ ] **Step 1.3 — Update `check_license_validity` to apply fingerprint to SOLO too**

Find the section that starts `if tier == "TRIAL" {` and expand it to cover SOLO as well:

```rust
    // ── 2. Fingerprint integritate (TRIAL și SOLO) ────────────────────────
    // Licențele plătite validate printr-un fingerprint local; validare cloud în viitor.
    if tier == "TRIAL" || tier == "SOLO" {
        let mid = machine_id();

        // machine_id binding — previne mutarea DB-ului pe altă mașină
        if !stored_mid.is_empty() && stored_mid != mid {
            return Ok(false);
        }

        let email_for_fp = email.to_lowercase();
        let expected_fp = compute_fingerprint(&email_for_fp, &mid, expires_at, &tier);
        let stored_fp = get_setting(pool, FP_SETTINGS_KEY).await;

        match stored_fp {
            Some(fp) if fp == expected_fp => {} // fingerprint OK
            _ => return Ok(false),              // lipsă sau alterat
        }
    }
```

- [ ] **Step 1.4 — Run `cargo check` — verify zero errors**

```bash
cd /Users/cris/Projects/efactura-desktop/src-tauri && cargo check 2>&1 | grep "^error"
```

Expected: no output (no errors).

- [ ] **Step 1.5 — Test: try invalid key in LicenseExpiredScreen or OnboardingWizard**

In the running app, enter `AAAA-BBBB-CCCC-DDDD` → should see:
"Cheia de licență este invalidă. Verificați că ați introdus corect cheia..."

- [ ] **Step 1.6 — Commit**

```bash
cd /Users/cris/Projects/efactura-desktop
git add src-tauri/src/commands/license.rs
git commit -m "security: add HMAC key validation + fingerprint for SOLO licenses

- validate_license_key(): checks format XXXX-XXXX-XXXX-XXXX + HMAC checksum
- Random/guessed keys rejected offline (1/65536 false-positive rate)
- SOLO licenses now get SHA-256 integrity fingerprint same as TRIAL
- machine_id binding extended to SOLO tier

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

---

## Task 2: Content Security Policy

**Files:**
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/capabilities/default.json`

### Why this matters

CSP `null` means the Tauri webview allows any inline script, any external resource. If any part of the app renders user-controlled HTML (invoice notes, contact names), XSS could exfiltrate ANAF tokens.

- [ ] **Step 2.1 — Add CSP to `tauri.conf.json`**

Replace:
```json
"security": {
  "csp": null
}
```

With:
```json
"security": {
  "csp": "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: asset: https://asset.localhost; connect-src ipc: http://ipc.localhost https://webservicesp.anaf.ro https://api.anaf.ro; font-src 'self' data:; object-src 'none'; base-uri 'none'; frame-src 'none'"
}
```

Explanation:
- `script-src 'self'` — blocks inline scripts, only bundled JS
- `style-src 'unsafe-inline'` — needed for React inline styles (the design system uses them extensively)
- `connect-src` — allows Tauri IPC + ANAF API URLs only
- `img-src data:` — allows base64 data URIs for icons
- `object-src 'none'` — blocks plugins

- [ ] **Step 2.2 — Verify app still loads after CSP change**

Run `npm run tauri dev` and open DevTools console → no CSP violation errors should appear during normal navigation.

- [ ] **Step 2.3 — Commit**

```bash
git add src-tauri/tauri.conf.json
git commit -m "security: add Content Security Policy to Tauri webview

Blocks external scripts, limits connect-src to Tauri IPC + ANAF URLs.
Prevents XSS exfiltration of ANAF tokens from user-controlled content.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

---

## Task 3: Restore Dev Seed + Feature Completeness Verification

**Files:**
- Modify: `src-tauri/src/lib.rs` (restore seed)
- Verify only (no code changes): all gap-fill features

### What to restore

The dev seed was commented out during the onboarding demo. It must be restored so developers can work with realistic data.

- [ ] **Step 3.1 — Restore dev seed in `lib.rs`**

Find (around line 69):
```rust
                        // #[cfg(debug_assertions)]
                        // if let Err(err) = db::seed::run_if_empty(&pool).await {
                        //     tracing::warn!(?err, "Seed failed");
                        // }
```

Replace with:
```rust
                        #[cfg(debug_assertions)]
                        if let Err(err) = db::seed::run_if_empty(&pool).await {
                            tracing::warn!(?err, "Seed failed");
                        }
```

- [ ] **Step 3.2 — Restore original DB backup**

```bash
DB_DIR="$HOME/Library/Application Support/com.lucaris.efactura"
if [ -f "$DB_DIR/data.db.BACKUP" ]; then
  cp "$DB_DIR/data.db.BACKUP" "$DB_DIR/data.db"
  echo "DB restored from backup"
else
  echo "No backup found — will use fresh DB with seed data on next start"
fi
```

- [ ] **Step 3.3 — Verify all Gap-Fill features work**

Run `npm run tauri dev` and manually test each:

```
□ Create invoice draft → Validate → 50+ BR-RO rules fire on errors
□ Set due_date before issue_date → BR-RO-024 error shown
□ Invoice PDF generation → Romanian diacritics (ă â î ș ț) render correctly
□ Payment method selector (30/10/48/42/58) visible in invoice edit
□ Settings → Pornire automată toggle works (sets LaunchAgent on macOS)
□ Settings → Verifică actualizări button present
□ Invoices list with seed data → virtual scroll smooth (no jank)
□ CSV import → template download + preview (dry run) before commit
□ Notification preferences per-type (os/inapp/off) respected
```

- [ ] **Step 3.4 — Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "fix: restore dev seed for development workflow

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

---

## Task 4: GitHub Actions CI — macOS + Windows Automated Builds

**Files:**
- Create: `.github/workflows/build.yml`
- Create: `.github/workflows/release.yml`

### Why

No CI means every release is hand-built, prone to inconsistency. The workflows build unsigned debug+release artifacts for PRs, and signed release artifacts on version tags.

- [ ] **Step 4.1 — Create CI workflow for PRs**

Create `.github/workflows/build.yml`:

```yaml
name: Build

on:
  push:
    branches: [main, develop]
  pull_request:
    branches: [main]

jobs:
  build-macos:
    name: Build macOS (universal)
    runs-on: macos-14
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust targets
        run: |
          rustup target add aarch64-apple-darwin
          rustup target add x86_64-apple-darwin

      - name: Install pnpm
        uses: pnpm/action-setup@v4
        with:
          version: 9

      - name: Install Node dependencies
        run: pnpm install

      - name: Build Tauri (universal macOS)
        run: pnpm build:mac
        env:
          TAURI_SIGNING_PRIVATE_KEY: ""  # Skip signing for CI builds

      - name: Upload macOS artifact
        uses: actions/upload-artifact@v4
        with:
          name: RoFactura-macOS-universal
          path: src-tauri/target/universal-apple-darwin/release/bundle/dmg/*.dmg

  build-windows:
    name: Build Windows (x64)
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install pnpm
        uses: pnpm/action-setup@v4
        with:
          version: 9

      - name: Install Node dependencies
        run: pnpm install

      - name: Build Tauri (Windows x64)
        run: pnpm build:win-x64

      - name: Upload Windows artifacts
        uses: actions/upload-artifact@v4
        with:
          name: RoFactura-Windows-x64
          path: |
            src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/*.exe
            src-tauri/target/x86_64-pc-windows-msvc/release/bundle/msi/*.msi
```

- [ ] **Step 4.2 — Create release workflow (signed, on version tags)**

Create `.github/workflows/release.yml`:

```yaml
name: Release

on:
  push:
    tags:
      - 'v*.*.*'

permissions:
  contents: write

jobs:
  release-macos:
    name: Release macOS (universal, signed)
    runs-on: macos-14
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust targets
        run: |
          rustup target add aarch64-apple-darwin
          rustup target add x86_64-apple-darwin

      - name: Install pnpm
        uses: pnpm/action-setup@v4
        with:
          version: 9

      - name: Install Node dependencies
        run: pnpm install

      - name: Import Apple certificate
        if: env.APPLE_CERTIFICATE != ''
        env:
          APPLE_CERTIFICATE: ${{ secrets.APPLE_CERTIFICATE }}
          APPLE_CERTIFICATE_PASSWORD: ${{ secrets.APPLE_CERTIFICATE_PASSWORD }}
          KEYCHAIN_PASSWORD: ${{ secrets.KEYCHAIN_PASSWORD }}
        run: |
          CERTIFICATE_PATH=$RUNNER_TEMP/build_certificate.p12
          KEYCHAIN_PATH=$RUNNER_TEMP/app-signing.keychain-db
          echo -n "$APPLE_CERTIFICATE" | base64 --decode -o $CERTIFICATE_PATH
          security create-keychain -p "$KEYCHAIN_PASSWORD" $KEYCHAIN_PATH
          security set-keychain-settings -lut 21600 $KEYCHAIN_PATH
          security unlock-keychain -p "$KEYCHAIN_PASSWORD" $KEYCHAIN_PATH
          security import $CERTIFICATE_PATH -P "$APPLE_CERTIFICATE_PASSWORD" -A \
            -t cert -f pkcs12 -k $KEYCHAIN_PATH
          security list-keychain -d user -s $KEYCHAIN_PATH

      - name: Build & sign Tauri (universal macOS)
        run: pnpm build:mac
        env:
          APPLE_SIGNING_IDENTITY: ${{ secrets.APPLE_SIGNING_IDENTITY }}
          APPLE_ID: ${{ secrets.APPLE_ID }}
          APPLE_PASSWORD: ${{ secrets.APPLE_PASSWORD }}
          APPLE_TEAM_ID: ${{ secrets.APPLE_TEAM_ID }}
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_UPDATER_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_UPDATER_PRIVATE_KEY_PASSWORD }}

      - name: Upload macOS release
        uses: softprops/action-gh-release@v2
        with:
          files: |
            src-tauri/target/universal-apple-darwin/release/bundle/dmg/*.dmg
          draft: true

  release-windows:
    name: Release Windows (x64, signed)
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install pnpm
        uses: pnpm/action-setup@v4
        with:
          version: 9

      - name: Install Node dependencies
        run: pnpm install

      - name: Build Tauri (Windows x64)
        run: pnpm build:win-x64
        env:
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_UPDATER_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_UPDATER_PRIVATE_KEY_PASSWORD }}

      - name: Upload Windows release
        uses: softprops/action-gh-release@v2
        with:
          files: |
            src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/*.exe
            src-tauri/target/x86_64-pc-windows-msvc/release/bundle/msi/*.msi
          draft: true
```

- [ ] **Step 4.3 — Add GitHub secrets documentation**

Create `.github/SECRETS.md`:

```markdown
# Required GitHub Secrets

## macOS Code Signing (optional but recommended for distribution)
- `APPLE_CERTIFICATE` — base64-encoded .p12 certificate (Developer ID Application)
- `APPLE_CERTIFICATE_PASSWORD` — certificate password
- `KEYCHAIN_PASSWORD` — temporary keychain password (any random string)
- `APPLE_SIGNING_IDENTITY` — e.g. "Developer ID Application: Lucaris SRL (TEAMID)"
- `APPLE_ID` — your Apple ID email for notarization
- `APPLE_PASSWORD` — app-specific password from appleid.apple.com
- `APPLE_TEAM_ID` — 10-char team ID from developer.apple.com

## Tauri Updater (required for auto-updates)
Generate with: `npx tauri signer generate -w ~/.tauri/rofactura.key`
- `TAURI_UPDATER_PRIVATE_KEY` — the generated private key content
- `TAURI_UPDATER_PRIVATE_KEY_PASSWORD` — the key password
Then put the PUBLIC key in `tauri.conf.json` → `plugins.updater.pubkey`
```

- [ ] **Step 4.4 — Commit**

```bash
mkdir -p .github/workflows
git add .github/workflows/build.yml .github/workflows/release.yml .github/SECRETS.md
git commit -m "ci: add GitHub Actions for macOS universal + Windows x64 builds

- build.yml: CI on push/PR (unsigned, artifact upload)
- release.yml: signed release on version tags (draft release)
- SECRETS.md: documents required GitHub secrets for signing

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

---

## Task 5: macOS Build Configuration (Hardened Runtime + DMG)

**Files:**
- Create: `src-tauri/entitlements.plist`
- Create: `src-tauri/Info.plist`
- Modify: `src-tauri/tauri.conf.json` (macOS bundle section)

- [ ] **Step 5.1 — Create macOS entitlements for Hardened Runtime**

Create `src-tauri/entitlements.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <!-- Hardened Runtime entitlements for RoFactura -->

  <!-- Required for JIT-compiled code (V8 in Tauri webview) -->
  <key>com.apple.security.cs.allow-jit</key>
  <true/>

  <!-- Required for Tauri's webview rendering -->
  <key>com.apple.security.cs.allow-unsigned-executable-memory</key>
  <true/>

  <!-- Keychain access for ANAF tokens and license -->
  <key>keychain-access-groups</key>
  <array>
    <string>$(AppIdentifierPrefix)ro.lucaris.efactura</string>
  </array>

  <!-- Network access for ANAF API calls -->
  <key>com.apple.security.network.client</key>
  <true/>

  <!-- File access for DB and archive -->
  <key>com.apple.security.files.user-selected.read-write</key>
  <true/>

  <!-- Notifications -->
  <key>com.apple.security.app-sandbox</key>
  <false/>
</dict>
</plist>
```

- [ ] **Step 5.2 — Add macOS bundle config to `tauri.conf.json`**

Add a `macOS` section inside `bundle`:

```json
"bundle": {
  "active": true,
  "targets": ["dmg", "msi", "nsis"],
  "icon": [
    "icons/32x32.png",
    "icons/128x128.png",
    "icons/128x128@2x.png",
    "icons/icon.icns",
    "icons/icon.ico"
  ],
  "category": "Business",
  "shortDescription": "Aplicație desktop pentru e-Factura ANAF",
  "longDescription": "RoFactura — Aplicație desktop pentru gestionarea facturilor electronice prin sistemul ANAF SPV. Multi-companie, validare RO_CIUS, arhivare locală.",
  "macOS": {
    "entitlements": "entitlements.plist",
    "exceptionDomain": "webservicesp.anaf.ro",
    "minimumSystemVersion": "12.0",
    "providerShortName": null,
    "signingIdentity": null
  },
  "windows": {
    "digestAlgorithm": "sha256",
    "timestampUrl": "http://timestamp.digicert.com",
    "tsp": false
  }
}
```

- [ ] **Step 5.3 — Test local macOS build (arm64 only for speed)**

```bash
cd /Users/cris/Projects/efactura-desktop
pnpm build:mac-arm
```

Expected: builds successfully to `src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/RoFactura_0.1.0_aarch64.dmg`

- [ ] **Step 5.4 — Commit**

```bash
git add src-tauri/entitlements.plist src-tauri/tauri.conf.json
git commit -m "build: add macOS Hardened Runtime entitlements + bundle config

- entitlements.plist: JIT, webview, keychain, network, file access
- macOS minimum version: 12.0 (Monterey)
- Windows: SHA-256 digest + DigiCert timestamp

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

---

## Task 6: Windows Installer Configuration (NSIS + MSI)

**Files:**
- Modify: `src-tauri/tauri.conf.json` (NSIS section)
- Create: `src-tauri/nsis/installer.nsh` (NSIS hook for Windows-specific setup)

- [ ] **Step 6.1 — Add NSIS configuration to `tauri.conf.json`**

Add `nsis` section inside `bundle`:

```json
"nsis": {
  "displayLanguageSelector": false,
  "languages": ["Romanian", "English"],
  "license": "LICENSE",
  "headerImage": "icons/128x128.png",
  "sidebarImage": "icons/128x128@2x.png",
  "installMode": "currentUser",
  "allowOverrideInstallMode": true,
  "minimumWebview2Version": "2.0.0",
  "shortcuts": {
    "desktop": true,
    "startMenu": true
  }
}
```

- [ ] **Step 6.2 — Add Windows-specific Rust feature flags**

In `src-tauri/Cargo.toml`, add Windows-specific dependencies:

```toml
[target.'cfg(target_os = "windows")'.dependencies]
winreg = "0.52"
```

This enables proper Windows Registry integration for autostart (beyond what tauri-plugin-autostart provides).

- [ ] **Step 6.3 — Add license file for NSIS installer**

Create `LICENSE` (root of project):

```
RoFactura License

Copyright (c) 2026 Lucaris SRL
All rights reserved.

This software is proprietary. Redistribution or reverse engineering
is prohibited without written consent from Lucaris SRL.

For licensing inquiries: contact@lucaris.ro
```

- [ ] **Step 6.4 — Test Windows build locally (cross-compile or on Windows runner)**

If you have a Windows machine or use GitHub Actions:
```bash
# On Windows:
pnpm build:win-x64
# Expected: src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/RoFactura_0.1.0_x64-setup.exe
```

- [ ] **Step 6.5 — Commit**

```bash
git add src-tauri/tauri.conf.json LICENSE
git commit -m "build: configure NSIS installer for Windows

- Romanian + English language selector
- currentUser install mode (no admin required)
- Desktop + Start Menu shortcuts
- LICENSE file for NSIS display

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

---

## Task 7: Generate Tauri Updater Keys + Wire Auto-Update

**Files:**
- Modify: `src-tauri/tauri.conf.json` (updater pubkey)
- Modify: `src/pages/Settings.tsx` (verify update button works)

### Why this is separate from CI

The updater needs an asymmetric keypair. The **private key** goes into GitHub Secrets; the **public key** goes directly into `tauri.conf.json` (it's safe to commit).

- [ ] **Step 7.1 — Generate updater keypair**

```bash
cd /Users/cris/Projects/efactura-desktop
npx tauri signer generate -w ~/.tauri/rofactura-updater.key
```

This outputs:
```
Public key: dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk...
Private key saved to: ~/.tauri/rofactura-updater.key
```

Copy the **public key** to `tauri.conf.json` → `plugins.updater.pubkey`.

- [ ] **Step 7.2 — Update `tauri.conf.json` updater section**

```json
"plugins": {
  "updater": {
    "pubkey": "PASTE_PUBLIC_KEY_HERE",
    "endpoints": [
      "https://releases.lucaris.ro/efactura/{{target}}/{{arch}}/latest.json"
    ],
    "dialog": true,
    "gracefulRestart": false
  }
}
```

- [ ] **Step 7.3 — Add the private key to GitHub Secrets**

```
Secret name: TAURI_UPDATER_PRIVATE_KEY
Secret value: (contents of ~/.tauri/rofactura-updater.key)

Secret name: TAURI_UPDATER_PRIVATE_KEY_PASSWORD  
Secret value: (the password you set during generation)
```

- [ ] **Step 7.4 — Verify Settings "Verifică actualizări" button**

In `src/pages/Settings.tsx`, find the update check button. It should call `invoke("check_update")` or use the Tauri updater plugin. Verify it shows an appropriate message when no update server is reachable:
- Expected: "Nu există actualizări disponibile" or a toast with the error

- [ ] **Step 7.5 — Commit**

```bash
git add src-tauri/tauri.conf.json
git commit -m "build: configure auto-updater with public key

Private key stored in GitHub Secrets (TAURI_UPDATER_PRIVATE_KEY).
Update endpoint: releases.lucaris.ro/efactura/{{target}}/{{arch}}/latest.json

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

---

## Task 8: macOS-Specific Performance Optimizations

**Files:**
- Modify: `src-tauri/tauri.conf.json` (window config)
- Modify: `src/styles/design.css` (macOS font rendering, scrollbars)

- [ ] **Step 8.1 — Enable window vibrancy and native feel on macOS**

In `tauri.conf.json`, update the window configuration:

```json
"windows": [
  {
    "title": "RoFactura",
    "width": 1280,
    "height": 800,
    "minWidth": 1024,
    "minHeight": 600,
    "resizable": true,
    "fullscreen": false,
    "center": true,
    "decorations": true,
    "transparent": false,
    "titleBarStyle": "Overlay",
    "hiddenTitle": true
  }
]
```

`titleBarStyle: "Overlay"` moves the traffic lights (close/minimize/maximize) into the window, giving a native macOS feel. The content shifts down automatically.

- [ ] **Step 8.2 — Add macOS-specific CSS for traffic light offset**

At the top of `src/styles/design.css`, add:

```css
/* macOS: traffic light area offset when using titleBarStyle=Overlay */
@supports (-webkit-touch-callout: none) {
  /* Tauri on macOS: offset app chrome below traffic lights */
  .app-shell {
    padding-top: env(safe-area-inset-top, 0px);
  }
}
```

In `src/components/layout/AppShell.tsx` or wherever the menu bar renders, ensure it has `data-tauri-drag-region` on the draggable area:

```tsx
<div className="menubar" data-tauri-drag-region style={{ WebkitAppRegion: "drag" } as React.CSSProperties}>
```

- [ ] **Step 8.3 — Verify macOS build still works after window config change**

```bash
pnpm build:mac-arm 2>&1 | tail -5
# Expected: Finished ... [optimized] with 0 errors
```

- [ ] **Step 8.4 — Commit**

```bash
git add src-tauri/tauri.conf.json src/styles/design.css src/components/layout/AppShell.tsx
git commit -m "ux: macOS native title bar overlay + drag region

Moves traffic lights into window chrome for native macOS look.
Adds data-tauri-drag-region on menu bar for window dragging.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

---

## Task 9: Exploit Prevention — Input Sanitization Audit

**Files:**
- Modify: `src-tauri/src/db/contacts.rs` (review dynamic SQL)
- Modify: `src-tauri/src/db/received.rs` (review dynamic SQL)

### What to verify

Dynamic SQL in `contacts.rs` and `received.rs` builds queries with string concat, but all values are passed via parameterized binds (`?1`, `?2`) — NOT interpolated directly. This is safe. However, the query structure (column selection, ORDER BY) is hardcoded, so there's no injection vector. **These are safe — just verify and document.**

- [ ] **Step 9.1 — Verify contacts dynamic SQL is safe**

Open `src-tauri/src/db/contacts.rs`, line ~88. Confirm:
- The `sql` string only has `AND company_id = ?1` and `AND (legal_name LIKE ?N OR cui LIKE ?N)` appended
- All `?1`, `?2` are filled via `.bind(cid)` and `.bind(format!("%{query}%"))` — SAFE
- No user-controlled data is interpolated directly into the SQL string

Add a comment to document this review:
```rust
// SECURITY: all user values bound via ?1, ?2 params — no injection vector.
// Column selection and ORDER BY are hardcoded — safe.
```

- [ ] **Step 9.2 — Verify received dynamic SQL is safe**

Open `src-tauri/src/db/received.rs`, line ~70. Same check. Add the same comment.

- [ ] **Step 9.3 — Check for panic-causing `expect()` calls in commands**

```bash
grep -rn "\.expect(" /Users/cris/Projects/efactura-desktop/src-tauri/src/commands/ | grep -v "//\|#\[test\]"
```

Any `.expect()` that can be triggered by user input = DoS potential. Replace with proper error handling:

For each found instance, replace:
```rust
.expect("license inserted")
```
With:
```rust
.ok_or_else(|| AppError::Internal("license not found after insert".into()))?
```

- [ ] **Step 9.4 — Commit**

```bash
git add src-tauri/src/db/contacts.rs src-tauri/src/db/received.rs src-tauri/src/db/license.rs
git commit -m "security: document SQL safety + replace expect() with proper errors

Dynamic SQL in contacts/received uses parameterized binds — confirmed safe.
Replace panic-causing .expect() in DB layer with AppError::Internal.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>"
```

---

## Final Verification Checklist

After all tasks complete:

```
Security:
□ cargo check → zero errors
□ tsc --noEmit → EXIT:0
□ Enter AAAA-BBBB-CCCC-DDDD as license key → rejected with clear error
□ Enter valid HMAC key → accepted
□ Manually change expires_at in SQLite → app shows LicenseExpiredScreen
□ Open DevTools → no CSP violation errors in console
□ DevTools → check Network: no external scripts loaded

Builds:
□ pnpm build:mac-arm → succeeds, DMG created in target/aarch64-apple-darwin/
□ pnpm build:win-x64 → succeeds on Windows/CI (NSIS .exe created)
□ GitHub Actions CI workflow runs on push to main

Features:
□ Dev seed fires on fresh DB — 2 companies, contacts, invoices
□ Invoice validation shows 50+ rules (try invalid VAT category)
□ PDF generation: diacritics render correctly
□ Virtual scroll: 100+ invoices smooth
□ Autostart toggle: creates/removes LaunchAgent on macOS
□ Updater "Verifică actualizări" button works (error or success)
```

---

## Risk Notes

- **HMAC key validation**: The embedded secret (`KEY_SECRET`) provides **offline** key validation. It's not a replacement for server-side validation. Sophisticated attackers who decompile the binary can extract the secret. Server-side validation (via Supabase or custom endpoint) should be added in v1.1.
- **Hardened Runtime**: `com.apple.security.cs.allow-jit` is required for WKWebView (Tauri's renderer). Without it, the app crashes on notarized builds. If Apple tightens this, we may need to switch to a CSP-only approach.
- **NSIS on macOS cross-compile**: NSIS builds require a Windows runner. The CI uses `windows-latest` for this — don't try to cross-compile NSIS from macOS.
- **`titleBarStyle: Overlay`**: Requires testing on macOS 12+ and Windows. On Windows, it may look different — add a platform check in CSS if needed.
