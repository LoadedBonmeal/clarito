# Bundled DUK Validation (A + D) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. This repo's house QA pattern (Sonnet exec → independent Opus QA → `bash scripts/verify-local.sh` green per task) applies on top.

**Goal:** Give the end user real, offline DUKIntegrator validation of D300/D394/SAF-T declarations before official export — bundled jars + a jlink JRE behind a swappable provider, with the pure-Rust preflight as the always-on floor.

**Architecture:** Two layers behind one facade. Layer A = existing `anaf_decl::preflight` (pure Rust, live). Layer D = new `anaf_decl::duk` provider that shells out to the real DUKIntegrator resolved from a `DukProvider` (bundled at runtime; env-based in tests). DUK runs inside the official-export command, blocking the file write on errors unless overridden; graceful fallback to A when the runtime is absent. DUK output is parsed into the existing `PreflightIssue` shape so both layers render identically.

**Tech Stack:** Rust (Tauri 2, tokio, std::process), React 19/TS (TanStack Query), Java (jlink minimal JRE, Temurin), GitHub Actions.

**Constraints (non-negotiable):** bundle id `com.lucaris.efactura` + `build.rs` license salt MUST NOT change. No push/merge without explicit user approval. Keep the `DukProvider` abstraction so "fetch ANAF's copy" stays a drop-in (legal redistribution caveat). Dev/runtime validation verified against the live DUK at `/tmp/dukrun` (`/opt/homebrew/opt/openjdk@17/bin/java -jar /tmp/dukrun/DUKIntegrator.jar -v D300 <xml> <res>` → `Validare fara erori`).

---

## File Structure

| Path | Responsibility | Action |
|---|---|---|
| `src-tauri/src/anaf_decl/duk/mod.rs` | DUK provider trait, runtime resolution, shell-out, output→`PreflightIssue` parser, `DukOutcome` | Create |
| `src-tauri/src/anaf_decl/validation.rs` | Dev/CI harness — refactor shell-out core into a shared fn `run_java_validator` reused by `duk` | Modify |
| `src-tauri/src/anaf_decl/mod.rs` | `pub mod duk;` | Modify |
| `src-tauri/src/commands/declarations.rs` | DUK step inside `export_d300_official` (+ D394/SAFT export cmds); `skip_duk_override` param; `DukOutcome` in return | Modify |
| `src-tauri/src/lib.rs` | (no new command needed — DUK runs inside existing export cmds) | (none) |
| `src/lib/tauri.ts` | `DukOutcome` type; thread `skipDukOverride` through `exportOfficial`/D394/SAFT export calls | Modify |
| `src/pages/Declarations.tsx`, `src/pages/reports/D394View.tsx`, `src/pages/reports/SaftView.tsx` | Handle `DukOutcome`: block + show DUK errors via `PreflightPanel` + "exportă oricum" override; graceful note when unavailable | Modify |
| `src-tauri/src/anaf_decl/duk/tests.rs` (or inline `#[cfg(test)]`) | parser + provider unit tests | Create |
| `src-tauri/tests/duk_runtime.rs` | integration: env provider + `/tmp/dukrun` validates D300/D394/SAFT fixtures | Create |
| `.github/workflows/build.yml` | jlink minimal JRE + copy DUK jars to `resources/` + per-arch macOS DMGs + smoke | Modify |
| `src-tauri/tauri.conf.json` | `bundle.resources` (jre-min + duk jars); per-arch DMG note | Modify |
| `scripts/jlink-jre.sh` | reproducible jlink invocation (CI + local) | Create |
| `src/components/layout/*` (staleness banner) | launch-time form-version check → "actualizați aplicația" | Modify (Phase 4) |

**Provider note:** runtime DUK uses `BundledProvider` (resolves Tauri resources). Tests use `EnvProvider` (reads `EFACTURA_DUK_JAR` + system `java`, i.e. `/tmp/dukrun`). Both yield `DukRuntime { java, jar_dir }`, so the logic is proven before packaging exists.

