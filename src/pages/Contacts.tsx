/**
 * Clienți & Furnizori — verbatim port of the design "Clienti si furnizori.html":
 *   .page-head (title + count sub + refresh sq-btn + btn-dark "Contact nou")
 *   .scr-card → .scr-toolbar (.tabs Toți/Clienți/Furnizori/Ambele · .spacer ·
 *   .scr-search) → .scr-table (CUI .doc · .cli + .cli-ava · chip Tip ·
 *   Localitate · Județ · TVA .pos ✓ · Email · .row-acts pen/trash) → .pager
 *   → modal .modal-back/.modal.lg (fgrid: CUI cu autofill ANAF, tip, denumire,
 *   persoană fizică, plătitor TVA, TVA la încasare, monedă, adresă…).
 *
 * ALL wiring preserved: api.contacts.list({companyId}), tip filter, search,
 * create/edit modal → api.contacts.create / api.contacts.update,
 * ANAF CUI autofill → api.companies.fetchAnafData (+ inactive / cash-VAT /
 * e-Factura status), delete confirm → api.contacts.delete, Import CSV →
 * CsvImportModal.
 */

import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { confirm } from "@tauri-apps/plugin-dialog";

import { Ic } from "@/components/shared/Ic";
import { CsvImportModal } from "@/components/shared/CsvImportModal";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";
import type { Contact, ContactType, CreateContactInput, UpdateContactInput } from "@/types";
import { COUNTRIES, CURRENCIES } from "@/lib/constants";

type TypeFilter = ContactType | "all";

const TYPE_LABELS: Record<ContactType, string> = {
  CUSTOMER: "Client",
  SUPPLIER: "Furnizor",
  BOTH: "Ambele",
};

/** Rows per pager page (design .pager parity — client-side). */
const PAGE_SIZE = 50;

/** Trash icon — not in Ic's set; inlined verbatim from the prototype. */
const TRASH_PATH =
  '<path d="m14.74 9-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 0 1-2.244 2.077H8.084a2.25 2.25 0 0 1-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 0 0-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 0 1 3.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 0 0-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 0 0-7.5 0"/>';

/** Check-circle (ANAF ok) icon — not in Ic's set; inlined verbatim from the prototype. */
const OK_CIRCLE_PATH = '<path d="M9 12.75 11.25 15 15 9.75M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z"/>';

/** Avatar initials for the .cli-ava chip. */
const ini = (s: string | undefined) =>
  (s ?? "—").replace(/[^A-Za-zĂÂÎȘȚăâîșț ]/g, "").split(/\s+/).filter(Boolean).map((w) => w[0]).join("").slice(0, 2).toUpperCase() || "—";

