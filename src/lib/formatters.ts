/**
 * Formats an optional monetary amount (string or number from backend) as Romanian RON.
 * Handles the case where backend returns Decimal as string (e.g. "1234.56").
 */
export function formatOptionalRon(amount?: string | number | null): string {
  if (amount === undefined || amount === null || amount === "") {
    return "sumă necunoscută";
  }
  const parsed = typeof amount === "number" ? amount : Number(amount);
  if (!Number.isFinite(parsed)) return "sumă necunoscută";
  return `${parsed.toFixed(2)} RON`;
}
