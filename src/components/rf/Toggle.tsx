import type { ChangeEvent, ReactNode } from "react";
import { cn } from "@/lib/utils";

// ─── Toggle (switch) ────────────────────────────────────────────────────────

export interface ToggleProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  className?: string;
  "aria-label"?: string;
}

export function Toggle({ checked, onChange, disabled, className, "aria-label": ariaLabel }: ToggleProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={ariaLabel}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn("rf-toggle", checked && "on", className)}
    />
  );
}

// ─── Checkbox ──────────────────────────────────────────────────────────────

export interface CheckboxProps {
  checked: boolean;
  onChange: (e: ChangeEvent<HTMLInputElement>) => void;
  disabled?: boolean;
  className?: string;
  children?: ReactNode;
  id?: string;
}

export function Checkbox({ checked, onChange, disabled, className, children, id }: CheckboxProps) {
  return (
    <label className={cn("rf-check", className)}>
      <input
        type="checkbox"
        id={id}
        checked={checked}
        onChange={onChange}
        disabled={disabled}
      />
      <span className="rf-box">
        {checked && (
          <svg width="10" height="8" viewBox="0 0 10 8" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M1 4l3 3 5-6" />
          </svg>
        )}
      </span>
      {children}
    </label>
  );
}
