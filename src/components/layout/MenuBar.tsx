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
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";

type MenuRow =
  | { type: "row"; icon: string; label: string; kbd?: string; onClick?: () => void }
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
      { type: "row", icon: "plus",      label: "Factură nouă",                  kbd: "Ctrl+N",       onClick: () => { void navigate({ to: "/invoices/new" }); } },
      { type: "row", icon: "invoiceIn", label: "Înregistrare factură primită",  kbd: "Ctrl+Shift+N", onClick: () => { void navigate({ to: "/received" }); } },
      { type: "row", icon: "users",     label: "Contact nou (client/furnizor)", kbd: "Ctrl+Alt+C",   onClick: () => { void navigate({ to: "/contacts" }); } },
      { type: "sep" },
      { type: "row", icon: "save",      label: "Salvează",                      kbd: "Ctrl+S" },
      { type: "row", icon: "copy",      label: "Salvează ca…",                  kbd: "Ctrl+Shift+S" },
      { type: "sep" },
      { type: "section", label: "Import / Export" },
      { type: "row", icon: "upload",    label: "Importă XML e-Factura…" },
      { type: "row", icon: "download",  label: "Exportă SAF-T (D406)…" },
      { type: "row", icon: "printer",   label: "Tipărește factura curentă",     kbd: "Ctrl+P" },
      { type: "sep" },
      { type: "row", icon: "x",         label: "Ieșire",                        kbd: "Alt+F4",       onClick: () => { void exit(0); } },
    ],
    "Editare": [
      { type: "row", icon: "pen",     label: "Anulează", kbd: "Ctrl+Z" },
      { type: "row", icon: "pen",     label: "Refă",     kbd: "Ctrl+Y" },
      { type: "sep" },
      { type: "row", icon: "copy",    label: "Decupează", kbd: "Ctrl+X" },
      { type: "row", icon: "copy",    label: "Copiază",   kbd: "Ctrl+C" },
      { type: "row", icon: "copy",    label: "Lipește",   kbd: "Ctrl+V" },
      { type: "sep" },
      { type: "row", icon: "search",  label: "Caută…",            kbd: "Ctrl+F" },
      { type: "row", icon: "command", label: "Paleta de comenzi", kbd: "Ctrl+K", onClick: () => setCommandOpen(true) },
    ],
    "Operațiuni": [
      { type: "section", label: "e-Factura" },
      { type: "row", icon: "cloudUp", label: "Trimite factura la ANAF", kbd: "F9",       onClick: () => { void navigate({ to: "/invoices" }); } },
      { type: "row", icon: "refresh", label: "Verifică status mesaje",  kbd: "F10",      onClick: () => { void navigate({ to: "/invoices" }); } },
      { type: "row", icon: "storno",  label: "Storno factură",          kbd: "Ctrl+F9" },
      { type: "sep" },
      { type: "section", label: "Bancă & casă" },
      { type: "row", icon: "bank",    label: "Punctare extras bancar" },
      { type: "row", icon: "receipt", label: "Înregistrare chitanță" },
      { type: "sep" },
      { type: "section", label: "Bulk" },
      { type: "row", icon: "check",   label: "Trimite selecția la ANAF" },
      { type: "row", icon: "tag",     label: "Aplică categorie pe selecție" },
    ],
    "Date": [
      { type: "row", icon: "buildings", label: "Companii administrate", kbd: "G C", onClick: () => { void navigate({ to: "/companies" }); } },
      { type: "row", icon: "users",     label: "Clienți",                           onClick: () => { void navigate({ to: "/contacts" }); } },
      { type: "row", icon: "users",     label: "Furnizori",                         onClick: () => { void navigate({ to: "/contacts" }); } },
      { type: "row", icon: "stock",     label: "Articole / Stocuri" },
      { type: "sep" },
      { type: "row", icon: "database",  label: "Plan de conturi" },
      { type: "row", icon: "tag",       label: "Cote TVA și taxe" },
      { type: "row", icon: "history",   label: "Audit & jurnal modificări" },
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
      { type: "row", icon: "reports", label: "Cartea mare",               onClick: () => { void navigate({ to: "/reports" }); } },
      { type: "row", icon: "reports", label: "Balanță de verificare",     onClick: () => { void navigate({ to: "/reports" }); } },
    ],
    "Vizualizare": [
      { type: "row", icon: "view", label: "Reîncarcă datele",            kbd: "F5" },
      { type: "row", icon: "view", label: "Mărește densitatea (compact)", kbd: "Ctrl+−" },
      { type: "row", icon: "view", label: "Micșorează densitatea",       kbd: "Ctrl+=" },
      { type: "sep" },
      { type: "row", icon: "view", label: "Mod întunecat",               kbd: "Ctrl+Shift+D", onClick: () => setTheme(theme === "dark" ? "light" : "dark") },
      { type: "row", icon: "view", label: "Arată coloane ascunse…" },
    ],
    "Ajutor": [
      { type: "row", icon: "help",     label: "Documentație e-Factura", kbd: "F1" },
      { type: "row", icon: "keyboard", label: "Scurtături tastatură",   kbd: "Ctrl+/" },
      { type: "sep" },
      { type: "row", icon: "info",     label: `Despre Efactura • v${version}` },
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
        <span>Efactura</span>
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
                  <div key={i} className="menu-row" onClick={row.onClick}>
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
        ANAF · SPV {anafStatus === "ok" ? "OK" : anafStatus.toUpperCase()}
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