---

## Phase 1 — Backend DUK provider + parser + export integration

### Task 1: `DukRuntime`, `DukProvider`, `EnvProvider`

**Files:**
- Create: `src-tauri/src/anaf_decl/duk/mod.rs`
- Modify: `src-tauri/src/anaf_decl/mod.rs` (add `pub mod duk;`)

- [ ] **Step 1: Write the failing test** (append to `duk/mod.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn env_provider_resolves_when_jar_and_java_present() {
        // Uses the live dev runtime if configured; otherwise asserts None (no panic).
        let p = EnvProvider;
        match std::env::var("EFACTURA_DUK_JAR") {
            Ok(j) if !j.is_empty() && std::path::Path::new(&j).is_file() => {
                let rt = p.resolve();
                assert!(rt.is_some(), "should resolve when jar present");
                let rt = rt.unwrap();
                assert!(rt.jar_dir.exists());
            }
            _ => assert!(p.resolve().is_none(), "no jar -> None, never panic"),
        }
    }
}
```

- [ ] **Step 2: Run test, verify it fails to compile** — Run: `cd src-tauri && cargo test --lib duk:: 2>&1 | tail -5` — Expected: FAIL (types not defined).

- [ ] **Step 3: Implement the types + EnvProvider** (top of `duk/mod.rs`)

```rust
//! Layer D — real DUKIntegrator validation at runtime, behind a swappable provider.
//! `BundledProvider` resolves the JRE+jars shipped in the app; `EnvProvider` uses
//! `EFACTURA_DUK_JAR` + system `java` (dev/CI). Both produce a `DukRuntime`.

use std::path::{Path, PathBuf};

use crate::anaf_decl::preflight::PreflightIssue;
use crate::anaf_decl::DeclKind;
use crate::error::AppResult;

/// A resolved DUK runtime: the `java` binary + the directory holding DUKIntegrator.jar + lib/.
#[derive(Debug, Clone)]
pub struct DukRuntime {
    pub java: PathBuf,
    pub jar_dir: PathBuf,
}

impl DukRuntime {
    fn duk_jar(&self) -> PathBuf {
        self.jar_dir.join("DUKIntegrator.jar")
    }
}

/// Locates a DUK runtime. Returns `None` when none is available (→ graceful fallback to layer A).
pub trait DukProvider {
    fn resolve(&self) -> Option<DukRuntime>;
}

/// Dev/CI provider: `$EFACTURA_DUK_JAR` points at DUKIntegrator.jar; `java` from PATH.
pub struct EnvProvider;

impl DukProvider for EnvProvider {
    fn resolve(&self) -> Option<DukRuntime> {
        let jar = std::env::var(crate::anaf_decl::validation::DUK_JAR_ENV).ok()?;
        let jar_path = PathBuf::from(&jar);
        if !jar_path.is_file() {
            return None;
        }
        let jar_dir = jar_path.parent()?.to_path_buf();
        // Prefer an explicit java, else rely on PATH lookup at run time.
        let java = std::env::var("EFACTURA_DUK_JAVA")
            .ok()
            .map(PathBuf::from)
            .filter(|p| p.is_file())
            .unwrap_or_else(|| PathBuf::from("java"));
        Some(DukRuntime { java, jar_dir })
    }
}
```

- [ ] **Step 4: Add `pub mod duk;`** to `src-tauri/src/anaf_decl/mod.rs` (next to `pub mod preflight;`).

- [ ] **Step 5: Run test, verify pass** — Run: `cd src-tauri && cargo test --lib duk::tests::env_provider 2>&1 | tail -5` — Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/anaf_decl/duk/mod.rs src-tauri/src/anaf_decl/mod.rs
git commit -m "feat(duk): DukRuntime + DukProvider + EnvProvider"
```

### Task 2: DUK output → `PreflightIssue` parser

**Files:** Modify `src-tauri/src/anaf_decl/duk/mod.rs`

- [ ] **Step 1: Write the failing test** (in the `tests` mod)

```rust
#[test]
fn parser_clean_output_is_passing() {
    let out = parse_duk_output("Validare fara erori fisier: /tmp/x.xml\n");
    assert!(out.passed);
    assert!(out.errors.is_empty());
}

#[test]
fn parser_error_lines_become_issues() {
    let raw = "Atentionari la validare fisier: /tmp/x.xml\n\
               A: validari globale\n TVA(25) nu se incadreaza in 11% +- marja 1%\n";
    let out = parse_duk_output(raw);
    assert!(!out.passed);
    assert!(!out.errors.is_empty());
    assert_eq!(out.errors[0].code, "DUK");
    assert_eq!(out.errors[0].severity, "error");
    assert!(out.errors.iter().any(|i| i.message.contains("nu se incadreaza")));
}
```

- [ ] **Step 2: Run, verify fail** — Run: `cd src-tauri && cargo test --lib duk::tests::parser 2>&1 | tail` — Expected: FAIL (`parse_duk_output` undefined).

- [ ] **Step 3: Implement `DukOutcome` + `parse_duk_output`**

```rust
/// Result of a DUK run, in the same `PreflightIssue` vocabulary as layer A.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DukOutcome {
    /// True only if DUK was available, ran, AND reported no errors.
    pub passed: bool,
    pub errors: Vec<PreflightIssue>,
}

