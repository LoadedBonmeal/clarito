/**
 * Editare factură — re-skinned to rf kit (Wave 2).
 * Preserves ALL wiring: api.invoices.get prefill, api.invoices.updateDraft,
 * api.companies.get, api.bnr.fetchRate (multi-currency), LineItemsEditor,
 * Ctrl+S shortcut, navigate-back on save.
 */

import { useState, useEffect, useCallback, useId } from "react";
import { useNavigate, useParams } from "@tanstack/react-router";
import { useQuery, useMutation } from "@tanstack/react-query";

import { Icon } from "@/components/shared/Icon";
import { ContactCombobox } from "@/components/shared/ContactCombobox";
import { LineItemsEditor } from "@/components/shared/LineItemsEditor";
import type { LineRow } from "@/components/shared/LineItemsEditor";
import {
  PageHeader, Btn, SectionCard, Field, Input, Select, Textarea,
} from "@/components/rf";
import { useAppStore } from "@/lib/store";
import { api } from "@/lib/tauri";
import { queryClient, queryKeys } from "@/lib/queries";
import { notify } from "@/lib/toasts";
import type { Contact, CreateLineInput } from "@/types";
import { parseDec, fmtRON } from "@/lib/utils";
import { formatError } from "@/lib/error-mapper";
import { CURRENCIES } from "@/lib/constants";
import { fmtShortcut } from "@/lib/platform";

function fmtDateRO(iso: string): string {
  const [y, m, d] = iso.split("-");
  return `${d}.${m}.${y}`;
}

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

