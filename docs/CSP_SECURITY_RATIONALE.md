# CSP & WebView Security Rationale

This documents the Content-Security-Policy in `src-tauri/tauri.conf.json` and the two
directives that are weaker than a textbook `default-src 'self'`. Both were reviewed in the
2026 audit (SEC-02 / SEC-08) and are **accepted, deliberate trade-offs** — not oversights.

Current policy:

```
default-src 'self';
script-src 'self' 'wasm-unsafe-eval';
style-src  'self' 'unsafe-inline';
img-src    'self' data: blob: asset: https://asset.localhost;
connect-src 'self' ipc: http://ipc.localhost https://webservicesp.anaf.ro https://api.anaf.ro;
font-src   'self' data:;
object-src 'none';
base-uri   'none';
frame-ancestors 'none';
form-action 'self'
```

## `script-src 'wasm-unsafe-eval'` (SEC-08) — required by the in-app PDF viewer

The PDF viewer (EmbedPDF) runs **PDFium compiled to WebAssembly**. Instantiating a WASM
module requires `'wasm-unsafe-eval'`; without it the viewer cannot start. Note this is
**not** `'unsafe-eval'` — it permits WASM compilation only, **not** JavaScript `eval()`/
`new Function()`, so the classic string-to-code injection vector stays closed.

Why this is low-risk here:
- The `.wasm` is **bundled in the app** (imported via `@embedpdf/pdfium/pdfium.wasm?url`) and
  served from the app origin. No WASM is fetched from the network at runtime.
- `connect-src` is allow-listed to `'self'` + the two ANAF hosts, so the WASM (and the rest of
  the app) cannot exfiltrate to or pull code from an arbitrary origin.
- PDF bytes opened in the viewer are local files the user already has (invoice archive / picked
  files), validated before load (`%PDF` magic byte + 100 MB cap — ROB-06).

Removing `'wasm-unsafe-eval'` is not an option: it would disable the PDF viewer entirely.

## `style-src 'unsafe-inline'` (SEC-02) — required by React inline styles + dynamic PDF sizing

The UI is React; component inline `style={{…}}` props and the PDF viewer's dynamically-computed
page dimensions both emit inline styles. `'unsafe-inline'` for **styles** allows CSS, not script,
so it is not a code-execution vector. The realistic risk of inline styles is CSS-based
data-exfiltration/UI-redress, which is mitigated here because:
- `connect-src` / `img-src` are allow-listed (no `*`), so CSS cannot beacon out to an attacker.
- There is no untrusted HTML sink: invoice PDF colours are computed server-side (Rust), and the
  app does not render attacker-controlled markup into the DOM.

Migrating ~1000 inline-style sites to nonce'd `<style>` tags + a hashing build step is a large,
fragile change for little marginal security gain given the allow-listed `connect-src`.

## Why not iframe-sandbox the PDF viewer?

Considered and rejected. Isolating EmbedPDF in a `sandbox`ed iframe with `postMessage` RPC
would let the main document drop `'wasm-unsafe-eval'`, but it adds: cross-context RPC latency
on every page render/scroll, a second WASM/asset load path, and cross-platform WebView quirks
(macOS WKWebView vs Windows WebView2). The gain is marginal — the WASM is bundled and
`connect-src` is already locked down — so the added complexity and failure surface aren't
justified. Revisit only if the viewer is ever made to load remote documents.

## Locked-down directives (defense-in-depth)

`object-src 'none'` (no plugins), `base-uri 'none'` (no `<base>` hijack), `frame-ancestors
'none'` (no embedding/clickjacking), `form-action 'self'` (no off-origin form posts), and an
allow-listed `connect-src` (only `'self'`, the IPC scheme, and the two ANAF API hosts) together
keep the exfiltration and injection surface small despite the two `unsafe-*` relaxations above.
