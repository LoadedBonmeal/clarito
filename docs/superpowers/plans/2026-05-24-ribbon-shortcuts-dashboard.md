# Ribbon, Shortcuts & Dashboard Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix ribbon button label truncation, add OS-aware keyboard shortcut formatting (⌘ on macOS, Ctrl on Windows), add a time-based personalized greeting to the Dashboard, and apply small UI polish fixes.

**Architecture:** Pure frontend changes. A new `src/lib/platform.ts` utility detects macOS via `navigator` and exposes `fmtShortcut(s)` to rewrite "Ctrl+N"-style strings into "⌘N"-style strings. Every place that renders a shortcut string imports and calls `fmtShortcut`. CSS changes live in `src/styles/design.css`. Dashboard greeting is computed inline in `DashboardPage`.

**Tech Stack:** React 19, TypeScript, Vite, TanStack Query/Router, i18next. No unit-test framework is configured in this repo (no vitest/jest, no `test` script in `package.json`). Verification is therefore done with `pnpm build` (runs `tsc && vite build` — catches type and import errors) plus a manual visual check in `pnpm tauri dev`.

**Convention note:** This repo's existing code uses `pnpm`. The lockfile is `pnpm-lock.yaml`. Use `pnpm`, not `npm`/`yarn`.

**Trace check for `fmtShortcut` (read before implementing):**
- `"Ctrl+N"` → `⌘N` (rule `Ctrl\+` → `⌘`)
- `"Ctrl+Shift+D"` → `⌘⇧D` (rule `Ctrl\+Shift\+` → `⌘⇧` runs first)
- `"Ctrl+Alt+C"` → `⌘⌥C` (rule `Ctrl\+Alt\+` → `⌘⌥` runs first)
- `"Ctrl F"` (space form) → `⌘F` (rule `Ctrl\s+` → `⌘`)
- `"Ctrl K Ctrl C"` → `⌘K ⌘C`
- `"Alt+F4"` → `⌥F4`, `"F5"`/`"F9"`/`"G C"`/`"↑↓"` → unchanged (no Ctrl/Alt/Shift token)

On Windows (`isMac === false`) every input is returned verbatim.

---

## Task 1: Create the platform utility

**Files:**
- Create: `src/lib/platform.ts`

- [ ] **Step 1: Create the file with exact contents**

Create `src/lib/platform.ts`:

```ts
/**
 * platform — detecție SO și formatare scurtături de tastatură.
 *
 * Pe macOS afișăm simbolurile native (⌘ ⌥ ⇧); pe Windows/Linux păstrăm
 * forma "Ctrl+N". Detecția folosește `navigator`, deci funcționează atât în
 * Tauri (WebView) cât și în browser.
 */

export const isMac: boolean =
  typeof navigator !== "undefined" &&
  (navigator.platform.toLowerCase().startsWith("mac") ||
    navigator.userAgent.toLowerCase().includes("mac os"));

/**
 * Convertește o scurtătură în forma potrivită SO-ului curent.
 * Pe Windows/Linux returnează șirul neschimbat.
 * Acceptă atât forma cu plus ("Ctrl+F") cât și cea cu spațiu ("Ctrl F").
 * Tastele fără modificator (F5, "G C", ↑↓) trec neschimbate.
 */
export function fmtShortcut(s: string): string {
  if (!isMac) return s;
  return s
    .replace(/Ctrl\+Shift\+/gi, "⌘⇧")
    .replace(/Ctrl\+Alt\+/gi, "⌘⌥")
    .replace(/Ctrl\+/gi, "⌘")
    .replace(/Ctrl\s+/gi, "⌘")
    .replace(/Alt\+/gi, "⌥")
    .replace(/Shift\+/gi, "⇧");
}
```

- [ ] **Step 2: Typecheck the new module compiles**

Run: `cd /Users/cris/Projects/efactura-desktop && pnpm exec tsc --noEmit`
Expected: PASS (no errors). The file is not yet imported anywhere, but `tsc` still type-checks it as part of the project.

