/**
 * MenuBar — Windows-style menu cu dropdown (Fișier / Editare / ... / Ajutor).
 *
 * Portat din Claude Design (chrome.jsx). Folosește clase din design.css
 * (.menubar, .menubar-item, .menu-dropdown, etc.).
 */

import { useEffect, useRef, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { exit } from "@tauri-apps/plugin-process";
import { useQuery } from "@tanstack/react-query";

import { Icon } from "@/components/shared/Icon";
import { useAppStore } from "@/lib/store";
import { queryClient, queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { fmtShortcut } from "@/lib/platform";

type MenuRow =
  | { type: "row"; icon: string; label: string; kbd?: string; onClick?: () => void; disabled?: boolean }
  | { type: "sep" }
  | { type: "section"; label: string };

function buildMenus(
  navigate: ReturnType<typeof useNavigate>,
  setCommandOpen: (open: boolean) => void,
  theme: string,
  setTheme: (t: "light" | "dark" | "system") => void,
  version: string,
): Record<string, MenuRow[]> {
  return {
    "Fișier": [
      { type: "row", icon: "plus",      label: "Factură nouă",                  kbd: fmtShortcut("Ctrl+N"),       onClick: () => { void navigate({ to: "/invoices/new" }); } },
      { type: "row", icon: "invoiceIn", label: "Înregistrare factură primită",  kbd: fmtShortcut("Ctrl+Shift+N"), onClick: () => { void navigate({ to: "/received" }); } },
      { type: "row", icon: "users",     label: "Contact nou (client/furnizor)", kbd: fmtShortcut("Ctrl+Alt+C"),   onClick: () => { void navigate({ to: "/contacts" }); } },
      { type: "sep" },
      { type: "row", icon: "save",      label: "Salvează",                      kbd: fmtShortcut("Ctrl+S"), onClick: () => { /* context-sensitive — handled by active page */ } },
      { type: "row", icon: "copy",      label: "Salvează ca…",                  kbd: fmtShortcut("Ctrl+Shift+S"), disabled: true },
      { type: "sep" },
      { type: "section", label: "Import / Export" },
      { type: "row", icon: "upload",    label: "Importă XML e-Factura…",                                          onClick: () => { void navigate({ to: "/received" }); } },
      { type: "row", icon: "download",  label: "Exportă SAF-T (D406)…",                                          onClick: () => { void navigate({ to: "/reports" }); } },
      { type: "row", icon: "printer",   label: "Tipărește factura curentă",     kbd: fmtShortcut("Ctrl+P"),       onClick: () => window.print() },
      { type: "sep" },
      { type: "row", icon: "x",         label: "Ieșire",                        kbd: fmtShortcut("Alt+F4"),       onClick: () => { void exit(0); } },
    ],
    "Editare": [
      { type: "row", icon: "pen",     label: "Anulează", kbd: fmtShortcut("Ctrl+Z"), disabled: true },
      { type: "row", icon: "pen",     label: "Refă",     kbd: fmtShortcut("Ctrl+Y"), disabled: true },
      { type: "sep" },
      { type: "row", icon: "copy",    label: "Decupează", kbd: fmtShortcut("Ctrl+X"), disabled: true },
      { type: "row", icon: "copy",    label: "Copiază",   kbd: fmtShortcut("Ctrl+C"), disabled: true },
      { type: "row", icon: "copy",    label: "Lipește",   kbd: fmtShortcut("Ctrl+V"), disabled: true },
      { type: "sep" },
      { type: "row", icon: "search",  label: "Caută…",            kbd: fmtShortcut("Ctrl+F"), disabled: true },
      { type: "row", icon: "command", label: "Paleta de comenzi", kbd: fmtShortcut("Ctrl+K"), onClick: () => setCommandOpen(true) },
    ],
    "Operațiuni": [
      { type: "section", label: "e-Factura" },
      { type: "row", icon: "cloudUp", label: "Trimite factura la ANAF", kbd: "F9",       onClick: () => { void navigate({ to: "/invoices" }); } },
      { type: "row", icon: "refresh", label: "Verifică status mesaje",  kbd: "F10",      onClick: () => { void navigate({ to: "/invoices" }); } },
      { type: "row", icon: "storno",  label: "Storno factură",          kbd: fmtShortcut("Ctrl+F9"), disabled: true },
      { type: "sep" },
      { type: "section", label: "Bancă & casă" },
      { type: "row", icon: "bank",    label: "Punctare extras bancar",  disabled: true },
      { type: "row", icon: "receipt", label: "Înregistrare chitanță",   disabled: true },
      { type: "sep" },
      { type: "section", label: "Bulk" },
      { type: "row", icon: "check",   label: "Trimite selecția la ANAF",    disabled: true },
      { type: "row", icon: "tag",     label: "Aplică categorie pe selecție", disabled: true },
    ],
    "Date": [
      { type: "row", icon: "buildings", label: "Companii administrate", kbd: "G C", onClick: () => { void navigate({ to: "/companies" }); } },
      { type: "row", icon: "users",     label: "Clienți",                           onClick: () => { void navigate({ to: "/contacts" }); } },
      { type: "row", icon: "users",     label: "Furnizori",                         onClick: () => { void navigate({ to: "/contacts" }); } },
      { type: "row", icon: "stock",     label: "Articole / Stocuri",        disabled: true },
      { type: "sep" },
      { type: "row", icon: "database",  label: "Plan de conturi",            disabled: true },
      { type: "row", icon: "tag",       label: "Cote TVA și taxe",           disabled: true },
      { type: "row", icon: "history",   label: "Audit & jurnal modificări",  disabled: true },
    ],
    "Rapoarte": [
      { type: "section", label: "Declarații ANAF" },
      { type: "row", icon: "reports", label: "D300 — Decont TVA",         onClick: () => { void navigate({ to: "/reports" }); } },
      { type: "row", icon: "reports", label: "D394 — Livrări/Achiziții",  onClick: () => { void navigate({ to: "/reports" }); } },
      { type: "row", icon: "reports", label: "D406 — SAF-T",              onClick: () => { void navigate({ to: "/reports" }); } },
      { type: "sep" },
      { type: "section", label: "Operative" },
      { type: "row", icon: "reports", label: "Jurnal de vânzări",         onClick: () => { void navigate({ to: "/reports" }); } },
      { type: "row", icon: "reports", label: "Jurnal de cumpărări",       onClick: () => { void navigate({ to: "/reports" }); } },
      { type: "row", icon: "reports", label: "Cartea mare",           disabled: true },
      { type: "row", icon: "reports", label: "Balanță de verificare", disabled: true },
    ],
    "Vizualizare": [
      { type: "row", icon: "view", label: "Reîncarcă datele",            kbd: "F5", onClick: () => void queryClient.refetchQueries({ type: "active" }) },
      { type: "row", icon: "view", label: "Mărește densitatea (compact)", kbd: fmtShortcut("Ctrl+−"), disabled: true },
      { type: "row", icon: "view", label: "Micșorează densitatea",       kbd: fmtShortcut("Ctrl+="),  disabled: true },
      { type: "sep" },
      { type: "row", icon: "view", label: "Mod întunecat",               kbd: fmtShortcut("Ctrl+Shift+D"), onClick: () => setTheme(theme === "dark" ? "light" : "dark") },
      { type: "row", icon: "view", label: "Arată coloane ascunse…", disabled: true },
    ],
    "Ajutor": [
      { type: "row", icon: "help",     label: "Documentație e-Factura", kbd: "F1", onClick: () => { void import("@tauri-apps/plugin-opener").then(m => m.openUrl("https://mfinante.gov.ro/ro/web/efactura/informatii-tehnice")); } },
      { type: "row", icon: "keyboard", label: "Scurtături tastatură",   kbd: fmtShortcut("Ctrl+/"), onClick: () => setCommandOpen(true) },
      { type: "sep" },
      { type: "row", icon: "info",     label: `Despre RoFactura • v${version}`, disabled: true },
    ],
  };
}

interface MenuBarProps {
  activeCompanyName: string;
  activeCompanyCui?: string;
  onOpenCompanySwitcher: () => void;
  anafStatus?: "ok" | "warn" | "err";
}

export function MenuBar({
  activeCompanyName,
  activeCompanyCui,
  onOpenCompanySwitcher,
  anafStatus = "ok",
}: MenuBarProps) {
  const navigate = useNavigate();
  const theme = useAppStore((s) => s.theme);
  const setTheme = useAppStore((s) => s.setTheme);
  const setCommandOpen = useAppStore((s) => s.setCommandOpen);

  const { data: appInfo } = useQuery({
    queryKey: queryKeys.appInfo,
    queryFn: () => api.system.appInfo(),
    staleTime: Infinity,
  });
  const version = appInfo?.version ?? "0.1.0";

  const MENUS = buildMenus(navigate, setCommandOpen, theme, setTheme, version);

  const [open, setOpen] = useState<string | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onDoc = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(null);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, []);

  return (
    <div className="menubar" ref={ref}>
      <div className="menubar-brand">
        <span className="menubar-brand-mark">eF</span>
        <span>RoFactura</span>
      </div>
      {Object.keys(MENUS).map((name) => (
        <div
          key={name}
          className={"menubar-item" + (open === name ? " open" : "")}
          onMouseDown={(e) => {
            e.preventDefault();
            setOpen(open === name ? null : name);
          }}
          onMouseEnter={() => {
            if (open) setOpen(name);
          }}
        >
          <u>{name[0]}</u>
          {name.slice(1)}
          {open === name && (
            <div
              className="menu-dropdown"
              onMouseDown={(e) => e.stopPropagation()}
            >
              {MENUS[name].map((row, i) => {
                if (row.type === "sep") return <div key={i} className="menu-sep" />;
                if (row.type === "section")
                  return <div key={i} className="menu-section">{row.label}</div>;
                return (
                  <div
                    key={i}
                    className={"menu-row" + (row.disabled ? " opacity-50 pointer-events-none" : "")}
                    onClick={row.disabled ? undefined : row.onClick}
                  >
                    <span className="menu-icon">
                      <Icon name={row.icon} size={13} />
                    </span>
                    <span>{row.label}</span>
                    <span className="menu-kbd">{row.kbd ?? ""}</span>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      ))}
      <div className="menubar-spacer" />
      <span className="menubar-anaf" title="Status conexiune ANAF / SPV">
        <span className={"anaf-dot " + (anafStatus === "ok" ? "" : anafStatus)} />
        ANAF · SPV {anafStatus === "ok" ? "Activ" : anafStatus.toUpperCase()}
      </span>
      <button
        type="button"
        className="menubar-company"
        onClick={onOpenCompanySwitcher}
        title="Schimbă compania activă (Ctrl+K Ctrl+C)"
      >
        <span className="swatch" style={{ background: "var(--accent)" }} />
        <span style={{ fontWeight: 600 }}>{activeCompanyName}</span>
        {activeCompanyCui && <span className="cui">· {activeCompanyCui}</span>}
        <Icon name="caret" size={11} />
      </button>
    </div>
  );
}
