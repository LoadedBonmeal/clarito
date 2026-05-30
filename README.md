# RoFactura — Aplicație desktop e-Factura ANAF

Aplicație desktop nativă pentru gestionarea facturilor electronice prin sistemul ANAF SPV. Built cu Tauri 2.0, React 19 și SQLite.

## Tech stack

- **Frontend:** React 19 + TypeScript (strict) + Vite + Tailwind v4
- **Backend:** Rust + Tauri 2.0 + SQLite (sqlx) + Tokio
- **Targets:** macOS (arm64 + x86_64 universal), Windows (x64 + arm64)

## Cerințe sistem

- **macOS:** 11.0 (Big Sur) sau mai nou, 4 GB RAM, 200 MB storage
- **Windows:** 10 build 1809+ sau mai nou, 4 GB RAM, 200 MB storage

## Setup dezvoltare

### Prerechizite

```bash
# Rust (toolchain stable)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Node.js 22+ și pnpm
npm install -g pnpm
```

### Targets Rust necesare

```bash
# macOS (pentru build local universal)
rustup target add aarch64-apple-darwin x86_64-apple-darwin

# Windows MSVC (pentru build pe Windows)
rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc
```

### Instalare dependențe

```bash
cd efactura-desktop
pnpm install
```

### Pornire dev mode

```bash
pnpm tauri dev
```

Aceasta pornește Vite dev server pe `localhost:1420` și lansează fereastra Tauri cu hot-reload pentru frontend.

## Build-uri de producție

### macOS Universal Binary (arm64 + Intel)

```bash
pnpm build:mac
```

Output: `src-tauri/target/universal-apple-darwin/release/bundle/dmg/RoFactura_0.1.0_universal.dmg`

### macOS doar Apple Silicon

```bash
pnpm build:mac-arm
```

### macOS doar Intel

```bash
pnpm build:mac-intel
```

### Windows x64 (rulează pe Windows)

```bash
pnpm build:win-x64
```

Output: `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/msi/RoFactura_0.1.0_x64_en-US.msi`

### Windows ARM64 (rulează pe Windows)

```bash
pnpm build:win-arm
```

## Build cross-platform

**Important:** build-urile Windows din macOS/Linux necesită fie:
- `cargo-xwin` (cross-compile cu Windows SDK descărcat) — vezi `https://github.com/rust-cross/cargo-xwin`
- O mașină Windows reală
- CI/CD (GitHub Actions) — config în planul de lansare

Recomandare: build-urile pentru release se fac via CI pentru semnătură + notarizare consistentă.

## Structură proiect

```
efactura-desktop/
├── src/                  # Frontend React + TS
├── src-tauri/            # Backend Rust + Tauri
│   ├── src/              # Cod Rust
│   ├── migrations/       # SQL migrations
│   ├── capabilities/     # Permisiuni Tauri
│   ├── Cargo.toml
│   └── tauri.conf.json
├── public/               # Assets statice
├── package.json
├── pnpm-workspace.yaml
└── vite.config.ts
```

## ANAF SPV

Aplicația folosește două medii ANAF:
- **TEST:** `https://api.anaf.ro/test/` — pentru dezvoltare și testare
- **PROD:** `https://api.anaf.ro/prod/` — pentru facturare reală

Comutarea se face din Setări → Avansat → "Folosește mediul test ANAF".

## Confidențialitate

- Toate datele sunt stocate **local** pe dispozitiv (SQLite + arhivă pe disk)
- Tokenii OAuth ANAF sunt criptați în OS Keychain (macOS Keychain / Windows Credential Manager)
- Nu există backend cloud propriu — comunicarea este direct cu ANAF

## Licență

Proprietary © Lucaris

## Support

- **Email**: support@lucaris.ro
- **Issues**: https://github.com/LoadedBonmeal/RoFactura/issues
- **Buton "Trimite feedback"** în app: Setări → Suport și feedback → deschide clientul de email cu diagnostic atașat (versiunea app, OS, machine ID anonimizat, ultimele 50 linii log)

### FAQ rapid

- **"Licența a expirat"** — vezi secțiunea Troubleshooting de mai jos
- **"Vreau să facturez un client EU"** — Contacte → Editează → setează Țară (orice ISO-3166) + Monedă (EUR/USD/etc.). VAT category-ul liniilor se va corecta automat la 0%/AE pentru intracomunitar
- **"App nu pornește după update"** — vezi secțiunea "License invalidated after version bump" de mai jos
- **"Cum trimit factura la ANAF"** — InvoiceDetail → buton "Trimite la ANAF". Necesită autorizare SPV (Setări → ANAF → Autorizează)

