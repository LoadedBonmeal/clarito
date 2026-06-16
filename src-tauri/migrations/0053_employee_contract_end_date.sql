-- Data încetării contractului (CIM) pe salariat — pentru proratarea bazei minime CAS/CASS part-time
-- la luni INCOMPLETE prin încetare la mijlocul lunii (art. 146 alin. (5^6) Cod fiscal / OMF 1855/2022;
-- câmp D112 A_13P = ROUND(salariu_minim × A_8 / NZL)). Nulabil: lipsă = contract activ toată luna
-- (regression-safe; comportament identic cu înainte pentru toți angajații existenți). ISO YYYY-MM-DD.
ALTER TABLE employees ADD COLUMN contract_end_date TEXT;
