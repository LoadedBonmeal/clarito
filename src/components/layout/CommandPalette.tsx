/**
 * CommandPalette — Ctrl+K overlay cu căutare comenzi, navigare și facturi recente.
 *
 * Folosește clasele din design.css:
 * .palette-scrim, .palette, .palette-input, .palette-list,
 * .palette-section, .palette-row, .palette-footer
 */

import { useEffect, useRef, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";

import { Icon } from "@/components/shared/Icon";
import { useAppStore } from "@/lib/store";
import { api } from "@/lib/tauri";
import { queryKeys } from "@/lib/queries";

interface Command {
  id: string;
  label: string;
  hint?: string;
  icon: string;
  section: string;
  action: () => void;
}

export function CommandPalette() {
  const commandOpen = useAppStore((s) => s.commandOpen);
  const setCommandOpen = useAppStore((s) => s.setCommandOpen);
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
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

  // Build commands list
  const COMMANDS: Command[] = [
    // Navigare
    {
      id: "nav-dashboard",
      label: "Privire generală (Dashboard)",
      hint: "G D",
      icon: "data",
      section: "Navigare",
      action: () => {
        navigate({ to: "/" });
        close();
      },
    },
    {
      id: "nav-invoices",
      label: "Facturi emise",
      hint: "G F",
      icon: "invoice",
      section: "Navigare",
      action: () => {
        navigate({ to: "/invoices" });
        close();
      },
    },
    {
      id: "nav-received",
      label: "Facturi primite",
      hint: "G R",
      icon: "invoiceIn",
      section: "Navigare",
      action: () => {
        navigate({ to: "/received" });
        close();
      },
    },
    {
      id: "nav-contacts",
      label: "Clienți & Furnizori",
      hint: "G C",
      icon: "users",
      section: "Navigare",
      action: () => {
        navigate({ to: "/contacts" });
        close();
      },
    },
    {
      id: "nav-companies",
      label: "Companii",
      icon: "buildings",
      section: "Navigare",
      action: () => {
        navigate({ to: "/companies" });
        close();
      },
    },
    {
      id: "nav-reports",
      label: "Rapoarte",
      icon: "reports",
      section: "Navigare",
      action: () => {
        navigate({ to: "/reports" });
        close();
      },
    },
    {
      id: "nav-notifications",
      label: "Notificări ANAF",
      icon: "bell",
      section: "Navigare",
      action: () => {
        navigate({ to: "/notifications" });
        close();
      },
    },
    {
      id: "nav-settings",
      label: "Setări",
      icon: "settings",
      section: "Navigare",
      action: () => {
        navigate({ to: "/settings" });
        close();
      },
    },
    // Acțiuni
    {
      id: "act-new-invoice",
      label: "Factură nouă",
      hint: "Ctrl+N",
      icon: "plus",
      section: "Acțiuni",
      action: () => {
        navigate({ to: "/invoices/new" });
        close();
      },
    },
    {
      id: "act-new-contact",
      label: "Contact / furnizor nou",
      icon: "users",
      section: "Acțiuni",
      action: () => {
        navigate({ to: "/contacts" });
        close();
      },
    },
    {
      id: "act-new-company",
      label: "Companie nouă",
      icon: "buildings",
      section: "Acțiuni",
      action: () => {
        navigate({ to: "/companies/new" });
        close();
      },
    },
  ];

  // Add recent invoices as commands
  const recentCmds: Command[] = (recentInvoices?.items ?? []).map((inv) => ({
    id: `inv-${inv.id}`,
    label: `Factură ${inv.fullNumber}`,
    hint: inv.issueDate,
    icon: "invoice",
    section: "Recente",
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
    <div className="palette-scrim" onClick={close}>
      <div
        className="palette"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={handleKeyDown}
      >
        <div className="palette-input">
          <Icon name="search" size={15} style={{ color: "var(--text-muted)" }} />
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setActiveIdx(0);
            }}
            placeholder="Caută comenzi, facturi, contacte…"
            autoComplete="off"
          />
          {query && (
            <button
              type="button"
              style={{
                background: "none",
                border: "none",
                cursor: "pointer",
                color: "var(--text-muted)",
                fontSize: 13,
              }}
              onClick={() => setQuery("")}
            >
              ✕
            </button>
          )}
        </div>
        <div className="palette-list">
          {filtered.length === 0 ? (
            <div
              style={{
                padding: "24px 14px",
                textAlign: "center",
                fontSize: 12,
                color: "var(--text-muted)",
              }}
            >
              Niciun rezultat pentru „{query}"
            </div>
          ) : (
            sections.map((section) => {
              const cmds = filtered.filter((c) => c.section === section);
              return (
                <div key={section}>
                  <div className="palette-section">{section}</div>
                  {cmds.map((cmd) => {
                    const idx = globalIdx++;
                    return (
                      <div
                        key={cmd.id}
                        className={"palette-row" + (idx === activeIdx ? " active" : "")}
                        onMouseEnter={() => setActiveIdx(idx)}
                        onClick={cmd.action}
                      >
                        <span className="ico">
                          <Icon name={cmd.icon} size={14} />
                        </span>
                        <span>{cmd.label}</span>
                        {cmd.hint && <span className="kbd">{cmd.hint}</span>}
                      </div>
                    );
                  })}
                </div>
              );
            })
          )}
        </div>
        <div className="palette-footer">
          <span>↑↓ navigare</span>
          <span>↵ execută</span>
          <span>Esc închide</span>
        </div>
      </div>
    </div>
  );
}
