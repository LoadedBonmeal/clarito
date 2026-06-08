-- Migration 0029: payment-date BNR exchange rate on payments + supplier payments.
--
-- Needed to book FX gain/loss (665 "Cheltuieli din diferențe de curs valutar" / 765 "Venituri
-- din diferențe de curs valutar") at settlement (OMFP 1802/2014 pct. 322): the receivable/
-- payable was booked at the INVOICE-date rate, the cash moves at the PAYMENT-date rate, and the
-- difference is the FX result. NULL = no payment-date rate captured → no FX leg (RON, or legacy
-- rows); the cash then falls back to the invoice rate as before.
ALTER TABLE payments ADD COLUMN exchange_rate REAL;
ALTER TABLE received_invoice_payments ADD COLUMN exchange_rate REAL;
