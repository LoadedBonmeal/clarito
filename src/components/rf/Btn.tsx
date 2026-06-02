import { type ButtonHTMLAttributes, type ReactNode } from "react";
import { cn } from "@/lib/utils";
import { Icon } from "@/components/shared/Icon";

export type BtnVariant = "primary" | "secondary" | "ghost" | "danger";
export type BtnSize = "sm" | "md" | "lg";

export interface BtnProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: BtnVariant;
  size?: BtnSize;
  icon?: string;
  iconRight?: string;
  block?: boolean;
  children?: ReactNode;
}

const ICON_SIZE: Record<BtnSize, number> = { sm: 13, md: 15, lg: 16 };

export function Btn({
  variant = "secondary",
  size = "md",
  icon,
  iconRight,
  block,
  children,
  className,
  ...rest
}: BtnProps) {
  const cls = cn(
    "rf-btn",
    `rf-btn--${variant}`,
    size === "sm" && "rf-btn--sm",
    size === "lg" && "rf-btn--lg",
    block && "rf-btn--block",
    className,
  );
  const icoSize = ICON_SIZE[size];
  return (
    <button className={cls} {...rest}>
      {icon && <Icon name={icon} size={icoSize} />}
      {children}
      {iconRight && <Icon name={iconRight} size={icoSize} />}
    </button>
  );
}
