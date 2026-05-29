import { useState, useEffect, useRef } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQuery, useMutation } from "@tanstack/react-query";
import { Icon } from "@/components/shared/Icon";
import { useAppStore } from "@/lib/store";
import { api } from "@/lib/tauri";
import { queryClient, queryKeys } from "@/lib/queries";
import type { AppErrorPayload, CreateLineInput, VatCategory } from "@/types";
import { fmtRON } from "@/lib/utils";

/** Extends CreateLineInput with a stable row key for React list rendering. */
type LineRow = CreateLineInput & { rowId: string };

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

const DEFAULT_LINE: CreateLineInput = {
  name: "",
  quantity: 1,
  unit: "buc",
  unitPrice: 0,
  vatRate: 21,
  vatCategory: "S" as VatCategory,
};

function newLineRow(base?: Partial<CreateLineInput>): LineRow {
  return { ...DEFAULT_LINE, ...base, rowId: crypto.randomUUID() };
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

  const { data: contacts = [] } = useQuery({
    queryKey: queryKeys.contacts.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () => api.contacts.list({ companyId: activeCompanyId ?? undefined }),
    enabled: !!activeCompanyId,
  });

  const [contactId, setContactId] = useState<string>("");
  const [series, setSeries] = useState<string>("");
  const [issueDate, setIssueDate] = useState<string>(todayISO());
  const [dueDate, setDueDate] = useState<string>(plusDaysISO(30));
  const [notes, setNotes] = useState<string>("");
  const [paymentMeansCode, setPaymentMeansCode] = useState<string>("30");
  const [lines, setLines] = useState<LineRow[]>([newLineRow()]);
  // Track the saved draft ID for live validation
  const [savedId, setSavedId] = useState<string | null>(null);
  // True when "Trimite la ANAF" was clicked — navigate to detail to trigger submit there
  const submitAfterSaveRef = useRef(false);
  const [submitError, setSubmitError] = useState<string | null>(null);

  // Live validation — runs after draft is saved
  const { data: validation, isFetching: validating } = useQuery({
    queryKey: ["validation", savedId],
    queryFn: () => api.invoices.validateDraft(savedId!),
    enabled: !!savedId,
    staleTime: 30_000,
  });

  // ANAF test mode setting — key must match backend: settings::keys::USE_ANAF_TEST_ENV
  const { data: testModeSetting } = useQuery({
    queryKey: queryKeys.anaf.testMode,
    queryFn: () => api.settings.get("use_anaf_test_env"),
  });
  const testMode = testModeSetting === "1";

  const selectedContact = contacts.find((c) => c.id === contactId) ?? null;

  const activeSeries = series || company?.invoiceSeries || "";
  const activeNumber = nextNumber ?? (company ? company.lastInvoiceNumber + 1 : 1);
  const fullNumber = activeSeries
    ? `${activeSeries}-${String(activeNumber).padStart(4, "0")}`
    : "—";

  const net = lines.reduce((s, l) => s + l.quantity * l.unitPrice, 0);
  const vat = lines.reduce((s, l) => s + l.quantity * l.unitPrice * (l.vatRate / 100), 0);
  const total = net + vat;

  const addLine = () => setLines((prev) => [...prev, newLineRow()]);

  const removeLine = (idx: number) =>
    setLines((prev) => prev.filter((_, i) => i !== idx));

  const updateLine = <K extends keyof CreateLineInput>(
    idx: number,
    key: K,
    value: CreateLineInput[K],
  ) =>
    setLines((prev) =>
      prev.map((l, i) => (i === idx ? ({ ...l, [key]: value } as LineRow) : l)),
    );

  const saveDraftMutation = useMutation({
    mutationFn: () => {
      if (!activeCompanyId) throw new Error("Nicio companie activă.");
      if (!contactId) throw new Error("Selectați un cumpărător.");
      if (lines.length === 0) throw new Error("Adăugați cel puțin o linie.");
      const lineErrors: string[] = [];
      lines.forEach((line, i) => {
        if (!line.name?.trim()) lineErrors.push(`Linia ${i + 1}: denumirea produsului/serviciului este obligatorie`);
        if ((line.quantity ?? 0) <= 0) lineErrors.push(`Linia ${i + 1}: cantitatea trebuie să fie mai mare decât 0`);
        if ((line.unitPrice ?? 0) < 0) lineErrors.push(`Linia ${i + 1}: prețul unitar nu poate fi negativ`);
        // Valid RO VAT rates: 0%, 5%, 9% (≤2025-07-31), 11% (≥2025-08-01), 19% (≤2025-07-31), 21% (≥2025-08-01)
        if (![0, 5, 9, 11, 19, 21].includes(line.vatRate ?? 21)) lineErrors.push(`Linia ${i + 1}: cota TVA trebuie să fie 0%, 5%, 9%, 11%, 19% sau 21%`);
      });
      if (lineErrors.length > 0) throw new Error(lineErrors.join("\n"));
      // Strip internal rowId before sending to backend
      const apiLines: CreateLineInput[] = lines.map(({ rowId: _rowId, ...rest }) => rest);
      return api.invoices.createDraft({
        companyId: activeCompanyId,
        contactId,
        series: activeSeries,
        number: activeNumber,
        issueDate,
        dueDate,
        currency: "RON",
        notes: notes || undefined,
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
          // Ensure we are authenticated before submitting
          const authenticated = await api.anaf.isAuthenticated(created.companyId);
          if (!authenticated) {
            await api.anaf.authorize(created.companyId);
          }
          await api.anaf.submitInvoice(created.companyId, created.id, testMode);
        } catch (e) {
          setSubmitError((e as unknown as AppErrorPayload).message ?? "Eroare la trimitere ANAF.");
          // Still navigate to detail so the user can see the invoice and retry
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
      if ((e.ctrlKey || e.metaKey) && e.key === 's') {
        e.preventDefault();
        saveDraftMutation.mutate();
      }
      if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
        e.preventDefault();
        // Submit to ANAF — save first, then auto-submit in onSuccess
        submitAfterSaveRef.current = true;
        setSubmitError(null);
        saveDraftMutation.mutate();
      }
      if ((e.ctrlKey || e.metaKey) && e.key === 'p') {
        e.preventDefault();
        window.print();
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [saveDraftMutation]);

  return (
    <div className="content">
      <div className="content-titlebar">
        <span className="content-title">
          <span className="crumb">e-Factura</span>
          <span className="crumb">Facturi emise</span>
          Factură nouă ·{" "}
          <span className="mono" style={{ fontWeight: 400, color: "var(--text-muted)" }}>
            {fullNumber}
          </span>
        </span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
          <button className="btn" onClick={() => navigate({ to: "/invoices" })}>
            <Icon name="x" size={12} /> Renunță <span className="kbd" style={{ marginLeft: 6 }}>Esc</span>
          </button>
          <button
            className="btn"
            onClick={() => saveDraftMutation.mutate()}
            disabled={saveDraftMutation.isPending}
          >
            <Icon name="draft" size={12} /> Salvează ca schiță{" "}
            <span className="kbd" style={{ marginLeft: 6 }}>Ctrl S</span>
          </button>
          <button className="btn" disabled onClick={() => window.print()}>
            <Icon name="eye" size={12} /> Previzualizare PDF{" "}
            <span className="kbd" style={{ marginLeft: 6 }}>Ctrl P</span>
          </button>
          <button
            className="btn primary"
            onClick={() => {
              submitAfterSaveRef.current = true;
              setSubmitError(null);
              saveDraftMutation.mutate();
            }}
            disabled={saveDraftMutation.isPending}
            title="Salvează și trimite la ANAF (vei fi redirecționat la pagina detaliu)"
          >
            <Icon name="cloudUp" size={12} /> Trimite la ANAF{" "}
            <span className="kbd" style={{ marginLeft: 6, opacity: 0.7 }}>Ctrl ↵</span>
          </button>
        </span>
      </div>

      {(saveDraftMutation.isError || submitError) && (
        <div style={{ padding: "8px 16px", background: "#FEE2E2", color: "#DC2626", fontSize: 12 }}>
          <Icon name="alert" size={12} />{" "}
          {submitError ??
            (saveDraftMutation.error instanceof Error
              ? saveDraftMutation.error.message
              : "Eroare la salvare.")}
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
                    value={activeSeries}
                    onChange={(e) => setSeries(e.target.value)}
                    style={{ width: 90 }}
                  />
                  <input
                    className="input mono"
                    value={String(activeNumber).padStart(4, "0")}
                    readOnly
                    style={{ width: 120 }}
                  />
                  {company && (
                    <span className="dim" style={{ fontSize: 11 }}>
                      auto-incrementat · ultima emisă: {company.invoiceSeries}-
                      {String(company.lastInvoiceNumber).padStart(4, "0")}
                    </span>
                  )}
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
                  <select
                    className="select"
                    value={contactId}
                    onChange={(e) => setContactId(e.target.value)}
                    style={{ width: 320 }}
                  >
                    <option value="">— selectați cumpărătorul —</option>
                    {contacts
                      .filter((c) => c.contactType === "CUSTOMER" || c.contactType === "BOTH")
                      .map((c) => (
                        <option key={c.id} value={c.id}>
                          {c.legalName}
                          {c.cui ? ` · ${c.cui}` : ""}
                        </option>
                      ))}
                  </select>
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
              <span style={{ display: "flex", gap: 6, alignItems: "center" }}>
                <span
                  style={{
                    fontSize: 10.5,
                    fontWeight: 400,
                    textTransform: "none",
                    letterSpacing: 0,
                    color: "var(--text-muted)",
                  }}
                >
                  Tasta <span className="kbd">↓</span> pe ultima linie creează una nouă ·{" "}
                  <span className="kbd">F4</span> deschide catalog articole
                </span>
              </span>
            </div>
            <div className="line-items">
              <table>
                <thead>
                  <tr>
                    <th style={{ width: 28 }}>#</th>
                    <th style={{ width: 110 }}>Cod</th>
                    <th>Descriere</th>
                    <th style={{ width: 64 }} className="num">Cant.</th>
                    <th style={{ width: 56 }}>UM</th>
                    <th style={{ width: 100 }} className="num">Preț unitar</th>
                    <th style={{ width: 64 }} className="num">TVA %</th>
                    <th style={{ width: 110 }} className="num">Valoare net</th>
                    <th style={{ width: 110 }} className="num">Total cu TVA</th>
                    <th style={{ width: 28 }}></th>
                  </tr>
                </thead>
                <tbody>
                  {lines.map((l, i) => {
                    const lineNet = l.quantity * l.unitPrice;
                    const lineTotal = lineNet * (1 + l.vatRate / 100);
                    return (
                      <tr key={l.rowId}>
                        <td
                          style={{
                            textAlign: "center",
                            color: "var(--text-dim)",
                            fontFamily: "var(--font-mono)",
                          }}
                        >
                          {i + 1}
                        </td>
                        <td>
                          <input
                            value={l.cpvCode ?? ""}
                            onChange={(e) => updateLine(i, "cpvCode", e.target.value || undefined)}
                            className="mono"
                          />
                        </td>
                        <td>
                          <input
                            value={l.name}
                            onChange={(e) => updateLine(i, "name", e.target.value)}
                          />
                        </td>
                        <td className="num">
                          <input
                            type="number"
                            value={l.quantity}
                            onChange={(e) => updateLine(i, "quantity", parseFloat(e.target.value) || 0)}
                            className="num"
                          />
                        </td>
                        <td>
                          <input
                            value={l.unit}
                            onChange={(e) => updateLine(i, "unit", e.target.value)}
                          />
                        </td>
                        <td className="num">
                          <input
                            type="number"
                            value={l.unitPrice}
                            onChange={(e) => updateLine(i, "unitPrice", parseFloat(e.target.value) || 0)}
                            className="num"
                          />
                        </td>
                        <td className="num">
                          <input
                            type="number"
                            value={l.vatRate}
                            onChange={(e) => updateLine(i, "vatRate", parseFloat(e.target.value) || 0)}
                            className="num"
                          />
                        </td>
                        <td className="num">
                          <input
                            value={lineNet.toFixed(2)}
                            className="num"
                            readOnly
                            style={{ color: "var(--text-muted)" }}
                          />
                        </td>
                        <td className="num">
                          <input
                            value={lineTotal.toFixed(2)}
                            className="num"
                            readOnly
                            style={{ fontWeight: 600 }}
                          />
                        </td>
                        <td>
                          <button
                            className="btn-icon"
                            onClick={() => removeLine(i)}
                            disabled={lines.length === 1}
                          >
                            <Icon name="trash" size={12} />
                          </button>
                        </td>
                      </tr>
                    );
                  })}
                  <tr className="line-add-row" onClick={addLine} style={{ cursor: "pointer" }}>
                    <td colSpan={10}>
                      <Icon name="plus" size={12} /> Adaugă linie · sau caută articol din catalog cu{" "}
                      <span className="kbd">F4</span>
                    </td>
                  </tr>
                </tbody>
                <tfoot>
                  <tr>
                    <td colSpan={6} style={{ textAlign: "right", color: "var(--text-muted)" }}>
                      Subtotal net
                    </td>
                    <td className="num"></td>
                    <td className="num tnum">{fmtRON(net)}</td>
                    <td className="num"></td>
                    <td></td>
                  </tr>
                  <tr>
                    <td colSpan={6} style={{ textAlign: "right", color: "var(--text-muted)" }}>
                      TVA
                    </td>
                    <td className="num"></td>
                    <td className="num tnum">{fmtRON(vat)}</td>
                    <td className="num"></td>
                    <td></td>
                  </tr>
                  <tr>
                    <td
                      colSpan={6}
                      style={{
                        textAlign: "right",
                        textTransform: "uppercase",
                        fontSize: 11,
                        letterSpacing: 0.04,
                      }}
                    >
                      Total de plată
                    </td>
                    <td className="num"></td>
                    <td className="num"></td>
                    <td className="num tnum" style={{ fontSize: 14, color: "var(--accent)" }}>
                      {fmtRON(total)}{" "}
                      <span style={{ fontSize: 10.5, color: "var(--text-muted)" }}>RON</span>
                    </td>
                    <td></td>
                  </tr>
                </tfoot>
              </table>
            </div>
          </div>

          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
            <div className="panel">
              <div className="panel-header">
                <span>Modalitate de plată</span>
                <span />
              </div>
              <div className="panel-body">
                <div className="form-grid" style={{ gridTemplateColumns: "120px 1fr" }}>
                  <label>Metodă</label>
                  <div className="field">
                    <select className="select" defaultValue="ot">
                      <option value="ot">Ordin de plată (OP)</option>
                      <option value="cash">Numerar</option>
                      <option value="card">Card bancar</option>
                      <option value="comp">Compensare</option>
                    </select>
                  </div>
                  <label>Cont bancar</label>
                  <div className="field">
                    <input
                      className="input mono"
                      defaultValue={company?.iban ?? ""}
                      style={{ width: 250 }}
                    />
                    {company?.bankName && (
                      <span className="muted" style={{ fontSize: 11 }}>
                        {company.bankName}
                      </span>
                    )}
                  </div>
                  <label>Referință</label>
                  <div className="field">
                    <input
                      className="input"
                      defaultValue="Plătiți în 30 zile de la data emiterii"
                    />
                  </div>
                  <label>Tip fiscal</label>
                  <div className="field">
                    <div className="seg">
                      <span className="seg-item active">Standard</span>
                      <span className="seg-item">TVA la încasare</span>
                      <span className="seg-item">Intracom.</span>
                    </div>
                  </div>
                </div>
              </div>
            </div>

            <div className="panel">
              <div className="panel-header">
                <span>Note · clauze · referințe</span>
                <span />
              </div>
              <div className="panel-body">
                <div className="form-grid" style={{ gridTemplateColumns: "120px 1fr" }}>
                  <label>Mod plată</label>
                  <div className="field">
                    <select
                      className="input"
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
        </div>

        <aside className="editor-validation">
          <div className="validation-summary">
            <h3>Validare RO_CIUS · live</h3>
            {savedId && validation ? (
              <>
                <div className="score">
                  <span className="pct">{validation.isValid ? "100" : Math.max(0, 100 - validation.errors.length * 20)}%</span>
                  <div className="validation-bar">
                    <div
                      className="fill"
                      style={{
                        width: (validation.isValid ? 100 : Math.max(0, 100 - validation.errors.length * 20)) + "%",
                        background: validation.isValid ? "var(--accent)" : validation.errors.length > 0 ? "#DC2626" : "#F59E0B",
                      }}
                    />
                  </div>
                </div>
                <div style={{ fontSize: 11, color: validation.isValid ? "#16A34A" : "var(--text-muted)", marginTop: 4 }}>
                  {validation.isValid
                    ? "✓ Validă — se poate trimite la ANAF"
                    : `${validation.errors.length} erori · ${validation.warnings.length} avertismente`}
                </div>
              </>
            ) : (
              <>
                <div className="score">
                  <span className="pct">{validating ? "…" : "—"}</span>
                  <div className="validation-bar">
                    <div className="fill" style={{ width: "0%" }} />
                  </div>
                </div>
                <div style={{ fontSize: 11, color: "var(--text-muted)", marginTop: 4 }}>
                  {validating ? "Se validează…" : "Salvați schiță pentru a valida"}
                </div>
              </>
            )}
          </div>
          <div className="validation-items">
            {savedId && validation ? (
              <>
                {validation.errors.map((msg, i) => (
                  <div key={`e${i}`} className="validation-item err">
                    <span className="ico"><Icon name="cancel" size={13} /></span>
                    <span>
                      <div className="title">Eroare</div>
                      <div className="desc">{msg}</div>
                    </span>
                  </div>
                ))}
                {validation.warnings.map((msg, i) => (
                  <div key={`w${i}`} className="validation-item warn">
                    <span className="ico"><Icon name="warning" size={13} /></span>
                    <span>
                      <div className="title">Avertisment</div>
                      <div className="desc">{msg}</div>
                    </span>
                  </div>
                ))}
                {validation.isValid && validation.errors.length === 0 && (
                  <div className="validation-item ok">
                    <span className="ico"><Icon name="check" size={13} /></span>
                    <span>
                      <div className="title">Factură validă</div>
                      <div className="desc">Toate regulile CIUS-RO sunt respectate.</div>
                    </span>
                  </div>
                )}
              </>
            ) : (
              <div className="validation-item warn">
                <span className="ico"><Icon name="warning" size={13} /></span>
                <span>
                  <div className="title">Validare indisponibilă</div>
                  <div className="desc">Completați formularul și salvați ca schiță pentru a valida.</div>
                </span>
              </div>
            )}
          </div>

          <div
            style={{
              marginTop: "auto",
              borderTop: "1px solid var(--border)",
              padding: "10px 12px",
              background: "var(--bg)",
              fontSize: 11,
              color: "var(--text-muted)",
            }}
          >
            <div
              style={{
                fontSize: 10,
                textTransform: "uppercase",
                letterSpacing: 0.1,
                color: "var(--text-dim)",
                marginBottom: 4,
              }}
            >
              Validare automată
            </div>
            Schema: <b>CIUS-RO 1.0.1</b>
          </div>
        </aside>
      </div>
    </div>
  );
}
