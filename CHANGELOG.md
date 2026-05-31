# Changelog

Toate modificările notabile ale RoFactura. Format: [Keep a Changelog](https://keepachangelog.com), versionare [SemVer](https://semver.org).

## [0.3.0] - 2026-05-31

### Added
- Declarația D300 (decont TVA, partea de vânzări) + pagină Declarații
- Editor șabloane recurente: creare + editare + „Salvează ca șablon" din factură
- Dialog scurtături tastatură (Ctrl+/) cu etichete native macOS/Windows
- GDPR: export complet date + ștergere totală (Setări → Confidențialitate)
- license-gen CLI (workspace crate separat) pentru emiterea cheilor de licență
- Configurare ANAF avansată (client_id, redirect, port, URL-uri) + mediu de test
- Coloană vatCategory în editor linii factură + LineItemsEditor wired în Recurring/InvoiceEdit
- Backup complet (inclusiv fișiere arhivă) + cap recurring loop
- Trial status surface + căutare în company switcher din sidebar
- Dashboard redesenat: segmente perioadă, acțiune Corectează, etichete panel
- Suport și feedback: secțiune dedicată + wiring mailto diagnostic gather
- Teste de integrare backend (migrații + contract schemă) și teste unitare frontend

### Fixed
- Feedback + Cumpără licență: deschidere corectă client email / browser (openUrl)
- ANAF SPV: mediu de test OAuth + configurare avansată + mesaje de eroare clare
- Rapoarte: export robust la runtime + reîmprospătare vizibilă pe dashboard
- Șabloane: preserve vatCategory la „Salvează ca șablon" (defect QA#1)
- CSV import transacțional + mesaj clar la licență expirată la activare
- Modals Storno + CsvImport + company switcher wrapped în Radix Dialog (a11y)
- Bundler: license-gen extras în workspace crate propriu (fix CI real)
- Bundler: set default-run pentru ca Tauri să bundleze efactura-desktop, nu license-gen

### Security
- Eliminat tauri-plugin-sql; fs scoped la directoarele app/user (SEC-R7-01/02)
- HMAC-SHA256 (RFC 2104) pentru cheile de licență + hardening cale import
- async-FS în comenzi (non-blocking I/O)

---

## [0.2.0] - 2026-05-30

### Added
- Selectare TVA (vatCategory) + clienți UE (câmp țară/monedă) în factură
- Single-instance focus (cross-platform) — o singură instanță a aplicației
- Anti-rollback: toleranță drift ceas ±30 zile pentru cheile de licență
- license-gen binar CLI (primul draft, ulterior mutat în workspace crate)
- Workflow CI: build NSIS-only pe Windows, workflow_dispatch, ubuntu diagnostic
- README: secțiuni Support, Cumpărare licență (Stripe), Troubleshooting

### Fixed
- Storno: atomic submit/storno claims + guards + migrație 0011
- Storno: storno_of_invoice_id FK + validare dată chrono + eliminat heuristica series='S'
- Import CSV: câmpuri cu ghilimele (crate csv în loc de split(';'))
- Formatters: formatOptionalRon folosește parseDec pentru consistență
- Programare DST-safe + audit log error logging + propagare coloane SAF-T
- Dashboard: redesenat conform Claude Design (segmente, Corectează, etichete)

### Changed
- Refactorizare background: mod.rs de 1242 linii împărțit în submodule focusate
- Centralizare query keys în factory queryKeys
- Politică CI: format/clippy/test/typecheck înainte de build

### Security
- Obfuscare secrete licență via build.rs XOR cycle (SEC-05)
- Hostname real via OS API + checksum 32-bit (SEC-09/10)
- Validare integritate DB backup înainte de restore
- Narrowing Tauri capabilities — eliminat process:default
- Redactare body răspuns ANAF din loguri, OsRng pentru PKCE

---

## [0.1.0] - 2026-05 (versiune inițială)

### Added
- Versiune inițială — aplicație desktop Tauri pentru e-Factura ANAF
- Multi-companie, validare RO_CIUS, arhivare locală
- Facturare, storno, import/export XML ANAF, trimitere SPV
- Plăți, facturi recurente, export SAF-T D406
- Design system v2, ribbon, meniu aplicație, shortcut-uri OS-aware
- Branding complet: icon, DMG, NSIS installer cu EULA română
- Sistem licențiere (TRIAL/SOLO) cu fingerprint hardware
- Securitate: CSP strict, FS scope, pipeline CI/CD
