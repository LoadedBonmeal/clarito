import { type ButtonHTMLAttributes, useState } from "react";
import { cn } from "@/lib/utils";
import { Icon } from "@/components/shared/Icon";

export interface IconBtnProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  icon: string;
  size?: number;
  ghost?: boolean;
}

export function IconBtn({ icon, size = 15, ghost, className, onClick, ...rest }: IconBtnProps) {
  // One-shot press animation: add `rf-pressing` on click, remove when the
  // icon's keyframe finishes (animationend bubbles up from the <svg>).
  const [pressing, setPressing] = useState(false);
  return (
    <button
      className={cn("rf-icon-btn", ghost && "rf-icon-btn--ghost", pressing && "rf-pressing", className)}
      {...rest}
      onClick={(e) => {
        setPressing(true);
        onClick?.(e);
      }}
      onAnimationEnd={() => setPressing(false)}
    >
      <Icon name={icon} size={size} />
    </button>
  );
}
