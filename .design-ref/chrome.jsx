/* ----------------------------------------------------------------------
   Chrome: MenuBar, Ribbon, Sidebar, StatusBar, CompanySwitcher
   ---------------------------------------------------------------------- */

const { useState, useRef, useEffect } = React;

/* ============================================================
   MenuBar — Windows-style File / Edit / ... with dropdowns
   ============================================================ */

const MENUS = {
  "Fișier": [
    { type: "row", icon: "plus",      label: "Factură nouă",                kbd: "Ctrl+N" },
    { type: "row", icon: "invoiceIn", label: "Înregistrare factură primită", kbd: "Ctrl+Shift+N" },
    { type: "row", icon: "users",     label: "Contact nou (client/furnizor)", kbd: "Ctrl+Alt+C" },
    { type: "sep" },
    { type: "row", icon: "save",      label: "Salvează",                    kbd: "Ctrl+S" },
    { type: "row", icon: "copy",      label: "Salvează ca…",                kbd: "Ctrl+Shift+S" },
    { type: "sep" },
    { type: "section", label: "Import / Export" },
    { type: "row", icon: "upload",    label: "Importă XML e-Factura…",      kbd: "" },
    { type: "row", icon: "download",  label: "Exportă SAF-T (D406)…",       kbd: "" },
    { type: "row", icon: "printer",   label: "Tipărește factura curentă",   kbd: "Ctrl+P" },
    { type: "sep" },
    { type: "row", icon: "x",         label: "Ieșire",                      kbd: "Alt+F4" },
  ],
  "Editare": [
    { type: "row", icon: "pen",       label: "Anulează",                    kbd: "Ctrl+Z" },
    { type: "row", icon: "pen",       label: "Refă",                        kbd: "Ctrl+Y" },
    { type: "sep" },
    { type: "row", icon: "copy",      label: "Decupează",                   kbd: "Ctrl+X" },
    { type: "row", icon: "copy",      label: "Copiază",                     kbd: "Ctrl+C" },
    { type: "row", icon: "copy",      label: "Lipește",                     kbd: "Ctrl+V" },
    { type: "sep" },
    { type: "row", icon: "search",    label: "Caută…",                      kbd: "Ctrl+F" },
    { type: "row", icon: "command",   label: "Paleta de comenzi",           kbd: "Ctrl+K" },
  ],
  "Operațiuni": [
    { type: "section", label: "e-Factura" },
    { type: "row", icon: "cloudUp",   label: "Trimite factura la ANAF",     kbd: "F9" },
    { type: "row", icon: "refresh",   label: "Verifică status mesaje",      kbd: "F10" },
    { type: "row", icon: "storno",    label: "Storno factură",              kbd: "Ctrl+F9" },
    { type: "sep" },
    { type: "section", label: "Bancă & casă" },
    { type: "row", icon: "bank",      label: "Punctare extras bancar",      kbd: "" },
    { type: "row", icon: "receipt",   label: "Înregistrare chitanță",       kbd: "" },
    { type: "sep" },
    { type: "section", label: "Bulk" },
    { type: "row", icon: "check",     label: "Trimite selecția la ANAF",    kbd: "" },
    { type: "row", icon: "tag",       label: "Aplică categorie pe selecție", kbd: "" },
  ],
  "Date": [
    { type: "row", icon: "buildings", label: "Companii administrate",       kbd: "G C" },
    { type: "row", icon: "users",     label: "Clienți",                     kbd: "" },
    { type: "row", icon: "users",     label: "Furnizori",                   kbd: "" },
    { type: "row", icon: "stock",     label: "Articole / Stocuri",          kbd: "" },
    { type: "sep" },
    { type: "row", icon: "database",  label: "Plan de conturi",             kbd: "" },
    { type: "row", icon: "tag",       label: "Cote TVA și taxe",            kbd: "" },
    { type: "row", icon: "history",   label: "Audit & jurnal modificări",   kbd: "" },
  ],
  "Rapoarte": [
    { type: "section", label: "Declarații ANAF" },
    { type: "row", icon: "reports",   label: "D300 — Decont TVA",           kbd: "" },
    { type: "row", icon: "reports",   label: "D394 — Livrări/Achiziții",     kbd: "" },
    { type: "row", icon: "reports",   label: "D406 — SAF-T",                kbd: "" },
    { type: "sep" },
    { type: "section", label: "Operative" },
    { type: "row", icon: "reports",   label: "Jurnal de vânzări",           kbd: "" },
    { type: "row", icon: "reports",   label: "Jurnal de cumpărări",         kbd: "" },
    { type: "row", icon: "reports",   label: "Cartea mare",                 kbd: "" },
    { type: "row", icon: "reports",   label: "Balanță de verificare",       kbd: "" },
  ],
  "Vizualizare": [
    { type: "row", icon: "view",      label: "Reîncarcă datele",            kbd: "F5" },
    { type: "row", icon: "view",      label: "Mărește densitatea (compact)", kbd: "Ctrl+−" },
    { type: "row", icon: "view",      label: "Micșorează densitatea",       kbd: "Ctrl+=" },
    { type: "sep" },
    { type: "row", icon: "view",      label: "Mod întunecat",               kbd: "Ctrl+Shift+D" },
    { type: "row", icon: "view",      label: "Arată coloane ascunse…",      kbd: "" },
  ],
  "Ajutor": [
    { type: "row", icon: "help",      label: "Documentație e-Factura",      kbd: "F1" },
    { type: "row", icon: "keyboard",  label: "Scurtături tastatură",        kbd: "Ctrl+/" },
    { type: "sep" },
    { type: "row", icon: "info",      label: "Despre Efactura • v0.1.0",    kbd: "" },
  ],
};

// ... rest of file content saved (full version is the clipboard text just extracted)
// MenuBar, Ribbon, SIDEBAR_MODULES, Sidebar, StatusBar, CompanySwitcher
// See /Users/cris/Projects/efactura-desktop/.design-ref/chrome-full.txt for complete
