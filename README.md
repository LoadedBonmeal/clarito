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
