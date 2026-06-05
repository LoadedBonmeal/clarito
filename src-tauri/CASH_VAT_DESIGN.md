# TVA la încasare (Cash VAT) — implementation design

Deep-research-derived spec (Cod fiscal art. 282 alin. (3)-(8), art. 297, art. 319;
OUG 111/2013; OUG 8/2026; ECJ C-169/12; OMFP 1802/2014). This is the build plan for the
cash-VAT regime. **Status: SPEC ready; implementation pending (tested slices below).**

## Why this is a feature, not a fix
A correct cash-VAT regime **rewrites the core D300 collected/deductible computation**:
VAT exigibility is deferred from the invoice date to the **collection date**, proportionally
on each (partial) receipt. The audit's "90-day auto-exigibilization" premise is **outdated** —
that obligation was repealed (OUG 111/2013, after ECJ C-169/12 *TNT Express*). There is **no**
90-day forced collection; uncollected output VAT stays in 4428 indefinitely.

## Authoritative rules
- **Eligibility**: optional regime; turnover plafon **5.000.000 lei** (2026-03-01..2026-12-31),
  **5.500.000 lei** from 2027-01-01 (was 4.5M). Opt in/out via form **097** + ANAF public
  *Registrul persoanelor care aplică TVA la încasare* (RPATVAÎ).
- **Exigibility**: on full/partial collection by ANY means (cash, transfer, offset/compensare,
  assignment/cesiune, payment instruments). Partial: `vat_exigibil = receipt × rate/(100+rate)`.
- **Exclusions (art. 282 alin. (6))** — normal exigibility (invoice date) applies, post straight
  to 4427/4426: reverse-charge (art. 307(2)-(6)/331), VAT-exempt, special regimes (art. 311-313
  margin), affiliated parties (art. 7 pct. 26). Also out of scope by nature: intra-EU/export/
  import, and cash B2C retail (exigible on the spot anyway).
- **Buyer side (art. 297(2)/(3))**: deduction on a purchase **from a cash-VAT supplier** is
  deferred to **payment**, even if the buyer is not on cash VAT — driven by the supplier's
  RPATVAÎ status. Reverse-charge/import/intra-EU acquisitions deduct immediately.
- **Invoice (art. 319)**: mandatory mention **"TVA la încasare"** (UBL note / BT-118) when the
  operation is under the regime; suppress for excluded operations.

## Schema (migrations)
- `companies`: `cash_vat INTEGER NOT NULL DEFAULT 0`, `cash_vat_start TEXT`, `cash_vat_end TEXT`,
  `vat_period TEXT DEFAULT 'monthly'`.
- `contacts`: `cash_vat INTEGER NOT NULL DEFAULT 0` (supplier RPATVAÎ status), `cash_vat_checked_at TEXT`.
- `invoices` / `received_invoices`: `cash_vat_applies INTEGER` (computed snapshot), `exclusion_reason TEXT`.
- New `vat_settlement_event(id, invoice_id, payment_id, event_date, vat_exigibil_bani, rate,
  side TEXT('collected'|'deductible'), method TEXT)` — **single source for D300**. Money in
  integer bani (i64).

## Exigibility engine (pure fn, money in bani, half-away-from-zero)
- `cash_vat_applies(invoice)` = company.cash_vat AND issue_date ∈ [start,end] AND place=RO AND
  not exempt AND not reverse-charge AND not special-regime AND partner not affiliated AND not
  intra-EU/import/export. Else `exclusion_reason` set → accrual (invoice-date) exigibility.
- On each payment, allocate to (base+VAT) pro-rata; release VAT = `paid × rate/(100+rate)`;
  the final payment trues-up the residual to clear 4428 to exactly zero (no bani drift).

## Posting templates (GL)
- Sale, cash-VAT, non-excluded — @invoice: `D 4111 / C 707(+70x)` net, `C 4428.colectat` VAT.
  @collection: `D 5121/5311 / C 4111`, then `D 4428.colectat / C 4427` for the released VAT
  (+ settlement-event row side='collected', dated pay_date).
- Purchase from cash-VAT supplier — @invoice: `D 3xx/6xx / C 401`, `D 4428.deductibil`.
  @payment: `D 401 / C 5121`, then `D 4426 / C 4428.deductibil` for the paid share.
- Excluded lines → straight to 4427/4426 at invoice date.
- Storno/credit note: target the correct bucket by parent settlement state — unsettled →
  reverse 4428; settled → adjust 4427/4426; partial → split proportionally.

## D300 routing
- Collected rows (rd.9/10/11) ← settlement events `side='collected'` in period, grouped by rate
  (NOT invoice lines by issue_date). Deductible rows ← `side='deductible'`. Anything still in
  4428 is excluded by construction. Period granularity = company.vat_period.
- Prior-period base adjustments (art. 287) → rd.16 regularizări (gate on a period lock).

## Plafon monitor
Rolling 12-month CA (excl. fixed-asset/intangible disposals); on crossing the date-applicable
plafon, flag mandatory exit (097 reminder, deadline 20th of the following month) and stop
applying cash VAT to NEW invoices from the next period (in-flight 4428 balances finish their
lifecycle). No 90-day job.

## Build slices (each: implement → `verify-local.sh` green → DUKIntegrator on a cash-VAT D300
## fixture → commit)
1. Schema + company/contact flags + settings UI toggle.
2. `cash_vat_applies` decision + exclusion matrix (pure, unit-tested).
3. `vat_settlement_event` population from payments + the proportional release engine (unit tests:
   full collection clears to zero; Σ releases == invoice VAT; excluded never settle).
4. D300 event-driven collected/deductible routing (re-route `compute_d300` / `d300_vat_totals`);
   reconcile against the existing reconcile + D394.
5. GL 4428.colectat/.deductibil postings + storno-against-settlement.
6. Invoice "TVA la încasare" mention (UBL BT-118 + PDF), suppressed for excluded ops.
7. Buyer-side deferred deduction (needs supplier-payment tracking on received invoices).
8. Plafon monitor + 097 workflow reminders.

## Test requirement
Must validate a generated cash-VAT D300 against **DUKIntegrator** (needs Java + a cash-VAT
fixture) before shipping — a wrong exigibility routing mis-states the decont.
