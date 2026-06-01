import type { ReactNode } from "react";
import { cn } from "@/lib/utils";
import { Icon } from "@/components/shared/Icon";

export interface EmptyProps {
  icon?: string;
  title?: ReactNode;
  children?: ReactNode;
  actions?: ReactNode;
  className?: string;
}

export function Empty({ icon = "box", title, children, actions, className }: EmptyProps) {
  return (
    <div className={cn("rf-empty", className)}>
      <span className="rf-e-ic">
        <Icon name={icon} size={20} />
      </span>
      {title && <div className="rf-e-title">{title}</div>}
      {children && <div style={{ fontSize: 13 }}>{children}</div>}
      {actions && <div style={{ marginTop: 4 }}>{actions}</div>}
    </div>
  );
}
