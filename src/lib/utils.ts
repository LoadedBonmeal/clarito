import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function formatRON(amount: number): string {
  return new Intl.NumberFormat("ro-RO", {
    style: "currency",
    currency: "RON",
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(amount);
}

/**
 * Formatează o sumă ca număr cu 2 zecimale, fără simbol de monedă.
 * Folosit în tabelele dense unde coloana deja indică RON.
 * Echivalent cu fmtRON din @/data/sample.
 */
export function fmtRON(amount: number): string {
  return amount.toLocaleString("ro-RO", {
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

export function formatDate(date: Date | string, withTime = false): string {
  const d = typeof date === "string" ? new Date(date) : date;
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
