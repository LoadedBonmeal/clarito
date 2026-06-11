/**
 * D394View — D394 livrări grupate pe partener + achiziții.
 * Embedded in the Reports page — Claude-Design classes (.scr-card / .scr-table /
 * .chip / .banner / .btn-dark / .pill-btn). ALL wiring preserved: compute query,
 * preflight, DUK block + "Exportă oricum", extract + official ANAF exports,
 * D394SubmissionModal.
 */

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
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
  const { t } = useTranslation();
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
    if (!activeCompanyId) { notify.warn(t("declarations.notify.selectCompany")); return; }
    if (!report || (report.partners.length === 0 && report.purchasePartners.length === 0)) {
      notify.info(t("declarations.notify.noData"));
      return;
    }
    const savePath = await saveDialog({
      title:       t("declarations.dialogs.saveD394Extract"),
      defaultPath: `d394-${periodFrom}-${periodTo}.xml`,
      filters:     [{ name: "XML", extensions: ["xml"] }],
    });
    if (!savePath) return;
    setExporting(true);
    try {
      const saved = await api.d394.export(activeCompanyId, periodFrom, periodTo, savePath);
      notify.success(t("declarations.d394.notify.extractSaved", { path: saved }));
      try { await openPath(saved); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, t("declarations.d394.notify.exportFailed")));
    } finally {
      setExporting(false);
    }
  };

  const handleExportOfficial = async (submission: D394Submission, override = false) => {
    if (!activeCompanyId) { notify.warn(t("declarations.notify.selectCompany")); return; }
    setLastSubmission(submission);
    const savePath = await saveDialog({
      title:       t("declarations.dialogs.saveD394Official"),
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
        notify.error(t("declarations.notify.dukErrors"));
        return;
      }
      setDukBlock(null);
      notify.success(
        res.dukAvailable
          ? t("declarations.d394.notify.officialSavedDuk", { path: res.path })
          : t("declarations.d394.notify.officialSavedNoDuk", { path: res.path }),
      );
      try { await openPath(res.path); } catch { /* reveal best-effort */ }
    } catch (err) {
      notify.error(formatError(err, t("declarations.d394.notify.exportOfficialFailed")));
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
            {t("declarations.common.exportAnyway")}
          </button>
        </div>
      )}

      {/* ── Livrări (vânzări) ──────────────────────────────────────────── */}
      <div className="scr-card">
        <div className="scr-toolbar">
          <div className="tt">{t("declarations.d394.title")}</div>
          <div className="spacer" />
          <button
            className="pill-btn"
            disabled={exporting || !activeCompanyId}
            onClick={() => void handleExport()}
            title={t("declarations.d394.extractTitle")}
          >
            <Ic name="dl" />
            {exporting ? t("declarations.common.exporting") : t("declarations.common.extractXml")}
          </button>
          <button
            className="btn-dark"
            disabled={exportingOfficial || !activeCompanyId || !activeCompany}
            onClick={() => setShowD394Modal(true)}
            title={t("declarations.d394.officialTitle")}
          >
            <Ic name="shield" />
            {exportingOfficial ? t("declarations.common.exporting") : t("declarations.d394.exportOfficial")}
          </button>
        </div>

        {isLoading ? (
          <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>{t("declarations.common.loading")}</div>
        ) : isError ? (
          <div style={{ padding: 16 }}>
            <QueryErrorBanner error={error} label={t("declarations.d394.reportLabel")} onRetry={() => void refetch()} />
          </div>
        ) : !report || report.partners.length === 0 ? (
          <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            {t("declarations.d394.emptySales")}
          </div>
        ) : (
          <>
            <table className="scr-table">
              <thead>
                <tr>
                  <th>{t("declarations.d394.headers.partnerCui")}</th>
                  <th>{t("declarations.d394.headers.name")}</th>
                  <th>{t("declarations.d394.headers.type")}</th>
                  <th className="r">{t("declarations.d394.headers.invoiceCount")}</th>
                  <th className="r">{t("declarations.d394.headers.base")}</th>
                  <th className="r">{t("declarations.d394.headers.vat")}</th>
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
              <span>{t("declarations.d394.total")} <b className="num">{report.invoiceCount}</b> {t("declarations.d394.invoicesWord")}</span>
              <span>{t("declarations.common.baseWord")} <b className="num">{fmtRON(totalBase)}</b></span>
              <span>{t("declarations.common.vat")} <b className="num">{fmtRON(totalVat)}</b></span>
            </div>
          </>
        )}
      </div>

      {/* ── Achiziții (received invoices) ──────────────────────────────── */}
      {report && (
        <div className="scr-card">
          <div className="scr-toolbar">
            <div className="tt">{t("declarations.d394.purchasesTitle")}</div>
          </div>

          {report.purchaseUnparsedCount > 0 && (
            <div style={{ padding: "14px 16px 0" }}>
              <div className="banner warn">
                <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_WARN }} />
                <span>
                  <b>
                    {t("declarations.detail.unparsedVat", { count: report.purchaseUnparsedCount })}
                  </b>{" "}
                  {t("declarations.d394.unparsedRest1")}{" "}
                  <b>{t("declarations.d394.unparsedAction")}</b> {t("declarations.d394.unparsedRest2")}
                </span>
              </div>
            </div>
          )}

          {report.purchasePartners.length === 0 ? (
            <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
              {report.purchaseInvoiceCount === 0
                ? t("declarations.d394.emptyPurchases")
                : t("declarations.d394.emptyUnparsed")}
            </div>
          ) : (
            <>
              <table className="scr-table">
                <thead>
                  <tr>
                    <th>{t("declarations.d394.headers.supplierCui")}</th>
                    <th>{t("declarations.d394.headers.name")}</th>
                    <th>{t("declarations.d394.headers.type")}</th>
                    <th className="r">{t("declarations.d394.headers.invoiceCount")}</th>
                    <th className="r">{t("declarations.d394.headers.base")}</th>
                    <th className="r">{t("declarations.d394.headers.vat")}</th>
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
                  {t("declarations.d394.totalPurchases")}{" "}
                  <b className="num">{report.purchasePartners.reduce((s, p) => s + p.invoiceCount, 0)}</b> {t("declarations.d394.invoicesWord")}
                </span>
                <span>{t("declarations.common.baseWord")} <b className="num">{fmtRON(totalPurchaseBase)}</b></span>
                <span>{t("declarations.common.vat")} <b className="num">{fmtRON(totalPurchaseVat)}</b></span>
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