/// Parse DUKIntegrator's textual output (result file or stdout) into issues.
/// Clean marker: a line containing "fara erori" / "corect" / "ok". Any line flagged
/// with a DUK error marker becomes an `error` `PreflightIssue` with code "DUK".
pub fn parse_duk_output(raw: &str) -> DukOutcome {
    let mut errors = Vec::new();
    for line in raw.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        let low = l.to_lowercase();
        let is_marker = low.contains("fara erori")
            || low.contains("fără erori")
            || low.starts_with("ok")
            || low.contains("validare fisier")
            || low.starts_with("a:")
            || low.starts_with("e:");
        let looks_error = low.contains("eroare")
            || low.contains("erori")
            || low.contains("nu se incadreaza")
            || low.contains("invalid")
            || low.contains("atentionare")
            || low.contains("nu este corect");
        if looks_error && !low.contains("fara erori") && !low.contains("fără erori") {
            errors.push(PreflightIssue {
                severity: "error".to_string(),
                code: "DUK".to_string(),
                message: l.to_string(),
                hint: "Eroare raportată de validatorul oficial ANAF (DUKIntegrator).".to_string(),
            });
        }
        let _ = is_marker;
    }
    let clean = raw.to_lowercase().contains("fara erori")
        || raw.to_lowercase().contains("fără erori");
    DukOutcome {
        passed: errors.is_empty() && clean,
        errors,
    }
}
```

> NOTE for the implementer: `PreflightIssue` fields are public (`severity, code, message, hint: String`, serde camelCase) — confirm at `anaf_decl/preflight.rs:19`. If its constructor helpers (`PreflightIssue::error`) are `pub`, prefer them.

- [ ] **Step 4: Run, verify pass** — Run: `cd src-tauri && cargo test --lib duk::tests::parser 2>&1 | tail` — Expected: PASS (both).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/anaf_decl/duk/mod.rs
git commit -m "feat(duk): parse DUKIntegrator output into PreflightIssue"
```

### Task 3: Shared shell-out core + `run_duk`

**Files:** Modify `src-tauri/src/anaf_decl/validation.rs` (extract core) + `src-tauri/src/anaf_decl/duk/mod.rs` (call it)

- [ ] **Step 1: Refactor — extract the java invocation from `validate_with_duk`** into a shared fn in `validation.rs`:

