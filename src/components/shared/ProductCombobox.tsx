import { useEffect, useId, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Icon } from "@/components/shared/Icon";
import { api } from "@/lib/tauri";
import { fmtRON } from "@/lib/utils";
import type { Product } from "@/types";

interface ProductComboboxProps {
  companyId: string;
  onSelect: (product: Product) => void;
  placeholder?: string;
  disabled?: boolean;
}

/**
 * R15 Wave 1 — Searchable product picker backed by `search_products`.
 *
 * Debounces input (250 ms) and fires when at least 2 characters are typed.
 * Supports keyboard navigation (ArrowUp/Down/Enter/Escape).
 * On select, calls `onSelect` with the chosen Product so the caller can
 * fill an invoice line. The input resets after selection (stateless — caller
 * controls its own line state).
 * Mirrors ContactCombobox.tsx conventions exactly.
 */
export function ProductCombobox({
  companyId,
  onSelect,
  placeholder = "Caută articol (denumire sau cod)…",
  disabled,
}: ProductComboboxProps) {
  const [query, setQuery] = useState("");
  const [debouncedQuery, setDebouncedQuery] = useState("");
  const [open, setOpen] = useState(false);
  const [highlight, setHighlight] = useState(0);
  const containerRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const listboxId = useId();

  // Debounce input → debouncedQuery (250 ms)
  useEffect(() => {
    const t = setTimeout(() => setDebouncedQuery(query.trim()), 250);
    return () => clearTimeout(t);
  }, [query]);

  // Search enabled only when typing 2+ chars, dropdown open, and companyId known
  const { data: results = [], isFetching } = useQuery({
    queryKey: ["products", "search", companyId, debouncedQuery],
    queryFn: () => api.products.search(companyId, debouncedQuery),
    enabled: open && debouncedQuery.length >= 2 && !!companyId,
    staleTime: 30_000,
  });

  // Close when clicking outside
  useEffect(() => {
    const onDocClick = (e: MouseEvent) => {
      if (
        containerRef.current &&
        !containerRef.current.contains(e.target as Node)
      ) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onDocClick);
    return () => document.removeEventListener("mousedown", onDocClick);
  }, []);

  // Reset highlight when result set changes
  useEffect(() => {
    setHighlight(0);
  }, [results.length]);

  const handleSelect = (p: Product) => {
    onSelect(p);
    setQuery("");
    setDebouncedQuery("");
    setOpen(false);
    inputRef.current?.blur();
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

  return (
    <div
      ref={containerRef}
      style={{ position: "relative", display: "inline-block", width: "100%" }}
    >
      <input
        ref={inputRef}
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
          style={{
            position: "absolute",
            top: "calc(100% + 4px)",
            left: 0,
            right: 0,
            zIndex: 50,
            background: "var(--input-bg, white)",
            border: "1px solid var(--input-border)",
            borderRadius: 5,
            boxShadow: "0 8px 24px rgba(0,0,0,0.10)",
            maxHeight: 260,
            overflowY: "auto",
          }}
        >
          {debouncedQuery.length < 2 ? (
            <div
              style={{ padding: "10px 12px", fontSize: 12, color: "var(--text-muted)" }}
            >
              Tastați cel puțin 2 caractere…
            </div>
          ) : isFetching ? (
            <div
              style={{ padding: "10px 12px", fontSize: 12, color: "var(--text-muted)" }}
            >
              Se caută…
            </div>
          ) : results.length === 0 ? (
            <div
              style={{ padding: "10px 12px", fontSize: 12, color: "var(--text-muted)" }}
            >
              Niciun articol găsit pentru „{debouncedQuery}".
            </div>
          ) : (
            results.map((p, idx) => {
              const active = idx === highlight;
              return (
                <button
                  key={p.id}
                  type="button"
                  role="option"
                  aria-selected={active}
                  onMouseDown={(e) => {
                    // Prevent input blur before click fires
                    e.preventDefault();
                  }}
                  onClick={() => handleSelect(p)}
                  onMouseEnter={() => setHighlight(idx)}
                  style={{
                    display: "block",
                    width: "100%",
                    textAlign: "left",
                    padding: "8px 12px",
                    border: "none",
                    borderBottom: "1px solid var(--border, transparent)",
                    background: active ? "var(--accent-soft, #F0F1F4)" : "transparent",
                    cursor: "pointer",
                    color: "var(--text)",
                    font: "inherit",
                  }}
                >
                  <div
                    style={{
                      display: "flex",
                      justifyContent: "space-between",
                      alignItems: "baseline",
                      gap: 8,
                    }}
                  >
                    <span style={{ fontSize: 13, fontWeight: 500 }}>{p.name}</span>
                    <span
                      className="tnum"
                      style={{ fontSize: 12, color: "var(--text-muted)", flexShrink: 0 }}
                    >
                      {fmtRON(p.unitPrice)} RON
                    </span>
                  </div>
                  <div
                    className="mono"
                    style={{ fontSize: 11, color: "var(--text-muted)" }}
                  >
                    {p.code ? `${p.code} · ` : ""}
                    {p.unit} · TVA {p.vatRate}%
                    {p.stockQty != null ? (
                      <span> · stoc {p.stockQty}</span>
                    ) : null}
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

/** Small inline trigger button — placed in a line row to open ProductCombobox. */
export function ProductPickerButton({
  companyId,
  onSelect,
  disabled,
}: {
  companyId: string;
  onSelect: (p: Product) => void;
  disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);

  if (!open) {
    return (
      <button
        type="button"
        className="btn-icon"
        title="Alege din catalog"
        disabled={disabled}
        onClick={() => setOpen(true)}
        style={{ fontSize: 10, padding: "1px 4px" }}
      >
        <Icon name="stock" size={11} />
      </button>
    );
  }

  return (
    <div style={{ position: "relative", minWidth: 220 }}>
      <ProductCombobox
        companyId={companyId}
        onSelect={(p) => {
          onSelect(p);
          setOpen(false);
        }}
        placeholder="Caută articol…"
        disabled={disabled}
      />
      <button
        type="button"
        className="btn-icon"
        title="Închide"
        onClick={() => setOpen(false)}
        style={{
          position: "absolute",
          right: 2,
          top: "50%",
          transform: "translateY(-50%)",
        }}
      >
        <Icon name="x" size={10} />
      </button>
    </div>
  );
}