export function InvoiceEditPage() {
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

  const exchangeRateId = useId();
  const currencyId = useId();

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

  const fullNumber = series
    ? `${series}-${String(invoiceNumber).padStart(4, "0")}`
    : "—";

  const editMutation = useMutation({
    mutationFn: () => {
      if (!activeCompanyId) throw new Error("Nicio companie activă.");
      if (!selectedContact) throw new Error("Selectați un client.");
      if (lines.length === 0) throw new Error("Adăugați cel puțin o linie.");

      const validVatRates = [0, 5, 9, 11, 19, 21];
      for (const [i, line] of lines.entries()) {
        if (!line.name?.trim()) {
          notify.warn(`Linia ${i + 1}: denumirea produsului/serviciului este obligatorie.`);
          throw new Error(`Linia ${i + 1}: denumirea produsului/serviciului este obligatorie.`);
        }
        const qty = Number(line.quantity);
        if (!Number.isFinite(qty) || qty <= 0) {
          notify.warn(`Linia ${i + 1}: cantitatea trebuie să fie mai mare decât 0.`);
          throw new Error(`Linia ${i + 1}: cantitatea trebuie să fie mai mare decât 0.`);
        }
        const price = Number(line.unitPrice);
        if (!Number.isFinite(price) || price < 0) {
          notify.warn(`Linia ${i + 1}: prețul unitar nu poate fi negativ.`);
          throw new Error(`Linia ${i + 1}: prețul unitar nu poate fi negativ.`);
        }
        if (!validVatRates.includes(Number(line.vatRate))) {
          notify.warn(`Linia ${i + 1}: cotă TVA invalidă (${line.vatRate}).`);
          throw new Error(`Linia ${i + 1}: cotă TVA invalidă.`);
        }
      }
      if (currency !== "RON") {
        const rate = parseFloat(exchangeRate);
        if (!Number.isFinite(rate) || rate <= 0) {
          notify.warn("Introduceți un curs valutar pozitiv pentru facturi non-RON.");
          throw new Error("Cursul valutar lipsește sau este invalid.");
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

  if (isLoading) {
    return <div style={{ padding: 32, fontSize: 13, color: "var(--rf-text-muted)" }}>Se încarcă…</div>;
  }

  if (!isLoading && !invoiceData) {
    return (
      <div style={{ padding: 32, fontSize: 13, color: "var(--rf-error)" }}>
        Factura nu a fost găsită sau nu poate fi editată.
      </div>
    );
  }

  if (!initialized) {
    return <div style={{ padding: 32, fontSize: 13, color: "var(--rf-text-muted)" }}>Se încarcă…</div>;
  }

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", background: "var(--rf-app-bg)" }}>
      <PageHeader
        title={`Editare factură`}
        sub={
          <span style={{ fontFamily: "var(--rf-mono)", fontSize: 13, color: "var(--rf-text-muted)" }}>
            {fullNumber}
          </span>
        }
        actions={
          <>
            <Btn
              variant="ghost"
              icon="x"
              onClick={() => navigate({ to: "/invoices/$id", params: { id } })}
            >
              Înapoi <span style={{ marginLeft: 4, fontSize: 11, opacity: 0.6 }}>Esc</span>
            </Btn>
            <Btn
              variant="primary"
              icon="draft"
              disabled={editMutation.isPending}
              onClick={() => editMutation.mutate()}
            >
              Salvează modificările{" "}
              <span style={{ marginLeft: 4, fontSize: 11, opacity: 0.7 }}>{fmtShortcut("Ctrl+S")}</span>
            </Btn>
          </>
        }
      />

      {editMutation.isError && (
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
          {editMutation.error instanceof Error
            ? editMutation.error.message
            : "Eroare la salvare."}
        </div>
      )}

      <div
        style={{
          flex: 1,
          overflow: "auto",
          padding: "0 32px 32px",
          display: "grid",
          gridTemplateColumns: "1fr 340px",
          gap: 20,
          alignItems: "start",
        }}
      >
        {/* LEFT column */}
        <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>

          {/* Parties & header */}
          <SectionCard icon="users" title="Antet factură · date generale">
            <div style={{ padding: "16px 20px", display: "flex", flexDirection: "column", gap: 14 }}>
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 14 }}>
                <Field label="Companie emitentă">
                  <input
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
                <Field label="Cumpărător">
                  <ContactCombobox
                    value={selectedContact}
                    onChange={setSelectedContact}
                    companyId={activeCompanyId ?? ""}
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

              <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: 14 }}>
                <Field label="Serie">
                  <input
                    className="rf-input"
                    value={series}
                    readOnly
                    style={{ fontFamily: "var(--rf-mono)", background: "var(--rf-toolbar-2)" }}
                  />
                </Field>
                <Field label="Număr">
                  <input
                    className="rf-input"
                    value={String(invoiceNumber).padStart(4, "0")}
                    readOnly
                    style={{ fontFamily: "var(--rf-mono)", background: "var(--rf-toolbar-2)" }}
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
                    type="date"
                    value={issueDate}
                    onChange={(e) => setIssueDate(e.target.value)}
                  />
                </Field>
                <Field label="Data scadenței">
                  <Input
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
                  <div style={{ width: 180 }}>
                    <Field label={`Curs valutar (RON / 1 ${currency})`}>
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
              sellerVatPayer={company?.vatPayer ?? true}
              showTotals
              companyId={activeCompanyId ?? undefined}
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

          {/* Notes & payment */}
          <SectionCard icon="file" title="Note · clauze · referințe">
            <div style={{ padding: "12px 20px 16px", display: "flex", flexDirection: "column", gap: 12 }}>
              <Field label="Modalitate plată">
                <Select
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
              <Field label="Observații">
                <Textarea
                  value={notes}
                  onChange={(e) => setNotes(e.target.value)}
                  style={{ height: 80 }}
                />
              </Field>
            </div>
          </SectionCard>
        </div>

        {/* RIGHT column — info/edit aside */}
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

          <SectionCard icon="pen" title="Editare schiță">
            <div style={{ padding: "12px 20px 16px" }}>
              <p style={{ fontSize: 13, color: "var(--rf-text-muted)", margin: 0 }}>
                Modificați datele facturii și apăsați „Salvează modificările".
              </p>
              <Btn
                variant="primary"
                block
                icon="draft"
                disabled={editMutation.isPending}
                onClick={() => editMutation.mutate()}
                style={{ marginTop: 12 }}
              >
                {editMutation.isPending ? "Se salvează…" : "Salvează modificările"}
              </Btn>
            </div>
          </SectionCard>

        </div>
      </div>
    </div>
  );
}