export function ContactsPage() {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const queryClient = useQueryClient();

  const [query, setQuery] = useState("");
  const [typeFilter, setTypeFilter] = useState<TypeFilter>("all");
  const [page, setPage] = useState(1);
  const [modal, setModal] = useState<"create" | { edit: Contact } | null>(null);
  const [showImportModal, setShowImportModal] = useState(false);

  const {
    data: contacts = [],
    isLoading,
    isError: contactsError,
    error: contactsErr,
    refetch: refetchContacts,
  } = useQuery({
    queryKey: queryKeys.contacts.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () => api.contacts.list({ companyId: activeCompanyId ?? undefined }),
    enabled: !!activeCompanyId,
  });

  const { data: companies = [] } = useQuery({
    queryKey: queryKeys.companies.list(),
    queryFn: () => api.companies.list(),
  });
  const activeCompany = companies.find((c) => c.id === activeCompanyId);

  const list = useMemo(() => {
    const q = query.trim().toLowerCase();
    return contacts
      .filter(
        (c) =>
          !q ||
          c.legalName.toLowerCase().includes(q) ||
          (c.cui ?? "").toLowerCase().includes(q) ||
          (c.city ?? "").toLowerCase().includes(q),
      )
      .filter((c) => typeFilter === "all" || c.contactType === typeFilter);
  }, [contacts, query, typeFilter]);

  // Reset to the first page when filters change
  useEffect(() => {
    setPage(1);
  }, [query, typeFilter]);

  const counts = {
    CUSTOMER: contacts.filter((c) => c.contactType === "CUSTOMER").length,
    SUPPLIER: contacts.filter((c) => c.contactType === "SUPPLIER").length,
    BOTH: contacts.filter((c) => c.contactType === "BOTH").length,
  };

  const totalPages = Math.max(1, Math.ceil(list.length / PAGE_SIZE));
  const curPage = Math.min(page, totalPages);
  const pageRows = list.slice((curPage - 1) * PAGE_SIZE, curPage * PAGE_SIZE);
  const rangeStart = list.length === 0 ? 0 : (curPage - 1) * PAGE_SIZE + 1;
  const rangeEnd = Math.min(curPage * PAGE_SIZE, list.length);

  // Numbered page buttons — window of max 5 around the current page
  const pageNums = useMemo(() => {
    const win = 5;
    let start = Math.max(1, curPage - Math.floor(win / 2));
    const end = Math.min(totalPages, start + win - 1);
    start = Math.max(1, end - win + 1);
    const out: number[] = [];
    for (let p = start; p <= end; p++) out.push(p);
    return out;
  }, [curPage, totalPages]);

  const deleteMutation = useMutation({
    mutationFn: (id: string) => {
      if (!activeCompanyId) return Promise.reject(new Error("Nicio companie activă."));
      return api.contacts.delete(id, activeCompanyId);
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.contacts.all });
    },
    onError: (e) => notify.error(formatError(e, "Nu s-a putut șterge contactul.")),
  });

  const handleDelete = async (c: Contact) => {
    if (!activeCompanyId) return;
    const ok = await confirm(
      `Șterge contactul "${c.legalName}"? Această acțiune nu poate fi anulată.`,
      { title: "Confirmare ștergere", kind: "warning" },
    );
    if (!ok) return;
    deleteMutation.mutate(c.id);
  };

  const tabs: Array<{ value: TypeFilter; label: string; count: number }> = [
    { value: "all",      label: "Toți",      count: contacts.length },
    { value: "CUSTOMER", label: "Clienți",   count: counts.CUSTOMER },
    { value: "SUPPLIER", label: "Furnizori", count: counts.SUPPLIER },
    { value: "BOTH",     label: "Ambele",    count: counts.BOTH },
  ];

  if (!activeCompanyId) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>Clienți &amp; Furnizori</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          Selectați o companie activă pentru a vedea contactele.
        </div>
      </div>
    );
  }

  return (
    <div className="main-inner wide">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>Clienți &amp; Furnizori</h1>
          <p className="sub">
            {contacts.length.toLocaleString("ro-RO")} contacte
            {activeCompany ? ` · ${activeCompany.legalName}` : ""}
          </p>
        </div>
        <div className="head-actions">
          <button className="sq-btn spin-btn" title="Reîmprospătează" onClick={() => void refetchContacts()}>
            <Ic name="sync" />
          </button>
          <button className="pill-btn" onClick={() => setShowImportModal(true)}>
            <Ic name="docUp" />Import CSV
          </button>
          <button className="btn-dark" onClick={() => setModal("create")}>
            <Ic name="plus" />Contact nou
          </button>
        </div>
      </div>

      <div className="scr-card">
        {/* toolbar */}
        <div className="scr-toolbar">
          <div className="tabs">
            {tabs.map((t) => (
              <div
                key={t.value}
                className={`tab${typeFilter === t.value ? " active" : ""}`}
                onClick={() => setTypeFilter(t.value)}
              >
                {t.label}<span className="cnt num">{t.count}</span>
              </div>
            ))}
          </div>
          <div className="spacer" />
          <div className="scr-search">
            <Ic name="lens" />
            <input
              type="text"
              placeholder="Nume sau CUI…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
          </div>
        </div>

        {/* table */}
        {isLoading ? (
          <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>Se încarcă…</div>
        ) : contactsError ? (
          <div style={{ padding: 16 }}>
            <QueryErrorBanner error={contactsErr} label="contactele" onRetry={() => void refetchContacts()} />
          </div>
        ) : list.length === 0 ? (
          <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            {contacts.length === 0
              ? "Niciun contact. Adăugați primul client sau furnizor cu butonul „Contact nou”."
              : "Niciun rezultat pentru filtrele aplicate."}
          </div>
        ) : (
          <>
            <table className="scr-table">
              <thead>
                <tr>
                  <th style={{ width: 120 }}>CUI</th>
                  <th>Denumire</th>
                  <th style={{ width: 110 }}>Tip</th>
                  <th style={{ width: 140 }}>Localitate</th>
                  <th style={{ width: 60 }}>Județ</th>
                  <th style={{ width: 60, textAlign: "center" }}>TVA</th>
                  <th style={{ width: 200 }}>Email</th>
                  <th className="r" style={{ width: 90 }}></th>
                </tr>
              </thead>
              <tbody>
                {pageRows.map((c) => (
                  <tr key={c.id}>
                    <td>{c.cui ? <span className="doc">{c.cui}</span> : <span className="muted">—</span>}</td>
                    <td>
                      <div className="cli">
                        <span className="cli-ava">{ini(c.legalName)}</span>
                        {c.legalName}
                        {c.isIndividual && (
                          <span className="chip sent" style={{ marginLeft: 6 }}>Persoană fizică</span>
                        )}
                      </div>
                    </td>
                    <td><span className="chip sent">{TYPE_LABELS[c.contactType]}</span></td>
                    <td>{c.city ?? <span className="muted">—</span>}</td>
                    <td>{c.county ?? <span className="muted">—</span>}</td>
                    <td style={{ textAlign: "center" }}>
                      {c.vatPayer ? <span className="pos">✓</span> : <span className="muted">—</span>}
                    </td>
                    <td className="muted">{c.email ?? "—"}</td>
                    <td>
                      <div className="row-acts">
                        <button className="mini-btn" title="Editează" onClick={() => setModal({ edit: c })}>
                          <Ic name="pen" />
                        </button>
                        <button className="mini-btn" title="Șterge" onClick={() => void handleDelete(c)}>
                          <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: TRASH_PATH }} />
                        </button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>

            {/* pager */}
            <div className="pager">
              <span>
                Afișezi <b>{rangeStart}–{rangeEnd}</b> din <b>{list.length.toLocaleString("ro-RO")}</b> contacte
              </span>
              <div className="pg-btns">
                <button
                  className="pg-btn"
                  disabled={curPage <= 1}
                  onClick={() => setPage(curPage - 1)}
                  aria-label="Pagina anterioară"
                >
                  {/* chevron-left — not in Ic's set; inlined verbatim */}
                  <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: '<path d="M15.75 19.5 8.25 12l7.5-7.5"/>' }} />
                </button>
                {pageNums.map((p) => (
                  <button
                    key={p}
                    className={`pg-btn${p === curPage ? " cur" : ""}`}
                    onClick={() => setPage(p)}
                  >
                    {p}
                  </button>
                ))}
                <button
                  className="pg-btn"
                  disabled={curPage >= totalPages}
                  onClick={() => setPage(curPage + 1)}
                  aria-label="Pagina următoare"
                >
                  <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: '<path d="m8.25 4.5 7.5 7.5-7.5 7.5"/>' }} />
                </button>
              </div>
            </div>
          </>
        )}
      </div>

      {/* Contact modal */}
      {modal !== null && (
        <ContactModal
          companyId={activeCompanyId}
          contact={modal === "create" ? null : modal.edit}
          onClose={() => setModal(null)}
          onSaved={() => {
            void queryClient.invalidateQueries({ queryKey: queryKeys.contacts.all });
            setModal(null);
          }}
        />
      )}

      {/* CSV Import */}
      {showImportModal && (
        <CsvImportModal
          type="contacts"
          companyId={activeCompanyId}
          onClose={() => setShowImportModal(false)}
          onSuccess={() => {
            void queryClient.invalidateQueries({ queryKey: queryKeys.contacts.all });
            setShowImportModal(false);
          }}
        />
      )}
    </div>
  );
}

