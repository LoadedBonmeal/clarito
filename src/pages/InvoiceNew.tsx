/**
 * Factură nouă — verbatim port of the design "Factura noua.html":
 *   .page-head (title + "Seria X · număr N generat automat" sub +
 *   pill-btn "Salvează ca schiță" ⌘S + btn-dark send-btn "Salvează și trimite la ANAF")
 *   .inv-grid → left: .scr-card "Părți & detalii factură" (.fgrid emitent/cumpărător +
 *   5-col serie/număr/monedă/date + fxRow curs BNR) · .scr-card "Linii factură"
 *   (LineItemsEditor kept) · .scr-card "Modalitate de plată" → right: .scr-card
 *   "Totaluri" (.tot-row pe cote + grand) · .scr-card "Validare schiță" (.vld items).
 *
 * ALL wiring preserved: api.companies.get + getNextInvoiceNumber,
 * ContactCombobox (autocompletare ANAF), LineItemsEditor, api.bnr.fetchRate,
 * api.invoices.createDraft, api.invoices.validateDraft (live validation),
 * api.anaf.isAuthenticated/authorize/submitInvoice, api.settings.get
 * ("use_anaf_test_env"), Ctrl+S / Ctrl+Enter / Ctrl+P shortcuts.
 */

import { useState, useEffect, useRef, useId } from "react";
import type { ReactNode } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQuery, useMutation } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { ContactCombobox } from "@/components/shared/ContactCombobox";
import { LineItemsEditor, deduceVatCategory } from "@/components/shared/LineItemsEditor";
import type { LineRow } from "@/components/shared/LineItemsEditor";
import { useAppStore } from "@/lib/store";
import { api } from "@/lib/tauri";
import { queryClient, queryKeys } from "@/lib/queries";
import type { Contact, CreateLineInput } from "@/types";
import { CURRENCIES } from "@/lib/constants";
import { fmtShortcut } from "@/lib/platform";
import { notify } from "@/lib/toasts";
import { fmtRON } from "@/lib/utils";
import { formatError } from "@/lib/error-mapper";

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];
const fmtRoDate = (iso: string) => {
  if (!iso) return "—";
  const [y, m, d] = iso.split("-");
  return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
};

