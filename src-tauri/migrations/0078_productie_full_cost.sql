-- Producție — cost complet (OMFP 1802/2014 pct. 8, IAS 2)
--
-- Extinde productie_orders cu componentele costului complet:
--   labour_cost           — manoperă directă (641/421) pentru acest ordin
--   overhead_cost         — regie totală introdusă de utilizator
--   overhead_fixed        — (opțional) componenta fixă a regiei
--   overhead_variable     — (opțional) componenta variabilă a regiei
--   normal_capacity_qty   — (opțional) capacitate normală (buc) pentru absorbția IAS 2
--   labour_cost_total     — manoperă totală (= labour_cost; stocat explicit)
--   overhead_absorbed     — regie capitalizată efectiv (după absorbție)
--   overhead_unabsorbed   — regie fixă neabsorbită (cheltuiala perioadei — NU în 345)
--   full_cost             — costul complet capitalizat în 345 = mat + manoperă + regie_absorbită
--   full_unit_cost        — full_cost / qty_produced
--
-- Ordin existent: noile coloane sunt NULL sau '0', deci full_cost = total_material_cost (backward compat).
-- Cutover: aplicat de la migration forward; ordinele anterioare nu sunt repostate.

ALTER TABLE productie_orders ADD COLUMN labour_cost          TEXT NOT NULL DEFAULT '0';
ALTER TABLE productie_orders ADD COLUMN overhead_cost        TEXT NOT NULL DEFAULT '0';
ALTER TABLE productie_orders ADD COLUMN overhead_fixed       TEXT;
ALTER TABLE productie_orders ADD COLUMN overhead_variable    TEXT;
ALTER TABLE productie_orders ADD COLUMN normal_capacity_qty  TEXT;
ALTER TABLE productie_orders ADD COLUMN overhead_absorbed    TEXT NOT NULL DEFAULT '0';
ALTER TABLE productie_orders ADD COLUMN overhead_unabsorbed  TEXT NOT NULL DEFAULT '0';
ALTER TABLE productie_orders ADD COLUMN full_cost            TEXT NOT NULL DEFAULT '0';
ALTER TABLE productie_orders ADD COLUMN full_unit_cost       TEXT NOT NULL DEFAULT '0';

-- Back-fill: pentru ordinele existente full_cost = total_material_cost, full_unit_cost = unit_cost.
UPDATE productie_orders
SET full_cost      = total_material_cost,
    full_unit_cost = unit_cost
WHERE full_cost = '0';