- [ ] **Step 3: Commit**

```bash
cd /Users/cris/Projects/efactura-desktop
git add src/lib/platform.ts
git commit -m "feat: add platform detection + fmtShortcut utility"
```

---

## Task 2: Fix ribbon button label truncation (CSS)

**Files:**
- Modify: `src/styles/design.css` (CSS variable block ~lines 84-97, `.ribbon-btn` ~lines 449-490, `.ribbon-group:last-child` ~line 432)

**Why:** `.ribbon-btn` is locked to `height: 64px`. With `padding: 6px 4px 6px`, a 22px icon, a 5px gap, and the 22px group label, the label has only ~25px of usable height while two lines need ~24.15px — under 1px of slack, so the 2nd line clips. Letting the button grow (`height: auto; min-height: 68px`), widening it (72px), and bumping the reserved label height (26px) removes the clip. `.ribbon-group-buttons` already has `align-items: stretch`, so all buttons in a row will match the tallest one automatically.

- [ ] **Step 1: Bump the `--ribbon-label-lines` variable from 24px to 26px**

In `src/styles/design.css` line 88, change:

```css
  --ribbon-label-lines: 24px;
```

to:

```css
  --ribbon-label-lines: 26px;
```

(The `--ribbon-h` calc on lines 93-97 already references `var(--ribbon-label-lines)`, so the row height updates automatically — no separate edit to the calc is required. The spec's "replace 24px ref in calc" is already satisfied because the calc uses the variable, not a literal.)

- [ ] **Step 2: Change `.ribbon-btn` width and height**

In `src/styles/design.css`, inside the `.ribbon-btn` rule (lines 449-468), change:

```css
  padding: 6px 4px 6px;
  width: 64px;
  height: 64px;
```

to:

```css
  padding: 6px 4px 6px;
  width: 72px;
  min-height: 68px;
  height: auto;
```

- [ ] **Step 3: Bump `.ribbon-btn .lbl` min-height to match and add word-break**

In `src/styles/design.css`, inside `.ribbon-btn .lbl` (lines 479-490), change:

```css
  overflow: hidden;
  min-height: 24px;
  width: 100%;
}
```

to:

```css
  overflow: hidden;
  min-height: 26px;
  width: 100%;
  word-break: break-word;
}
```

- [ ] **Step 4: Add `padding-right` to the last ribbon group (Task D #2)**

In `src/styles/design.css` line 432, change:

```css
.ribbon-group:last-child .ribbon-group-buttons { border-right: 0; }
```

to:

```css
.ribbon-group:last-child .ribbon-group-buttons { border-right: 0; padding-right: 10px; }
```

(Existing right padding inside `.ribbon-group-buttons` is `8px` — line 429 `padding: var(--ribbon-btnrow-py) 8px var(--ribbon-btnrow-pb)`. Bumping the last group to `10px` adds the requested ~2px breathing room at the right edge of the ribbon.)

- [ ] **Step 5: Verify build still compiles**

Run: `cd /Users/cris/Projects/efactura-desktop && pnpm build`
Expected: PASS (CSS is not type-checked, but this confirms nothing else broke).

- [ ] **Step 6: Visual check**

Run: `cd /Users/cris/Projects/efactura-desktop && pnpm tauri dev`
Look at the ribbon. Confirm: "Factură nouă", "Primită nouă", "Verifică status" each show both words on two lines with no clipping; buttons in a group are equal height; the right edge of the last group is not tight against the divider/window edge.

- [ ] **Step 7: Commit**

```bash
cd /Users/cris/Projects/efactura-desktop
git add src/styles/design.css
git commit -m "fix: ribbon button labels no longer clip 2nd line"
```

---

## Task 3: Add global `.kbd-hint` style (Task D #1)

**Files:**
- Modify: `src/styles/design.css` (after the `.search .kbd-hint` rule, lines 773-781)

**Why:** `.kbd-hint` is currently only styled when nested under `.search`. Several pages render `<span className="kbd-hint">` outside a `.search` wrapper (verified in Invoices/Contacts/Companies/Received), so those spans are unstyled. Add a global rule with the same look.

- [ ] **Step 1: Add the global rule directly after the existing scoped rule**

In `src/styles/design.css`, find (lines 773-781):

```css
.search .kbd-hint {
  font-family: var(--font-mono);
  font-size: 10.5px;
  color: var(--text-dim);
  border: 1px solid var(--border);
  padding: 1px 5px;
  background: var(--bg);
  border-radius: 3px;
}
```

Replace it with (adds a global `.kbd-hint` selector alongside the scoped one):

```css
.kbd-hint,
.search .kbd-hint {
  font-family: var(--font-mono);
  font-size: 10.5px;
  color: var(--text-dim);
  border: 1px solid var(--border);
  padding: 1px 5px;
  background: var(--bg);
  border-radius: 3px;
}
```

- [ ] **Step 2: Verify build**

Run: `cd /Users/cris/Projects/efactura-desktop && pnpm build`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
cd /Users/cris/Projects/efactura-desktop
git add src/styles/design.css
git commit -m "fix: style .kbd-hint globally, not only inside .search"
```

---

## Task 4: OS-aware shortcuts in MenuBar

**Files:**
- Modify: `src/components/layout/MenuBar.tsx` (imports lines 8-16; `kbd:` values inside `buildMenus`, lines 32-103)

**Why:** All menu accelerators are hardcoded "Ctrl+…"/"Alt+…". Wrapping each with `fmtShortcut` makes them render as ⌘/⌥/⇧ on macOS. F-keys and `G C` pass through unchanged, so wrapping them is harmless.

- [ ] **Step 1: Add the import**

In `src/components/layout/MenuBar.tsx`, after line 16 (`import { api } from "@/lib/tauri";`), add:

```ts
import { fmtShortcut } from "@/lib/platform";
```

- [ ] **Step 2: Wrap every `kbd:` value with `fmtShortcut(...)`**

Edit each `kbd:` literal inside `buildMenus`. Apply these exact replacements (each `old` is unique enough on its line; if an Edit collision occurs, include the surrounding `label:` text for uniqueness):

| Line | Old | New |
|------|-----|-----|
| 32 | `kbd: "Ctrl+N",` | `kbd: fmtShortcut("Ctrl+N"),` |
| 33 | `kbd: "Ctrl+Shift+N",` | `kbd: fmtShortcut("Ctrl+Shift+N"),` |
| 34 | `kbd: "Ctrl+Alt+C",` | `kbd: fmtShortcut("Ctrl+Alt+C"),` |
| 36 | `kbd: "Ctrl+S" }` | `kbd: fmtShortcut("Ctrl+S") }` |
| 37 | `kbd: "Ctrl+Shift+S" }` | `kbd: fmtShortcut("Ctrl+Shift+S") }` |
| 42 | `kbd: "Ctrl+P" }` | `kbd: fmtShortcut("Ctrl+P") }` |
| 44 | `kbd: "Alt+F4",` | `kbd: fmtShortcut("Alt+F4"),` |
| 47 | `kbd: "Ctrl+Z" }` | `kbd: fmtShortcut("Ctrl+Z") }` |
| 48 | `kbd: "Ctrl+Y" }` | `kbd: fmtShortcut("Ctrl+Y") }` |
| 50 | `kbd: "Ctrl+X" }` | `kbd: fmtShortcut("Ctrl+X") }` |
| 51 | `kbd: "Ctrl+C" }` | `kbd: fmtShortcut("Ctrl+C") }` |
| 52 | `kbd: "Ctrl+V" }` | `kbd: fmtShortcut("Ctrl+V") }` |
| 54 | `kbd: "Ctrl+F" }` | `kbd: fmtShortcut("Ctrl+F") }` |
| 55 | `kbd: "Ctrl+K",` | `kbd: fmtShortcut("Ctrl+K"),` |
| 61 | `kbd: "Ctrl+F9" }` | `kbd: fmtShortcut("Ctrl+F9") }` |
| 95 | `kbd: "Ctrl+−" }` | `kbd: fmtShortcut("Ctrl+−") }` |
| 96 | `kbd: "Ctrl+=" }` | `kbd: fmtShortcut("Ctrl+=") }` |
| 98 | `kbd: "Ctrl+Shift+D",` | `kbd: fmtShortcut("Ctrl+Shift+D"),` |
| 103 | `kbd: "Ctrl+/" }` | `kbd: fmtShortcut("Ctrl+/") }` |

Leave the platform-agnostic ones AS-IS (no wrap needed — they are unchanged by `fmtShortcut` either way, so wrapping is optional; for minimal diff, do NOT touch them): line 59 `kbd: "F9"`, line 60 `kbd: "F10"`, line 72 `kbd: "G C"`, line 94 `kbd: "F5"`, line 102 `kbd: "F1"`.

> Tip for the executor: the fastest reliable way is one `Edit` per row from the table above. Do not use `replace_all` on `kbd: "Ctrl` because the trailing strings differ per line.

