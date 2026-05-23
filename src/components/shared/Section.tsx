import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

interface SectionProps {
  title?: string;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
}

/**
 * Container bordurat, fără radius, cu header subtle gradient.
 */
export function Section({ title, actions, children, className }: SectionProps) {
  return (
    <section
      className={cn(
        "overflow-hidden border border-border bg-background shadow-[0_1px_0_oklch(0_0_0/0.03)]",
        className,
      )}
    >
      {(title || actions) && (
        <header className="flex h-7 items-center justify-between border-b border-border bg-gradient-to-b from-muted/40 to-muted/70 px-2.5">
          {title && (
            <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
              {title}
            </span>
          )}
          {actions && <div className="flex items-center gap-1">{actions}</div>}
        </header>
      )}
      {children}
    </section>
  );
}

interface FieldRowProps {
  label: string;
  children: ReactNode;
  mono?: boolean;
}

export function FieldRow({ label, children, mono }: FieldRowProps) {
  return (
    <div className="flex border-b border-border last:border-b-0">
      <div className="flex w-[160px] shrink-0 items-center border-r border-border bg-muted/30 px-2.5 py-1 text-[11px] text-muted-foreground">
        {label}
      </div>
      <div
        className={cn(
          "flex min-w-0 flex-1 items-center px-2.5 py-1 text-[12px]",
          mono && "font-mono text-[11px]",
        )}
      >
        {children}
      </div>
    </div>
  );
}

export function FieldGroup({ children }: { children: ReactNode }) {
  return <div className="divide-y divide-border">{children}</div>;
}
