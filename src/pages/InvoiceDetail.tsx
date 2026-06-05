/**
 * Detaliu factură emisă — re-skinned to rf kit (Wave 2).
 * Preserves ALL wiring: api.invoices.get, api.companies.get, api.contacts.get,
 * api.payments.summary, api.anaf.*, api.ubl.*, api.invoices.duplicate/storno,
 * api.recurring.create, api.integrations.smartbillPush, openUrl mailto,
 * writeText clipboard, window.print. Both modals preserved (storno, template).
 * Right panel layout: Actions → Payments → Status ANAF → Events → Attachments.
 */

import { useState, useEffect } from "react";
import { useParams, useNavigate } from "@tanstack/react-router";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openPath, openUrl } from "@tauri-apps/plugin-opener";

import { Icon } from "@/components/shared/Icon";
import { StatusBadge } from "@/components/shared/StatusBadge";
import { notify } from "@/lib/toasts";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { fmtRON, parseDec } from "@/lib/utils";
import { formatError } from "@/lib/error-mapper";
import type { AppErrorPayload } from "@/types";
import { useAppStore } from "@/lib/store";
import { fmtShortcut } from "@/lib/platform";
import { efacturaDeadline, deadlineDaysLeft, formatDeadline } from "@/lib/efacturaDeadline";
import {
  PageHeader, Btn, SectionCard, Card, Modal, Banner,
} from "@/components/rf";

