-- Migration 0027: payments-OUT against received (supplier) invoices.
--
-- Buyer-side TVA la încasare (Cod fiscal art. 297 alin. (2)-(3); Norme pct. 69) defers the
-- right to deduct input VAT until the supplier is PAID, when the supplier applies cash VAT OR
-- the buyer itself does. The app could only record payments against ISSUED (sales) invoices;
-- this table is the missing capability — payments made to suppliers, matched per received
-- invoice (deduction is earmarked per invoice, not global FIFO). Mirrors the sales `payments`
-- table. "plata" is any settlement (bank/cash/card/instrument/compensare/cesiune); the method
-- only fixes the date that unlocks the deferred deduction — it never gates the right.
CREATE TABLE IF NOT EXISTS received_invoice_payments (
    id                   TEXT    PRIMARY KEY NOT NULL,
    received_invoice_id  TEXT    NOT NULL REFERENCES received_invoices(id) ON DELETE CASCADE,
    company_id           TEXT    NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    amount               TEXT    NOT NULL,
    currency             TEXT    NOT NULL DEFAULT 'RON',
    paid_at              TEXT    NOT NULL,
    method               TEXT    NOT NULL DEFAULT 'transfer',
    reference            TEXT,
    notes                TEXT,
    created_at           INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_recv_payments_invoice ON received_invoice_payments(received_invoice_id);
CREATE INDEX IF NOT EXISTS idx_recv_payments_company ON received_invoice_payments(company_id);
CREATE INDEX IF NOT EXISTS idx_recv_payments_paid_at ON received_invoice_payments(paid_at);
