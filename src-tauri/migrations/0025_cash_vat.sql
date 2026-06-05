-- Migration 0025: cash-VAT regime flag (TVA la încasare) on companies.
--
-- Optional regime (Cod fiscal art. 282 alin. 3-8): VAT exigibility is deferred from the
-- invoice date to the collection date. This is the seller-side foundation flag; the
-- exigibility engine, GL 4428 postings and D300 event-driven routing are built on top
-- (see src-tauri/CASH_VAT_DESIGN.md). Default 0 — the regime is opt-in.
ALTER TABLE companies ADD COLUMN cash_vat INTEGER NOT NULL DEFAULT 0;
-- contacts.cash_vat: the SUPPLIER's cash-VAT status (from ANAF's RPATVAÎ register) —
-- drives the buyer's deferred deduction even when the buyer is not on cash VAT.
ALTER TABLE contacts ADD COLUMN cash_vat INTEGER NOT NULL DEFAULT 0;
