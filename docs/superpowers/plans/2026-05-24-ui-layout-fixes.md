# UI Layout Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix six chrome/layout issues in the efactura-desktop ribbon and menubar: solid (not dotted) menubar underlines, ribbon group labels above icons, no horizontal/vertical scroll, fully Romanian UI text, and fluid window resize.

**Architecture:** Pure presentation changes. Edit `src/styles/design.css` (chrome styling + layout grid), `src/components/layout/Ribbon.tsx` (group restructure + label position), and `src/components/layout/MenuBar.tsx` (Romanian text). No state, data, or API changes. The CSS grid (`.app` / `.workspace`) already drives layout via `1fr` and already carries `min-width:0; min-height:0` on flex children — the only true overflow culprit is the ribbon forcing `min-width: max-content`.

**Tech Stack:** React 19 + TypeScript, Tauri 2.0, Vite, pnpm. Type-check via `pnpm exec tsc --noEmit`. No unit-test harness for chrome components — verification is type-check + visual.

> **IMPORTANT — corrections to the briefing context:** The real class names are `.app` (not `.app-shell`), `.workspace` (not `.app-body`), `.content` / `.content-shell` (not `.page-content`), defined in `src/components/layout/AppShell.tsx`. Resize fixes (`min-width:0; min-height:0`) are **already present** on `.workspace`, `.content`, `.content-shell`, and `.content-body` (`overflow:auto`). The dotted underline is `1px dotted` at design.css:285. The brand string "Efactura" appears **twice** in MenuBar.tsx (line 152 and line 105 inside `buildMenus`). The menubar status uses uppercase: `anafStatus.toUpperCase()` at line 194.

---

## File Structure

**Modified files only (no new files):**

- `src/styles/design.css` — menubar underline (line 285), `.ribbon` / `.ribbon-group` / `.ribbon-group-label` / `.ribbon-btn` blocks (lines 401–466). Responsibility: all chrome + layout styling.
- `src/components/layout/Ribbon.tsx` — restructure 5 groups → 3, drop disabled placeholder buttons, move `.ribbon-group-label` before `.ribbon-group-buttons`. Responsibility: ribbon toolbar markup.
- `src/components/layout/MenuBar.tsx` — Romanian text (brand ×2, status label). Responsibility: top menu bar markup.
- `src/pages/Settings.tsx` — one stray English literal (`"Task"`, `"Trial gratuit"`). Responsibility: settings page.

---

## Task 1: Scan for English text (baseline inventory)

**Files:**
- Read-only scan of: `src/pages/*.tsx`, `src/components/**/*.tsx`

- [ ] **Step 1: Run the English-literal scan**

Run (from repo root `/Users/cris/Projects/efactura-desktop`):

```bash
grep -rnoE '"[A-Z][a-z]+ ?[a-z]*"' src/pages src/components \
  | grep -iE 'Save|Cancel|Loading|Error|Success|Submit|Close|Search|Filter|Delete|Edit|Create|Download|Upload|Send|Refresh|Unknown|Draft|Sent|Failed|Pending|Retry|Confirm|Apply|Reset|Clear|Select|Choose|Done|Trial|Task|Backup'
```

Expected findings (already verified — confirm they still match):
- `src/pages/Settings.tsx:179` — `"Trial gratuit"` → change to `"Probă gratuită"`
- `src/pages/Settings.tsx:634` — `<th>Task</th>` → change to `<th>Sarcină</th>`
- `src/components/layout/MenuBar.tsx:194` — `"OK"` (known, fixed in Task 5)

NOT changes (valid Romanian or accepted loanwords — leave as-is): `Status`, `Total`, `Data`, `Standard`, `Tip`, `Email`, `Backup`, `Solo`, `Esc`, `Tab`, `Ctrl`, `Rezultat`, `Emitent`. Key-cap labels (`Esc`, `Tab`, `Ctrl S`, etc.) stay in their canonical form.

- [ ] **Step 2: Record findings**

No code change in this task. Confirm the three actionable strings above. If the scan surfaces additional clearly-English UI strings (e.g. a literal `"Save"` or `"Loading"`), add them to the Task 5 change list before proceeding.

- [ ] **Step 3: No commit** (inventory only — folds into Task 5 commit).

---

## Task 2: Menubar dotted underline → solid line

**Files:**
- Modify: `src/styles/design.css:285`

- [ ] **Step 1: Change the underline style**

FROM:

```css
.menubar-item u { text-decoration: none; border-bottom: 1px dotted currentColor; }
```

