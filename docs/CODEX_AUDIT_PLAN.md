# efactura-desktop — Full Audit Plan for Codex

> **App path:** `/Users/cris/Projects/efactura-desktop`
> **Stack:** Tauri 2.0 · Rust backend · React 19 + TypeScript · SQLite · TanStack Router/Query
> **Run backend check:** `cargo check --manifest-path /Users/cris/Projects/efactura-desktop/src-tauri/Cargo.toml`
> **Run frontend check:** `cd /Users/cris/Projects/efactura-desktop && npx tsc --noEmit`
> **Run full build:** `cd /Users/cris/Projects/efactura-desktop && cargo tauri build`

This plan is self-contained. Every task lists exact paths, the bad pattern (with a grep where possible),
the concrete fix, and a verification command. Execute tasks top to bottom within an area. After each fix,
run the area's verification command before moving on. Do not batch-commit unrelated areas.

## Critical constraints Codex must NEVER violate
- AppState field is `.db` (NOT `.pool`). Access the pool via `state.db` (see `src-tauri/src/state.rs`).
- ANAF tokens must NEVER be logged or written to disk in plaintext. Tokens live in the OS keychain only
  (`src-tauri/src/anaf/keychain.rs`). Logging `company_id` is fine; logging token values is forbidden.
- Money math ONLY with `rust_decimal::Decimal` — NEVER f64/f32 for arithmetic. Rounding is `.round_dp(2)`.
- UBL XML output must have a UTF-8 BOM (`\u{FEFF}`, bytes `EF BB BF`) prepended (see `ubl/generator.rs`).
- Use `uuid::Uuid::now_v7()` (NOT `new_v4()`). The helper is `crate::db::models::new_id()`.
- SQL queries must use parameterized placeholders (`?1`, `?2`, …) — never interpolate user values into SQL.
  Interpolating *constant* column lists (e.g. `SELECT {SELECT_INVOICE} …`) is allowed; values never are.
- Do NOT change the `identifier` `com.lucaris.efactura`, the keychain service name `efactura`, or the
  CIUS-RO `CustomizationID` string. These are wire/format contracts.

---

## Area 1: Security Audit

### Task 1.1 — Scope the Tauri capability permissions
**What to check:** `src-tauri/src/capabilities/default.json` (path is `src-tauri/capabilities/default.json`).
It currently grants broad `*:default` sets including `fs:default`, `shell:default`, `process:default`.
**What bad looks like:**
```
grep -nE '"(fs|shell|process):default"' /Users/cris/Projects/efactura-desktop/src-tauri/capabilities/default.json
```
`fs:default` and `shell:default` are far wider than this app needs. The app only reads/writes inside
`$APPDATA` (archive, db, backups) plus user-selected files via the dialog plugin; it never needs raw shell
exec from the webview.
**What to fix:**
1. Replace `"fs:default"` with scoped permissions: `"fs:allow-read-file"`, `"fs:allow-write-file"` limited via
   a `fs:scope` entry to `$APPDATA/**` and `$APPDATA/archive/**`. Remove blanket access to `$HOME/**`.
2. Remove `"shell:default"` entirely unless a concrete `shell` call exists. Verify with
   `grep -rn "plugin_shell\|Shell\|\.shell()" /Users/cris/Projects/efactura-desktop/src-tauri/src` and
   `grep -rn "plugin-shell" /Users/cris/Projects/efactura-desktop/src`. If unused, also drop
   `tauri-plugin-shell` from `Cargo.toml` and `.plugin(tauri_plugin_shell::init())` from `lib.rs`.
3. Keep `opener:default` (used for opening the archive folder / browser).
**Verification command:**
`cargo check --manifest-path /Users/cris/Projects/efactura-desktop/src-tauri/Cargo.toml` then
`cd /Users/cris/Projects/efactura-desktop && cargo tauri build --debug` (capability schema is validated at build).

### Task 1.2 — Confirm ANAF tokens are keychain-only (no plaintext fallback)
**What to check:** `src-tauri/src/anaf/keychain.rs`, `src-tauri/src/anaf/oauth.rs`, `src-tauri/src/commands/anaf.rs`.
Tokens are stored via `keyring::Entry::new("efactura", company_id)` — this is correct (OS keychain).
**What bad looks like:** any token written to SQLite, `tauri-plugin-store`, or a file. Run:
```
grep -rniE "access_token|refresh_token" /Users/cris/Projects/efactura-desktop/src-tauri/src | grep -viE "keychain|oauth|TokenBundle|//|expires"
grep -rn "set_password\|get_password" /Users/cris/Projects/efactura-desktop/src-tauri/src/anaf/keychain.rs
```
Then confirm no token reaches the store plugin:
`grep -rni "token" /Users/cris/Projects/efactura-desktop/src/lib | grep -i store`.
**What to fix:** if any token persistence outside the keychain is found, delete it and route through
`TokenBundle::save/load/delete`. Today this appears clean — if so, no code change; record "verified, no
plaintext token storage" in the commit/PR notes.
**Verification command:**
`grep -rniE "access_token|refresh_token" /Users/cris/Projects/efactura-desktop/src-tauri/src/db /Users/cris/Projects/efactura-desktop/src-tauri/src/commands | grep -v keychain` must return nothing.

### Task 1.3 — Confirm no raw SQL string interpolation of values
**What to check:** all `src-tauri/src/db/*.rs`. Dynamic SQL exists in `db/invoices.rs::list` and
`db/received.rs`/`db/contacts.rs` — WHERE clauses are built with numbered placeholders and `.bind()`.
**What bad looks like:** a user value pushed into the SQL string instead of bound. Run:
```
grep -rnE "format!\(\"(SELECT|INSERT|UPDATE|DELETE)" /Users/cris/Projects/efactura-desktop/src-tauri/src/db
grep -rn "query(&format!" /Users/cris/Projects/efactura-desktop/src-tauri/src
```
Inspect each hit: the only interpolated tokens must be constant column lists (`SELECT_INVOICE`,
`SELECT_COLUMNS`), placeholder strings (`?{n}`), or static `WHERE` fragments — never a value from a
`filter`/input struct.
**What to fix:** if any value is interpolated, convert it to a bound parameter. Current state looks safe;
if confirmed, record "verified, all values parameterized".
**Verification command:**
`cargo check --manifest-path /Users/cris/Projects/efactura-desktop/src-tauri/Cargo.toml` and manual review
of each grep hit (expected hits are in `invoices.rs:203,216,235`, `received.rs:93,121`, `contacts.rs:88,111`,
`companies.rs:114,124`, `license.rs:26`, `notifications.rs:68`, `certificates.rs:75`).

