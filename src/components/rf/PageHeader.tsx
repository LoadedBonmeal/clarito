import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export interface PageHeaderProps {
  /** Screen name / breadcrumb label */
  screen?: ReactNode;
  title: ReactNode;
  /** Short subtitle below the title */
  sub?: ReactNode;
  /** Longer description paragraph */
  desc?: ReactNode;
  /** Action buttons placed at the right */
  actions?: ReactNode;
  className?: string;
}

export function PageHeader({ screen: _screen, title, sub, desc, actions, className }: PageHeaderProps) {
  return (
    <div className={cn("rf-page-head", className)}>
      <div>
        <h1 className="rf-page-title">{title}</h1>
        {sub && <div className="rf-page-sub">{sub}</div>}
        {desc && <div className="rf-page-desc">{desc}</div>}
      </div>
      {actions && (
        <div className="rf-toolbar-row" style={{ flexShrink: 0 }}>
          {actions}
        </div>
      )}
    </div>
  );
}