## Cumpărare licență

### Pentru utilizatori

1. Click "Cumpără licență →" în app (Setări → Suport și feedback) sau în ecranul "Licența a expirat"
2. Se deschide Stripe Payment Link în browser-ul tău
3. După plată, primești cheia pe email în câteva ore (manual la început)
4. Introdu cheia: Setări → Licență → Activează, sau în ecranul de expirare → "Am deja o licență"

### Pentru dev (issue manual al cheilor)

```bash
cd /path/to/efactura-desktop
cargo run --bin license-gen -- \
  --tier SOLO \
  --email customer@example.com \
  --expires-days 365
```

Output (stdout): cheia `XXXX-XXXX-XXXX-CCCCCCCC` — copy-paste în email-ul către client. Stderr conține metadata: tier, email, build version.

⚠️ **Important**: cheile sunt legate de build-ul curent (versiunea din `Cargo.toml`). Dacă faci version bump (ex: 0.2.0 → 0.3.0), salt-ul XOR din `build.rs` se schimbă și cheile vechi devin invalide. Re-emite cheile pentru clienții existenți după fiecare release.

### Setup Stripe Payment Link (one-time, ~10 minute)

1. `dashboard.stripe.com` → Products → New → "RoFactura SOLO 1-an" → preț (ex: €120 / RON 599)
2. → Create Payment Link → copy URL-ul (format: `https://buy.stripe.com/xxxxx`)
3. În app: Setări → setări avansate → `purchase_url` = URL-ul de mai sus. Butonul "Cumpără licență" din app va folosi automat acest URL.
4. (Opțional, pentru auto-issue) Webhook: `dashboard.stripe.com → Developers → Webhooks → Add endpoint` → eveniment `checkout.session.completed`. Endpoint-ul tău (Vercel / Cloudflare Worker) primește event-ul cu email-ul clientului → invocă `license-gen` → trimite cheia automat. Effort estimat: 2-3h.

Fără webhook: monitorizezi `Payments` în Stripe dashboard și rulezi `license-gen` manual de fiecare dată (acceptabil până la ~50 vânzări / lună).

## Troubleshooting

### Licența invalidată după version bump

**Simptom**: după update la o versiune nouă, app-ul arată "Licența a expirat" deși licența ta era validă în versiunea anterioară.

**Cauza**: salt-ul XOR din `build.rs` se calculează din `pkg_name + pkg_version`. Bump-ul de versiune schimbă salt-ul → fingerprint-urile vechi nu mai corespund.

**Fix utilizator**: contactează support@lucaris.ro cu email-ul cu care ai cumpărat → primești cheie nouă în câteva ore.

**Fix dev pe propria mașină**:

```bash
# Stop app
pkill -f "RoFactura|efactura-desktop"

# Clean state
sqlite3 ~/Library/Application\ Support/com.lucaris.efactura/data.db \
  "DELETE FROM license WHERE id=1; \
   DELETE FROM settings WHERE key LIKE 'license_%';"
security delete-generic-password \
  -s "ro.lucaris.efactura.trial.v1" -a "trial_status" 2>/dev/null

# Re-generate trial / activation key (de pe Mac-ul tău, în repo dir)
cargo run --bin license-gen -- --tier SOLO --email tu@tine.ro
# Apoi pornește app + activează cheia obținută
```

### Mailto button doesn't open the mail client (Linux)

```bash
sudo apt install xdg-utils    # sau equivalentul pentru distroul tău
```

Asigură-te că ai un client default setat: `xdg-mime default thunderbird.desktop x-scheme-handler/mailto` (înlocuiește `thunderbird.desktop` cu clientul tău).

### Anti-rollback hard-fail după sleep lung / NTP correction

**Simptom**: după ce laptop-ul a stat ore în sleep, app-ul refuză să pornească: "Licența nu mai este validă pe această mașină".

**Cauza** (rezolvată în v0.2.0): vechiul anti-rollback check refuza dacă `last_seen > now`. Acum tolerează drift până la 30 zile (1 zi silent, 1-30 zile warning, > 30 zile hard fail).

**Dacă ai versiune < 0.2.0**: update la latest.

**Dacă ai > 0.2.0 și tot apare**: clean state cu sql-ul din "Licența invalidată" de mai sus.

### Build local: "Blocking waiting for file lock on build directory"

`pnpm tauri dev` și `pnpm build:mac` (sau `cargo build`) intră în conflict pe lock-ul cargo target. Oprește dev (Ctrl+C în terminalul respectiv) înainte de un release build, sau folosește `CARGO_TARGET_DIR=/tmp/cargo-release pnpm build:mac` pentru target separat.
