/**
 * StatusBadge — badge de status e-Factura.
 *
 * Acceptă atât statusuri din backend (UPPERCASE, e.g. "VALIDATED")
 * cât și pe cele vechi lowercase din sample data.
 * CSS-ul din design.css folosește clase lowercase — se face normalizare.
 */

const LABELS: Record<string, string> = {
  // InvoiceStatus (real API — UPPERCASE)
  draft:      "Schiță",
  queued:     "În așteptare",
  submitted:  "Trimisă",
  validated:  "Validată",
  rejected:   "Respinsă",
  storned:    "Stornată",
  // ReceivedStatus (real API — UPPERCASE)
  new:        "Nouă",
  reviewed:   "Revizuită",
  approved:   "Aprobată",
  archived:   "Arhivată",
  // legacy (sample data uses "pending" instead of "queued")
  pending:    "În așteptare",
  // Payment statuses
  paid:       "Plătit",
  unpaid:     "Neplătit",
  partial:    "Parțial",
  overdue:    "Restanță",
  // Recurring statuses
  active:     "Activ",
  inactive:   "Inactiv",
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
    <span className={"badge " + cssClass(status)} style={{ textTransform: "none" }}>
      <span className="dot" />
      {LABELS[lower] ?? status}
    </span>
  );
}
