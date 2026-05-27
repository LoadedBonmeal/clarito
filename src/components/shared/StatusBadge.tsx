/**
 * StatusBadge — badge de status e-Factura.
 *
 * Acceptă atât statusuri din backend (UPPERCASE, e.g. "VALIDATED")
 * cât și pe cele vechi lowercase din sample data.
 * CSS-ul din design.css folosește clase lowercase — se face normalizare.
 */

const LABELS: Record<string, string> = {
  // InvoiceStatus (real API — UPPERCASE)
  draft:      "SCHIȚĂ",
  queued:     "ÎN AȘTEPTARE",
  submitted:  "TRIMISĂ",
  validated:  "VALIDATĂ",
  rejected:   "RESPINSĂ",
  storned:    "STORNATĂ",
  // ReceivedStatus (real API — UPPERCASE)
  new:        "NOUĂ",
  reviewed:   "REVIZUITĂ",
  approved:   "APROBATĂ",
  archived:   "ARHIVATĂ",
  // legacy (sample data uses "pending" instead of "queued")
  pending:    "ÎN AȘTEPTARE",
  // Payment statuses
  paid:       "PLĂTIT",
  unpaid:     "NEPLĂTIT",
  partial:    "PARȚIAL",
  overdue:    "RESTANȚĂ",
  // Recurring statuses
  active:     "ACTIV",
  inactive:   "INACTIV",
};

/** Returnează clasa CSS corespunzătoare (toate clasele din design.css sunt lowercase). */
function cssClass(status: string): string {
  const lower = status.toLowerCase();
  if (lower === "queued") return "pending";   // no .badge.queued in CSS
  if (lower === "storned") return "archived"; // no .badge.storned in CSS
  return lower;
}

export function StatusBadge({ status }: { status: string }) {
  const lower = status.toLowerCase();
  return (
    <span className={"badge " + cssClass(status)}>
      <span className="dot" />
      {LABELS[lower] ?? status}
    </span>
  );
}
