/**
 * D394View — D394 livrări grupate pe partener + achiziții.
 * Embedded in the Reports page — Claude-Design classes (.scr-card / .scr-table /
 * .chip / .banner / .btn-dark / .pill-btn). ALL wiring preserved: compute query,
 * preflight, DUK block + "Exportă oricum", extract + official ANAF exports,
 * D394SubmissionModal.
 */

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { PreflightPanel } from "@/components/shared/PreflightPanel";
import { D394SubmissionModal } from "@/components/modals/D394SubmissionModal";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON, parseDec } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { queryKeys } from "@/lib/queries";
import type { D394Submission } from "@/types";
import type { PreflightIssue } from "@/lib/tauri";

interface Props {
  dateFrom: string;
  dateTo:   string;
}

// Warn triangle — not in the Ic set, inlined verbatim from the prototype.
const IC_WARN =
  '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>';

export function D394View({ dateFrom, dateTo }: Props) {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [exporting,         setExporting]         = useState(false);
  const [exportingOfficial, setExportingOfficial] = useState(false);
  const [showD394Modal,     setShowD394Modal]     = useState(false);
  const [dukBlock,          setDukBlock]          = useState<PreflightIssue[] | null>(null);
  const [lastSubmission,    setLastSubmission]    = useState<D394Submission | null>(null);

  const periodFrom = dateFrom;
  const periodTo   = dateTo;

  // Fetch active company for pre-filling submission modal.
  const { data: activeCompany } = useQuery({
    queryKey: queryKeys.companies.detail(activeCompanyId ?? ""),
    queryFn: () => api.companies.get(activeCompanyId!),
    enabled: !!activeCompanyId,
  });

  const {
    data:    report,
    isLoading,
    isError,
    error,
    refetch,
  } = useQuery({
    queryKey: ["d394", activeCompanyId ?? "", periodFrom, periodTo],
    queryFn:  () => api.d394.compute(activeCompanyId!, periodFrom, periodTo),
    enabled:  !!activeCompanyId && !!periodFrom && !!periodTo,
    staleTime: 60_000,
  });

  // ── Pre-export validation (preflight) ──────────────────────────────────────
  const { data: preflightIssues = [] } = useQuery({
    queryKey: ["preflight", "d394", activeCompanyId ?? "", periodFrom, periodTo],
    queryFn: () => api.declarations.preflight(activeCompanyId!, "D394", periodFrom, periodTo),
    enabled: !!activeCompanyId && !!periodFrom && !!periodTo,
    staleTime: 30_000,
  });

  const handleExport = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    if (!report || (report.partners.length === 0 && report.purchasePartners.length === 0)) {
      notify.info("Nu există date pentru perioada selectată.");
      return;
    }
    const savePath = await saveDialog({
      title:       "Salvează D394 XML (extract)",
      defaultPath: `d394-${periodFrom}-${periodTo}.xml`,
      filters:     [{ name: "XML", extensions: ["xml"] }],
    });
    if (!savePath) return;
    setExporting(true);
    try {
      const saved = await api.d394.export(activeCompanyId, periodFrom, periodTo, savePath);
      notify.success(`D394 extract salvat: ${saved}`);
      try { await openPath(saved); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut exporta D394."));
    } finally {
      setExporting(false);
    }
  };

  const handleExportOfficial = async (submission: D394Submission, override = false) => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    setLastSubmission(submission);
    const savePath = await saveDialog({
      title:       "Salvează D394 oficial ANAF (XML)",
      defaultPath: `d394-oficial-${periodFrom}-${periodTo}.xml`,
      filters:     [{ name: "XML", extensions: ["xml"] }],
    });
    if (!savePath) return;
    setExportingOfficial(true);
    try {
      const res = await api.d394.exportD394Official(
        activeCompanyId,
        periodFrom,
        periodTo,
        savePath,
        submission,
        override,
      );
      if (!res.written) {
        setDukBlock(res.issues);
        notify.error("DUKIntegrator a găsit erori. Corectați-le sau exportați oricum.");
        return;
      }
      setDukBlock(null);
      notify.success(
        res.dukAvailable
          ? `D394 oficial salvat (DUK: valid): ${res.path}`
          : `D394 oficial salvat: ${res.path} (validare DUK indisponibilă local)`,
      );
      try { await openPath(res.path); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut exporta D394 oficial."));
    } finally {
      setExportingOfficial(false);
    }
  };

  const totalBase         = parseDec(report?.totalBase         ?? "0");
  const totalVat          = parseDec(report?.totalVat          ?? "0");
  const totalPurchaseBase = parseDec(report?.totalPurchaseBase ?? "0");
  const totalPurchaseVat  = parseDec(report?.totalPurchaseVat  ?? "0");

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
      {/* ── Preflight validation panel ────────────────────────────────── */}
      {preflightIssues.length > 0 && <PreflightPanel issues={preflightIssues} />}

      {/* ── DUK block panel ──────────────────────────────────────────── */}
      {dukBlock && (
        <div>
          <PreflightPanel issues={dukBlock} />
          <button
            className="pill-btn"
            style={{ marginTop: 8, color: "var(--red)", borderColor: "rgba(220,38,38,.4)" }}
            onClick={() => lastSubmission && void handleExportOfficial(lastSubmission, true)}
          >
            Exportă oricum (ignoră DUK)
          </button>
        </div>
      )}

      {/* ── Livrări (vânzări) ──────────────────────────────────────────── */}
      <div className="scr-card">
        <div className="scr-toolbar">
          <div className="tt">D394 — Declarație informativă livrări / achiziții pe partener</div>
          <div className="spacer" />
          <button
            className="pill-btn"
            disabled={exporting || !activeCompanyId}
            onClick={() => void handleExport()}
            title="Export extract D394 (document de lucru)"
          >
            <Ic name="dl" />
            {exporting ? "Export…" : "Extract XML"}
          </button>
          <button
            className="btn-dark"
            disabled={exportingOfficial || !activeCompanyId || !activeCompany}
            onClick={() => setShowD394Modal(true)}
            title="Export D394 conform schemei oficiale ANAF v5"
          >
            <Ic name="shield" />
            {exportingOfficial ? "Export…" : "Export oficial ANAF"}
          </button>
        </div>

        {isLoading ? (
          <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>Se încarcă…</div>
        ) : isError ? (
          <div style={{ padding: 16 }}>
            <QueryErrorBanner error={error} label="raportul D394" onRetry={() => void refetch()} />
          </div>
        ) : !report || report.partners.length === 0 ? (
          <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            Nicio livrare validată în perioada selectată.
          </div>
        ) : (
          <>
            <table className="scr-table">
              <thead>
                <tr>
                  <th>CUI partener</th>
                  <th>Denumire</th>
                  <th>Tip</th>
                  <th className="r">Nr. facturi</th>
                  <th className="r">Bază impozabilă</th>
                  <th className="r">TVA</th>
                </tr>
              </thead>
              <tbody>
                {report.partners.map((p, i) => (
                  <tr key={i}>
                    <td className="doc">{p.partnerCui || <span style={{ color: "var(--dim)" }}>—</span>}</td>
                    <td style={{ fontWeight: 500 }}>{p.partnerName}</td>
                    <td><span className="chip sent">{p.vatCategory}</span></td>
                    <td className="r num">{p.invoiceCount}</td>
                    <td className="r num">{fmtRON(p.base)}</td>
                    <td className="r num">{fmtRON(p.vat)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <div className="tot-foot">
              <span>TOTAL: <b className="num">{report.invoiceCount}</b> facturi</span>
              <span>bază <b className="num">{fmtRON(totalBase)}</b></span>
              <span>TVA <b className="num">{fmtRON(totalVat)}</b></span>
            </div>
          </>
        )}
      </div>

      {/* ── Achiziții (received invoices) ──────────────────────────────── */}
      {report && (
        <div className="scr-card">
          <div className="scr-toolbar">
            <div className="tt">D394 — Achiziții per furnizor</div>
          </div>

          {report.purchaseUnparsedCount > 0 && (
            <div style={{ padding: "14px 16px 0" }}>
              <div className="banner warn">
                <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_WARN }} />
                <span>
                  <b>
                    {report.purchaseUnparsedCount}{" "}
                    {report.purchaseUnparsedCount === 1 ? "factură primită nu are" : "facturi primite nu au"}{" "}
                    încă defalcare TVA
                  </b>{" "}
                  — lista furnizorilor este parțială. Folosiți{" "}
                  <b>«Recalculează TVA din XML»</b> în Jurnal cumpărări pentru a completa datele.
                </span>
              </div>
            </div>
          )}

          {report.purchasePartners.length === 0 ? (
            <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
              {report.purchaseInvoiceCount === 0
                ? "Nicio factură primită în perioada selectată."
                : "Nicio factură primită cu defalcare TVA parsată. Folosiți «Recalculează TVA din XML» în Jurnal cumpărări."}
            </div>
          ) : (
            <>
              <table className="scr-table">
                <thead>
                  <tr>
                    <th>CUI furnizor</th>
                    <th>Denumire</th>
                    <th>Tip</th>
                    <th className="r">Nr. facturi</th>
                    <th className="r">Bază impozabilă</th>
                    <th className="r">TVA</th>
                  </tr>
                </thead>
                <tbody>
                  {report.purchasePartners.map((p, i) => (
                    <tr key={i}>
                      <td className="doc">{p.partnerCui || <span style={{ color: "var(--dim)" }}>—</span>}</td>
                      <td style={{ fontWeight: 500 }}>{p.partnerName}</td>
                      <td><span className="chip sent">{p.vatCategory}</span></td>
                      <td className="r num">{p.invoiceCount}</td>
                      <td className="r num">{fmtRON(p.base)}</td>
                      <td className="r num">{fmtRON(p.vat)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <div className="tot-foot">
                <span>
                  TOTAL ACHIZIȚII (parsate):{" "}
                  <b className="num">{report.purchasePartners.reduce((s, p) => s + p.invoiceCount, 0)}</b> facturi
                </span>
                <span>bază <b className="num">{fmtRON(totalPurchaseBase)}</b></span>
                <span>TVA <b className="num">{fmtRON(totalPurchaseVat)}</b></span>
              </div>
            </>
          )}
        </div>
      )}

      {/* D394 Submission Modal (export oficial) */}
      {activeCompany && (
        <D394SubmissionModal
          open={showD394Modal}
          onOpenChange={setShowD394Modal}
          company={activeCompany}
          onSubmit={(sub) => void handleExportOfficial(sub)}
        />
      )}
    </div>
  );
}
