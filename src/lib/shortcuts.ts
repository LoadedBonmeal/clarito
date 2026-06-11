/**
 * shortcuts — centralized shortcut definitions for the whole app.
 *
 * Keys are stored in canonical "Ctrl+..." form and rendered through
 * fmtShortcut() at display time so macOS gets ⌘/⇧/⌥ symbols.
 *
 * Descriptions are i18n keys resolved at render time — call
 * getShortcutGroups(t) from a component that owns a `t` instance.
 */

import { isMac } from "@/lib/platform";

export interface ShortcutDef {
  keys: string;
  description: string;
}

export interface ShortcutGroup {
  title: string;
  items: ShortcutDef[];
}

export function getShortcutGroups(t: (key: string) => string): ShortcutGroup[] {
  return [
    {
      title: t("shared.shortcuts.groups.general"),
      items: [
        { keys: "Ctrl+K",   description: t("shared.shortcuts.items.commandPalette") },
        { keys: "Ctrl+N",   description: t("shared.shortcuts.items.newInvoice") },
        { keys: "Ctrl+/",   description: t("shared.shortcuts.items.shortcutsList") },
        { keys: "F5",       description: t("shared.shortcuts.items.refreshData") },
      ],
    },
    {
      title: t("shared.shortcuts.groups.invoiceEditor"),
      items: [
        { keys: "Ctrl+S",     description: t("shared.shortcuts.items.saveDraft") },
        { keys: "Ctrl+Enter", description: t("shared.shortcuts.items.sendAnaf") },
        { keys: "Ctrl+P",     description: t("shared.shortcuts.items.print") },
        { keys: "Esc",        description: t("shared.shortcuts.items.cancelClose") },
      ],
    },
    {
      title: t("shared.shortcuts.groups.quickNav"),
      items: [
        { keys: "Ctrl+Shift+N", description: t("shared.shortcuts.items.recordReceived") },
        { keys: "Ctrl+Alt+C",   description: t("shared.shortcuts.items.newContact") },
        { keys: "Ctrl+Shift+S", description: t("shared.shortcuts.items.saveAs") },
        { keys: isMac ? "Cmd+Q" : "Alt+F4", description: t("shared.shortcuts.items.quit") },
      ],
    },
    {
      title: t("shared.shortcuts.groups.anafOps"),
      items: [
        { keys: "F9",        description: t("shared.shortcuts.items.sendInvoiceAnaf") },
        { keys: "F10",       description: t("shared.shortcuts.items.checkAnafStatus") },
        { keys: "Ctrl+F9",   description: t("shared.shortcuts.items.creditNote") },
        { keys: "Ctrl+D",    description: t("shared.shortcuts.items.downloadSpv") },
      ],
    },
    {
      title: t("shared.shortcuts.groups.company"),
      items: [
        { keys: "Ctrl+Shift+D", description: t("shared.shortcuts.items.toggleTheme") },
      ],
    },
  ];
}
