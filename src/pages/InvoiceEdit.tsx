/**
 * Editare factură — same design DOM as the freshly-ported InvoiceNew.tsx
 * (verbatim layout of "Factura noua.html"): .main-inner.wide + .page-head +
 * .inv-grid → left: .scr-card "Părți & detalii factură" (.fgrid emitent/
 * cumpărător + 5-col serie/număr/monedă/date + fxRow curs BNR) · .scr-card
 * "Linii factură" (LineItemsEditor kept) · .scr-card "Modalitate de plată"
 * → right: .scr-card "Totaluri" (.tot-row pe cote + grand) · .scr-card
 * "Validare schiță" (.vld items, live api.invoices.validateDraft).
 *
 * ALL wiring preserved: api.invoices.get prefill, api.contacts.get,
 * api.companies.get, api.invoices.updateDraft, api.bnr.fetchRate
 * (multi-currency), ContactCombobox, LineItemsEditor, Ctrl+S shortcut,
 * navigate-back on save. Draft-only: non-DRAFT invoices show a guard.
 */

import { useState, useEffect, useCallback, useId } from "react";
import type { ReactNode } from "react";
import { useNavigate, useParams } from "@tanstack/react-router";
import { useQuery, useMutation } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { ContactCombobox } from "@/components/shared/ContactCombobox";
import { LineItemsEditor } from "@/components/shared/LineItemsEditor";
import type { LineRow } from "@/components/shared/LineItemsEditor";
import { useAppStore } from "@/lib/store";
import { api } from "@/lib/tauri";
import { queryClient, queryKeys } from "@/lib/queries";
import { notify } from "@/lib/toasts";
import type { Contact, CreateLineInput } from "@/types";
import { parseDec, fmtRON } from "@/lib/utils";
import { formatError } from "@/lib/error-mapper";
import { CURRENCIES } from "@/lib/constants";
import { fmtShortcut } from "@/lib/platform";

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];
const fmtRoDate = (iso: string) => {
  if (!iso) return "—";
  const [y, m, d] = iso.split("-");
  return `${d} ${RO_MON[Number(m) - 1] ?? m} ${y}`;
};

