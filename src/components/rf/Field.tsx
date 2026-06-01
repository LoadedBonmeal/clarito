import type { InputHTMLAttributes, SelectHTMLAttributes, ReactNode, TextareaHTMLAttributes } from "react";
import { cn } from "@/lib/utils";
import { Icon } from "@/components/shared/Icon";

// ─── Field wrapper ─────────────────────────────────────────────────────────

export interface FieldProps {
  label?: ReactNode;
  required?: boolean;
  help?: string;
  error?: string;
  children: ReactNode;
  className?: string;
}

export function Field({ label, required, help, error, children, className }: FieldProps) {
  return (
    <div className={cn("rf-field", className)}>
      {label && (
        <label>
          {label}
          {required && <span className="req"> *</span>}
        </label>
      )}
      {children}
      {(error ?? help) && (
        <span className={cn("rf-help", error && "rf-help--err")}>
          {error ?? help}
        </span>
      )}
    </div>
  );
}

// ─── Input ─────────────────────────────────────────────────────────────────

export interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
  error?: boolean;
  num?: boolean;
  iconLeft?: string;
}

export function Input({ error, num, iconLeft, className, ...rest }: InputProps) {
  if (iconLeft) {
    return (
      <div className="rf-input-icon">
        <Icon name={iconLeft} size={14} />
        <input
          className={cn(
            "rf-input",
            error && "rf-input--err",
            num && "rf-input--num",
            className,
          )}
          {...rest}
        />
      </div>
    );
  }
  return (
    <input
      className={cn(
        "rf-input",
        error && "rf-input--err",
        num && "rf-input--num",
        className,
      )}
      {...rest}
    />
  );
}

// ─── Select ────────────────────────────────────────────────────────────────

export interface SelectProps extends SelectHTMLAttributes<HTMLSelectElement> {
  error?: boolean;
}

export function Select({ error, className, children, ...rest }: SelectProps) {
  return (
    <div className="rf-select-wrap">
      <select
        className={cn("rf-select", error && "rf-input--err", className)}
        {...rest}
      >
        {children}
      </select>
      <Icon name="chevDown" size={14} className="rf-chev" />
    </div>
  );
}

// ─── Textarea ──────────────────────────────────────────────────────────────

export interface TextareaProps extends TextareaHTMLAttributes<HTMLTextAreaElement> {
  error?: boolean;
}

export function Textarea({ error, className, ...rest }: TextareaProps) {
  return (
    <textarea
      className={cn("rf-textarea", error && "rf-input--err", className)}
      {...rest}
    />
  );
}

// ─── SearchInput ───────────────────────────────────────────────────────────

export interface SearchInputProps extends InputHTMLAttributes<HTMLInputElement> {
  containerClassName?: string;
}

export function SearchInput({ containerClassName, className, ...rest }: SearchInputProps) {
  return (
    <div className={cn("rf-search", containerClassName)}>
      <Icon name="search" size={14} />
      <input
        type="search"
        className={className}
        {...rest}
      />
    </div>
  );
}