- [ ] **Step 3: Typecheck**

Run: `cd /Users/cris/Projects/efactura-desktop && pnpm exec tsc --noEmit`
Expected: PASS. If you see "fmtShortcut is declared but never read", you missed wrapping at least one value — re-check the table.

- [ ] **Step 4: Commit**

```bash
cd /Users/cris/Projects/efactura-desktop
git add src/components/layout/MenuBar.tsx
git commit -m "feat: OS-aware shortcut labels in menu bar"
```

---

## Task 5: OS-aware shortcuts in CommandPalette

**Files:**
- Modify: `src/components/layout/CommandPalette.tsx` (imports lines 13-16; `hint: "Ctrl+N"` line 153)

**Why:** The COMMANDS array has one Ctrl-based hint (`"Ctrl+N"`); the `G D`/`G F`/`G R`/`G C` hints and the recent-invoice date hints are platform-agnostic and must stay untouched.

- [ ] **Step 1: Add the import**

In `src/components/layout/CommandPalette.tsx`, after line 16 (`import { queryKeys } from "@/lib/queries";`), add:

```ts
import { fmtShortcut } from "@/lib/platform";
```

- [ ] **Step 2: Wrap the single Ctrl hint**

At line 153, change:

```ts
      hint: "Ctrl+N",
```

