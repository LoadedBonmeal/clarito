/**
 * Ribbon — toolbar grupat cu icon+label butoane mari uniforme.
 *
 * 5 grupuri: Operațiuni · Sincronizare ANAF · Date · Rapoarte · Instrumente.
 * Reproduce fidel design-ul original (chrome.jsx).
 * Butoanele fără backend v1 sunt marcate disabled (tooltip "În curând").
 */

import { useRef, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";

import { Icon } from "@/components/shared/Icon";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { queryKeys } from "@/lib/queries";

interface RibbonProps {
  onOpenPalette: () => void;
}

export function Ribbon({ onOpenPalette }: RibbonProps) {
  const navigate = useNavigate();
  const ribbonRef = useRef<HTMLDivElement>(null);
  const queryClient = useQueryClient();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [stornoOpen, setStornoOpen] = useState(false);
  const [stornoNumber, setStornoNumber] = useState("");
  const [stornoReason, setStornoReason] = useState("");
  const [stornoLoading, setStornoLoading] = useState(false);
  const [stornoError, setStornoError] = useState("");

  function handleWheel(e: React.WheelEvent<HTMLDivElement>) {
    // On Windows/Linux a plain vertical scroll wheel won't scroll a horizontal
    // overflow container. Translate deltaY → scrollLeft so the ribbon scrolls
    // horizontally with a regular mouse wheel (no Shift required).
    if (ribbonRef.current && Math.abs(e.deltaY) > Math.abs(e.deltaX)) {
      e.preventDefault();
      ribbonRef.current.scrollLeft += e.deltaY;
    }
  }

  return (
    <>
    <div className="ribbon" ref={ribbonRef} onWheel={handleWheel}>
      {/* OPERAȚIUNI */}
      <div className="ribbon-group">
        <div className="ribbon-group-label">Operațiuni</div>
        <div className="ribbon-group-buttons">
          <BtnBig icon="plus"      label="Factură nouă" primary hint="Ctrl+N"       onClick={() => navigate({ to: "/invoices/new" })} />
          <BtnBig icon="invoiceIn" label="Primită nouă"         hint="Ctrl+Shift+N" onClick={() => navigate({ to: "/received" })} />
          <BtnBig icon="storno"    label="Storno"               hint="Ctrl+F9"      onClick={() => { setStornoOpen(true); setStornoNumber(""); setStornoReason(""); setStornoError(""); }} />
          <BtnBig icon="receipt"   label="Chitanță"             disabled />
          <BtnBig icon="bank"      label="Plată"                disabled />
          <BtnBig icon="users"     label="Contact"                                  onClick={() => navigate({ to: "/contacts" })} />
        </div>
      </div>

      {/* SINCRONIZARE ANAF */}
      <div className="ribbon-group">
        <div className="ribbon-group-label">Sincronizare ANAF</div>
        <div className="ribbon-group-buttons">
          <BtnBig icon="cloudUp"  label="Trimite ANAF"    hint="F9"     onClick={() => navigate({ to: "/invoices" })} />
          <BtnBig icon="cloudDn"  label="Descarcă SPV"    hint="Ctrl+D" onClick={() => navigate({ to: "/received" })} />
          <BtnBig icon="refresh"  label="Verifică status" hint="F10"    onClick={() => navigate({ to: "/invoices" })} />
          <BtnBig icon="anaf"     label="Mesaje SPV"                    onClick={() => navigate({ to: "/notifications" })} />
          <BtnBig icon="download" label="Export XML"                    onClick={() => navigate({ to: "/invoices" })} />
          <BtnBig icon="upload"   label="Import XML"                    onClick={() => navigate({ to: "/received" })} />
        </div>
      </div>

      {/* DATE */}
      <div className="ribbon-group">
        <div className="ribbon-group-label">Date</div>
        <div className="ribbon-group-buttons">
          <BtnBig icon="buildings" label="Companii"     onClick={() => navigate({ to: "/companies" })} />
          <BtnBig icon="users"     label="Contacte"     onClick={() => navigate({ to: "/contacts" })} />
          <BtnBig icon="stock"     label="Articole"     disabled />
          <BtnBig icon="database"  label="Plan conturi" disabled />
          <BtnBig icon="tag"       label="Cote TVA"     disabled />
          <BtnBig icon="history"   label="Audit log"    onClick={() => navigate({ to: "/notifications" })} />
        </div>
      </div>

      {/* RAPOARTE & DECLARAȚII */}
      <div className="ribbon-group">
        <div className="ribbon-group-label">Rapoarte & Declarații</div>
        <div className="ribbon-group-buttons">
          <BtnBig icon="reports" label="D300 TVA"          onClick={() => navigate({ to: "/reports" })} />
          <BtnBig icon="reports" label="D394"              onClick={() => navigate({ to: "/reports" })} />
          <BtnBig icon="reports" label="D406 SAF-T"        onClick={() => navigate({ to: "/reports" })} />
          <BtnBig icon="reports" label="Jurnal vânzări"    onClick={() => navigate({ to: "/reports" })} />
          <BtnBig icon="reports" label="Jurnal cumpărări"  onClick={() => navigate({ to: "/reports" })} />
          <BtnBig icon="reports" label="Export contabil"   onClick={() => navigate({ to: "/reports" })} />
        </div>
      </div>

      {/* INSTRUMENTE */}
      <div className="ribbon-group">
        <div className="ribbon-group-label">Instrumente</div>
        <div className="ribbon-group-buttons">
          <BtnBig icon="command"  label="Comenzi"    hint="Ctrl+K" onClick={onOpenPalette} />
          <BtnBig icon="keyboard" label="Scurtături" hint="Ctrl+/" onClick={onOpenPalette} />
          <BtnBig icon="settings" label="Setări"                   onClick={() => navigate({ to: "/settings" })} />
        </div>
      </div>
    </div>

    {/* ── Storno dialog ─────────────────────────────────────────────────── */}
    {stornoOpen && (
      <div
        className="palette-scrim"
        style={{ alignItems: "center", paddingTop: 0 }}
        onClick={() => setStornoOpen(false)}
      >
        <div
          onClick={(e) => e.stopPropagation()}
          style={{
            background: "var(--bg-content)",
            border: "1px solid var(--border)",
            minWidth: 340,
            maxWidth: 440,
            boxShadow: "0 8px 32px rgba(0,0,0,0.18)",
            padding: 20,
          }}
        >
          <div style={{ fontWeight: 700, fontSize: 13, marginBottom: 14, color: "var(--text)" }}>
            Emite factură storno
          </div>
          <div style={{ marginBottom: 10 }}>
            <label style={{ fontSize: 11, color: "var(--text-muted)", display: "block", marginBottom: 4 }}>
              Număr factură originală
            </label>
            <input
              autoFocus
              style={{ width: "100%", boxSizing: "border-box", padding: "5px 8px", fontSize: 12, border: "1px solid var(--border)", background: "var(--bg-input, var(--bg))", color: "var(--text)" }}
              placeholder="ex: FACT1"
              value={stornoNumber}
              onChange={(e) => setStornoNumber(e.target.value)}
            />
          </div>
          <div style={{ marginBottom: 14 }}>
            <label style={{ fontSize: 11, color: "var(--text-muted)", display: "block", marginBottom: 4 }}>
              Motiv stornare
            </label>
            <input
              style={{ width: "100%", boxSizing: "border-box", padding: "5px 8px", fontSize: 12, border: "1px solid var(--border)", background: "var(--bg-input, var(--bg))", color: "var(--text)" }}
              placeholder="ex: Eroare cantitate"
              value={stornoReason}
              onChange={(e) => setStornoReason(e.target.value)}
            />
          </div>
          {stornoError && (
            <div style={{ fontSize: 11, color: "var(--error, #c0392b)", marginBottom: 10 }}>{stornoError}</div>
          )}
          <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
            <button className="btn" onClick={() => setStornoOpen(false)}>Anulează</button>
            <button
              className="btn primary"
              disabled={stornoLoading || !stornoNumber.trim()}
              onClick={async () => {
                if (!activeCompanyId) { setStornoError("Selectați o companie activă."); return; }
                if (!stornoNumber.trim()) { setStornoError("Introduceți numărul facturii."); return; }
                setStornoLoading(true); setStornoError("");
                try {
                  // Find invoice by full_number
                  const result = await api.invoices.list({
                    companyId: activeCompanyId,
                    query: stornoNumber.trim(),
                  });
                  const inv = result.items.find(
                    (i) => i.fullNumber.toLowerCase() === stornoNumber.trim().toLowerCase()
                  );
                  if (!inv) {
                    setStornoError(`Factura "${stornoNumber}" nu a fost găsită.`);
                    return;
                  }
                  await api.invoices.storno(inv.id, stornoReason.trim() || "Stornare");
                  void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.list() });
                  setStornoOpen(false);
                  navigate({ to: "/invoices" });
                } catch (e) {
                  setStornoError((e as { message?: string }).message ?? String(e));
                } finally {
                  setStornoLoading(false);
                }
              }}
            >
              {stornoLoading ? "Se procesează…" : "Emite storno"}
            </button>
          </div>
        </div>
      </div>
    )}
    </>
  );
}

// ─── Building block ────────────────────────────────────────────────────────

interface BtnBigProps {
  icon: string;
  label: string;
  primary?: boolean;
  active?: boolean;
  onClick?: () => void;
  hint?: string;
  disabled?: boolean;
}

function BtnBig({ icon, label, primary, active, onClick, hint, disabled }: BtnBigProps) {
  return (
    <button
      type="button"
      className={
        "ribbon-btn" +
        (primary ? " primary" : "") +
        (active  ? " active"  : "") +
        (disabled ? " disabled" : "")
      }
      onClick={disabled ? undefined : onClick}
      title={disabled ? "În curând" : hint}
      style={disabled ? { opacity: 0.38, cursor: "not-allowed", pointerEvents: "none" } : undefined}
    >
      <span className="ico">
        <Icon name={icon} size={22} />
      </span>
      <span className="lbl">{label}</span>
      {hint && !disabled && (
        <span className="caret">
          <Icon name="caret" size={8} />
        </span>
      )}
    </button>
  );
}
