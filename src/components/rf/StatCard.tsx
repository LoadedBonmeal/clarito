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
        <div className="rf-label">{label}</div>
        {icon && (
          <span className="rf-stat-ic">
            <Icon name={icon} size={16} />
          </span>
        )}
      </div>
      <div className="rf-value">
        {value}
        {unit && <span className="rf-unit">{unit}</span>}
      </div>
      {(delta || ctx) && (
        <div className="rf-stat-foot">
          {delta && <span className={`rf-delta rf-delta--${deltaDir}`}>{delta}</span>}
          {ctx && <span className="rf-ctx">{ctx}</span>}
        </div>
      )}
    </div>
  );
}
