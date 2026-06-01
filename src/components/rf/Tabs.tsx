import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export interface Tab<T extends string = string> {
  value: T;
  label: ReactNode;
  badge?: ReactNode;
}

export interface TabsProps<T extends string = string> {
  tabs: Tab<T>[];
  value: T;
  onChange: (value: T) => void;
  className?: string;
}

export function Tabs<T extends string = string>({
  tabs,
  value,
  onChange,
  className,
}: TabsProps<T>) {
  return (
    <div className={cn("rf-tabs", className)}>
      {tabs.map((tab) => (
        <button
          key={tab.value}
          type="button"
          className={tab.value === value ? "active" : ""}
          onClick={() => onChange(tab.value)}
        >
          {tab.label}
          {tab.badge !== undefined && (
            <span className="rf-nav-badge" style={{ marginLeft: 6 }}>
              {tab.badge}
            </span>
          )}
        </button>
      ))}
    </div>
  );
}
