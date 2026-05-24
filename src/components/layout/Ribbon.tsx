/**
 * Ribbon — toolbar grupat cu icon+label butoane mari uniforme.
 *
 * 5 grupuri: Operațiuni · Sincronizare ANAF · Date · Rapoarte · Instrumente.
 * Reproduce fidel design-ul original (chrome.jsx).
 * Butoanele fără backend v1 sunt marcate disabled (tooltip "În curând").
 */

import { useNavigate } from "@tanstack/react-router";

import { Icon } from "@/components/shared/Icon";

interface RibbonProps {
  onOpenPalette: () => void;
}

export function Ribbon({ onOpenPalette }: RibbonProps) {
  const navigate = useNavigate();

  return (
    <div className="ribbon">
      {/* OPERAȚIUNI */}
      <div className="ribbon-group">
        <div className="ribbon-group-label">Operațiuni</div>
        <div className="ribbon-group-buttons">
          <BtnBig icon="plus"      label="Factură nouă" primary hint="Ctrl+N"       onClick={() => navigate({ to: "/invoices/new" })} />
          <BtnBig icon="invoiceIn" label="Primită nouă"         hint="Ctrl+Shift+N" onClick={() => navigate({ to: "/received" })} />
          <BtnBig icon="storno"    label="Storno"               hint="Ctrl+F9"      onClick={() => navigate({ to: "/invoices" })} />
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
          <BtnBig icon="reports" label="Balanță"           onClick={() => navigate({ to: "/reports" })} />
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
