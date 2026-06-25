# Code signing — Clarito

The build/CI is fully wired for signing; it only needs **your** certificates as GitHub secrets.
Signed + notarized installers are produced by `.github/workflows/release.yml`, which runs when you
push a tag like `v0.7.0`. Until the secrets exist, use the unsigned/ad-hoc builds (see bottom).

Set every secret with:
```bash
gh secret set <NAME> --repo LoadedBonmeal/clarito           # paste value when prompted
gh secret set <NAME> --repo LoadedBonmeal/clarito < file    # from a file (e.g. base64)
```

## 1. macOS — Developer ID + notarization (~$99/yr)

1. Enrol in the **Apple Developer Program**: <https://developer.apple.com/programs/> ($99/yr).
2. In Xcode (Settings → Accounts → Manage Certificates) or the portal, create a
   **"Developer ID Application"** certificate. Export it from Keychain as a `.p12` (with a password).
3. Base64-encode it: `base64 -i DeveloperID.p12 | pbcopy`.
4. Generate an **app-specific password**: <https://account.apple.com> → Sign-In & Security →
   App-Specific Passwords.
5. Find your **Team ID**: <https://developer.apple.com/account> → Membership.

Set these secrets:

| Secret | Value |
|---|---|
| `APPLE_CERTIFICATE` | base64 of the `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | the `.p12` export password |
| `APPLE_SIGNING_IDENTITY` | `Developer ID Application: Your Name (TEAMID)` |
| `APPLE_ID` | your Apple ID email |
| `APPLE_PASSWORD` | the app-specific password (step 4) |
| `APPLE_TEAM_ID` | your 10-char Team ID |
| `KEYCHAIN_PASSWORD` | any random string (CI keychain) |

> `src-tauri/tauri.conf.json` → `bundle.macOS.providerShortName` is currently `"LUCARIS"` — change it
> to your provider short name (App Store Connect → Membership) or it can be removed; notarization here
> uses the Apple-ID method (`APPLE_ID`/`APPLE_PASSWORD`/`APPLE_TEAM_ID`), which doesn't require it.

## 2. Windows — code-signing certificate

EV certificates ship on hardware tokens that **can't be automated in CI**. Use one of:

- **OV (Organization Validation) certificate** as a `.pfx` file — works in CI. ~$100–400/yr
  (Sectigo, DigiCert, SSL.com). Identity-verified to your company.
- **Azure Trusted Signing** (modern, cloud, ~$10/mo) — recommended; needs a small workflow tweak
  to use the `azure/trusted-signing-action` instead of the `.pfx` env vars.

For the `.pfx` route, set:

| Secret | Value |
|---|---|
| `WINDOWS_CERTIFICATE` | base64 of the `.pfx` |
| `WINDOWS_CERTIFICATE_PASSWORD` | the `.pfx` password |

## 3. Tauri auto-updater signing (already referenced by CI)

```bash
pnpm tauri signer generate -w ~/.tauri/clarito.key       # prints the public key + a password
gh secret set TAURI_SIGNING_PRIVATE_KEY < ~/.tauri/clarito.key
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD         # the password you chose
```
Put the printed **public** key into `tauri.conf.json` → `plugins.updater.pubkey` if you use updates.

## 4. Produce a signed + notarized release

```bash
git tag v0.7.0 && git push private v0.7.0
```
`release.yml` then builds macOS (universal, signed + notarized) and Windows (signed NSIS) and creates a
**draft GitHub Release** with both attached. The `check-signing-secrets` job fails fast and tells you
exactly which secret is missing.

## Until then — the current unsigned/ad-hoc builds

- **macOS** (`Clarito_0.7.0_universal.dmg`): the app is **ad-hoc-signed** so it runs on Apple Silicon.
  First launch: **right-click → Open** (once), or `xattr -dr com.apple.quarantine /Applications/Clarito.app`.
- **Windows** (`*-setup.exe` / `*.msi`): unsigned → SmartScreen shows "Windows protected your PC" →
  **More info → Run anyway**.
