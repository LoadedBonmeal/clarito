import type { ReactNode } from "react";
import { cn } from "@/lib/utils";
import { Icon } from "@/components/shared/Icon";

export type BannerVariant = "warning" | "error" | "info" | "success";

const ICON_MAP: Record<BannerVariant, string> = {
  warning: "alert",
  error: "xCircle",
  info: "info",
  success: "checkCircle",
};

export interface BannerProps {
  variant?: BannerVariant;
  title?: ReactNode;
  children?: ReactNode;
  icon?: string;
  className?: string;
  actions?: ReactNode;
}

export function Banner({
  variant = "info",
  title,
  children,
  icon,
  className,
  actions,
}: BannerProps) {
  const iconName = icon ?? ICON_MAP[variant];
  return (
    <div className={cn("rf-banner", `rf-banner--${variant}`, className)}>
      <span className="rf-b-ic">
        <Icon name={iconName} size={16} />
      </span>
      <div className="rf-b-body">
        {title && <div className="rf-b-title">{title}</div>}
        {children && <div className="rf-b-msg">{children}</div>}
        {actions && <div style={{ marginTop: 8 }}>{actions}</div>}
      </div>
    </div>
  );
}
