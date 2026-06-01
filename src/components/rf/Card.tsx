import type { ReactNode } from "react";
import { cn } from "@/lib/utils";
import { Icon } from "@/components/shared/Icon";

export interface CardProps {
  children: ReactNode;
  className?: string;
  pad?: boolean;
}

export function Card({ children, className, pad }: CardProps) {
  return (
    <div className={cn("rf-card", pad && "rf-card-pad", className)}>
      {children}
    </div>
  );
}

export interface SectionCardProps {
  icon?: string;
  title: string;
  subtitle?: string;
  children: ReactNode;
  className?: string;
  actions?: ReactNode;
}

export function SectionCard({ icon, title, subtitle, children, className, actions }: SectionCardProps) {
  return (
    <div className={cn("rf-card", className)}>
      <div className="rf-card-head">
        {icon && (
          <span className="rf-ic">
            <Icon name={icon} size={16} />
          </span>
        )}
        <div style={{ flex: 1 }}>
          <h3>{title}</h3>
          {subtitle && <div className="rf-sub">{subtitle}</div>}
        </div>
        {actions}
      </div>
      {children}
    </div>
  );
}
