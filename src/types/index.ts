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
  totalEmployerCost: string;
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
  taxableProfit: string;
  tax16: string;
  sponsorshipCap: string;
  sponsorshipCredit: string;
  taxAfterCredits: string;
  anticipatedPayments: string;
  balanceDue: string;
  balanceRecoverable: string;
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
    | "Other";
  message: string;
}