```rust
/// Run a Java declaration validator and return its raw textual output (result file
/// contents, falling back to stdout). `java` is the binary, `jar` the DUKIntegrator jar.
pub fn run_java_validator(
    java: &Path,
    jar: &Path,
    decl: DeclKind,
    xml_path: &Path,
) -> AppResult<String> {
    let result_path = std::env::temp_dir().join(format!(
        "duk_result_{}.txt",
        xml_path.file_stem().and_then(|s| s.to_str()).unwrap_or("decl")
    ));
    let output = std::process::Command::new(java)
        .arg("-jar").arg(jar)
        .arg("-v").arg(decl.as_duk_type())
        .arg(xml_path).arg(&result_path)
        .output()
        .map_err(|e| AppError::Other(format!("Nu pot porni DUKIntegrator: {e}")))?;
    let body = std::fs::read_to_string(&result_path)
        .unwrap_or_else(|_| String::from_utf8_lossy(&output.stdout).to_string());
    let _ = std::fs::remove_file(&result_path);
    Ok(body)
}
```
Then rewrite `validate_with_duk` to call `run_java_validator(Path::new("java"), &PathBuf::from(jar), decl, xml_path)` and keep its existing `DukResult` parsing (unchanged behaviour for dev/CI).

- [ ] **Step 2: Verify dev harness still builds + its tests pass** — Run: `cd src-tauri && cargo test --lib validation:: 2>&1 | tail` — Expected: PASS (no behaviour change).

- [ ] **Step 3: Add `run_duk` to `duk/mod.rs`** (uses the shared core + parser; 15 s timeout via a thread guard)

```rust
/// Run DUK against `xml` using the runtime from `provider`. Returns `None` when no
/// runtime is available (caller falls back to layer A). Never panics.
pub fn run_duk(
    provider: &dyn DukProvider,
    decl: DeclKind,
    xml: &Path,
) -> AppResult<Option<DukOutcome>> {
    let Some(rt) = provider.resolve() else {
        return Ok(None);
    };
    let raw = crate::anaf_decl::validation::run_java_validator(&rt.java, &rt.duk_jar(), decl, xml)?;
    Ok(Some(parse_duk_output(&raw)))
}
```

- [ ] **Step 4: Integration test against the live runtime** — Create `src-tauri/tests/duk_runtime.rs`:

```rust
//! Skips unless EFACTURA_DUK_JAR + java are configured (dev/CI with /tmp/dukrun).
use std::path::Path;
use efactura_desktop_lib::anaf_decl::duk::{run_duk, EnvProvider};
use efactura_desktop_lib::anaf_decl::DeclKind;

#[test]
fn duk_validates_a_known_good_d300() {
    if std::env::var("EFACTURA_DUK_JAR").map(|v| v.is_empty()).unwrap_or(true) {
        eprintln!("SKIP duk_runtime: EFACTURA_DUK_JAR not set");
        return;
    }
    // Reuse the d300_xsd test's dump: generate a valid D300 first via that harness,
    // or point at a committed-good fixture. Here we expect the implementer to write a
    // tiny valid D300 to a temp file using the same builders as tests/d300_xsd.rs.
    let xml = std::env::var("DUK_TEST_D300_XML").unwrap_or_default();
    if xml.is_empty() || !Path::new(&xml).exists() {
        eprintln!("SKIP: set DUK_TEST_D300_XML to a generated valid D300 path");
        return;
    }
    let out = run_duk(&EnvProvider, DeclKind::D300, Path::new(&xml)).expect("run");
    let out = out.expect("runtime available");
    assert!(out.passed, "expected DUK clean, got {:?}", out.errors);
}
```

> Implementer: wire `DUK_TEST_D300_XML` by reusing `tests/d300_xsd.rs`'s `EFACTURA_DUMP_DIR` dump (`/tmp/d300.xml`), or factor the D300 builders into a shared test helper. Run with:
> `cd src-tauri && EFACTURA_DUK_JAR=/tmp/dukrun/DUKIntegrator.jar EFACTURA_DUK_JAVA=/opt/homebrew/opt/openjdk@17/bin/java DUK_TEST_D300_XML=/tmp/d300.xml cargo test --test duk_runtime -- --nocapture`
> Expected: PASS (`Validare fara erori`).