to:

```ts
      hint: fmtShortcut("Ctrl+N"),
```

Do NOT touch lines 68/79/90/101 (`G D`/`G F`/`G R`/`G C`) or line 187 (`hint: inv.issueDate`).

- [ ] **Step 3: Typecheck**

Run: `cd /Users/cris/Projects/efactura-desktop && pnpm exec tsc --noEmit`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
cd /Users/cris/Projects/efactura-desktop
git add src/components/layout/CommandPalette.tsx
git commit -m "feat: OS-aware shortcut hint in command palette"
```

---

## Task 6: OS-aware shortcuts in Invoices page

**Files:**
- Modify: `src/pages/Invoices.tsx` (imports lines 12-19; kbd at line 178; kbd-hint at line 243)

- [ ] **Step 1: Add the import**

In `src/pages/Invoices.tsx`, after line 18 (`import { fmtRON } from "@/lib/utils";`), add:

```ts
import { fmtShortcut } from "@/lib/platform";
```

- [ ] **Step 2: Wrap the "Ctrl N" button badge**

At line 178 (inside the `<span className="kbd" …>` on the new-invoice button), change:

```tsx
              Ctrl N
```

to:

```tsx
              {fmtShortcut("Ctrl N")}
```

- [ ] **Step 3: Wrap the search "Ctrl F" hint**

At line 243, change:

```tsx
          <span className="kbd-hint">Ctrl F</span>
```

to:

```tsx
          <span className="kbd-hint">{fmtShortcut("Ctrl F")}</span>
