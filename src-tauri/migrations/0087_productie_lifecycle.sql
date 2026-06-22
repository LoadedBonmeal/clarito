-- Producție lifecycle: draft / planned → in_progress → finalized / cancelled
--
-- Existingrows have status='finalized' (set by produce()) — backward compatible.
-- New column: planned_date (YYYY-MM-DD) for planned orders (optional for finalized).
--
-- Status lifecycle:
--   planned    → created by create_planned_order(); no stock/GL posted
--   in_progress → optional intermediate (set explicitly or via future partial-execution)
--   finalized  → execute_order() / direct produce() — stock consumed + GL posted
--   cancelled  → cancel_order() from planned/in_progress only; no GL
--   draft      → reserved for future UI-in-progress; same semantics as planned

ALTER TABLE productie_orders ADD COLUMN planned_date TEXT;

-- Index for status-filtered queries (list by status)
CREATE INDEX IF NOT EXISTS idx_productie_orders_company_status
    ON productie_orders(company_id, status);