TO:

```css
.menubar-item u { text-decoration: none; border-bottom: 1.5px solid currentColor; }
```

- [ ] **Step 2: Verify**

Run:

```bash
grep -n 'menubar-item u' src/styles/design.css
```

Expected: line shows `border-bottom: 1.5px solid currentColor;` (no `dotted`).

- [ ] **Step 3: Commit**

```bash
git add src/styles/design.css
git commit -m "style(menubar): solid underline on tab accelerator letters"
```

---

## Task 3: Move ribbon group labels above icons

This task reorders the label/buttons inside each ribbon group (JSX) and flips the label's separator border (CSS). Task 4 will then prune the groups; doing the reorder first keeps the diffs reviewable.

**Files:**
- Modify: `src/components/layout/Ribbon.tsx` (every `ribbon-group` block, lines 22–86)
- Modify: `src/styles/design.css:431-446` (`.ribbon-group-label`)

- [ ] **Step 1: Reorder label before buttons in all 5 groups (Ribbon.tsx)**

For EACH of the five `<div className="ribbon-group">` blocks, move the `<div className="ribbon-group-label">…</div>` line to be the FIRST child (before `<div className="ribbon-group-buttons">`).

OPERAȚIUNI group — FROM:

```tsx
      <div className="ribbon-group">
        <div className="ribbon-group-buttons">
          <BtnBig icon="plus"      label="Factură nouă"   primary hint="Ctrl+N" onClick={() => navigate({ to: "/invoices/new" })} />
          <BtnBig icon="invoiceIn" label="Primită nouă"   hint="Ctrl+Shift+N"   onClick={() => navigate({ to: "/received" })} />
          <BtnBig icon="storno"    label="Storno"         hint="Ctrl+F9" />
          <BtnBig icon="receipt"   label="Chitanță"       disabled />
          <BtnBig icon="bank"      label="Plată"          disabled />
          <BtnBig icon="users"     label="Contact nou"    onClick={() => navigate({ to: "/contacts" })} />
        </div>
        <div className="ribbon-group-label">Operațiuni</div>
      </div>
```

TO:

```tsx
      <div className="ribbon-group">
        <div className="ribbon-group-label">Operațiuni</div>
        <div className="ribbon-group-buttons">
          <BtnBig icon="plus"      label="Factură nouă"   primary hint="Ctrl+N" onClick={() => navigate({ to: "/invoices/new" })} />
          <BtnBig icon="invoiceIn" label="Primită nouă"   hint="Ctrl+Shift+N"   onClick={() => navigate({ to: "/received" })} />
          <BtnBig icon="storno"    label="Storno"         hint="Ctrl+F9" />
          <BtnBig icon="receipt"   label="Chitanță"       disabled />
          <BtnBig icon="bank"      label="Plată"          disabled />
          <BtnBig icon="users"     label="Contact nou"    onClick={() => navigate({ to: "/contacts" })} />
        </div>
      </div>
```

Apply the identical reorder to the remaining four groups: move `<div className="ribbon-group-label">Sincronizare ANAF</div>`, `<div className="ribbon-group-label">Date</div>`, `<div className="ribbon-group-label">Rapoarte &amp; Declarații</div>`, and `<div className="ribbon-group-label">Instrumente</div>` each to be the first child of its group.

- [ ] **Step 2: Flip the label separator border (design.css)**

FROM (lines 431–446):

```css
.ribbon-group-label {
  height: var(--ribbon-grouplabel-h);
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 10.5px;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-muted);
  padding: 4px 10px 6px;
  text-indent: 0.06em;
  white-space: nowrap;
  border-top: 1px solid var(--border-soft);
  margin-top: 2px;
}
```

TO:

```css
.ribbon-group-label {
  height: var(--ribbon-grouplabel-h);
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 10.5px;
  font-weight: 600;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-muted);
  padding: 4px 10px;
  text-indent: 0.06em;
  white-space: nowrap;
  border-bottom: 1px solid var(--border-soft);
  margin-bottom: 2px;
}
```

(Height math is unchanged — same elements, reordered — so `--ribbon-h` needs no edit.)

- [ ] **Step 3: Verify**

Run:

```bash
pnpm exec tsc --noEmit
grep -n 'border-bottom: 1px solid var(--border-soft)' src/styles/design.css
```

Expected: tsc exits 0 (no errors); grep shows the `.ribbon-group-label` line. Visually, every group label now sits at the TOP of the group with a hairline below it.

- [ ] **Step 4: Commit**

