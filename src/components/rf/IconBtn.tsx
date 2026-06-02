import { type ButtonHTMLAttributes } from "react";
import { cn } from "@/lib/utils";
import { Icon } from "@/components/shared/Icon";

export interface IconBtnProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  icon: string;
  size?: number;
  ghost?: boolean;
}

export function IconBtn({ icon, size = 15, ghost, className, ...rest }: IconBtnProps) {
  return (
    <button
      className={cn("rf-icon-btn", ghost && "rf-icon-btn--ghost", className)}
      {...rest}
    >
      <Icon name={icon} size={size} />
    </button>
  );
}
