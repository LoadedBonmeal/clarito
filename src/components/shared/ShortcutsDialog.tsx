/**
 * ShortcutsDialog — keyboard shortcuts cheatsheet (design .modal-back/.modal).
 *
 * Renders SHORTCUT_GROUPS from lib/shortcuts.ts; key labels pass through
 * fmtShortcut so macOS sees ⌘/⇧/⌥ while Windows shows "Ctrl".
 * Open/close is controlled externally via open + onOpenChange props.
 */

import { useEffect } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { getShortcutGroups } from "@/lib/shortcuts";
import { fmtShortcut } from "@/lib/platform";

interface ShortcutsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function ShortcutsDialog({ open, onOpenChange }: ShortcutsDialogProps) {
  const { t } = useTranslation();
  const groups = getShortcutGroups(t);

  useEffect(() => {
    if (!open) return;
    const h = (e: KeyboardEvent) => { if (e.key === "Escape") onOpenChange(false); };
    document.addEventListener("keydown", h);
    return () => document.removeEventListener("keydown", h);
  }, [open, onOpenChange]);

  if (!open) return null;

  return createPortal(
    <div
      className="modal-back show"
      style={{ position: "fixed" }}
      onMouseDown={(e) => { if (e.target === e.currentTarget) onOpenChange(false); }}
    >
      <div className="modal" style={{ width: 560, maxHeight: "80vh", display: "flex", flexDirection: "column" }}>
        <div className="modal-head">
          <div>
            <div className="mt">{t("shared.shortcuts.title")}</div>
            <div className="ms">{t("shared.shortcuts.description")}</div>
          </div>
          <button className="modal-x" onClick={() => onOpenChange(false)} aria-label={t("shared.common.close")}>
            <Ic name="xMark" />
          </button>
        </div>
        <div className="modal-body" style={{ overflowY: "auto", display: "flex", flexDirection: "column", gap: 18 }}>
          {groups.map((group) => (
            <div key={group.title}>
              <div className="col-title" style={{ padding: "0 0 6px", borderBottom: "1px solid var(--line)", marginBottom: 6 }}>
                {group.title}
              </div>
              <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                {group.items.map((item) => (
                  <div
                    key={item.keys}
                    style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "3px 0" }}
                  >
                    <span style={{ fontSize: 12.5, color: "var(--text)" }}>{item.description}</span>
                    <span className="kbd" style={{ flexShrink: 0, marginLeft: 16 }}>{fmtShortcut(item.keys)}</span>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>,
    document.body,
  );
}
