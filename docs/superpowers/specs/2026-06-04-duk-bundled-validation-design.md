# Design — Real DUK validation at the consumer (bundled A + D)

- **Date:** 2026-06-04
- **Status:** Approved (brainstorm) → pending implementation plan
- **Author:** Clarito / Lucaris
- **Branch:** `duk-bundled-validation`

## Context / Problem

DUKIntegrator (ANAF's official Java validator for the D300 / D394 / SAF-T D406 declarations)
runs only as a **dev/CI tool** today (`anaf_decl/validation.rs`, gated on `EFACTURA_DUK_JAR`).
In the **shipped app there is no DUK**: no Java, no jars. The end user (accountant) gets only
the pure-Rust **preflight** (`anaf_decl::preflight`, "layer A"), which catches the *common*
DUK-fatal issues but is a **subset** of DUK's rules. So a declaration can pass the app's checks
yet still be rejected by ANAF's DUK on upload.

**Goal:** give the end user **real, local DUK validation** of declarations before export —
authoritative, offline, with friendly Romanian errors — without coupling the app to a fragile
external setup. Chosen strategy: a **mix of A (extend preflight) + D (bundle DUK + a JRE)**.

## Decisions (from brainstorm)

| Topic | Decision |
|---|---|
| DUK runtime source | **Bundle** ANAF's DUK jars + a `jlink` minimal JRE inside the installer (behind a swappable `DukProvider`) |
| JRE | Produced with `jlink` in CI (Temurin); **not hosted** — shipped in the bundle |
| UX gating | **Layer A always-on (live)**; **Layer D auto-runs at "Export oficial ANAF", blocking on DUK errors**, with graceful fallback + an explicit "exportă oricum" override |
| Jar currency | Jars ship **lockstep with the generator** per app release (no jar↔generator desync); plus a launch-time "form revision available → update app" notice |
| macOS packaging | **Per-arch DMGs** (arm64 + Intel), each with its matching JRE (avoids ~80 MB double-JRE in a universal DMG) |
| Legal | Redistributing ANAF's DUK is a **gray zone** — abstraction keeps "fetch ANAF's copy" as a fallback; seek written ANAF clearance |

## Architecture — two layers behind one validation facade

- **Layer A — `anaf_decl::preflight` (exists, to extend):** pure-Rust, instant, zero deps.
  Runs live; friendly RO `PreflightIssue`s. The always-available floor.
- **Layer D — new `anaf_decl::duk` provider:** runs the real DUKIntegrator.
  - `trait DukProvider { fn resolve(&self) -> Option<DukRuntime>; }` where
    `DukRuntime { java: PathBuf, jar_dir: PathBuf }`.
  - `BundledProvider` resolves the bundled `jre-min/bin/java` + `duk/` jar dir via Tauri's
    resource resolver. (Future `FetchedProvider` / `SystemProvider` are drop-in alternatives —
    this is what de-risks the licensing question.)
  - `fn run(kind: DeclKind, xml: &Path) -> AppResult<DukReport>` — reuses today's
    `validate_with_duk` shell-out, repointed at the resolved runtime; 15 s timeout; never panics.
  - `DukReport { passed: bool, errors: Vec<PreflightIssue> }` — DUK's raw RO output lines are
    parsed into the **same `PreflightIssue` shape** as layer A so the UI renders both identically.

## Components

- `src-tauri/src/anaf_decl/duk/mod.rs` — `DukProvider` trait, `BundledProvider`, `run()`,
  DUK-output→`PreflightIssue` parser. (Refactor the dev-only `validation.rs` shell-out into a
  shared helper both can call.)
- `src-tauri/resources/jre-min/` + `src-tauri/resources/duk/` — produced in CI; declared in
  `tauri.conf.json` `bundle.resources`. `duk/` holds `DUKIntegrator.jar` + `lib/*.jar` (the
  subset `-v` needs; signing `config/` excluded).
- `src-tauri/src/commands/declarations.rs` — new command
  `validate_declaration_duk(kind: String, xml_path: String) -> Vec<PreflightIssue>` (+ a `passed`
  flag); registered in `lib.rs`.
- Frontend: `api.declarations.validateDuk(kind, xmlPath)`; export handlers in
  `Declarations.tsx`, `reports/D394View.tsx`, `reports/SaftView.tsx` call it before writing the
  official XML; a shared result panel (reuse `PreflightPanel`).

## Data flow

1. Accountant works → **layer A** runs live → preflight badge/banners.
2. Click **"Export oficial ANAF"** → generate XML to a temp path → **layer D** runs DUK.
   - DUK clean → write the file + "✓ DUK: valid".
   - DUK errors → **block** the export, show friendly RO errors (parsed), let them fix; keep an
     explicit **"exportă oricum"** override (respects accountant authority; defaults to safe).
   - DUK runtime unavailable/unrunnable → **graceful fallback** to layer-A result + a
     non-blocking note ("validarea DUK completă indisponibilă"). Never trap the user.

## Packaging (CI)

- `actions/setup-java` (Temurin 17 or 21) per OS/arch job.
- `jdeps --print-module-deps` on the DUK jars to derive the module set (expected:
  `java.base,java.desktop,java.logging,java.xml,java.naming,java.management` + `jdk.crypto.ec`
  if signing modules are ever needed); `jlink --add-modules … --strip-debug --no-man-pages
  --no-header-files --compress=2 --output resources/jre-min` (~40–50 MB).
- Copy DUK jars into `resources/duk/`. Then `tauri build`.
- **macOS:** build two DMGs (`aarch64-apple-darwin`, `x86_64-apple-darwin`), each with its arch's
  JRE. **Windows:** single x64 NSIS.
- CI smoke step: `resources/jre-min/bin/java -version` + one `-v D300` run on a fixture.

## Updates

- DUK jars + generator ship **together** per app release → no desync (a form revision needs both,
  so they move as one).
- Launch-time **form-version check** against the CDN (`releases.lucaris.ro`): if ANAF has revised a
  form the installed app doesn't support, show "formular ANAF actualizat — actualizați aplicația".

## Error handling / robustness

- DUK call: `tokio::process` with a **15 s timeout** (SAF-T files are large — may need more);
  on timeout/launch-failure → graceful fallback (layer A), log a warning, no panic.
- Parser tolerant of DUK output format drift (treat unknown error lines as generic errors).
- All amounts/paths validated; the existing `validate_export_path` guard still applies to the
  final write.

## Testing

- **Unit:** `BundledProvider` path resolution (mock resource dir); DUK-output→`PreflightIssue`
  parser (sample DUK error + "Validare fara erori" cases).
- **Integration** (`tests/d300_xsd.rs` + new): with a real runtime, the D300/D394/SAF-T scenarios
  validate "Validare fara erori"; a deliberately-broken XML yields parsed errors.
- **CI:** jlink + `java -version` smoke + one DUK `-v` run.

## Out of scope / risks / open questions

- **Legal:** ANAF DUK redistribution clearance (the abstraction lets us switch to fetch-from-ANAF
  if needed). **Action: email ANAF for written OK.**
- Installer size grows (mac ~23→~63 MB/arch; Win ~9→~55 MB) — acceptable for an accounting app.
- SAF-T D406 large-file DUK timeout tuning.
- The macOS move from universal → per-arch DMGs affects the updater feed (`{{target}}/{{arch}}`)
  and the release process (two DMGs) — confirm updater config handles per-arch.
- Layer-A extension (more DUK rules in pure Rust) is complementary and tracked separately; this
  spec focuses on the D layer + the A/D orchestration.

## Rough phases (detail comes from the implementation plan)

1. `duk` provider module + DUK-output parser + command + tests (using the existing `/tmp/dukrun`
   runtime as the dev provider, so logic lands before packaging).
2. Frontend: validate-on-export wiring + result panel + override + graceful fallback.
3. CI packaging: jlink + bundle resources + per-arch macOS DMGs + smoke.
4. Updates: form-version staleness notice.
