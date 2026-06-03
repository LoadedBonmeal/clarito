/**
 * D394View — D394 livrări grupate pe partener + achiziții.
 * Wave 5 — rf look: SectionCard + rf-tbl + Banner
 */

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { SectionCard, Btn, Banner, Badge } from "@/components/rf";
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

interface Props {
  dateFrom: string;
  dateTo:   string;
}

export function D394View({ dateFrom, dateTo }: Props) {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [exporting,         setExporting]         = useState(false);
  const [exportingOfficial, setExportingOfficial] = useState(false);
  const [showD394Modal,     setShowD394Modal]     = useState(false);

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

  const handleExportOfficial = async (submission: D394Submission) => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    const savePath = await saveDialog({
      title:       "Salvează D394 oficial ANAF (XML)",
      defaultPath: `d394-oficial-${periodFrom}-${periodTo}.xml`,
      filters:     [{ name: "XML", extensions: ["xml"] }],
    });
    if (!savePath) return;
    setExportingOfficial(true);
    try {
      const saved = await api.d394.exportD394Official(
        activeCompanyId,
        periodFrom,
        periodTo,
        savePath,
        submission,
      );
      notify.success(`D394 oficial salvat: ${saved}`);
      try { await openPath(saved); } catch { /* reveal best-effort */ }
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
    <div className="rf-col">
      {/* ── Preflight validation panel ────────────────────────────────── */}
      <PreflightPanel issues={preflightIssues} />

      {/* ── Livrări (vânzări) ──────────────────────────────────────────── */}
      <SectionCard
        icon="declaration"
        title="D394 — Declarație informativă livrări / achiziții pe partener"
        actions={
          <div style={{ display: "flex", gap: 8 }}>
            <Btn
              variant="secondary"
              size="sm"
              icon="xml"
              disabled={exporting || !activeCompanyId}
              onClick={() => void handleExport()}
              title="Export extract D394 (document de lucru)"
            >
              {exporting ? "Export…" : "Extract XML"}
            </Btn>
            <Btn
              variant="primary"
              size="sm"
              icon="anaf"
              disabled={exportingOfficial || !activeCompanyId || !activeCompany}
              onClick={() => setShowD394Modal(true)}
              title="Export D394 conform schemei oficiale ANAF v5"
            >
              {exportingOfficial ? "Export…" : "Export oficial ANAF"}
            </Btn>
          </div>
        }
      >
        {isLoading ? (
          <div style={{ padding: "12px 16px", fontSize: 12.5, color: "var(--rf-text-muted)" }}>Se încarcă…</div>
        ) : isError ? (
          <div style={{ padding: "0 16px 16px" }}>
            <QueryErrorBanner error={error} label="raportul D394" onRetry={() => void refetch()} />
          </div>
        ) : !report || report.partners.length === 0 ? (
          <div style={{ padding: "12px 16px", fontSize: 12.5, color: "var(--rf-text-muted)" }}>
            Nicio livrare validată în perioada selectată.
          </div>
        ) : (
          <div className="rf-tbl-wrap">
            <table className="rf-tbl">
              <thead>
                <tr>
                  <th>CUI partener</th>
                  <th>Denumire</th>
                  <th>Tip</th>
                  <th className="right">Nr. facturi</th>
                  <th className="right">Bază impozabilă</th>
                  <th className="right">TVA</th>
                </tr>
              </thead>
              <tbody>
                {report.partners.map((p, i) => (
                  <tr key={i}>
                    <td className="rf-mono">{p.partnerCui || <span style={{ color: "var(--rf-text-dim)" }}>—</span>}</td>
                    <td style={{ fontWeight: 500 }}>{p.partnerName}</td>
                    <td><Badge variant="info">{p.vatCategory}</Badge></td>
                    <td className="right rf-mono">{p.invoiceCount}</td>
                    <td className="right rf-mono">{fmtRON(p.base)}</td>
                    <td className="right rf-mono">{fmtRON(p.vat)}</td>
                  </tr>
                ))}
              </tbody>
              <tfoot>
                <tr>
                  <td colSpan={3}>TOTAL</td>
                  <td className="right rf-mono">{report.invoiceCount}</td>
                  <td className="right rf-mono">{fmtRON(totalBase)}</td>
                  <td className="right rf-mono">{fmtRON(totalVat)}</td>
                </tr>
              </tfoot>
            </table>
          </div>
        )}
      </SectionCard>

      {/* ── Achiziții (received invoices) ──────────────────────────────── */}
      {report && (
        <SectionCard icon="fileIn" title="D394 — Achiziții per furnizor">
          {report.purchaseUnparsedCount > 0 && (
            <div style={{ padding: "0 16px 12px" }}>
              <Banner variant="warning">
                <b>{report.purchaseUnparsedCount}{" "}
                {report.purchaseUnparsedCount === 1 ? "factură primită nu are" : "facturi primite nu au"}{" "}
                încă defalcare TVA</b>{" "}
                — lista furnizorilor este parțială. Folosiți{" "}
                <b>«Recalculează TVA din XML»</b> în Jurnal cumpărări pentru a completa datele.
              </Banner>
            </div>
          )}

          {report.purchasePartners.length === 0 ? (
            <div style={{ padding: "12px 16px", fontSize: 12.5, color: "var(--rf-text-muted)" }}>
              {report.purchaseInvoiceCount === 0
                ? "Nicio factură primită în perioada selectată."
                : "Nicio factură primită cu defalcare TVA parsată. Folosiți «Recalculează TVA din XML» în Jurnal cumpărări."}
            </div>
          ) : (
            <div className="rf-tbl-wrap">
              <table className="rf-tbl">
                <thead>
                  <tr>
                    <th>CUI furnizor</th>
                    <th>Denumire</th>
                    <th>Tip</th>
                    <th className="right">Nr. facturi</th>
                    <th className="right">Bază impozabilă</th>
                    <th className="right">TVA</th>
                  </tr>
                </thead>
                <tbody>
                  {report.purchasePartners.map((p, i) => (
                    <tr key={i}>
                      <td className="rf-mono">{p.partnerCui || <span style={{ color: "var(--rf-text-dim)" }}>—</span>}</td>
                      <td style={{ fontWeight: 500 }}>{p.partnerName}</td>
                      <td><Badge variant="neutral">{p.vatCategory}</Badge></td>
                      <td className="right rf-mono">{p.invoiceCount}</td>
                      <td className="right rf-mono">{fmtRON(p.base)}</td>
                      <td className="right rf-mono">{fmtRON(p.vat)}</td>
                    </tr>
                  ))}
                </tbody>
                <tfoot>
                  <tr>
                    <td colSpan={3}>TOTAL ACHIZIȚII (parsate)</td>
                    <td className="right rf-mono">{report.purchasePartners.reduce((s, p) => s + p.invoiceCount, 0)}</td>
                    <td className="right rf-mono">{fmtRON(totalPurchaseBase)}</td>
                    <td className="right rf-mono">{fmtRON(totalPurchaseVat)}</td>
                  </tr>
                </tfoot>
              </table>
            </div>
          )}
        </SectionCard>
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
