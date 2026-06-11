/**
 * StatusBadge — badge de status e-Factura, redat ca design .chip.
 *
 * Acceptă atât statusuri din backend (UPPERCASE, e.g. "VALIDATED")
 * cât și pe cele vechi lowercase din sample data.
 *
 * Variant map (păstrat pentru teste — getVariant):
 *   success → chip paid:  validated, approved, paid, active
 *   info    → chip sent:  submitted, new
 *   neutral → chip sent:  draft, unpaid, archived, inactive
 *   error   → chip late:  rejected, overdue
 *   warning → chip wait:  queued, pending, partial, reviewed, storned
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

/** @internal exported for tests */
export function getVariant(lower: string): Variant {
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

/** Variant → design .chip class (paid|wait|late|sent). */
const CHIP_CLS: Record<Variant, string> = {
  success: "paid",
  info:    "sent",
  neutral: "sent",
  error:   "late",
  warning: "wait",
};

export function StatusBadge({ status }: { status: string }) {
  const lower = status.toLowerCase();
  const variant = getVariant(lower);
  return (
    <span className={`chip ${CHIP_CLS[variant]}`}>
      {LABELS[lower] ?? status}
    </span>
  );
}
