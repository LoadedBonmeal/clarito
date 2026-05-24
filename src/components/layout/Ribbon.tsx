/**
 * Ribbon — toolbar grupat cu icon+label butoane mari uniforme.
 *
 * Toate butoanele sunt BtnBig (68×uniform px, icon 22px, label 2 linii clamp).
 * ribbon-stack / ribbon-btn-small au fost eliminate — layout uniform garantat.
 */

import { useNavigate, useLocation } from "@tanstack/react-router";

import { Icon } from "@/components/shared/Icon";

interface RibbonProps {
  onOpenPalette: () => void;
}

export function Ribbon({ onOpenPalette }: RibbonProps) {
  const navigate = useNavigate();
  const location = useLocation();

  return (
    <div className="ribbon">
      {/* OPERAȚIUNI */}
      <div className="ribbon-group">
        <div className="ribbon-group-buttons">
          <BtnBig icon="plus"      label="Factură nouă"   primary hint="Ctrl+N" onClick={() => navigate({ to: "/invoices/new" })} />
          <BtnBig icon="invoiceIn" label="Primită nouă"   hint="Ctrl+Shift+N"   onClick={() => navigate({ to: "/received" })} />
          <BtnBig icon="storno"    label="Storno"         hint="Ctrl+F9" />
          <BtnBig icon="receipt"   label="Chitanță"       disabled />
          <BtnBig icon="bank"      label="Plată"          disabled />
          <BtnBig icon="users"     label="Contact nou"    onClick={() => navigate({ to: "/contacts" })} />
        </div>
        <div className="ribbon-group-label">Operațiuni</div>
      </div>

      {/* SINCRONIZARE */}
      <div className="ribbon-group">
        <div className="ribbon-group-buttons">
          <BtnBig icon="cloudUp"  label="Trimite ANAF"    hint="F9"      onClick={() => navigate({ to: "/invoices" })} />
          <BtnBig icon="cloudDn"  label="Descarcă SPV"    hint="Ctrl+D"  onClick={() => navigate({ to: "/received" })} />
          <BtnBig icon="refresh"  label="Verifică status" hint="F10"     onClick={() => navigate({ to: "/invoices" })} />
          <BtnBig icon="anaf"     label="Mesaje SPV"                     onClick={() => navigate({ to: "/notifications" })} />
          <BtnBig icon="download" label="Export XML"      disabled />
          <BtnBig icon="upload"   label="Import XML"      disabled />
        </div>
        <div className="ribbon-group-label">Sincronizare ANAF</div>
      </div>

      {/* DATE */}
      <div className="ribbon-group">
        <div className="ribbon-group-buttons">
          <BtnBig
            icon="buildings" label="Companii"
            active={location.pathname.startsWith("/companies")}
            onClick={() => navigate({ to: "/companies" })}
          />
          <BtnBig icon="users"    label="Contacte"      onClick={() => navigate({ to: "/contacts" })} />
          <BtnBig icon="stock"    label="Articole"      disabled />
          <BtnBig icon="database" label="Plan conturi"  disabled />
          <BtnBig icon="tag"      label="Cote TVA"      disabled />
          <BtnBig icon="history"  label="Audit log"     disabled />
        </div>
        <div className="ribbon-group-label">Date</div>
      </div>

      {/* RAPOARTE */}
      <div className="ribbon-group">
        <div className="ribbon-group-buttons">
          <BtnBig icon="reports" label="D300 TVA"         disabled />
          <BtnBig icon="reports" label="D394"             disabled />
          <BtnBig icon="reports" label="D406 SAF-T"       disabled />
          <BtnBig icon="reports" label="Jurn. vânzări"    disabled />
          <BtnBig icon="reports" label="Jurn. cumpărări"  disabled />
          <BtnBig icon="reports" label="Balanță"          disabled />
        </div>
        <div className="ribbon-group-label">Rapoarte &amp; Declarații</div>
      </div>

      {/* INSTRUMENTE */}
      <div className="ribbon-group" style={{ flex: 1 }}>
        <div className="ribbon-group-buttons">
          <BtnBig icon="command"  label="Comenzi"    hint="Ctrl+K" onClick={onOpenPalette} />
          <BtnBig icon="keyboard" label="Scurtături" hint="Ctrl+/" disabled />
          <BtnBig icon="settings" label="Setări"     onClick={() => navigate({ to: "/settings" })} />
        </div>
        <div className="ribbon-group-label">Instrumente</div>
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
  /** Buton neimplementat — afișat estompat, fără click */
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
