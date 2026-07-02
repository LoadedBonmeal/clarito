-- W8-1: scutirea art. 43 alin. (4) Cod fiscal — dividendele plătite unei persoane
-- juridice ROMÂNE care deține minimum 10% din capital pentru minimum 1 an (la data
-- plății) sunt SCUTITE de impozit pe dividende. Utilizatorul atestă condiția de
-- participație bifând câmpul; când e setat, cota = 0, impozit = 0, nota GL nu mai
-- are rând 446 (457 e creditat cu întregul brut) și distribuirea NU intră în
-- obligația D100 cod 150.
ALTER TABLE dividends ADD COLUMN exempt_art43_4 INTEGER NOT NULL DEFAULT 0;
