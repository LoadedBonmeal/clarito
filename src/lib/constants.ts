export const VAT_RATES = [0, 5, 9, 11, 19, 21] as const;
export type VatRate = typeof VAT_RATES[number];

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
