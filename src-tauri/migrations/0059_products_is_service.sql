-- Wave P1-B: promote is_service from import staging onto the live products table.
-- 0 = stocabil/goods (default), 1 = serviciu (non-stocabil).
-- NULL rows are treated as 0 (goods) by the application.
ALTER TABLE products ADD COLUMN is_service INTEGER NOT NULL DEFAULT 0;
