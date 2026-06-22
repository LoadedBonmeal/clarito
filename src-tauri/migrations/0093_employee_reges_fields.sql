-- Registrul General de Evidenţa Salariaţilor (REGES-Online, HG 295/2025) —
-- câmpuri suplimentare per angajat necesare exportului: funcţia + codul COR 6 cifre.
-- Nullable cu DEFAULT '' pentru că angajaţii existenţi rămân valizi; utilizatorul
-- le completează la nevoie înainte de exportul REGES.

ALTER TABLE employees ADD COLUMN functia     TEXT NOT NULL DEFAULT '';
ALTER TABLE employees ADD COLUMN cod_cor     TEXT NOT NULL DEFAULT '';
