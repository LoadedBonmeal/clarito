import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

export function TableShell({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "overflow-hidden border border-border bg-card shadow-[0_1px_0_oklch(0_0_0/0.03)]",
        className,
      )}
    >
      {children}
    </div>
  );
}

export function DataTable({ children }: { children: ReactNode }) {
  return <table className="w-full border-collapse text-[12px]">{children}</table>;
}

export function THead({ children }: { children: ReactNode }) {
  return (
    <thead className="border-b-[1.5px] border-border-strong bg-gradient-to-b from-muted/50 to-muted text-[10px] uppercase tracking-wider text-muted-foreground">
      {children}
    </thead>
  );
}

export function TBody({ children }: { children: ReactNode }) {
  return <tbody className="stripe">{children}</tbody>;
}

interface ThProps {
  children: ReactNode;
  className?: string;
  align?: "left" | "right" | "center";
}

export function Th({ children, className, align = "left" }: ThProps) {
  return (
    <th
      className={cn(
        "h-7 border-r border-border/40 px-2.5 font-semibold last:border-r-0",
        align === "right" && "text-right",
        align === "center" && "text-center",
        align === "left" && "text-left",
        className,
      )}
    >
      {children}
    </th>
  );
}

interface TdProps {
  children: ReactNode;
  className?: string;
  align?: "left" | "right" | "center";
  mono?: boolean;
}

export function Td({ children, className, align = "left", mono }: TdProps) {
  return (
    <td
      className={cn(
        "h-7 border-b border-border/40 px-2.5",
        align === "right" && "text-right",
        align === "center" && "text-center",
        align === "left" && "text-left",
        mono && "font-mono text-[11px]",
        className,
      )}
    >
      {children}
    </td>
  );
}

interface TrProps {
  children: ReactNode;
  onClick?: () => void;
  className?: string;
}

export function Tr({ children, onClick, className }: TrProps) {
  return (
    <tr
      onClick={onClick}
      className={cn(
        onClick && "cursor-pointer hover:bg-accent/70",
        className,
      )}
    >
      {children}
    </tr>
  );
}
