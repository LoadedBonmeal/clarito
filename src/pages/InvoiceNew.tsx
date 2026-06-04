/**
 * Factură nouă — re-skinned to rf kit (Wave 2).
 * Preserves 100% of wiring: api.companies.get + getNextInvoiceNumber,
 * api.contacts.list (via ContactCombobox), LineItemsEditor,
 * api.bnr.fetchRate (multi-currency), api.invoices.createDraft,
 * api.invoices.validateDraft (live validation panel), api.anaf.*,
 * api.settings.get("use_anaf_test_env"). Payment panel preserved.
 */

import { useState, useEffect, useRef, useId } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQuery, useMutation } from "@tanstack/react-query";

import { Icon } from "@/components/shared/Icon";
import { ContactCombobox } from "@/components/shared/ContactCombobox";
import { LineItemsEditor, deduceVatCategory } from "@/components/shared/LineItemsEditor";
import type { LineRow } from "@/components/shared/LineItemsEditor";
import {
  PageHeader, Btn, Badge, SectionCard, Field, Input, Select, Textarea,
} from "@/components/rf";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useAppStore } from "@/lib/store";
import { api } from "@/lib/tauri";
import { queryClient, queryKeys } from "@/lib/queries";
import type { AppErrorPayload, Contact, CreateLineInput } from "@/types";
import { CURRENCIES } from "@/lib/constants";
import { fmtShortcut } from "@/lib/platform";
import { notify } from "@/lib/toasts";
import { fmtRON } from "@/lib/utils";
import { formatError } from "@/lib/error-mapper";

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