- [ ] **Step 5: Run the full gate** — Run: `bash scripts/verify-local.sh 2>&1 | tail -4` — Expected: green.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/anaf_decl/validation.rs src-tauri/src/anaf_decl/duk/mod.rs src-tauri/tests/duk_runtime.rs
git commit -m "feat(duk): shared java-validator core + run_duk + live integration test"
```

### Task 4: DUK step inside `export_d300_official` (block + override + graceful)

**Files:** Modify `src-tauri/src/commands/declarations.rs`

- [ ] **Step 1: Define the export result type** (top of declarations.rs, near D300Report)

```rust
/// Result of an official export attempt with the DUK gate.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficialExportResult {
    /// The written file path, or empty if blocked by DUK.
    pub path: String,
    pub written: bool,
    /// Whether a DUK runtime was available to validate.
    pub duk_available: bool,
    /// Whether DUK reported clean (only meaningful when duk_available).
    pub duk_passed: bool,
    pub issues: Vec<crate::anaf_decl::preflight::PreflightIssue>,
}
```

- [ ] **Step 2: Thread DUK through `export_d300_official`** — change the signature to add `skip_duk_override: bool` and return `OfficialExportResult`. After `generate_d300_xml(&rows, &ver)?` produces `xml`:

```rust
    // Validate with the bundled DUK before writing (layer D). Graceful: if no runtime, proceed.
    let tmp = std::env::temp_dir().join("d300_official_check.xml");
    std::fs::write(&tmp, xml.as_bytes()).map_err(|e| AppError::Other(e.to_string()))?;
    let provider = crate::anaf_decl::duk::BundledProvider::new(&app); // app: AppHandle param
    let duk = crate::anaf_decl::duk::run_duk(&provider, DeclKind::D300, &tmp)?;
    let _ = std::fs::remove_file(&tmp);

    let (duk_available, duk_passed, issues) = match &duk {
        Some(o) => (true, o.passed, o.errors.clone()),
        None => (false, false, Vec::new()),
    };
    // Block only when DUK ran, found errors, and the user did NOT override.
    if duk_available && !duk_passed && !skip_duk_override {
        return Ok(OfficialExportResult {
            path: String::new(), written: false, duk_available, duk_passed, issues,
        });
    }
    std::fs::write(&dest, xml.as_bytes()).map_err(|e| AppError::Other(e.to_string()))?;
    Ok(OfficialExportResult { path: dest, written: true, duk_available, duk_passed, issues })
```

> The command must take `app: tauri::AppHandle` (add to the signature) so `BundledProvider` can resolve resources. Keep `state: State<AppState>` too.

- [ ] **Step 3: Add `BundledProvider`** to `duk/mod.rs`:

```rust
/// Runtime provider: resolves the jlink JRE + DUK jars bundled as Tauri resources.
pub struct BundledProvider {
    jre_bin: PathBuf,
    jar_dir: PathBuf,
}
impl BundledProvider {
    pub fn new(app: &tauri::AppHandle) -> Self {
        use tauri::Manager;
        let res = app.path().resource_dir().unwrap_or_default();
        let java = if cfg!(windows) { "jre-min/bin/java.exe" } else { "jre-min/bin/java" };
        Self { jre_bin: res.join(java), jar_dir: res.join("duk") }
    }
}
impl DukProvider for BundledProvider {
    fn resolve(&self) -> Option<DukRuntime> {
        if self.jre_bin.is_file() && self.jar_dir.join("DUKIntegrator.jar").is_file() {
            Some(DukRuntime { java: self.jre_bin.clone(), jar_dir: self.jar_dir.clone() })
        } else {
            None // not bundled (e.g. dev) → graceful fallback
        }
    }
}
```

- [ ] **Step 4: Repeat for D394 + SAF-T export commands** — apply the same generate→DUK→block/override/write to the D394 and SAF-T official-export commands (find them near `export_d300_official` / in `saft.rs`/`commands`), using `DeclKind::D394` / `DeclKind::D406`.

- [ ] **Step 5: Unit test the gate logic** (declarations.rs `#[cfg(test)]`): a test that `OfficialExportResult` blocks (written=false) when `duk_available && !duk_passed && !override`, and writes when `override` or `!duk_available`. (Use a fake outcome; the live DUK path is covered by `duk_runtime.rs`.)

