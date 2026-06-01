/**
 * StatusBadge — badge de status e-Factura, stilizat cu rf pill.
 *
 * Acceptă atât statusuri din backend (UPPERCASE, e.g. "VALIDATED")
 * cât și pe cele vechi lowercase din sample data.
 * Restyle: rf-badge + dot colorat.
 *
 * Variant map:
 *   success (green):  validated, approved, paid, active
 *   info (indigo):    submitted, new
 *   neutral (grey):   draft, unpaid, archived, inactive
 *   error (red):      rejected, overdue
 *   warning (amber):  queued, pending, partial, reviewed, storned
 */

const LABELS: Record<string, string> = {
  // InvoiceStatus
  draft:     "Schiță",
  queued:    "În așteptare",
  submitted: "Trimisă",
  validated: "Validată",
  rejected:  "Respinsă",
  storned:   "Stornată",
  // ReceivedStatus
  new:       "Nouă",
  reviewed:  "Revizuită",
  approved:  "Aprobată",
  archived:  "Arhivată",
  // legacy
  pending:   "În așteptare",
  // Payment statuses
  paid:      "Plătit",
  unpaid:    "Neplătit",
  partial:   "Parțial",
  overdue:   "Restanță",
  // Recurring statuses
  active:    "Activ",
  inactive:  "Inactiv",
};

type Variant = "success" | "info" | "neutral" | "error" | "warning";

function getVariant(lower: string): Variant {
  switch (lower) {
    case "validated":
    case "approved":
    case "paid":
    case "active":
      return "success";
    case "submitted":
    case "new":
      return "info";
    case "draft":
    case "unpaid":
    case "archived":
    case "inactive":
      return "neutral";
    case "rejected":
    case "overdue":
      return "error";
    case "queued":
    case "pending":
    case "partial":
    case "reviewed":
    case "storned":
      return "warning";
    default:
      return "neutral";
  }
}

export function StatusBadge({ status }: { status: string }) {
  const lower = status.toLowerCase();
  const variant = getVariant(lower);
  return (
    <span className={`rf-badge rf-badge--${variant}`}>
      <span className="rf-dot" />
      {LABELS[lower] ?? status}
    </span>
  );
}
