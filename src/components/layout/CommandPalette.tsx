/**
 * CommandPalette — Ctrl+K overlay.
 *
 * Design re-skin: .palette-back backdrop (modal-back-like, top-centered) →
 * .palette panel (.scr-search input row · .col-title group labels ·
 * .pop-item rows with Ic icons · .kbd hints · .palette-foot).
 * Page-specific rules: src/styles/page-palette.css.
 *
 * All existing commands preserved verbatim (fuzzy search, keyboard nav,
 * navigate actions, recent invoices, "Comută tema").
 */

import { useEffect, useRef, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { useAppStore } from "@/lib/store";
import { api } from "@/lib/tauri";
import { queryKeys } from "@/lib/queries";
import { fmtShortcut } from "@/lib/platform";

interface Command {
  id: string;
  label: string;
  hint?: string;
  icon: string;
  section: string;
  action: () => void;
}

export function CommandPalette() {
  const { t } = useTranslation();
  const commandOpen = useAppStore((s) => s.commandOpen);
  const setCommandOpen = useAppStore((s) => s.setCommandOpen);
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const theme = useAppStore((s) => s.theme);
  const setTheme = useAppStore((s) => s.setTheme);
  const navigate = useNavigate();
  const [query, setQuery] = useState("");
  const [activeIdx, setActiveIdx] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  // Load recent invoices for quick navigation
  const { data: recentInvoices } = useQuery({
    queryKey: queryKeys.invoices.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () =>
      api.invoices.list({
        companyId: activeCompanyId ?? undefined,
        page: { offset: 0, limit: 5 },
      }),
    enabled: commandOpen && !!activeCompanyId,
  });

  const close = () => {
    setCommandOpen(false);
    setQuery("");
    setActiveIdx(0);
  };

  // Focus input when opened
  useEffect(() => {
    if (commandOpen) {
      setTimeout(() => inputRef.current?.focus(), 10);
      setQuery("");
      setActiveIdx(0);
    }
  }, [commandOpen]);

  // Build commands list (all original commands + "Comută tema")
  const sectionNav = t("shell.palette.sections.navigate");
  const sectionActions = t("shell.palette.sections.actions");
  const COMMANDS: Command[] = [
    // Navigare
    {
      id: "nav-dashboard",
      label: t("shell.palette.dashboard"),
      hint: "G D",
      icon: "grid",
      section: sectionNav,
      action: () => { navigate({ to: "/" }); close(); },
    },
    {
      id: "nav-invoices",
      label: t("shell.nav.invoicesIssued"),
      hint: "G F",
      icon: "docUp",
      section: sectionNav,
      action: () => { navigate({ to: "/invoices" }); close(); },
    },
    {
      id: "nav-received",
      label: t("shell.nav.invoicesReceived"),
      hint: "G R",
      icon: "docDown",
      section: sectionNav,
      action: () => { navigate({ to: "/received" }); close(); },
    },
    {
      id: "nav-contacts",
      label: t("shell.nav.contacts"),
      hint: "G C",
      icon: "users",
      section: sectionNav,
      action: () => { navigate({ to: "/contacts" }); close(); },
    },
    {
      id: "nav-companies",
      label: t("shell.nav.companies"),
      icon: "building",
      section: sectionNav,
      action: () => { navigate({ to: "/companies" }); close(); },
    },
    {
      id: "nav-reports",
      label: t("shell.nav.reports"),
      icon: "chart",
      section: sectionNav,
      action: () => { navigate({ to: "/reports" }); close(); },
    },
    {
      id: "nav-notifications",
      label: t("shell.palette.anafNotifications"),
      icon: "bell",
      section: sectionNav,
      action: () => { navigate({ to: "/notifications" }); close(); },
    },
    {
      id: "nav-settings",
      label: t("shell.profile.settings"),
      icon: "cog",
      section: sectionNav,
      action: () => { navigate({ to: "/settings" }); close(); },
    },
    // Acțiuni
    {
      id: "act-new-invoice",
      label: t("shell.topbar.newInvoice"),
      hint: fmtShortcut("Ctrl+N"),
      icon: "plus",
      section: sectionActions,
      action: () => { navigate({ to: "/invoices/new" }); close(); },
    },
    {
      id: "act-new-contact",
      label: t("shell.palette.openContacts"),
      icon: "users",
      section: sectionActions,
      action: () => { navigate({ to: "/contacts" }); close(); },
    },
    {
      id: "act-new-company",
      label: t("shell.palette.newCompany"),
      icon: "building",
      section: sectionActions,
      action: () => { navigate({ to: "/companies/new" }); close(); },
    },
    // Comută tema
    {
      id: "act-toggle-theme",
      label: theme === "dark" ? t("shell.palette.toggleThemeLight") : t("shell.palette.toggleThemeDark"),
      icon: "eye",
      section: sectionActions,
      action: () => {
        setTheme(theme === "dark" ? "light" : "dark");
        close();
      },
    },
  ];

  // Add recent invoices as commands
  const recentCmds: Command[] = (recentInvoices?.items ?? []).map((inv) => ({
    id: `inv-${inv.id}`,
    label: t("shell.palette.invoiceItem", { n: inv.fullNumber }),
    hint: inv.issueDate,
    icon: "docUp",
    section: t("shell.palette.sections.recent"),
    action: () => {
      navigate({ to: "/invoices/$id", params: { id: inv.id } });
      close();
    },
  }));

  const allCommands = [...recentCmds, ...COMMANDS];

  // Filter by query
  const filtered = query.trim()
    ? allCommands.filter(
        (c) =>
          c.label.toLowerCase().includes(query.toLowerCase()) ||
          c.section.toLowerCase().includes(query.toLowerCase()),
      )
    : allCommands;

  // Group by section
  const sections = Array.from(new Set(filtered.map((c) => c.section)));

  // Keyboard navigation
  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      close();
      return;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActiveIdx((i) => Math.min(i + 1, filtered.length - 1));
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      setActiveIdx((i) => Math.max(i - 1, 0));
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      filtered[activeIdx]?.action();
      return;
    }
  };

  if (!commandOpen) return null;

  let globalIdx = 0;

  return (
    <div className="palette-back" onClick={close}>
      <div
        className="palette"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={handleKeyDown}
      >
        <div className="palette-search">
          <div className="scr-search">
            <Ic name="lens" />
            <input
              ref={inputRef}
              value={query}
              onChange={(e) => {
                setQuery(e.target.value);
                setActiveIdx(0);
              }}
              placeholder={t("shell.palette.placeholder")}
              autoComplete="off"
            />
            {query && (
              <button type="button" className="mini-btn" onClick={() => setQuery("")}>
                <Ic name="xMark" />
              </button>
            )}
          </div>
        </div>

        <div className="palette-list">
          {filtered.length === 0 ? (
            <div className="palette-empty">
              {t("shell.palette.noResults", { query })}
            </div>
          ) : (
            sections.map((section) => {
              const cmds = filtered.filter((c) => c.section === section);
              return (
                <div key={section}>
                  <div className="col-title">{section}</div>
                  {cmds.map((cmd) => {
                    const idx = globalIdx++;
                    return (
                      <button
                        key={cmd.id}
                        type="button"
                        className={`pop-item${idx === activeIdx ? " active" : ""}`}
                        onMouseEnter={() => setActiveIdx(idx)}
                        onClick={cmd.action}
                      >
                        <Ic name={cmd.icon} />
                        <span style={{ flex: 1, textAlign: "left" }}>{cmd.label}</span>
                        {cmd.hint && <span className="kbd num">{cmd.hint}</span>}
                      </button>
                    );
                  })}
                </div>
              );
            })
          )}
        </div>

        <div className="palette-foot">
          <span><span className="kbd">↑↓</span> {t("shell.palette.footNavigate")}</span>
          <span><span className="kbd">↵</span> {t("shell.palette.footRun")}</span>
          <span><span className="kbd">Esc</span> {t("shell.palette.footClose")}</span>
        </div>
      </div>
    </div>
  );
}
