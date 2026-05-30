# Release Signing — Setup Guide

## Current Status

| Item | Status |
|---|---|
| Tauri updater pubkey | Configured in `tauri.conf.json` |
| macOS ad-hoc signing (local dev) | Works locally |
| macOS Developer ID signing | Needs Apple Developer Program certificate |
| macOS notarization | Needs App Store Connect API key |
| Windows Authenticode signing | Needs code-signing certificate |
| CI release pipeline | `release.yml` ready — will enforce signing on v* tags |

## Required GitHub Secrets

Configure these in: **GitHub → Repository → Settings → Secrets → Actions**

### macOS Signing

| Secret | How to get |
|---|---|
| `APPLE_CERTIFICATE` | Base64 of Developer ID Application .p12 file: `base64 -i cert.p12 \| tr -d '\n'` |
| `APPLE_CERTIFICATE_PASSWORD` | Password used when exporting the .p12 |
| `APPLE_SIGNING_IDENTITY` | From `security find-identity -p codesigning` — looks like `Developer ID Application: Lucaris SRL (TEAMID)` |
| `KEYCHAIN_PASSWORD` | Any secure random password for the CI keychain |
| `APPLE_ID` | Apple ID email used for notarization |
| `APPLE_TEAM_ID` | From developer.apple.com — your 10-char team ID |
| `APPLE_API_KEY_ID` | App Store Connect API key ID (create at appstoreconnect.apple.com) |
| `APPLE_API_ISSUER_ID` | App Store Connect API issuer ID |

### Windows Signing

| Secret | How to get |
|---|---|
| `WINDOWS_CERTIFICATE` | Base64 of .pfx file: `base64 -w 0 cert.pfx` |
| `WINDOWS_CERTIFICATE_PASSWORD` | Password for the .pfx file |

**Alternative for Windows:** Azure Trusted Signing (~€10/month, no EV cert needed):
- Create Azure Trusted Signing account
- Use `tauri-plugin-authenticode` or `signtool` in CI

### Updater Signing

| Secret | How to get |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | Contents of `~/.tauri/efactura-desktop.key` (generated with `cargo tauri signer generate`) |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Password chosen during key generation |

## Release Process

1. Ensure all secrets above are configured
2. Create and push a version tag: `git tag v1.0.0 && git push origin v1.0.0`
3. CI pipeline triggers automatically:
   - Checks all secrets are present (fails if any missing)
   - Runs full quality gate (fmt, clippy, tests, tsc)
   - Builds signed + notarized macOS DMG
   - Builds signed Windows MSI + NSIS
   - Creates draft GitHub Release with both artifacts

## Local Development

Local builds use ad-hoc signing (`-`) and are NOT notarized.
This is expected — only CI release builds use Developer ID + notarization.

To test a local build:
```bash
pnpm tauri build
```
The resulting `.dmg` will show a Gatekeeper warning on other machines (expected for ad-hoc builds).
