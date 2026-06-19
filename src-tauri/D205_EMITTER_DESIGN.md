# D205 emitter — design document (dividend channel with real DUK validation)

> **Status: ✅ IMPLEMENTED + DUK-VALIDATED (2026-06-15).** The emitter ships in
> `src/anaf_decl/d205_xml.rs` (`build_d205_xml`) + `commands/dividends.rs::export_d205_official`, gated by
> the bundled `D205Validator.jar`. A golden D205 validates **clean** against ANAF's own validator
> (`Validare fără erori`) — see the gated test `d205_xml::tests::duk_validates_d205`.
>
> **The schema below (§2) is the ORIGINAL design hypothesis. Several points were WRONG and were corrected
> by running the real validator (verify-first, 4 rounds) — see the box immediately below for the
> validator-verified truth. Where they conflict, the box wins.**

## ✅ VALIDATOR-VERIFIED schema (corrections to §2)

Established by running the bundled `java -jar DUKIntegrator.jar -v D205` on golden XML until clean:

- **`D205Validator.jar`**: bundled at `resources/duk/lib/D205Validator.jar`, **349,183 bytes**,
  sha256 `e8b33294d6fe846315f14bfb0b6b46250e79ad78fe935680aaf02f0e64c7d208`, class
  `d205validator.Validator`, built 2026-02-24, from
  `https://static.anaf.ro/static/10/Anaf/update5/D205_36/D205Validator.jar`. The namespace `:v3` maps to
  the validator's **internal v8 schema** (latest). Validates clean **without** a `versiuniCurente.txt`
  entry (like D112). There is **no shippable XSD** — the schema is compiled into the jar.
