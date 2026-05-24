/**
 * Ribbon — toolbar grupat cu icon+label butoane mari uniforme.
 *
 * 3 grupuri: Operațiuni · Sincronizare ANAF · Instrumente.
 * Etichetele grupului sunt DEASUPRA butoanelor (nu dedesubt).
 * Lățime fixă fără scroll orizontal: se încadrează de la 1024px.
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
          <BtnBig icon="plus"      label="Factură nouă" primary hint="Ctrl+N" onClick={() => navigate({ to: "/invoices/new" })} />
          <BtnBig icon="invoiceIn" label="Primită nouă" hint="Ctrl+Shift+N"   onClick={() => navigate({ to: "/received" })} />
          <BtnBig icon="users"     label="Contact nou"  onClick={() => navigate({ to: "/contacts" })} />
        </div>
      </div>

      {/* SINCRONIZARE ANAF */}
      <div className="ribbon-group">
        <div className="ribbon-group-label">Sincronizare ANAF</div>
        <div className="ribbon-group-buttons">
          <BtnBig icon="cloudUp" label="Trimite ANAF"    hint="F9"     onClick={() => navigate({ to: "/invoices" })} />
          <BtnBig icon="cloudDn" label="Descarcă SPV"    hint="Ctrl+D" onClick={() => navigate({ to: "/received" })} />
          <BtnBig icon="refresh" label="Verifică status" hint="F10"    onClick={() => navigate({ to: "/invoices" })} />
          <BtnBig icon="anaf"    label="Mesaje SPV"      onClick={() => navigate({ to: "/notifications" })} />
        </div>
      </div>

      {/* INSTRUMENTE */}
      <div className="ribbon-group" style={{ marginLeft: "auto" }}>
        <div className="ribbon-group-label">Instrumente</div>
        <div className="ribbon-group-buttons">
          <BtnBig icon="command"  label="Comenzi" hint="Ctrl+K" onClick={onOpenPalette} />
          <BtnBig icon="settings" label="Setări"  onClick={() => navigate({ to: "/settings" })} />
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
        (active ? " active" : "") +
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
