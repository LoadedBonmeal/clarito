# DUK Bundled Validation — Release Notes

## Overview

Starting from this task (T7 / branch `duk-bundled-validation`), every Clarito
release ships with a minimal JRE (`jre-min/`) and the ANAF DUK validator JAR
(`duk/`) embedded inside the application bundle.  The `BundledProvider` in
`src-tauri/src/` resolves these at runtime via Tauri's `resource_dir()`.

---

## `DUK_JARS_URL` secret

CI requires a **repository secret** named `DUK_JARS_URL` that points to a
version-pinned ZIP containing the DUK validator artifacts:

```
DUKIntegrator.jar
lib/
  *.jar          (DUK dependency jars)
config/
  *.xml / *.xsd  (optional ANAF config files)
```

The ZIP is fetched at build time by the `Stage DUK validator jars` step in
`build.yml` and extracted into `src-tauri/resources/duk/`.

**Version pinning**: the ZIP URL must be version-pinned (e.g. via a dated
release URL or a content-addressed artifact store) and bumped in lockstep with
the DUK version used by the D300/D394/SAF-T generators.  A mismatch between the
generator's expected DUK rules and the bundled JAR version will cause validation
failures at runtime.

---

## Per-arch macOS DMGs

The old single `universal-apple-darwin` build has been replaced by two
per-architecture builds:

| Job | Runner | Target | Artifact name |
|-----|--------|--------|---------------|
| `build-macos-arm64` | `macos-14` (Apple Silicon) | `aarch64-apple-darwin` | `RoFactura-macOS-arm64-unsigned-dev` |
| `build-macos-x64` | `macos-14` (arm64 runner + x64 JDK) | `x86_64-apple-darwin` | `RoFactura-macOS-x64-unsigned-dev` |

### Updater feed implication

The updater endpoint is `releases.lucaris.ro/efactura/{{target}}/{{arch}}/latest.json`.
Serving two DMGs means the release pipeline (release.yml) must publish to BOTH:
- `…/darwin/aarch64/latest.json`
- `…/darwin/x86_64/latest.json`

Update `release.yml` accordingly when wiring signed distribution builds.

### x86_64 JRE on arm64 runner (DONE_WITH_CONCERNS)

The `build-macos-x64` job runs on `macos-14` (arm64).  `setup-java@v4` is
called with `architecture: x64` which requests a Temurin x86_64 JDK.  On an
arm64 runner this depends on Rosetta 2 and whether the JDK download exposes
an x86_64 binary.  If it does not, the `jlink` step will fail to produce a
correct x86_64 JRE and the build will error.

**Mitigation options (pick one when wiring production)**:
1. Switch `build-macos-x64` to `runs-on: macos-13` (Intel runner, native x64).
2. Use a cross-compiler Docker image or a separate self-hosted Intel runner.
3. Accept arm64-only macOS distribution and drop the x64 job.

---

## ANAF clearance reminder

DUK (the ANAF offline validator) and its accompanying configuration files are
distributed by ANAF and are subject to their terms.

**DO NOT ship a public release with the bundled DUK JARs until ANAF clearance
has been obtained** confirming that redistributing DUKIntegrator.jar inside the
application bundle is permitted.

Contact: ANAF IT Support / D-SAF team before any public release that includes
the DUK artifacts.
