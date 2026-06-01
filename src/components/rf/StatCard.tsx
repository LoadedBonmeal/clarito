import type { ReactNode } from "react";
import { cn } from "@/lib/utils";
import { Icon } from "@/components/shared/Icon";

export type DeltaDir = "up" | "down" | "neutral";

export interface StatCardProps {
  label: string;
  value: ReactNode;
  unit?: string;
  icon?: string;
  ctx?: ReactNode;
  delta?: ReactNode;
  deltaDir?: DeltaDir;
  className?: string;
}

export function StatCard({
  label,
  value,
  unit,
  icon,
  ctx,
  delta,
  deltaDir = "neutral",
  className,
}: StatCardProps) {
  return (
    <div className={cn("rf-card rf-stat", className)}>
      <div className="rf-stat-top">
        {icon && (
          <span className="rf-stat-ic">
            <Icon name={icon} size={20} />
          </span>
        )}
        {delta && (
          <span className={`rf-delta rf-delta--${deltaDir}`}>
            {delta}
          </span>
        )}
      </div>
      <div className="rf-label">{label}</div>
      <div className="rf-value">
        {value}
        {unit && <span className="rf-unit">{unit}</span>}
      </div>
      {ctx && <div className="rf-ctx">{ctx}</div>}
    </div>
  );
}