```

Leave the literal navigation hints at lines 512-514 (`↑↓`, `Enter`, `Space`) unchanged.

- [ ] **Step 4: Typecheck**

Run: `cd /Users/cris/Projects/efactura-desktop && pnpm exec tsc --noEmit`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd /Users/cris/Projects/efactura-desktop
git add src/pages/Invoices.tsx
git commit -m "feat: OS-aware shortcuts on Invoices page"
```

---

## Task 7: OS-aware shortcuts in Contacts page

**Files:**
- Modify: `src/pages/Contacts.tsx` (imports lines 9-14; kbd-hint at line 118)

- [ ] **Step 1: Add the import**

In `src/pages/Contacts.tsx`, after line 13 (`import { useAppStore } from "@/lib/store";`), add:

```ts
import { fmtShortcut } from "@/lib/platform";
```

- [ ] **Step 2: Wrap the "Ctrl F" hint**

At line 118, change:

```tsx
          <span className="kbd-hint">Ctrl F</span>
```

to:

```tsx
          <span className="kbd-hint">{fmtShortcut("Ctrl F")}</span>
```

- [ ] **Step 3: Typecheck**

Run: `cd /Users/cris/Projects/efactura-desktop && pnpm exec tsc --noEmit`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
cd /Users/cris/Projects/efactura-desktop
git add src/pages/Contacts.tsx
git commit -m "feat: OS-aware shortcut on Contacts page"
```

---

## Task 8: OS-aware shortcuts in Companies page

**Files:**
- Modify: `src/pages/Companies.tsx` (imports lines 11-14; kbd-hint at line 159; kbd at line 367)

- [ ] **Step 1: Add the import**

In `src/pages/Companies.tsx`, after line 13 (`import { api } from "@/lib/tauri";`), add:

```ts
import { fmtShortcut } from "@/lib/platform";
```

- [ ] **Step 2: Wrap the "Ctrl F" search hint**

At line 159, change:

```tsx
          <span className="kbd-hint">Ctrl F</span>
```

to:

```tsx
          <span className="kbd-hint">{fmtShortcut("Ctrl F")}</span>
```

- [ ] **Step 3: Wrap the "Ctrl K Ctrl C" hint**

At line 367, change:

```tsx
          <span className="kbd">Ctrl K Ctrl C</span> selector rapid companie
```

to:

```tsx
          <span className="kbd">{fmtShortcut("Ctrl K Ctrl C")}</span> selector rapid companie
```

(`fmtShortcut("Ctrl K Ctrl C")` → `⌘K ⌘C` on macOS, unchanged on Windows.)

- [ ] **Step 4: Typecheck**

Run: `cd /Users/cris/Projects/efactura-desktop && pnpm exec tsc --noEmit`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd /Users/cris/Projects/efactura-desktop
git add src/pages/Companies.tsx
git commit -m "feat: OS-aware shortcuts on Companies page"
```

---

## Task 9: OS-aware shortcuts in Received page

**Files:**
- Modify: `src/pages/Received.tsx` (imports lines 13-19; kbd at line 148; kbd-hint at line 213)

**Note:** The `<span className="kbd">` on the sync button (lines 139-149) currently shows `F5` (platform-agnostic). The spec mentions "any 'Ctrl' kbd near line 140" — there is no Ctrl there, only `F5`. `F5` passes through `fmtShortcut` unchanged, so wrapping it is harmless and keeps the pattern uniform. We wrap it for consistency.

- [ ] **Step 1: Add the import**

In `src/pages/Received.tsx`, after line 18 (`import { fmtRON } from "@/lib/utils";`), add:

```ts
import { fmtShortcut } from "@/lib/platform";
```

- [ ] **Step 2: Wrap the "F5" sync-button badge**

At line 148 (the badge text inside the `<span className="kbd" …>`), change:

```tsx
                F5
```

to:

```tsx
                {fmtShortcut("F5")}
```

