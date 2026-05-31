/**
 * shortcuts — centralized shortcut definitions for the whole app.
 *
 * Keys are stored in canonical "Ctrl+..." form and rendered through
 * fmtShortcut() at display time so macOS gets ⌘/⇧/⌥ symbols.
 */

export interface ShortcutDef {
  keys: string;
  description: string;
}

export interface ShortcutGroup {
  title: string;
  items: ShortcutDef[];
}

export const SHORTCUT_GROUPS: ShortcutGroup[] = [
  {
    title: "General",
    items: [
      { keys: "Ctrl+K",   description: "Paletă de comenzi" },
      { keys: "Ctrl+N",   description: "Factură nouă" },
      { keys: "Ctrl+/",   description: "Această listă de scurtături" },
      { keys: "F5",       description: "Reîmprospătează datele" },
    ],
  },
  {
    title: "Editor factură",
    items: [
      { keys: "Ctrl+S",     description: "Salvează ciorna" },
      { keys: "Ctrl+Enter", description: "Trimite la ANAF" },
      { keys: "Ctrl+P",     description: "Tipărește / Preview" },
      { keys: "Esc",        description: "Renunță / Închide" },
    ],
  },
  {
    title: "Navigare rapidă",
    items: [
      { keys: "Ctrl+Shift+N", description: "Înregistrare factură primită" },
      { keys: "Ctrl+Alt+C",   description: "Contact nou (client/furnizor)" },
      { keys: "Ctrl+Shift+S", description: "Salvează ca… (duplică factura)" },
      { keys: "Alt+F4",       description: "Ieșire din aplicație" },
    ],
  },
  {
    title: "Operațiuni ANAF",
    items: [
      { keys: "F9",        description: "Trimite factura la ANAF" },
      { keys: "F10",       description: "Verifică status mesaje ANAF" },
      { keys: "Ctrl+F9",   description: "Storno factură" },
      { keys: "Ctrl+D",    description: "Descarcă SPV" },
    ],
  },
  {
    title: "Companie",
    items: [
      { keys: "Ctrl+Shift+D", description: "Comută mod întunecat / luminos" },
    ],
  },
];
