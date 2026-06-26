-- Audit hardening: several hot lookup/FK columns lacked indices, forcing full-table scans on common
-- list/convert/cascade paths. All pure additive (no data change), idempotent. Each *_id that is an
-- ON DELETE CASCADE child also gets an index so a parent delete doesn't scan the child table.

-- Sales: quote/order listing by company, conversion, line retrieval + cascade.
CREATE INDEX IF NOT EXISTS idx_quotes_company       ON quotes(company_id);
CREATE INDEX IF NOT EXISTS idx_quotes_contact       ON quotes(contact_id);
CREATE INDEX IF NOT EXISTS idx_orders_company       ON orders(company_id);
CREATE INDEX IF NOT EXISTS idx_orders_contact       ON orders(contact_id);
CREATE INDEX IF NOT EXISTS idx_quote_lines_quote    ON quote_lines(quote_id);
CREATE INDEX IF NOT EXISTS idx_order_lines_order    ON order_lines(order_id);

-- Treasury advances / expense reports (deconturi): filter by company / employee / advance + cascade.
CREATE INDEX IF NOT EXISTS idx_treasury_advances_company  ON treasury_advances(company_id);
CREATE INDEX IF NOT EXISTS idx_treasury_advances_employee ON treasury_advances(employee_id);
CREATE INDEX IF NOT EXISTS idx_expense_reports_company    ON expense_reports(company_id);
CREATE INDEX IF NOT EXISTS idx_expense_reports_advance    ON expense_reports(advance_id);
CREATE INDEX IF NOT EXISTS idx_expense_reports_employee   ON expense_reports(employee_id);
CREATE INDEX IF NOT EXISTS idx_expense_lines_report       ON expense_lines(report_id);

-- Administrative tables: company-scoped lookups.
CREATE INDEX IF NOT EXISTS idx_secondary_offices_company ON secondary_offices(company_id);
CREATE INDEX IF NOT EXISTS idx_payroll_config_company    ON payroll_config(company_id);
