-- Migration 0026: cash-VAT (TVA la încasare) regime effective window on companies.
--
-- Gates the collection-date VAT deferral (slice 4) so invoices issued BEFORE the company
-- adopted the regime — or after it exited — keep normal invoice-date exigibility. Without
-- this window, a mid-year adopter's earlier invoices would be wrongly deferred when later
-- collected (they were already declared at issue date).
--
-- NULL cash_vat_start = regime active from the start of the company's history;
-- NULL cash_vat_end   = regime still active. Both are inclusive ISO dates (YYYY-MM-DD).
ALTER TABLE companies ADD COLUMN cash_vat_start TEXT;
ALTER TABLE companies ADD COLUMN cash_vat_end   TEXT;
