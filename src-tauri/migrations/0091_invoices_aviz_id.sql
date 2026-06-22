-- Link an emitted invoice back to the aviz that originated it.
-- NULL for all normal (non-aviz-backed) invoices.
-- When set, generate_gl_entries skips the standard D4111=C707+C4427 posting for
-- this invoice — revenue was already recognised at aviz issuance (D418=C707+C4428),
-- and the reclass (D4111=C418, D4428=C4427) is posted by convert_aviz_to_invoice.
ALTER TABLE invoices ADD COLUMN aviz_id TEXT REFERENCES avize(id);

CREATE INDEX IF NOT EXISTS idx_invoices_aviz_id ON invoices(aviz_id);