### Task 1.4 — Fix path traversal in `import_invoice_xml` and `change_archive_location`
**What to check:** `src-tauri/src/commands/import.rs` (the `import_invoice_xml` command, ~line 324–455) and
`src-tauri/src/commands/archive.rs::change_archive_location` (~line 205–231).
**What bad looks like:**
```
grep -n "app_data_dir: String\|PathBuf::from(&app_data_dir)\|new_path: String\|Path::new(&new_path)" /Users/cris/Projects/efactura-desktop/src-tauri/src/commands/import.rs /Users/cris/Projects/efactura-desktop/src-tauri/src/commands/archive.rs
```
`import_invoice_xml` accepts `app_data_dir: String` *from the frontend* and does
`PathBuf::from(&app_data_dir).join(...)` then `std::fs::write`. A malicious/buggy caller can write outside
the app sandbox. `change_archive_location` accepts an arbitrary `new_path` and creates/copies into it.
**What to fix:**
1. In `import_invoice_xml`, stop trusting the frontend-supplied `app_data_dir`. Derive it inside Rust from
   `app.path().app_data_dir()?` (the command already takes `app: AppHandle` — if not, add it). Remove the
   `app_data_dir` parameter from the command signature and from `src/lib/tauri.ts` (`importData.invoiceXml`)
   and its caller in `src/components/shared/CsvImportModal.tsx`.
2. After building any target path, canonicalize the parent and assert it stays under the app data dir:
   ```rust
   let base = app.path().app_data_dir()?;
   let canon = archive_dir.canonicalize().unwrap_or(archive_dir.clone());
   if !canon.starts_with(&base) { return Err(AppError::Validation("Cale invalidă".into())); }
   ```
3. For `change_archive_location`, keep it (user explicitly picks a folder via dialog) but reject paths
   containing `..` components: `if new_dir.components().any(|c| c == std::path::Component::ParentDir) { return Err(...) }`.
**Verification command:**
`cargo check --manifest-path /Users/cris/Projects/efactura-desktop/src-tauri/Cargo.toml` and
`cd /Users/cris/Projects/efactura-desktop && npx tsc --noEmit`.