function fmtDateRO(iso: string): string {
  const [y, m, d] = iso.split("-");
  return `${d}.${m}.${y}`;
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

export function InvoiceNewPage() {
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
  const issueDateId = useId();
  const dueDateId = useId();
  const currencyId = useId();
  const exchangeRateId = useId();
  const contactId = useId();
  const paymentMethodId = useId();
  const paymentIbanId = useId();
  const paymentReferenceId = useId();
  const paymentMeansCodeId = useId();
  const notesId = useId();

  // Auto-prefill currency from contact
  useEffect(() => {
    if (selectedContact?.currency) setCurrency(selectedContact.currency);
  }, [selectedContact]);

  async function handleFetchBnrRate() {
    if (!currency || !issueDate) return;
    setBnrLoading(true);
    try {
      const rate = await api.bnr.fetchRate(currency, issueDate);
      setExchangeRate(String(rate));
      notify.success(`Curs BNR preluat: ${rate}`);
    } catch (err) {
      notify.error(formatError(err, "Nu s-a putut prelua cursul BNR."));
    } finally {
      setBnrLoading(false);
    }
  }

  // Totals for RON-equivalent display
  const invoiceNet = lines.reduce((s, l) => s + Math.round(l.quantity * l.unitPrice * 100) / 100, 0);
  const invoiceVat = lines.reduce((s, l) => {
    const lineNet = Math.round(l.quantity * l.unitPrice * 100) / 100;
    return s + Math.round(lineNet * (l.vatRate / 100) * 100) / 100;
  }, 0);
  const invoiceTotal = invoiceNet + invoiceVat;
  const parsedRate = parseFloat(exchangeRate);
  const rateValid = currency !== "RON" && Number.isFinite(parsedRate) && parsedRate > 0;

  const [savedId, setSavedId] = useState<string | null>(null);
  const submitAfterSaveRef = useRef(false);
  const [submitError, setSubmitError] = useState<string | null>(null);

  const { data: testModeSetting } = useQuery({
    queryKey: queryKeys.anaf.testMode,
    queryFn: () => api.settings.get("use_anaf_test_env"),
  });
  const testMode = testModeSetting === "1";

  // Live validation
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
      if (!activeCompanyId) throw new Error("Nicio companie activă.");
      if (!selectedContact) throw new Error("Selectați un client.");
      if (lines.length === 0) throw new Error("Adăugați cel puțin o linie.");
      const lineErrors: string[] = [];
      lines.forEach((line, i) => {
        if (!line.name?.trim()) lineErrors.push(`Linia ${i + 1}: denumirea este obligatorie`);
        if ((line.quantity ?? 0) <= 0) lineErrors.push(`Linia ${i + 1}: cantitatea trebuie > 0`);
        if ((line.unitPrice ?? 0) < 0) lineErrors.push(`Linia ${i + 1}: prețul nu poate fi negativ`);
        if (![0, 5, 9, 11, 19, 21].includes(line.vatRate ?? 21))
          lineErrors.push(`Linia ${i + 1}: cotă TVA invalidă`);
      });
      if (lineErrors.length > 0) throw new Error(lineErrors.join("\n"));
      if (currency !== "RON") {
        const rate = parseFloat(exchangeRate);
        if (!Number.isFinite(rate) || rate <= 0) {
          notify.warn("Introduceți un curs valutar pozitiv pentru facturi non-RON.");
          throw new Error("Cursul valutar lipsește sau este invalid.");
        }
      }
      const apiLines: CreateLineInput[] = lines.map(({ rowId: _rowId, ...rest }) => rest);
      const extraNotes = [
        paymentMethod !== "ot" && `Metodă plată: ${paymentMethod}`,
        paymentIban && `IBAN: ${paymentIban}`,
        paymentReference && `Ref: ${paymentReference}`,
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
          setSubmitError((e as unknown as AppErrorPayload).message ?? "Eroare la trimitere ANAF.");
          navigate({ to: "/invoices/$id", params: { id: created.id } });
          return;
        }
      }
      navigate({ to: "/invoices/$id", params: { id: created.id } });
    },
    onError: (e) => {
      submitAfterSaveRef.current = false;
      setSubmitError((e as unknown as AppErrorPayload).message ?? "Eroare la salvare.");
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
      <div style={{ padding: 40, textAlign: "center" }}>
        <p style={{ fontSize: 14, color: "var(--rf-text-muted)", marginBottom: 16 }}>
          Selectați o companie activă din bara laterală pentru a emite o factură.
        </p>
      </div>
    );
  }

  const validationScore = validation
    ? validation.isValid ? 100 : Math.max(0, 100 - validation.errors.length * 20)
    : 0;

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", background: "var(--rf-app-bg)" }}>
      <PageHeader
        title={
          <span>
            Factură nouă{" "}
            <Badge variant="neutral">Ciornă</Badge>
          </span>
        }
        sub={
          <span style={{ fontFamily: "var(--rf-mono)", fontSize: 13, color: "var(--rf-text-muted)" }}>
            {fullNumber}
          </span>
        }
        actions={
          <>
            <Btn variant="ghost" icon="x" onClick={() => navigate({ to: "/invoices" })}>
              Renunță <span style={{ marginLeft: 4, fontSize: 11, opacity: 0.6 }}>Esc</span>
            </Btn>
            <Btn
              variant="secondary"
              icon="draft"
              disabled={saveDraftMutation.isPending}
              onClick={() => saveDraftMutation.mutate()}
            >
              Salvează ca schiță{" "}
              <span style={{ marginLeft: 4, fontSize: 11, opacity: 0.6 }}>{fmtShortcut("Ctrl+S")}</span>
            </Btn>
            <Btn variant="secondary" icon="eye" disabled onClick={() => window.print()}>
              Previzualizare PDF{" "}
              <span style={{ marginLeft: 4, fontSize: 11, opacity: 0.6 }}>{fmtShortcut("Ctrl+P")}</span>
            </Btn>
            <Btn
              variant="primary"
              icon="cloudUp"
              disabled={saveDraftMutation.isPending}
              onClick={() => {
                submitAfterSaveRef.current = true;
                setSubmitError(null);
                saveDraftMutation.mutate();
              }}
              title="Salvează și trimite la ANAF"
            >
              Trimite la ANAF{" "}
              <span style={{ marginLeft: 4, fontSize: 11, opacity: 0.7 }}>{fmtShortcut("Ctrl+Enter")}</span>
            </Btn>
          </>
        }
      />

      {(saveDraftMutation.isError || submitError) && (
        <div
          style={{
            margin: "0 32px 8px",
            padding: "10px 14px",
            background: "var(--rf-error-bg)",
            border: "1px solid var(--rf-error-bd)",
            borderRadius: 8,
            color: "var(--rf-error)",
            fontSize: 13,
          }}
        >
          <Icon name="alert" size={14} style={{ marginRight: 6 }} />
          {submitError ??
            (saveDraftMutation.error instanceof Error
              ? saveDraftMutation.error.message
              : "Eroare la salvare.")}
        </div>
      )}

      <div
        style={{
          flex: 1,
          overflow: "auto",
          padding: "0 32px 32px",
          display: "grid",
          gridTemplateColumns: "1fr 360px",
          gap: 20,
          alignItems: "start",
        }}
      >
        {/* LEFT column */}
        <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>

          {/* Parties & header fields */}
          <SectionCard icon="users" title="Părți & detalii factură">
            <div style={{ padding: "12px 20px 16px", display: "flex", flexDirection: "column", gap: 14 }}>
              {/* 2-col: emitent + cumpărător */}
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 14 }}>
                <Field label="Companie emitentă">
                  <input
                    id={companyEmitentId}
                    className="rf-input"
                    value={company?.legalName ?? ""}
                    readOnly
                    style={{ background: "var(--rf-toolbar-2)" }}
                  />
                  {company && (
                    <span style={{ fontSize: 11, color: "var(--rf-text-muted)", fontFamily: "var(--rf-mono)", marginTop: 2 }}>
                      CUI {company.cui}{company.registryNumber ? ` · ${company.registryNumber}` : ""}
                    </span>
                  )}
                </Field>
                <Field label="Cumpărător" required>
                  <ContactCombobox
                    inputId={contactId}
                    value={selectedContact}
                    onChange={setSelectedContact}
                    companyId={activeCompanyId}
                    disabled={!activeCompanyId}
                    filterType={["CUSTOMER", "BOTH"]}
                    width={280}
                  />
                  {selectedContact && (
                    <span style={{ fontSize: 11, color: "var(--rf-text-muted)", fontFamily: "var(--rf-mono)", marginTop: 2 }}>
                      {selectedContact.cui ?? "—"}
                      {selectedContact.vatPayer
                        ? <span style={{ color: "var(--rf-success)", marginLeft: 8 }}>✓ plătitor TVA</span>
                        : <span style={{ color: "var(--rf-text-dim)", marginLeft: 8 }}>neplătitor TVA</span>
                      }
                    </span>
                  )}
                </Field>
              </div>

              {/* 3-col meta fields */}
              <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: 14 }}>
                <Field label="Serie">
                  <input
                    id={seriesId}
                    className="rf-input"
                    style={{ fontFamily: "var(--rf-mono)" }}
                    value={activeSeries}
                    onChange={(e) => setSeries(e.target.value)}
                  />
                </Field>
                <Field label="Număr" help="Generat automat">
                  <input
                    className="rf-input"
                    style={{ fontFamily: "var(--rf-mono)", background: "var(--rf-toolbar-2)" }}
                    value={String(activeNumber).padStart(4, "0")}
                    readOnly
                  />
                </Field>
                <Field label="Monedă">
                  <Select
                    id={currencyId}
                    value={currency}
                    onChange={(e) => setCurrency(e.target.value)}
                  >
                    {CURRENCIES.map((c) => (
                      <option key={c} value={c}>{c}</option>
                    ))}
                  </Select>
                </Field>
                <Field label="Data emiterii">
                  <Input
                    id={issueDateId}
                    type="date"
                    value={issueDate}
                    onChange={(e) => setIssueDate(e.target.value)}
                  />
                </Field>
                <Field label="Data scadenței">
                  <Input
                    id={dueDateId}
                    type="date"
                    value={dueDate}
                    onChange={(e) => setDueDate(e.target.value)}
                  />
                  {issueDate && dueDate && (
                    <span style={{ fontSize: 11, color: "var(--rf-text-muted)", marginTop: 2 }}>
                      {fmtDateRO(issueDate)} → {fmtDateRO(dueDate)}
                    </span>
                  )}
                </Field>
              </div>

              {/* Multi-currency BNR panel */}
              {currency !== "RON" && (
                <div
                  style={{
                    display: "flex",
                    gap: 14,
                    alignItems: "flex-end",
                    background: "var(--rf-info-bg)",
                    border: "1px solid var(--rf-info-bd)",
                    borderRadius: 8,
                    padding: 12,
                  }}
                >
                  <div style={{ width: 160 }}>
                <Field label={`Curs valutar ${currency}/RON`}>
                    <Input
                      id={exchangeRateId}
                      type="number"
                      min="0.0001"
                      step="0.0001"
                      value={exchangeRate}
                      onChange={(e) => setExchangeRate(e.target.value)}
                      placeholder="ex: 4.9700"
                      num
                    />
                  </Field>
                  </div>
                  <Btn
                    variant="secondary"
                    size="sm"
                    icon="refresh"
                    disabled={bnrLoading || !issueDate || !currency}
                    onClick={handleFetchBnrRate}
                  >
                    {bnrLoading ? "Se preia…" : "Preia curs BNR"}
                  </Btn>
                  {rateValid && (
                    <span style={{ fontSize: 12, color: "var(--rf-info)", marginBottom: 6 }}>
                      Total RON: <b style={{ fontFamily: "var(--rf-mono)" }}>{fmtRON(invoiceTotal * parsedRate)}</b>
                    </span>
                  )}
                </div>
              )}
            </div>
          </SectionCard>

          {/* Line items */}
          <SectionCard icon="list" title="Linii factură" subtitle={`${lines.length} articole`}>
            <LineItemsEditor
              lines={lines}
              onChange={setLines}
              buyerCountry={selectedContact?.country ?? "RO"}
              sellerVatPayer={vatPayer}
              showTotals
              companyId={activeCompanyId ?? undefined}
              currency={currency}
            />
            {rateValid && (
              <div
                style={{
                  padding: "10px 20px",
                  borderTop: "1px solid var(--rf-border)",
                  fontSize: 12,
                  color: "var(--rf-text-muted)",
                }}
              >
                <span style={{ fontWeight: 600, color: "var(--rf-text)" }}>
                  Echivalent RON (curs {parsedRate.toFixed(4)}):
                </span>{" "}
                Net: <span style={{ fontFamily: "var(--rf-mono)" }}>{fmtRON(invoiceNet * parsedRate)}</span>
                {" · "}
                TVA: <span style={{ fontFamily: "var(--rf-mono)" }}>{fmtRON(invoiceVat * parsedRate)}</span>
                {" · "}
                Total: <span style={{ fontFamily: "var(--rf-mono)", fontWeight: 700 }}>{fmtRON(invoiceTotal * parsedRate)} RON</span>
              </div>
            )}
          </SectionCard>

          {/* Payment + Notes row */}
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 20 }}>
            {/* Payment panel */}
            <SectionCard icon="bank" title="Modalitate de plată">
              <div style={{ padding: "12px 20px 16px", display: "flex", flexDirection: "column", gap: 12 }}>
                <Field label="Metodă">
                  <Select
                    id={paymentMethodId}
                    value={paymentMethod}
                    onChange={(e) => setPaymentMethod(e.target.value)}
                  >
                    <option value="ot">Ordin de plată (OP)</option>
                    <option value="cash">Numerar</option>
                    <option value="card">Card bancar</option>
                    <option value="comp">Compensare</option>
                  </Select>
                </Field>
                <Field label="Cont bancar (IBAN)">
                  <Input
                    id={paymentIbanId}
                    value={paymentIban || company?.iban || ""}
                    onChange={(e) => setPaymentIban(e.target.value)}
                    style={{ fontFamily: "var(--rf-mono)" }}
                  />
                  {company?.bankName && (
                    <span style={{ fontSize: 11, color: "var(--rf-text-muted)", marginTop: 2 }}>
                      {company.bankName}
                    </span>
                  )}
                </Field>
                <Field label="Referință">
                  <Input
                    id={paymentReferenceId}
                    value={paymentReference}
                    onChange={(e) => setPaymentReference(e.target.value)}
                    placeholder="Plătiți în 30 zile de la data emiterii"
                  />
                </Field>
                <Field
                  label={
                    <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
                      Mod plată
                      <TooltipProvider>
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <span
                              style={{
                                cursor: "help", fontSize: 10, color: "var(--rf-text-muted)",
                                border: "1px solid var(--rf-border-strong)", borderRadius: "50%",
                                width: 13, height: 13, display: "inline-flex", alignItems: "center",
                                justifyContent: "center", lineHeight: 1, flexShrink: 0,
                              }}
                            >?</span>
                          </TooltipTrigger>
                          <TooltipContent side="top" style={{ maxWidth: 260 }}>
                            <strong>Coduri UNECE:</strong><br />
                            <b>10</b> — Numerar<br />
                            <b>30</b> — Transfer bancar<br />
                            <b>42</b> — Debit direct<br />
                            <b>48</b> — Card bancar<br />
                            <b>58</b> — Transfer SEPA
                          </TooltipContent>
                        </Tooltip>
                      </TooltipProvider>
                    </span>
                  }
                >
                  <Select
                    id={paymentMeansCodeId}
                    value={paymentMeansCode}
                    onChange={(e) => setPaymentMeansCode(e.target.value)}
                  >
                    <option value="30">Transfer bancar (30)</option>
                    <option value="10">Numerar (10)</option>
                    <option value="48">Card (48)</option>
                    <option value="42">Cont bancar (42)</option>
                    <option value="58">SEPA (58)</option>
                  </Select>
                </Field>
              </div>
            </SectionCard>

            {/* Notes panel */}
            <SectionCard icon="file" title="Note · clauze · referințe">
              <div style={{ padding: "12px 20px 16px" }}>
                <Field label="Observații">
                  <Textarea
                    id={notesId}
                    value={notes}
                    onChange={(e) => setNotes(e.target.value)}
                    style={{ height: 100 }}
                  />
                </Field>
              </div>
            </SectionCard>
          </div>
        </div>

        {/* RIGHT column — totals + validation panel */}
        <div style={{ position: "sticky", top: 0, display: "flex", flexDirection: "column", gap: 16 }}>

          {/* Totals */}
          <SectionCard icon="chart" title="Totaluri">
            <div style={{ padding: "12px 20px 16px", display: "flex", flexDirection: "column", gap: 8 }}>
              <div style={{ display: "flex", justifyContent: "space-between", fontSize: 13 }}>
                <span style={{ color: "var(--rf-text-muted)" }}>Total net</span>
                <span style={{ fontFamily: "var(--rf-mono)", fontWeight: 600 }}>{fmtRON(invoiceNet)}</span>
              </div>
              <div style={{ display: "flex", justifyContent: "space-between", fontSize: 13 }}>
                <span style={{ color: "var(--rf-text-muted)" }}>Total TVA</span>
                <span style={{ fontFamily: "var(--rf-mono)" }}>{fmtRON(invoiceVat)}</span>
              </div>
              <div style={{ borderTop: "1px solid var(--rf-border)", margin: "4px 0" }} />
              <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline" }}>
                <span style={{ fontWeight: 700 }}>Total de plată</span>
                <span style={{ fontFamily: "var(--rf-mono)", fontSize: 22, fontWeight: 700, color: "var(--rf-accent)" }}>
                  {fmtRON(invoiceTotal)}{" "}
                  <span style={{ fontSize: 13, color: "var(--rf-text-muted)", fontWeight: 400 }}>{currency}</span>
                </span>
              </div>
              {rateValid && (
                <div
                  style={{
                    display: "flex", justifyContent: "space-between",
                    background: "var(--rf-info-bg)", border: "1px solid var(--rf-info-bd)",
                    borderRadius: 8, padding: "8px 12px", marginTop: 4,
                  }}
                >
                  <span style={{ fontSize: 12.5, color: "var(--rf-info)", fontWeight: 600 }}>Echivalent RON</span>
                  <span style={{ fontFamily: "var(--rf-mono)", fontSize: 14, fontWeight: 700, color: "var(--rf-info)" }}>
                    {fmtRON(invoiceTotal * parsedRate)}
                  </span>
                </div>
              )}
            </div>
          </SectionCard>

          {/* Live validation panel */}
          <div
            style={{
              background: "var(--rf-content)",
              border: "1px solid var(--rf-border)",
              borderRadius: "var(--rf-radius)",
              overflow: "hidden",
              boxShadow: "var(--rf-shadow-sm)",
            }}
          >
            {/* Summary */}
            <div style={{ padding: "14px 18px 10px", borderBottom: "1px solid var(--rf-border)" }}>
              <div style={{ fontSize: 12, fontWeight: 700, textTransform: "uppercase", letterSpacing: "0.06em", color: "var(--rf-text-dim)", marginBottom: 8 }}>
                Validare RO_CIUS · live
              </div>
              {savedId && validation ? (
                <>
                  <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 4 }}>
                    <span style={{ fontSize: 24, fontWeight: 700, fontFamily: "var(--rf-mono)", color: validation.isValid ? "var(--rf-success)" : "var(--rf-error)" }}>
                      {validationScore}%
                    </span>
                    <div style={{ flex: 1, height: 6, background: "var(--rf-neutral-bg)", borderRadius: 999 }}>
                      <div
                        style={{
                          width: `${validationScore}%`, height: "100%",
                          background: validation.isValid ? "var(--rf-success)" : validation.errors.length > 0 ? "var(--rf-error)" : "var(--rf-warning)",
                          borderRadius: 999, transition: "width .3s",
                        }}
                      />
                    </div>
                  </div>
                  <div style={{ fontSize: 12, color: validation.isValid ? "var(--rf-success)" : "var(--rf-text-muted)" }}>
                    {validation.isValid
                      ? "✓ Validă — se poate trimite la ANAF"
                      : `${validation.errors.length} erori · ${validation.warnings.length} avertismente`}
                  </div>
                </>
              ) : (
                <>
                  <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 4 }}>
                    <span style={{ fontSize: 24, fontWeight: 700, fontFamily: "var(--rf-mono)", color: "var(--rf-text-dim)" }}>
                      {validating ? "…" : "—"}
                    </span>
                    <div style={{ flex: 1, height: 6, background: "var(--rf-neutral-bg)", borderRadius: 999 }}>
                      <div style={{ width: "0%", height: "100%", background: "var(--rf-text-dim)", borderRadius: 999 }} />
                    </div>
                  </div>
                  <div style={{ fontSize: 12, color: "var(--rf-text-muted)" }}>
                    {validating ? "Se validează…" : "Salvați schiță pentru a valida"}
                  </div>
                </>
              )}
            </div>

            {/* Validation items */}
            <div style={{ maxHeight: 260, overflowY: "auto" }}>
              {savedId && validation ? (
                <>
                  {validation.errors.map((msg, i) => (
                    <div
                      key={`e${i}`}
                      style={{
                        display: "flex", gap: 10, padding: "10px 18px",
                        borderBottom: "1px solid var(--rf-border)",
                        background: "var(--rf-error-bg)",
                      }}
                    >
                      <Icon name="cancel" size={14} style={{ color: "var(--rf-error)", flexShrink: 0, marginTop: 1 }} />
                      <div>
                        <div style={{ fontSize: 12, fontWeight: 600, color: "var(--rf-error)" }}>Eroare</div>
                        <div style={{ fontSize: 12, color: "var(--rf-text-muted)" }}>{msg}</div>
                      </div>
                    </div>
                  ))}
                  {validation.warnings.map((msg, i) => (
                    <div
                      key={`w${i}`}
                      style={{
                        display: "flex", gap: 10, padding: "10px 18px",
                        borderBottom: "1px solid var(--rf-border)",
                        background: "var(--rf-warning-bg)",
                      }}
                    >
                      <Icon name="warning" size={14} style={{ color: "var(--rf-warning)", flexShrink: 0, marginTop: 1 }} />
                      <div>
                        <div style={{ fontSize: 12, fontWeight: 600, color: "var(--rf-warning)" }}>Avertisment</div>
                        <div style={{ fontSize: 12, color: "var(--rf-text-muted)" }}>{msg}</div>
                      </div>
                    </div>
                  ))}
                  {validation.isValid && validation.errors.length === 0 && (
                    <div
                      style={{
                        display: "flex", gap: 10, padding: "10px 18px",
                        background: "var(--rf-success-bg)",
                      }}
                    >
                      <Icon name="check" size={14} style={{ color: "var(--rf-success)", flexShrink: 0, marginTop: 1 }} />
                      <div>
                        <div style={{ fontSize: 12, fontWeight: 600, color: "var(--rf-success)" }}>Factură validă</div>
                        <div style={{ fontSize: 12, color: "var(--rf-text-muted)" }}>
                          Toate regulile CIUS-RO sunt respectate.
                        </div>
                      </div>
                    </div>
                  )}
                </>
              ) : (
                <div
                  style={{
                    display: "flex", gap: 10, padding: "10px 18px",
                    background: "var(--rf-warning-bg)",
                  }}
                >
                  <Icon name="warning" size={14} style={{ color: "var(--rf-warning)", flexShrink: 0, marginTop: 1 }} />
                  <div>
                    <div style={{ fontSize: 12, fontWeight: 600, color: "var(--rf-warning)" }}>Validare indisponibilă</div>
                    <div style={{ fontSize: 12, color: "var(--rf-text-muted)" }}>
                      Completați formularul și salvați ca schiță pentru a valida.
                    </div>
                  </div>
                </div>
              )}
            </div>

            {/* Schema footer */}
            <div
              style={{
                padding: "8px 18px",
                borderTop: "1px solid var(--rf-border)",
                background: "var(--rf-toolbar-2)",
                fontSize: 11,
                color: "var(--rf-text-dim)",
              }}
            >
              Schema: <b style={{ color: "var(--rf-text-muted)" }}>CIUS-RO 1.0.1</b>
            </div>
          </div>

          {/* Generate & Send card */}
          <SectionCard icon="fileOut" title="Generează & trimite">
            <div style={{ padding: "12px 20px 16px", display: "flex", flexDirection: "column", gap: 8 }}>
              <Btn
                variant="secondary"
                icon="file"
                block
                disabled={saveDraftMutation.isPending}
                onClick={() => saveDraftMutation.mutate()}
              >
                Salvează ca schiță
              </Btn>
              <Btn
                variant="primary"
                icon="cloudUp"
                block
                disabled={saveDraftMutation.isPending}
                onClick={() => {
                  submitAfterSaveRef.current = true;
                  setSubmitError(null);
                  saveDraftMutation.mutate();
                }}
              >
                Trimite la ANAF
              </Btn>
              <div
                style={{
                  display: "flex", gap: 6, fontSize: 12, color: "var(--rf-text-muted)", marginTop: 4,
                }}
              >
                <Icon name="info" size={13} style={{ flexShrink: 0, marginTop: 1 }} />
                Validarea verifică structura conform schemei CIUS-RO înainte de trimitere.
              </div>
            </div>
          </SectionCard>
        </div>
      </div>
    </div>
  );
}
