-- Migration 0039: art. 146 alin. (5^7) Cod fiscal — categoria de excepție de la baza minimă CAS/CASS
-- part-time. '' = fără excepție (se aplică majorarea la salariul minim dacă e cazul);
-- 'elev_student' (lit. a) | 'ucenic' (lit. b) | 'dizabilitate' (lit. c) | 'contracte_multiple'
-- (lit. e). Pensionarii (lit. d) sunt deja marcați prin coloana `pensionar`.
ALTER TABLE employees ADD COLUMN exceptie_cas_min TEXT NOT NULL DEFAULT '';
