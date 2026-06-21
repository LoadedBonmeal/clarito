-- Fix: replace the out-of-CHECK 'invoicing' status sentinel with a dedicated
-- boolean latch column. The CHECK on quotes/orders only allows known statuses
-- ('draft','sent','accepted','invoiced','cancelled','expired'/'cancelled') so
-- writing status='invoicing' fails in production but was hidden by hand-rolled
-- test schemas that had no CHECK. The converting flag is an in-flight latch
-- (0 = idle, 1 = convert in progress) with no CHECK constraint needed.
ALTER TABLE quotes  ADD COLUMN converting INTEGER NOT NULL DEFAULT 0;
ALTER TABLE orders  ADD COLUMN converting INTEGER NOT NULL DEFAULT 0;