```bash
git add src/components/layout/Ribbon.tsx src/styles/design.css
git commit -m "style(ribbon): move group labels above icon buttons"
```

---

## Task 4: Restructure ribbon for no horizontal scroll

Prune 5 groups → 3 (drop **Date** and **Rapoarte** — both reachable via sidebar + menubar), drop all `disabled` placeholder buttons from the survivors, shrink button width 72→64px, and disable ribbon overflow + the `max-content` floor that forced the bar wider than the window.

**Files:**
- Modify: `src/components/layout/Ribbon.tsx` (remove Date + Rapoarte groups; trim disabled buttons)
- Modify: `src/styles/design.css` (`.ribbon` line 410, `.ribbon-group` line 418, `.ribbon-btn` line 453)

- [ ] **Step 1: Replace the ribbon body with the 3-group version (Ribbon.tsx)**

Replace the entire `return ( … )` JSX (lines 20–88, the `<div className="ribbon">` block produced after Task 3) with:

```tsx
  return (
    <div className="ribbon">
      {/* OPERAȚIUNI */}
      <div className="ribbon-group">
        <div className="ribbon-group-label">Operațiuni</div>
        <div className="ribbon-group-buttons">
          <BtnBig icon="plus"      label="Factură nouă" primary hint="Ctrl+N" onClick={() => navigate({ to: "/invoices/new" })} />
          <BtnBig icon="invoiceIn" label="Primită nouă" hint="Ctrl+Shift+N"   onClick={() => navigate({ to: "/received" })} />
          <BtnBig icon="users"     label="Contact nou"  onClick={() => navigate({ to: "/contacts" })} />
        </div>
      </div>

      {/* SINCRONIZARE ANAF */}
      <div className="ribbon-group">
        <div className="ribbon-group-label">Sincronizare ANAF</div>
        <div className="ribbon-group-buttons">
          <BtnBig icon="cloudUp" label="Trimite ANAF"    hint="F9"     onClick={() => navigate({ to: "/invoices" })} />
          <BtnBig icon="cloudDn" label="Descarcă SPV"    hint="Ctrl+D" onClick={() => navigate({ to: "/received" })} />
          <BtnBig icon="refresh" label="Verifică status" hint="F10"    onClick={() => navigate({ to: "/invoices" })} />
          <BtnBig icon="anaf"    label="Mesaje SPV"      onClick={() => navigate({ to: "/notifications" })} />
        </div>
      </div>

      {/* INSTRUMENTE */}
      <div className="ribbon-group" style={{ flex: 1 }}>
        <div className="ribbon-group-label">Instrumente</div>
        <div className="ribbon-group-buttons">
          <BtnBig icon="command"  label="Comenzi" hint="Ctrl+K" onClick={onOpenPalette} />
          <BtnBig icon="settings" label="Setări"  onClick={() => navigate({ to: "/settings" })} />
        </div>
      </div>
    </div>
  );
```

Note: `location` from `useLocation()` is no longer referenced (the Companii `active={…}` button was in the removed Date group). Remove its declaration to avoid a TS6133 unused-variable error.

FROM (lines 17–18):

```tsx
  const navigate = useNavigate();
  const location = useLocation();
```

TO:

```tsx
  const navigate = useNavigate();
```

And FROM the import (line 8):

```tsx
import { useNavigate, useLocation } from "@tanstack/react-router";
```

TO:

```tsx
import { useNavigate } from "@tanstack/react-router";
```

- [ ] **Step 2: Kill ribbon overflow + max-content floor (design.css)**

`.ribbon` — FROM (line 410):

```css
  overflow-x: auto;
```

TO:

```css
  overflow-x: hidden;
```

`.ribbon-group` — FROM (lines 413–420):

```css
.ribbon-group {
  display: inline-flex;
  flex-direction: column;
  align-items: stretch;
  flex-shrink: 0;
  min-width: max-content;
  position: relative;
}
```

TO:

```css
.ribbon-group {
  display: inline-flex;
  flex-direction: column;
  align-items: stretch;
  flex-shrink: 0;
  position: relative;
}
```

- [ ] **Step 3: Shrink button width 72→64 (design.css)**

`.ribbon-btn` — FROM (line 453):

```css
  width: 72px;
```

TO:

```css
  width: 64px;
```

- [ ] **Step 4: Verify width budget + types**

Run:

```bash
pnpm exec tsc --noEmit
grep -n 'overflow-x: hidden' src/styles/design.css
grep -n 'min-width: max-content' src/styles/design.css || echo "max-content removed OK"
```

