import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/** Parse a Decimal string (or legacy number) to a JS number for arithmetic/display. */
export const parseDec = (s: string | number | undefined | null): number =>
  parseFloat(String(s ?? "0")) || 0;

export function formatRON(amount: string | number): string {
  return new Intl.NumberFormat("ro-RO", {
    style: "currency",
    currency: "RON",
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(parseDec(amount));
}

/**
 * Formatează o sumă ca număr cu 2 zecimale, fără simbol de monedă.
 * Folosit în tabelele dense unde coloana deja indică RON.
 * Echivalent cu fmtRON din @/data/sample.
 */
export function fmtRON(amount: string | number): string {
  return parseDec(amount).toLocaleString("ro-RO", {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  });
}

export function formatNumber(value: number, decimals = 2): string {
  return new Intl.NumberFormat("ro-RO", {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  }).format(value);
}

/**
 * Parse a YYYY-MM-DD string into a local-time Date (avoiding the UTC-midnight
 * shift that `new Date("2026-01-15")` produces, which renders as Jan 14 in
 * EET/UTC+2). For Date objects or strings that include time info the behaviour
 * is unchanged.
 */
function parseDateLocal(date: Date | string): Date {
  if (typeof date !== "string") return date;
  // ISO date-only strings are exactly 10 chars: YYYY-MM-DD
  if (/^\d{4}-\d{2}-\d{2}$/.test(date)) {
    const [y, m, d] = date.split("-").map(Number);
    return new Date(y, m - 1, d);
  }
  return new Date(date);
}

export function formatDate(date: Date | string, withTime = false): string {
  const d = parseDateLocal(date);
  if (withTime) {
    return new Intl.DateTimeFormat("ro-RO", {
      day: "numeric",
      month: "long",
      year: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    }).format(d);
  }
  return new Intl.DateTimeFormat("ro-RO", {
    day: "numeric",
    month: "long",
    year: "numeric",
  }).format(d);
}

/** Lunile anului, nume complete (ro) — partajat de pagini (Reports/Declarations/GlLedger/SAF-T). */
export const MONTHS_RO = [
  "Ianuarie", "Februarie", "Martie", "Aprilie", "Mai", "Iunie",
  "Iulie", "August", "Septembrie", "Octombrie", "Noiembrie", "Decembrie",
] as const;

/** Lunile anului, abreviate (ro) — pentru selectoare compacte (Payroll/Assets). */
export const MONTHS_RO_SHORT = [
  "Ian", "Feb", "Mar", "Apr", "Mai", "Iun", "Iul", "Aug", "Sep", "Oct", "Nov", "Dec",
] as const;