- [ ] **Step 3: Wrap the "Ctrl F" search hint**

At line 213, change:

```tsx
          <span className="kbd-hint">Ctrl F</span>
```

to:

```tsx
          <span className="kbd-hint">{fmtShortcut("Ctrl F")}</span>
```

Leave the row-action hints at lines 443-445 (`A`, `R`, `↑↓`) unchanged.

- [ ] **Step 4: Typecheck**

Run: `cd /Users/cris/Projects/efactura-desktop && pnpm exec tsc --noEmit`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd /Users/cris/Projects/efactura-desktop
git add src/pages/Received.tsx
git commit -m "feat: OS-aware shortcuts on Received page"
```

---

## Task 10: Dashboard — OS-aware shortcut, breadcrumb fix, and personalized greeting

**Files:**
- Modify: `src/pages/Dashboard.tsx` (imports lines 5-14; greeting vars after line 149; breadcrumb line 155; "Ctrl N" line 399; dash-summary lines 203-243)

This task bundles the three Dashboard changes (Task B item 6, Task C items 1 & 2) because they touch one file and verify together.

- [ ] **Step 1: Add the import**

In `src/pages/Dashboard.tsx`, after line 14 (`import { useAppStore } from "@/lib/store";`), add:

```ts
import { fmtShortcut } from "@/lib/platform";
```

- [ ] **Step 2: Add greeting variables**

In `src/pages/Dashboard.tsx`, line 149 currently reads:

```tsx
  const monthLabel = now.toLocaleDateString("ro-RO", { month: "long", year: "numeric" });
```

Replace it with (adds `greeting` + `todayStr` alongside the existing `monthLabel`):

```tsx
  const monthLabel = now.toLocaleDateString("ro-RO", { month: "long", year: "numeric" });

  const hour = now.getHours();
  const greeting = hour < 12 ? "Bună dimineața" : hour < 17 ? "Bună ziua" : "Bună seara";
  const todayStr = now.toLocaleDateString("ro-RO", {
    weekday: "long",
    day: "numeric",
    month: "long",
    year: "numeric",
  });
```

(`now`, `activeCompany`, `unreadCount`, `rejectedCount`, `thisMonth`, `totalNet`, `totalVat`, `validatedCount`, `fmtRON` are all already defined earlier in this component — verified.)

- [ ] **Step 3: Fix the breadcrumb title**

At line 155, change:

```tsx
          <span className="crumb">Efactura</span>
```

to:

```tsx
          <span className="crumb">RoFactura</span>
```

- [ ] **Step 4: Wrap the "Ctrl N" badge on the empty-state new-invoice button**

At line 399, change:

```tsx
                <span className="kbd" style={{ marginLeft: 6 }}>Ctrl N</span>
```

to:

```tsx
                <span className="kbd" style={{ marginLeft: 6 }}>{fmtShortcut("Ctrl N")}</span>
```

(Leave the `F5` badges at lines 165 and 458 as-is — they are platform-agnostic and out of scope for the greeting bundle. Optional: wrap with `fmtShortcut("F5")` for uniformity; not required.)

- [ ] **Step 5: Replace the `dash-summary` block with the greeting version**

In `src/pages/Dashboard.tsx`, the current `dash-summary` block is lines 203-243:

```tsx
        <div className="dash-summary">
          {activeCompany ? (
            <>
              Companie activă:{" "}
              <span className="b">{activeCompany.legalName}</span>
              {" · "}
            </>
          ) : null}
          {unreadCount > 0 && (
            <>
              <span className="pill">
                <Icon name="bell" size={11} />
                {unreadCount} mesaje neprocesate
              </span>{" "}
            </>
          )}
          {rejectedCount > 0 && (
            <>
              <span className="pill">
                <Icon name="alert" size={11} />
                {rejectedCount}{" "}
                {rejectedCount === 1 ? "factură respinsă" : "facturi respinse"} de ANAF
              </span>{" "}
            </>
          )}
          În luna curentă ai emis{" "}
          <span className="b">{thisMonth.length} facturi</span> totalizând{" "}
          <span className="b tnum">{fmtRON(totalNet + totalVat)} RON</span>
          {thisMonth.length > 0 && (
            <>
              , dintre care{" "}
              <span className="b">{validatedCount} validate</span> de ANAF
              {rejectedCount > 0 && (
                <>
                  {" "}și <span className="neg">{rejectedCount} respinse</span>
                </>
              )}
            </>
          )}
          .
        </div>
