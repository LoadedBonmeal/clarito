-- Migration 0021: art331_code — art. 331 reverse-charge product category
-- for D394 op11 codPR tracking.
--
-- Both columns are nullable TEXT; no backfill — existing rows default to NULL,
-- and the D394 builder falls back to codPR=22 when the value is absent.
--
-- products.art331_code:       tag set by the user in the product catalog
-- invoice_line_items.art331_code: snapshot copied from the product at invoice-
--                                  create time (snapshot pattern — no FK to products)

ALTER TABLE products ADD COLUMN art331_code TEXT;
ALTER TABLE invoice_line_items ADD COLUMN art331_code TEXT;