- [ ] **Step 6: Update the frontend command signatures in `src/lib/tauri.ts`** — add `skipDukOverride` arg + `DukOutcome`/`OfficialExportResult` return type to `exportOfficial` (and D394/SAFT). (Frontend wiring is Phase 2; this step is just the type + invoke arg so the build stays consistent.)

- [ ] **Step 7: Run full gate** — `bash scripts/verify-local.sh 2>&1 | tail -4` — Expected: green.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/commands/declarations.rs src-tauri/src/anaf_decl/duk/mod.rs src/lib/tauri.ts
git commit -m "feat(duk): DUK gate in official export (block + override + graceful)"
```

> **Phase 1 gate:** `bash scripts/verify-local.sh` green; `duk_runtime.rs` passes against `/tmp/dukrun`. Then **independent Opus QA** (re-run DUK on D300/D394/SAFT scenarios; verify block/override/fallback paths; confirm validation.rs dev behaviour unchanged).

---

## Phase 2 — Frontend validate-on-export UX

### Task 5: Handle `OfficialExportResult` in the three export pages

**Files:** Modify `src/pages/Declarations.tsx`, `src/pages/reports/D394View.tsx`, `src/pages/reports/SaftView.tsx`; reuse `src/components/shared/PreflightPanel.tsx`.

- [ ] **Step 1: Add export-result state** (each page): `const [dukBlock, setDukBlock] = useState<PreflightIssue[] | null>(null);`

- [ ] **Step 2: Update `handleExportOfficial`** (Declarations.tsx ~line 187; D394View ~89; SaftView ~79) to consume the new result:

```ts
const res = await api.declarations.exportOfficial(/* ...args..., */ false /* skipDukOverride */);
if (!res.written) {
  setDukBlock(res.issues);                 // blocked by DUK → show errors + offer override
  notify.error("DUKIntegrator a găsit erori. Corectați sau exportați oricum.");
  return;
}
setDukBlock(null);
notify.success(
  res.dukAvailable ? `Export oficial salvat (DUK: valid): ${res.path}`
                   : `Export oficial salvat: ${res.path} (validare DUK indisponibilă)`
);
```

- [ ] **Step 3: Render the DUK block panel + override** above/near the export button, only when `dukBlock`:

```tsx
{dukBlock && (
  <div style={{ marginTop: 12 }}>
    <PreflightPanel issues={dukBlock} />
    <Btn variant="danger" onClick={() => void handleExportOfficial(/* args, */ true /* override */)}>
      Exportă oricum (ignoră DUK)
    </Btn>
  </div>
)}
```
(Make `handleExportOfficial` accept an `override = false` param and pass it as `skipDukOverride`.)

- [ ] **Step 4: tsc + build** — Run: `cd /Users/cris/Projects/efactura-desktop && pnpm exec tsc --noEmit && pnpm build 2>&1 | tail -3` — Expected: clean.

- [ ] **Step 5: Run full gate** — `bash scripts/verify-local.sh 2>&1 | tail -4` — green.

- [ ] **Step 6: Commit**

```bash
git add src/pages/Declarations.tsx src/pages/reports/D394View.tsx src/pages/reports/SaftView.tsx src/lib/tauri.ts
git commit -m "feat(duk): validate-on-export UX — block, friendly errors, override, graceful"
```

> **Phase 2 gate:** gate green; manual smoke in `pnpm tauri dev` if feasible (export a deliberately-broken declaration → DUK block panel appears → "exportă oricum" writes the file). Then **Opus QA**.

---

## Phase 3 — CI packaging (jlink JRE + bundle + per-arch macOS)

### Task 6: Reproducible jlink script

**Files:** Create `scripts/jlink-jre.sh`

- [ ] **Step 1: Write the script**

```bash
#!/usr/bin/env bash
# Produce a minimal JRE for DUKIntegrator into ./src-tauri/resources/jre-min
# Usage: JAVA_HOME=<jdk> scripts/jlink-jre.sh
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/src-tauri/resources/jre-min"
MODULES="java.base,java.desktop,java.logging,java.xml,java.naming,java.management,jdk.crypto.ec"
rm -rf "$OUT"
"${JAVA_HOME}/bin/jlink" --add-modules "$MODULES" \
  --strip-debug --no-man-pages --no-header-files --compress=2 --output "$OUT"