```

Replace the entire block with:

```tsx
        <div className="dash-summary">
          <span className="b">
            {greeting}
            {activeCompany ? `, ${activeCompany.legalName}` : ""}.
          </span>{" "}
          Astăzi este {todayStr}.{" "}
          {unreadCount > 0 && (
            <>
              <span className="pill">
                <Icon name="bell" size={11} />
                {unreadCount} mesaje SPV neprocesate
              </span>{" "}
            </>
          )}
          {rejectedCount > 0 && (
            <>
              <span className="pill">
                <Icon name="alert" size={11} />
                {rejectedCount}{" "}
                {rejectedCount === 1 ? "factură respinsă" : "facturi respinse"} de ANAF
              </span>{" "}
            </>
          )}
          În luna curentă ai emis{" "}
          <span className="b">
            {thisMonth.length} {thisMonth.length === 1 ? "factură" : "facturi"}
          </span>{" "}
          totalizând{" "}
          <span className="b tnum">{fmtRON(totalNet + totalVat)} RON</span>
          {thisMonth.length > 0 && (
            <>
              , dintre care <span className="b">{validatedCount} validate</span> de ANAF
              {rejectedCount > 0 && (
                <>
                  {" "}și <span className="neg">{rejectedCount} respinse</span>
                </>
              )}
            </>
          )}
          .
        </div>
