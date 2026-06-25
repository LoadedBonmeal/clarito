/**
 * All VAT rates the app recognizes, INCLUDING the historical 5% and 19% (the pre-reform
 * standard/reduced rates abolished by Legea 141/2025 effective 2025-08-01). Kept for parsing,
 * editing, and storno of pre-reform documents. Use this for "is this a known rate" checks.
 */
export const VAT_RATES_ALL = [0, 5, 9, 11, 19, 21] as const;
export type VatRate = typeof VAT_RATES_ALL[number];

/**
 * VAT rates selectable for NEW documents under 2026 law: 21% standard, 11% reduced, 9% reduced
 * (new-housing transition, valid through 31.07.2026 — Legea 141/2025), and 0% (zero-rated).
 * The historical 5%/19% are intentionally excluded here so new invoices can't pick an abolished
 * rate; editing an existing line still preserves its stored rate via the dropdown merge in
 * LineItemsEditor. This is only the FALLBACK — the live rate list comes from the user-managed
 * Cote TVA table (api.vatRates.list).
 */
export const VAT_RATES = [0, 9, 11, 21] as const;

export const VAT_CATEGORIES = ['S', 'AE', 'E', 'Z', 'O', 'K', 'G'] as const;
export type VatCategory = typeof VAT_CATEGORIES[number];

export const VAT_CATEGORY_LABELS: Record<VatCategory, string> = {
  S: 'Standard (TVA inclusă)',
  AE: 'Taxare inversă',
  E: 'Scutit',
  Z: 'Cotă zero',
  O: 'În afara sferei TVA',
  K: 'Intracomunitar scutit',
  G: 'Export scutit',
};

export const COUNTRIES: { code: string; name: string }[] = [
  { code: 'RO', name: 'România' },
  { code: 'AT', name: 'Austria' },
  { code: 'BE', name: 'Belgia' },
  { code: 'BG', name: 'Bulgaria' },
  { code: 'HR', name: 'Croația' },
  { code: 'CY', name: 'Cipru' },
  { code: 'CZ', name: 'Cehia' },
  { code: 'DK', name: 'Danemarca' },
  { code: 'EE', name: 'Estonia' },
  { code: 'FI', name: 'Finlanda' },
  { code: 'FR', name: 'Franța' },
  { code: 'DE', name: 'Germania' },
  { code: 'GR', name: 'Grecia' },
  { code: 'HU', name: 'Ungaria' },
  { code: 'IE', name: 'Irlanda' },
  { code: 'IT', name: 'Italia' },
  { code: 'LV', name: 'Letonia' },
  { code: 'LT', name: 'Lituania' },
  { code: 'LU', name: 'Luxemburg' },
  { code: 'MT', name: 'Malta' },
  { code: 'NL', name: 'Olanda' },
  { code: 'PL', name: 'Polonia' },
  { code: 'PT', name: 'Portugalia' },
  { code: 'SK', name: 'Slovacia' },
  { code: 'SI', name: 'Slovenia' },
  { code: 'ES', name: 'Spania' },
  { code: 'SE', name: 'Suedia' },
  { code: 'GB', name: 'Marea Britanie' },
  { code: 'CH', name: 'Elveția' },
  { code: 'NO', name: 'Norvegia' },
  { code: 'US', name: 'Statele Unite' },
  { code: 'MD', name: 'Republica Moldova' },
];

export const CURRENCIES = ['RON', 'EUR', 'USD', 'GBP', 'CHF'] as const;
export type Currency = typeof CURRENCIES[number];
