# Accounting Software UI

_Started 2026-05-19 20:52 UTC_

---

## User

Make me the ui for an accountancy app that is similar to SAGA/SMARTBILL/WINMENTOR but more user friendly/spaced/pleasing for the user

## Assistant

_[tool: list_files]_

_[tool: questions_v2]_

## User

Continue with these answers to your questions:

## User

VISUAL TONE: Professional & trustworthy (banks/finance) — this is for Romanian accountants, not consumers. Think SAGA/WinMentor density but modernized.

## User

POWER-USER FEATURES TO SURFACE: Keyboard shortcuts everywhere, Bulk actions on lists, Inline editing in tables, Saved filters/views, Audit trail/activity log. Skip AI assistant (accountants want control, not magic) and Quick-add (redundant with shortcuts).

## User

ONE THING DIFFERENT from SAGA/SmartBill/WinMentor: This app is e-Factura-native from the ground up. SAGA/SmartBill/WinMentor are desktop accounting apps with e-Factura bolted on as an afterthought. Every screen shows real-time ANAF status. Errors from ANAF are translated into concrete inline actions — instead of cryptic "Cod TVA invalid", show "TVA-ul clientului ACME SRL nu corespunde cu ANAF — click pentru a corecta". The user can never accidentally submit an invalid invoice — validation is live, not after-submit. Multi-company is first-class (one accountant managing 15 SRLs), not bolted on.

## User

TWEAKS PANEL: Density (compact/cozy/comfy), Color accent, Light/dark mode, Romanian/English labels.

## User

COPY/LABELS LANGUAGE: Romanian (primary market — Romanian accountants and SRL owners).

## User

LAYOUT (top-to-bottom, Win32 business app structure):

## User

1. MenuBar (h-26px): Fișier / Editare / Operațiuni / Date / Rapoarte / Vizualizare / Ajutor — with keyboard shortcuts displayed (Ctrl+N, Ctrl+S, F5)

## User

2. Ribbon toolbar (h-68px): grouped icon+label buttons (18px icons stacked over 10.5px text), sections separated by vertical dividers with small uppercase section labels at bottom: "OPERAȚIUNI", "SINCRONIZARE", "DATE"

## User

3. Left sidebar (190px): flat module list with 3px color-bar on left edge of active item (Facturi=emerald, Primite=amber, Companii=violet, Contacte=cyan, Rapoarte=rose)

## User

4. Main content area: dense data tables with h-7 rows, tabular numbers, zebra striping, sticky header with subtle gradient

## User

5. Status bar (h-22px): chips separated by vertical dividers — "Conectat | ANAF OK | Sync 12:34 | 2 companii | v0.1.0"

## User

DESIGN TOKENS:

## User

- Font: Segoe UI / Tahoma stack, 12px base, 10-11px for chrome

## User

- Radius: 0 everywhere (sharp business corners — no rounded SaaS cards)

## User

- Primary: deep navy blue oklch(0.4 0.14 245)

## User

- Background: off-white with warm tint, sidebar slightly grey (layered depth)

## User

- Inputs have subtle inset shadow (Windows native feel)

## User

- Subtle gradients only on section headers and toolbar (not flashy)

## User

PAGES TO DESIGN (focus on these 5):

## User

1. DASHBOARD — text summary inline (NOT stat tiles with icons in colored bubbles), then compact table of administered companies, then table of recent invoices with status badges (DRAFT/SUBMITTED/VALIDATED/REJECTED colored consistently)

## User

2. COMPANII (Companies list) — toolbar with search + filters + add button, dense data table with columns: CUI (mono), Denumire, Localitate, Județ, SPV (green check / muted X), Serie, Nr. ultim

## User

3. FACTURĂ NOUĂ (Invoice create) — multi-section form with label-LEFT layout in grid, line items in editable inline table with auto-calc (qty × unit_price + VAT), live RO_CIUS validation panel on the right showing errors with one-click fix actions

## User