"$OUT/bin/java" -version
```
> Implementer: verify the module set with `"${JAVA_HOME}/bin/jdeps" --multi-release 17 --print-module-deps src-tauri/tools/anaf/d300/D300Validator.jar src-tauri/tools/anaf/d300/D300Pdf.jar` and add any missing modules. `java.desktop` (Swing) is required.

- [ ] **Step 2: Run locally to verify size + boot** — Run: `JAVA_HOME=/opt/homebrew/opt/openjdk@17 bash scripts/jlink-jre.sh && du -sh src-tauri/resources/jre-min` — Expected: `java -version` prints; size ~40–60 MB.

- [ ] **Step 3: Stage the DUK jars** — copy the validation jars into `src-tauri/resources/duk/` (`DUKIntegrator.jar` + `lib/*.jar` from the vendored `src-tauri/tools/anaf/` set used at `/tmp/dukrun`). Add `src-tauri/resources/jre-min/` + `src-tauri/resources/duk/` to `.gitignore` (built/vendored, like the other tools) — they're produced in CI, not committed.

- [ ] **Step 4: Commit the script**

```bash
git add scripts/jlink-jre.sh .gitignore
git commit -m "build(duk): reproducible jlink minimal JRE script"
```

### Task 7: Tauri bundle resources + per-arch macOS

**Files:** Modify `src-tauri/tauri.conf.json`, `.github/workflows/build.yml`

- [ ] **Step 1: Add `bundle.resources`** in `tauri.conf.json`:

```json
"resources": ["resources/jre-min/**/*", "resources/duk/**/*"]
```
(Merge with any existing `resources` usage; keep `licenseFile`, dmg/nsis images intact. Do NOT change `identifier`.)

- [ ] **Step 2: macOS → per-arch DMGs in `build.yml`** — replace the single `--target universal-apple-darwin` build with two builds, each running `scripts/jlink-jre.sh` first (with that arch's JDK) and `pnpm tauri build --target aarch64-apple-darwin` / `x86_64-apple-darwin`; upload both DMGs as artifacts (`...-macOS-arm64-...`, `...-macOS-x64-...`). Add `actions/setup-java@v4` (Temurin 17) to both macOS and Windows jobs, and a step `JAVA_HOME=$JAVA_HOME_17_X64 bash scripts/jlink-jre.sh` (Windows: produce `jre-min` then `pnpm tauri build --target x86_64-pc-windows-msvc --bundles nsis`).

- [ ] **Step 3: CI smoke step** — after build, assert the bundled runtime works:
```bash
# macOS example (adjust path to the .app inside the dmg or the build dir)
src-tauri/target/<triple>/release/resources/jre-min/bin/java -version
```

- [ ] **Step 4: Updater feed** — the move from `universal` to per-arch changes artifact paths. Confirm `tauri.conf.json` `updater.endpoints` (`releases.lucaris.ro/efactura/{{target}}/{{arch}}/latest.json`) and the release process publish per-arch `latest.json`. Document this in the release notes step. (No identifier change.)

- [ ] **Step 5: Push branch + let CI build; verify both macOS arches + Windows produce installers and the smoke `java -version` passes.** (Push only after user approval per constraints.)

- [ ] **Step 6: Commit**

```bash
git add src-tauri/tauri.conf.json .github/workflows/build.yml
git commit -m "build(duk): bundle jre-min + duk jars; per-arch macOS DMGs + JRE smoke"
```

> **Phase 3 gate:** CI green on all three targets; the bundled `java -version` smoke passes; a manual install of one DMG + one NSIS validates a real declaration offline. **Opus QA** confirms installer size deltas are as expected and `identifier`/license salt unchanged.

---

## Phase 4 — Launch-time form-version staleness notice

### Task 8: "Formular ANAF actualizat → actualizați aplicația" banner

**Files:** new `src-tauri/src/anaf_decl/form_versions.rs` (or a small command) + a frontend banner in the app shell.

- [ ] **Step 1: Define the supported form versions** in Rust (constants matching the bundled jars, e.g. `D300_V12`, `D394_V5`, `D406_V1`). Add a command `check_form_versions() -> Vec<FormStaleness>` that fetches a small JSON manifest from `https://releases.lucaris.ro/efactura/anaf-forms.json` (timeout 5 s, graceful on failure → empty) and compares to the bundled constants.