function localDateISO(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

function todayISO(): string {
  return localDateISO(new Date());
}

function plusDaysISO(days: number): string {
  const d = new Date();
  d.setDate(d.getDate() + days);
  return localDateISO(d);
}

function newLineRow(vatPayer: boolean, base?: Partial<CreateLineInput>): LineRow {
  // 2026 standard rate is 21% (Legea 141/2025, from 1-Aug-2025). Non-payers → 0.
  const vatRate = vatPayer ? 21 : 0;
  return {
    name: "",
    quantity: 1,
    unit: "buc",
    unitPrice: 0,
    vatRate,
    vatCategory: deduceVatCategory(vatRate, "RO", vatPayer),
    ...base,
    rowId: crypto.randomUUID(),
  };
}

// Prototype icon paths not present in Ic.tsx (inlined verbatim).
const IC_BOOKMARK =
  '<path d="M17.593 3.322c1.1.128 1.907 1.077 1.907 2.185V21L12 17.25 4.5 21V5.507c0-1.108.806-2.057 1.907-2.185a48.507 48.507 0 0 1 11.186 0Z"/>';
const SIC_OK =
  '<path d="M9 12.75 11.25 15 15 9.75M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z"/>';
const SIC_WARN =
  '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>';

/** One design `.vld` validation row (ok/bad/warn icon + title + detail). */
function Vld({ kind, title, sub }: { kind: "ok" | "bad" | "warn"; title: ReactNode; sub?: ReactNode }) {
  return (
    <div className={`vld ${kind}`}>
      <svg
        className="sic"
        viewBox="0 0 24 24"
        dangerouslySetInnerHTML={{ __html: kind === "ok" ? SIC_OK : SIC_WARN }}
      />
      <div>
        <div className="vt">{title}</div>
        {sub != null && <div className="vs">{sub}</div>}
      </div>
    </div>
  );
}

export function InvoiceNewPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);

  const { data: company } = useQuery({
    queryKey: queryKeys.companies.detail(activeCompanyId ?? ""),
    queryFn: () => api.companies.get(activeCompanyId!),
    enabled: !!activeCompanyId,
  });

  const { data: nextNumber } = useQuery({
    queryKey: [...queryKeys.companies.detail(activeCompanyId ?? ""), "nextInvoiceNumber"],
    queryFn: () => api.companies.getNextInvoiceNumber(activeCompanyId!),
    enabled: !!activeCompanyId,
  });

  const [selectedContact, setSelectedContact] = useState<Contact | null>(null);
  const [series, setSeries] = useState<string>("");
  const [issueDate, setIssueDate] = useState<string>(todayISO());
  const [dueDate, setDueDate] = useState<string>(plusDaysISO(30));
  /** True once the user has manually edited the due date field. */
  const [dueDateUserSet, setDueDateUserSet] = useState(false);
  const [currency, setCurrency] = useState<string>("RON");
  const [exchangeRate, setExchangeRate] = useState<string>("");
  const [bnrLoading, setBnrLoading] = useState(false);
  const [notes, setNotes] = useState<string>("");
  const [paymentMeansCode, setPaymentMeansCode] = useState<string>("30");
  const [paymentMethod, setPaymentMethod] = useState<string>("ot");
  const [paymentIban, setPaymentIban] = useState<string>("");
  const [paymentReference, setPaymentReference] = useState<string>("");
  const vatPayer = company?.vatPayer ?? true;
  const [lines, setLines] = useState<LineRow[]>([newLineRow(vatPayer)]);

  const companyEmitentId = useId();
  const seriesId = useId();
  const numberId = useId();
  const issueDateId = useId();
  const dueDateId = useId();
  const currencyId = useId();
  const exchangeRateId = useId();
  const contactInputId = useId();
  const paymentMethodId = useId();
  const paymentIbanId = useId();
  const paymentReferenceId = useId();
  const paymentMeansCodeId = useId();
  const notesId = useId();

  // Auto-prefill currency from contact
  useEffect(() => {
    if (selectedContact?.currency) setCurrency(selectedContact.currency);
  }, [selectedContact]);

  // Auto-prefill due date from contact's paymentTermDays (only if not manually set).
  useEffect(() => {
    if (dueDateUserSet) return;
    if (selectedContact?.paymentTermDays != null && issueDate) {
      const base = new Date(issueDate + "T00:00:00");
      base.setDate(base.getDate() + selectedContact.paymentTermDays);
      setDueDate(localDateISO(base));
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedContact, issueDate]);

  async function handleFetchBnrRate() {
    if (!currency || !issueDate) return;
    setBnrLoading(true);
    try {
      const rate = await api.bnr.fetchRate(currency, issueDate);
      setExchangeRate(String(rate));
      notify.success(t("invoiceForm.notify.bnrFetched", { rate }));
    } catch (err) {
      notify.error(formatError(err, t("invoiceForm.errors.bnrFetch")));
    } finally {
      setBnrLoading(false);
    }
  }

  // Totals — overall + per-VAT-rate breakdown (design .tot-row pe cote)
  const invoiceNet = lines.reduce((s, l) => s + Math.round(l.quantity * l.unitPrice * 100) / 100, 0);
  const invoiceVat = lines.reduce((s, l) => {
    const lineNet = Math.round(l.quantity * l.unitPrice * 100) / 100;
    return s + Math.round(lineNet * (l.vatRate / 100) * 100) / 100;
  }, 0);
  const invoiceTotal = invoiceNet + invoiceVat;
  const parsedRate = parseFloat(exchangeRate);
  const rateValid = currency !== "RON" && Number.isFinite(parsedRate) && parsedRate > 0;

  const vatGroups = (() => {
    const m = new Map<number, { base: number; vat: number }>();
    for (const l of lines) {
      const net = Math.round(l.quantity * l.unitPrice * 100) / 100;
      const vat = Math.round(net * (l.vatRate / 100) * 100) / 100;
      const g = m.get(l.vatRate) ?? { base: 0, vat: 0 };
      g.base += net;
      g.vat += vat;
      m.set(l.vatRate, g);
    }
    return Array.from(m.entries()).sort((a, b) => b[0] - a[0]);
  })();

  const [savedId, setSavedId] = useState<string | null>(null);
  const submitAfterSaveRef = useRef(false);
  const [submitError, setSubmitError] = useState<string | null>(null);

  const { data: testModeSetting } = useQuery({
    queryKey: queryKeys.anaf.testMode,
    queryFn: () => api.settings.get("use_anaf_test_env"),
  });
  const testMode = testModeSetting === "1";

  // Live validation (after first draft save)
  const { data: validation, isFetching: validating } = useQuery({
    queryKey: queryKeys.invoiceValidation.get(savedId ?? ""),
    queryFn: () => api.invoices.validateDraft(savedId!, activeCompanyId!),
    enabled: !!savedId && !!activeCompanyId,
    staleTime: 30_000,
  });

  const activeSeries = series || company?.invoiceSeries || "";
  const activeNumber = nextNumber ?? (company ? company.lastInvoiceNumber + 1 : 1);
  const fullNumber = activeSeries
    ? `${activeSeries}-${String(activeNumber).padStart(4, "0")}`
    : "—";

  const saveDraftMutation = useMutation({
    mutationFn: () => {
      if (!activeCompanyId) throw new Error(t("invoiceForm.errors.noActiveCompany"));
      if (!selectedContact) throw new Error(t("invoiceForm.errors.selectClient"));
      if (lines.length === 0) throw new Error(t("invoiceForm.errors.addLine"));
      const lineErrors: string[] = [];
      lines.forEach((line, i) => {
        if (!line.name?.trim()) lineErrors.push(t("invoiceForm.errors.lineName", { n: i + 1 }));
        if ((line.quantity ?? 0) <= 0) lineErrors.push(t("invoiceForm.errors.lineQty", { n: i + 1 }));
        if ((line.unitPrice ?? 0) < 0) lineErrors.push(t("invoiceForm.errors.linePrice", { n: i + 1 }));
        if (![0, 5, 9, 11, 19, 21].includes(line.vatRate ?? 21))
          lineErrors.push(t("invoiceForm.errors.lineVat", { n: i + 1 }));
      });
      if (lineErrors.length > 0) throw new Error(lineErrors.join("\n"));
      if (currency !== "RON") {
        const rate = parseFloat(exchangeRate);
        if (!Number.isFinite(rate) || rate <= 0) {
          notify.warn(t("invoiceForm.errors.exchangeRatePositive"));
          throw new Error(t("invoiceForm.errors.exchangeRateMissing"));
        }
      }
      const apiLines: CreateLineInput[] = lines.map(({ rowId: _rowId, ...rest }) => rest);
      const extraNotes = [
        paymentMethod !== "ot" && t("invoiceForm.notes.paymentMethod", { method: paymentMethod }),
        paymentIban && t("invoiceForm.notes.iban", { iban: paymentIban }),
        paymentReference && t("invoiceForm.notes.reference", { ref: paymentReference }),
      ].filter(Boolean).join(" | ");
      const finalNotes = extraNotes
        ? (notes ? `${notes}\n${extraNotes}` : extraNotes)
        : notes;
      const parsedExchangeRate = currency !== "RON" ? parseFloat(exchangeRate) : undefined;
      return api.invoices.createDraft({
        companyId: activeCompanyId,
        contactId: selectedContact.id,
        series: activeSeries,
        number: activeNumber,
        issueDate,
        dueDate,
        currency,
        exchangeRate: parsedExchangeRate,
        notes: finalNotes || undefined,
        paymentMeansCode,
        lines: apiLines,
      });
    },
    onSuccess: async (created) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
      setSavedId(created.id);
      const shouldSubmit = submitAfterSaveRef.current;
      submitAfterSaveRef.current = false;
      if (shouldSubmit) {
        try {
          const authenticated = await api.anaf.isAuthenticated(created.companyId);
          if (!authenticated) await api.anaf.authorize(created.companyId);
          await api.anaf.submitInvoice(created.companyId, created.id, testMode);
        } catch (e) {
          setSubmitError(formatError(e, t("invoiceForm.errors.anafSubmit")));
          navigate({ to: "/invoices/$id", params: { id: created.id } });
          return;
        }
      }
      navigate({ to: "/invoices/$id", params: { id: created.id } });
    },
    onError: (e) => {
      submitAfterSaveRef.current = false;
      setSubmitError(formatError(e, t("invoiceForm.errors.save")));
    },
  });

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "s") {
        e.preventDefault();
        if (saveDraftMutation.isPending) return;
        saveDraftMutation.mutate();
      }
      if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
        e.preventDefault();
        if (saveDraftMutation.isPending) return;
        submitAfterSaveRef.current = true;
        setSubmitError(null);
        saveDraftMutation.mutate();
      }
      if ((e.ctrlKey || e.metaKey) && e.key === "p") {
        e.preventDefault();
        window.print();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [saveDraftMutation]);

  if (!activeCompanyId) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>{t("invoiceForm.head.titleNew")}</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          {t("invoiceForm.guard.selectCompanyNew")}
        </div>
      </div>
    );
  }

  // Client-side pre-checks (design .vld rows) — live before first save
  const buyerOk = !!selectedContact;
  const seriesOk = !!activeSeries;
  const dueOk = !!issueDate && !!dueDate && dueDate >= issueDate;
  const localBad = (buyerOk ? 0 : 1) + (seriesOk ? 0 : 1) + (dueOk ? 0 : 1);
  const serverErrors = validation?.errors ?? [];
  const serverWarnings = validation?.warnings ?? [];
  const totalErrors = localBad + serverErrors.length;

  const vldChip = validating
    ? { cls: "wait", icon: SIC_WARN, label: t("invoiceForm.validation.validating") }
    : totalErrors > 0
      ? { cls: "late", icon: SIC_WARN, label: t("invoiceForm.validation.errorCount", { count: totalErrors }) }
      : savedId && validation?.isValid
        ? { cls: "paid", icon: SIC_OK, label: t("invoiceForm.validation.valid") }
        : { cls: "wait", icon: SIC_WARN, label: t("invoiceForm.validation.notValidated") };

  const saveError = submitError ??
    (saveDraftMutation.isError
      ? (saveDraftMutation.error instanceof Error ? saveDraftMutation.error.message : t("invoiceForm.errors.save"))
      : null);

  return (
    <div className="main-inner wide">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>{t("invoiceForm.head.titleNew")}</h1>
          <p className="sub">
            {t("invoiceForm.head.seriesLabel")} <span className="num">{activeSeries || "—"}</span>{" "}
            {t("invoiceForm.head.numberLabel")}{" "}
            <span className="num">{String(activeNumber).padStart(4, "0")}</span>{" "}
            {t("invoiceForm.head.autoGenerated")}
          </p>
        </div>
        <div className="head-actions">
          <button className="pill-btn" onClick={() => void navigate({ to: "/invoices" })}>
            {t("invoiceForm.actions.cancel")}<span className="kbd">Esc</span>
          </button>
          <button
            className="pill-btn"
            disabled
            title={t("invoiceForm.actions.comingSoon")}
            style={{ opacity: 0.5, cursor: "default" }}
            onClick={() => window.print()}
          >
            <Ic name="eye" />{t("invoiceForm.actions.previewPdf")}
          </button>
          <button
            className="pill-btn"
            disabled={saveDraftMutation.isPending}
            onClick={() => saveDraftMutation.mutate()}
          >
            <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_BOOKMARK }} />
            {t("invoiceForm.actions.saveDraft")}<span className="kbd">{fmtShortcut("Ctrl+S")}</span>
          </button>
          <button
            className="btn-dark send-btn"
            disabled={saveDraftMutation.isPending}
            title={t("invoiceForm.actions.saveAndSendTitle", { shortcut: fmtShortcut("Ctrl+Enter") })}
            onClick={() => {
              submitAfterSaveRef.current = true;
              setSubmitError(null);
              saveDraftMutation.mutate();
            }}
          >
            <Ic name="send" />
            {saveDraftMutation.isPending ? t("invoiceForm.actions.saving") : t("invoiceForm.actions.saveAndSend")}
          </button>
        </div>
      </div>

      {/* save / submit error banner */}
      {saveError && (
        <div
          style={{
            display: "flex", gap: 8, alignItems: "flex-start", marginBottom: 14,
            padding: "10px 14px", border: "1px solid var(--red)", borderRadius: 10,
            background: "var(--red-bg, rgba(217,72,53,.06))", color: "var(--red)",
            fontSize: 12.5, whiteSpace: "pre-line",
          }}
        >
          <svg
            className="sic" viewBox="0 0 24 24"
            style={{ width: 14, height: 14, flex: "none", marginTop: 1, stroke: "var(--red)", strokeWidth: 1.6, fill: "none", strokeLinecap: "round", strokeLinejoin: "round" }}
            dangerouslySetInnerHTML={{ __html: SIC_WARN }}
          />
          {saveError}
        </div>
      )}

      <div className="inv-grid">
        <div>
          {/* PĂRȚI & DETALII */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar"><div className="tt">{t("invoiceForm.cards.parties")}</div></div>
            <div className="card-pad">
              <div className="fgrid">
                <div className="field">
                  <label htmlFor={companyEmitentId}>{t("invoiceForm.fields.issuer")}</label>
                  <input
                    className="input"
                    id={companyEmitentId}
                    type="text"
                    value={company ? `${company.legalName}${company.cui ? ` · ${company.cui}` : ""}` : ""}
                    disabled
                    style={{ background: "var(--fill)", color: "var(--text-2)" }}
                  />
                  {company?.registryNumber && (
                    <span className="hint num">{company.registryNumber}</span>
                  )}
                </div>
                <div className="field">
                  <label htmlFor={contactInputId}>{t("invoiceForm.fields.buyer")} <span className="req">*</span></label>
                  <ContactCombobox
                    inputId={contactInputId}
                    value={selectedContact}
                    onChange={setSelectedContact}
                    companyId={activeCompanyId}
                    disabled={!activeCompanyId}
                    filterType={["CUSTOMER", "BOTH"]}
                    width={280}
                  />
                  {selectedContact ? (
                    <span className="hint num">
                      {selectedContact.cui ?? "—"}
                      {selectedContact.vatPayer
                        ? <span style={{ color: "var(--green)", marginLeft: 8 }}>✓ {t("invoiceForm.fields.vatPayer")}</span>
                        : <span style={{ marginLeft: 8 }}>{t("invoiceForm.fields.nonVatPayer")}</span>}
                    </span>
                  ) : (
                    <span className="hint">
                      {t("invoiceForm.fields.newPartnerHint")}{" "}
                      <a className="link" onClick={() => void navigate({ to: "/contacts" })}>
                        {t("invoiceForm.fields.contactsLink")}
                      </a>
                    </span>
                  )}
                </div>
              </div>
              <div className="fgrid" style={{ gridTemplateColumns: "repeat(5,1fr)", marginTop: 13 }}>
                <div className="field">
                  <label htmlFor={seriesId}>{t("invoiceForm.fields.series")}</label>
                  <input
                    className="input num"
                    id={seriesId}
                    type="text"
                    value={activeSeries}
                    onChange={(e) => setSeries(e.target.value)}
                  />
                </div>
                <div className="field">
                  <label htmlFor={numberId}>{t("invoiceForm.fields.number")}</label>
                  <input
                    className="input num"
                    id={numberId}
                    type="text"
                    value={String(activeNumber).padStart(4, "0")}
                    disabled
                    style={{ background: "var(--fill)", color: "var(--text-2)" }}
                  />
                  <span className="hint">{t("invoiceForm.head.autoGenerated")}</span>
                </div>
                <div className="field">
                  <label htmlFor={currencyId}>{t("invoiceForm.fields.currency")}</label>
                  <select
                    className="select"
                    id={currencyId}
                    value={currency}
                    onChange={(e) => setCurrency(e.target.value)}
                  >
                    {CURRENCIES.map((c) => (
                      <option key={c} value={c}>{c}</option>
                    ))}
                  </select>
                </div>
                <div className="field">
                  <label htmlFor={issueDateId}>{t("invoiceForm.fields.issueDate")}</label>
                  <input
                    className="input num"
                    id={issueDateId}
                    type="date"
                    value={issueDate}
                    onChange={(e) => setIssueDate(e.target.value)}
                  />
                </div>
                <div className="field">
                  <label htmlFor={dueDateId}>{t("invoiceForm.fields.dueDate")}</label>
                  <input
                    className="input num"
                    id={dueDateId}
                    type="date"
                    value={dueDate}
                    onChange={(e) => { setDueDate(e.target.value); setDueDateUserSet(true); }}
                  />
                </div>
              </div>
              {currency !== "RON" && (
                <div className="fgrid" style={{ gridTemplateColumns: "1fr 1fr", marginTop: 13 }}>
                  <div className="field">
                    <label htmlFor={exchangeRateId}>
                      {t("invoiceForm.fields.exchangeRate")} <span className="num">{currency}</span>/RON
                    </label>
                    <input
                      className="input num"
                      id={exchangeRateId}
                      type="number"
                      min="0.0001"
                      step="0.0001"
                      value={exchangeRate}
                      onChange={(e) => setExchangeRate(e.target.value)}
                      placeholder={t("invoiceForm.fields.ratePlaceholder")}
                      style={{ textAlign: "right" }}
                    />
                    {rateValid && (
                      <span className="hint">
                        {t("invoiceForm.fields.totalRon")} <b className="num">{fmtRON(invoiceTotal * parsedRate)}</b>
                      </span>
                    )}
                  </div>
                  <div className="field">
                    <label>&nbsp;</label>
                    <button
                      className="pill-btn spin-btn"
                      style={{ width: "max-content" }}
                      disabled={bnrLoading || !issueDate || !currency}
                      onClick={() => void handleFetchBnrRate()}
                    >
                      <Ic name="sync" />
                      {bnrLoading ? t("invoiceForm.actions.fetchingBnr") : t("invoiceForm.actions.fetchBnr")}
                    </button>
                  </div>
                </div>
              )}
            </div>
          </div>

          {/* LINII */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">{t("invoiceForm.cards.lines")}</div>
              <div className="spacer" />
              <span className="muted" style={{ fontSize: 12 }}>
                {t("invoiceForm.lines.item", { count: lines.length })} · {t("invoiceForm.lines.categories")}
              </span>
            </div>
            <LineItemsEditor
              lines={lines}
              onChange={setLines}
              buyerCountry={selectedContact?.country ?? "RO"}
              sellerVatPayer={vatPayer}
              showTotals={false}
              companyId={activeCompanyId ?? undefined}
              currency={currency}
              issueDate={issueDate}
            />
            {rateValid && (
              <div style={{ padding: "10px 16px", borderTop: "1px solid var(--line)", fontSize: 12, color: "var(--text-2)" }}>
                <b style={{ color: "var(--text)" }}>{t("invoiceForm.lines.ronEquiv", { rate: parsedRate.toFixed(4) })}</b>{" "}
                {t("invoiceForm.lines.net")} <span className="num">{fmtRON(invoiceNet * parsedRate)}</span>
                {" · "}
                {t("invoiceForm.lines.vat")} <span className="num">{fmtRON(invoiceVat * parsedRate)}</span>
                {" · "}
                {t("invoiceForm.lines.total")} <b className="num">{fmtRON(invoiceTotal * parsedRate)} RON</b>
              </div>
            )}
          </div>

          {/* MODALITATE DE PLATĂ */}
          <div className="scr-card">
            <div className="scr-toolbar"><div className="tt">{t("invoiceForm.cards.payment")}</div></div>
            <div className="card-pad">
              <div className="fgrid" style={{ gridTemplateColumns: "repeat(3,1fr)" }}>
                <div className="field">
                  <label htmlFor={paymentMethodId}>{t("invoiceForm.fields.method")}</label>
                  <select
                    className="select"
                    id={paymentMethodId}
                    value={paymentMethod}
                    onChange={(e) => setPaymentMethod(e.target.value)}
                  >
                    <option value="ot">{t("invoiceForm.payment.op")}</option>
                    <option value="cash">{t("invoiceForm.payment.cash")}</option>
                    <option value="card">{t("invoiceForm.payment.card")}</option>
                    <option value="comp">{t("invoiceForm.payment.comp")}</option>
                  </select>
                </div>
                <div className="field">
                  <label htmlFor={paymentIbanId}>{t("invoiceForm.fields.iban")}</label>
                  <input
                    className="input num"
                    id={paymentIbanId}
                    type="text"
                    value={paymentIban || company?.iban || ""}
                    onChange={(e) => setPaymentIban(e.target.value)}
                  />
                  {company?.bankName && <span className="hint">{company.bankName}</span>}
                </div>
                <div className="field">
                  <label htmlFor={paymentMeansCodeId}>{t("invoiceForm.fields.ublCode")}</label>
                  <select
                    className="select"
                    id={paymentMeansCodeId}
                    value={paymentMeansCode}
                    onChange={(e) => setPaymentMeansCode(e.target.value)}
                  >
                    <option value="30">{t("invoiceForm.payment.ubl30")}</option>
                    <option value="10">{t("invoiceForm.payment.ubl10")}</option>
                    <option value="48">{t("invoiceForm.payment.ubl48")}</option>
                    <option value="42">{t("invoiceForm.payment.ubl42")}</option>
                    <option value="58">{t("invoiceForm.payment.ubl58")}</option>
                  </select>
                </div>
              </div>
              <div className="field" style={{ marginTop: 13 }}>
                <label htmlFor={paymentReferenceId}>{t("invoiceForm.fields.reference")}</label>
                <input
                  className="input"
                  id={paymentReferenceId}
                  type="text"
                  value={paymentReference}
                  onChange={(e) => setPaymentReference(e.target.value)}
                  placeholder={t("invoiceForm.fields.referencePlaceholder")}
                />
              </div>
              <div className="field" style={{ marginTop: 13 }}>
                <label htmlFor={notesId}>{t("invoiceForm.fields.notes")}</label>
                <textarea
                  className="input"
                  id={notesId}
                  placeholder={t("invoiceForm.fields.optional")}
                  value={notes}
                  onChange={(e) => setNotes(e.target.value)}
                />
              </div>
            </div>
          </div>
        </div>

        {/* RIGHT: TOTALURI + VALIDARE */}
        <div>
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">{t("invoiceForm.cards.totals")}</div>
              <div className="spacer" />
              <span className="muted num" style={{ fontSize: 12 }}>{currency}</span>
            </div>
            <div className="card-pad">
              {vatGroups.map(([rate, g]) => (
                <div key={rate} style={{ display: "contents" }}>
                  <div className="tot-row">
                    <span>{rate === 0 ? t("invoiceForm.totals.baseZero") : t("invoiceForm.totals.base", { rate })}</span>
                    <span className="tv num">{fmtRON(g.base)}</span>
                  </div>
                  <div className="tot-row">
                    <span>{t("invoiceForm.totals.vatRate", { rate })}</span>
                    <span className="tv num">{fmtRON(g.vat)}</span>
                  </div>
                </div>
              ))}
              <div className="tot-row">
                <span>{t("invoiceForm.totals.subtotal")}</span>
                <span className="tv num">{fmtRON(invoiceNet)}</span>
              </div>
              <div className="tot-row">
                <span>{t("invoiceForm.totals.totalVat")}</span>
                <span className="tv num">{fmtRON(invoiceVat)}</span>
              </div>
              <div className="tot-row grand">
                <span>{t("invoiceForm.totals.totalDue")}</span>
                <span className="tv num">{fmtRON(invoiceTotal)} {currency}</span>
              </div>
              {rateValid && (
                <div className="tot-row">
                  <span>{t("invoiceForm.totals.ronEquiv", { rate: parsedRate.toFixed(4) })}</span>
                  <span className="tv num">{fmtRON(invoiceTotal * parsedRate)}</span>
                </div>
              )}
            </div>
          </div>

          <div className="scr-card">
            <div className="scr-toolbar">
              <div className="tt">{t("invoiceForm.cards.validation")}</div>
              <div className="spacer" />
              <span className={`chip ${vldChip.cls}`}>
                <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: vldChip.icon }} />
                {vldChip.label}
              </span>
            </div>
            <div className="card-pad" style={{ paddingTop: 6, paddingBottom: 8 }}>
              <Vld
                kind={buyerOk ? "ok" : "bad"}
                title={buyerOk ? t("invoiceForm.validation.buyerOk") : t("invoiceForm.validation.buyerMissing")}
                sub={
                  buyerOk
                    ? `${selectedContact!.legalName}${selectedContact!.cui ? ` · ${selectedContact!.cui}` : ""}`
                    : t("invoiceForm.validation.buyerHintNew")
                }
              />
              <Vld
                kind={seriesOk ? "ok" : "bad"}
                title={seriesOk ? t("invoiceForm.validation.seriesOk") : t("invoiceForm.validation.seriesMissing")}
                sub={<><code>BT-1</code> {t("invoiceForm.validation.uniqueId", { number: fullNumber })}</>}
              />
              <Vld
                kind={dueOk ? "ok" : "bad"}
                title={dueOk ? t("invoiceForm.validation.dueOk") : t("invoiceForm.validation.dueBad")}
                sub={<><code>BT-9</code> {fmtRoDate(dueDate)} ≥ {fmtRoDate(issueDate)}</>}
              />
              {savedId && validation ? (
                <>
                  {serverErrors.map((msg, i) => (
                    <Vld key={`e${i}`} kind="bad" title={msg} sub={<><code>CIUS-RO</code> {t("invoiceForm.validation.serverError")}</>} />
                  ))}
                  {serverWarnings.map((msg, i) => (
                    <Vld key={`w${i}`} kind="warn" title={msg} sub={<><code>CIUS-RO</code> {t("invoiceForm.validation.serverWarning")}</>} />
                  ))}
                  {validation.isValid && serverErrors.length === 0 && (
                    <Vld
                      kind="ok"
                      title={t("invoiceForm.validation.allValid")}
                      sub={t("invoiceForm.validation.allValidSub")}
                    />
                  )}
                </>
              ) : (
                <Vld
                  kind="warn"
                  title={validating ? t("invoiceForm.validation.validating") : t("invoiceForm.validation.unavailable")}
                  sub={validating ? undefined : t("invoiceForm.validation.unavailableHintNew")}
                />
              )}
              <div className="hint" style={{ padding: "9px 0 4px", borderTop: "1px solid var(--line)" }}>
                {t("invoiceForm.validation.schema")} <b style={{ color: "var(--text-2)" }}>CIUS-RO 1.0.1</b>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
