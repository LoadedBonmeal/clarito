/**
 * Tipuri care oglindesc structurile DB din `src-tauri/src/db/*`.
 *
 * Convenție serde: backend-ul folosește `#[serde(rename_all = "camelCase")]`
 * pe structuri, deci totul ajunge în JSON ca `camelCase`.
 *
 * Status enums sunt UPPERCASE pe wire.
 */

// ─── Enums ────────────────────────────────────────────────────────────────

export type InvoiceStatus =
  | "DRAFT"
  | "QUEUED"
  | "SUBMITTED"
  | "VALIDATED"
  | "REJECTED"
  | "STORNED";

export type ReceivedStatus =
  | "NEW"
  | "REVIEWED"
  | "APPROVED"
  | "REJECTED"
  | "ARCHIVED";

export type ContactType = "CUSTOMER" | "SUPPLIER" | "BOTH";

export type LicenseTier = "TRIAL" | "SOLO" | "ACCOUNTANT" | "FIRM";

export type VatCategory = "S" | "Z" | "E" | "AE" | "K" | "G" | "O";

// ─── Pagination ───────────────────────────────────────────────────────────

export interface Page {
  offset: number;
  limit: number;
}

export interface Paginated<T> {
  items: T[];
  total: number;
  offset: number;
  limit: number;
}

// ─── Company ──────────────────────────────────────────────────────────────

export interface Company {
  id: string;
  cui: string;
  legalName: string;
  tradeName: string | null;
  registryNumber: string | null;
  vatPayer: boolean;

  address: string;
  city: string;
  county: string;
  postalCode: string | null;
  country: string;

  email: string | null;
  phone: string | null;
  iban: string | null;
  bankName: string | null;

  isActive: boolean;
  spvEnabled: boolean;
  /** "micro" (impozit pe venit 1%) or "profit" (impozit pe profit 16%) — 2026. */
  taxRegime: string;

  invoiceSeries: string;
  lastInvoiceNumber: number;

  logoPath: string | null;

  createdAt: number;
  updatedAt: number;
}

/** Micro-ceiling status (100.000 EUR, OUG 89/2025) for a company in a year. */
export interface TaxRegimeStatus {
  taxRegime: string;
  ytdTurnoverRon: string;
  ceilingRon: string;
  pct: number;
  /** "ok" | "approaching" | "exceeded" | "na" (profit regime). */
  level: string;
  note: string | null;
  /** Cash-VAT plafon (5.000.000 lei). */
  cashVatPlafonRon: string;
  /** "ok" | "approaching" | "exceeded" | "na" (not on cash VAT). */
  cashVatLevel: string;
  cashVatNote: string | null;
}

export interface CreateCompanyInput {
  cui: string;
  legalName: string;
  tradeName?: string;
  registryNumber?: string;
  vatPayer?: boolean;
  address: string;
  city: string;
  county: string;
  postalCode?: string;
  country?: string;
  email?: string;
  phone?: string;
  iban?: string;
  bankName?: string;
  invoiceSeries?: string;
  /** "micro" or "profit" — settable at creation. */
  taxRegime?: string;
}

export type UpdateCompanyInput = Partial<
  Omit<CreateCompanyInput, "cui">
> & {
  isActive?: boolean;
  spvEnabled?: boolean;
  logoPath?: string;
  /** "micro" or "profit" (tax regime). */
  taxRegime?: string;
};

// ─── Contact ──────────────────────────────────────────────────────────────

export interface Contact {
  id: string;
  companyId: string;
  contactType: ContactType;
  cui: string | null;
  legalName: string;
  vatPayer: boolean;
  /** True for an individual/consumer (persoană fizică) — B2C; no CUI required. */
  isIndividual: boolean;
  /** TVA la încasare (cash VAT) — buyer-side deduction deferred to payment (art. 297). */
  cashVat: boolean;
  address: string | null;
  city: string | null;
  county: string | null;
  country: string;
  email: string | null;
  phone: string | null;
  currency: string | null;
  /** IBAN for bank transfers (used for invoice payment instructions and AP/AR matching). */
  iban: string | null;
  bankName: string | null;
  swift: string | null;
  /** Default payment term in days — auto-fills invoice due date on contact selection. */
  paymentTermDays: number | null;
  createdAt: number;
  updatedAt: number;
}

export interface CreateContactInput {
  companyId: string;
  contactType: ContactType;
  cui?: string;
  legalName: string;
  vatPayer?: boolean;
  isIndividual?: boolean;
  /** TVA la încasare (cash VAT) — captured from ANAF. */
  cashVat?: boolean;
  address?: string;
  city?: string;
  county?: string;
  country?: string;
  email?: string;
  phone?: string;
  currency?: string;
  iban?: string;
  bankName?: string;
  swift?: string;
  paymentTermDays?: number;
}

export type UpdateContactInput = Partial<Omit<CreateContactInput, "companyId">>;

export interface ContactFilter {
  companyId?: string;
  query?: string;
}

// ─── Invoice ──────────────────────────────────────────────────────────────

