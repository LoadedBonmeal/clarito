import { parseDec } from "./utils";

/**
 * Formats an optional monetary amount (string or number from backend) as Romanian RON.
 * Handles the case where backend returns Decimal as string (e.g. "1234.56").
 *
 * Delegates parsing to `parseDec` for consistency with the rest of the codebase.
 * Note: `parseDec` returns 0 for non-numeric strings, so this function explicitly
 * pre-checks for invalid numeric content to preserve the "sumă necunoscută" signal.
 */
export function formatOptionalRon(amount?: string | number | null): string {
  if (amount === undefined || amount === null || amount === "") {
    return "sumă necunoscută";
  }
  // Pre-check: a finite Number(amount) implies numeric content; reject NaN/Infinity
  // *before* parseDec swallows them into 0.
  const probe = typeof amount === "number" ? amount : Number(amount);
  if (!Number.isFinite(probe)) return "sumă necunoscută";
  const parsed = parseDec(amount);
  if (!Number.isFinite(parsed)) return "sumă necunoscută";
  return `${parsed.toFixed(2)} RON`;
}
