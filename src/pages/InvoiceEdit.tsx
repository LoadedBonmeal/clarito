import { useState, useEffect, useCallback } from "react";
import { useNavigate, useParams } from "@tanstack/react-router";
import { useQuery, useMutation } from "@tanstack/react-query";
import { Icon } from "@/components/shared/Icon";
import { ContactCombobox } from "@/components/shared/ContactCombobox";
import { LineItemsEditor } from "@/components/shared/LineItemsEditor";
import type { LineRow } from "@/components/shared/LineItemsEditor";
import { useAppStore } from "@/lib/store";
import { api } from "@/lib/tauri";
import { queryClient, queryKeys } from "@/lib/queries";
import { notify } from "@/lib/toasts";
import type { Contact, CreateLineInput } from "@/types";
import { parseDec } from "@/lib/utils";
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
    queryKey: queryKeys.invoices.detail(id),
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
  const [notes, setNotes] = useState<string>("");
  const [paymentMeansCode, setPaymentMeansCode] = useState<string>("30");
  const [lines, setLines] = useState<LineRow[]>([newLineRow()]);
  const [initialized, setInitialized] = useState(false);

  // Pre-fill form from loaded invoice
  useEffect(() => {
    if (invoiceData?.invoice && !initialized) {
      const inv = invoiceData.invoice;
      // Load the attached contact by id (avoids fetching the full list)
      void api.contacts
        .get(inv.contactId)
        .then((c) => setSelectedContact(c))
        .catch(() => setSelectedContact(null));
      setSeries(inv.series);
      setInvoiceNumber(inv.number);
      setIssueDate(inv.issueDate);
      setDueDate(inv.dueDate);
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

  const fullNumber = series
    ? `${series}-${String(invoiceNumber).padStart(4, "0")}`
    : "—";

  const editMutation = useMutation({
    mutationFn: () => {
      if (!activeCompanyId) throw new Error("Nicio companie activă.");
      if (!selectedContact) throw new Error("Selectați un client.");
      if (lines.length === 0) throw new Error("Adăugați cel puțin o linie.");

      // Per-line validation (mirrors InvoiceNew validation)
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
          notify.warn(`Linia ${i + 1}: cotă TVA invalidă (${line.vatRate}). Valori permise: ${validVatRates.join(", ")}%.`);
          throw new Error(`Linia ${i + 1}: cotă TVA invalidă.`);
        }
      }

      // Strip internal rowId before sending to backend
      const apiLines: CreateLineInput[] = lines.map(({ rowId: _rowId, ...rest }) => rest);

      // R14 Wave A: pass activeCompanyId as explicit ownership argument.
      return api.invoices.updateDraft(id, activeCompanyId, {
        companyId: activeCompanyId,
        contactId: selectedContact.id,
        series,
        number: invoiceNumber,
        issueDate,
        dueDate,
        currency: "RON",
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

  // Ctrl+S / Cmd+S — save the draft (mirrors InvoiceNew.tsx)
  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === 's') {
        e.preventDefault();
        editMutation.mutate();
      }
    },
    [editMutation],
  );

  useEffect(() => {
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [handleKeyDown]);

  if (isLoading || !initialized) {
    return (
      <div className="content">
        <div style={{ padding: 24, fontSize: 12, color: "var(--text-muted)" }}>Se încarcă…</div>
      </div>
    );
  }

  if (!isLoading && !initialized && invoiceData !== undefined) {
    return (
      <div className="content">
        <div style={{ padding: 24, fontSize: 12, color: "#DC2626" }}>
          Factura nu a fost găsită sau nu poate fi editată.
        </div>
      </div>
    );
  }

  return (
    <div className="content">
      <div className="content-titlebar">
        <span className="content-title">
          <span className="crumb">e-Factura</span>
          <span className="crumb" onClick={() => navigate({ to: "/invoices" })} style={{ cursor: "pointer" }}>Facturi emise</span>
          Editare factură ·{" "}
          <span className="mono" style={{ fontWeight: 400, color: "var(--text-muted)" }}>
            {fullNumber}
          </span>
        </span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
          <button className="btn" onClick={() => navigate({ to: "/invoices/$id", params: { id } })}>
            <Icon name="x" size={12} /> Renunță <span className="kbd" style={{ marginLeft: 6 }}>Esc</span>
          </button>
          <button
            className="btn primary"
            onClick={() => editMutation.mutate()}
            disabled={editMutation.isPending}
          >
            <Icon name="draft" size={12} /> Salvează modificările{" "}
            <span className="kbd" style={{ marginLeft: 6 }}>{fmtShortcut("Ctrl+S")}</span>
          </button>
        </span>
      </div>

      {editMutation.isError && (
        <div style={{ padding: "8px 16px", background: "#FEE2E2", color: "#DC2626", fontSize: 12 }}>
          <Icon name="alert" size={12} />{" "}
          {editMutation.error instanceof Error
            ? editMutation.error.message
            : "Eroare la salvare."}
        </div>
      )}

      <div className="editor-split">
        <div className="editor-main">

          <div className="panel" style={{ marginBottom: 12 }}>
            <div className="panel-header">
              <span>Antet factură · date generale</span>
              <span style={{ display: "flex", gap: 6 }}>
                <span className="kbd">Tab</span>
                <span style={{ textTransform: "none", letterSpacing: 0, fontWeight: 400, fontSize: 10.5 }}>
                  pentru câmpul următor
                </span>
              </span>
            </div>
            <div className="panel-body">
              <div className="form-grid">
                <div className="form-section-title">Emitent</div>
                <label>Companie emitentă</label>
                <div className="field">
                  <input
                    className="input"
                    value={company?.legalName ?? ""}
                    readOnly
                    style={{ width: 320, background: "var(--bg)" }}
                  />
                  {company && (
                    <span className="mono muted" style={{ fontSize: 11 }}>
                      CUI {company.cui}
                      {company.registryNumber ? ` · ${company.registryNumber}` : ""}
                    </span>
                  )}
                </div>
                <label>Serie / Număr</label>
                <div className="field">
                  <input
                    className="input mono"
                    value={series}
                    readOnly
                    style={{ width: 90, background: "var(--bg-subtle, var(--bg))", cursor: "default" }}
                  />
                  <input
                    className="input mono"
                    value={String(invoiceNumber).padStart(4, "0")}
                    readOnly
                    style={{ width: 120 }}
                  />
                </div>
                <label>Data emiterii</label>
                <div className="field">
                  <input
                    className="input"
                    type="date"
                    value={issueDate}
                    onChange={(e) => setIssueDate(e.target.value)}
                    style={{ width: 130 }}
                  />
                  <Icon name="calendar" size={14} style={{ color: "var(--text-muted)" }} />
                </div>
                <label>Data scadenței</label>
                <div className="field">
                  <input
                    className="input"
                    type="date"
                    value={dueDate}
                    onChange={(e) => setDueDate(e.target.value)}
                    style={{ width: 130 }}
                  />
                  <span className="muted" style={{ fontSize: 11 }}>
                    {issueDate && dueDate
                      ? `${fmtDateRO(issueDate)} → ${fmtDateRO(dueDate)}`
                      : "30 zile · termen standard"}
                  </span>
                </div>

                <div className="form-section-title">Cumpărător</div>
                <label>Cumpărător</label>
                <div className="field">
                  <ContactCombobox
                    value={selectedContact}
                    onChange={setSelectedContact}
                    companyId={activeCompanyId ?? ""}
                    disabled={!activeCompanyId}
                    filterType={["CUSTOMER", "BOTH"]}
                    width={320}
                  />
                </div>
                {selectedContact && (
                  <>
                    <label>CUI</label>
                    <div className="field">
                      <span className="mono muted" style={{ fontSize: 12 }}>
                        {selectedContact.cui ?? "—"}
                      </span>
                      {selectedContact.vatPayer ? (
                        <span style={{ fontSize: 11, color: "#16A34A" }}>
                          <Icon name="check" size={12} /> plătitor TVA
                        </span>
                      ) : (
                        <span style={{ fontSize: 11, color: "var(--text-muted)" }}>
                          neplătitor TVA
                        </span>
                      )}
                    </div>
                    <label>Adresă</label>
                    <div className="field">
                      <span className="muted" style={{ fontSize: 12 }}>
                        {[selectedContact.address, selectedContact.city, selectedContact.county, selectedContact.country]
                          .filter(Boolean)
                          .join(", ")}
                      </span>
                    </div>
                  </>
                )}
              </div>
            </div>
          </div>

          <div className="panel" style={{ marginBottom: 12 }}>
            <div className="panel-header">
              <span>Linii factură · {lines.length} articole</span>
            </div>
            <LineItemsEditor
              lines={lines}
              onChange={setLines}
              buyerCountry={selectedContact?.country ?? "RO"}
              sellerVatPayer={company?.vatPayer ?? true}
              showTotals
              companyId={activeCompanyId ?? undefined}
            />
          </div>

          <div className="panel">
            <div className="panel-header">
              <span>Note · clauze · referințe</span>
              <span />
            </div>
            <div className="panel-body">
              <div className="form-grid" style={{ gridTemplateColumns: "120px 1fr" }}>
                <label>Modalitate plată</label>
                <div className="field">
                  <select
                    className="input"
                    style={{ width: "100%" }}
                    value={paymentMeansCode}
                    onChange={(e) => setPaymentMeansCode(e.target.value)}
                  >
                    <option value="30">Transfer bancar (30)</option>
                    <option value="10">Numerar (10)</option>
                    <option value="48">Card (48)</option>
                    <option value="42">Cont bancar (42)</option>
                    <option value="58">SEPA (58)</option>
                  </select>
                </div>
                <label>Observații</label>
                <div className="field" style={{ alignItems: "flex-start" }}>
                  <textarea
                    className="input"
                    style={{ width: "100%", height: 64, padding: 6, resize: "vertical" }}
                    value={notes}
                    onChange={(e) => setNotes(e.target.value)}
                  />
                </div>
              </div>
            </div>
          </div>
        </div>

        <aside className="editor-validation">
          <div className="validation-summary">
            <h3>Editare schiță</h3>
            <div style={{ fontSize: 11, color: "var(--text-muted)", marginTop: 8 }}>
              Modificați datele facturii și apăsați „Salvează modificările".
            </div>
          </div>
        </aside>
      </div>
    </div>
  );
}