export interface Invoice {
  id: string;
  companyId: string;
  contactId: string;
  series: string;
  number: number;
  fullNumber: string;
  issueDate: string;
  dueDate: string;
  currency: string;
  exchangeRate: number | null;
  subtotalAmount: string;
  vatAmount: string;
  totalAmount: string;
  status: InvoiceStatus;
  anafUploadId: string | null;
  anafIndex: string | null;
  anafSubmittedAt: number | null;
  anafValidatedAt: number | null;
  anafRejectedAt: number | null;
  xmlPath: string | null;
  pdfPath: string | null;
  signatureXmlPath: string | null;
  rejectionReason: string | null;
  rejectionCode: string | null;
  notes: string | null;
  paymentMeansCode: string;
  /// BIZ-13: FK to the original invoice this credit note reverses. Set only on
  /// storno credit notes; null for regular invoices and STORNED originals.
  stornoOfInvoiceId: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface LineItem {
  id: string;
  invoiceId: string;
  position: number;
  name: string;
  description: string | null;
  quantity: string;
  unit: string;
  unitPrice: string;
  vatRate: string;
  vatCategory: VatCategory;
  subtotalAmount: string;
  vatAmount: string;
  totalAmount: string;
  cpvCode: string | null;
  /** Art. 331 product category snapshot (from product at creation). */
  art331Code: string | null;
}

export interface InvoiceEvent {
  id: string;
  invoiceId: string;
  eventType: string;
  message: string;
  metadata: string | null;
  createdAt: number;
}

export interface InvoiceWithLines {
  invoice: Invoice;
  lines: LineItem[];
  events: InvoiceEvent[];
}

export interface CreateLineInput {
  name: string;
  description?: string;
  quantity: number;
  unit: string;
  unitPrice: number;
  vatRate: number;
  vatCategory: VatCategory;
  cpvCode?: string;
  /** Art. 331 product category snapshot (from product). Used for D394 codPR. */
  art331Code?: string;
  /**
   * Sales-revenue GL nature → 701 (produse finite) | 704 (servicii) | 707 (mărfuri, default) | 709 (reduceri).
   * Defaults to "goods" (→ 707) when absent. Set to "service" for service products (→ 704).
   * User can override freely.
   */
  revenueKind?: string;
}

export interface CreateInvoiceInput {
  companyId: string;
  contactId: string;
  series: string;
  number: number;
  issueDate: string;
  dueDate: string;
  currency?: string;
  exchangeRate?: number | null;
  notes?: string;
  paymentMeansCode?: string;
  lines: CreateLineInput[];
}

export interface InvoiceFilter {
  companyId?: string;
  statuses?: InvoiceStatus[];
  dateFrom?: string;
  dateTo?: string;
  query?: string;
  page?: Page;
}

// ─── Received Invoice ─────────────────────────────────────────────────────

export interface ReceivedInvoice {
  id: string;
  companyId: string;
  anafDownloadId: string;
  anafIndex: string | null;
  issuerCui: string;
  issuerName: string;
  series: string | null;
  number: string | null;
  totalAmount: string;
  netAmount?: string | null;
  vatAmount?: string | null;
  currency: string;
  exchangeRate?: number | null;
  issueDate: string;
  xmlPath: string;
  pdfPath: string | null;
  status: ReceivedStatus;
  /** Tipul achiziției intra-UE: "goods" (default, R5/R18) sau "services" (R7/R20). */
  intraEuKind: "goods" | "services";
  downloadedAt: number;
  createdAt: number;
}

export interface ReceivedFilter {
  companyId?: string;
  statuses?: ReceivedStatus[];
  page?: Page;
}

// ─── Notification ─────────────────────────────────────────────────────────

export interface Notification {
  id: string;
  notificationType: string;
  title: string;
  body: string;
  data: string | null;
  isRead: boolean;
  readAt: number | null;
  osNotificationShown: boolean;
  createdAt: number;
}

// ─── License ──────────────────────────────────────────────────────────────

export interface License {
  id: number;
  licenseKey: string | null;
  tier: LicenseTier;
  activatedAt: number | null;
  expiresAt: number;
  machineId: string;
  email: string | null;
  lastValidatedAt: number | null;
  /** True if `expiresAt` is in the past. Computed by the backend on each fetch. */
  isExpired: boolean;
  /** Days remaining in a TRIAL period (negative when expired). Null for non-TRIAL tiers. */
  trialDaysRemaining: number | null;
}

// ─── Auth / RBAC (P2 Wave 8) ─────────────────────────────────────────────

export type UserRole = "admin" | "contabil" | "operator" | "viewer";

export interface CurrentUser {
  id: string;
  username: string;
  role: UserRole;
}

export interface AuthStatus {
  needsSetup: boolean;
  authenticated: boolean;
  currentUser: CurrentUser | null;
}

export interface UserRow {
  id: string;
  username: string;
  role: UserRole;
  isActive: boolean;
  failedAttempts: number;
  lockedUntil: number | null;
  createdAt: number;
  lastLogin: number | null;
}

export interface CreateUserInput {
  username: string;
  password: string;
  role: UserRole;
}

export interface UpdateUserInput {
  role?: UserRole;
  isActive?: boolean;
}

// ─── System ───────────────────────────────────────────────────────────────

export interface AppInfo {
  name: string;
  version: string;
  dbPath: string;
  appDataDir: string;
}

export interface SyncResult {
  statusPolls: number;
  newReceived: number;
  updatedAt: number;
}

// ─── UBL / XML validation ─────────────────────────────────────────────────

export interface ValidationResult {
  valid: boolean;
  errors: string[];
  warnings: string[];
}

// ─── Certificate ANAF ────────────────────────────────────────────────────

export interface Certificate {
  id: string;
  companyId: string;
  keychainRef: string;
  issuedAt: number;
  expiresAt: number;
  refreshableUntil: number;
  isActive: boolean;
  lastRefreshedAt: number | null;
  lastUsedAt: number | null;
  createdAt: number;
  updatedAt: number;
}

export interface AnafCompanyData {
  cui: string;
  legalName: string;
  address: string;
  city: string;
  county: string;
  postalCode: string | null;
  registryNumber: string | null;
  phone: string | null;
  vatPayer: boolean;
  /** TVA la încasare (cash VAT). */
  cashVat: boolean;
  /** Registered in "Registrul RO e-Factura". */
  efacturaRegistered: boolean;
  /** False = inactive contributor (restricted buyer deductibility, art. 11). */
  active: boolean;
}

export interface AnafStatusResult {
  stare: string;
  descriere: string | null;
  anafIndex: string | null;
}

// ─── Reports ─────────────────────────────────────────────────────────────

export interface VatGroup {
  rate: string;
  /** VAT category code (e.g. "S", "Z", "E", "AE", "K", "G", "O"). Two groups at the same rate
   *  but with different categories must be distinct (e.g. 0% Exempt vs 0% Zero-rated). */
  vatCategory: string;
  baseAmount: string;
  vatAmount: string;
  invoiceCount: number;
}

export interface VatReport {
  dateFrom: string;
  dateTo: string;
  companyId: string | null;
  totalBase: string;
  totalVat: string;
  totalAmount: string;
  invoiceCount: number;
  vatGroups: VatGroup[];
  generatedAt: number;
}

export interface ExportReportParams {
  dateFrom?: string;
  dateTo?: string;
  companyId?: string;
}

// ─── D300 Decont TVA ─────────────────────────────────────────────────────────

/** Un grup de TVA colectat (cotă + categorie) din D300. */
export interface D300Group {
  vatRate: string;
  vatCategory: string;
  base: string;
  vat: string;
}

/** Raportul D300 — TVA colectat (vânzări) + TVA deductibil (achiziții). */
export interface D300Report {
  companyCui: string;
  periodFrom: string;
  periodTo: string;
  groups: D300Group[];
  totalBase: string;
  totalVat: string;
  invoiceCount: number;
  // Wave B: achiziții
  /** Grupuri TVA deductibil (achiziții), din received_invoice_vat_lines. */
  purchaseGroups: D300Group[];
  /** Total baze impozabile achiziții (RON), 2 zecimale. */
  totalDeductibleBase: string;
  /** Total TVA deductibil (RON), 2 zecimale. */
  totalDeductibleVat: string;
  /** Numărul de facturi primite (status != REJECTED) în perioadă. */
  purchaseInvoiceCount: number;
  /** Facturi primite fără defalcare TVA (net_amount IS NULL). */
  purchaseUnparsedCount: number;
  /** TVA netă de plată = TVA colectată − TVA deductibilă (negativă = de recuperat). */
  netVat: string;
  // Wave 8: regularizări cote vechi (auto-computed prefill values)
  /** Σ baza vânzări S la cote vechi 19%/5% → R16_1 prefill. */
  regColectataBaza: string;
  /** Σ TVA vânzări S la cote vechi 19%/5% → R16_2 prefill. */
  regColectataTva: string;
  /** Σ baza achiziții S la cote vechi 19%/9%/5% → R30_1 prefill. */
  regDedusaBaza: string;
  /** Σ TVA achiziții S la cote vechi 19%/9%/5% → R30_2 prefill. */
  regDedusaTva: string;
}

// ─── D394 Declarație informativă livrări/achiziții ───────────────────────────

/** Un partener (client) din declarația D394 — livrări (vânzări). */
export interface D394Partner {
  /** CUI-ul partenerului. Poate fi "" dacă nu a fost completat. */
  partnerCui: string;
  /** Denumirea legală a partenerului. */
  partnerName: string;
  /** Categoria TVA (S/AE/E/Z/O/K/G) — D394 raportează separate pe categorie. */
  vatCategory: string;
  /** Cota TVA normalizată la procent întreg (ex. "19", "9", "5", "0").
   *  Corespunde enum-ului D394 cota {0,5,9,11,19,20,21,24}. */
  vatRate: string;
  /** Numărul de facturi VALIDATED emise către partener în perioadă. */
  invoiceCount: number;
  /** Baza impozabilă totală (net), 2 zecimale. */
  base: string;
  /** TVA colectat total, 2 zecimale. */
  vat: string;
}

/** Raportul D394 — livrări (vânzări) + achiziții per partener. */
// ─── e-Transport (UIT) ────────────────────────────────────────────────────
export interface EtransportGood {
  codScopOperatiune: string;
  codTarifar?: string;
  denumireMarfa: string;
  cantitate: number;
  codUnitateMasura: string;
  greutateNeta?: number | null;
  greutateBruta: number;
  valoareLeiFaraTva?: number | null;
}
export interface EtransportPartner {
  codTara: string;
  cod?: string;
  denumire: string;
}
export interface EtransportTransport {
  nrVehicul: string;
  nrRemorca1?: string;
  nrRemorca2?: string;
  codTaraOrgTransport?: string;
  codOrgTransport?: string;
  denumireOrgTransport?: string;
  dataTransport: string;
}
export interface EtransportRouteLoc {
  codPtf?: number | null;
  codBirouVamal?: string | null;
  codJudet?: number | null;
  denumireLocalitate?: string;
  denumireStrada?: string;
  numar?: string;
  codPostal?: string;
  alteInfo?: string;
}
export interface EtransportDoc {
  tipDocument: string;
  numarDocument?: string;
  dataDocument?: string;
}
export interface EtransportDeclaration {
  codDeclarant: string;
  refDeclarant?: string;
  codTipOperatiune: string;
  goods: EtransportGood[];
  partner: EtransportPartner;
  transport: EtransportTransport;
  locStart: EtransportRouteLoc;
  locFinal: EtransportRouteLoc;
  documents: EtransportDoc[];
}
export interface EtransportUploadResponse {
  // Serialized as-is from ANAF's response (snake_case index + UIT) — not camelCased.
  index_incarcare: string;
  UIT?: string | null;
}

/** Evidența unei declarații e-Transport transmise (UIT + termen de valabilitate). */
export interface EtransportDeclRecord {
  id: string;
  companyId: string;
  uit: string | null;
  indexIncarcare: string;
  codTipOperatiune: string;
  partnerName: string;
  vehicle: string;
  testMode: boolean;
  /** Unix epoch (secunde). */
  submittedAt: number;
  /** Unix epoch (secunde) — UIT expiră la această dată (5/15 zile). */
  expiresAt: number;
}

/** SPV general inbox (SPVWS2) item — recipise/notificări/somații/decizii. */
export interface SpvInboxItem {
  id: string;
  tip: string;
  dataCreare: string;
  cif: string;
  idSolicitare: string | null;
  detalii: string | null;
  /** recipisa | notificare | somatie | decizie | factura | altele. */
  category: string;
}

/** One JSON file extracted from the e-TVA precompletat (P300ETVA) zip fetched from ANAF. */
export interface EtvaPrecompletatFile {
  name: string;
  json: string;
}

/** RO e-TVA — precompletat (P300ETVA) values imported from SPV for the self-check. */
export interface EtvaPrecompletat {
  collectedVat: string;
  deductibleVat: string;
}

/** RO e-TVA — one reconciled line (D300 vs precompletat). */
export interface EtvaLine {
  label: string;
  d300: string;
  precompletat: string;
  diff: string;
  diffPct: string;
  /** |diff| ≥ 5.000 lei AND |diff%| ≥ 20% (the significance guideline). */
  significant: boolean;
  note: string | null;
}

export interface EtvaReconciliation {
  periodFrom: string;
  periodTo: string;
  lines: EtvaLine[];
  anySignificant: boolean;
  /** Company on TVA la încasare → divergences are expected (not errors). */
  cashVat: boolean;
}

/** D390 declarația recapitulativă (VIES) — one aggregated operation row. */
export interface D390Op {
  /** Operation type: L/T/A/P/S/R. */
  tip: string;
  /** Partner country code (2 letters). */
  tara: string;
  /** Partner VAT id without the country prefix. */
  codO: string;
  /** Partner name. */
  denO: string;
  /** Taxable base in RON (whole lei). */
  baza: number;
}

export interface D390Doc {
  luna: number;
  an: number;
  operations: D390Op[];
  /** Intra-EU operations skipped for a missing/invalid partner VAT id (under-reporting flag). */
  dropped: number;
}

export interface D390Submission {
  dRec?: boolean;
  numeDeclar?: string;
  prenumeDeclar?: string;
  functieDeclar?: string;
}

export interface D394Report {
  companyCui: string;
  periodFrom: string;
  periodTo: string;
  /** Parteneri livrări sortați descrescător după baza impozabilă. */
  partners: D394Partner[];
  totalBase: string;
  totalVat: string;
  invoiceCount: number;
  // Wave B: achiziții
  /** Parteneri achiziții (furnizori cu linii VAT parsate), sortați descrescător după baza impozabilă. */
  purchasePartners: D394Partner[];
  /** Total baze impozabile achiziții (RON), 2 zecimale. */
  totalPurchaseBase: string;
  /** Total TVA deductibil achiziții (RON), 2 zecimale. */
  totalPurchaseVat: string;
  /** Numărul de facturi primite (status != REJECTED) în perioadă. */
  purchaseInvoiceCount: number;
  /** Facturi primite fără defalcare TVA (net_amount IS NULL). */
  purchaseUnparsedCount: number;
}

// ─── Feedback / Diagnostic ────────────────────────────────────────────────

export interface DiagnosticReport {
  appVersion: string;
  os: string;
  arch: string;
  machineIdHash: string;
  logTail: string[];
  licenseSummary: { tier: string; daysRemaining: number | null };
}

// ─── GDPR ────────────────────────────────────────────────────────────────────

export interface DataExportResult {
  path: string;
  bytes: number;
}

// ─── Product (articol / catalog) ─────────────────────────────────────────

// ─── P2 Wave 1: product types + account mapping ───────────────────────────

/** Canonical product type values (OMFP 1802/2014). */
export type ProductType =
  | "marfa"
  | "produs_finit"
  | "materie_prima"
  | "material_consumabil"
  | "serviciu";

export const PRODUCT_TYPES: ProductType[] = [
  "marfa",
  "produs_finit",
  "materie_prima",
  "material_consumabil",
  "serviciu",
];

/**
 * Effective account mapping for a product type.
 * Either a company override or the code default.
 */
export interface AccountMapping {
  stockAccount: string | null;
  expenseAccount: string | null;
  incomeAccount: string | null;
  usesStock: boolean;
  retailCapable: boolean;
}

/** Full row returned by list_account_mappings — includes override flag. */
export interface EffectiveAccountMapping extends AccountMapping {
  productType: ProductType;
  /** True when a company-specific override row exists in account_mapping. */
  isOverride: boolean;
}

export interface SetAccountMappingInput {
  stockAccount: string | null;
  expenseAccount: string | null;
  incomeAccount: string | null;
  usesStock: boolean;
  retailCapable: boolean;
}

/** A named product group scoped to a company. */
export interface ProductGroup {
  id: string;
  companyId: string;
  name: string;
  createdAt: number;
}

export interface ProductGroupInput {
  name: string;
}

export interface Product {
  id: string;
  companyId: string;
  name: string;
  unit: string;
  unitPrice: string;
  vatRate: string;
  vatCategory: string;
  code: string | null;
  stockQty: string | null;
  /** Art. 331 reverse-charge product category code for D394 op11 codPR. Null = use default 22. */
  art331Code: string | null;
  /** Stock valuation policy (OMFP 1802): 'FIFO' | 'CMP'. Null = CMP. */
  valuationMethod: string | null;
  /** GL stock account (371/301/345…). Null = 371. */
  stockAccount: string | null;
  /** True when this product is a service (non-stocabil): no fișă de magazie, no stock qty.
   *  GL revenue default: serviciu → 704; marfă → 707. */
  isService: boolean;
  /** Canonical product type (P2 Wave 1). Drives default GL account mapping.
   *  Consistent with isService: serviciu ⇔ isService=true. */
  productType: ProductType;
  /** Optional product group id. */
  productGroupId: string | null;
  active: boolean;
  createdAt: number;
  updatedAt: number;
}

export interface ProductInput {
  name: string;
  unit?: string;
  unitPrice?: string;
  vatRate?: string;
  vatCategory?: string;
  code?: string;
  stockQty?: string;
  /** Art. 331 product category code. Set only when vatCategory="AE". */
  art331Code?: string;
  /** True when this product is a service (non-stocabil). Defaults to false (goods). */
  isService?: boolean;
  /** Canonical product type. Defaults to "serviciu" when isService=true, else "marfa". */
  productType?: ProductType;
  /** Optional product group id. */
  productGroupId?: string;
  active?: boolean;
}

export interface UpdateProductInput {
  name?: string;
  unit?: string;
  unitPrice?: string;
  vatRate?: string;
  vatCategory?: string;
  code?: string;
  stockQty?: string;
  /** Art. 331 product category code. Set only when vatCategory="AE". */
  art331Code?: string;
  /** True when this product is a service (non-stocabil). None = leave unchanged. */
  isService?: boolean;
  /** Canonical product type. None = leave unchanged. */
  productType?: ProductType;
  /** Optional product group id. None = leave unchanged. */
  productGroupId?: string;
  active?: boolean;
}

// ─── VAT Rate (cotă TVA editabilă — catalog global) ───────────────────────

/**
 * R15 Wave 2: A single entry in the global VAT-rate catalog.
 * This table is intentionally NOT company-scoped — Romanian VAT rates are
 * national and shared across all companies in the app.
 */
export interface VatRate {
  id: string;
  rate: string;
  label: string;
  active: boolean;
  sortOrder: number;
  createdAt: number;
}

export interface VatRateInput {
  rate: string;
  label: string;
  active?: boolean;
  sortOrder?: number;
}

export interface UpdateVatRateInput {
  rate?: string;
  label?: string;
  active?: boolean;
  sortOrder?: number;
}

// ─── Receipt (chitanță) ───────────────────────────────────────────────────

export interface Receipt {
  id: string;
  companyId: string;
  series: string;
  number: number;
  contactId: string | null;
  invoiceId: string | null;
  amount: string;
  currency: string;
  issueDate: string;
  payerName: string | null;
  notes: string | null;
  pdfPath: string | null;
  createdAt: number;
}

export interface ReceiptInput {
  series?: string;
  contactId?: string;
  invoiceId?: string;
  amount: string;
  currency?: string;
  issueDate: string;
  payerName?: string;
  notes?: string;
}

// ─── Account (plan de conturi) — R15 Wave 4 ──────────────────────────────

/**
 * R15 Wave 4: A single entry in the company-scoped chart of accounts (PCG).
 * Each company has its own catalog; account codes are unique per company.
 */
export interface Account {
  id: string;
  companyId: string;
  accountCode: string;
  accountName: string;
  accountClass: number | null;
  parentCode: string | null;
  active: boolean;
  createdAt: number;
  updatedAt: number;
}

export interface AccountInput {
  accountCode: string;
  accountName: string;
  accountClass?: number;
  parentCode?: string;
  active?: boolean;
}

export interface UpdateAccountInput {
  accountCode?: string;
  accountName?: string;
  accountClass?: number;
  parentCode?: string;
  active?: boolean;
}

// ─── D300Submission — câmpuri suplimentare pentru exportul oficial ANAF ────

/**
 * Mirrors Rust `D300Submission` (src-tauri/src/anaf_decl/d300/mod.rs).
 * `#[serde(rename_all = "camelCase")]` + `#[serde(default)]` on several fields.
 */
export interface D300Submission {
  // Declarant
  numeDeclar: string;
  prenumeDeclar: string;
  functieDeclar: string;
  // Companie / bancă
  caen: string;
  banca: string;
  cont: string;
  // Tip decont / temei legal
  tipDecont: string;           // "L" | "T" | "S" | "A"
  temei?: number;              // default 0
  depusReprezentant?: boolean; // default false
  // Flags regim special
  bifaInterne?: boolean;
  bifaCereale?: boolean;
  bifaMob?: boolean;
  bifaDisp?: boolean;
  bifaCons?: boolean;
  // Rambursare / pro-rata
  solicitRamb?: boolean;
  nrEvid?: string;   // default "0"
  proRata?: number;  // default 100.0
  // Wave 8: regularizări cote vechi (optional overrides; null = use auto-computed)
  regColectataBaza?: number | null;  // R16_1 override (lei întregi)
  regColectataTva?: number | null;   // R16_2 override
  regDedusaBaza?: number | null;     // R30_1 override
  regDedusaTva?: number | null;      // R30_2 override
}

// ─── D394Submission — câmpuri suplimentare pentru exportul oficial D394 ────

/**
 * Mirrors Rust `D394Submission` (src-tauri/src/anaf_decl/d394/mod.rs).
 * `#[serde(rename_all = "camelCase")]` + `#[serde(default)]` on several fields.
 */
export interface D394Submission {
  tipD394: string;           // "L" | "T" | "S" | "A"
  sistemTva?: boolean;       // default false
  opEfectuate?: boolean;     // default false
  caen: string;
  telefon: string;
  // Reprezentant
  denR: string;
  functieReprez: string;
  adresaR: string;
  // Întocmit
  tipIntocmit?: number;       // default 0
  denIntocmit: string;
  cifIntocmit?: number;       // default 0 (i64 in Rust)
  calitateIntocmit?: string | null; // optional
  // Alte flag-uri
  optiune?: boolean;
  prsAfiliat?: boolean;
  solicit?: boolean;
  // Cartuș G (încasări AMEF) + facturi simplificate — totaluri introduse manual pe cotă.
  nrBfI1?: number;
  cashRows?: D394CashRow[];
}

/**
 * Un rând per cotă TVA cu totalurile (lei întregi) pentru încasări numerar (Î1/Î2) și facturi
 * simplificate declarate manual în D394 (cartuș G + I). Mirrors Rust `D394CashRow`.
 * Sumele-total incasari_i1/i2 se calculează din aceste rânduri (regula DUK) — nu se trimit separat.
 */
export interface D394CashRow {
  cota: number;
  bazaI1?: number;
  tvaI1?: number;
  bazaI2?: number;
  tvaI2?: number;
  bazaFsl?: number;
  tvaFsl?: number;
  bazaFslCod?: number;
  tvaFslCod?: number;
  bazaFsa?: number;
  tvaFsa?: number;
  bazaFsai?: number;
  tvaFsai?: number;
  bazaBfai?: number;
  tvaBfai?: number;
}

// ─── GL — rezultat generare note contabile ────────────────────────────────

/**
 * Mirrors Rust `GlPostResult` (src-tauri/src/db/gl.rs).
 * `#[serde(rename_all = "camelCase")]`
 */
export interface GlPostResult {
  journalsInserted: number;
  entriesInserted: number;
  journalsReplaced: number;
  skippedReceived: number;
  /** Referințele facturilor primite sărite (fără defalcare TVA). */
  skippedReceivedRefs: string[];
}

/**
 * Mirrors Rust `ReconcileReport` (src-tauri/src/db/gl.rs).
 * `#[serde(rename_all = "camelCase")]`
 * All monetary fields are RON strings with 2 decimal places.
 */
export interface ReconcileReport {
  balanced: boolean;
  totalDebit: string;
  totalCredit: string;
  vatCollectedGl: string;
  vatCollectedD300: string;
  vatDeductibleGl: string;
  vatDeductibleD300: string;
  discrepancies: string[];
}

/**
 * Mirrors Rust `VatSettlementResult` (src-tauri/src/db/gl.rs) — period-end VAT close.
 * Monetary fields are RON strings with 2 decimals.
 */
export interface VatSettlementResult {
  collected: string;
  deductible: string;
  netVat: string;
  dePlata: string;
  deRecuperat: string;
  entryDate: string;
  posted: boolean;
}

/** One account row of the balanța de verificare (cod 14-6-30). RON strings, 2 decimals. */
export interface TrialBalanceRow {
  accountCode: string;
  accountName: string;
  openingDebit: string;
  openingCredit: string;
  periodDebit: string;
  periodCredit: string;
  totalDebit: string;
  totalCredit: string;
  closingDebit: string;
  closingCredit: string;
}

/** One line of the Registru-jurnal (cod 14-1-1). */
export interface JournalRegisterRow {
  nrCrt: number;
  date: string;
  document: string;
  explanation: string;
  debitAccount: string;
  creditAccount: string;
  debit: string;
  credit: string;
}

/** Mirrors Rust `JournalRegister` (src-tauri/src/db/gl.rs). */
export interface JournalRegister {
  rows: JournalRegisterRow[];
  totalDebit: string;
  totalCredit: string;
  balanced: boolean;
}

/** One movement line of a Cartea mare account sheet (fișă de cont). */
export interface LedgerEntry {
  date: string;
  document: string;
  explanation: string;
  contra: string;
  debit: string;
  credit: string;
  balance: string;
  balanceSide: string;
}

/** One synthetic-account sheet of the Cartea mare (cod 14-1-3). */
export interface LedgerAccount {
  accountCode: string;
  accountName: string;
  openingDebit: string;
  openingCredit: string;
  entries: LedgerEntry[];
  totalDebit: string;
  totalCredit: string;
  closingDebit: string;
  closingCredit: string;
}

/** Mirrors Rust `TrialBalance` (src-tauri/src/db/gl.rs) — balanța de verificare. */
export interface TrialBalance {
  rows: TrialBalanceRow[];
  totalOpeningDebit: string;
  totalOpeningCredit: string;
  totalPeriodDebit: string;
  totalPeriodCredit: string;
  totalTotalDebit: string;
  totalTotalCredit: string;
  totalClosingDebit: string;
  totalClosingCredit: string;
  balanced: boolean;
}

// ─── Cont de profit și pierdere (P&L) ────────────────────────────────────

export interface PnlLine {
  code: string;
  name: string;
  amount: string;
}

export interface ClosingEntry {
  debitAccount: string;
  creditAccount: string;
  amount: string;
}

export interface ClosePeriodResult {
  totalRevenue: string;
  totalExpense: string;
  result: string;
  entriesCount: number;
  posted: boolean;
  entryDate: string;
}

export interface StockMovementInput {
  companyId: string;
  productId: string;
  entryDate: string;
  qty: string;
  unitCost?: string;
  docType?: string;
  docRef?: string;
  gestiuneId?: string;  // optional; backend resolves to default if omitted
}

export interface StockLedgerRow {
  id: string;
  entryDate: string;
  direction: string;
  qty: string;
  unitCost: string;
  value: string;
  runQty: string;
  runValue: string;
  docType: string | null;
  docRef: string | null;
  gestiuneId: string | null;
}

export interface Gestiune {
  id: string;
  companyId: string;
  cod: string;
  denumire: string;
  tip: string;
  metodaEvaluare: string;
  contStoc: string;
  adresa: string | null;
  dispersataTeritorila: number;
  isDefault: number;
  activ: number;
  createdAt: number;
}

export interface GestiuneInput {
  cod: string;
  denumire: string;
  tip?: string;
  metodaEvaluare?: string;
  contStoc?: string;
  adresa?: string;
  dispersataTeritorila?: boolean;
}

export interface FixedAsset {
  id: string;
  companyId: string;
  assetCode: string;
  accountId: string;
  description: string;
  valuationClass: string;
  supplierId: string;
  supplierName: string;
  dateOfAcquisition: string;
  startUpDate: string;
  acquisitionCost: string;
  lifeMonths: number;
  depreciationMethod: string;
  depreciationPct: string;
  disposalDate: string | null;
  active: boolean;
  createdAt: number;
  updatedAt: number;
}

export interface FixedAssetInput {
  assetCode: string;
  accountId?: string;
  description: string;
  dateOfAcquisition: string;
  startUpDate?: string;
  acquisitionCost: string;
  lifeMonths?: number;
  depreciationMethod?: string;
  disposalDate?: string | null;
  active?: boolean;
}

export interface AssetDepreciationState {
  assetId: string;
  assetCode: string;
  description: string;
  monthlyCharge: string;
  accumulated: string;
  bookValue: string;
  expenseAcct: string;
  amortAcct: string;
}

export interface DepreciationRun {
  states: AssetDepreciationState[];
  totalAmount: string;
  posted: boolean;
  entryDate: string;
}

export interface Employee {
  id: string;
  companyId: string;
  cnp: string;
  fullName: string;
  grossSalary: string;
  personalDeduction: string;
  employmentDate: string | null;
  /** Data încetării contractului (ISO); null = activ. Prorata baza minimă part-time pe luni incomplete. */
  contractEndDate: string | null;
  active: boolean;
  tipAsigurat: string;
  pensionar: boolean;
  tipContract: string;
  oreNorma: number;
  /** art. 146 (5^7) excepție de la baza minimă CAS/CASS part-time: ''/'elev_student'/'ucenic'/
   *  'dizabilitate'/'contracte_multiple' (pensionarii via `pensionar`). */
  exceptieCasMin: string;
  /** CIF-ul sediului secundar la care e repartizat (D112 angajatorF2); '' = sediu principal. */
  sediuCif: string;
  /** Beneficiar al sumei netaxabile din salariul minim (art. III OUG 89/2025): normă întreagă,
   *  salariu de bază = salariul minim, fără diminuare în 2026 → carve-out 300/200 lei. */
  beneficiarSumaNetaxabila: boolean;
  createdAt: number;
  updatedAt: number;
}

/** Sediu secundar / punct de lucru (D112 angajatorF2). */
export interface SecondaryOffice {
  id: string;
  companyId: string;
  cif: string;
  name: string;
  createdAt: number;
}

/** Certificat de concediu medical (OUG 158/2005) — registru, sursa D112 asiguratD. */
export interface MedicalLeave {
  id: string;
  companyId: string;
  employeeId: string;
  periodYm: string;
  serie: string;
  numar: string;
  codIndemnizatie: string;
  dataAcordare: string;
  dataInceput: string;
  dataSfarsit: string;
  zileAngajator: number;
  zileFnuass: number;
  bazaCalcul: string;
  zileBaza: number;
  sumaAngajator: string;
  sumaFnuass: string;
  procent: number;
  locPrescriere: number;
  codBoala: string;
  createdAt: number;
}

export interface MedicalLeaveInput {
  companyId: string;
  employeeId: string;
  periodYm: string;
  serie?: string;
  numar?: string;
  codIndemnizatie?: string;
  dataAcordare?: string;
  dataInceput?: string;
  dataSfarsit?: string;
  zileAngajator?: number;
  zileFnuass?: number;
  bazaCalcul?: string;
  zileBaza?: number;
  sumaAngajator?: string;
  sumaFnuass?: string;
  procent?: number;
  locPrescriere?: number;
  codBoala?: string;
}

export interface CreateEmployeeInput {
  companyId: string;
  cnp: string;
  fullName: string;
  grossSalary: string;
  personalDeduction?: string;
  employmentDate?: string | null;
  contractEndDate?: string | null;
  tipAsigurat?: string;
  pensionar?: boolean;
  tipContract?: string;
  oreNorma?: number;
  exceptieCasMin?: string;
  sediuCif?: string;
  beneficiarSumaNetaxabila?: boolean;
}

export type UpdateEmployeeInput = Partial<Omit<CreateEmployeeInput, "companyId">> & {
  active?: boolean;
};

export interface EmployeeState {
  employeeId: string;
  fullName: string;
  gross: string;
  cas: string;
  cass: string;
  incomeTax: string;
  net: string;
  cam: string;
  // NOTE: concedii (CCI 0,85%) removed — abolished 1 Jan 2018 by OUG 79/2017.
}

export interface PayrollRun {
  states: EmployeeState[];
  totalGross: string;
  totalCas: string;
  totalCass: string;
  totalIncomeTax: string;
  totalNet: string;
  totalCam: string;
  // NOTE: totalConcedii (CCI 0,85%) removed — abolished 1 Jan 2018 by OUG 79/2017.
  posted: boolean;
  entryDate: string;
}

/** Payroll (D112 core) — one salary state. */
export interface PayrollInput {
  gross: string;
  personalDeduction?: string;
}

export interface PayrollResult {
  gross: string;
  cas: string;
  cass: string;
  personalDeduction: string;
  taxableBase: string;
  incomeTax: string;
  net: string;
  cam: string;
  // NOTE: concedii (CCI 0,85%) removed — abolished 1 Jan 2018 by OUG 79/2017.
  // totalEmployerCost = gross + cam (CAM 2,25% only).
  totalEmployerCost: string;
}

/** Salary simulator — options (all optional, defaults to H1 2026, 0 dependents). */
export interface SalarySimOpts {
  /** Number of dependents (0–4+) for the ANAF personal deduction table (art. 77 CF). */
  dependents?: number;
  /** Full-time min-wage beneficiary (art. III OUG 89/2025) — enables 300/200 lei non-taxable sum. */
  beneficiarSumaNetaxabila?: boolean;
  /** Month (1–12). Defaults to 6. */
  month?: number;
  /** Year. Defaults to 2026. */
  year?: number;
}

/** Salary simulator result — full breakdown gross → net + employer cost. */
export interface SalarySimResult {
  gross: string;
  cas: string;
  cass: string;
  nonTaxable: string;
  deducerePersonala: string;
  impozitBase: string;
  impozit: string;
  net: string;
  cam: string;
  totalEmployerCost: string;
  /** Max deduction from ANAF table (before art. 77(2) gross ceiling). */
  deducereTabel: string;
  /** Deduction that actually entered the calculation. */
  deducereEfectiva: string;
  /** True if the non-taxable carve-out was applied. */
  carveoutApplied: boolean;
}

/** Intrastat threshold monitor (per flow). */
export interface IntrastatFlowStatus {
  ytdRon: string;
  pct: number;
  level: string; // "ok" | "approaching" | "exceeded"
}
export interface IntrastatStatus {
  thresholdRon: string;
  dispatches: IntrastatFlowStatus;
  arrivals: IntrastatFlowStatus;
}

/** Obligație informativă de impozit pe dividende în D100 (cod creanță + denumire + sumă + scadență 25 a lunii). */
export interface DividendObligation {
  /** Cod de creanță Nomenclator: "604" (persoane fizice, art. 97) sau "150" (persoane juridice, art. 43). */
  codOblig: string;
  label: string;
  /** Suma impozitului reținut (lei, 2 zecimale). */
  amount: string;
  /** Scadența declarării/plății — zz.ll.aaaa. */
  deadline: string;
  /** Numărul de distribuiri agregate. */
  count: number;
}

/** D100 (obligații de plată) — quarterly obligation row + dividend obligations due in the quarter. */
export interface D100Result {
  /** False când D100 nu se aplică (profit, trim. IV → se regularizează prin D101). */
  applicable: boolean;
  note: string | null;
  codOblig: string;
  label: string;
  base: string;
  ratePct: string;
  sumaDatorata: string;
  priorPayments: string;
  sumaDePlata: string;
  scadenta: string;
  /** Obligații INFORMATIVE de impozit pe dividende cu scadența în trimestru (D100 nu emite XML). */
  dividendObligations: DividendObligation[];
}

/** D101 (impozit pe profit) worksheet adjustments — the user-entered fiscal items. */
export interface D101Input {
  nonTaxableRevenue?: string;
  fiscalDeductions?: string;
  nonDeductibleExpenses?: string;
  priorLoss?: string;
  sponsorship?: string;
  anticipatedPayments?: string;
}

export interface D101Result {
  accountingResult: string;
  nonTaxableRevenue: string;
  fiscalDeductions: string;
  nonDeductibleExpenses: string;
  fiscalResult: string;
  priorLoss: string;
  lossUsed: string;
  lossRemaining: string;
  taxableProfit: string;
  tax16: string;
  sponsorshipCap: string;
  sponsorshipCredit: string;
  taxAfterCredits: string;
  anticipatedPayments: string;
  balanceDue: string;
  balanceRecoverable: string;
}

export interface IncomeTaxResult {
  taxRegime: string;
  expenseAccount: string;
  payableAccount: string;
  amount: string;
  estimated: boolean;
  posted: boolean;
  entryDate: string;
}

export interface AnnualCloseResult {
  year: number;
  result121: string;
  kind: string;
  posted: boolean;
  entryDate: string;
}

export interface BilantReport {
  periodTo: string;
  immobilizedAssets: string;
  inventory: string;
  receivables: string;
  shortInvestments: string;
  cashBank: string;
  prepaidExpenses: string;
  totalAssets: string;
  equity: string;
  currentResult: string;
  provisions: string;
  longTermDebt: string;
  currentLiabilities: string;
  deferredRevenue: string;
  totalEquityLiabilities: string;
  balanced: boolean;
  entitySizeNote: string;
}

export interface ProfitLoss {
  periodFrom: string;
  periodTo: string;
  taxRegime: string;
  revenueLines: PnlLine[];
  expenseLines: PnlLine[];
  operatingRevenue: string;
  cifraAfaceri: string;
  financialRevenue: string;
  totalRevenue: string;
  operatingExpense: string;
  financialExpense: string;
  totalExpense: string;
  grossResult: string;
  incomeTax: string;
  incomeTaxEstimated: boolean;
  netResult: string;
  closingEntries: ClosingEntry[];
}

// ─── ANAF form-version staleness ─────────────────────────────────────────

/** One stale declaration form returned by `check_form_versions`. */
export interface FormStaleness {
  form: string;
  bundled: string;
  latest: string;
}

// ─── Error (din backend) ──────────────────────────────────────────────────

export interface AppErrorPayload {
  kind:
    | "NotFound"
    | "Validation"
    | "Database"
    | "Migration"
    | "Io"
    | "Json"
    | "Tauri"
    | "Conflict"
    | "Xml"
    | "Pdf"
    | "Xlsx"
    | "Archive"
    | "Other"
    | "SessionExpired";
  message: string;
}

/** Rând din istoricul depunerilor de declarații fiscale. */
export interface Filing {
  id: string;
  companyId: string;
  /** Tipul declarației: "D300", "D390", "D394", "D112", "D205", "D207", "SAFT", "BILANT". */
  kind: string;
  /** Perioada: "YYYY-MM" pentru lunar, "YYYY" pentru anual. */
  period: string;
  isRectificative: boolean;
  filePath: string | null;
  /** Starea curentă: "EXPORTED" | "SUBMITTED" | "ACCEPTED" | "REJECTED". */
  anafStatus: string;
  /** Timestamp Unix (secunde) al momentului exportului. */
  filedAt: number;
}

/** Rând din lista perioadelor blocate. */
export interface PeriodLock {
  id: string;
  companyId: string;
  /** Luna blocată: "YYYY-MM". */
  period: string;
  /** Unix timestamp (secunde) al blocării. */
  lockedAt: number;
  /** Sursa blocării: "declaration:D300" | "declaration:D112" | "manual" | … */
  source: string;
  lockedBy: string | null;
  note: string | null;
}

// ─── Note contabile manuale (cod 14-6-2A) ────────────────────────────────────

/** O linie dintr-o notă contabilă manuală — trimisă de frontend la create. */
export interface ManualLineInput {
  accountCode: string;
  /** Suma debit ca string (ex. "100.00"); "" sau "0" = zero. */
  debit: string;
  /** Suma credit ca string; "" sau "0" = zero. */
  credit: string;
}

/** O linie a unei note contabile manuale — returnată de list. */
export interface ManualJournalLine {
  accountCode: string;
  accountName: string | null;
  debit: string;
  credit: string;
}

/** Vizualizare completă a unei note contabile manuale. */
export interface ManualJournalView {
  sourceId: string;
  journalId: string;
  date: string;
  description: string;
  lines: ManualJournalLine[];
  totalDebit: string;
  totalCredit: string;
}

// ─── Inventariere + Registru-inventar (P1 Wave 5) ────────────────────────────

/** Tip sesiune de inventariere (OMFP 2861/2009). */
export type InventorySessionType =
  | "ANUAL"
  | "INCEPERE"
  | "INCETARE"
  | "PREDARE_GESTIUNE"
  | "CALAMITATE";

/** Status sesiune inventariere. */
export type InventorySessionStatus = "DRAFT" | "FINALIZED";

/** Cauza diferenței de inventar. */
export type InventoryDiffCause =
  | "perisabilitati"
  | "imputabil"
  | "neimputabil"
  | "depreciere"
  | "altele";

/** Sesiune de inventariere (Listă de inventariere cod 14-3-12). */
export interface InventorySession {
  id: string;
  companyId: string;
  referenceDate: string;
  fiscalYear: number;
  type: InventorySessionType;
  gestiune: string | null;
  status: InventorySessionStatus;
  comisieMembers: string; // JSON array
  notes: string | null;
  createdAt: number;
  updatedAt: number;
}

/** Linie din lista de inventariere. */
export interface InventoryLine {
  id: string;
  sessionId: string;
  accountCode: string;
  itemName: string;
  um: string;
  qtyScriptic: string;
  qtyFaptic: string;
  unitPrice: string;
  valueContabila: string;
  valueInventar: string;
  diffValue: string;
  diffCause: InventoryDiffCause | null;
  imputable: number;
  productId: string | null;
  createdAt: number;
  updatedAt: number;
}

/** Input pentru crearea unei sesiuni de inventariere. */
export interface CreateInventorySessionInput {
  companyId: string;
  referenceDate: string;
  fiscalYear: number;
  type?: InventorySessionType;
  gestiune?: string;
  comisieMembers?: string;
  notes?: string;
}

/** Input pentru actualizarea cantității faptice pe o linie. */
export interface UpdateInventoryLineFapticInput {
  lineId: string;
  sessionId: string;
  companyId: string;
  qtyFaptic: string;
  diffCause?: InventoryDiffCause;
  imputable?: boolean;
}

/** Rând din Registrul-inventar (cod 14-1-2, OMFP 2634/2015). */
export interface RegistruInventarEntry {
  id: string;
  companyId: string;
  fiscalYear: number;
  seqNo: number;
  recapText: string;
  valueContabila: string;
  valueInventar: string;
  diffValue: string;
  diffCause: string;
  sourceSessionId: string | null;
  createdAt: number;
}

// ─── Reevaluare valutară (P1 Wave 7) ─────────────────────────────────────────

/** O linie de reevaluare per factură — returnată de `list_fx_revaluations`. */
export interface FxRevaluationRow {
  id: string;
  companyId: string;
  period: string;
  invoiceId: string;
  /** "ISSUED" = creanță 4111 / "RECEIVED" = datorie 401 */
  invoiceKind: "ISSUED" | "RECEIVED";
  currency: string;
  /** Sold valutar deschis (foreign_total - foreign_paid). */
  foreignOutstanding: string;
  /** Cursul BNR din ultima zi bancară a perioadei. */
  monthEndRate: string;
  /** Cursul anterior (din reevaluarea lunii precedente sau booking rate). */
  priorRate: string;
  /** Valoarea în lei la cursul month_end_rate. */
  revaluedLei: string;
  /** Valoarea în lei la cursul prior_rate. */
  priorLei: string;
  /** Diferența (signed): revalued_lei - prior_lei. Pozitiv = favorabil. */
  diffLei: string;
  createdAt: number;
}

/** Rezultatul rulării `compute_fx_revaluation`. */
export interface FxRevaluationResult {
  /** Perioada reevaluată ("YYYY-MM"). */
  period: string;
  /** Număr de facturi cu diff ≠ 0 reevaluate. */
  rowsPosted: number;
  /** Diferențe totale favorabile (lei) — C 765. */
  totalFavorable: string;
  /** Diferențe totale nefavorabile (lei) — D 665. */
  totalUnfavorable: string;
  /** Diferența netă (favorabil - nefavorabil). */
  netDiff: string;
  /** source_id-ul notei GL postate ("FX_REVAL-YYYY-MM"). */
  glSourceId: string;
  /** Ultima zi bancară folosită pentru curs BNR. */
  monthEndDate: string;
}

// ─── NIR (Notă de Intrare Recepție) ──────────────────────────────────────────

export type NirStatus = "draft" | "finalized";

export interface NirDocument {
  id: string;
  companyId: string;
  gestiuneId: string;
  receivedInvoiceId: string | null;
  supplierName: string | null;
  supplierCui: string | null;
  nirSeries: string | null;
  nirNumber: number;
  nirDate: string;
  retailMode: boolean;
  status: NirStatus;
  comisieReceptie: string | null;
  observatii: string | null;
  createdAt: number;
  finalizedAt: number | null;
}

export interface NirLine {
  id: string;
  nirId: string;
  productId: string | null;
  denumire: string;
  um: string | null;
  qty: string;
  unitCost: string;
  vatRate: string;
  adaosPct: string | null;
  valueCost: string;
  valueAdaos: string;
  valueTvaNeex: string;
  pretAmanunt: string;
  lineNo: number;
}

export interface NirWithLines {
  document: NirDocument;
  lines: NirLine[];
}

export interface NirLineInput {
  productId?: string;
  denumire: string;
  um?: string;
  qty: string;
  unitCost: string;
  vatRate: string;
  adaosPct?: string;
  lineNo: number;
}

export interface NirInput {
  gestiuneId: string;
  receivedInvoiceId?: string;
  supplierName?: string;
  supplierCui?: string;
  nirDate: string;
  retailMode?: boolean;
  comisieReceptie?: string;
  observatii?: string;
  lines: NirLineInput[];
}

// ─── Stock Transfers (bon de transfer 14-3-3A) ────────────────────────────────

export interface StockTransfer {
  id: string;
  companyId: string;
  productId: string;
  fromGestiuneId: string;
  toGestiuneId: string;
  transferDate: string;
  qty: string;
  unitCost: string;
  value: string;
  transferRef: string | null;
  notes: string | null;
  createdAt: number;
}

export interface TransferInput {
  productId: string;
  fromGestiuneId: string;
  toGestiuneId: string;
  transferDate: string;
  qty: string;
  transferRef?: string;
  notes?: string;
}

// ─── Producție / BOM (P2 Wave 5) ─────────────────────────────────────────────

export interface Bom {
  id: string;
  companyId: string;
  productId: string;
  name: string;
  outputQty: string;
  active: number;
  createdAt: number;
}

export interface BomLine {
  id: string;
  bomId: string;
  componentProductId: string;
  qty: string;
  um: string | null;
  lineNo: number;
}

export interface BomWithLines {
  id: string;
  companyId: string;
  productId: string;
  name: string;
  outputQty: string;
  active: number;
  createdAt: number;
  lines: BomLine[];
}

export interface BomLineInput {
  componentProductId: string;
  qty: string;
  um?: string;
  lineNo: number;
}

export interface BomInput {
  productId: string;
  name: string;
  outputQty: string;
  lines: BomLineInput[];
}

export interface ProductieOrder {
  id: string;
  companyId: string;
  bomId: string;
  productId: string;
  gestiuneId: string;
  qtyProduced: string;
  productionDate: string;
  totalMaterialCost: string;
  unitCost: string;
  // Full-cost fields (migration 0078)
  labourCost: string;
  overheadCost: string;
  overheadFixed: string | null;
  overheadVariable: string | null;
  normalCapacityQty: string | null;
  overheadAbsorbed: string;
  overheadUnabsorbed: string;
  fullCost: string;
  fullUnitCost: string;
  status: string;
  notes: string | null;
  createdAt: number;
}

export interface ProduceInput {
  bomId: string;
  gestiuneId: string;
  qtyProduced: string;
  productionDate: string;
  notes?: string;
  /** Direct labour cost for this order (641/421). Default 0. */
  labourCost?: string;
  /** Total overhead cost (if no fixed/variable split). Default 0. */
  overheadCost?: string;
  /** Fixed overhead component (optional, for IAS 2 absorption). */
  overheadFixed?: string;
  /** Variable overhead component (optional). */
  overheadVariable?: string;
  /** Normal capacity in units (optional, required for fixed overhead IAS 2 absorption). */
  normalCapacityQty?: string;
}

// ─── Fiscal Receipts / Raport Z ───────────────────────────────────────────────

export type FiscalReceiptStatus = "DRAFT" | "POSTED" | "STORNAT";

export interface FiscalReceipt {
  id: string;
  companyId: string;
  serieCasa: string;
  nrZ: number;
  reportDate: string;
  nrBonuri: number;
  total: string;
  numerar: string;
  card: string;
  tichete: string;
  status: FiscalReceiptStatus;
  retailMethod: number;
  notes: string | null;
  createdAt: number;
}

export interface FiscalReceiptVatLine {
  id: string;
  receiptId: string;
  vatCategory: string;
  rate: string;
  baza: string;
  tva: string;
}

export interface FiscalReceiptInvoiceLink {
  id: string;
  receiptId: string;
  invoiceId: string;
  amount: string;
  payMeans: "CASH" | "CARD";
}

export interface FiscalReceiptDetail {
  receipt: FiscalReceipt;
  vatLines: FiscalReceiptVatLine[];
  invoiceLinks: FiscalReceiptInvoiceLink[];
}

export interface FiscalReceiptInput {
  serieCasa: string;
  nrZ: number;
  reportDate: string;
  nrBonuri?: number;
  total: string;
  numerar: string;
  card: string;
  tichete?: string;
  retailMethod?: number;
  notes?: string;
}

export interface VatLineInput {
  vatCategory?: string;
  rate: string;
  baza: string;
  tva: string;
}

export interface InvoiceLinkInput {
  invoiceId: string;
  amount: string;
  payMeans: "CASH" | "CARD";
}
