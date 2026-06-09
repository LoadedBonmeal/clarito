-- Migration 0032: backfill the result/tax accounts into existing companies' charts so the
-- period-close (6/7 → 121), the income-tax expense (691/698 → 4411/4418) and the annual 121 → 117
-- reset have their accounts. New companies get these from standard_accounts(); this covers
-- companies created before migration 0032. Idempotent via the NOT EXISTS guard.
INSERT INTO chart_of_accounts (id, company_id, account_code, account_name, account_class)
SELECT lower(hex(randomblob(16))), c.id, v.code, v.name, v.class
FROM companies c
CROSS JOIN (
    SELECT '117' AS code, 'Rezultatul reportat' AS name, 1 AS class
    UNION ALL SELECT '4411', 'Impozitul pe profit', 4
    UNION ALL SELECT '4418', 'Impozitul pe venit', 4
    UNION ALL SELECT '691', 'Cheltuieli cu impozitul pe profit', 6
    UNION ALL SELECT '698', 'Cheltuieli cu impozitul pe venit și cu alte impozite', 6
) v
WHERE NOT EXISTS (
    SELECT 1 FROM chart_of_accounts ca
    WHERE ca.company_id = c.id AND ca.account_code = v.code
);