### Task 1.5 — Enforce company ownership on every invoice-mutating/reading command
**What to check:** `src-tauri/src/commands/anaf.rs`, `src-tauri/src/commands/ubl.rs`,
`src-tauri/src/commands/invoices.rs`. Only `anaf_submit_invoice` checks
`if invoice.company_id != company_id` (anaf.rs ~line 122). Confirm:
```
grep -rn "company_id != \|company_id ==" /Users/cris/Projects/efactura-desktop/src-tauri/src/commands
```
**What bad looks like:** commands that take both `company_id` and `invoice_id` (or derive company from the
invoice) without verifying the relationship: `anaf_check_invoice_status` (anaf.rs ~273) takes `company_id`
and `invoice_id` but never asserts the invoice belongs to that company. This is a privilege/data-scoping gap
in a multi-company app.
**What to fix:** in `anaf_check_invoice_status`, after `db_invoices::get`, add the same guard used in
`anaf_submit_invoice`:
```rust
if invoice.company_id != company_id {
    return Err(AppError::Validation("Factura nu aparține companiei selectate.".into()));
}
```
For `generate_invoice_xml`/`generate_invoice_pdf`/`set_invoice_status`/`delete_invoice` (which derive the
company from the invoice and don't accept a separate `company_id`), no cross-company guard is needed, but add
a brief code comment stating the company is derived from the invoice row so future edits don't introduce a
mismatch.
**Verification command:**
`grep -n "company_id != company_id\|aparține companiei" /Users/cris/Projects/efactura-desktop/src-tauri/src/commands/anaf.rs`
should show the guard in both `anaf_submit_invoice` and `anaf_check_invoice_status`; then
`cargo check --manifest-path /Users/cris/Projects/efactura-desktop/src-tauri/Cargo.toml`.

### Task 1.6 — Tighten the Content Security Policy
**What to check:** `src-tauri/tauri.conf.json` → `app.security.csp`.
Current value: `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: asset: https://asset.localhost; connect-src ipc: http://ipc.localhost https://webservicesp.anaf.ro https://api.anaf.ro; font-src 'self' data:; object-src 'none'; base-uri 'none'`.
**What bad looks like:** `style-src 'unsafe-inline'` and a missing `frame-ancestors`. `script-src` is already
`'self'` (good — no `unsafe-eval`). The OAuth flow hits `https://logincert.anaf.ro` but that happens in the
*system browser* (see `oauth.rs::open_browser`), not the webview, so it does NOT need to be in `connect-src`.
**What to fix:**
1. Add `frame-ancestors 'none'` and `form-action 'self'` to the CSP.
2. `style-src 'unsafe-inline'` is required by Tailwind v4 injected styles and inline `style={{}}` props used
   heavily across pages (e.g. `InvoiceNew.tsx`). Keep it but add a code comment in `tauri.conf.json` is not
   possible (JSON); instead document the reason in the PR. Do not remove it (removing it will break the UI).
**Verification command:**
`cd /Users/cris/Projects/efactura-desktop && cargo tauri build --debug` (config is validated), then launch and
confirm styling is intact.

### Task 1.7 — Dependency vulnerability audit
**What to check:** Rust crates in `src-tauri/Cargo.toml`, JS deps in `package.json`.
**What bad looks like:** known CVEs (advisory-db hits).
**What to fix / how to run:**
- Rust: `cargo install cargo-audit` (once), then
  `cargo audit --file /Users/cris/Projects/efactura-desktop/src-tauri/Cargo.lock`.
  For each advisory: bump the crate to the patched version in `Cargo.toml`; if no fix exists, document the
  advisory ID and why it is not exploitable (e.g. unused code path).
- JS: this repo uses pnpm (see `build.beforeDevCommand: "pnpm dev"`). Run
  `cd /Users/cris/Projects/efactura-desktop && pnpm audit --prod`. Apply `pnpm update <pkg>` for fixable
  advisories; record un-fixable ones.
- Do NOT blindly run `pnpm audit fix` / major version bumps on React 19, Tauri 2, or TanStack — pin to the
  nearest patched minor and re-run `npx tsc --noEmit`.
**Verification command:**
`cargo audit --file /Users/cris/Projects/efactura-desktop/src-tauri/Cargo.lock` exits 0 (or with documented
exceptions) and `cd /Users/cris/Projects/efactura-desktop && pnpm audit --prod`.

---

## Area 2: Rust Backend Correctness

### Task 2.1 — Eliminate `unwrap()`/`expect()` in runtime paths
**What to check:** all `src-tauri/src/**`. Run:
```
grep -rn "\.unwrap()\|\.expect(" /Users/cris/Projects/efactura-desktop/src-tauri/src
```
**What bad looks like:** a panic-on-error in a request path. Known hits:
- `src/lib.rs` (the final `.expect("error while running tauri application")` — acceptable, it is the top-level
  startup; leave it).
- `src/background/mod.rs` (one hit — inspect; background tasks must not panic the runtime).
- `src/ubl/rocius_rules.rs` date parse uses `.parse().unwrap_or(0)` — that's `unwrap_or`, safe (not a panic).
**What to fix:** replace any genuine `.unwrap()`/`.expect()` in commands, db, anaf, ubl, background (other than
the `lib.rs` startup line) with `?` returning `AppResult<T>`, or `.unwrap_or(...)` / `if let`/`match` for
non-fatal cases. The background-task hit must degrade gracefully (log + continue), never panic.
**Verification command:**
`grep -rn "\.unwrap()\|\.expect(" /Users/cris/Projects/efactura-desktop/src-tauri/src | grep -v "src/lib.rs"`
should return only `unwrap_or`/`unwrap_or_else`/`unwrap_or_default` (which are safe) — confirm each remaining
hit is one of those, then `cargo check --manifest-path /Users/cris/Projects/efactura-desktop/src-tauri/Cargo.toml`.

### Task 2.2 — Make money storage and UBL serialization use Decimal end-to-end
**What to check:** `src-tauri/src/db/invoices.rs` (the `Invoice` and `LineItem` structs store
`subtotal_amount: f64`, `vat_amount: f64`, `total_amount: f64`, `unit_price: f64`, `vat_rate: f64`,
`quantity: f64`) and `src-tauri/src/ubl/generator.rs` (`fmt_amount(v: f64)` and `format_decimal_2(v: f64)` at
the bottom — these `format!("{:.2}", v)` directly on f64).
**What bad looks like:**
```
grep -n "f64" /Users/cris/Projects/efactura-desktop/src-tauri/src/db/invoices.rs
grep -n "fn fmt_amount\|fn format_decimal_2\|: f64" /Users/cris/Projects/efactura-desktop/src-tauri/src/ubl/generator.rs
```
The DB column type is `REAL` (SQLite has no decimal type), so the *storage* f64 is a platform constraint, BUT
the XML amounts are produced by `format!("{:.2}", f64_value)` directly — this is binary-float rounding on the
serialization boundary and can emit e.g. `0.00` vs `0.01` drift relative to the line-level Decimal math done
in `invoices::create`. The totals must be byte-for-byte consistent with `cac:TaxTotal` sums or ANAF rejects.
**What to fix:**
1. In `ubl/generator.rs`, change `fmt_amount` and `format_decimal_2` to route through Decimal before
   formatting, so rounding is decimal not binary:
   ```rust
   fn fmt_amount(v: f64) -> String {
       use rust_decimal::Decimal;
       Decimal::try_from(v).unwrap_or(Decimal::ZERO).round_dp(2).to_string()
   }
   ```
   (and the same for `format_decimal_2`, but `.round_dp(2)` for amounts; for `vat_rate`/`quantity` keep 2 dp).
   Note `Decimal::to_string()` does not pad trailing zeros — if ANAF requires exactly 2 decimals, format as
   `format!("{:.2}", Decimal)` which the crate supports.
2. Verify the `TaxTotal` grouping (`write_tax_total`) already sums via Decimal (it does — `Decimal::from_str(&fmt_amount(...))`).
   After step 1 the inputs become decimal-rounded, removing the f64 drift.
3. Do NOT change the struct field types to a non-`f64` type unless you also update every `query_as`/`bind`
   call and the `serde-float` behavior — that is a larger refactor and out of scope here. The fix is the
   *serialization* path.
**Verification command:**
`cargo check --manifest-path /Users/cris/Projects/efactura-desktop/src-tauri/Cargo.toml`, then generate an XML
for a seeded invoice and confirm `LegalMonetaryTotal/PayableAmount` == sum of line totals (see Area 6, Task 6.2).

### Task 2.3 — Confirm UUID v7 everywhere
**What to check:** `src-tauri/src/db/models.rs::new_id` and `src-tauri/src/anaf/oauth.rs::random_hex`.
**What bad looks like:**
```
grep -rn "new_v4\|Uuid::new" /Users/cris/Projects/efactura-desktop/src-tauri/src
```
**What to fix:** none expected — `new_id()` uses `now_v7()` and `oauth.rs` uses `now_v7()`. If any `new_v4()`
appears, replace with `uuid::Uuid::now_v7()`. Record "verified, all UUIDs are v7".
**Verification command:**
`grep -rn "new_v4" /Users/cris/Projects/efactura-desktop/src-tauri/src` returns nothing.

### Task 2.4 — Verify multi-step writes are transactional
**What to check:** `src-tauri/src/db/invoices.rs::create` and `set_status` (use `pool.begin()` … `tx.commit()`
— good), and `src-tauri/src/commands/anaf.rs::anaf_submit_invoice`.
**What bad looks like:** `anaf_submit_invoice` performs several sequential `sqlx::query(...).execute(pool)`
calls (`UPDATE ... xml_path`, `UPDATE ... QUEUED`, then `mark_submitted`, then `INSERT invoice_events`) each
on `pool`, not a transaction, with `.ok()` swallowing errors. The network upload sits between them, so a
partial failure leaves the invoice in `QUEUED` with no upload id. This is acceptable by design (status reflects
in-flight state and the background poller reconciles) — but the silent `.ok()` on the DB writes hides real
errors.
**What to fix:** keep the QUEUED-before-upload pattern (correct for crash recovery), but replace the
`.execute(pool).await.ok()` swallows with proper `?`/`map_err(AppError::Database)?` so a DB failure surfaces
instead of being lost. Do NOT wrap the network call inside a DB transaction (would hold a connection across I/O).
**Verification command:**
`grep -n "\.ok();" /Users/cris/Projects/efactura-desktop/src-tauri/src/commands/anaf.rs` — the DB-write hits in
`anaf_submit_invoice` should be gone (replaced by `?`), then
`cargo check --manifest-path /Users/cris/Projects/efactura-desktop/src-tauri/Cargo.toml`.

### Task 2.5 — Migration additivity (SQLite < 3.35 safety)
**What to check:** `src-tauri/migrations/0001_initial.sql`, `src-tauri/migrations/0002_payment_means.sql`, and
any new migration. `0002` is `ALTER TABLE invoices ADD COLUMN payment_means_code TEXT NOT NULL DEFAULT '30'`
— additive, safe.
**What bad looks like:**
```
grep -rniE "DROP COLUMN|RENAME COLUMN|DROP TABLE|ALTER COLUMN" /Users/cris/Projects/efactura-desktop/src-tauri/migrations
```
`DROP COLUMN` requires SQLite ≥ 3.35; `bundled` sqlx ships a modern SQLite, but to be safe and reversible,
migrations must remain additive (ADD COLUMN with DEFAULT, CREATE TABLE/INDEX IF NOT EXISTS).
**What to fix:** none expected. If a future migration needs to drop/rename, use the table-rebuild pattern
(create new table, copy, drop old, rename) inside a transaction — never a bare `DROP COLUMN`. Add a comment
to any new migration stating it is additive.
**Verification command:**
`grep -rniE "DROP COLUMN|RENAME COLUMN" /Users/cris/Projects/efactura-desktop/src-tauri/migrations` returns
nothing; `cargo check` (migrations are validated by `sqlx::migrate!` at compile time via `db/pool.rs`).

---

## Area 3: Frontend Correctness & UX

### Task 3.1 — Add a React error boundary around the router
**What to check:** `src/App.tsx`, `src/router.tsx`, `src/main.tsx`. Confirm none exists:
```
grep -rniE "errorboundary|componentDidCatch|getDerivedStateFromError|errorComponent" /Users/cris/Projects/efactura-desktop/src
```
**What bad looks like:** zero hits — an uncaught render error white-screens the whole desktop app with no
recovery.
**What to fix:** add a class `ErrorBoundary` component (`src/components/shared/ErrorBoundary.tsx`) implementing
`getDerivedStateFromError` + `componentDidCatch` that renders a Romanian fallback ("A apărut o eroare
neașteptată") with a "Reîncarcă" button calling `window.location.reload()`. Wrap `<RouterProvider>` in
`src/App.tsx`:
```tsx
<QueryClientProvider client={queryClient}>
  <ErrorBoundary>
    <RouterProvider router={router} />
  </ErrorBoundary>
  <Toaster />
</QueryClientProvider>
```
Optionally also set TanStack Router's `defaultErrorComponent` in `createRouter` (`src/router.tsx`) for
per-route errors.
**Verification command:**
`cd /Users/cris/Projects/efactura-desktop && npx tsc --noEmit`, then temporarily throw in a page to confirm the
fallback renders (revert the throw before committing).

### Task 3.2 — Handle `isLoading`/`isError` for every `useQuery`
**What to check:** all `src/pages/*.tsx`. Many `useQuery` results destructure only `data` (e.g.
`InvoiceNew.tsx` lines 46, 52, 58, 86 use `const { data } = useQuery(...)` with no `isLoading`/`error`). Find
all usages:
```
grep -rn "useQuery(" /Users/cris/Projects/efactura-desktop/src/pages
```
**What bad looks like:** a query whose loading/error is never surfaced, so the page renders with stale/empty
data and the user never learns a fetch failed.
**What to fix:** for the primary data query on each page (the one the page can't render without), destructure
`isLoading` and `isError`/`error` and render a `<Skeleton>` (exists at `src/components/ui/skeleton.tsx`) while
loading and an inline error banner on failure. Secondary/enrichment queries (e.g. `nextNumber`, `testMode` in
`InvoiceNew.tsx`) may keep silent fallbacks. Prioritize: `Invoices.tsx`, `InvoiceDetail.tsx`,
`InvoiceNew.tsx`, `InvoiceEdit.tsx`, `Received.tsx`, `Companies.tsx`, `Dashboard.tsx`, `Reports.tsx`.
**Verification command:**
`cd /Users/cris/Projects/efactura-desktop && npx tsc --noEmit` and manual: throttle/disconnect to see the
loading + error states.

### Task 3.3 — Validate required invoice fields before save
**What to check:** `src/pages/InvoiceNew.tsx` (`saveDraftMutation`, lines 118–162) and
`src/pages/InvoiceEdit.tsx`.
**What bad looks like:** `InvoiceNew.tsx` checks `activeCompanyId`, `contactId`, and `lines.length` inside
`mutationFn` but does NOT validate per-line fields. A line with empty `name`, `quantity <= 0`, or
`unitPrice < 0` is sent to the backend and only rejected later by `rocius_rules` after a round-trip. Also the
"Modalitate de plată" panel (`InvoiceNew.tsx` lines 540–587) renders a `<select defaultValue="ot">` and an
IBAN/reference input that are NOT wired to state — they look editable but are dead controls.
**What to fix:**
1. Before `api.invoices.createDraft`, validate each line client-side: non-empty `name`, `quantity > 0`,
   `unitPrice >= 0`, `vatRate` ∈ {0,5,9,19}. Collect messages and `throw new Error(messages.join("; "))` (the
   existing `onError` already shows them in the red banner at lines 229–237).
2. Either wire the "Modalitate de plată" select + IBAN/reference inputs to state and into the payload, or
   remove them to avoid implying they affect the invoice. The functional payment-means selector is the
   separate one at lines 596–609 (`paymentMeansCode`) — keep that one.
**Verification command:**
`cd /Users/cris/Projects/efactura-desktop && npx tsc --noEmit`; manual: try to save a draft with an empty line
name and confirm the client blocks it before any IPC call.

### Task 3.4 — Make obvious mutations optimistic where safe
**What to check:** `src/pages/InvoiceDetail.tsx`, `src/pages/Notifications.tsx`, `src/pages/Received.tsx`.
**What bad looks like:** `notifications.markRead` / `markAllRead` and `received.updateStatus` invalidate and
refetch (`grep -n "markRead\|updateStatus\|invalidateQueries" src/pages/Notifications.tsx src/pages/Received.tsx`)
causing a visible flicker for an action whose result is certain.
**What to fix:** for `markNotificationRead`/`markAllRead` add `onMutate` optimistic update (set `isRead: true`
in the cached list, snapshot for rollback in `onError`) per the TanStack Query optimistic pattern. Do NOT make
ANAF submit/status mutations optimistic — those depend on a remote result.
**Verification command:**
`cd /Users/cris/Projects/efactura-desktop && npx tsc --noEmit`; manual: marking a notification read updates the
badge instantly with no flash.

### Task 3.5 — Keyboard + accessibility on interactive elements
**What to check:** all `src/pages/*.tsx` and `src/components/**`. Run:
```
grep -rn "onClick=" /Users/cris/Projects/efactura-desktop/src/pages | grep -v "<button\|<a "
grep -rcn "aria-label\|htmlFor" /Users/cris/Projects/efactura-desktop/src
```
**What bad looks like:**
- Non-button elements with `onClick` and no keyboard handler. Example: `InvoiceNew.tsx` line 489
  `<tr className="line-add-row" onClick={addLine}>` — a clickable row that is not focusable and has no
  `onKeyDown`. Only one `onKeyDown` exists in the entire `src/` tree (the global Ctrl+S handler in
  `InvoiceNew.tsx`).
- Inputs without an associated `<label htmlFor>` or `aria-label`. In `InvoiceNew.tsx` the line-item table
  inputs (lines 419–459) have no labels/aria — screen readers announce nothing.
**What to fix:**
1. Convert clickable `<tr>`/`<div>` actions to `<button>` where layout allows, or add
   `role="button" tabIndex={0} onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") addLine(); }}`.
2. Add `aria-label` to icon-only buttons (`btn-icon` trash button line 478) and to the unlabeled line-item
   inputs (e.g. `aria-label="Cantitate"`, `aria-label="Preț unitar"`).
**Verification command:**
`cd /Users/cris/Projects/efactura-desktop && npx tsc --noEmit`; manual: Tab through the invoice form and
confirm every control is reachable and the add-line action fires on Enter.

---

## Area 4: Performance

### Task 4.1 — Resolve the virtual-list fixed vs dynamic height contradiction
**What to check:** `src/pages/Invoices.tsx`. Run:
```
grep -n "useVirtualizer\|estimateSize\|measureElement" /Users/cris/Projects/efactura-desktop/src/pages/Invoices.tsx
```
**What bad looks like:** `estimateSize: () => 32` (line ~106) declares a fixed 32px row, but the row element
also has `ref={rowVirtualizer.measureElement}` (line ~487), which forces per-row dynamic measurement on every
render. With a fixed row height the `measureElement` ref is wasted work (layout thrash on large lists) and the
two signals conflict.
**What to fix:** the invoice rows are uniform height — keep `estimateSize: () => 32` and REMOVE
`ref={rowVirtualizer.measureElement}` from the row. Ensure the row's CSS height is exactly 32px (set
`height: 32px` / matching line-height on the row class) so the estimate is correct. If rows are genuinely
variable height, do the opposite: keep `measureElement` and make `estimateSize` a best-guess — but for this
list, fixed is correct.
**Verification command:**
`cd /Users/cris/Projects/efactura-desktop && npx tsc --noEmit`; manual: scroll a list of 1000+ seeded invoices
and confirm smooth scrolling with no row jump/reflow.

### Task 4.2 — Scope query invalidations
**What to check:** all `invalidateQueries` calls. Run:
```
grep -rn "invalidateQueries" /Users/cris/Projects/efactura-desktop/src
```
**What bad looks like:** an invalidation with an empty key `queryKey: []` (full cache wipe) — there are none
currently. Borderline-broad keys exist: `Invoices.tsx` uses `["invoices"]` and `["received"]` (string-literal
roots) instead of the typed `queryKeys.invoices.list()`/`queryKeys.received.list()` helpers used elsewhere.
These invalidate the whole entity (acceptable) but bypass the typed key factory and can over-invalidate detail
caches.
**What to fix:** replace the string-literal keys in `Invoices.tsx` (lines ~178, 231, 398) with the typed
`queryKeys.*` helpers from `src/lib/queries.ts` so invalidation scope is consistent and intentional. Do NOT
introduce `queryKey: []` anywhere.
**Verification command:**
`grep -rn 'invalidateQueries({ queryKey: \[\] })' /Users/cris/Projects/efactura-desktop/src` returns nothing,
and `grep -n '\["invoices"\]\|\["received"\]' /Users/cris/Projects/efactura-desktop/src/pages/Invoices.tsx`
returns nothing after the fix; then `npx tsc --noEmit`.

### Task 4.3 — Move XML/PDF/XLSX generation off the async executor
**What to check:** `src-tauri/src/commands/ubl.rs` (`generate_invoice_xml`, `generate_invoice_pdf`),
`src-tauri/src/ubl/generator.rs`, `src-tauri/src/ubl/pdf.rs`, `src-tauri/src/commands/integrations.rs`
(`export_invoices_xlsx`). Run:
```
grep -rn "spawn_blocking\|generate_ubl\|generate_pdf\|rust_xlsxwriter" /Users/cris/Projects/efactura-desktop/src-tauri/src/commands
```
**What bad looks like:** `generate_invoice_pdf` calls the synchronous `generate_pdf(&input)` (CPU-bound
`printpdf` work) directly inside an `async` Tauri command, and `generate_invoice_xml` calls `generate_ubl`
plus a synchronous `std::fs::write` — both block the tokio worker thread. For large invoices/exports this
stalls other async commands.
**What to fix:** wrap the CPU-bound + blocking-fs portions in `tokio::task::spawn_blocking`:
```rust
let bytes = tauri::async_runtime::spawn_blocking(move || generate_pdf(&input))
    .await
    .map_err(|e| AppError::Pdf(e.to_string()))??;
```
Apply the same to `generate_pdf`, the `generate_ubl` + `fs::write` block in `generate_invoice_xml`, and the
`rust_xlsxwriter` workbook save in `export_invoices_xlsx`. The DB fetches stay async; only the
generate-and-write block moves into `spawn_blocking`. Capture the needed owned data (`input`) before the closure.
**Verification command:**
`cargo check --manifest-path /Users/cris/Projects/efactura-desktop/src-tauri/Cargo.toml`; manual: generate a
PDF while a sync is running and confirm the UI stays responsive.

### Task 4.4 — Confirm SQLite indexes on hot filter columns
**What to check:** `src-tauri/migrations/0001_initial.sql`. Run:
```
grep -n "CREATE INDEX" /Users/cris/Projects/efactura-desktop/src-tauri/migrations/0001_initial.sql
```
**What bad looks like:** a frequently filtered column with no index. Current indexes already cover the hot
paths: `idx_invoices_company_status (company_id, status)`, `idx_invoices_issue_date (issue_date)`,
`idx_invoices_anaf_upload`, `idx_lines_invoice`, `idx_contacts_company`, `idx_received_company_status`.
The `invoices::list` query orders by `issue_date DESC, number DESC` and filters on `company_id`, `status`,
`issue_date` — all covered.
**What to fix:** none expected. If you add a new filterable column (e.g. filtering received invoices by
`supplier_cui`), add a matching `CREATE INDEX IF NOT EXISTS` in a NEW additive migration (see Task 2.5).
Record "verified, hot-path indexes present".
**Verification command:**
`grep -c "CREATE INDEX" /Users/cris/Projects/efactura-desktop/src-tauri/migrations/0001_initial.sql` ≥ 12, and
the `EXPLAIN QUERY PLAN` of `invoices::list` (run against the dev db) shows index usage, not `SCAN`.

---

## Area 5: macOS + Windows Build & Installer

### Task 5.1 — macOS bundle signing/entitlements/min OS
**What to check:** `src-tauri/tauri.conf.json` → `bundle.macOS`, and `src-tauri/entitlements.plist`.
Current: `signingIdentity: "-"` (ad-hoc signing — NOT a real Developer ID), `minimumSystemVersion: "12.0"`,
`entitlements: "entitlements.plist"` (exists, contains `allow-jit`, `allow-unsigned-executable-memory`,
`network.client`, `files.user-selected.read-write` — correct for a Tauri WKWebView app).
**What bad looks like:** `signingIdentity: "-"` ships an ad-hoc-signed, un-notarized app — Gatekeeper will warn
users and the auto-updater will be untrusted.
**What to fix:** for distribution, set `signingIdentity` to the real `"Developer ID Application: Lucaris SRL
(TEAMID)"` and add notarization. Since secrets can't live in the repo, document the required env/CI steps in
the PR and leave `"-"` for local dev. Do NOT commit certificates. Keep `entitlements.plist` as-is (verified
correct). Confirm `providerShortName: "LUCARIS"` matches the Apple team's provider short name before
notarization.
**Verification command:**
`cd /Users/cris/Projects/efactura-desktop && cargo tauri build --target aarch64-apple-darwin` produces a
`.app`/`.dmg`; `codesign -dv --verbose=4 <path/to/.app>` shows the expected identity (ad-hoc locally).

### Task 5.2 — Windows installer config
**What to check:** `src-tauri/tauri.conf.json` → `bundle.windows`. Current: `digestAlgorithm: "sha256"`,
`timestampUrl: digicert`, `nsis` block configured (Romanian + English, `installMode: currentUser`).
`bundle.targets` includes `"msi"` and `"nsis"`.
**What bad looks like:** no `certificateThumbprint` set, so the installer is unsigned (SmartScreen warning).
```
grep -n "certificateThumbprint\|signCommand\|webviewInstallMode" /Users/cris/Projects/efactura-desktop/src-tauri/tauri.conf.json
```
(no hits today).
**What to fix:** add `"certificateThumbprint"` under `bundle.windows` for code signing (value supplied via CI
secret, not committed) and document the signing prerequisites in the PR. Optionally set
`bundle.windows.webviewInstallMode` to control WebView2 bootstrapping. Do NOT commit a thumbprint/cert.
**Verification command:**
`cd /Users/cris/Projects/efactura-desktop && cargo tauri build --target x86_64-pc-windows-msvc` (run on a
Windows host or CI) produces signed `.msi`/`.exe`. On macOS this target won't fully link — document that
Windows bundles are produced in CI.

### Task 5.3 — macOS universal binary
**What to check:** `package.json` scripts (`build:mac` uses `--target universal-apple-darwin`) and installed
rust targets.
**What bad looks like:** missing per-arch targets so the universal build fails. Verify:
```
rustup target list --installed | grep -E "aarch64-apple-darwin|x86_64-apple-darwin"
```
Both are installed (confirmed). `tauri.conf.json` `bundle.targets` is `["dmg","msi","nsis"]` (format list,
not arch list — arch is selected via `--target` on the CLI, which is correct).
**What to fix:** none expected. Ensure the documented release command is
`pnpm build:mac` (= `tauri build --target universal-apple-darwin`). If a target were missing, install with
`rustup target add aarch64-apple-darwin x86_64-apple-darwin`.
**Verification command:**
`cd /Users/cris/Projects/efactura-desktop && pnpm build:mac` then
`lipo -info "src-tauri/target/universal-apple-darwin/release/RoFactura"` reports
`x86_64 arm64`.

### Task 5.4 — Windows cross-compile command (documentation)
**What to check:** `package.json` scripts: `build:win-x64` = `tauri build --target x86_64-pc-windows-msvc`,
`build:win-arm` = `... aarch64-pc-windows-msvc`. Both MSVC targets are installed.
**What bad looks like:** assuming a macOS host can produce a Windows MSVC bundle — it cannot (needs the MSVC
toolchain + WebView2). 
**What to fix:** document in the PR that Windows artifacts are built on a Windows runner via
`cargo tauri build --target x86_64-pc-windows-msvc` (and `aarch64-pc-windows-msvc` for ARM). No code change.
**Verification command:**
`grep -n "build:win" /Users/cris/Projects/efactura-desktop/package.json` shows both scripts present.

### Task 5.5 — Auto-updater endpoint and signing key
**What to check:** `src-tauri/tauri.conf.json` → `plugins.updater`. Current:
`pubkey: ""` (EMPTY), `endpoints: ["https://releases.lucaris.ro/efactura/{{target}}/{{arch}}/latest.json"]`.
The plugin is registered in `lib.rs` (`tauri_plugin_updater::Builder::default().build()`).
**What bad looks like:** `"pubkey": ""` — an empty updater public key means update signatures cannot be
verified; the updater is either non-functional or (worse) accepts unsigned updates.
```
grep -n '"pubkey"' /Users/cris/Projects/efactura-desktop/src-tauri/tauri.conf.json
```
**What to fix:** generate an updater keypair with `pnpm tauri signer generate -w ~/.tauri/efactura.key`, set
`plugins.updater.pubkey` to the generated PUBLIC key (safe to commit), and keep the PRIVATE key as a CI secret
used to sign release artifacts. Never commit the private key. Verify the `endpoints` host
(`releases.lucaris.ro`) is HTTPS (it is).
**Verification command:**
`grep -n '"pubkey": ""' /Users/cris/Projects/efactura-desktop/src-tauri/tauri.conf.json` returns nothing after
the fix; `cargo tauri build --debug` validates config.

### Task 5.6 — Required app icon sizes
**What to check:** `src-tauri/icons/` and the `bundle.icon` array in `tauri.conf.json`.
**What bad looks like:** a referenced icon missing on disk. The config references
`32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.icns`, `icon.ico` — all present (confirmed via
`ls src-tauri/icons`). `icon.icns` and `icon.ico` exist for macOS/Windows; PNG ladder present.
**What to fix:** none expected. If `cargo tauri build` complains about a missing size, regenerate the full set
with `pnpm tauri icon src-tauri/icons/icon-source.png` (the source PNG exists at `icons/icon-source.png`).
**Verification command:**
`ls /Users/cris/Projects/efactura-desktop/src-tauri/icons/{32x32.png,128x128.png,128x128@2x.png,icon.icns,icon.ico}`
lists all five; `cargo tauri build --debug` succeeds.

---

## Area 6: Romanian Compliance (ANAF/UBL)

### Task 6.1 — Validate generated XML against the RO_CIUS-UBL schema
**What to check:** the XML produced by `generate_invoice_xml` (command in `src-tauri/src/commands/ubl.rs`,
generator in `src-tauri/src/ubl/generator.rs`). The structural validator
(`src-tauri/src/ubl/validator.rs`) only checks business presence, NOT the XSD.
**What bad looks like:** XML that passes the internal validator but is rejected by ANAF's XSD. The repo does
not currently ship the official `*.xsd` files.
**What to fix:**
1. Download the official UBL 2.1 + CIUS-RO schemas (from the ANAF/mfinante e-Factura distribution) into
   `src-tauri/resources/xsd/` (create the dir). Add the dir to `bundle.resources` if you want it shipped, or
   keep it dev-only for CI validation.
2. Validate a generated sample (write the XML to `/tmp/invoice.xml` from a seeded draft) with:
   ```
   xmllint --noout --schema /Users/cris/Projects/efactura-desktop/src-tauri/resources/xsd/maindoc/UBL-Invoice-2.1.xsd /tmp/invoice.xml
   ```
   Note `xmllint` does not enforce the CIUS-RO Schematron rules — those are covered by `rocius_rules.rs`.
3. Fix any XSD violation by adjusting element order/cardinality in `generator.rs` (UBL is order-sensitive).
**Verification command:**
`xmllint --noout --schema <UBL-Invoice-2.1.xsd> /tmp/invoice.xml` prints `validates`.

### Task 6.2 — Confirm UTF-8 BOM on XML output
**What to check:** `src-tauri/src/ubl/generator.rs` end of `generate_ubl` (lines ~167–170):
```rust
let mut with_bom = String::from("\u{FEFF}");
with_bom.push_str(&xml_string);
```
The BOM is prepended in memory. Then `commands/ubl.rs::generate_invoice_xml` does
`std::fs::write(&path, xml.as_bytes())` and `commands/anaf.rs` does `std::fs::write(&xml_path, xml_string.as_bytes())`.
**What bad looks like:** the BOM dropped before writing, or a re-encode that strips it.
```
grep -n "FEFF\|with_bom\|BOM" /Users/cris/Projects/efactura-desktop/src-tauri/src/ubl/generator.rs
```
**What to fix:** none expected — `\u{FEFF}` encodes to bytes `EF BB BF` in UTF-8 and `.as_bytes()` preserves
it. Add a unit test in `generator.rs` asserting the first three bytes:
```rust
#[test]
fn xml_starts_with_utf8_bom() {
    let xml = generate_ubl(&sample_input());
    assert_eq!(&xml.unwrap().as_bytes()[..3], &[0xEF, 0xBB, 0xBF]);
}
```
(Provide a minimal `sample_input()` helper or gate behind `#[cfg(test)]`.)
**Verification command:**
Generate an XML, then `head -c 3 /tmp/invoice.xml | xxd` shows `efbb bf`; or run the unit test with
`cargo test --manifest-path /Users/cris/Projects/efactura-desktop/src-tauri/Cargo.toml xml_starts_with_utf8_bom`.

### Task 6.3 — Storno invoices: InvoiceTypeCode 381 + BillingReference
**What to check:** `src-tauri/src/ubl/generator.rs` (lines ~74–94) and `src-tauri/src/commands/invoices.rs::storno_invoice`.
Current logic: `let type_code = if input.storno_ref.is_some() { "381" } else { "380" };` and a
`cac:BillingReference/cac:InvoiceDocumentReference/cbc:ID` block is emitted when `storno_ref.is_some()`.
The storno command negates line quantities and writes `STORNO_OF:{full_number}|{reason}` into notes; the
generator extracts the original number from that prefix.
**What bad looks like:** a credit note emitted as `380`, or `381` without a `BillingReference`. Also note the
prefix-parsing differs between `commands/ubl.rs::generate_invoice_xml` (`strip_prefix("STORNO_OF:").map(|orig| orig.to_string())`
— keeps the `|reason` tail) and `commands/anaf.rs::anaf_submit_invoice` (`.split('|').next()` — strips the
reason). The ubl.rs path puts the reason into the BillingReference ID, which is wrong.
**What to fix:** make `generate_invoice_xml` parse the storno ref the same way `anaf_submit_invoice` does —
split on `|` and take the first segment so `cbc:ID` is the bare original invoice number:
```rust
let storno_ref = inv.notes.as_deref().and_then(|n| {
    n.strip_prefix("STORNO_OF:").map(|rest| rest.split('|').next().unwrap_or(rest).to_string())
});
```
Confirm `rule_br_ro_050_storno_needs_billing_ref` and `rule_br_ro_051_storno_lines_negative` in
`rocius_rules.rs` still pass.
**Verification command:**
Generate XML for a storno invoice and grep:
`grep -E "InvoiceTypeCode|BillingReference|<cbc:ID>" /tmp/storno.xml` shows `381`, a `BillingReference`, and
the `cbc:ID` is the original number with NO `|reason` suffix; then
`cargo check --manifest-path /Users/cris/Projects/efactura-desktop/src-tauri/Cargo.toml`.

### Task 6.4 — Enforce valid Romanian VAT rates
**What to check:** `src-tauri/src/ubl/rocius_rules.rs::rule_br_ro_035_line_vat_rates` (lines ~414–447) and the
frontend default in `src/pages/InvoiceNew.tsx` (`vatRate: 19`).
**What bad looks like:** a standard-rate (`S`) line with a rate other than 5/9/19, or a zero-category line
with a non-zero rate. The rule already enforces: category `S` ⇒ rate ∈ {5,9,19}; categories `Z|E|AE|K|G|O` ⇒
rate == 0. Confirm:
```
grep -n "19.0\|9.0\|5.0\|0.0\|vat_rate" /Users/cris/Projects/efactura-desktop/src-tauri/src/ubl/rocius_rules.rs
```
**What to fix:** backend is correct. Add the SAME guard on the frontend so users get immediate feedback: in
`InvoiceNew.tsx`/`InvoiceEdit.tsx` validate `vatRate ∈ {0,5,9,19}` before save (ties into Task 3.3). Note the
historical 0% rate is valid; the standard rate set is {5,9,19} (do not hardcode only 19). Keep the default
`vatRate: 19` for category `S`.
**Verification command:**
`cargo test --manifest-path /Users/cris/Projects/efactura-desktop/src-tauri/Cargo.toml` (if a rule test exists)
or manual: an `S` line at 7% triggers `[BR-RO-035]`; `cd /Users/cris/Projects/efactura-desktop && npx tsc --noEmit`.

### Task 6.5 — CUI format validation (`^(RO)?\d{2,10}$`)
**What to check:** `src-tauri/src/ubl/rocius_rules.rs::rule_br_ro_010_supplier_cui` (lines ~102–120) and the
buyer rules (~162–190). Current supplier check: strips `RO`/`ro`, requires 2–10 ASCII digits — equivalent to
`^(RO)?\d{2,10}$` (case-insensitive prefix).
**What bad looks like:** CUI accepted with letters, wrong length, or a buyer marked VAT-payer whose CUI lacks
the `RO` prefix (rule BR-RO-017 covers this).
```
grep -n "is_ascii_digit\|trim_start_matches(\"RO\")\|digits.len()" /Users/cris/Projects/efactura-desktop/src-tauri/src/ubl/rocius_rules.rs
```
**What to fix:** backend logic is correct and matches the required pattern. Add an equivalent client-side check
when creating/editing a Company (`src/pages/CompanyNew.tsx`, `CompanyEdit.tsx`) and Contact
(`src/pages/Contacts.tsx`) so bad CUIs are caught before save:
`/^(RO)?\d{2,10}$/i.test(cui.trim())`. Record "backend CUI validation verified equivalent to `^(RO)?\d{2,10}$`".
**Verification command:**
`cd /Users/cris/Projects/efactura-desktop && npx tsc --noEmit`; manual: entering `RO12` (valid) passes,
`ROABC`/`RO1` fails on both the form and the rules engine.

### Task 6.6 — Invoice series/number atomicity (no race / no gaps)
**What to check:** `src-tauri/src/db/invoices.rs::create` (lines ~315–333). The number is allocated INSIDE the
transaction: `UPDATE companies SET last_invoice_number = last_invoice_number + 1` then
`SELECT last_invoice_number`, then `full_number = format!("{}-{:04}", series, allocated_number)`. The frontend
`input.number` is intentionally ignored ("numărul real e întotdeauna alocat aici"). There is a
`UNIQUE(company_id, series, number)` constraint (migration 0001 line ~158).
**What bad looks like:** allocating the number outside the transaction, or trusting the client-supplied number
(which `InvoiceNew.tsx` computes as `lastInvoiceNumber + 1` and could collide under concurrency).
```
grep -n "last_invoice_number\|allocated_number\|UNIQUE(company_id" /Users/cris/Projects/efactura-desktop/src-tauri/src/db/invoices.rs /Users/cris/Projects/efactura-desktop/src-tauri/migrations/0001_initial.sql
```
**What to fix:** backend is correct — the increment+read+insert all run on `&mut *tx` within one
`pool.begin()`…`commit()`, and SQLite serializes writers (WAL), so two concurrent creates cannot get the same
number. Verify the SqlitePool is WAL (`db/pool.rs` sets `SqliteJournalMode::Wal` — confirmed). Note: the
`get_next_invoice_number` command used by the UI is a *preview* only and must never be the source of truth —
add a code comment to that effect in `commands/companies.rs`. No functional change expected.
**Verification command:**
`grep -n "Wal\|begin()\|last_invoice_number + 1" /Users/cris/Projects/efactura-desktop/src-tauri/src/db/pool.rs /Users/cris/Projects/efactura-desktop/src-tauri/src/db/invoices.rs`
confirms WAL + in-transaction allocation; optionally write a concurrency test creating N drafts in parallel and
assert N distinct sequential numbers with no gap.

---

## Final pass

After completing all areas, run the full verification suite and confirm all pass:
```
cargo check --manifest-path /Users/cris/Projects/efactura-desktop/src-tauri/Cargo.toml
cargo test  --manifest-path /Users/cris/Projects/efactura-desktop/src-tauri/Cargo.toml
cd /Users/cris/Projects/efactura-desktop && npx tsc --noEmit
cd /Users/cris/Projects/efactura-desktop && cargo tauri build --debug
```
Commit each area as its own focused commit. Do not bundle unrelated changes. For any task marked
"none expected / verified", state the verification result in the commit body rather than making a no-op change.