// ─── ContactModal — design .modal-back/.modal.lg with .fgrid fields ──────────

function ContactModal({
  companyId,
  contact,
  onClose,
  onSaved,
}: {
  companyId: string;
  contact: Contact | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const isEdit = contact !== null;
  const [currency, setCurrency] = useState<string>(contact?.currency ?? "RON");
  const [form, setForm] = useState<CreateContactInput>({
    companyId,
    contactType: contact?.contactType ?? "CUSTOMER",
    cui: contact?.cui ?? "",
    legalName: contact?.legalName ?? "",
    vatPayer: contact?.vatPayer ?? false,
    isIndividual: contact?.isIndividual ?? false,
    cashVat: contact?.cashVat ?? false,
    address: contact?.address ?? "",
    city: contact?.city ?? "",
    county: contact?.county ?? "",
    country: contact?.country ?? "RO",
    email: contact?.email ?? "",
    phone: contact?.phone ?? "",
  });
  const [error, setError] = useState<string | null>(null);

  // ANAF CUI lookup → auto-fill the form (name/address/vatPayer/cashVat) + surface inactive /
  // cash-VAT / e-Factura status. Fired on the CUI field's blur (valid RO CUI) or the button.
  const [anafInfo, setAnafInfo] = useState<
    { inactive: boolean; cashVat: boolean; efactura: boolean } | null
  >(null);
  const [lastLookedUp, setLastLookedUp] = useState<string>("");
  const anafLookup = useMutation({
    mutationFn: (cui: string) => api.companies.fetchAnafData(cui),
    onSuccess: (d) => {
      setForm((f) => ({
        ...f,
        legalName: d.legalName || f.legalName,
        address: d.address || f.address,
        city: d.city || f.city,
        county: d.county || f.county,
        vatPayer: d.vatPayer,
        cashVat: d.cashVat,
      }));
      setAnafInfo({ inactive: !d.active, cashVat: d.cashVat, efactura: d.efacturaRegistered });
      notify.success(`Date preluate din ANAF: ${d.legalName}`);
    },
    onError: () => {
      setAnafInfo(null);
      notify.error("CUI-ul nu a fost găsit în baza ANAF.");
    },
  });

  const triggerAnafLookup = () => {
    const raw = (form.cui ?? "").trim();
    const clean = raw.toUpperCase().replace(/^RO/, "");
    // Only for a RO-format CUI on a non-individual; skip duplicate lookups of the same value.
    if (form.isIndividual || !/^\d{2,10}$/.test(clean) || clean === lastLookedUp) return;
    setLastLookedUp(clean);
    anafLookup.mutate(raw);
  };

  const create = useMutation({
    mutationFn: (input: CreateContactInput) => api.contacts.create(input),
    onSuccess: onSaved,
    onError: (e) => setError(formatError(e, "Eroare la creare.")),
  });

  const update = useMutation({
    mutationFn: (input: UpdateContactInput) =>
      api.contacts.update(contact!.id, companyId, input),
    onSuccess: onSaved,
    onError: (e) => setError(formatError(e, "Eroare la salvare.")),
  });

  const isPending = create.isPending || update.isPending;

  const field = (key: keyof CreateContactInput) => ({
    value: (form[key] as string) ?? "",
    onChange: (e: React.ChangeEvent<HTMLInputElement | HTMLSelectElement>) =>
      setForm((f) => ({ ...f, [key]: e.target.value })),
  });

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    if (!form.legalName.trim()) {
      setError("Denumirea este obligatorie.");
      return;
    }
    if (form.cui?.trim()) {
      const cuiClean = form.cui.trim().toUpperCase().replace(/^RO/, "");
      if (!/^\d{2,10}$/.test(cuiClean)) {
        setError("CUI invalid — trebuie să conțină 2-10 cifre (ex: RO12345678 sau 12345678)");
        return;
      }
    }
    const input: CreateContactInput = {
      ...form,
      cui: form.cui?.trim() || undefined,
      address: form.address?.trim() || undefined,
      city: form.city?.trim() || undefined,
      county: form.county?.trim() || undefined,
      email: form.email?.trim() || undefined,
      phone: form.phone?.trim() || undefined,
      currency: currency || undefined,
    };
    if (isEdit) {
      const { companyId: _cid, ...updateInput } = input;
      update.mutate(updateInput as UpdateContactInput);
    } else {
      create.mutate(input);
    }
  };

  // Da/Nu boolean selects (design .select parity)
  const boolSelect = (key: "isIndividual" | "vatPayer" | "cashVat") => ({
    value: form[key] ? "da" : "nu",
    onChange: (e: React.ChangeEvent<HTMLSelectElement>) =>
      setForm((f) => ({ ...f, [key]: e.target.value === "da" })),
  });

  return createPortal(
    <div
      className="modal-back show"
      style={{ position: "fixed" }}
      onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}
    >
      <div className="modal lg">
        <div className="modal-head">
          <div>
            <div className="mt">{isEdit ? `Editează: ${contact.legalName}` : "Contact nou"}</div>
            <div className="ms">
              Datele se completează automat din registrul ANAF după CUI (denumire, adresă, TVA,
              TVA la încasare, stare activă)
            </div>
          </div>
          <button className="modal-x" onClick={onClose} aria-label="Închide">
            <Ic name="xMark" />
          </button>
        </div>
        <form id="contact-form" className="modal-body" onSubmit={handleSubmit}>
          <div className="fgrid">
            <div className="field">
              <label>CUI / CIF</label>
              <div style={{ display: "flex", gap: 6 }}>
                <div className="in-wrap" style={{ flex: 1 }}>
                  <input
                    className={`input num${anafInfo ? " valid" : ""}`}
                    type="text"
                    placeholder="ex. RO12345678"
                    {...field("cui")}
                    onBlur={triggerAnafLookup}
                  />
                  {anafInfo && !anafInfo.inactive && (
                    <svg className="in-ic ok" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: OK_CIRCLE_PATH }} />
                  )}
                </div>
                <button
                  type="button"
                  className="pill-btn"
                  style={{ height: 36, flex: "none" }}
                  disabled={anafLookup.isPending || (form.isIndividual as boolean)}
                  title="Preia datele firmei din ANAF după CUI"
                  onClick={() => {
                    setLastLookedUp("");
                    triggerAnafLookup();
                  }}
                >
                  {anafLookup.isPending ? "…" : "ANAF ↓"}
                </button>
              </div>
              {anafInfo && (
                <span className={anafInfo.inactive ? "err" : "okk"} style={anafInfo.inactive ? { fontSize: 11.5, color: "var(--red)" } : undefined}>
                  Găsit în ANAF · {anafInfo.inactive ? "INACTIV" : "activ"}
                  {anafInfo.efactura ? " · înregistrat în Registrul RO e-Factura" : ""}
                </span>
              )}
            </div>
            <div className="field">
              <label>Tip contact</label>
              <select
                className="select"
                value={form.contactType}
                onChange={(e) => setForm((f) => ({ ...f, contactType: e.target.value as ContactType }))}
              >
                <option value="CUSTOMER">Client</option>
                <option value="SUPPLIER">Furnizor</option>
                <option value="BOTH">Ambele</option>
              </select>
            </div>
            <div className="field span2">
              <label>Denumire <span className="req">*</span></label>
              <input className="input" type="text" placeholder="S.C. Exemplu S.R.L." {...field("legalName")} autoFocus />
            </div>
            <div className="field">
              <label>Persoană fizică (B2C)</label>
              <select className="select" {...boolSelect("isIndividual")}>
                <option value="da">Da</option>
                <option value="nu">Nu</option>
              </select>
              <span className="hint">persoanele fizice nu necesită CUI</span>
            </div>
            <div className="field">
              <label>Plătitor TVA</label>
              <select className="select" {...boolSelect("vatPayer")}>
                <option value="da">Da</option>
                <option value="nu">Nu</option>
              </select>
            </div>
            <div className="field">
              <label>TVA la încasare</label>
              <select className="select" {...boolSelect("cashVat")}>
                <option value="da">Da</option>
                <option value="nu">Nu</option>
              </select>
              <span className="hint">preluat din ANAF — amână deducerea până la plată (art. 297)</span>
            </div>
            <div className="field">
              <label>Monedă implicită</label>
              <select className="select" value={currency} onChange={(e) => setCurrency(e.target.value)}>
                {CURRENCIES.map((c) => (
                  <option key={c} value={c}>{c}</option>
                ))}
              </select>
            </div>
            <div className="field span2">
              <label>Adresă</label>
              <input className="input" type="text" placeholder="Str. Exemplu nr. 1" {...field("address")} />
            </div>
            <div className="field">
              <label>Localitate</label>
              <input className="input" type="text" placeholder="Cluj-Napoca" {...field("city")} />
            </div>
            <div className="field">
              <label>Județ</label>
              <input className="input" type="text" placeholder="CJ" {...field("county")} />
            </div>
            {/* Țară — real field the prototype lacks; kept (design .select) */}
            <div className="field">
              <label>Țară</label>
              <select
                className="select"
                value={form.country ?? "RO"}
                onChange={(e) => setForm((f) => ({ ...f, country: e.target.value }))}
              >
                {COUNTRIES.map((c) => (
                  <option key={c.code} value={c.code}>{c.name} ({c.code})</option>
                ))}
              </select>
            </div>
            <div className="field">
              <label>Email</label>
              <input className="input" type="text" placeholder="opțional" {...field("email")} />
            </div>
            <div className="field">
              <label>Telefon</label>
              <input className="input num" type="text" placeholder="opțional" {...field("phone")} />
            </div>
          </div>

          {/* ANAF status notes (real functionality — restyled with design tokens) */}
          {anafInfo?.inactive && (
            <div style={{ marginTop: 12, padding: "8px 12px", borderRadius: 8, fontSize: 12.5, color: "var(--red)", background: "rgba(220,38,38,.06)", border: "1px solid rgba(220,38,38,.18)" }}>
              Contribuabil <b>INACTIV</b> la ANAF — facturile primite au deductibilitate
              restricționată pentru cheltuieli și TVA (art. 11 Cod fiscal). Verificați înainte de a
              înregistra achiziții.
            </div>
          )}
          {anafInfo?.cashVat && (
            <div style={{ marginTop: 12, padding: "8px 12px", borderRadius: 8, fontSize: 12.5, color: "var(--amber)", background: "rgba(180,83,9,.07)", border: "1px solid rgba(180,83,9,.18)" }}>
              Furnizor cu <b>TVA la încasare</b> — TVA deductibilă se amână până la plata facturii
              (art. 297 Cod fiscal).
            </div>
          )}

          {error && (
            <div style={{ marginTop: 12, padding: "8px 12px", borderRadius: 8, fontSize: 12.5, color: "var(--red)", background: "rgba(220,38,38,.06)", border: "1px solid rgba(220,38,38,.18)" }}>
              {error}
            </div>
          )}
        </form>
        <div className="modal-foot">
          <button className="pill-btn" onClick={onClose} disabled={isPending}>Renunță</button>
          <button
            className="btn-dark"
            type="submit"
            form="contact-form"
            disabled={isPending}
            style={isPending ? { opacity: 0.6 } : undefined}
          >
            <Ic name="check" />
            {isPending ? "Se salvează…" : "Salvează contact"}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
