import { useEffect, useId, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Ic } from "@/components/shared/Ic";
import { api } from "@/lib/tauri";
import type { Contact, ContactType } from "@/types";

interface ContactComboboxProps {
  value: Contact | null;
  onChange: (contact: Contact | null) => void;
  companyId: string;
  placeholder?: string;
  disabled?: boolean;
  /** Optional client-side filter applied on top of search results (e.g. only CUSTOMER/BOTH). */
  filterType?: ContactType[];
  /** Width of the input / selected pill, mirroring the previous <select> sizing. */
  width?: number | string;
  /** Forwarded to the underlying <input> so an outer <label htmlFor=…> can target it (A11Y-06). */
  inputId?: string;
}

/**
 * MISS-05 — Typeahead contact picker backed by `search_contacts`.
 * Re-skinned to the Claude-Design vocabulary (.input / .pop / .pop-item / .mini-btn).
 *
 * Debounces input (250ms) and only fires the search when at least 2 characters
 * are typed. Supports keyboard navigation (ArrowUp/Down/Enter/Escape) and
 * collapses to a compact "selected" pill once a contact is chosen, with a
 * clear button to reset.
 */
export function ContactCombobox({
  value,
  onChange,
  companyId,
  placeholder = "Caută client (nume sau CUI)…",
  disabled,
  filterType,
  width = 320,
  inputId,
}: ContactComboboxProps) {
  const [query, setQuery] = useState("");
  const [debouncedQuery, setDebouncedQuery] = useState("");
  const [open, setOpen] = useState(false);
  const [highlight, setHighlight] = useState(0);
  const containerRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const listboxId = useId();

  // Debounce input → debouncedQuery (250ms)
  useEffect(() => {
    const t = setTimeout(() => setDebouncedQuery(query.trim()), 250);
    return () => clearTimeout(t);
  }, [query]);

  // Search query enabled only when typing 2+ chars and dropdown is open
  const { data: rawResults = [], isFetching } = useQuery({
    queryKey: ["contacts", "search", companyId, debouncedQuery],
    queryFn: () => api.contacts.search(debouncedQuery, companyId),
    enabled: open && debouncedQuery.length >= 2 && !!companyId,
    staleTime: 30_000,
  });

  const results = filterType
    ? rawResults.filter((c) => filterType.includes(c.contactType))
    : rawResults;

  // Close when clicking outside
  useEffect(() => {
    const onDocClick = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onDocClick);
    return () => document.removeEventListener("mousedown", onDocClick);
  }, []);

  // Reset highlight whenever the result set changes
  useEffect(() => {
    setHighlight(0);
  }, [results.length]);

  const handleSelect = (c: Contact) => {
    onChange(c);
    setQuery("");
    setOpen(false);
    inputRef.current?.blur();
  };

  const handleClear = () => {
    onChange(null);
    setQuery("");
    setDebouncedQuery("");
    // Defer focus until the input is re-rendered
    requestAnimationFrame(() => inputRef.current?.focus());
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (!open) {
      if (e.key === "ArrowDown" || e.key === "Enter") {
        e.preventDefault();
        setOpen(true);
      }
      return;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setHighlight((h) => Math.min(h + 1, Math.max(results.length - 1, 0)));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setHighlight((h) => Math.max(h - 1, 0));
    } else if (e.key === "Enter") {
      if (results[highlight]) {
        e.preventDefault();
        handleSelect(results[highlight]);
      }
    } else if (e.key === "Escape") {
      e.preventDefault();
      setOpen(false);
    }
  };

  const widthStyle = typeof width === "number" ? `${width}px` : width;

  // Selected state — render a compact pill with a clear button
  if (value) {
    return (
      <div
        ref={containerRef}
        style={{
          position: "relative",
          display: "inline-flex",
          alignItems: "center",
          gap: 8,
          width: widthStyle,
          minHeight: 36,
          padding: "4px 6px 4px 11px",
          border: "1px solid var(--line)",
          background: "var(--bg)",
          borderRadius: 8,
        }}
      >
        <div style={{ flex: 1, minWidth: 0, lineHeight: 1.25 }}>
          <div
            style={{
              fontSize: 13,
              fontWeight: 500,
              color: "var(--text)",
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {value.legalName}
          </div>
          <div className="doc num" style={{ fontSize: 11 }}>
            {value.cui ?? "—"}
            {value.country && value.country !== "RO" ? ` · ${value.country}` : ""}
          </div>
        </div>
        {!disabled && (
          <button
            type="button"
            onClick={handleClear}
            className="mini-btn"
            aria-label="Șterge selecția"
            title="Schimbă cumpărătorul"
          >
            <Ic name="xMark" />
          </button>
        )}
      </div>
    );
  }

  // Empty state — input + dropdown (.pop.show panel)
  return (
    <div
      ref={containerRef}
      style={{ position: "relative", display: "inline-block", width: widthStyle }}
    >
      <input
        ref={inputRef}
        id={inputId}
        className="input"
        type="text"
        value={query}
        onChange={(e) => {
          setQuery(e.target.value);
          setOpen(true);
        }}
        onFocus={() => setOpen(true)}
        onKeyDown={onKeyDown}
        placeholder={placeholder}
        disabled={disabled}
        autoComplete="off"
        aria-autocomplete="list"
        aria-expanded={open}
        aria-controls={listboxId}
        role="combobox"
        style={{ width: "100%" }}
      />
      {open && !disabled && (
        <div
          id={listboxId}
          role="listbox"
          className="pop show"
          style={{
            top: "calc(100% + 4px)",
            left: 0,
            right: 0,
            maxHeight: 260,
            overflowY: "auto",
          }}
        >
          {debouncedQuery.length < 2 ? (
            <div className="muted" style={{ padding: "9px 10px", fontSize: 12 }}>
              Tastați cel puțin 2 caractere…
            </div>
          ) : isFetching ? (
            <div className="muted" style={{ padding: "9px 10px", fontSize: 12 }}>
              Se caută…
            </div>
          ) : results.length === 0 ? (
            <div className="muted" style={{ padding: "9px 10px", fontSize: 12 }}>
              Niciun rezultat pentru „{debouncedQuery}".
            </div>
          ) : (
            results.map((c, idx) => {
              const active = idx === highlight;
              return (
                <button
                  key={c.id}
                  type="button"
                  role="option"
                  aria-selected={active}
                  className="pop-item"
                  onMouseDown={(e) => {
                    e.preventDefault();
                  }}
                  onClick={() => handleSelect(c)}
                  onMouseEnter={() => setHighlight(idx)}
                  style={{
                    display: "block",
                    width: "100%",
                    height: "auto",
                    textAlign: "left",
                    padding: "7px 10px",
                    border: 0,
                    whiteSpace: "normal",
                    background: active ? "var(--fill)" : "transparent",
                    fontFamily: "inherit",
                  }}
                >
                  <div style={{ fontSize: 13, fontWeight: 500, color: "var(--text)" }}>
                    {c.legalName}
                  </div>
                  <div className="doc num" style={{ fontSize: 11 }}>
                    {c.cui ?? "—"}
                    {c.country && c.country !== "RO" ? ` · ${c.country}` : ""}
                  </div>
                </button>
              );
            })
          )}
        </div>
      )}
    </div>
  );
}