```rust
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormStaleness { pub form: String, pub bundled: String, pub latest: String }
```

- [ ] **Step 2: Test** — unit test the comparison (bundled == latest → empty; bundled < latest → one entry). Network fetch is mocked/skipped.

- [ ] **Step 3: Frontend** — on app launch, call `api.system.checkFormVersions()`; if non-empty, show a dismissible banner in the shell: "Formular ANAF actualizat ({form}) — actualizați aplicația pentru validare corectă." Link to the updater/check-update.

- [ ] **Step 4: Gate green + commit**

```bash
git add src-tauri/src/anaf_decl/form_versions.rs src-tauri/src/lib.rs src/lib/tauri.ts src/components/layout/*
git commit -m "feat(duk): launch-time ANAF form-version staleness notice"
```

> **Phase 4 gate:** gate green; banner shows when the manifest reports a newer form; absent/offline manifest → no banner (graceful). **Opus QA**.

---

## Self-Review (done by plan author)

- **Spec coverage:** A-always + D-on-export-blocking-graceful-override (Tasks 4–5) ✓; bundled jars + jlink JRE behind `DukProvider` (Tasks 1,3,6,7) ✓; output→`PreflightIssue` (Task 2) ✓; per-arch macOS (Task 7) ✓; lockstep jars (Task 6 staging + Phase-3 build) + staleness notice (Task 8) ✓; DUK-provider abstraction for the legal fallback (Task 1/3) ✓; identifier/salt untouched (Task 7 note) ✓.
- **Placeholder scan:** integration test intentionally parameterizes the fixture path (`DUK_TEST_D300_XML`) with explicit wiring instructions — acceptable; the implementer reuses the existing d300_xsd builders. No "TODO/handle edge cases" left.
- **Type consistency:** `DukOutcome { passed, errors: Vec<PreflightIssue> }`, `DukRuntime { java, jar_dir }`, `OfficialExportResult { path, written, duk_available, duk_passed, issues }`, `DukProvider::resolve -> Option<DukRuntime>`, `run_duk(provider, decl, xml) -> Option<DukOutcome>` — consistent across Tasks 1–5. `PreflightIssue` reused from `preflight.rs` (no redefinition).

## Risks / open items (carry into execution)
- ANAF DUK redistribution clearance (legal) — the abstraction makes "fetch ANAF's copy" a drop-in if needed.
- `jdeps` module set must be verified per JDK version; `java.desktop` is mandatory (Swing).
- SAF-T large files may need a longer DUK timeout — add a configurable timeout if a real SAF-T run exceeds the default.
- Per-arch macOS changes the updater feed + release process (two DMGs) — confirm `latest.json` per arch.
