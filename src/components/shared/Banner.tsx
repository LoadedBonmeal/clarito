/**
 * Banner — design-native alert strip (.banner / .warn / .danger / .ok from
 * clarito-shell.css). API-compatible with the old rf Banner so call sites only
 * change their import.
 */

import type { ReactNode } from "react";

const VARIANT_CLS: Record<string, string> = {
  info: "",
  warning: " warn",
  error: " danger",
  success: " ok",
};

const ICON: Record<string, string> = {
  info: '<path d="m11.25 11.25.041-.02a.75.75 0 0 1 1.063.852l-.708 2.836a.75.75 0 0 0 1.063.853l.041-.021M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Zm-9-3.75h.008v.008H12V8.25Z"/>',
  warning: '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>',
  error: '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>',
  success: '<path d="M9 12.75 11.25 15 15 9.75M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z"/>',
};

export function Banner({
  variant = "info",
  title,
  actions,
  children,
}: {
  variant?: "info" | "warning" | "error" | "success";
  title?: ReactNode;
  actions?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className={`banner${VARIANT_CLS[variant] ?? ""}`}>
      <svg className="ic" viewBox="0 0 24 24" aria-hidden="true" dangerouslySetInnerHTML={{ __html: ICON[variant] ?? ICON.info }} />
      <div style={{ flex: 1, minWidth: 0 }}>
        {title && <b>{title}</b>}
        {title && children ? <span> — </span> : null}
        {children}
      </div>
      {actions && <div style={{ flexShrink: 0, display: "flex", gap: 6, alignItems: "center" }}>{actions}</div>}
    </div>
  );
}
