/**
 * Ribbon — toolbar grupat cu icon+label butoane mari uniforme.
 *
 * 5 grupuri: Operațiuni · Sincronizare ANAF · Date · Rapoarte · Instrumente.
 * Reproduce fidel design-ul original (chrome.jsx).
 * Butoanele fără backend v1 sunt marcate disabled (tooltip "În curând").
 */

import { useRef, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { Icon } from "@/components/shared/Icon";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { queryKeys } from "@/lib/queries";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { fmtShortcut } from "@/lib/platform";

interface RibbonProps {
  onOpenPalette: () => void;
  onOpenShortcuts: () => void;
}

export function Ribbon({ onOpenPalette, onOpenShortcuts }: RibbonProps) {
  const navigate = useNavigate();
  const ribbonRef = useRef<HTMLDivElement>(null);
  const queryClient = useQueryClient();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const selectedInvoiceId = useAppStore((s) => s.selectedInvoiceId);

  // Read ANAF test mode from persistent settings (same key as Settings.tsx / backend)
  const { data: testModeSetting } = useQuery({
    queryKey: queryKeys.anaf.testMode,
    queryFn: () => api.settings.get("use_anaf_test_env"),
  });
  const anafTestMode = testModeSetting === "1";

  const [stornoOpen, setStornoOpen] = useState(false);
  const [stornoNumber, setStornoNumber] = useState("");
  const [stornoReason, setStornoReason] = useState("");
  const [stornoLoading, setStornoLoading] = useState(false);
  const [stornoError, setStornoError] = useState("");

  const handleDownloadPdf = async () => {
    if (!selectedInvoiceId) { notify.warn("Selectați o factură din listă."); return; }
    try {
      const pdfPath = await api.ubl.generatePdf(selectedInvoiceId);
      const { openPath } = await import("@tauri-apps/plugin-opener");
      await openPath(pdfPath);
      notify.success("PDF deschis");
    } catch (e) { notify.error(formatError(e, 'Nu s-a putut genera PDF-ul.')); }
  };

  const handleExportXml = async () => {
    if (!selectedInvoiceId) { notify.warn("Selectați o factură din listă."); return; }
    try {
      const xml = await api.ubl.generateXml(selectedInvoiceId);
      const { save } = await import("@tauri-apps/plugin-dialog");
      const { writeTextFile } = await import("@tauri-apps/plugin-fs");
      const path = await save({ defaultPath: "factura.xml", filters: [{ name: "XML", extensions: ["xml"] }] });
      if (path) { await writeTextFile(path, xml); notify.success(`XML salvat: ${path}`); }
    } catch (e) { notify.error(formatError(e, 'Nu s-a putut exporta XML-ul.')); }
  };

  const handleImportXml = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const filePath = await open({ filters: [{ name: "XML e-Factura", extensions: ["xml"] }] });
      if (!filePath || typeof filePath !== "string") return;
      const result = await api.importData.invoiceXmlFromFile(filePath, activeCompanyId);
      if (result.imported > 0) {
        notify.success(`Factură importată: ${result.invoiceNumber} — ${result.supplierName}`);
        void queryClient.invalidateQueries({ queryKey: queryKeys.received.all });
      } else {
        notify.error(`Import eșuat: ${result.errors.join("; ")}`);
      }
    } catch (e) { notify.error(formatError(e, 'Nu s-a putut importa fișierul XML.')); }
  };

  const handleSubmitAnaf = async () => {
    if (!selectedInvoiceId || !activeCompanyId) { notify.warn("Selectați o factură și o companie activă."); return; }
    const testMode = anafTestMode;
    try {
      await notify.promise(
        api.anaf.submitInvoice(activeCompanyId, selectedInvoiceId, testMode),
        { loading: "Se trimite la ANAF…", success: "Trimis cu succes", error: "Eroare la trimitere" }
      );
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
    } catch (_) {}
  };

  const handleSyncSpv = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    try {
      const newCount = await api.anaf.syncSpv(activeCompanyId, anafTestMode);
      void queryClient.invalidateQueries({ queryKey: queryKeys.received.all });
      void queryClient.invalidateQueries({ queryKey: queryKeys.notifications.all });
      if (newCount > 0) {
        notify.success(`${newCount} mesaje SPV noi descărcate`);
      } else {
        notify.info("Nicio factură nouă în SPV");
      }
      navigate({ to: "/received" });
    } catch (e) {
      notify.error(formatError(e, 'Sincronizarea SPV a eșuat.'));
    }
  };

  const handleCheckStatus = async () => {
    if (!selectedInvoiceId || !activeCompanyId) { notify.warn("Selectați o factură și o companie activă."); return; }
    const testMode = anafTestMode;
    try {
      const status = await api.anaf.checkStatus(activeCompanyId, selectedInvoiceId, testMode);
      notify.info(`Status ANAF: ${status}`);
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
    } catch (e) { notify.error(formatError(e, 'Nu s-a putut verifica statusul la ANAF.')); }
  };

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
          <BtnBig icon="plus"      label="Factură nouă" primary hint={fmtShortcut("Ctrl+N")}       onClick={() => navigate({ to: "/invoices/new" })} />
          <BtnBig icon="invoiceIn" label="Primită nouă"         hint={fmtShortcut("Ctrl+Shift+N")} onClick={() => navigate({ to: "/received" })} />
          <BtnBig icon="storno"    label="Storno"               hint={fmtShortcut("Ctrl+F9")}      onClick={() => { setStornoOpen(true); setStornoNumber(""); setStornoReason(""); setStornoError(""); }} />
          <BtnBig icon="receipt"   label="Chitanță"             disabled />
          <BtnBig icon="bank"      label="Plată"                disabled />
          <BtnBig icon="users"     label="Contact"                                  onClick={() => navigate({ to: "/contacts" })} />
        </div>
      </div>

      {/* SINCRONIZARE ANAF */}
      <div className="ribbon-group">
        <div className="ribbon-group-label">Sincronizare ANAF</div>
        <div className="ribbon-group-buttons">
          <BtnBig icon="cloudUp"  label="Trimite ANAF"    hint="F9"                         onClick={handleSubmitAnaf}   title={selectedInvoiceId ? "F9" : "Selectați o factură"} />
          <BtnBig icon="cloudDn"  label="Descarcă SPV"    hint={fmtShortcut("Ctrl+D")}  onClick={handleSyncSpv} />
          <BtnBig icon="refresh"  label="Verifică status" hint="F10"    onClick={handleCheckStatus}  title={selectedInvoiceId ? "F10" : "Selectați o factură"} />
          <BtnBig icon="anaf"     label="Mesaje SPV"                    onClick={() => navigate({ to: "/notifications" })} />
          <BtnBig icon="download" label="Export XML"                    onClick={handleExportXml}    title={selectedInvoiceId ? undefined : "Selectați o factură"} />
          <BtnBig icon="file"     label="Descarcă PDF"                  onClick={handleDownloadPdf}  title={selectedInvoiceId ? undefined : "Selectați o factură"} />
          <BtnBig icon="upload"   label="Import XML"                    onClick={handleImportXml} />
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
          <BtnBig icon="reports" label="D300 TVA"          onClick={() => navigate({ to: "/declarations" })} />
          <BtnBig icon="reports" label="D394"              onClick={() => navigate({ to: "/reports", search: { view: "d394" } })} />
          <BtnBig icon="reports" label="D406 SAF-T"        onClick={() => navigate({ to: "/reports", search: { view: "saft" } })} />
          <BtnBig icon="reports" label="Jurnal vânzări"    onClick={() => navigate({ to: "/reports", search: { view: "sales-journal" } })} />
          <BtnBig icon="reports" label="Jurnal cumpărări"  onClick={() => navigate({ to: "/reports", search: { view: "purchase-journal" } })} />
          <BtnBig icon="reports" label="Export contabil"   onClick={() => navigate({ to: "/reports", search: { view: "accounting-export" } })} />
        </div>
      </div>

      {/* INSTRUMENTE */}
      <div className="ribbon-group">
        <div className="ribbon-group-label">Instrumente</div>
        <div className="ribbon-group-buttons">
          <BtnBig icon="command"  label="Comenzi"    hint={fmtShortcut("Ctrl+K")} onClick={onOpenPalette} />
          <BtnBig icon="keyboard" label="Scurtături" hint={fmtShortcut("Ctrl+/")} onClick={onOpenShortcuts} />
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
                  // R14 Wave A: pass activeCompanyId for ownership verification.
                  await api.invoices.storno(inv.id, activeCompanyId, stornoReason.trim() || "Stornare");
                  // TS-12: invalidate the full invoices namespace (including filtered list queries)
                  void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
                  setStornoOpen(false);
                  navigate({ to: "/invoices" });
                } catch (e) {
                  setStornoError(formatError(e, 'Stornarea a eșuat.'));
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
  title?: string;
}

function BtnBig({ icon, label, primary, active, onClick, hint, disabled, title }: BtnBigProps) {
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
      title={disabled ? "În curând" : (title ?? hint)}
      aria-label={label}
      aria-disabled={disabled}
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
