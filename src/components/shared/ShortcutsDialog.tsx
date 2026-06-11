/**
 * ShortcutsDialog — keyboard shortcuts cheatsheet.
 *
 * Renders SHORTCUT_GROUPS from lib/shortcuts.ts; key labels pass through
 * fmtShortcut so macOS sees ⌘/⇧/⌥ while Windows shows "Ctrl".
 * Open/close is controlled externally via open + onOpenChange props.
 */

import { useTranslation } from "react-i18next";

import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from "@/components/ui/dialog";
import { getShortcutGroups } from "@/lib/shortcuts";
import { fmtShortcut } from "@/lib/platform";

interface ShortcutsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function ShortcutsDialog({ open, onOpenChange }: ShortcutsDialogProps) {
  const { t } = useTranslation();
  const groups = getShortcutGroups(t);
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="sm:max-w-xl"
        style={{ maxHeight: "80vh", overflowY: "auto" }}
      >
        <DialogHeader>
          <DialogTitle>{t("shared.shortcuts.title")}</DialogTitle>
          <DialogDescription>
            {t("shared.shortcuts.description")}
          </DialogDescription>
        </DialogHeader>

        <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
          {groups.map((group) => (
            <div key={group.title}>
              <div
                style={{
                  fontSize: 10.5,
                  fontWeight: 700,
                  textTransform: "uppercase",
                  letterSpacing: "0.07em",
                  color: "var(--text-muted)",
                  marginBottom: 8,
                  paddingBottom: 4,
                  borderBottom: "1px solid var(--border-soft, var(--border))",
                }}
              >
                {group.title}
              </div>
              <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                {group.items.map((item) => (
                  <div
                    key={item.keys}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "space-between",
                      padding: "3px 0",
                    }}
                  >
                    <span
                      style={{
                        fontSize: 12,
                        color: "var(--text)",
                      }}
                    >
                      {item.description}
                    </span>
                    <span
                      className="kbd"
                      style={{
                        fontFamily: "var(--font-mono, monospace)",
                        fontSize: 11,
                        flexShrink: 0,
                        marginLeft: 16,
                      }}
                    >
                      {fmtShortcut(item.keys)}
                    </span>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  );
}