function newLineRow(base?: Partial<CreateLineInput>): LineRow {
  return {
    name: "",
    quantity: 1,
    unit: "buc",
    unitPrice: 0,
    vatRate: 21,
    vatCategory: "S",
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

export function InvoiceEditPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { id } = useParams({ from: "/invoices/$id/edit" });
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);

  const { data: invoiceData, isLoading } = useQuery({
    queryKey: [...queryKeys.invoices.detail(id), activeCompanyId],
    queryFn: () => api.invoices.get(id, activeCompanyId ?? ""),
    enabled: !!activeCompanyId,
  });

  const { data: company } = useQuery({
    queryKey: queryKeys.companies.detail(activeCompanyId ?? ""),
    queryFn: () => api.companies.get(activeCompanyId!),
    enabled: !!activeCompanyId,
  });

  const [selectedContact, setSelectedContact] = useState<Contact | null>(null);
  const [series, setSeries] = useState<string>("");
  const [invoiceNumber, setInvoiceNumber] = useState<number>(1);
  const [issueDate, setIssueDate] = useState<string>("");
  const [dueDate, setDueDate] = useState<string>("");
  const [currency, setCurrency] = useState<string>("RON");
  const [exchangeRate, setExchangeRate] = useState<string>("");
  const [bnrLoading, setBnrLoading] = useState(false);
  const [notes, setNotes] = useState<string>("");
  const [paymentMeansCode, setPaymentMeansCode] = useState<string>("30");
  const [lines, setLines] = useState<LineRow[]>([newLineRow()]);
  const [initialized, setInitialized] = useState(false);

  const companyEmitentId = useId();
  const contactInputId = useId();
  const seriesId = useId();
  const numberId = useId();
  const currencyId = useId();
  const issueDateId = useId();
  const dueDateId = useId();
  const exchangeRateId = useId();
  const paymentMeansCodeId = useId();
  const notesId = useId();

  // Pre-fill form from loaded invoice
  useEffect(() => {
    if (invoiceData?.invoice && !initialized) {
      const inv = invoiceData.invoice;
      void api.contacts
        .get(inv.contactId, activeCompanyId ?? "")
        .then((c) => setSelectedContact(c))
        .catch(() => setSelectedContact(null));
      setSeries(inv.series);
      setInvoiceNumber(inv.number);
      setIssueDate(inv.issueDate);
      setDueDate(inv.dueDate);
      setCurrency(inv.currency ?? "RON");
      setExchangeRate(inv.exchangeRate != null ? String(inv.exchangeRate) : "");
      setNotes(inv.notes ?? "");
      setPaymentMeansCode(inv.paymentMeansCode ?? "30");
      setLines(
        invoiceData.lines.map((l, i) => ({
          rowId: (l as { id?: string }).id ?? `line-${i}`,
          name: l.name,
          description: l.description ?? undefined,
          quantity: parseDec(l.quantity),
          unit: l.unit,
          unitPrice: parseDec(l.unitPrice),
          vatRate: parseDec(l.vatRate),
          vatCategory: l.vatCategory,
          cpvCode: l.cpvCode ?? undefined,
        }))
      );
      setInitialized(true);
    }
  }, [invoiceData, initialized]);

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

  const fullNumber = series
    ? `${series}-${String(invoiceNumber).padStart(4, "0")}`
    : "—";

  // Live CIUS-RO validation on the existing draft (same API as InvoiceNew)
  const { data: validation, isFetching: validating } = useQuery({
    queryKey: queryKeys.invoiceValidation.get(id),
    queryFn: () => api.invoices.validateDraft(id, activeCompanyId!),
    enabled: !!activeCompanyId && initialized,
    staleTime: 30_000,
  });

  const editMutation = useMutation({
    mutationFn: () => {
      if (!activeCompanyId) throw new Error(t("invoiceForm.errors.noActiveCompany"));
      if (!selectedContact) throw new Error(t("invoiceForm.errors.selectClient"));
      if (lines.length === 0) throw new Error(t("invoiceForm.errors.addLine"));

      const validVatRates = [0, 5, 9, 11, 19, 21];
      for (const [i, line] of lines.entries()) {
        if (!line.name?.trim()) {
          notify.warn(t("invoiceForm.errors.lineNameFull", { n: i + 1 }));
          throw new Error(t("invoiceForm.errors.lineNameFull", { n: i + 1 }));
        }
        const qty = Number(line.quantity);
        if (!Number.isFinite(qty) || qty <= 0) {
          notify.warn(t("invoiceForm.errors.lineQtyFull", { n: i + 1 }));
          throw new Error(t("invoiceForm.errors.lineQtyFull", { n: i + 1 }));
        }
        const price = Number(line.unitPrice);
        if (!Number.isFinite(price) || price < 0) {
          notify.warn(t("invoiceForm.errors.linePriceFull", { n: i + 1 }));
          throw new Error(t("invoiceForm.errors.linePriceFull", { n: i + 1 }));
        }
        if (!validVatRates.includes(Number(line.vatRate))) {
          notify.warn(t("invoiceForm.errors.lineVatFull", { n: i + 1, rate: line.vatRate }));
          throw new Error(t("invoiceForm.errors.lineVat", { n: i + 1 }));
        }
      }
      if (currency !== "RON") {
        const rate = parseFloat(exchangeRate);
        if (!Number.isFinite(rate) || rate <= 0) {
          notify.warn(t("invoiceForm.errors.exchangeRatePositive"));
          throw new Error(t("invoiceForm.errors.exchangeRateMissing"));
        }
      }

      const apiLines: CreateLineInput[] = lines.map(({ rowId: _rowId, ...rest }) => rest);
      const parsedExchangeRate = currency !== "RON" ? parseFloat(exchangeRate) : undefined;

      return api.invoices.updateDraft(id, activeCompanyId, {
        companyId: activeCompanyId,
        contactId: selectedContact.id,
        series,
        number: invoiceNumber,
        issueDate,
        dueDate,
        currency,
        exchangeRate: parsedExchangeRate,
        notes: notes || undefined,
        paymentMeansCode,
        lines: apiLines,
      });
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
      navigate({ to: "/invoices/$id", params: { id } });
    },
  });

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "s") {
        e.preventDefault();
        if (editMutation.isPending) return;
        editMutation.mutate();
      }
    },
    [editMutation],
  );

  useEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);

  if (!activeCompanyId) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>{t("invoiceForm.head.titleEdit")}</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          {t("invoiceForm.guard.selectCompanyEdit")}
        </div>
      </div>
    );
  }

  if (isLoading || (invoiceData && !initialized)) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>{t("invoiceForm.head.titleEdit")}</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          {t("invoiceForm.guard.loading")}
        </div>
      </div>
    );
  }

  if (!invoiceData) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>{t("invoiceForm.head.titleEdit")}</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          {t("invoiceForm.guard.notFound")}
        </div>
      </div>
    );
  }

  // Draft-only guard: only DRAFT invoices can be edited
  if (invoiceData.invoice.status !== "DRAFT") {
    return (
      <div className="main-inner wide">
        <div className="page-head">
          <div>
            <h1>{t("invoiceForm.head.titleEdit")}</h1>
            <p className="sub"><span className="num">{fullNumber}</span></p>
          </div>
          <div className="head-actions">
            <button className="pill-btn" onClick={() => void navigate({ to: "/invoices/$id", params: { id } })}>
              {t("invoiceForm.actions.backToInvoice")}
            </button>
          </div>
        </div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          {t("invoiceForm.guard.draftOnly")}
        </div>
      </div>
    );
  }

  // Client-side pre-checks (design .vld rows)
  const buyerOk = !!selectedContact;
  const seriesOk = !!series;
  const dueOk = !!issueDate && !!dueDate && dueDate >= issueDate;
  const localBad = (buyerOk ? 0 : 1) + (seriesOk ? 0 : 1) + (dueOk ? 0 : 1);
  const serverErrors = validation?.errors ?? [];
  const serverWarnings = validation?.warnings ?? [];
  const totalErrors = localBad + serverErrors.length;

  const vldChip = validating
    ? { cls: "wait", icon: SIC_WARN, label: t("invoiceForm.validation.validating") }
    : totalErrors > 0
      ? { cls: "late", icon: SIC_WARN, label: t("invoiceForm.validation.errorCount", { count: totalErrors }) }
      : validation?.isValid
        ? { cls: "paid", icon: SIC_OK, label: t("invoiceForm.validation.valid") }
        : { cls: "wait", icon: SIC_WARN, label: t("invoiceForm.validation.notValidated") };

  const saveError = editMutation.isError
    ? (editMutation.error instanceof Error ? editMutation.error.message : t("invoiceForm.errors.save"))
    : null;

  return (
    <div className="main-inner wide">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>{t("invoiceForm.head.titleEdit")}</h1>
          <p className="sub">
            {t("invoiceForm.head.seriesLabel")} <span className="num">{series || "—"}</span>{" "}
            {t("invoiceForm.head.numberLabel")}{" "}
            <span className="num">{String(invoiceNumber).padStart(4, "0")}</span>{" "}
            {t("invoiceForm.head.existingDraft")}
          </p>
        </div>
        <div className="head-actions">
          <button
            className="pill-btn"
            onClick={() => void navigate({ to: "/invoices/$id", params: { id } })}
          >
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
            className="btn-dark send-btn"
            disabled={editMutation.isPending}
            title={t("invoiceForm.actions.saveChangesTitle", { shortcut: fmtShortcut("Ctrl+S") })}
            onClick={() => editMutation.mutate()}
          >
            <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_BOOKMARK }} />
            {editMutation.isPending ? t("invoiceForm.actions.saving") : t("invoiceForm.actions.saveChanges")}
            <span className="kbd">{fmtShortcut("Ctrl+S")}</span>
          </button>
        </div>
      </div>

      {/* save error banner */}
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
                    value={series}
                    disabled
                    style={{ background: "var(--fill)", color: "var(--text-2)" }}
                  />
                </div>
                <div className="field">
                  <label htmlFor={numberId}>{t("invoiceForm.fields.number")}</label>
                  <input
                    className="input num"
                    id={numberId}
                    type="text"
                    value={String(invoiceNumber).padStart(4, "0")}
                    disabled
                    style={{ background: "var(--fill)", color: "var(--text-2)" }}
                  />
                  <span className="hint">{t("invoiceForm.fields.allocatedAtCreation")}</span>
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
                    onChange={(e) => setDueDate(e.target.value)}
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
              sellerVatPayer={company?.vatPayer ?? true}
              showTotals={false}
              companyId={activeCompanyId ?? undefined}
              currency={currency}
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
                    : t("invoiceForm.validation.buyerHintEdit")
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
              {validation ? (
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
                  sub={validating ? undefined : t("invoiceForm.validation.unavailableHintEdit")}
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
