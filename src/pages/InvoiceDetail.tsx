/**
 * Detaliu factură emisă — date REALE din backend (api.invoices.get),
 * cu vizualul Win32 portat din Claude Design.
 */

import { useState, useEffect } from "react";
import { useParams, useNavigate } from "@tanstack/react-router";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";

import { Icon } from "@/components/shared/Icon";
import { StatusBadge } from "@/components/shared/StatusBadge";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { fmtRON, parseDec } from "@/lib/utils";
import type { AppErrorPayload } from "@/types";
import { useAppStore } from "@/lib/store";

export function InvoiceDetailPage() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { id } = useParams({ from: "/invoices/$id" });
  const setSelectedInvoiceId = useAppStore((s) => s.setSelectedInvoiceId);

  useEffect(() => {
    setSelectedInvoiceId(id);
    return () => setSelectedInvoiceId(null);
  }, [id, setSelectedInvoiceId]);

  const [actionError, setActionError] = useState<string | null>(null);
  const [statusMessage, setStatusMessage] = useState<string | null>(null);
  const [showStornoModal, setShowStornoModal] = useState(false);
  const [stornoReason, setStornoReason] = useState("");
  const [xmlCopied, setXmlCopied] = useState(false);

  const { data, isLoading } = useQuery({
    queryKey: queryKeys.invoices.detail(id),
    queryFn: () => api.invoices.get(id),
  });

  const { data: company } = useQuery({
    queryKey: queryKeys.companies.detail(data?.invoice.companyId ?? ""),
    queryFn: () => api.companies.get(data!.invoice.companyId),
    enabled: !!data?.invoice.companyId,
  });

  const { data: contact } = useQuery({
    queryKey: queryKeys.contacts.detail(data?.invoice.contactId ?? ""),
    queryFn: () => api.contacts.get(data!.invoice.contactId),
    enabled: !!data?.invoice.contactId,
  });

  const { data: paymentSummary } = useQuery({
    queryKey: ["payments", "summary", id, data?.invoice.companyId ?? ""],
    queryFn: () => api.payments.summary(id, data!.invoice.companyId),
    enabled: !!data?.invoice.companyId,
  });

  // ANAF auth status
  const { data: isAnafAuth, refetch: refetchAnafAuth } = useQuery({
    queryKey: queryKeys.anaf.auth(data?.invoice.companyId ?? ""),
    queryFn: () => api.anaf.isAuthenticated(data!.invoice.companyId),
    enabled: !!data?.invoice.companyId,
  });

  // ANAF test mode setting — key must match backend: settings::keys::USE_ANAF_TEST_ENV
  const { data: testModeSetting } = useQuery({
    queryKey: queryKeys.anaf.testMode,
    queryFn: () => api.settings.get("use_anaf_test_env"),
  });

  const testMode = testModeSetting === "1";

  const generateXml = useMutation({
    mutationFn: () => api.ubl.generateXml(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.detail(id) });
      setActionError(null);
    },
    onError: (e) => setActionError((e as unknown as AppErrorPayload).message ?? "Eroare generare XML."),
  });

  const generatePdf = useMutation({
    mutationFn: () => api.ubl.generatePdf(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.detail(id) });
      setActionError(null);
    },
    onError: (e) => setActionError((e as unknown as AppErrorPayload).message ?? "Eroare generare PDF."),
  });

  // Authorize ANAF (standalone mutation used inside submitInvoice flow)
  const authorizeAnaf = useMutation({
    mutationFn: () => api.anaf.authorize(data!.invoice.companyId),
    onSuccess: () => {
      void refetchAnafAuth();
    },
    onError: (e) => setActionError((e as unknown as AppErrorPayload).message ?? "Eroare autorizare ANAF."),
  });

  // Submit invoice to ANAF (with auto-authorize if needed)
  const submitInvoice = useMutation({
    mutationFn: async () => {
      const companyId = data!.invoice.companyId;
      let authenticated = isAnafAuth;
      if (!authenticated) {
        await api.anaf.authorize(companyId);
        authenticated = await api.anaf.isAuthenticated(companyId);
        if (!authenticated) {
          throw new Error("Autorizarea ANAF a eșuat sau a fost anulată.");
        }
      }
      return api.anaf.submitInvoice(companyId, id, testMode);
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.detail(id) });
      void queryClient.invalidateQueries({ queryKey: queryKeys.anaf.auth(data?.invoice.companyId ?? "") });
      setActionError(null);
      setStatusMessage(null);
    },
    onError: (e) => setActionError((e as unknown as AppErrorPayload).message ?? "Eroare trimitere ANAF."),
  });

  // Check ANAF status
  const checkStatus = useMutation({
    mutationFn: () => api.anaf.checkStatus(data!.invoice.companyId, id, testMode),
    onSuccess: (stare) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.detail(id) });
      setActionError(null);
      setStatusMessage(`Status ANAF: ${stare}`);
    },
    onError: (e) => setActionError((e as unknown as AppErrorPayload).message ?? "Eroare verificare status."),
  });

  // Storno invoice — creates a proper 381 credit note
  const stornoInvoice = useMutation({
    mutationFn: (reason: string) => api.invoices.storno(id, reason),
    onSuccess: (stornoInv) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.detail(id) });
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
      setStatusMessage(`Factură storno creată: ${stornoInv.fullNumber}`);
      setActionError(null);
    },
    onError: (e) => setActionError((e as unknown as AppErrorPayload).message ?? "Eroare stornare."),
  });

  // Push to SmartBill
  const pushSmartbill = useMutation({
    mutationFn: () => api.integrations.smartbillPush(data!.invoice.companyId, id),
    onSuccess: (result) => {
      setStatusMessage(`Factură trimisă în SmartBill: ${result}`);
      setActionError(null);
    },
    onError: (e) => setActionError((e as unknown as AppErrorPayload).message ?? "Eroare trimitere SmartBill."),
  });

  if (isLoading) {
    return (
      <div className="content">
        <div style={{ padding: 24, fontSize: 12, color: "var(--text-muted)" }}>Se încarcă…</div>
      </div>
    );
  }

  if (!data) {
    return (
      <div className="content">
        <div style={{ padding: 24, fontSize: 12, color: "var(--text-muted)" }}>Factura nu a fost găsită.</div>
      </div>
    );
  }

  const { invoice, lines, events } = data;

  return (
    <div className="content">
      <div className="content-titlebar">
        <span className="content-title">
          <span className="crumb">e-Factura</span>
          <span className="crumb" onClick={() => navigate({ to: "/invoices" })} style={{ cursor: "pointer" }}>
            Facturi emise
          </span>
          <span className="mono">{invoice.fullNumber}</span>
        </span>
        <span style={{ marginLeft: 12 }}>
          <StatusBadge status={invoice.status} />
        </span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
          <button type="button" className="btn" onClick={() => navigate({ to: "/invoices" })}>
            <Icon name="chevronLeft" size={12} /> Înapoi
          </button>
          {invoice.status === "DRAFT" && (
            <button
              type="button"
              className="btn"
              onClick={() => navigate({ to: "/invoices/$id/edit", params: { id } })}
            >
              <Icon name="pen" size={12} /> Editează
            </button>
          )}
          <span className="divider-v" style={{ margin: "0 4px" }} />
          <button
            type="button"
            className="btn"
            disabled={invoice.status !== "VALIDATED" || stornoInvoice.isPending}
            onClick={() => setShowStornoModal(true)}
          >
            <Icon name="storno" size={12} /> {stornoInvoice.isPending ? "Stornare…" : "Storno"}
          </button>
          <button type="button" className="btn" disabled>
            <Icon name="printer" size={12} /> Tipărește
          </button>
        </span>
      </div>

      {actionError && (
        <div style={{ margin: "8px 14px 0", padding: "7px 10px", background: "#FEE2E2", border: "1px solid #FECACA", fontSize: 11, color: "#991B1B" }}>
          {actionError}
          <button type="button" style={{ marginLeft: 8, fontSize: 11, background: "none", border: "none", color: "#991B1B", cursor: "pointer" }} onClick={() => setActionError(null)}>✕</button>
        </div>
      )}

      {statusMessage && !actionError && (
        <div style={{ margin: "8px 14px 0", padding: "7px 10px", background: "#D1FAE5", border: "1px solid #A7F3D0", fontSize: 11, color: "#065F46" }}>
          {statusMessage}
          <button type="button" style={{ marginLeft: 8, fontSize: 11, background: "none", border: "none", color: "#065F46", cursor: "pointer" }} onClick={() => setStatusMessage(null)}>✕</button>
        </div>
      )}

      <div className="detail-actions">
        {/* Generate XML */}
        {!invoice.xmlPath ? (
          <button
            type="button"
            className="btn primary"
            disabled={generateXml.isPending}
            onClick={() => generateXml.mutate()}
          >
            <Icon name="file" size={12} />
            {generateXml.isPending ? "Generare…" : "Generează XML UBL"}
          </button>
        ) : (
          <button
            type="button"
            className="btn"
            disabled={generateXml.isPending}
            onClick={() => generateXml.mutate()}
          >
            <Icon name="refresh" size={12} /> Regenerează XML
          </button>
        )}

        {/* Generate PDF */}
        {!invoice.pdfPath ? (
          <button
            type="button"
            className="btn"
            disabled={generatePdf.isPending}
            onClick={() => generatePdf.mutate()}
          >
            <Icon name="file" size={12} />
            {generatePdf.isPending ? "Generare…" : "Generează PDF"}
          </button>
        ) : (
          <button
            type="button"
            className="btn"
            disabled={generatePdf.isPending}
            onClick={() => generatePdf.mutate()}
          >
            <Icon name="refresh" size={12} /> Regenerează PDF
          </button>
        )}

        {/* Download PDF */}
        <button
          type="button"
          className="btn"
          disabled={generatePdf.isPending || !data}
          onClick={() => generatePdf.mutate()}
        >
          <Icon name="download" size={12} /> PDF
        </button>

        {/* Send to ANAF — only available for DRAFT invoices */}
        {invoice.xmlPath && invoice.status === "DRAFT" && (
          <button
            type="button"
            className={invoice.anafUploadId || invoice.anafIndex ? "btn" : "btn primary"}
            disabled={submitInvoice.isPending || authorizeAnaf.isPending}
            onClick={() => submitInvoice.mutate()}
          >
            <Icon name="cloudUp" size={12} />
            {authorizeAnaf.isPending
              ? "Autorizare ANAF…"
              : submitInvoice.isPending
              ? "Trimitere…"
              : "Trimite la ANAF"}
          </button>
        )}

        {/* Check ANAF status */}
        {(invoice.status === "SUBMITTED" || !!invoice.anafIndex) && (
          <button
            type="button"
            className="btn"
            disabled={checkStatus.isPending}
            onClick={() => checkStatus.mutate()}
          >
            <Icon name="refresh" size={12} />
            {checkStatus.isPending ? "Verificare…" : "Verifică status ANAF"}
          </button>
        )}

        {/* Push to SmartBill */}
        <button
          type="button"
          className="btn"
          disabled={pushSmartbill.isPending || !data}
          onClick={() => pushSmartbill.mutate()}
          title="Trimite factura în SmartBill"
        >
          <Icon name="cloudUp" size={12} /> {pushSmartbill.isPending ? "SmartBill…" : "SmartBill"}
        </button>

        {/* Email mailto — only when contact has email */}
        {contact?.email && (
          <button
            type="button"
            className="btn"
            title={`Trimite email la ${contact.email}`}
            onClick={() => {
              const subject = encodeURIComponent(`Factură ${invoice.fullNumber}`);
              const body = encodeURIComponent(
                `Bună ziua,\n\nVă transmitem factura ${invoice.fullNumber} din data ${invoice.issueDate}, în valoare de ${invoice.totalAmount} ${invoice.currency}.\n\nCu stimă`
              );
              void openUrl(`mailto:${contact.email}?subject=${subject}&body=${body}`);
            }}
          >
            <Icon name="mail" size={12} /> Email
          </button>
        )}

        {/* Copy XML to clipboard */}
        {invoice.xmlPath && (
          <button
            type="button"
            className="btn"
            title="Copiază conținutul XML în clipboard"
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
            <Icon name="copy" size={12} /> {xmlCopied ? "Copiat ✓" : "Copiază XML"}
          </button>
        )}

        {/* ANAF auth status indicator */}
        <span style={{ marginLeft: "auto", display: "inline-flex", alignItems: "center", gap: 6, fontSize: 11 }}>
          {isAnafAuth ? (
            <span style={{ color: "#16A34A", display: "inline-flex", alignItems: "center", gap: 4 }}>
              <Icon name="check" size={11} /> Autentificat ANAF ✓
            </span>
          ) : (
            <span style={{ color: "var(--text-muted)", display: "inline-flex", alignItems: "center", gap: 4 }}>
              <Icon name="warning" size={11} /> Neautentificat ANAF
              {data?.invoice.companyId && (
                <button
                  type="button"
                  style={{ marginLeft: 4, fontSize: 11, background: "none", border: "none", color: "var(--accent)", cursor: "pointer", textDecoration: "underline", padding: 0 }}
                  disabled={authorizeAnaf.isPending}
                  onClick={() => authorizeAnaf.mutate()}
                >
                  {authorizeAnaf.isPending ? "Se autorizează…" : "Autorizează"}
                </button>
              )}
            </span>
          )}
        </span>

        {invoice.anafValidatedAt && (
          <span style={{ display: "inline-flex", alignItems: "center", gap: 6, fontSize: 11, color: "var(--text-muted)" }}>
            <Icon name="clock" size={12} />
            Validat la ANAF:{" "}
            <b style={{ color: "var(--text)" }}>
              {new Date(invoice.anafValidatedAt * 1000).toLocaleString("ro-RO")}
            </b>
          </span>
        )}
      </div>

      <div className="split-60-40">
        {/* PDF preview */}
        <div className="invoice-preview">
          <div className="invoice-paper">
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start" }}>
              <div>
                <h1>Factură fiscală</h1>
                <div style={{ fontSize: 10, color: "#777", letterSpacing: "0.06em" }}>
                  {invoice.fullNumber} · {invoice.issueDate}
                </div>
              </div>
              <div style={{ textAlign: "right" }}>
                {invoice.status === "VALIDATED" && (
                  <div className="seal">e-Factura · validată</div>
                )}
                {invoice.anafIndex && (
                  <div style={{ fontSize: 9.5, color: "#777", marginTop: 4 }}>
                    Index ANAF:{" "}
                    <span style={{ fontFamily: "var(--font-mono)" }}>{invoice.anafIndex}</span>
                  </div>
                )}
              </div>
            </div>

            <div className="ip-grid">
              <div>
                <div className="ip-label">Furnizor</div>
                {company ? (
                  <div style={{ fontSize: 10.5, marginTop: 4 }}>
                    <div style={{ fontWeight: 600 }}>{company.legalName}</div>
                    <div style={{ color: "#555" }}>CUI: {company.cui}</div>
                    <div style={{ color: "#555" }}>{company.address}</div>
                    <div style={{ color: "#555" }}>{company.city}, {company.county}</div>
                  </div>
                ) : (
                  <div style={{ fontSize: 10.5, color: "#555", marginTop: 4 }}>ID: {invoice.companyId}</div>
                )}
              </div>
              <div>
                <div className="ip-label">Cumpărător</div>
                {contact ? (
                  <div style={{ fontSize: 10.5, marginTop: 4 }}>
                    <div style={{ fontWeight: 600 }}>{contact.legalName}</div>
                    {contact.cui && <div style={{ color: "#555" }}>CUI: {contact.cui}</div>}
                    {contact.address && <div style={{ color: "#555" }}>{contact.address}</div>}
                    {contact.city && <div style={{ color: "#555" }}>{contact.city}{contact.county ? `, ${contact.county}` : ""}</div>}
                  </div>
                ) : (
                  <div style={{ fontSize: 10.5, color: "#555", marginTop: 4 }}>ID: {invoice.contactId}</div>
                )}
              </div>
            </div>

            {lines.length > 0 && (
              <table>
                <thead>
                  <tr>
                    <th style={{ width: 24 }}>#</th>
                    <th>Descriere</th>
                    <th style={{ width: 50, textAlign: "right" }}>UM</th>
                    <th style={{ width: 50, textAlign: "right" }}>Cant.</th>
                    <th style={{ width: 70, textAlign: "right" }}>Preț</th>
                    <th style={{ width: 40, textAlign: "right" }}>TVA</th>
                    <th style={{ width: 80, textAlign: "right" }}>Valoare</th>
                  </tr>
                </thead>
                <tbody>
                  {lines.map((l, i) => (
                    <tr key={l.id}>
                      <td style={{ color: "#999" }}>{i + 1}</td>
                      <td>
                        <div style={{ fontWeight: 600 }}>{l.name}</div>
                        {l.description && <div style={{ fontSize: 9.5, color: "#666" }}>{l.description}</div>}
                      </td>
                      <td style={{ textAlign: "right" }}>{l.unit}</td>
                      <td style={{ textAlign: "right" }}>{l.quantity}</td>
                      <td style={{ textAlign: "right" }}>{fmtRON(l.unitPrice)}</td>
                      <td style={{ textAlign: "right" }}>{l.vatRate}%</td>
                      <td style={{ textAlign: "right", fontWeight: 600 }}>{fmtRON(l.subtotalAmount)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}

            <div className="totals">
              <div className="row">
                <span>Subtotal net</span>
                <span>{fmtRON(invoice.subtotalAmount)} {invoice.currency}</span>
              </div>
              <div className="row">
                <span>TVA</span>
                <span>{fmtRON(invoice.vatAmount)} {invoice.currency}</span>
              </div>
              <div className="row grand">
                <span>Total de plată</span>
                <span>{fmtRON(invoice.totalAmount)} {invoice.currency}</span>
              </div>
            </div>

            {invoice.notes && (
              <div style={{ marginTop: 26, fontSize: 9.5, color: "#777", borderTop: "1px solid #DDD", paddingTop: 8 }}>
                {/* Strip internal STORNO_OF prefix stored for UBL generation */}
                {invoice.notes.startsWith("STORNO_OF:")
                  ? invoice.notes.replace(/^STORNO_OF:[^|]*\|?/, "")
                  : invoice.notes}
              </div>
            )}
          </div>

          <div style={{ marginTop: 8, fontSize: 10.5, color: "var(--text-muted)" }}>
            Previzualizare document
          </div>
        </div>

        {/* METADATA + TIMELINE */}
        <div className="invoice-meta">
          <div className="invoice-meta-section">
            <h3>Metadate factură</h3>
            <dl className="invoice-meta-kv">
              <dt>Număr</dt>
              <dd><span className="mono"><b>{invoice.fullNumber}</b></span></dd>
              <dt>Serie</dt>
              <dd className="mono">{invoice.series}</dd>
              <dt>Data emiterii</dt>
              <dd>{invoice.issueDate}</dd>
              <dt>Data scadenței</dt>
              <dd>{invoice.dueDate}</dd>
              <dt>Monedă</dt>
              <dd className="mono">{invoice.currency}</dd>
              {invoice.exchangeRate && (
                <>
                  <dt>Curs valutar</dt>
                  <dd className="tnum">{invoice.exchangeRate}</dd>
                </>
              )}
            </dl>
          </div>

          <div className="invoice-meta-section">
            <h3>Plăți</h3>
            {(() => {
              const total = parseDec(invoice.totalAmount);
              const paid = parseDec(paymentSummary?.paidAmount ?? "0");
              const remaining = total - paid;
              const status = paymentSummary?.paymentStatus ?? "UNPAID";
              return (
                <>
                  <div style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 8 }}>
                    <StatusBadge status={status} />
                  </div>
                  <dl className="invoice-meta-kv">
                    <dt>Total factură</dt>
                    <dd className="tnum">{fmtRON(total)} {invoice.currency}</dd>
                    <dt>Total plătit</dt>
                    <dd className="tnum">{fmtRON(paid)} {invoice.currency}</dd>
                    <dt>Rest de plată</dt>
                    <dd className="tnum" style={{ fontWeight: 600, color: remaining > 0 ? "#B91C1C" : "#166534" }}>
                      {fmtRON(remaining)} {invoice.currency}
                    </dd>
                  </dl>
                  {paymentSummary && paymentSummary.payments.length > 0 ? (
                    <div style={{ marginTop: 10, display: "flex", flexDirection: "column", gap: 6 }}>
                      {paymentSummary.payments.map((p) => (
                        <div
                          key={p.id}
                          style={{
                            display: "flex",
                            alignItems: "center",
                            gap: 8,
                            padding: 6,
                            border: "1px solid var(--border-soft)",
                            background: "var(--bg)",
                            fontSize: 11,
                          }}
                        >
                          <span style={{ flex: 1 }}>
                            <b className="tnum">{fmtRON(p.amount)} {p.currency}</b>
                            <span style={{ color: "var(--text-muted)" }}> · {p.paidAt}</span>
                          </span>
                          <span style={{ fontSize: 10, color: "var(--text-dim)" }}>
                            {p.method}{p.reference ? ` · ${p.reference}` : ""}
                          </span>
                        </div>
                      ))}
                    </div>
                  ) : (
                    <div className="dim" style={{ fontSize: 11, marginTop: 8 }}>
                      Nicio plată înregistrată.
                    </div>
                  )}
                  <button
                    type="button"
                    className="btn"
                    style={{ marginTop: 10 }}
                    onClick={() => navigate({ to: "/payments" })}
                  >
                    Adaugă plată
                  </button>
                </>
              );
            })()}
          </div>

          <div className="invoice-meta-section">
            <h3>Status ANAF</h3>
            <div style={{ display: "flex", gap: 6, alignItems: "center", marginBottom: 6 }}>
              <StatusBadge status={invoice.status} />
              {invoice.anafIndex && (
                <span className="mono dim" style={{ fontSize: 10.5 }}>{invoice.anafIndex}</span>
              )}
            </div>
            <dl className="invoice-meta-kv">
              {invoice.anafSubmittedAt && (
                <>
                  <dt>Trimisă la</dt>
                  <dd className="mono">{new Date(invoice.anafSubmittedAt * 1000).toLocaleString("ro-RO")}</dd>
                </>
              )}
              {invoice.anafValidatedAt && (
                <>
                  <dt>Validată la</dt>
                  <dd className="mono">{new Date(invoice.anafValidatedAt * 1000).toLocaleString("ro-RO")}</dd>
                </>
              )}
              {invoice.anafRejectedAt && (
                <>
                  <dt>Respinsă la</dt>
                  <dd className="mono" style={{ color: "#DC2626" }}>{new Date(invoice.anafRejectedAt * 1000).toLocaleString("ro-RO")}</dd>
                </>
              )}
              {invoice.rejectionReason && (
                <>
                  <dt>Motiv respingere</dt>
                  <dd style={{ color: "#DC2626", fontSize: 11 }}>{invoice.rejectionReason}</dd>
                </>
              )}
            </dl>
          </div>

          {events.length > 0 && (
            <div className="invoice-meta-section">
              <h3>Evenimente · jurnal</h3>
              <div className="timeline">
                {events.map((e) => (
                  <div key={e.id} className="timeline-row info">
                    <span className="dot" />
                    <span className="time">{new Date(e.createdAt * 1000).toLocaleTimeString("ro-RO")}</span>
                    <span className="what">
                      {e.message}
                      <span className="meta">{e.eventType}</span>
                    </span>
                  </div>
                ))}
              </div>
            </div>
          )}

          <div className="invoice-meta-section">
            <h3>Atașamente</h3>
            <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
              {invoice.xmlPath ? (
                <div style={{ display: "flex", alignItems: "center", gap: 8, padding: 6, border: "1px solid var(--border-soft)", background: "var(--bg)", fontSize: 11.5 }}>
                  <Icon name="file" size={14} style={{ color: "var(--text-muted)" }} />
                  <span style={{ flex: 1 }}>{invoice.fullNumber}.xml</span>
                  <span style={{ fontSize: 10, color: "var(--text-dim)" }}>UBL 2.1 CIUS-RO</span>
                </div>
              ) : null}
              {invoice.pdfPath ? (
                <div style={{ display: "flex", alignItems: "center", gap: 8, padding: 6, border: "1px solid var(--border-soft)", background: "var(--bg)", fontSize: 11.5 }}>
                  <Icon name="file" size={14} style={{ color: "var(--text-muted)" }} />
                  <span style={{ flex: 1 }}>{invoice.fullNumber}.pdf</span>
                  <span style={{ fontSize: 10, color: "var(--text-dim)" }}>PDF A4</span>
                </div>
              ) : null}
              {!invoice.xmlPath && !invoice.pdfPath && (
                <span className="dim" style={{ fontSize: 11 }}>Niciun atașament. Generați XML-ul mai întâi.</span>
              )}
            </div>
          </div>
        </div>
      </div>

      {/* Storno confirmation modal */}
      {showStornoModal && (
        <div style={{
          position: "fixed", inset: 0, background: "rgba(0,0,0,0.45)",
          display: "flex", alignItems: "center", justifyContent: "center", zIndex: 999
        }}>
          <div className="panel" style={{ width: 420, padding: 20, boxShadow: "0 8px 32px rgba(0,0,0,0.2)" }}>
            <h3 style={{ margin: "0 0 10px", fontSize: 13, fontWeight: 600 }}>Stornare factură {invoice.fullNumber}</h3>
            <p style={{ fontSize: 11.5, color: "var(--text-muted)", margin: "0 0 10px" }}>
              Această acțiune marchează factura ca anulată. Introduceți motivul stornării:
            </p>
            <textarea
              value={stornoReason}
              onChange={e => setStornoReason(e.target.value)}
              placeholder="Ex: Eroare de preț, anulare comandă..."
              style={{
                width: "100%", minHeight: 72, padding: "6px 8px", boxSizing: "border-box",
                border: "1px solid var(--border)", borderRadius: 3,
                background: "var(--surface)", color: "var(--text)",
                fontSize: 11.5, resize: "vertical", fontFamily: "inherit"
              }}
              autoFocus
            />
            <div style={{ display: "flex", gap: 6, justifyContent: "flex-end", marginTop: 12 }}>
              <button className="btn" onClick={() => { setShowStornoModal(false); setStornoReason(""); }}>
                Anulează
              </button>
              <button
                className="btn danger"
                disabled={!stornoReason.trim() || stornoInvoice.isPending}
                onClick={() => {
                  stornoInvoice.mutate(stornoReason.trim());
                  setShowStornoModal(false);
                  setStornoReason("");
                }}
              >
                {stornoInvoice.isPending ? "Se stornează…" : "Stornează factura"}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
