-- Migration 0034: backfill the payroll accounts into existing companies' charts so the monthly
-- payroll posting (641/421, 4315 CAS, 4316 CASS, 444 impozit, 646/436 CAM) has its accounts.
-- New companies get them from standard_accounts(); idempotent via the NOT EXISTS guard.
INSERT INTO chart_of_accounts (id, company_id, account_code, account_name, account_class)
SELECT lower(hex(randomblob(16))), c.id, v.code, v.name, v.class
FROM companies c
CROSS JOIN (
    SELECT '4315' AS code, 'Contribuția de asigurări sociale (CAS)' AS name, 4 AS class
    UNION ALL SELECT '4316', 'Contribuția de asigurări sociale de sănătate (CASS)', 4
    UNION ALL SELECT '436', 'Contribuția asiguratorie pentru muncă (CAM)', 4
    UNION ALL SELECT '444', 'Impozitul pe venituri de natura salariilor', 4
    UNION ALL SELECT '646', 'Cheltuieli privind contribuția asiguratorie pentru muncă', 6
) v
WHERE NOT EXISTS (
    SELECT 1 FROM chart_of_accounts ca
    WHERE ca.company_id = c.id AND ca.account_code = v.code
);
