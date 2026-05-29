# Release Signing — REQUIRED BEFORE SHIPPING

## Current status

- Tauri updater pubkey is configured in `src-tauri/tauri.conf.json`. ✅
- macOS app signing: `signingIdentity = "-"` (ad-hoc only). Public distribution requires Developer ID + notarization — **external blocker, not code**.
- Windows installer signing: no Authenticode certificate configured yet — **external blocker, not code**.

Before any public release:

1. Generate signing keypair:
   ```bash
   cargo tauri signer generate -w ~/.tauri/efactura-desktop.key
   ```
   This prints the public key to stdout.

2. Set the public key in `src-tauri/tauri.conf.json`:
   ```json
   "updater": {
     "pubkey": "PASTE_PUBLIC_KEY_HERE",
     "endpoints": ["https://releases.lucaris.ro/efactura/{{target}}/{{arch}}/latest.json"]
   }
   ```

3. Set CI secrets (GitHub Actions / build pipeline):
   - `TAURI_PRIVATE_KEY` = contents of `~/.tauri/efactura-desktop.key`
   - `TAURI_KEY_PASSWORD` = your key password (if set during generation)

4. Sign each release artifact during CI build — the Tauri bundler picks up
   `TAURI_PRIVATE_KEY` automatically when bundling.

## Risk

An empty pubkey means auto-updates are not signature-verified.
A compromised update endpoint (`releases.lucaris.ro`) could serve arbitrary code
to all users without any integrity check.

## Reference

- Tauri updater docs: https://tauri.app/plugin/updater/
- Tauri signer CLI: `cargo tauri signer --help`