Expected: tsc exits 0; `.ribbon` shows `overflow-x: hidden`; the `min-width: max-content` grep prints `max-content removed OK`.

Width budget at the 1024px minimum window (sidebar 224px → 800px for ribbon): Operațiuni 3×64 + gaps/padding ≈ 212px; Sincronizare 4×64 + ≈ 286px; Instrumente fills remaining via `flex:1`. Total well under 800px — no scroll.

- [ ] **Step 5: Commit**

```bash
git add src/components/layout/Ribbon.tsx src/styles/design.css
git commit -m "fix(ribbon): drop placeholder groups/buttons and disable horizontal scroll"
```

---

## Task 5: Romanian text fixes

**Files:**
- Modify: `src/components/layout/MenuBar.tsx` (lines 105, 152, 194)
- Modify: `src/pages/Settings.tsx` (lines 179, 634)

- [ ] **Step 1: Brand name "Efactura" → "RoFactura" (MenuBar.tsx, both occurrences)**

Line 152 — FROM:

```tsx
        <span>Efactura</span>
```

TO:

```tsx
        <span>RoFactura</span>
```

Line 105 — FROM:

```tsx
      { type: "row", icon: "info",     label: `Despre Efactura • v${version}` },
```

TO:

```tsx
      { type: "row", icon: "info",     label: `Despre RoFactura • v${version}` },
```

- [ ] **Step 2: Status label "OK" → "Activ" (MenuBar.tsx line 194)**

FROM:

```tsx
        ANAF · SPV {anafStatus === "ok" ? "OK" : anafStatus.toUpperCase()}
```

TO:

```tsx
        ANAF · SPV {anafStatus === "ok" ? "Activ" : anafStatus.toUpperCase()}
```

(The `warn`/`err` branch keeps `anafStatus.toUpperCase()` — those are short status codes, acceptable.)

- [ ] **Step 3: Settings English literals (Settings.tsx)**

Line 179 — FROM:

```tsx
"Trial gratuit"
```

TO:

```tsx
"Probă gratuită"
```

Line 634 — FROM:

```tsx
                      <th>Task</th>
```

TO:

```tsx
                      <th>Sarcină</th>
```

(Plus any additional clearly-English UI strings surfaced in Task 1 Step 2 — apply the same FROM/TO treatment here.)

- [ ] **Step 4: Verify**

Run:

```bash
pnpm exec tsc --noEmit
grep -rn 'Efactura\|"OK"\|Trial gratuit\|>Task<' src/components/layout/MenuBar.tsx src/pages/Settings.tsx || echo "all replaced OK"
```

Expected: tsc exits 0; grep prints `all replaced OK` (none of the old strings remain).

- [ ] **Step 5: Commit**

```bash
git add src/components/layout/MenuBar.tsx src/pages/Settings.tsx
git commit -m "i18n: replace remaining English UI strings with Romanian"
```

---

## Task 6: No vertical scroll + fluid resize

The grid layout (`.app` rows `menubar / ribbon / 1fr / statusbar`, `height:100vh; overflow:hidden`) and the resize guards (`min-width:0; min-height:0` on `.workspace`/`.content`/`.content-shell`; `.content-body { overflow:auto }`) are ALREADY correct in design.css. After Task 4 removed the ribbon's horizontal overflow, the only remaining hardening is ensuring the ribbon row itself cannot push vertical height and that `.workspace` carries `min-width:0`.

**Files:**
- Modify: `src/styles/design.css` (`.ribbon` block; `.workspace` block lines 208–214)

- [ ] **Step 1: Confirm existing guards (read-only check)**

Run:

```bash
grep -nA3 '^.app {' src/styles/design.css
grep -nA6 '^.content {' src/styles/design.css
grep -nA3 '^.content-body {' src/styles/design.css
```

Expected: `.app` has `height: 100vh; overflow: hidden;`; `.content` has `min-width: 0; min-height: 0; overflow: hidden;`; `.content-body` has `overflow: auto;`. If all present, no change needed for those — they already satisfy "no vertical scroll on the frame, internal scroll on pages."

- [ ] **Step 2: Add min-width:0 + overflow guard to `.workspace` (design.css)**

FROM (lines 208–214):

```css
.workspace {
  display: grid;
  grid-template-columns: var(--sidebar-w) 1fr;
  min-height: 0;
  border-top: 1px solid var(--border);
  border-bottom: 1px solid var(--border);
}
```

TO:

```css
.workspace {
  display: grid;
  grid-template-columns: var(--sidebar-w) 1fr;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
  border-top: 1px solid var(--border);
  border-bottom: 1px solid var(--border);
}
```

- [ ] **Step 3: Pin ribbon height so it never grows the row (design.css)**

`.ribbon` — append `height: var(--ribbon-h);` and `flex-shrink: 0;` so the ribbon row stays a fixed band regardless of content. FROM (the `.ribbon` block, after the Task 4 edit):

```css
.ribbon {
  background: var(--bg-toolbar);
  border-bottom: 1px solid var(--border);
  display: flex;
  align-items: stretch;
  padding: var(--ribbon-pad-top) 0 0;
  user-select: none;
  position: relative;
  z-index: 20;
  overflow-x: hidden;
  flex-wrap: nowrap;
}
```

TO:

```css
.ribbon {
  background: var(--bg-toolbar);
  border-bottom: 1px solid var(--border);
  display: flex;
  align-items: stretch;
  padding: var(--ribbon-pad-top) 0 0;
  user-select: none;
  position: relative;
  z-index: 20;
  overflow: hidden;
  flex-wrap: nowrap;
  min-width: 0;
}
```

(`overflow: hidden` now blocks both axes; `min-width: 0` lets the flex row shrink with the window.)

- [ ] **Step 4: Verify**

Run:

```bash
pnpm exec tsc --noEmit
grep -nA4 '^.workspace {' src/styles/design.css
```

Expected: tsc exits 0; `.workspace` shows `min-width: 0;` and `overflow: hidden;`. Manually resize the window narrow/wide and tall/short: the outer frame must not show scrollbars; only `.content-body` (page interior) scrolls.

- [ ] **Step 5: Commit**

```bash
git add src/styles/design.css
git commit -m "fix(layout): harden workspace + ribbon against frame scroll on resize"
```

---

## Task 7: Final verification

**Files:** none (verification only)

- [ ] **Step 1: Type-check the whole project**

Run:

```bash
pnpm exec tsc --noEmit
```

Expected: exits 0, no errors. (Notably no TS6133 for the removed `useLocation`.)

- [ ] **Step 2: Confirm no residual issues**

Run:

```bash
grep -rn 'dotted currentColor' src/styles/design.css || echo "no dotted underline OK"
grep -rn 'min-width: max-content' src/styles/design.css || echo "no max-content floor OK"
grep -rn 'overflow-x: auto' src/styles/design.css | grep -i ribbon || echo "ribbon not auto-scroll OK"
grep -rn 'Efactura' src/ || echo "no Efactura brand OK"
```

Expected: each line prints its `… OK` confirmation (no matches remain).

- [ ] **Step 3: Visual smoke test**

Run:

```bash
pnpm dev
```

Open the app and confirm:
1. Menubar accelerator letters (F, E, O…) have a solid underline, not dots.
2. Ribbon shows 3 groups (Operațiuni / Sincronizare ANAF / Instrumente) with labels ON TOP.
3. No horizontal scrollbar on the ribbon at any window width ≥ 1024px.
4. No vertical scrollbar on the app frame; pages scroll internally.
5. Resizing the window (narrow↔wide, short↔tall) reflows fluidly with no clipped chrome.
6. All visible chrome text is Romanian; status reads "ANAF · SPV Activ".

- [ ] **Step 4: Final commit (only if Step 3 surfaced fixes)**

If the smoke test required no further edits, all commits from Tasks 2–6 already stand and no extra commit is needed. If a fix was made, commit it with a descriptive message, e.g.:

```bash
git add -A
git commit -m "fix(ui): address smoke-test findings for ribbon/menubar layout"
```

---

## Self-Review

**Spec coverage:**
1. Dotted → solid underline → Task 2 ✓
2. Group labels above icons → Task 3 ✓
3. Fully Romanian → Tasks 1 + 5 ✓
4. No horizontal scroll → Task 4 ✓
5. No vertical scroll → Task 6 ✓ (mostly pre-existing; hardened)
6. Dynamic resize → Task 6 ✓ (ribbon `min-width:0` + workspace guards; grid `1fr` already correct)

**Placeholder scan:** No TBD/"handle edge cases"/"similar to Task N". Every code step shows exact FROM/TO. ✓

**Type consistency:** `useLocation` removal (Task 4) matches its only usage being the removed Date group's `active={location.pathname…}`. Class names verified against AppShell.tsx: `.app`, `.workspace`, `.content-shell`, `.content`, `.content-body`. CSS variable `--ribbon-h` left untouched (element set unchanged). ✓
