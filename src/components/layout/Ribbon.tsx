/**
 * Ribbon — toolbar grupat cu icon+label butoane mari și stive de butoane mici.
 *
 * Portat din Claude Design (chrome.jsx). Folosește clasele:
 * .ribbon, .ribbon-group, .ribbon-btn (.primary, .active), .ribbon-btn-small,
 * .ribbon-stack, .ribbon-group-label.
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
          <BtnBig
            icon="plus"
            label="Factură nouă"
            primary
            hint="Ctrl+N"
            onClick={() => navigate({ to: "/invoices/new" })}
          />
          <BtnBig icon="invoiceIn" label="Primită nouă" hint="Ctrl+Shift+N" onClick={() => navigate({ to: "/received" })} />
          <BtnBig icon="storno" label="Storno" hint="Ctrl+F9" />
          <div className="ribbon-stack">
            <BtnSmall icon="receipt" label="Chitanță" />
            <BtnSmall icon="bank" label="Plată" />
            <BtnSmall icon="users" label="Contact" onClick={() => navigate({ to: "/contacts" })} />
          </div>
        </div>
        <div className="ribbon-group-label">Operațiuni</div>
      </div>

      {/* SINCRONIZARE */}
      <div className="ribbon-group">
        <div className="ribbon-group-buttons">
          <BtnBig icon="cloudUp" label="Trimite ANAF" hint="F9" onClick={() => navigate({ to: "/invoices" })} />
          <BtnBig icon="cloudDn" label="Descarcă SPV" hint="Ctrl+D" onClick={() => navigate({ to: "/received" })} />
          <BtnBig icon="refresh" label="Verifică status" hint="F10" onClick={() => navigate({ to: "/invoices" })} />
          <div className="ribbon-stack">
            <BtnSmall icon="anaf" label="Mesaje SPV" onClick={() => navigate({ to: "/received" })} />
            <BtnSmall icon="download" label="Export XML" />
            <BtnSmall icon="upload" label="Import XML" />
          </div>
        </div>
        <div className="ribbon-group-label">Sincronizare ANAF</div>
      </div>

      {/* DATE */}
      <div className="ribbon-group">
        <div className="ribbon-group-buttons">
          <BtnBig
            icon="buildings"
            label="Companii"
            active={location.pathname.startsWith("/companies")}
            onClick={() => navigate({ to: "/companies" })}
          />
          <BtnBig
            icon="users"
            label="Contacte"
            onClick={() => navigate({ to: "/contacts" })}
          />
          <BtnBig icon="stock" label="Articole" />
          <div className="ribbon-stack">
            <BtnSmall icon="database" label="Plan conturi" />
            <BtnSmall icon="tag" label="Cote TVA" />
            <BtnSmall icon="history" label="Audit log" />
          </div>
        </div>
        <div className="ribbon-group-label">Date</div>
      </div>

      {/* RAPOARTE */}
      <div className="ribbon-group">
        <div className="ribbon-group-buttons">
          <BtnBig icon="reports" label="D300 TVA" />
          <BtnBig icon="reports" label="D394" />
          <BtnBig icon="reports" label="D406 SAF-T" />
          <div className="ribbon-stack">
            <BtnSmall icon="reports" label="Jurnal vânzări" />
            <BtnSmall icon="reports" label="Jurnal cumpărări" />
            <BtnSmall icon="reports" label="Balanță" />
          </div>
        </div>
        <div className="ribbon-group-label">Rapoarte &amp; Declarații</div>
      </div>

      {/* INSTRUMENTE */}
      <div className="ribbon-group" style={{ flex: 1 }}>
        <div className="ribbon-group-buttons">
          <BtnBig
            icon="command"
            label="Comenzi"
            hint="Ctrl+K"
            onClick={onOpenPalette}
          />
          <BtnBig icon="keyboard" label="Scurtături" hint="Ctrl+/" />
          <BtnBig
            icon="settings"
            label="Setări"
            onClick={() => navigate({ to: "/settings" })}
          />
        </div>
        <div className="ribbon-group-label">Instrumente</div>
      </div>
    </div>
  );
}

// ─── Building blocks ──────────────────────────────────────────────────────

interface BtnBigProps {
  icon: string;
  label: string;
  primary?: boolean;
  active?: boolean;
  onClick?: () => void;
  hint?: string;
}

function BtnBig({ icon, label, primary, active, onClick, hint }: BtnBigProps) {
  return (
    <button
      type="button"
      className={
        "ribbon-btn" +
        (primary ? " primary" : "") +
        (active ? " active" : "")
      }
      onClick={onClick}
      title={hint}
    >
      <span className="ico">
        <Icon name={icon} size={22} />
      </span>
      <span className="lbl">{label}</span>
      {hint && (
        <span className="caret">
          <Icon name="caret" size={8} />
        </span>
      )}
    </button>
  );
}

function BtnSmall({
  icon,
  label,
  onClick,
}: {
  icon: string;
  label: string;
  onClick?: () => void;
}) {
  return (
    <button type="button" className="ribbon-btn-small" onClick={onClick}>
      <span className="ico">
        <Icon name={icon} size={14} />
      </span>
      <span>{label}</span>
    </button>
  );
}
