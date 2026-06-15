/**
 * Factură detaliu — verbatim port of the design "Factura detaliu.html":
 *   .page-head (.crumb "Facturi emise › nr" · .head-title h1+chip · sub
 *   "client · emisă · scadență" · .head-actions PDF / XML / Storno /
 *   btn-dark "Înregistrează plata" + "···" pop with the real extra actions) →
 *   .scr-card .steps timeline (Schiță → Trimisă → Validată → Încasată) →
 *   .cols-2: left = Linii factură (.scr-table + .sold-line totals) + Status
 *   ANAF · evenimente (.spv-log index încărcare/index ANAF + jurnal complet),
 *   right = Client (.kv fișă) + Detalii (real metadata the prototype lacks) +
 *   Plăți încasate (.pay-row + .sold-line sold rămas) →
 *   .modal-back/.modal storno + înregistrează plata + șablon recurent.
 *
 * ALL wiring preserved: api.invoices.get, api.companies.get, api.contacts.get,
 * api.payments.summary, api.anaf.isAuthenticated/authorize/submitInvoice/
 * checkStatus (test mode), api.ubl.generatePdf (openPath) / generateXml,
 * api.invoices.duplicate/storno, api.recurring.create (șablon recurent),
 * api.integrations.smartbillPush, mailto email client, copy-XML clipboard,
 * window.print, storno-DRAFT guidance banner, e-Factura 5-day deadline banner.
 * Payment registration wired for real: api.payments.add (+ exchangeRate for
 * FX invoices) and api.payments.delete on the listed receipts.
 */

import { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { useParams, useNavigate } from "@tanstack/react-router";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";

import { Ic } from "@/components/shared/Ic";
import { useAnimatedClose } from "@/hooks/use-animated-close";
import { useOpenPdf } from "@/hooks/use-open-pdf";
import { notify } from "@/lib/toasts";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import type { AddPaymentArgs, Payment } from "@/lib/tauri";
import { fmtRON, parseDec } from "@/lib/utils";
import { formatError } from "@/lib/error-mapper";
import type { InvoiceStatus } from "@/types";
import { useAppStore } from "@/lib/store";
import { fmtShortcut } from "@/lib/platform";
import { efacturaDeadline, deadlineDaysLeft, formatDeadline } from "@/lib/efacturaDeadline";

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];
const fmtRoDate = (iso: string | null | undefined) => {
  if (!iso) return "—";
  const [y, m, d] = iso.split("-");
  return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
};
/** Unix seconds → "30 mai 2026, 10:44" (prototype .sd/.e2 format). */
const fmtRoDateTime = (unixSec: number | null | undefined) => {
  if (!unixSec) return "—";
  const d = new Date(unixSec * 1000);
  return `${String(d.getDate()).padStart(2, "0")} ${RO_MON[d.getMonth()]} ${d.getFullYear()}, ${String(
    d.getHours(),
  ).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
};

/** "Banchero Media SRL" → "BM" (prototype .cli-ava initials). */
function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "—";
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase();
  return (parts[0][0] + parts[1][0]).toUpperCase();
}

// Status → design chip (.chip variants + icon + i18n label key) — head .head-title chip.
const STATUS_CHIP: Record<InvoiceStatus, { cls: string; icon: string; labelKey: string }> = {
  DRAFT:     { cls: "sent", icon: "docText", labelKey: "detail.status.draft" },
  QUEUED:    { cls: "wait", icon: "clock",   labelKey: "detail.status.queued" },
  SUBMITTED: { cls: "sent", icon: "send",    labelKey: "detail.status.submitted" },
  VALIDATED: { cls: "paid", icon: "checkC",  labelKey: "detail.status.validatedAnaf" },
  REJECTED:  { cls: "late", icon: "xMark",   labelKey: "detail.status.rejected" },
  STORNED:   { cls: "wait", icon: "undo",    labelKey: "detail.status.storned" },
};

const METHOD_KEYS: Record<string, string> = {
  transfer: "detail.method.transfer",
  cash: "detail.method.cash",
  card: "detail.method.card",
  other: "detail.method.other",
};

export function InvoiceDetailPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { id } = useParams({ from: "/invoices/$id" });
  const setSelectedInvoiceId = useAppStore((s) => s.setSelectedInvoiceId);
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);

  useEffect(() => {
    setSelectedInvoiceId(id);
    return () => setSelectedInvoiceId(null);
  }, [id, setSelectedInvoiceId]);

  const [actionError, setActionError] = useState<string | null>(null);
  const [statusMessage, setStatusMessage] = useState<string | null>(null);
  const [showStornoModal, setShowStornoModal] = useState(false);
  const [stornoReason, setStornoReason] = useState("");
  const [xmlCopied, setXmlCopied] = useState(false);
  const [openPop, setOpenPop] = useState<"" | "more">("");

  // Pay modal (design payModal) — real api.payments.add wiring
  const [showPayModal, setShowPayModal] = useState(false);
  const [payForm, setPayForm] = useState({
    amount: "",
    paidAt: new Date().toISOString().slice(0, 10),
    method: "transfer",
    reference: "",
    notes: "",
    exchangeRate: "",
  });

  // Save-as-recurring-template state
  const [showSaveAsTemplate, setShowSaveAsTemplate] = useState(false);
  const [templateFrequency, setTemplateFrequency] = useState("monthly");
  const [templateName, setTemplateName] = useState("");

  // Animated-exit close handlers (play .modal-back.closing before unmount)
  const { closing: stornoClosing, close: closeStorno } = useAnimatedClose(
    useCallback(() => { setShowStornoModal(false); setStornoReason(""); }, []),
  );
  const { closing: payClosing, close: closePay } = useAnimatedClose(
    useCallback(() => setShowPayModal(false), []),
  );
  const { closing: templateClosing, close: closeTemplate } = useAnimatedClose(
    useCallback(() => setShowSaveAsTemplate(false), []),
  );

  // Close head "···" pop on outside click (design .pop pattern)
  useEffect(() => {
    if (!openPop) return;
    const h = () => setOpenPop("");
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, [openPop]);

  const openPdf = useOpenPdf();

  const { data, isLoading } = useQuery({
    queryKey: queryKeys.invoices.detail(id),
    queryFn: () => api.invoices.get(id, activeCompanyId ?? ""),
    enabled: !!activeCompanyId,
  });

  const { data: company } = useQuery({
    queryKey: queryKeys.companies.detail(data?.invoice.companyId ?? ""),
    queryFn: () => api.companies.get(data!.invoice.companyId),
    enabled: !!data?.invoice.companyId,
  });

  const { data: contact } = useQuery({
    queryKey: queryKeys.contacts.detail(data?.invoice.contactId ?? ""),
    queryFn: () => api.contacts.get(data!.invoice.contactId, activeCompanyId ?? ""),
    enabled: !!data?.invoice.contactId && !!activeCompanyId,
  });

  const { data: paymentSummary } = useQuery({
    queryKey: queryKeys.payments.summary(id, data?.invoice.companyId ?? ""),
    queryFn: () => api.payments.summary(id, data!.invoice.companyId),
    enabled: !!data?.invoice.companyId,
  });

  const { data: isAnafAuth, refetch: refetchAnafAuth } = useQuery({
    queryKey: queryKeys.anaf.auth(data?.invoice.companyId ?? ""),
    queryFn: () => api.anaf.isAuthenticated(data!.invoice.companyId),
    enabled: !!data?.invoice.companyId,
  });

  const { data: testModeSetting } = useQuery({
    queryKey: queryKeys.anaf.testMode,
    queryFn: () => api.settings.get("use_anaf_test_env"),
  });

  const testMode = testModeSetting === "1";

  const generateXml = useMutation({
    mutationFn: () => {
      if (!activeCompanyId) return Promise.reject(new Error(t("detail.noActiveCompany")));
      return api.ubl.generateXml(id, activeCompanyId);
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.detail(id) });
      setActionError(null);
      notify.success(t("detail.notify.xmlGenerated"));
    },
    onError: (e) => setActionError(formatError(e, t("detail.notify.xmlError"))),
  });

  const generatePdf = useMutation({
    mutationFn: () => {
      if (!activeCompanyId) return Promise.reject(new Error(t("detail.noActiveCompany")));
      return api.ubl.generatePdf(id, activeCompanyId);
    },
    onSuccess: async (pdfPath) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.detail(id) });
      setActionError(null);
      notify.success(t("detail.notify.pdfGenerated"));
      if (pdfPath) {
        try { await openPdf(pdfPath, `${data?.invoice.fullNumber ?? "factura"}.pdf`); }
        catch (e) { notify.error(t("detail.notify.pdfOpenError", { error: String(e) })); }
      }
    },
    onError: (e) => setActionError(formatError(e, t("detail.notify.pdfError"))),
  });

  const authorizeAnaf = useMutation({
    mutationFn: () => api.anaf.authorize(data!.invoice.companyId),
    onSuccess: () => { void refetchAnafAuth(); },
    onError: (e) => setActionError(formatError(e, t("detail.notify.anafAuthError"))),
  });

  const submitInvoice = useMutation({
    mutationFn: async () => {
      const companyId = data!.invoice.companyId;
      let authenticated = isAnafAuth;
      if (!authenticated) {
        await api.anaf.authorize(companyId);
        authenticated = await api.anaf.isAuthenticated(companyId);
        if (!authenticated) throw new Error(t("detail.notify.anafAuthFailed"));
      }
      return api.anaf.submitInvoice(companyId, id, testMode);
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.detail(id) });
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
      void queryClient.invalidateQueries({ queryKey: queryKeys.anaf.auth(data?.invoice.companyId ?? "") });
      setActionError(null);
      setStatusMessage(null);
    },
    onError: (e) => setActionError(formatError(e, t("detail.notify.anafSendError"))),
  });

  const checkStatus = useMutation({
    mutationFn: () => api.anaf.checkStatus(data!.invoice.companyId, id, testMode),
    onSuccess: (stare) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.detail(id) });
      setActionError(null);
      setStatusMessage(t("detail.notify.anafStatus", { status: stare }));
    },
    onError: (e) => setActionError(formatError(e, t("detail.notify.statusCheckError"))),
  });

  const duplicateInvoice = useMutation({
    mutationFn: () => {
      if (!activeCompanyId) return Promise.reject(new Error(t("detail.noActiveCompany")));
      return api.invoices.duplicate(id, activeCompanyId);
    },
    onSuccess: (newId) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
      notify.success(t("detail.notify.duplicated"));
      void navigate({ to: "/invoices/$id", params: { id: newId } });
    },
    onError: (e) => setActionError(formatError(e, t("detail.notify.duplicateError"))),
  });

  const stornoInvoice = useMutation({
    mutationFn: (reason: string) => {
      if (!activeCompanyId) return Promise.reject(new Error(t("detail.noActiveCompany")));
      return api.invoices.storno(id, activeCompanyId, reason);
    },
    onSuccess: (stornoInv) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.detail(id) });
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
      setStatusMessage(t("detail.notify.stornoCreated", { nr: stornoInv.fullNumber }));
      setActionError(null);
      // Navigate to the new credit note so the accountant sees the guidance banner
      // and can immediately generate XML + submit to ANAF to complete the cancellation.
      void navigate({ to: "/invoices/$id", params: { id: stornoInv.id } });
    },
    onError: (e) => setActionError(formatError(e, t("detail.notify.stornoError"))),
  });

  const pushSmartbill = useMutation({
    mutationFn: () => api.integrations.smartbillPush(data!.invoice.companyId, id),
    onSuccess: (result) => {
      setStatusMessage(t("detail.notify.smartbillSent", { result }));
      setActionError(null);
    },
    onError: (e) => setActionError(formatError(e, t("detail.notify.smartbillError"))),
  });

  const addPayment = useMutation({
    mutationFn: (args: AddPaymentArgs) => api.payments.add(args),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.payments.summary(id, data?.invoice.companyId ?? "") });
      void queryClient.invalidateQueries({ queryKey: ["payments"] });
      void queryClient.invalidateQueries({ queryKey: ["payment_summaries"] });
      notify.success(t("detail.notify.paymentAdded"));
      setShowPayModal(false);
    },
    onError: (e) => notify.error(formatError(e, t("detail.notify.paymentAddError"))),
  });

  const deletePayment = useMutation({
    mutationFn: (paymentId: string) => api.payments.delete(paymentId, data!.invoice.companyId),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.payments.summary(id, data?.invoice.companyId ?? "") });
      void queryClient.invalidateQueries({ queryKey: ["payments"] });
      void queryClient.invalidateQueries({ queryKey: ["payment_summaries"] });
      notify.success(t("detail.notify.paymentDeleted"));
    },
    onError: (e) => notify.error(formatError(e, t("detail.notify.paymentDeleteError"))),
  });

  const saveAsTemplateMutation = useMutation({
    mutationFn: (args: Parameters<typeof api.recurring.create>[0]) => api.recurring.create(args),
    onSuccess: () => {
      notify.success(t("detail.notify.templateCreated"));
      setShowSaveAsTemplate(false);
      setTemplateName("");
      setTemplateFrequency("monthly");
    },
    onError: (e) => setActionError(formatError(e, t("detail.notify.templateError"))),
  });

  function nextMonthDate(): string {
    const d = new Date();
    const next = new Date(d.getFullYear(), d.getMonth() + 1, d.getDate());
    return `${next.getFullYear()}-${String(next.getMonth() + 1).padStart(2, "0")}-${String(next.getDate()).padStart(2, "0")}`;
  }

  function handleSaveAsTemplate() {
    if (!data) return;
    const { invoice, lines: invoiceLines } = data;
    if (!templateName.trim()) { notify.warn(t("detail.notify.templateNameRequired")); return; }
    const recurringLines = invoiceLines.map((l) => ({
      name: l.name,
      quantity: typeof l.quantity === "string" ? Number(l.quantity) : l.quantity,
      unit: l.unit ?? "buc",
      unitPrice: typeof l.unitPrice === "string" ? Number(l.unitPrice) : l.unitPrice,
      vatRate: typeof l.vatRate === "string" ? Number(l.vatRate) : l.vatRate,
      vatCategory: l.vatCategory,
    }));
    saveAsTemplateMutation.mutate({
      companyId: invoice.companyId,
      templateName: templateName.trim(),
      clientId: invoice.contactId,
      frequency: templateFrequency,
      nextIssueDate: nextMonthDate(),
      dayOfMonth: new Date().getDate(),
      autoSubmitAnaf: false,
      series: invoice.series,
      linesJson: JSON.stringify(recurringLines),
      notes: undefined,
    });
  }

  async function handleCopyXml() {
    if (!data?.invoice.xmlPath) return;
    try {
      const { readTextFile } = await import("@tauri-apps/plugin-fs");
      const content = await readTextFile(data.invoice.xmlPath);
      await writeText(content);
      setXmlCopied(true);
      setTimeout(() => setXmlCopied(false), 2000);
    } catch {
      setActionError(t("detail.notify.copyXmlError"));
    }
  }

  if (!activeCompanyId) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>{t("detail.invoiceTitle")}</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          {t("detail.selectCompany")}
        </div>
      </div>
    );
  }

  if (isLoading) {
    return (
      <div className="main-inner wide">
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>{t("detail.loading")}</div>
      </div>
    );
  }

  if (!data) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>{t("detail.invoiceTitle")}</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          {t("detail.notFound")}
        </div>
      </div>
    );
  }

  const { invoice, lines, events } = data;
  const chip = STATUS_CHIP[invoice.status] ?? STATUS_CHIP.DRAFT;

  // ── payments / timeline derived state ──────────────────────────────────────
  const total = parseDec(invoice.totalAmount);
  const paid = parseDec(paymentSummary?.paidAmount ?? "0");
  const remaining = Math.max(0, total - paid);
  const payStatus = paymentSummary?.paymentStatus ?? "UNPAID";
  const payChip = payStatus === "PAID"
    ? { cls: "paid", icon: "check", label: t("detail.pay.collected") }
    : payStatus === "PARTIAL"
      ? { cls: "wait", icon: "clock", label: t("detail.pay.partial") }
      : { cls: "sent", icon: "dot", label: t("detail.pay.uncollected") };
  const payments: Payment[] = paymentSummary?.payments ?? [];
  const isFx = invoice.currency !== "RON";

  // Reached stage: 0 Schiță · 1 Trimisă · 2 Validată · 3 Încasată integral.
  const rejected = invoice.status === "REJECTED";
  const reached = payStatus === "PAID"
    ? 3
    : invoice.anafValidatedAt || invoice.status === "VALIDATED" || invoice.status === "STORNED"
      ? 2
      : invoice.anafSubmittedAt || invoice.status === "SUBMITTED" || invoice.status === "QUEUED"
        ? 1
        : 0;
  const stepCls = (idx: number) =>
    idx < reached ? "step done" : idx === reached ? "step curr" : "step todo";

  const payAmountNum = parseFloat(payForm.amount);
  const paySaveDisabled = addPayment.isPending || !payForm.amount || !payForm.paidAt;

  const canStorno = invoice.status === "VALIDATED" && invoice.stornoOfInvoiceId === null;
  const canSubmit = !!invoice.xmlPath && invoice.status === "DRAFT";
  const canCheckStatus = invoice.status === "SUBMITTED" || invoice.status === "QUEUED" || !!invoice.anafIndex;

  return (
    <div className="main-inner wide">

      {/* page head */}
      <div className="page-head">
        <div>
          <div className="crumb">
            <a onClick={() => void navigate({ to: "/invoices" })} style={{ cursor: "pointer" }}>{t("detail.crumb.issued")}</a>
            <span className="sep">›</span>
            <span className="num">{invoice.fullNumber}</span>
          </div>
          <div className="head-title">
            <h1 className="num">{invoice.fullNumber}</h1>
            <span className={`chip ${chip.cls}`}><Ic name={chip.icon} cls="sic" />{t(chip.labelKey)}</span>
          </div>
          <p className="sub">
            {contact?.legalName ?? "—"} · {t("detail.head.issuedOn", { date: fmtRoDate(invoice.issueDate) })} · {t("detail.head.dueOn", { date: fmtRoDate(invoice.dueDate) })}
          </p>
        </div>
        <div className="head-actions">
          <button className="pill-btn" disabled={generatePdf.isPending} onClick={() => generatePdf.mutate()}>
            <Ic name="dl" />{generatePdf.isPending ? "PDF…" : "PDF"}
          </button>
          <button className="pill-btn" disabled={generateXml.isPending} onClick={() => generateXml.mutate()}>
            <Ic name="code" />{generateXml.isPending ? "XML…" : invoice.xmlPath ? "XML" : t("detail.actions.generateXml")}
          </button>
          {canStorno && (
            <button
              className="pill-btn"
              style={{ color: "var(--amber)" }}
              onClick={() => setShowStornoModal(true)}
            >
              <svg className="ic" viewBox="0 0 24 24" style={{ stroke: "var(--amber)" }} aria-hidden="true">
                <path d="M9 15 3 9m0 0 6-6M3 9h12a6 6 0 0 1 0 12h-3" />
              </svg>
              {t("detail.actions.storno")}
            </button>
          )}
          {invoice.status !== "DRAFT" && invoice.status !== "STORNED" && (
            <button className="btn-dark" onClick={() => {
              setPayForm({
                amount: remaining > 0 ? remaining.toFixed(2) : "",
                paidAt: new Date().toISOString().slice(0, 10),
                method: "transfer",
                reference: "",
                notes: "",
                exchangeRate: "",
              });
              setShowPayModal(true);
            }}>
              <Ic name="card" />{t("detail.actions.recordPayment")}
            </button>
          )}

          {/* more-actions pop — real features the prototype lacks */}
          <div className="nou-wrap" style={{ position: "relative" }}>
            <button
              className="sq-btn"
              title={t("detail.actions.moreTitle")}
              onMouseDown={(e) => e.stopPropagation()}
              onClick={() => setOpenPop(openPop === "more" ? "" : "more")}
            >
              <Ic name="dots" />
            </button>
            {openPop === "more" && (
              <div className="pop show" style={{ right: 0, top: 40, width: 230 }} onMouseDown={(e) => e.stopPropagation()}>
                <div className="col-title">{t("detail.actions.invoiceActions")}</div>
                {invoice.status === "DRAFT" && (
                  <button className="pop-item" onClick={() => { setOpenPop(""); void navigate({ to: "/invoices/$id/edit", params: { id } }); }}>
                    <Ic name="pen" />{t("detail.actions.edit")}
                  </button>
                )}
                <button className="pop-item" disabled={duplicateInvoice.isPending} onClick={() => { setOpenPop(""); duplicateInvoice.mutate(); }}>
                  <Ic name="copy" />{duplicateInvoice.isPending ? t("detail.actions.duplicating") : t("detail.actions.duplicate")}
                </button>
                <button className="pop-item" title={t("detail.actions.printTitle", { shortcut: fmtShortcut("Ctrl+P") })} onClick={() => { setOpenPop(""); window.print(); }}>
                  <Ic name="printer" />{t("detail.actions.print")}
                </button>
                <button
                  className="pop-item"
                  onClick={() => {
                    setOpenPop("");
                    setTemplateName(t("detail.templateModal.defaultName", { nr: invoice.fullNumber }));
                    setTemplateFrequency("monthly");
                    setShowSaveAsTemplate(true);
                  }}
                >
                  <Ic name="loop" />{t("detail.actions.recurringTemplate")}
                </button>
                <div className="pop-div" />
                <div className="col-title">{t("detail.actions.sendSection")}</div>
                {contact?.email && (
                  <button
                    className="pop-item"
                    onClick={() => {
                      setOpenPop("");
                      const subject = encodeURIComponent(t("detail.email.subject", { nr: invoice.fullNumber }));
                      const body = encodeURIComponent(
                        t("detail.email.body", {
                          nr: invoice.fullNumber,
                          date: invoice.issueDate,
                          amount: fmtRON(invoice.totalAmount),
                          currency: invoice.currency,
                        }),
                      );
                      void openUrl(`mailto:${encodeURIComponent(contact.email ?? "")}?subject=${subject}&body=${body}`);
                    }}
                  >
                    <Ic name="mail" />{t("detail.actions.emailToClient")}
                  </button>
                )}
                <button className="pop-item" disabled={pushSmartbill.isPending} onClick={() => { setOpenPop(""); pushSmartbill.mutate(); }}>
                  <Ic name="docUp" />{pushSmartbill.isPending ? "SmartBill…" : t("detail.actions.sendSmartbill")}
                </button>
                {invoice.xmlPath && (
                  <button className="pop-item" onClick={() => { setOpenPop(""); void handleCopyXml(); }}>
                    <Ic name="copy" />{xmlCopied ? t("detail.actions.copied") : t("detail.actions.copyXml")}
                  </button>
                )}
              </div>
            )}
          </div>
        </div>
      </div>

      {/* error / status banners (design .banner) */}
      {actionError && (
        <div className="banner danger">
          <Ic name="xMark" />
          <div><b>{t("detail.banner.error")}</b> {actionError}</div>
          <span className="bx" onClick={() => setActionError(null)}>✕</span>
        </div>
      )}
      {statusMessage && !actionError && (
        <div className="banner ok">
          <Ic name="check" />
          <div>{statusMessage}</div>
          <span className="bx" onClick={() => setStatusMessage(null)}>✕</span>
        </div>
      )}

      {/* REG-STORNO: guidance banner for DRAFT credit notes.
          A storno credit note that is still DRAFT has NOT been sent to ANAF yet
          and therefore has NOT fiscally cancelled the original invoice. */}
      {invoice.status === "DRAFT" && invoice.stornoOfInvoiceId !== null && (
        <div className="banner warn">
          <Ic name="undo" />
          <div>
            <b>{t("detail.banner.stornoTitle")}</b> {t("detail.banner.stornoBody1")}{" "}
            <b>{t("detail.banner.stornoBodyEm")}</b> {t("detail.banner.stornoBody2")}
          </div>
        </div>
      )}

      {/* e-Factura 5-working-day send deadline (2026+) — informational. */}
      {invoice.issueDate >= "2026-01-01" &&
        ["DRAFT", "QUEUED", "REJECTED"].includes(invoice.status) &&
        (() => {
          const dl = efacturaDeadline(invoice.issueDate);
          if (!dl) return null;
          const left = deadlineDaysLeft(dl);
          const overdue = left < 0;
          const n = Math.abs(left);
          return (
            <div className={`banner${overdue ? " danger" : left <= 1 ? " warn" : ""}`}>
              <Ic name="calendar" />
              <div>
                {t("detail.banner.deadlinePrefix")} <b>{formatDeadline(dl)}</b>{" "}
                {overdue
                  ? t("detail.banner.deadlineOverdue", { count: n })
                  : t("detail.banner.deadlineLeft", { count: left })}
              </div>
            </div>
          );
        })()}

      {/* timeline */}
      <div className="scr-card" style={{ marginBottom: 14 }}>
        <div className="card-pad" style={{ padding: "20px 28px 18px" }}>
          <div className="steps">
            <div className={stepCls(0)}>
              <div className="dot">{reached >= 0 && <Ic name="check" cls="sic" />}</div>
              <div className="sl">{t("detail.timeline.draft")}</div>
              <div className="sd num">{fmtRoDateTime(invoice.createdAt)}</div>
            </div>
            <div className={stepCls(1)}>
              <div className="dot">{reached >= 1 && <Ic name="check" cls="sic" />}</div>
              <div className="sl">{t("detail.timeline.submitted")}</div>
              <div className="sd num">
                {invoice.anafSubmittedAt
                  ? fmtRoDateTime(invoice.anafSubmittedAt)
                  : invoice.status === "QUEUED" ? t("detail.timeline.queued") : "—"}
              </div>
            </div>
            <div className={rejected ? "step fail" : stepCls(2)}>
              <div className="dot">
                {rejected ? <Ic name="xMark" cls="sic" /> : reached >= 2 ? <Ic name="check" cls="sic" /> : null}
              </div>
              <div className="sl">{rejected ? t("detail.timeline.rejectedAnaf") : t("detail.timeline.validated")}</div>
              <div className="sd num">
                {rejected
                  ? fmtRoDateTime(invoice.anafRejectedAt)
                  : invoice.anafValidatedAt ? fmtRoDateTime(invoice.anafValidatedAt) : "—"}
              </div>
            </div>
            <div className={stepCls(3)}>
              <div className="dot">{reached >= 3 && <Ic name="check" cls="sic" />}</div>
              <div className="sl">{t("detail.timeline.collectedFull")}</div>
              <div className="sd">
                {payStatus === "PAID"
                  ? <span className="num">{t("detail.timeline.collectedFullLower")}</span>
                  : <>{t("detail.timeline.remainingBalance")} <span className="num">{fmtRON(remaining)} {invoice.currency}</span></>}
              </div>
            </div>
          </div>
        </div>
      </div>

      <div className="cols-2">
        <div>
          {/* linii */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">{t("detail.lines.title")}</div>
              <div className="spacer" />
              <span className="muted num" style={{ fontSize: 12 }}>{invoice.currency}</span>
            </div>
            {lines.length === 0 ? (
              <div style={{ padding: "30px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
                {t("detail.lines.empty")}
              </div>
            ) : (
              <table className="scr-table">
                <thead>
                  <tr><th>{t("detail.lines.item")}</th><th className="r">{t("detail.lines.qty")}</th><th>{t("detail.lines.unit")}</th><th className="r">{t("detail.lines.unitPrice")}</th><th>{t("detail.lines.vat")}</th><th className="r">{t("detail.lines.amount")}</th></tr>
                </thead>
                <tbody>
                  {lines.map((l) => (
                    <tr key={l.id}>
                      <td>
                        {l.name}
                        {l.description && (
                          <div style={{ fontSize: 11.5, color: "var(--text-2)", marginTop: 1 }}>{l.description}</div>
                        )}
                      </td>
                      <td className="r num">{l.quantity}</td>
                      <td>{l.unit}</td>
                      <td className="r num">{fmtRON(l.unitPrice)}</td>
                      <td><span className="doc">{parseDec(l.vatRate)}% · {l.vatCategory}</span></td>
                      <td className="r num">{fmtRON(l.subtotalAmount)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
            <div className="sold-line" style={{ borderTop: "1px solid var(--line)", background: "var(--bg-content)" }}>
              <span>
                {t("detail.lines.subtotal")} <span className="num">{fmtRON(invoice.subtotalAmount)}</span> · {t("detail.lines.vat")}{" "}
                <span className="num">{fmtRON(invoice.vatAmount)}</span>
              </span>
              <b className="num">{t("detail.lines.total")} {fmtRON(invoice.totalAmount)} {invoice.currency}</b>
            </div>
          </div>

          {/* istoric SPV + jurnal */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">{t("detail.anaf.title")}</div>
              <div className="spacer" />
              <span className="muted" style={{ fontSize: 12 }}>
                e-Factura · {testMode ? t("detail.anaf.testEnv") : t("detail.anaf.prodEnv")}
              </span>
              {canSubmit && (
                <button
                  className="pill-btn send-btn"
                  disabled={submitInvoice.isPending || authorizeAnaf.isPending}
                  onClick={() => submitInvoice.mutate()}
                >
                  <Ic name="send" />
                  {authorizeAnaf.isPending ? t("detail.anaf.authorizing") : submitInvoice.isPending ? t("detail.anaf.sending") : t("detail.anaf.sendToAnaf")}
                </button>
              )}
              {canCheckStatus && (
                <button
                  className="sq-btn spin-btn"
                  title={t("detail.anaf.checkStatusTitle")}
                  disabled={checkStatus.isPending}
                  onClick={() => checkStatus.mutate()}
                >
                  <Ic name="sync" />
                </button>
              )}
            </div>
            <div className="spv-log">
              {/* SPV milestones — index încărcare / index ANAF (real values) */}
              {invoice.anafSubmittedAt && (
                <div className="spv-ev">
                  <div className="act-ic"><Ic name="send" /></div>
                  <div>
                    <div className="e1"><b>{t("detail.anaf.sentToSpv")}</b></div>
                    <div className="e2 num">
                      {fmtRoDateTime(invoice.anafSubmittedAt)}
                      {invoice.anafUploadId && <> · {t("detail.anaf.uploadIndex")} <span className="doc">{invoice.anafUploadId}</span></>}
                    </div>
                  </div>
                </div>
              )}
              {invoice.anafValidatedAt && (
                <div className="spv-ev">
                  <div className="act-ic"><Ic name="checkC" /></div>
                  <div>
                    <div className="e1"><b>{t("detail.anaf.validatedBy")}</b> {t("detail.anaf.noErrors")}</div>
                    <div className="e2 num">
                      {fmtRoDateTime(invoice.anafValidatedAt)}
                      {invoice.anafIndex && <> · {t("detail.anaf.anafIndex")} <span className="doc">{invoice.anafIndex}</span></>}
                    </div>
                  </div>
                </div>
              )}
              {invoice.anafRejectedAt && (
                <div className="spv-ev">
                  <div className="act-ic"><Ic name="xMark" /></div>
                  <div>
                    <div className="e1"><b style={{ color: "var(--red)" }}>{t("detail.anaf.rejectedBy")}</b>{invoice.rejectionCode ? ` · ${t("detail.anaf.code", { code: invoice.rejectionCode })}` : ""}</div>
                    <div className="e2 num">
                      {fmtRoDateTime(invoice.anafRejectedAt)}
                      {invoice.rejectionReason && <> · {invoice.rejectionReason}</>}
                    </div>
                  </div>
                </div>
              )}
              {!invoice.anafSubmittedAt && !invoice.anafValidatedAt && !invoice.anafRejectedAt && (
                <div className="spv-ev">
                  <div className="act-ic"><Ic name="clock" /></div>
                  <div>
                    <div className="e1"><b>{t("detail.anaf.notSent")}</b></div>
                    <div className="e2">
                      {invoice.xmlPath ? t("detail.anaf.xmlReady") : t("detail.anaf.generateFirst")}
                    </div>
                  </div>
                </div>
              )}

              {/* full event journal — real feature the prototype lacks */}
              {events.length > 0 && (
                <>
                  <div className="pop-div" style={{ margin: "6px 10px" }} />
                  <div className="col-title">{t("detail.anaf.fullJournal")}</div>
                  {events.map((e) => (
                    <div className="spv-ev" key={e.id}>
                      <div className="act-ic"><Ic name="docText" /></div>
                      <div>
                        <div className="e1"><b>{e.message}</b></div>
                        <div className="e2 num">{fmtRoDateTime(e.createdAt)} · {e.eventType}</div>
                      </div>
                    </div>
                  ))}
                </>
              )}
            </div>
            {/* ANAF auth indicator — real feature the prototype lacks */}
            <div className="sold-line">
              {isAnafAuth ? (
                <span style={{ display: "flex", alignItems: "center", gap: 6, color: "var(--green)" }}>
                  <Ic name="check" cls="sic" />{t("detail.anaf.authenticated")}
                </span>
              ) : (
                <span style={{ display: "flex", alignItems: "center", gap: 6 }}>
                  {t("detail.anaf.notAuthenticated")}
                  <a
                    className="link"
                    style={{ cursor: "pointer" }}
                    onClick={() => { if (!authorizeAnaf.isPending) authorizeAnaf.mutate(); }}
                  >
                    {authorizeAnaf.isPending ? t("detail.anaf.authorizingLower") : t("detail.anaf.authorize")}
                  </a>
                </span>
              )}
              <span className="muted" style={{ fontSize: 11.5 }}>{t("detail.anaf.archiveNote")}</span>
            </div>
          </div>
        </div>

        <div>
          {/* client */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">{t("detail.client.title")}</div>
              <div className="spacer" />
              <a
                className="see-all"
                style={{ height: "auto", padding: 0, cursor: "pointer" }}
                onClick={() => void navigate({ to: "/contacts" })}
              >
                {t("detail.client.viewProfile")}<Ic name="chevR" />
              </a>
            </div>
            <div className="card-pad">
              {contact ? (
                <>
                  <div className="cli" style={{ marginBottom: 12 }}>
                    <span className="cli-ava">{initials(contact.legalName)}</span>
                    <b style={{ fontSize: 13.5 }}>{contact.legalName}</b>
                  </div>
                  <dl className="kv" style={{ gridTemplateColumns: "110px 1fr", fontSize: 12.5 }}>
                    <dt>CUI</dt><dd className="num">{contact.cui ?? "—"}</dd>
                    <dt>{t("detail.client.address")}</dt>
                    <dd>{[contact.address, contact.city, contact.county].filter(Boolean).join(", ") || "—"}</dd>
                    <dt>{t("detail.client.email")}</dt><dd>{contact.email ?? "—"}</dd>
                    <dt>{t("detail.client.phone")}</dt><dd className="num">{contact.phone ?? "—"}</dd>
                    <dt>{t("detail.client.vat")}</dt>
                    <dd>{contact.vatPayer ? t("detail.client.payer") : t("detail.client.nonPayer")} · {t("detail.client.cashVat")} {contact.cashVat ? t("detail.client.yes") : t("detail.client.no")}</dd>
                  </dl>
                </>
              ) : (
                <div style={{ fontSize: 12.5, color: "var(--text-2)" }}>{t("detail.client.clientId")} {invoice.contactId}</div>
              )}
            </div>
          </div>

          {/* detalii factură — real metadata the prototype lacks */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar"><div className="tt">{t("detail.details.title")}</div></div>
            <div className="card-pad">
              <dl className="kv" style={{ gridTemplateColumns: "110px 1fr", fontSize: 12.5 }}>
                <dt>{t("detail.details.series")}</dt><dd className="num">{invoice.series}</dd>
                <dt>{t("detail.details.supplier")}</dt>
                <dd>{company ? <>{company.legalName} <span className="num">· {company.cui}</span></> : invoice.companyId}</dd>
                <dt>{t("detail.details.currency")}</dt><dd className="num">{invoice.currency}</dd>
                {invoice.exchangeRate !== null && (
                  <><dt>{t("detail.details.fxRate")}</dt><dd className="num">{invoice.exchangeRate}</dd></>
                )}
                <dt>{t("detail.details.attachments")}</dt>
                <dd>
                  {invoice.xmlPath || invoice.pdfPath ? (
                    <>
                      {invoice.xmlPath && <span className="doc">{invoice.fullNumber}.xml · UBL 2.1 CIUS-RO</span>}
                      {invoice.xmlPath && invoice.pdfPath && <br />}
                      {invoice.pdfPath && <span className="doc">{invoice.fullNumber}.pdf · PDF A4</span>}
                    </>
                  ) : (
                    t("detail.details.generateXmlFirst")
                  )}
                </dd>
                {invoice.notes && (
                  <>
                    <dt>{t("detail.details.notes")}</dt>
                    <dd>
                      {invoice.notes.startsWith("STORNO_OF:")
                        ? invoice.notes.replace(/^STORNO_OF:[^|]*\|?/, "")
                        : invoice.notes}
                    </dd>
                  </>
                )}
              </dl>
            </div>
          </div>

          {/* plăți */}
          <div className="scr-card">
            <div className="scr-toolbar">
              <div className="tt">{t("detail.payments.title")}</div>
              <div className="spacer" />
              <span className={`chip ${payChip.cls}`}><Ic name={payChip.icon} cls="sic" />{payChip.label}</span>
            </div>
            {payments.length === 0 ? (
              <div style={{ padding: "22px 14px", textAlign: "center", fontSize: 12.5, color: "var(--text-2)" }}>
                {t("detail.payments.empty")}
              </div>
            ) : (
              payments.map((p) => (
                <div className="pay-row" key={p.id}>
                  <div className="act-ic" style={{ width: 28, height: 28 }}><Ic name="card" /></div>
                  <div>
                    <div className="p1">{METHOD_KEYS[p.method] ? t(METHOD_KEYS[p.method]) : p.method}</div>
                    <div className="p2 num">{fmtRoDate(p.paidAt)}{p.reference ? ` · ${t("detail.payments.ref", { ref: p.reference })}` : ""}</div>
                  </div>
                  <span className="amt num">{fmtRON(p.amount)}</span>
                  <button
                    className="mini-btn"
                    title={t("detail.payments.deleteTitle")}
                    disabled={deletePayment.isPending}
                    onClick={() => deletePayment.mutate(p.id)}
                  >
                    <Ic name="xMark" />
                  </button>
                </div>
              ))
            )}
            <div className="sold-line">
              <span>{t("detail.payments.remainingToCollect")}</span>
              <b className="num">{fmtRON(remaining)} {invoice.currency}</b>
            </div>
          </div>
        </div>
      </div>

      {/* modal storno */}
      {showStornoModal && (
        <div
          className={`modal-back ${stornoClosing ? "closing" : "show"}`}
          style={{ position: "fixed" }}
          onMouseDown={(e) => { if (e.target === e.currentTarget) closeStorno(); }}
        >
          <div className="modal">
            <div className="modal-head">
              <div>
                <div className="mt" style={{ color: "var(--red)" }}>{t("detail.stornoModal.title")}</div>
                <div className="ms num">{invoice.fullNumber} · {t("detail.stornoModal.sub")}</div>
              </div>
              <button className="modal-x" onClick={() => closeStorno()}>
                <Ic name="xMark" />
              </button>
            </div>
            <div className="modal-body">
              <div className="field">
                <label>{t("detail.stornoModal.reasonLabel")} <span className="req">*</span></label>
                <textarea
                  className="input"
                  placeholder={t("detail.stornoModal.reasonPlaceholder")}
                  value={stornoReason}
                  onChange={(e) => setStornoReason(e.target.value)}
                  autoFocus
                />
              </div>
            </div>
            <div className="modal-foot">
              <span className="left">{t("detail.stornoModal.footNote")}</span>
              <button className="pill-btn" onClick={() => closeStorno()}>
                {t("detail.stornoModal.cancel")}
              </button>
              <button
                className="btn-dark"
                style={{ background: "var(--red)" }}
                disabled={!stornoReason.trim() || stornoInvoice.isPending}
                onClick={() => {
                  stornoInvoice.mutate(stornoReason.trim());
                  closeStorno();
                }}
              >
                {stornoInvoice.isPending ? t("detail.stornoModal.pending") : t("detail.stornoModal.confirm")}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* modal plată */}
      {showPayModal && (
        <div
          className={`modal-back ${payClosing ? "closing" : "show"}`}
          style={{ position: "fixed" }}
          onMouseDown={(e) => { if (e.target === e.currentTarget) closePay(); }}
        >
          <div className="modal">
            <div className="modal-head">
              <div>
                <div className="mt">{t("detail.actions.recordPayment")}</div>
                <div className="ms num">
                  {invoice.fullNumber} · {contact?.legalName ?? "—"} · {t("detail.payModal.balance", { amount: fmtRON(remaining), currency: invoice.currency })}
                </div>
              </div>
              <button className="modal-x" onClick={() => closePay()}>
                <Ic name="xMark" />
              </button>
            </div>
            <div className="modal-body">
              <div className="fgrid">
                <div className="field">
                  <label>{t("detail.payModal.amountLabel", { currency: invoice.currency })} <span className="req">*</span></label>
                  <input
                    className="input num"
                    type="number"
                    step="0.01"
                    min="0.01"
                    placeholder="0.00"
                    value={payForm.amount}
                    onChange={(e) => setPayForm((f) => ({ ...f, amount: e.target.value }))}
                    style={{ textAlign: "right" }}
                  />
                </div>
                <div className="field">
                  <label>{t("detail.payModal.dateLabel")}</label>
                  <input
                    className="input num"
                    type="date"
                    value={payForm.paidAt}
                    onChange={(e) => setPayForm((f) => ({ ...f, paidAt: e.target.value }))}
                  />
                </div>
                {isFx && (
                  <div className="field">
                    <label>{t("detail.payModal.fxLabel")}</label>
                    <input
                      className="input num"
                      type="number"
                      step="0.0001"
                      min="0"
                      placeholder={t("detail.payModal.fxPlaceholder")}
                      value={payForm.exchangeRate}
                      onChange={(e) => setPayForm((f) => ({ ...f, exchangeRate: e.target.value }))}
                      style={{ textAlign: "right" }}
                    />
                  </div>
                )}
                <div className="field">
                  <label>{t("detail.payModal.methodLabel")}</label>
                  <select
                    className="select"
                    value={payForm.method}
                    onChange={(e) => setPayForm((f) => ({ ...f, method: e.target.value }))}
                  >
                    {Object.entries(METHOD_KEYS).map(([v, k]) => (
                      <option key={v} value={v}>{t(k)}</option>
                    ))}
                  </select>
                </div>
                <div className="field">
                  <label>{t("detail.payModal.refLabel")}</label>
                  <input
                    className="input"
                    type="text"
                    placeholder={t("detail.payModal.refPlaceholder")}
                    value={payForm.reference}
                    onChange={(e) => setPayForm((f) => ({ ...f, reference: e.target.value }))}
                  />
                </div>
                <div className="field span2">
                  <label>{t("detail.payModal.noteLabel")}</label>
                  <textarea
                    className="input"
                    placeholder={t("detail.payModal.notePlaceholder")}
                    value={payForm.notes}
                    onChange={(e) => setPayForm((f) => ({ ...f, notes: e.target.value }))}
                  />
                </div>
              </div>
            </div>
            <div className="modal-foot">
              <span className="left">
                {Number.isFinite(payAmountNum) && payAmountNum > 0
                  ? payAmountNum >= remaining
                    ? t("detail.payModal.fullPays")
                    : t("detail.payModal.partialPays", { amount: fmtRON(remaining - payAmountNum), currency: invoice.currency })
                  : ""}
              </span>
              <button className="pill-btn" onClick={() => closePay()}>{t("detail.payModal.cancel")}</button>
              <button
                className="btn-dark"
                disabled={paySaveDisabled}
                onClick={() => {
                  const rate = parseFloat(payForm.exchangeRate);
                  addPayment.mutate({
                    invoiceId: id,
                    companyId: invoice.companyId,
                    amount: payForm.amount,
                    currency: invoice.currency,
                    paidAt: payForm.paidAt,
                    method: payForm.method,
                    reference: payForm.reference || undefined,
                    notes: payForm.notes || undefined,
                    exchangeRate: Number.isFinite(rate) && rate > 0 ? rate : undefined,
                  });
                }}
              >
                <Ic name="check" />{addPayment.isPending ? t("detail.payModal.saving") : t("detail.payModal.save")}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* modal șablon recurent — real feature the prototype lacks */}
      {showSaveAsTemplate && (
        <div
          className={`modal-back ${templateClosing ? "closing" : "show"}`}
          style={{ position: "fixed" }}
          onMouseDown={(e) => { if (e.target === e.currentTarget) closeTemplate(); }}
        >
          <div className="modal">
            <div className="modal-head">
              <div>
                <div className="mt">{t("detail.templateModal.title")}</div>
                <div className="ms num">{invoice.fullNumber} · {t("detail.templateModal.sub", { series: invoice.series, n: lines.length })}</div>
              </div>
              <button className="modal-x" onClick={() => closeTemplate()}>
                <Ic name="xMark" />
              </button>
            </div>
            <div className="modal-body">
              <div className="fgrid">
                <div className="field span2">
                  <label>{t("detail.templateModal.nameLabel")} <span className="req">*</span></label>
                  <input
                    className="input"
                    type="text"
                    value={templateName}
                    onChange={(e) => setTemplateName(e.target.value)}
                    autoFocus
                  />
                </div>
                <div className="field span2">
                  <label>{t("detail.templateModal.freqLabel")}</label>
                  <select
                    className="select"
                    value={templateFrequency}
                    onChange={(e) => setTemplateFrequency(e.target.value)}
                  >
                    <option value="monthly">{t("detail.templateModal.monthly")}</option>
                    <option value="quarterly">{t("detail.templateModal.quarterly")}</option>
                    <option value="annual">{t("detail.templateModal.annual")}</option>
                  </select>
                </div>
              </div>
            </div>
            <div className="modal-foot">
              <span className="left">{t("detail.templateModal.firstIssue")}</span>
              <button className="pill-btn" onClick={() => closeTemplate()}>{t("detail.stornoModal.cancel")}</button>
              <button
                className="btn-dark"
                disabled={saveAsTemplateMutation.isPending}
                onClick={handleSaveAsTemplate}
              >
                {saveAsTemplateMutation.isPending ? t("detail.payModal.saving") : t("detail.templateModal.create")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
