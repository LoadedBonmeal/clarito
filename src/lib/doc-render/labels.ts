/**
 * doc-render/labels — the human-label dictionary that turns cryptic ANAF XML attribute names
 * (cui, cifR, baza1, divid_D, …) into the Romanian labels a person reads, plus value formatters
 * (money / date / month / coded enums). This is the data that makes the XML preview look like the
 * official ANAF document instead of a tag dump.
 *
 * Labels are a TRANSCRIPTION of the authoritative sources already in the repo — the Rust generator
 * doc comments (verified against the official DUK validator, e.g. `anaf_decl/d205_xml.rs`) and the
 * entry forms / i18n — never invented. Resolution never throws: an unknown attribute falls back to
 * its raw name, so a new/unmapped field still renders (just un-prettified) rather than breaking.
 */
import { MONTHS_RO, fmtRON, formatDate, formatNumber } from "@/lib/utils";

export type FieldFormat = "money_lei" | "money2" | "date" | "month" | "enum" | "cnp" | "text";

export interface FieldSpec {
  label: string;
  format?: FieldFormat;
  /** For `format: "enum"` — code → human meaning (e.g. d_rec 0→"Inițială"). */
  enumMap?: Record<string, string>;
}

export type LabelDict = Record<string, FieldSpec>;

/** Format a raw XML attribute value per its FieldSpec. Always returns a string; never throws. */
export function formatValue(raw: string, spec: FieldSpec | undefined): string {
  const v = (raw ?? "").trim();
  if (!spec || !spec.format || spec.format === "text" || spec.format === "cnp") return v;
  switch (spec.format) {
    case "money_lei": {
      // ANAF declaration sums are whole lei (N15 integers) — no decimals, thousands grouped.
      const n = Number(v);
      return Number.isFinite(n) ? `${formatNumber(n, 0)} lei` : v;
    }
    case "money2": {
      const n = Number(v);
      return Number.isFinite(n) ? fmtRON(n) : v;
    }
    case "date":
      return /^\d{4}-\d{2}-\d{2}$/.test(v) ? formatDate(v) : v;
    case "month": {
      const m = Number(v);
      return m >= 1 && m <= 12 ? MONTHS_RO[m - 1] : v;
    }
    case "enum":
      return spec.enumMap?.[v] ?? v;
    default:
      return v;
  }
}

// ── Pan-declaration fields (shared antet/header attributes across most ANAF declarations) ─────────
export const GLOBAL_FIELDS: LabelDict = {
  cui: { label: "Cod fiscal (CUI)" },
  cif: { label: "Cod fiscal (CIF)" },
  den: { label: "Denumire" },
  adresa: { label: "Adresă" },
  an: { label: "An" },
  luna: { label: "Lună", format: "month" },
  d_rec: {
    label: "Tip declarație",
    format: "enum",
    enumMap: { "0": "Inițială", "1": "Rectificativă" },
  },
  nume_declar: { label: "Nume declarant" },
  prenume_declar: { label: "Prenume declarant" },
  functie_declar: { label: "Funcție declarant" },
};

const TIP_VENIT: Record<string, string> = { "08": "Dividende" };

// ── D205 — informativă privind impozitul reținut la sursă (capitol dividende) ─────────────────────
export const D205_FIELDS: LabelDict = {
  an: { label: "An de venit" },
  totalPlata_A: { label: "Total plată (control)", format: "money_lei" },
  // sect_II — recapitulația secțiunii
  tip_venit: { label: "Tip venit", format: "enum", enumMap: TIP_VENIT },
  nrben: { label: "Nr. beneficiari" },
  Tcastig: { label: "Total câștig", format: "money_lei" },
  Tpierd: { label: "Total pierdere", format: "money_lei" },
  T_VB: { label: "Total venit brut", format: "money_lei" },
  T_GAR: { label: "Total garanție", format: "money_lei" },
  Tbaza: { label: "Total bază de calcul", format: "money_lei" },
  Timp: { label: "Total impozit", format: "money_lei" },
  // benef — rândul de beneficiar
  id_inreg: { label: "Nr." },
  tip_venit1: { label: "Tip venit", format: "enum", enumMap: TIP_VENIT },
  tip_plata: { label: "Tip plată", format: "enum", enumMap: { "2": "Finală" } },
  Rezid: { label: "Rezidență", format: "enum", enumMap: { "1": "Rezident", "2": "Nerezident" } },
  cifR: { label: "CNP beneficiar", format: "cnp" },
  den1: { label: "Nume beneficiar" },
  baza1: { label: "Bază de calcul", format: "money_lei" },
  imp1: { label: "Impozit reținut", format: "money_lei" },
  divid_D: { label: "Dividende distribuite", format: "money_lei" },
  divid_P: { label: "Dividende plătite", format: "money_lei" },
};

const DOC_DICTS: Record<string, LabelDict> = {
  D205: D205_FIELDS,
};

/**
 * Resolve a field's label + formatter for a given document: per-document override → global → raw
 * name fallback. `docKey` is the descriptor key (e.g. "D205"); unknown keys just use the globals.
 */
export function resolveField(docKey: string, attr: string): FieldSpec {
  const doc = DOC_DICTS[docKey];
  return doc?.[attr] ?? GLOBAL_FIELDS[attr] ?? { label: attr };
}

/**
 * CIUS-RO VAT category code → Romanian label, ported from `ubl/pdf.rs::vat_label` so the in-app
 * invoice document and the generated PDF agree. `percent` is the numeric rate (e.g. "21").
 */
export function vatCategoryLabel(code: string, percent: string): string {
  const p = (percent ?? "").trim();
  switch ((code ?? "").trim()) {
    case "S":
      return p ? `${p}%` : "TVA standard";
    case "Z":
      return "0% (cotă zero)";
    case "E":
      return "0% (scutit)";
    case "AE":
      return "0% (taxare inversă)";
    case "K":
      return "0% (intracomunitar)";
    case "G":
      return "0% (export)";
    case "O":
      return "0% (în afara sferei)";
    default:
      return p ? `${p}%` : (code ?? "");
  }
}
