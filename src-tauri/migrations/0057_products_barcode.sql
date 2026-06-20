-- Wave C W1: add barcode column to products.
-- barcode (EAN-13 / GTIN) is the preferred cross-system product dedup key.
-- The column is nullable so existing rows are unaffected.
ALTER TABLE products ADD COLUMN barcode TEXT;
CREATE INDEX idx_products_barcode ON products(company_id, barcode) WHERE barcode IS NOT NULL;
