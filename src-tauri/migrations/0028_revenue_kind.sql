-- Migration 0028: per-line revenue kind → split sales revenue 701/704/707/709 (OMFP 1802/2014).
--
-- Until now all sales revenue was posted to 707 "Venituri din vânzarea mărfurilor". The revenue
-- account is determined by the NATURE of the line (pct. 76 / funcțiunea clasei 7):
--   product  → 701 "Venituri din vânzarea produselor finite"
--   service  → 704 "Venituri din servicii prestate"
--   goods    → 707 "Venituri din vânzarea mărfurilor"  (default — preserves prior behaviour)
--   reduction→ 709 "Reduceri comerciale acordate" (post-invoice commercial reductions granted,
--              recorded via credit notes; pct. 76 alin. (3)/(5))
ALTER TABLE invoice_line_items ADD COLUMN revenue_kind TEXT NOT NULL DEFAULT 'goods';

-- Ensure 709 is in every company's chart (701/704/707 are already seeded) so SAF-T MasterFiles
-- (built from chart_of_accounts) and the GL postings stay consistent.
INSERT INTO chart_of_accounts (id, company_id, account_code, account_name, account_class, active)
SELECT c.id || '-acc-709', c.id, '709', 'Reduceri comerciale acordate', 7, 1
FROM companies c
WHERE EXISTS (
        -- only companies that already have a seeded chart (avoid creating a lone-709 chart)
        SELECT 1 FROM chart_of_accounts a2 WHERE a2.company_id = c.id
    )
  AND NOT EXISTS (
        SELECT 1 FROM chart_of_accounts a WHERE a.company_id = c.id AND a.account_code = '709'
    );