- **Root `<declaratie205>`**: attrs `xmlns`, `luna="12"`, `an`, `d_rec`, **`cui`** (declarant CUI — NOT
  `cif`), **`adresa`** (REQUIRED), `den`, `nume_declar`, `prenume_declar`, `functie_declar`,
  `totalPlata_A`. **There is NO `version` attribute** (the `version="1.0"` in the XML prolog is just the
  XML declaration). `totalPlata_A = nrben + Tcastig + Tpierd + T_VB + T_GAR + Tbaza + Timp` (the zero
  totals don't change it → `nrben + Tbaza + Timp` for dividends).
- **`<sect_II>` is SELF-CLOSING** (a recap, NOT a wrapper around benef). Required attrs: `tip_venit="08"`,
  `nrben`, **`Tcastig`, `Tpierd`, `T_VB`, `T_GAR`** (all REQUIRED, = `0` for dividends), `Tbaza`, `Timp`.
- **`<benef>` rows are SIBLINGS** — direct children of `<declaratie205>`, AFTER the `<sect_II>` recap (NOT
  nested inside it); each self-identifies via `tip_venit1`. Each benef needs **`id_inreg`** (1-based
  sequential registration id, the unique key — REQUIRED) + `tip_venit1="08"`, `tip_plata="2"`, `Rezid`,
  `cifR` (CNP), `den1`, `baza1`, `imp1`, `divid_D`, `divid_P`. Money = whole-lei N15 integers.
- **Canonical clean shape** (verified):
  ```xml
  <declaratie205 xmlns="mfp:anaf:dgti:d205:declaratie:v3" luna="12" an="2025" d_rec="0"
                 cui="13548146" adresa="…" den="…" nume_declar="…" prenume_declar="…"
                 functie_declar="Administrator" totalPlata_A="11001">
    <sect_II tip_venit="08" nrben="1" Tcastig="0" Tpierd="0" T_VB="0" T_GAR="0" Tbaza="10000" Timp="1000"/>
    <benef id_inreg="1" tip_venit1="08" tip_plata="2" Rezid="1" cifR="1900101410011" den1="…"
           baza1="10000" imp1="1000" divid_D="10000" divid_P="9000"/>
  </declaratie205>
  ```

---

## 1. Why D205 (and why not D100)

Dividend withholding tax has **two** ANAF reporting channels:

| Channel | Cadence | XML | DUK validation | Status in app |
|---|---|---|---|---|
| **D100** | monthly, 25th of month after payment | ❌ none (ANAF design — PDF inteligent / Soft A&J only) | ❌ none exists | ✅ computed + surfaced (informational row in the D100 view) |
| **D205** | **annual**, informative, per beneficiary | ✅ `declaratie205` v3 | ✅ `D205Validator.jar` (J9.0.5) | ❌ **this document** |

So D205 is the dividend channel where we *can* (and should) ship a real, DUK-gated official XML export —
exactly like D300/D394/D406/D112 already do. D100 can never get a DUK gate (no validator exists); that is
why the D100 view shows dividends only as an **informational** reminder.

**Deadline:** annual — **last day of February** of the year following the income year, shifted to the next
working day (so for 2025 income the legal term lands on **2 March 2026**, because 28 Feb 2026 is a Saturday).
Reuse `db::payroll::is_working_day` for the shift, exactly like `saft::d406_deadline`.

---

## 2. Schema (`d205_2025_v3.xsd`) — byte-verified

- **Root element:** `<declaratie205>`
- **Namespace:** `mfp:anaf:dgti:d205:declaratie:v3`
- **Schema version attribute:** `version="1.00"` (the model/version string the validator pins on)
- **Encoding:** UTF-8, no BOM (same as the other ANAF emitters)
- **All data are XML ATTRIBUTES**, not child elements (this is the key structural difference from
  D300/bilanț, which use child elements — see §6).
- **Money is whole-lei `N(15)` INTEGERS** — emit `i64` via `crate::anaf_decl::round_lei(...)`, **never** a
  2-dp decimal. (Contrast: D112/D300 use whole lei too, but the dividend *engine* stores `tax_amount` as a
  2-dp string — it must be rounded to whole lei for D205.)

### Three-level nesting

```
<declaratie205  …header attributes… >          ← 1 per declaration
  <sect_II tip_venit="08" …recap attributes… >  ← 1 per income-type (08 = dividende)
    <benef …beneficiary attributes… />           ← 1 per beneficiary (per CNP)
    <benef … />
    …
  </sect_II>
  <sect_II tip_venit="…" …>  …                    ← other income types, if ever added
</declaratie205>
```

### `<declaratie205>` header attributes

| Attr | Meaning | Value / rule |
|---|---|---|
| `luna` | reporting month | **MUST = 12** (D205 is annual) |
| `an` | income year | `≥ 2025` (the schema floor); e.g. `2025` |
| `d_rec` | declaration type | `0` = initial, `1` = rectificative |
| `cif` | declarant fiscal code | the company CUI (digits only, no `RO`) |
| `den` | declarant name | company legal name (`trunc` to schema max) |
| `nume_declar` / `prenume_declar` / `functie_declar` | signer identity | from company/declarant settings (as D112) |
| `totalPlata_A` | grand total | **computed**, see formula below |

### `<sect_II>` recap attributes (one per income type)

| Attr | Meaning |
|---|---|
| `tip_venit` | income-type code — **`08` = dividende** |
| `nrben` | number of `<benef>` rows in this section |
| `Tbaza` | Σ `baza1` over the section's beneficiaries |
| `Timp` | Σ `imp1` over the section's beneficiaries |

(Other recap accumulators `Tcastig`, `Tpierd`, `T_VB`, `T_GAR` exist in the schema for other income
types and are `0`/absent for dividends.)

### `<benef>` attributes (one per beneficiary, dividends)

| Attr | Meaning | Rule for dividends |
|---|---|---|
| `tip_venit1` | income-type (repeated on the row) | `08` |
| `tip_plata` | payment type | `2` = **final/definitivă** |
| `Rezid` | residence | `1` (resident — forced for the SME case) |
| `cifR` | beneficiary fiscal id | **CNP, N(13), mod-11 valid** (or CUI for a legal-person beneficiary) |
| `den1` | beneficiary name | `trunc` to schema max |
| `baza1` | tax base | dividend gross attributable to this beneficiary (whole lei) |
| `imp1` | tax withheld | `baza1 × rate` (whole lei) — see rate note |
| `divid_D` | dividends **distributed** (GROSS) | whole lei (= `baza1`) |
| `divid_P` | dividends **paid** (NET = gross − tax) | whole lei, derived as **`baza1 − imp1`** — per OPANAF 154/2024 "dividende plătite" are NET sums paid to the shareholder, not gross |

**Rate is NOT a field.** The validator derives the expected rate from `tip_venit=08` + `an`:
**16 % for 2026**, **10 % for 2025** (and the 2025-interim transitional case), **8 % for 2024**. The
emitter must compute `imp1` with the same `dividend_tax_rate(...)` the engine already uses, and the value
must match what the validator recomputes — otherwise it errors. One `<benef>` per `(tip_venit=08, cifR)`
pair: **aggregate all of a beneficiary's dividends for the year into a single row**.

### `totalPlata_A` formula (verified against the validator)

```
totalPlata_A = Σ over all <sect_II> of ( nrben + Tcastig + Tpierd + T_VB + T_GAR + Tbaza + Timp )
```

For a dividends-only declaration this reduces to `Σ (nrben + Tbaza + Timp)`. Compute it last, after every
section is built; the validator recomputes and rejects a mismatch.

---

## 3. Worked example (one resident beneficiary, 2025 income, 10 %)

```xml
<?xml version="1.0" encoding="UTF-8"?>
<declaratie205 xmlns="mfp:anaf:dgti:d205:declaratie:v3" version="1.00"
               luna="12" an="2025" d_rec="0"
               cif="41927384" den="ANDREI CONSULTING SRL"
               nume_declar="Andrei" prenume_declar="Popescu" functie_declar="Administrator"
               totalPlata_A="11001">
  <sect_II tip_venit="08" nrben="1" Tbaza="10000" Timp="1000">
    <benef tip_venit1="08" tip_plata="2" Rezid="1"
           cifR="1900101410011" den1="Ion Gheorghe"
           baza1="10000" imp1="1000" divid_D="10000" divid_P="9000"/>
  </sect_II>
</declaratie205>
```

`totalPlata_A = nrben(1) + Tbaza(10000) + Timp(1000) = 11001`.

---

## 4. DUK gate — bundling `D205Validator.jar`

The dispatch machinery already works: `run_duk(provider, DeclKind::D205, xml)` →
`validation::run_java_validator` runs `java -jar DUKIntegrator.jar -v D205 <xml> <result>` → DUKIntegrator
loads `lib/D205Validator.jar` → class `d205validator.Validator`. Three small wiring steps:

1. **Add the validator jar** to `src-tauri/resources/duk/lib/`:
   - `D205Validator.jar` (J9.0.5, ~349 KB, class `d205validator.Validator`)
   - `D205Pdf.jar` **only if** DUK must also emit the signed PDF (not needed for validation-only)
   - Source: `https://static.anaf.ro/static/10/Anaf/update5/D205_36/`
   - `DUKIntegrator.jar`, the generic `Validator.jar`, `bcprov`/`bcmail`/iText are **reused unchanged**.
2. **Pin the version** in `src-tauri/resources/duk/config/versiuniCurente.txt` — add a line matching the
   shipped jar, e.g. `D205;J9.0.5;P5.0.1`. (NB: D112 needed **no** such line — it validated clean without
   one — so confirm empirically whether D205 needs it by running the bundled DUK on a golden XML first,
   exactly as we did for D112. Add the line only if the validator reports a version mismatch.)
3. **Extend `DeclKind`** (`src/anaf_decl/mod.rs`): add `D205` variant + `as_duk_type(D205) => "D205"`.

The whole jlink JRE / DUKIntegrator harness (`duk/mod.rs`, `validation.rs`) is untouched — it's
declaration-agnostic.

> **Verify-first rule (from D112):** before making the DUK gate *blocking*, run the bundled
> `java -jar DUKIntegrator.jar -v D205 <golden.xml> <result>` end-to-end and confirm `D205Validator`
> loads and returns "Validare fără erori". Only then wire it as a hard gate in the export command.

---

## 5. Schema-version row + vendored XSD

Add a `SchemaVersion` row in `src/anaf_decl/version.rs::schema_versions()` (same shape as the D300/D406
rows):

```rust
SchemaVersion {
    decl: DeclKind::D205,
    valid_from: d(2025, 1, 1),
    valid_to: None,                 // open-ended until ANAF publishes a v4
    namespace: "mfp:anaf:dgti:d205:declaratie:v3",
    root_element: "declaratie205",
    schema_label: "D205 v3 (≥2025)",
    duk_type: "D205",
}
```

Vendor the XSD at `src-tauri/tools/anaf/d205_2025_v3.xsd` (next to `Ro_SAFT_Schema_v249.xsd`) so the
`xmllint --schema` layer (`validation::validate_with_xsd`) can run as an offline pre-check before DUK —
same belt-and-suspenders pattern as SAF-T.

---

## 6. Emitter shape — mirror the NESTED emitters, NOT flat D300

D205 is **3-level nested with attributes**. The flat child-element emitters (D300) are the wrong template;
mirror `bilant_xml` / `saft` (nested), using the `anaf_decl::xml.rs` writer:

- `new_writer()` → `start_elem(w, name)` / `end_elem(w, name)` → `finish(w) -> String`
- `trunc(s, max_chars)` for length-bounded text fields (`den`, `den1`)
- `crate::anaf_decl::round_lei(decimal) -> i64` for every money attribute (whole-lei N15)

**Helper gap to fill:** the current `xml.rs` helpers (`write_text_elem`, `write_decimal_elem`) write
**child elements**. D205 needs elements *with attributes*. Add a small attribute-aware helper, e.g.:

```rust
/// Open an element with attributes: `<name k1="v1" k2="v2" …>` (values XML-escaped).
pub fn start_elem_attrs(w: &mut XmlWriter, name: &str, attrs: &[(&str, &str)]) -> AppResult<()> { … }
/// Self-closing element with attributes: `<name … />` (for <benef/>).
pub fn empty_elem_attrs(w: &mut XmlWriter, name: &str, attrs: &[(&str, &str)]) -> AppResult<()> { … }
```

Keep escaping consistent with the existing writer. `<benef>` is self-closing; `<declaratie205>` and
`<sect_II>` open/close around their children.

### Suggested module layout

- `src/anaf_decl/d205_xml.rs` — pure emitter: `build_d205_xml(header, beneficiaries) -> AppResult<String>`
  + a `#[ignore]` dump test (`dump_d205`) like `dump_standard_d112`, for the verify-first DUK run.
- `commands/declarations.rs` (or a new `commands/d205.rs`) — the Tauri command
  `export_d205_official(app, state, params, skip_duk_override) -> OfficialExportResult`, **identical
  shape to `export_saft_official` / the new `export_d112_xml`**: build XML → temp file →
  `run_duk(BundledProvider, DeclKind::D205, tmp)` → `duk_gate_allows_write(...)` → write on pass.
- Frontend: a binding `exportD205Official(...) -> OfficialExportResult` + a Dividende-page (or Declarații)
  action that surfaces DUK `issues` via `PreflightPanel` + the "exportă oricum" override — reuse the exact
  pattern just shipped for D112 (`Payroll.tsx` `runD112` + `D112Modal`).

---

## 7. PREREQUISITE (gating step) — structured beneficiary on the dividend model

**This is the blocker; do it first.** D205 needs, per beneficiary: **CNP (`cifR`, N13 mod-11) + name
(`den1`) + residence (`Rezid`)**. The `dividends` table today stores only a **free-text `shareholder`**
(`src/db/dividends.rs`: `shareholder: Option<String>`), which cannot populate `cifR` (a mod-11-validated
CNP is mandatory — the validator rejects a missing/invalid one).

Required data-model work before the emitter is viable:

1. **Migration** — add `beneficiary_cnp TEXT`, `beneficiary_name TEXT`, `beneficiary_resident INTEGER`
   (default 1) to `dividends` (a new `migrations/00NN_*.sql`); keep `shareholder` for back-compat /
   display, or backfill `beneficiary_name` from it.
2. **Model + CRUD** — extend `Dividend` / `DividendInput` (validate the CNP with the existing CNP guard
   used by D112; reuse `anaf_decl::d112`'s CNP validation).
3. **UI** — `src/pages/Dividends.tsx`: add CNP + name (+ resident) fields to the entry form.
4. **Aggregation** — for a given `an`, group the year's dividends by `beneficiary_cnp`, summing
   `gross → divid_D`/`baza1` and `tax_amount → imp1` (rounded to whole lei), producing one
   `<benef>` per CNP. `divid_P` (dividends paid) is NOT summed from a stored field — it is the **NET**
   amount, derived at emit time as `baza1 − imp1` (OPANAF 154/2024: amounts paid to the shareholder are
   net of the withheld dividend tax), which keeps the document identity `divid_P = baza1 − imp1` exact.

Alternatively introduce a first-class "shareholders" concept (CNP/name/resident) and link dividends to it
— cleaner long-term, more work. Either way, **`cifR` cannot be synthesized from free text** — this is the
hard prerequisite.

---

## 8. Implementation checklist (in order)

- [ ] **Prereq:** migration + model + UI for structured beneficiary (CNP/name/resident) on dividends (§7).
- [ ] Vendor `d205_2025_v3.xsd` under `tools/anaf/`.
- [ ] `DeclKind::D205` + `as_duk_type` arm (§4.3).
- [ ] `SchemaVersion` row for D205 in `version.rs` (§5).
- [ ] `xml.rs`: attribute-aware element helpers (§6).
- [ ] `d205_xml.rs`: `build_d205_xml` (nested, attributes, whole-lei, `totalPlata_A`) + `#[ignore]` dump test.
- [ ] **Verify-first:** bundle `D205Validator.jar`; run the bundled DUK `-v D205` on the dump → "fără erori";
      add a `versiuniCurente.txt` line only if a version mismatch appears.
- [ ] `#[cfg]`-gated test: `run_duk(D205)` on the golden XML returns `passed` (like `duk_validates_standard_d112`).
- [ ] `export_d205_official` command (OfficialExportResult + DUK gate + `skip_duk_override`).
- [ ] Frontend binding + Dividende/Declarații action with `PreflightPanel` block + override (mirror D112).
- [ ] Gate green (`scripts/verify-local.sh`) + per-step commits on a feature branch.

---

## 9. Invariants to preserve

- **Bundle id `com.lucaris.efactura` + `build.rs` license salt MUST NOT change.**
- Money attributes: whole-lei `i64` via `round_lei` — never 2-dp.
- `luna` MUST be `12`; `an ≥ 2025`.
- `imp1` must equal the validator's recomputed `baza1 × rate(08, an)` — reuse `dividend_tax_rate`.
- `cifR` must be a mod-11-valid CNP (or a valid CUI for legal-person beneficiaries).
- DUK gate identical to `export_saft_official` (graceful when no runtime; blocks only on real ERRORS;
  ATT warnings pass).
