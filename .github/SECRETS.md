# Required GitHub Secrets for CI/CD

## macOS Code Signing
| Secret | Description |
|--------|-------------|
| `APPLE_CERTIFICATE` | Base64-encoded .p12 Developer ID certificate |
| `APPLE_CERTIFICATE_PASSWORD` | Certificate password |
| `APPLE_SIGNING_IDENTITY` | e.g. "Developer ID Application: Lucaris SRL (TEAM123)" |
| `APPLE_ID` | Apple ID email for notarization |
| `APPLE_PASSWORD` | App-specific password from appleid.apple.com |
| `APPLE_TEAM_ID` | 10-char team ID from developer.apple.com |

## Tauri Auto-Updater
Generate with: `npx tauri signer generate -w ~/.tauri/rofactura.key`

| Secret | Description |
|--------|-------------|
| `TAURI_UPDATER_PRIVATE_KEY` | Private key content |
| `TAURI_UPDATER_PRIVATE_KEY_PASSWORD` | Private key password |

Put the **public key** in `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`.
