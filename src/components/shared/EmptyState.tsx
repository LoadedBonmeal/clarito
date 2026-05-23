import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

interface EmptyStateProps {
  icon?: LucideIcon;
  title: string;
  description?: string;
  action?: ReactNode;
  /** Compact: pentru folosirea în secțiuni mici (in-table empty etc.). */
  compact?: boolean;
}

/**
 * Empty state — pattern business sobru. Fără ilustrații, fără carduri
 * decorative. Doar text centrat și (opțional) un buton.
 */
export function EmptyState({
  icon: Icon,
  title,
  description,
  action,
  compact = false,
}: EmptyStateProps) {
  return (
    <div
      className={
        compact
          ? "flex flex-col items-center justify-center px-4 py-8 text-center"
          : "flex flex-col items-center justify-center px-4 py-16 text-center"
      }
    >
      {Icon && (
        <Icon className="mb-2 h-5 w-5 text-muted-foreground" strokeWidth={1.5} />
      )}
      <p className="text-[12px] font-medium">{title}</p>
      {description && (
        <p className="mt-1 max-w-sm text-[11px] text-muted-foreground">
          {description}
        </p>
      )}
      {action && <div className="mt-3">{action}</div>}
    </div>
  );
}