```

(Changes vs. original: opening greeting + date replace "Companie activă:"; "mesaje neprocesate" → "mesaje SPV neprocesate"; the invoice count now pluralizes "factură"/"facturi".)

- [ ] **Step 6: Typecheck**

Run: `cd /Users/cris/Projects/efactura-desktop && pnpm exec tsc --noEmit`
Expected: PASS. Watch for "greeting/todayStr declared but never read" (means Step 5 didn't land) or unterminated-JSX errors (means the block replacement was partial).

- [ ] **Step 7: Visual check**

Run: `cd /Users/cris/Projects/efactura-desktop && pnpm tauri dev`
On the Dashboard confirm: breadcrumb reads "RoFactura"; summary opens with a greeting ("Bună dimineața/ziua/seara, <Company>.") and today's date; SPV/rejected pills still appear when counts > 0; the "Ctrl N" badge shows ⌘N on macOS.

- [ ] **Step 8: Commit**

```bash
cd /Users/cris/Projects/efactura-desktop
git add src/pages/Dashboard.tsx
git commit -m "feat: dashboard greeting, RoFactura breadcrumb, OS-aware shortcut"
```

---

## Task 11: Verification sweep (Task D #3 & #4) and full build

**Files:**
- Inspect only: `src/components/layout/StatusBar.tsx`, `src/styles/design.css`

This task confirms the two "check" items in Task D and runs a final full build. No code changes are expected; if the checks below reveal a real issue, fix it inline and note it.

- [ ] **Step 1: Confirm ribbon font-size consistency (Task D #3)**

The button label `.ribbon-btn .lbl` (design.css ~line 480) and the group label `.ribbon-group-label` (~line 438) both use `font-size: 10.5px`. Verify both still read `10.5px` after Task 2's edits.

Run: `cd /Users/cris/Projects/efactura-desktop && grep -n "font-size: 10.5px" src/styles/design.css`
Expected: matches include both the `.ribbon-btn .lbl` and `.ribbon-group-label` rules (plus the `.kbd-hint` rule from Task 3). No change needed — this is a confirmation step. The two ribbon labels use the same family (`--font-ui` inherited from body) and weight differences (`500` vs `600`) are intentional, so rendering is consistent.

- [ ] **Step 2: Confirm StatusBar has no hardcoded English (Task D #4)**

Read `src/components/layout/StatusBar.tsx`. All user-facing strings are Romanian: "conectat", "neautentificat", "Ultima sincronizare", "Mesaje SPV", "noi", "Companie activă", "companii administrate", "RO_CIUS 1.0.1 · RON · ro-RO". `ANAF`, `SPV`, `RON`, `ro-RO`, `v{version}` are proper nouns/codes, not English UI copy.

Run: `cd /Users/cris/Projects/efactura-desktop && grep -niE "connected|active|company|loading|error|search|settings|sync" src/components/layout/StatusBar.tsx`
Expected: only matches inside identifiers/props (e.g. `activeCompanyName`, `activeCompanyId`, `companyColor`, `last_sync_at`, `lastSyncLabel`) — NOT in any rendered string literal. No change needed. If a rendered English string is found, translate it to Romanian and note it in the commit.

- [ ] **Step 3: Full production build**

Run: `cd /Users/cris/Projects/efactura-desktop && pnpm build`
Expected: PASS (`tsc` clean, `vite build` produces `dist/`).

- [ ] **Step 4: Final visual smoke test**

Run: `cd /Users/cris/Projects/efactura-desktop && pnpm tauri dev`
Walk through: Ribbon (no clipped labels), Menu bar (⌘ on macOS), Command palette (⌘N hint), Invoices/Contacts/Companies/Received search hints, Dashboard greeting + breadcrumb. Confirm Windows behaviour is unaffected by reasoning that `isMac === false` returns every shortcut verbatim (or test on a Windows machine if available).

- [ ] **Step 5: Commit any fixes from this sweep (only if changes were made)**

```bash
cd /Users/cris/Projects/efactura-desktop
git add -A
git commit -m "chore: verification sweep — ribbon font + statusbar i18n"
```

If Steps 1-2 produced no changes, skip this commit (do not create an empty commit).

---

## Self-Review

**Spec coverage:**
- Task A (ribbon truncation) → Task 2. Width 64→72, height 64→`min-height:68px; height:auto`, `--ribbon-label-lines` 24→26 (the `--ribbon-h` calc references the variable, so it updates automatically — verified in source), `word-break: break-word` on `.lbl`. ✅
- Task B (platform util + apply everywhere) → Task 1 (create `platform.ts`), Tasks 4-10 (MenuBar, Invoices, Contacts, Companies, Received, Dashboard, CommandPalette). All 7 listed sites covered. ✅
- Task C (greeting + breadcrumb) → Task 10 Steps 2, 3, 5. ✅
- Task D (#1 global kbd-hint → Task 3; #2 last-group padding → Task 2 Step 4; #3 font check → Task 11 Step 1; #4 status-bar i18n check → Task 11 Step 2). ✅

**Placeholder scan:** No "TBD"/"handle edge cases"/"similar to". Every code step shows complete code. ✅

**Type/name consistency:** `fmtShortcut` and `isMac` named identically everywhere; import path `@/lib/platform` matches the created file `src/lib/platform.ts` (repo uses `@/` alias for `src/`, confirmed by existing imports like `@/lib/store`). Dashboard greeting uses only pre-existing in-scope vars (`now`, `activeCompany`, etc.), verified against source lines 93-147. ✅

**Deviations from spec (intentional, noted for the executor):**
- The spec's Task B says "wrap each Ctrl/Alt/Shift one" in MenuBar; F-keys and `G C` are left unwrapped (no behavioural difference, smaller diff).
- Spec Task D #2 said "padding-right: 2px"; the last group already has 8px inner right padding, so the plan sets it to 10px to actually add ~2px, matching the intent ("avoid tight edge").
- No unit tests are written because the repo has no test framework configured; verification is `tsc`/`pnpm build` + manual visual check, stated up front.
