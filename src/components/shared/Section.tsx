import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

interface SectionProps {
  title?: string;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
  /** When true, adds a 3px left border in accent color to visually highlight the section. */
  highlight?: boolean;
  /** Optional badge text rendered inline after the title (e.g. "NOU"). */
  badge?: string;
}

/**
 * Container bordurat, fără radius, cu header subtle gradient.
 */
export function Section({ title, actions, children, className, highlight, badge }: SectionProps) {
  return (
    <section
      className={cn(
        "overflow-hidden border border-border bg-background shadow-[0_1px_0_oklch(0_0_0/0.03)]",
        className,
      )}
      style={highlight ? { borderLeft: "3px solid var(--accent)" } : undefined}
    >
      {(title || actions) && (
        <header className="flex h-7 items-center justify-between border-b border-border bg-gradient-to-b from-muted/40 to-muted/70 px-2.5">
          {title && (
            <span className="flex items-center text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
              {title}
              {badge && (
                <span
                  style={{
                    fontSize: 9,
                    padding: "2px 5px",
                    background: "var(--accent-soft)",
                    color: "var(--accent)",
                    marginLeft: 8,
                    textTransform: "none",
                    letterSpacing: 0,
                    fontWeight: 600,
                    lineHeight: 1.4,
                  }}
                >
                  {badge}
                </span>
              )}
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
  /** When set, renders the label as a <label> element linked to the given input id. */
  htmlFor?: string;
}

export function FieldRow({ label, children, mono, htmlFor }: FieldRowProps) {
  return (
    <div className="flex border-b border-border last:border-b-0">
      <div className="flex w-[160px] shrink-0 items-center border-r border-border bg-muted/30 px-2.5 py-1 text-[11px] text-muted-foreground">
        {htmlFor ? (
          <label htmlFor={htmlFor} style={{ cursor: "pointer" }}>
            {label}
          </label>
        ) : (
          label
        )}
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