4. DETALIU FACTURĂ (Invoice detail) — 60/40 split: left = PDF preview, right = metadata + ANAF events timeline + contextual actions (Trimite, Verifică status, Storno)

## User

5. FACTURI PRIMITE (Received invoices) — table with issuer CUI, name, amount, status (NEW/REVIEWED/APPROVED/REJECTED/ARCHIVED), one-click approve/reject from row

## User

KEY UX DIFFERENTIATORS to show in mockups:

## User

- ANAF error inline with action button: "❌ TVA-ul cumpărătorului nu corespunde — [Corectează]" not just "Cod TVA invalid"

## User

- Multi-company switcher in top-right of toolbar (accountant managing 15 SRLs)

## User

- Live validation while typing in invoice form (red highlight on invalid fields with inline hint, not just on submit)

## User

- Cmd+K command palette mockup overlay (search + actions: Factură nouă, Sincronizează, Adaugă contact)

## User

- Keyboard hints visible in menus (Ctrl+N, Ctrl+S shown in menu items)

## User

AVOID (these make it feel SaaS, not legitimate business):

## User

- Big rounded cards with shadows

## User

- Stat tiles with icons in colored circles (dashboard cliché)

## User

- Centered hero sections, large hero headings

## User

- Generic empty states with cartoon illustrations

## User

- Excessive whitespace — this is a dense info-first app for power users

## User

Questions timed out; go with defaults

## Assistant

_[tool: update_todos]_

_[tool: copy_starter_component]_

_[tool: write_file]_

Got it — Win32 business-app density with a modern modernization layer. Building this as a multi-file React prototype.

_[tool: write_file]_

_[tool: write_file]_

Now I'll build out each module. Starting with icons and data, then chrome, then screens, then app shell.

_[tool: write_file]_

_[tool: write_file]_

_[tool: write_file]_

_[tool: write_file]_

_[tool: write_file]_

_[tool: write_file]_

_[tool: write_file]_

_[tool: grep]_

_[tool: write_file]_

_[tool: done]_

_[tool: save_screenshot]_

Clean load. Let me check the dashboard quickly then fork the verifier.

_[tool: view_image]_

_[tool: fork_verifier_agent]_

Looks solid — proper Win32 chrome, inline ANAF callout, no SaaS cliches. Forking verifier to sweep all screens.Built a Romanian e-Factura accountancy app prototype with full Win32 business-app structure.

**5 working screens** (click sidebar / ribbon to navigate):
- **Privire generală** — inline text summary, KPI strip (no bubble tiles), companies table, live ANAF activity timeline, inline error callout with [Corectează] action
- **Facturi emise** — saved-view tabs, dense table with bulk actions, status badges
- **Facturi primite** — inline approve/reject on row hover, status pipeline (NEW → REVIEWED → APPROVED)
- **Companii** — 15 SRL list with CUI mono, SPV checks, Serie, Nr. ultim
- **Factură nouă** — label-left form, editable inline line items with auto-calc, **right-side live RO_CIUS validation panel** with one-click [Corectează] / [Schimbă în neplătitor] fixes
- **Detaliu factură** — 60/40 PDF preview + metadata + ANAF events timeline + contextual actions

**Key differentiators from SAGA/SmartBill/WinMentor:**
- e-Factura-native: ANAF status visible on every screen (menubar dot, status bar, row badges)
- ANAF errors translated to concrete actions instead of cryptic codes
- Live validation on the right side of the editor — invalid invoices can't be submitted
- Multi-company switcher pinned top-right (15 SRLs sample data)
- **Ctrl+K** command palette (try it)
- Keyboard hints in every menu

**Tweaks panel** (toolbar toggle): density (compact/cozy/comfy), accent color (navy/emerald/burgundy/teal/graphite), dark mode, RO/EN.

The whole thing uses 0-radius corners, Segoe UI, 12px base, layered warm off-white surfaces — deliberately *not* a modern rounded-SaaS look.

