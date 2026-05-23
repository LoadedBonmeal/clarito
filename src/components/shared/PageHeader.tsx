import type { CSSProperties, ReactNode } from "react";

import { cn } from "@/lib/utils";

interface PageHeaderProps {
  title: string;
  meta?: ReactNode;
  actions?: ReactNode;
}

/**
 * Page header dens (h-8) — titlu + meta inline + acțiuni dreapta.
 */
export function PageHeader({ title, meta, actions }: PageHeaderProps) {
  return (
    <div className="flex h-8 shrink-0 items-center justify-between gap-3 border-b border-border bg-background px-3">
      <div className="flex min-w-0 items-center gap-3">
        <h2 className="truncate text-[12px] font-semibold">{title}</h2>
        {meta && (
          <div className="flex items-center gap-2 text-[11px] text-muted-foreground">
            {meta}
          </div>
        )}
      </div>
      {actions && <div className="flex shrink-0 items-center gap-1">{actions}</div>}
    </div>
  );
}

/**
 * Toolbar local pentru pagină (h-8) — pattern SAGA: imediat sub PageHeader,
 * conține filtre, search și acțiuni specifice tabelului.
 */
export function Toolbar({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "flex h-8 shrink-0 items-center gap-1.5 border-b border-border bg-muted/30 px-3",
        className,
      )}
    >
      {children}
    </div>
  );
}

interface PageContentProps {
  children: ReactNode;
  className?: string;
  padded?: boolean;
  style?: CSSProperties;
}

export function PageContent({ children, className, padded = true, style }: PageContentProps) {
  return <div className={cn(padded && "p-3", className)} style={style}>{children}</div>;
}