export function InvoiceDetailPage() {
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

  // Save-as-recurring-template state
  const [showSaveAsTemplate, setShowSaveAsTemplate] = useState(false);
  const [templateFrequency, setTemplateFrequency] = useState("monthly");
  const [templateName, setTemplateName] = useState("");

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
      if (!activeCompanyId) return Promise.reject(new Error("Nicio companie activă."));
      return api.ubl.generateXml(id, activeCompanyId);
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.detail(id) });
      setActionError(null);
    },
    onError: (e) => setActionError((e as unknown as AppErrorPayload).message ?? "Eroare generare XML."),
  });

  const generatePdf = useMutation({
    mutationFn: () => {
      if (!activeCompanyId) return Promise.reject(new Error("Nicio companie activă."));
      return api.ubl.generatePdf(id, activeCompanyId);
    },
    onSuccess: async (pdfPath) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.detail(id) });
      setActionError(null);
      notify.success("PDF generat.");
      if (pdfPath) {
        try { await openPath(pdfPath); }
        catch (e) { notify.error(`Nu pot deschide PDF: ${e}`); }
      }
    },
    onError: (e) => setActionError((e as unknown as AppErrorPayload).message ?? "Eroare generare PDF."),
  });

  const authorizeAnaf = useMutation({
    mutationFn: () => api.anaf.authorize(data!.invoice.companyId),
    onSuccess: () => { void refetchAnafAuth(); },
    onError: (e) => setActionError((e as unknown as AppErrorPayload).message ?? "Eroare autorizare ANAF."),
  });

  const submitInvoice = useMutation({
    mutationFn: async () => {
      const companyId = data!.invoice.companyId;
      let authenticated = isAnafAuth;
      if (!authenticated) {
        await api.anaf.authorize(companyId);
        authenticated = await api.anaf.isAuthenticated(companyId);
        if (!authenticated) throw new Error("Autorizarea ANAF a eșuat sau a fost anulată.");
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
    onError: (e) => setActionError((e as unknown as AppErrorPayload).message ?? "Eroare trimitere ANAF."),
  });

  const checkStatus = useMutation({
    mutationFn: () => api.anaf.checkStatus(data!.invoice.companyId, id, testMode),
    onSuccess: (stare) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.detail(id) });
      setActionError(null);
      setStatusMessage(`Status ANAF: ${stare}`);
    },
    onError: (e) => setActionError((e as unknown as AppErrorPayload).message ?? "Eroare verificare status."),
  });

  const duplicateInvoice = useMutation({
    mutationFn: () => {
      if (!activeCompanyId) return Promise.reject(new Error("Nicio companie activă."));
      return api.invoices.duplicate(id, activeCompanyId);
    },
    onSuccess: (newId) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
      notify.success("Factură duplicată.");
      void navigate({ to: "/invoices/$id", params: { id: newId } });
    },
    onError: (e) => setActionError((e as unknown as AppErrorPayload).message ?? "Eroare duplicare."),
  });

  const stornoInvoice = useMutation({
    mutationFn: (reason: string) => {
      if (!activeCompanyId) return Promise.reject(new Error("Nicio companie activă."));
      return api.invoices.storno(id, activeCompanyId, reason);
    },
    onSuccess: (stornoInv) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.detail(id) });
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
      setStatusMessage(`Factură storno creată: ${stornoInv.fullNumber}`);
      setActionError(null);
      // Navigate to the new credit note so the accountant sees the guidance banner
      // and can immediately generate XML + submit to ANAF to complete the cancellation.
      void navigate({ to: "/invoices/$id", params: { id: stornoInv.id } });
    },
    onError: (e) => setActionError((e as unknown as AppErrorPayload).message ?? "Eroare stornare."),
  });

  const pushSmartbill = useMutation({
    mutationFn: () => api.integrations.smartbillPush(data!.invoice.companyId, id),
    onSuccess: (result) => {
      setStatusMessage(`Factură trimisă în SmartBill: ${result}`);
      setActionError(null);
    },
    onError: (e) => setActionError((e as unknown as AppErrorPayload).message ?? "Eroare trimitere SmartBill."),
  });

  const saveAsTemplateMutation = useMutation({
    mutationFn: (args: Parameters<typeof api.recurring.create>[0]) => api.recurring.create(args),
    onSuccess: () => {
      notify.success("Șablon recurent creat din factură.");
      setShowSaveAsTemplate(false);
      setTemplateName("");
      setTemplateFrequency("monthly");
    },
    onError: (e) => setActionError(formatError(e, "Nu s-a putut crea șablonul recurent.")),
  });

  function nextMonthDate(): string {
    const d = new Date();
    const next = new Date(d.getFullYear(), d.getMonth() + 1, d.getDate());
    return `${next.getFullYear()}-${String(next.getMonth() + 1).padStart(2, "0")}-${String(next.getDate()).padStart(2, "0")}`;
  }

  function handleSaveAsTemplate() {
    if (!data) return;
    const { invoice, lines: invoiceLines } = data;
    if (!templateName.trim()) { notify.warn("Introduceți un nume pentru șablon."); return; }
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

  if (isLoading) {
    return (
      <div style={{ padding: 32, fontSize: 13, color: "var(--rf-text-muted)" }}>Se încarcă…</div>
    );
  }

  if (!data) {
    return (
      <div style={{ padding: 32, fontSize: 13, color: "var(--rf-text-muted)" }}>
        Factura nu a fost găsită.
      </div>
    );
  }

  const { invoice, lines, events } = data;

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", background: "var(--rf-app-bg)" }}>
      {/* Page header */}
      <PageHeader
        screen="Facturi emise › Detaliu"
        title={
          <span style={{ display: "flex", alignItems: "center", gap: 10 }}>
            {invoice.fullNumber}
            <StatusBadge status={invoice.status} />
          </span>
        }
        actions={
          <>
            <Btn variant="ghost" icon="chevLeft" onClick={() => navigate({ to: "/invoices" })}>
              Înapoi
            </Btn>
            {invoice.status === "DRAFT" && (
              <Btn
                variant="secondary"
                icon="pen"
                onClick={() => navigate({ to: "/invoices/$id/edit", params: { id } })}
              >
                Editează
              </Btn>
            )}
            <Btn
              variant="secondary"
              icon="copy"
              onClick={() => duplicateInvoice.mutate()}
              disabled={duplicateInvoice.isPending}
            >
              {duplicateInvoice.isPending ? "Duplicare…" : "Duplicare"}
            </Btn>
            {invoice.status === "VALIDATED" && invoice.stornoOfInvoiceId === null && (
              <Btn
                variant="secondary"
                icon="storno"
                disabled={stornoInvoice.isPending}
                onClick={() => setShowStornoModal(true)}
              >
                Storno
              </Btn>
            )}
            <Btn
              variant="secondary"
              icon="printer"
              onClick={() => window.print()}
              title={`Tipărește (${fmtShortcut("Ctrl+P")})`}
            >
              Tipărește
            </Btn>
            <Btn
              variant="secondary"
              icon="refresh"
              onClick={() => {
                setTemplateName(`Șablon din ${invoice.fullNumber}`);
                setTemplateFrequency("monthly");
                setShowSaveAsTemplate(true);
              }}
            >
              Șablon recurent
            </Btn>
          </>
        }
      />

      {/* Error / status banners */}
      {actionError && (
        <div
          style={{
            margin: "0 32px 4px",
            padding: "8px 12px",
            background: "var(--rf-error-bg)",
            border: "1px solid var(--rf-error-bd)",
            borderRadius: 8,
            fontSize: 12,
            color: "var(--rf-error)",
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
          }}
        >
          <span>{actionError}</span>
          <button
            type="button"
            style={{ border: "none", background: "none", color: "var(--rf-error)", cursor: "pointer" }}
            onClick={() => setActionError(null)}
          >
            ✕
          </button>
        </div>
      )}
      {statusMessage && !actionError && (
        <div
          style={{
            margin: "0 32px 4px",
            padding: "8px 12px",
            background: "var(--rf-success-bg)",
            border: "1px solid var(--rf-success-bd)",
            borderRadius: 8,
            fontSize: 12,
            color: "var(--rf-success)",
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
          }}
        >
          <span>{statusMessage}</span>
          <button
            type="button"
            style={{ border: "none", background: "none", color: "var(--rf-success)", cursor: "pointer" }}
            onClick={() => setStatusMessage(null)}
          >
            ✕
          </button>
        </div>
      )}

      {/* REG-STORNO: sticky guidance banner for DRAFT credit notes.
          A storno credit note that is still DRAFT has NOT been sent to ANAF yet
          and therefore has NOT fiscally cancelled the original invoice. The accountant
          must generate the XML and submit it to complete the cancellation. */}
      {invoice.status === "DRAFT" && invoice.stornoOfInvoiceId !== null && (
        <div style={{ margin: "0 32px 8px" }}>
          <Banner variant="warning" title="Factură storno — acțiune necesară">
            Această factură storno trebuie generată (XML) și trimisă la ANAF pentru a
            anula fiscal factura originală. Până la validarea de către ANAF, anularea
            fiscală <strong>nu este efectivă</strong> și factura originală continuă să
            apară în declarații.
          </Banner>
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
            <div style={{ margin: "0 32px 8px" }}>
              <Banner variant={overdue ? "error" : left <= 1 ? "warning" : "info"}>
                e-Factura: de trimis până la <strong>{formatDeadline(dl)}</strong>{" "}
                {overdue
                  ? `— termen depășit cu ${n} ${n === 1 ? "zi" : "zile"}.`
                  : `(${left} ${left === 1 ? "zi" : "zile"} rămase). Termen legal: 5 zile lucrătoare de la emitere.`}
              </Banner>
            </div>
          );
        })()}

      {/* Main split layout */}
      <div
        style={{
          flex: 1,
          overflow: "auto",
          display: "grid",
          gridTemplateColumns: "1fr 340px",
          gap: 20,
          padding: "0 32px 32px",
          alignItems: "start",
        }}
      >
        {/* LEFT — invoice document preview */}
        <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
          <Card pad>
            {/* Header */}
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start", marginBottom: 20 }}>
              <div>
                <h1 style={{ fontSize: 20, fontWeight: 700, margin: "0 0 4px" }}>Factură fiscală</h1>
                <div style={{ fontSize: 12, color: "var(--rf-text-dim)" }}>
                  {invoice.fullNumber} · {invoice.issueDate}
                </div>
              </div>
              <div style={{ textAlign: "right" }}>
                {invoice.status === "VALIDATED" && (
                  <div
                    style={{
                      background: "var(--rf-success-bg)",
                      color: "var(--rf-success)",
                      border: "1px solid var(--rf-success-bd)",
                      borderRadius: 6,
                      padding: "4px 12px",
                      fontSize: 11,
                      fontWeight: 700,
                      letterSpacing: "0.05em",
                    }}
                  >
                    e-Factura · validată
                  </div>
                )}
                {invoice.anafIndex && (
                  <div style={{ fontSize: 11, color: "var(--rf-text-dim)", marginTop: 4 }}>
                    Index ANAF:{" "}
                    <span style={{ fontFamily: "var(--rf-mono)" }}>{invoice.anafIndex}</span>
                  </div>
                )}
              </div>
            </div>

            {/* Parties */}
            <div
              style={{
                display: "grid",
                gridTemplateColumns: "1fr 1fr",
                gap: 24,
                padding: "16px 0",
                borderTop: "1px solid var(--rf-border)",
                borderBottom: "1px solid var(--rf-border)",
                marginBottom: 16,
              }}
            >
              <div>
                <div style={{ fontSize: 11, fontWeight: 600, textTransform: "uppercase", letterSpacing: "0.07em", color: "var(--rf-text-dim)", marginBottom: 6 }}>
                  Furnizor
                </div>
                {company ? (
                  <>
                    <div style={{ fontWeight: 600 }}>{company.legalName}</div>
                    <div style={{ fontSize: 12, color: "var(--rf-text-muted)", fontFamily: "var(--rf-mono)" }}>
                      CUI: {company.cui}
                    </div>
                    <div style={{ fontSize: 12, color: "var(--rf-text-muted)" }}>
                      {[company.address, company.city, company.county].filter(Boolean).join(", ")}
                    </div>
                  </>
                ) : (
                  <div style={{ fontSize: 12, color: "var(--rf-text-dim)" }}>ID: {invoice.companyId}</div>
                )}
              </div>
              <div style={{ textAlign: "right" }}>
                <div style={{ fontSize: 11, fontWeight: 600, textTransform: "uppercase", letterSpacing: "0.07em", color: "var(--rf-text-dim)", marginBottom: 6 }}>
                  Cumpărător
                </div>
                {contact ? (
                  <>
                    <div style={{ fontWeight: 600 }}>{contact.legalName}</div>
                    {contact.cui && (
                      <div style={{ fontSize: 12, color: "var(--rf-text-muted)", fontFamily: "var(--rf-mono)" }}>
                        CUI: {contact.cui}
                      </div>
                    )}
                    <div style={{ fontSize: 12, color: "var(--rf-text-muted)" }}>
                      {[contact.address, contact.city, contact.county].filter(Boolean).join(", ")}
                    </div>
                  </>
                ) : (
                  <div style={{ fontSize: 12, color: "var(--rf-text-dim)" }}>ID: {invoice.contactId}</div>
                )}
              </div>
            </div>

            {/* Meta row */}
            <div style={{ display: "flex", gap: 24, flexWrap: "wrap", marginBottom: 16 }}>
              {[
                ["Data emiterii", invoice.issueDate],
                ["Data scadenței", invoice.dueDate],
                ["Monedă", invoice.currency],
              ].map(([k, v]) => (
                <div key={k}>
                  <div style={{ fontSize: 11, color: "var(--rf-text-dim)" }}>{k}</div>
                  <div style={{ fontSize: 13, fontWeight: 500, marginTop: 2 }}>{v}</div>
                </div>
              ))}
              {invoice.exchangeRate && (
                <div>
                  <div style={{ fontSize: 11, color: "var(--rf-text-dim)" }}>Curs valutar</div>
                  <div style={{ fontSize: 13, fontWeight: 500, marginTop: 2, fontFamily: "var(--rf-mono)" }}>
                    {invoice.exchangeRate}
                  </div>
                </div>
              )}
            </div>

            {/* Line items */}
            {lines.length > 0 && (
              <table className="rf-tbl" style={{ marginBottom: 16 }}>
                <thead>
                  <tr>
                    <th style={{ width: 24 }}>#</th>
                    <th>Descriere</th>
                    <th style={{ width: 60 }} className="right">UM</th>
                    <th style={{ width: 60 }} className="right">Cant.</th>
                    <th style={{ width: 90 }} className="right">Preț</th>
                    <th style={{ width: 50 }} className="right">TVA</th>
                    <th style={{ width: 100 }} className="right">Valoare</th>
                  </tr>
                </thead>
                <tbody>
                  {lines.map((l, i) => (
                    <tr key={l.id}>
                      <td style={{ color: "var(--rf-text-dim)" }}>{i + 1}</td>
                      <td>
                        <div style={{ fontWeight: 600 }}>{l.name}</div>
                        {l.description && (
                          <div style={{ fontSize: 11, color: "var(--rf-text-muted)" }}>{l.description}</div>
                        )}
                      </td>
                      <td className="right" style={{ fontFamily: "var(--rf-mono)" }}>{l.unit}</td>
                      <td className="right" style={{ fontFamily: "var(--rf-mono)" }}>{l.quantity}</td>
                      <td className="right" style={{ fontFamily: "var(--rf-mono)" }}>{fmtRON(l.unitPrice)}</td>
                      <td className="right" style={{ fontFamily: "var(--rf-mono)" }}>{l.vatRate}%</td>
                      <td className="right" style={{ fontFamily: "var(--rf-mono)", fontWeight: 700 }}>
                        {fmtRON(l.subtotalAmount)}
                      </td>
                    </tr>
                  ))}
                </tbody>
                <tfoot>
                  <tr>
                    <td colSpan={6} className="right" style={{ fontWeight: 400, fontSize: 12 }}>Total net</td>
                    <td className="right" style={{ fontFamily: "var(--rf-mono)" }}>{fmtRON(invoice.subtotalAmount)} {invoice.currency}</td>
                  </tr>
                  <tr>
                    <td colSpan={6} className="right" style={{ fontWeight: 400, fontSize: 12 }}>Total TVA</td>
                    <td className="right" style={{ fontFamily: "var(--rf-mono)" }}>{fmtRON(invoice.vatAmount)} {invoice.currency}</td>
                  </tr>
                  <tr>
                    <td colSpan={6} className="right">Total de plată</td>
                    <td className="right" style={{ fontFamily: "var(--rf-mono)", color: "var(--rf-accent)", fontSize: 16, fontWeight: 700 }}>
                      {fmtRON(invoice.totalAmount)} {invoice.currency}
                    </td>
                  </tr>
                </tfoot>
              </table>
            )}

            {invoice.notes && (
              <div
                style={{
                  marginTop: 12,
                  fontSize: 12,
                  color: "var(--rf-text-muted)",
                  borderTop: "1px solid var(--rf-border)",
                  paddingTop: 10,
                }}
              >
                {invoice.notes.startsWith("STORNO_OF:")
                  ? invoice.notes.replace(/^STORNO_OF:[^|]*\|?/, "")
                  : invoice.notes}
              </div>
            )}
          </Card>
        </div>

        {/* RIGHT — actions + meta cards */}
        <div style={{ display: "flex", flexDirection: "column", gap: 16, position: "sticky", top: 0 }}>

          {/* Actions card */}
          <SectionCard icon="send" title="Acțiuni">
            <div style={{ padding: "0 16px 16px", display: "flex", flexDirection: "column", gap: 8 }}>
              {/* Generate/Regenerate XML */}
              <Btn
                variant={!invoice.xmlPath ? "primary" : "secondary"}
                icon={invoice.xmlPath ? "refresh" : "file"}
                block
                disabled={generateXml.isPending}
                onClick={() => generateXml.mutate()}
              >
                {generateXml.isPending ? "Generare…" : invoice.xmlPath ? "Regenerează XML" : "Generează XML UBL"}
              </Btn>

              {/* Generate/Regenerate PDF */}
              <Btn
                variant="secondary"
                icon={invoice.pdfPath ? "refresh" : "file"}
                block
                disabled={generatePdf.isPending}
                onClick={() => generatePdf.mutate()}
              >
                {generatePdf.isPending ? "Generare…" : invoice.pdfPath ? "Regenerează PDF" : "Generează PDF"}
              </Btn>

              {/* Download PDF */}
              {invoice.pdfPath && (
                <Btn
                  variant="secondary"
                  icon="download"
                  block
                  disabled={generatePdf.isPending}
                  onClick={() => generatePdf.mutate()}
                >
                  Descarcă PDF
                </Btn>
              )}

              {/* Send to ANAF */}
              {invoice.xmlPath && invoice.status === "DRAFT" && (
                <Btn
                  variant={invoice.anafUploadId || invoice.anafIndex ? "secondary" : "primary"}
                  icon="cloudUp"
                  block
                  disabled={submitInvoice.isPending || authorizeAnaf.isPending}
                  onClick={() => submitInvoice.mutate()}
                >
                  {authorizeAnaf.isPending ? "Autorizare ANAF…" : submitInvoice.isPending ? "Trimitere…" : "Trimite la ANAF"}
                </Btn>
              )}

              {/* Check ANAF status */}
              {(invoice.status === "SUBMITTED" || !!invoice.anafIndex) && (
                <Btn
                  variant="secondary"
                  icon="refresh"
                  block
                  disabled={checkStatus.isPending}
                  onClick={() => checkStatus.mutate()}
                >
                  {checkStatus.isPending ? "Verificare…" : "Verifică status ANAF"}
                </Btn>
              )}

              {/* SmartBill */}
              <Btn
                variant="secondary"
                icon="cloudUp"
                block
                disabled={pushSmartbill.isPending}
                onClick={() => pushSmartbill.mutate()}
              >
                {pushSmartbill.isPending ? "SmartBill…" : "SmartBill"}
              </Btn>

              {/* Email */}
              {contact?.email && (
                <Btn
                  variant="secondary"
                  icon="mail"
                  block
                  onClick={() => {
                    const subject = encodeURIComponent(`Factură ${invoice.fullNumber}`);
                    const body = encodeURIComponent(
                      `Bună ziua,\n\nVă transmitem factura ${invoice.fullNumber} din data ${invoice.issueDate}, în valoare de ${fmtRON(invoice.totalAmount)} ${invoice.currency}.\n\nCu stimă`
                    );
                    void openUrl(`mailto:${encodeURIComponent(contact.email ?? "")}?subject=${subject}&body=${body}`);
                  }}
                >
                  Email
                </Btn>
              )}

              {/* Copy XML */}
              {invoice.xmlPath && (
                <Btn
                  variant="secondary"
                  icon="copy"
                  block
                  onClick={async () => {
                    try {
                      const { readTextFile } = await import("@tauri-apps/plugin-fs");
                      const content = await readTextFile(invoice.xmlPath!);
                      await writeText(content);
                      setXmlCopied(true);
                      setTimeout(() => setXmlCopied(false), 2000);
                    } catch {
                      setActionError("Nu s-a putut copia XML-ul în clipboard.");
                    }
                  }}
                >
                  {xmlCopied ? "Copiat ✓" : "Copiază XML"}
                </Btn>
              )}

              {/* ANAF auth indicator */}
              <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 12, paddingTop: 4 }}>
                {isAnafAuth ? (
                  <span style={{ color: "var(--rf-success)", display: "flex", alignItems: "center", gap: 4 }}>
                    <Icon name="check" size={12} /> Autentificat ANAF
                  </span>
                ) : (
                  <span style={{ color: "var(--rf-text-muted)", display: "flex", alignItems: "center", gap: 4 }}>
                    <Icon name="warning" size={12} /> Neautentificat ANAF
                    {data?.invoice.companyId && (
                      <button
                        type="button"
                        style={{ background: "none", border: "none", color: "var(--rf-accent)", cursor: "pointer", textDecoration: "underline", fontSize: 12, padding: 0 }}
                        disabled={authorizeAnaf.isPending}
                        onClick={() => authorizeAnaf.mutate()}
                      >
                        {authorizeAnaf.isPending ? "Se autorizează…" : "Autorizează"}
                      </button>
                    )}
                  </span>
                )}
              </div>
            </div>
          </SectionCard>

          {/* Payments card */}
          <SectionCard icon="bank" title="Plăți">
            <div style={{ padding: "0 16px 16px" }}>
              {(() => {
                const total = parseDec(invoice.totalAmount);
                const paid = parseDec(paymentSummary?.paidAmount ?? "0");
                const remaining = total - paid;
                const pStatus = paymentSummary?.paymentStatus ?? "UNPAID";
                return (
                  <>
                    <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 8 }}>
                      <StatusBadge status={pStatus} />
                      <span style={{ fontFamily: "var(--rf-mono)", fontSize: 12 }}>
                        {fmtRON(paid)} / {fmtRON(total)} {invoice.currency}
                      </span>
                    </div>
                    {/* Progress bar */}
                    <div style={{ height: 6, background: "var(--rf-neutral-bg)", borderRadius: 999, marginBottom: 12 }}>
                      <div
                        style={{
                          width: `${total > 0 ? Math.min(100, (paid / total) * 100) : 0}%`,
                          height: "100%",
                          background: remaining <= 0 ? "var(--rf-success)" : "var(--rf-warning)",
                          borderRadius: 999,
                          transition: "width .3s",
                        }}
                      />
                    </div>
                    <dl
                      style={{
                        display: "grid",
                        gridTemplateColumns: "1fr auto",
                        rowGap: 4,
                        fontSize: 12,
                        marginBottom: 12,
                      }}
                    >
                      <dt style={{ color: "var(--rf-text-muted)" }}>Total factură</dt>
                      <dd style={{ fontFamily: "var(--rf-mono)", textAlign: "right", margin: 0 }}>
                        {fmtRON(total)} {invoice.currency}
                      </dd>
                      <dt style={{ color: "var(--rf-text-muted)" }}>Total plătit</dt>
                      <dd style={{ fontFamily: "var(--rf-mono)", textAlign: "right", margin: 0 }}>
                        {fmtRON(paid)} {invoice.currency}
                      </dd>
                      <dt style={{ color: "var(--rf-text-muted)" }}>Rest de plată</dt>
                      <dd
                        style={{
                          fontFamily: "var(--rf-mono)", textAlign: "right", margin: 0,
                          fontWeight: 700, color: remaining > 0 ? "var(--rf-error)" : "var(--rf-success)",
                        }}
                      >
                        {fmtRON(remaining)} {invoice.currency}
                      </dd>
                    </dl>
                    {paymentSummary && paymentSummary.payments.length > 0 && (
                      <div style={{ display: "flex", flexDirection: "column", gap: 4, marginBottom: 10 }}>
                        {paymentSummary.payments.map((p) => (
                          <div
                            key={p.id}
                            style={{
                              display: "flex",
                              alignItems: "center",
                              gap: 8,
                              padding: "6px 8px",
                              border: "1px solid var(--rf-border)",
                              borderRadius: 6,
                              fontSize: 12,
                            }}
                          >
                            <span style={{ flex: 1 }}>
                              <b style={{ fontFamily: "var(--rf-mono)" }}>{fmtRON(p.amount)} {p.currency}</b>
                              <span style={{ color: "var(--rf-text-muted)" }}> · {p.paidAt}</span>
                            </span>
                            <span style={{ fontSize: 11, color: "var(--rf-text-dim)" }}>
                              {p.method}{p.reference ? ` · ${p.reference}` : ""}
                            </span>
                          </div>
                        ))}
                      </div>
                    )}
                    <button
                      type="button"
                      className="rf-btn rf-btn--secondary rf-btn--sm rf-btn--block"
                      onClick={() => navigate({ to: "/payments" })}
                    >
                      <Icon name="plus" size={13} /> Adaugă plată
                    </button>
                  </>
                );
              })()}
            </div>
          </SectionCard>

          {/* Status ANAF card */}
          <SectionCard icon="anaf" title="Status ANAF">
            <div style={{ padding: "0 16px 16px" }}>
              <div style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 10 }}>
                <StatusBadge status={invoice.status} />
                {invoice.anafIndex && (
                  <span style={{ fontFamily: "var(--rf-mono)", color: "var(--rf-text-dim)", fontSize: 11 }}>
                    {invoice.anafIndex}
                  </span>
                )}
              </div>
              <dl
                style={{
                  display: "grid",
                  gridTemplateColumns: "auto 1fr",
                  columnGap: 12,
                  rowGap: 4,
                  fontSize: 12,
                }}
              >
                {invoice.anafSubmittedAt && (
                  <>
                    <dt style={{ color: "var(--rf-text-muted)" }}>Trimisă la</dt>
                    <dd style={{ fontFamily: "var(--rf-mono)", margin: 0 }}>
                      {new Date(invoice.anafSubmittedAt * 1000).toLocaleString("ro-RO")}
                    </dd>
                  </>
                )}
                {invoice.anafValidatedAt && (
                  <>
                    <dt style={{ color: "var(--rf-text-muted)" }}>Validată la</dt>
                    <dd style={{ fontFamily: "var(--rf-mono)", margin: 0 }}>
                      {new Date(invoice.anafValidatedAt * 1000).toLocaleString("ro-RO")}
                    </dd>
                  </>
                )}
                {invoice.anafRejectedAt && (
                  <>
                    <dt style={{ color: "var(--rf-text-muted)" }}>Respinsă la</dt>
                    <dd style={{ fontFamily: "var(--rf-mono)", color: "var(--rf-error)", margin: 0 }}>
                      {new Date(invoice.anafRejectedAt * 1000).toLocaleString("ro-RO")}
                    </dd>
                  </>
                )}
                {invoice.rejectionReason && (
                  <>
                    <dt style={{ color: "var(--rf-text-muted)" }}>Motiv respingere</dt>
                    <dd style={{ color: "var(--rf-error)", fontSize: 11, margin: 0 }}>{invoice.rejectionReason}</dd>
                  </>
                )}
              </dl>
            </div>
          </SectionCard>

          {/* Events timeline */}
          {events.length > 0 && (
            <SectionCard icon="clock" title="Evenimente · jurnal">
              <div style={{ padding: "4px 0 8px" }}>
                {events.map((e, i) => (
                  <div key={e.id} style={{ display: "flex", gap: 12, padding: "8px 16px" }}>
                    <div style={{ display: "flex", flexDirection: "column", alignItems: "center" }}>
                      <span
                        style={{
                          width: 24, height: 24, borderRadius: "50%", display: "grid", placeItems: "center",
                          background: "var(--rf-info-bg)", color: "var(--rf-info)", flexShrink: 0,
                        }}
                      >
                        <Icon name="file" size={12} />
                      </span>
                      {i < events.length - 1 && (
                        <span style={{ width: 1.5, flex: 1, background: "var(--rf-border)", marginTop: 3 }} />
                      )}
                    </div>
                    <div style={{ paddingBottom: 4 }}>
                      <div style={{ fontSize: 12.5, lineHeight: 1.4 }}>{e.message}</div>
                      <div style={{ fontSize: 11, color: "var(--rf-text-dim)", marginTop: 1 }}>
                        {new Date(e.createdAt * 1000).toLocaleString("ro-RO")}
                        <span style={{ color: "var(--rf-text-dim)", marginLeft: 6 }}>{e.eventType}</span>
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            </SectionCard>
          )}

          {/* Attachments */}
          <SectionCard icon="file" title="Atașamente">
            <div style={{ padding: "0 16px 16px", display: "flex", flexDirection: "column", gap: 6 }}>
              {invoice.xmlPath ? (
                <div
                  style={{
                    display: "flex", alignItems: "center", gap: 8, padding: "7px 10px",
                    border: "1px solid var(--rf-border)", borderRadius: 8, fontSize: 12,
                  }}
                >
                  <Icon name="file" size={14} style={{ color: "var(--rf-text-muted)" }} />
                  <span style={{ flex: 1 }}>{invoice.fullNumber}.xml</span>
                  <span style={{ fontSize: 11, color: "var(--rf-text-dim)" }}>UBL 2.1 CIUS-RO</span>
                </div>
              ) : null}
              {invoice.pdfPath ? (
                <div
                  style={{
                    display: "flex", alignItems: "center", gap: 8, padding: "7px 10px",
                    border: "1px solid var(--rf-border)", borderRadius: 8, fontSize: 12,
                  }}
                >
                  <Icon name="file" size={14} style={{ color: "var(--rf-text-muted)" }} />
                  <span style={{ flex: 1 }}>{invoice.fullNumber}.pdf</span>
                  <span style={{ fontSize: 11, color: "var(--rf-text-dim)" }}>PDF A4</span>
                </div>
              ) : null}
              {!invoice.xmlPath && !invoice.pdfPath && (
                <span style={{ fontSize: 12, color: "var(--rf-text-dim)" }}>
                  Niciun atașament. Generați XML-ul mai întâi.
                </span>
              )}
            </div>
          </SectionCard>

          {/* Metadate */}
          <SectionCard icon="info" title="Metadate factură">
            <div style={{ padding: "0 16px 16px" }}>
              <dl
                style={{
                  display: "grid",
                  gridTemplateColumns: "auto 1fr",
                  columnGap: 16,
                  rowGap: 4,
                  fontSize: 12,
                }}
              >
                <dt style={{ color: "var(--rf-text-muted)" }}>Număr</dt>
                <dd style={{ fontFamily: "var(--rf-mono)", fontWeight: 700, margin: 0 }}>{invoice.fullNumber}</dd>
                <dt style={{ color: "var(--rf-text-muted)" }}>Serie</dt>
                <dd style={{ fontFamily: "var(--rf-mono)", margin: 0 }}>{invoice.series}</dd>
                <dt style={{ color: "var(--rf-text-muted)" }}>Data emiterii</dt>
                <dd style={{ margin: 0 }}>{invoice.issueDate}</dd>
                <dt style={{ color: "var(--rf-text-muted)" }}>Data scadenței</dt>
                <dd style={{ margin: 0 }}>{invoice.dueDate}</dd>
                <dt style={{ color: "var(--rf-text-muted)" }}>Monedă</dt>
                <dd style={{ fontFamily: "var(--rf-mono)", margin: 0 }}>{invoice.currency}</dd>
                {invoice.exchangeRate && (
                  <>
                    <dt style={{ color: "var(--rf-text-muted)" }}>Curs valutar</dt>
                    <dd style={{ fontFamily: "var(--rf-mono)", margin: 0 }}>{invoice.exchangeRate}</dd>
                  </>
                )}
              </dl>
            </div>
          </SectionCard>

        </div>
      </div>

      {/* Save-as-recurring-template modal (rf Modal) */}
      <Modal
        open={showSaveAsTemplate}
        onOpenChange={(open) => { if (!open) setShowSaveAsTemplate(false); }}
        title="Salvează ca șablon recurent"
        footer={
          <>
            <button className="rf-btn rf-btn--secondary" onClick={() => setShowSaveAsTemplate(false)}>
              Anulează
            </button>
            <button
              className="rf-btn rf-btn--primary"
              disabled={saveAsTemplateMutation.isPending}
              onClick={handleSaveAsTemplate}
            >
              {saveAsTemplateMutation.isPending ? "Se salvează…" : "Creează șablon"}
            </button>
          </>
        }
      >
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <div className="rf-field">
            <label>Nume șablon *</label>
            <input
              className="rf-input"
              value={templateName}
              onChange={(e) => setTemplateName(e.target.value)}
              autoFocus
            />
          </div>
          <div className="rf-field">
            <label>Frecvență</label>
            <div className="rf-select-wrap">
              <select
                className="rf-select"
                value={templateFrequency}
                onChange={(e) => setTemplateFrequency(e.target.value)}
              >
                <option value="monthly">Lunar</option>
                <option value="quarterly">Trimestrial</option>
                <option value="annual">Anual</option>
              </select>
              <Icon name="chevDown" size={14} className="rf-chev" />
            </div>
          </div>
          <div style={{ fontSize: 11.5, color: "var(--rf-text-muted)" }}>
            Prima emitere: luna viitoare · seria: {invoice.series} · {lines.length} articol(e) din factură
          </div>
        </div>
      </Modal>

      {/* Storno modal (rf Modal) */}
      <Modal
        open={showStornoModal}
        onOpenChange={(open) => { if (!open) { setShowStornoModal(false); setStornoReason(""); } }}
        title={`Stornare factură ${invoice.fullNumber}`}
        footer={
          <>
            <button
              className="rf-btn rf-btn--secondary"
              onClick={() => { setShowStornoModal(false); setStornoReason(""); }}
            >
              Anulează
            </button>
            <button
              className="rf-btn rf-btn--danger"
              disabled={!stornoReason.trim() || stornoInvoice.isPending}
              onClick={() => {
                stornoInvoice.mutate(stornoReason.trim());
                setShowStornoModal(false);
                setStornoReason("");
              }}
            >
              {stornoInvoice.isPending ? "Se stornează…" : "Stornează factura"}
            </button>
          </>
        }
      >
        <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
          <div style={{ fontSize: 13, color: "var(--rf-text-muted)" }}>
            Această acțiune marchează factura ca anulată. Introduceți motivul stornării:
          </div>
          <textarea
            value={stornoReason}
            onChange={(e) => setStornoReason(e.target.value)}
            placeholder="Ex: Eroare de preț, anulare comandă..."
            className="rf-textarea"
            style={{ minHeight: 72 }}
            autoFocus
          />
        </div>
      </Modal>
    </div>
  );
}
