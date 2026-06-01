import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export type BadgeVariant = "success" | "error" | "warning" | "info" | "neutral";

export interface BadgeProps {
  variant?: BadgeVariant;
  dot?: boolean;
  children: ReactNode;
  className?: string;
}

export function Badge({ variant = "neutral", dot = true, children, className }: BadgeProps) {
  return (
    <span className={cn("rf-badge", `rf-badge--${variant}`, className)}>
      {dot && <span className="rf-dot" />}
      {children}
    </span>
  );
}
