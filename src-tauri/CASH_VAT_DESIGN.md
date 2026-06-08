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

## D300 routing — VERIFIED against OPANAF 174/2026 F300 (Anexa 1 form + Anexa 2 instr.)
Deep-research (4-agent workflow, primary-source read of the official PDF, adversarial pass
**NOT REFUTED**) confirmed:
- **No dedicated cash-VAT operative row.** Deferred output/input VAT flows into the SAME
  per-rate rows, in the COLLECTION / PAYMENT period. Each row's instruction is keyed to
  *exigibilitate*, not invoice date.
- **Collected (output):** rd.9 = 21%, rd.10 = 11%, rd.11 = 9% (Legea 141/2025) → rd.19 total.
  Collections of OLD-rate invoices (24/20/19/9/5%, by the invoice's ORIGINAL rate) → rd.16
  "Regularizări taxă colectată" (the app already routes 19/5% S to reg_colectata).
- **Deductible (input):** rd.24 = 21%, rd.25 = 11% → rd.31/32. Old-rate → rd.33. NOTE: rd.26 =
  reverse-charge/simplificare (26.1=21%, 26.2=11%), NOT a 5% row; there is NO 5% deductible row
  and NO rd.24.1/25.1 legacy sub-rows in OPANAF 174/2026.
- **Memo rows (informational; never enter rd.19/rd.35 or TVA de plată):** rd.A/A1 = closing 4428
  (TVA neexigibilă) balance on UNcollected cash-VAT sales; rd.B/B1 = buyer analogue (input not
  yet paid). A1/B1 look-back is contradictory in the official doc (face: 6 luni/2 trim.; instr.:
  5 luni/trim. anterior) → make configurable, default to the instructions.
- **Base defers with the VAT:** rd.9/10/11 base = proportional net (paid_allocated − vat_released).
- **Excluded (art. 282(6)) stay on issue_date:** AE reverse-charge → rd.13; E exempt → rd.14;
  K intra-EU → rd.1/3 (exigibility art. 222 Directive / art. 319(9)). Only category 'S' domestic
  standard supplies of a cash-VAT company defer to collection.
- **e-TVA precompletat will legitimately diverge** (built from e-Factura issue-date data) — do
  NOT auto-align D300 to it; conformare-reply obligation removed 2026-01-01.

## Slice 4 decomposition (seller side first; buyer side is slice 7)
- **4a — snapshot:** `invoices.cash_vat_applies INTEGER NOT NULL DEFAULT 0`, set = company.cash_vat
  at invoice creation. Per-invoice snapshot (immune to later regime changes); existing rows = 0
  (no behaviour change). Routing gates on this, NOT the live company flag.
- **4b — pure allocation:** `allocate_collection(s_lines, paid_before, payment)` → per-rate
  (released_base, released_vat) using `vat_released` with the FULL invoice gross as denominator
  (a payment settles the whole invoice pro-rata; only S lines defer, E/Z stay at issue_date).
- **4c — wiring:** `cash_vat_collected_groups(pool, company, period)` summing released base+VAT over
  payments (paid_at ∈ period) against cash_vat_applies sales invoices, grouped by ORIGINAL rate;
  splice into `compute_d300` + `d300_vat_totals` so excluded categories + non-cash-VAT are byte
  -identical to today. Old-rate buckets feed the existing reg_colectata (rd.16) loop.
- **4d — official XSD D300** (d300/rows.rs) wiring + DUKIntegrator on a cash-VAT fixture.

## Progress
- **DONE** slices 1, 2, 3, 4a, 4b, 5, 6, 8 (all gated green, committed, not pushed).
- **8** (plafon monitor): plafon_lei(date) (4.5M pre-2026 / 5M 2026 / 5.5M 2027+, OUG 8/2026)
  + plafon_breach_month + compute_plafon_status command — cumulative current-year CA vs plafon,
  exit-notificare deadline (20th of month after breach) + cash-vat-stops date (end of following
  period). Deep-researched (ANAF Brașov 2026 guide). UI 097-reminder banner = follow-up.
- **5** (GL): post_sales_invoice credits 4428 for non-storno cash-VAT S; post_payment posts
  the per-rate D 4428 / C 4427 release; clears to zero on full collection. Adversarial QA fixed
  two GL↔D300 consistency bugs pre-commit: STORNED originals now deferred only when VALIDATED
  (D300 + GL aligned); FX paid_before accumulated per-payment to match cash_vat_collected_groups.
  Deferred to slice 7: 4428.col/.ded analytic split + collected-then-refunded storno reversal.
- **CANNOT run locally**: DUKIntegrator (slice 4d) — jar present but no Java runtime installed.
- **4b** wires both `compute_d300` and `d300_vat_totals`; the official XSD D300 follows
  automatically (rows.rs maps the D300Report). Adversarial QA (3-lens workflow) fixed before
  commit: storno reversal was dropped for cash-VAT (now gated on `CAST(total_amount)>0` so
  credit notes stay on the issue-date path); SQL `TRIM(vat_category)` symmetry; half-away
  `ron_to_bani`; date-only `paid_at`; payment converted in invoice currency.
- **TODO 4d** — DUKIntegrator on a cash-VAT fixture (needs Java; can't run locally).
- **TODO 5** — GL 4428.colectat/.deductibil postings + **proportional** storno-against-settlement
  (4b does only accrual-style storno reversal; reconcile still diverges for cash-VAT until 5).
- **TODO 6** — invoice "TVA la încasare" mention. **TODO 7** — buyer-side (needs payments-out on
  received invoices). **TODO 8** — plafon monitor + 097.

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
