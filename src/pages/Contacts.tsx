/**
 * Contacte (clienți / furnizori) — date reale din backend.
 */

import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { Icon } from "@/components/shared/Icon";
import { CsvImportModal } from "@/components/shared/CsvImportModal";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtShortcut } from "@/lib/platform";
import type { Contact, ContactType, CreateContactInput, UpdateContactInput } from "@/types";

type TypeFilter = ContactType | "all";

const TYPE_LABELS: Record<ContactType, string> = {
  CUSTOMER: "Client",
  SUPPLIER: "Furnizor",
  BOTH: "Client/Furnizor",
};


export function ContactsPage() {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  const [query, setQuery] = useState("");
  const [typeFilter, setTypeFilter] = useState<TypeFilter>("all");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [modal, setModal] = useState<"create" | { edit: Contact } | null>(null);
  const [showImportModal, setShowImportModal] = useState(false);

  const { data: contacts = [], isLoading, isError: contactsError, error: contactsErr, refetch: refetchContacts } = useQuery({
    queryKey: queryKeys.contacts.list({ companyId: activeCompanyId ?? undefined }),
    queryFn: () => api.contacts.list({ companyId: activeCompanyId ?? undefined }),
  });

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

  const customers = contacts.filter((c) => c.contactType === "CUSTOMER" || c.contactType === "BOTH").length;
  const suppliers = contacts.filter((c) => c.contactType === "SUPPLIER" || c.contactType === "BOTH").length;

  const deleteMutation = useMutation({
    mutationFn: (id: string) => api.contacts.delete(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.contacts.all });
    },
  });

  const handleDelete = (c: Contact) => {
    if (!window.confirm(`Șterge contactul "${c.legalName}"? Această acțiune nu poate fi anulată.`)) return;
    deleteMutation.mutate(c.id);
  };

  const toggleAll = () => {
    setSelected(selected.size === list.length ? new Set() : new Set(list.map((c) => c.id)));
  };
  const toggleOne = (id: string) => {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id); else next.add(id);
    setSelected(next);
  };

  return (
    <div className="content">
      <div className="content-titlebar">
        <span className="content-title">
          <span className="crumb">Date</span>
          {t('contacts.title')}
        </span>
        <span className="muted" style={{ fontSize: 11 }}>
          {list.length} din {contacts.length} contacte
        </span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
          <button type="button" className="btn" onClick={() => setShowImportModal(true)}>
            <Icon name="upload" size={12} /> {t('contacts.importCsv')}
          </button>
          <button type="button" className="btn primary" onClick={() => setModal("create")}>
            <Icon name="plus" size={12} /> {t('contacts.newContact')}
          </button>
        </span>
      </div>

      <div className="views-bar">
        <span className={"view-tab " + (typeFilter === "all" ? "active" : "")} onClick={() => setTypeFilter("all")}>
          Toate <span className="count">{contacts.length}</span>
        </span>
        <span className={"view-tab " + (typeFilter === "CUSTOMER" ? "active" : "")} onClick={() => setTypeFilter("CUSTOMER")}>
          Clienți <span className="count">{customers}</span>
        </span>
        <span className={"view-tab " + (typeFilter === "SUPPLIER" ? "active" : "")} onClick={() => setTypeFilter("SUPPLIER")}>
          Furnizori <span className="count">{suppliers}</span>
        </span>
      </div>

      <div className="content-toolbar">
        <div className="search">
          <Icon name="search" size={13} />
          <input
            placeholder="Caută după nume, CUI sau localitate…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          <span className="kbd-hint">{fmtShortcut("Ctrl F")}</span>
        </div>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6, alignItems: "center" }}>
          {selected.size > 0 && (
            <span style={{ fontSize: 11, fontWeight: 600 }}>{selected.size} selectate</span>
          )}
          <button
            type="button"
            className="btn-icon"
            title="Reîmprospătează"
            onClick={() => void queryClient.invalidateQueries({ queryKey: queryKeys.contacts.all })}
          >
            <Icon name="refresh" size={14} />
          </button>
        </span>
      </div>

      <div className="content-body">
        {isLoading ? (
          <div style={{ padding: 24, fontSize: 12, color: "var(--text-muted)" }}>Se încarcă…</div>
        ) : contactsError ? (
          <QueryErrorBanner error={contactsErr} label="contactele" onRetry={() => void refetchContacts()} />
        ) : list.length === 0 ? (
          <div style={{ padding: 40, textAlign: "center", fontSize: 12, color: "var(--text-muted)" }}>
            {contacts.length === 0
              ? "Niciun contact. Adaugă primul client sau furnizor."
              : "Niciun rezultat pentru filtrele aplicate."}
          </div>
        ) : (
          <table className="dt">
            <thead>
              <tr>
                <th className="ck">
                  <input
                    type="checkbox"
                    className="cbx"
                    checked={selected.size === list.length && list.length > 0}
                    onChange={toggleAll}
                  />
                </th>
                <th style={{ width: 110 }}>CUI</th>
                <th>Denumire</th>
                <th style={{ width: 80 }}>Tip</th>
                <th style={{ width: 130 }}>Localitate</th>
                <th style={{ width: 60 }}>Județ</th>
                <th style={{ width: 64 }} className="num">TVA</th>
                <th style={{ width: 170 }}>Email</th>
                <th style={{ width: 90 }}>Acțiuni</th>
              </tr>
            </thead>
            <tbody>
              {list.map((c: Contact) => (
                <tr key={c.id}>
                  <td className="ck" onClick={(e) => e.stopPropagation()}>
                    <input type="checkbox" className="cbx" checked={selected.has(c.id)} onChange={() => toggleOne(c.id)} />
                  </td>
                  <td className="mono">{c.cui ?? <span className="dim">—</span>}</td>
                  <td><b>{c.legalName}</b></td>
                  <td>
                    <span className={"badge " + (c.contactType === "CUSTOMER" ? "validated" : c.contactType === "SUPPLIER" ? "pending" : "info")}>
                      {TYPE_LABELS[c.contactType]}
                    </span>
                  </td>
                  <td>{c.city ?? <span className="dim">—</span>}</td>
                  <td className="mono">{c.county ?? <span className="dim">—</span>}</td>
                  <td className="num">
                    {c.vatPayer ? (
                      <span style={{ color: "#16A34A", display: "inline-flex" }}><Icon name="check" size={13} /></span>
                    ) : (
                      <span className="dim"><Icon name="x" size={13} /></span>
                    )}
                  </td>
                  <td className="muted" style={{ fontSize: 11 }}>{c.email ?? <span className="dim">—</span>}</td>
                  <td onClick={(e) => e.stopPropagation()}>
                    <button
                      type="button"
                      className="btn-icon"
                      title="Editează"
                      onClick={() => setModal({ edit: c })}
                    >
                      <Icon name="pen" size={13} />
                    </button>
                    <button
                      type="button"
                      className="btn-icon"
                      title="Șterge"
                      onClick={() => handleDelete(c)}
                    >
                      <Icon name="x" size={13} />
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      <div style={{ padding: "6px 14px", borderTop: "1px solid var(--border)", background: "var(--bg)", display: "flex", gap: 16, fontSize: 11, color: "var(--text-muted)" }}>
        <span>Total: <b style={{ color: "var(--text)" }}>{list.length}</b> contacte</span>
        <span>Clienți: <b style={{ color: "var(--text)" }}>{customers}</b></span>
        <span>Furnizori: <b style={{ color: "var(--text)" }}>{suppliers}</b></span>
      </div>

      {modal && (
        <ContactModal
          companyId={activeCompanyId ?? ""}
          contact={modal === "create" ? null : modal.edit}
          onClose={() => setModal(null)}
          onSaved={() => {
            void queryClient.invalidateQueries({ queryKey: queryKeys.contacts.all });
            setModal(null);
          }}
        />
      )}

      {showImportModal && (
        <CsvImportModal
          type="contacts"
          companyId={activeCompanyId ?? ""}
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
  const [form, setForm] = useState<CreateContactInput>({
    companyId,
    contactType: contact?.contactType ?? "CUSTOMER",
    cui: contact?.cui ?? "",
    legalName: contact?.legalName ?? "",
    vatPayer: contact?.vatPayer ?? false,
    address: contact?.address ?? "",
    city: contact?.city ?? "",
    county: contact?.county ?? "",
    country: contact?.country ?? "RO",
    email: contact?.email ?? "",
    phone: contact?.phone ?? "",
  });
  const [error, setError] = useState<string | null>(null);

  const create = useMutation({
    mutationFn: (input: CreateContactInput) => api.contacts.create(input),
    onSuccess: onSaved,
    onError: (e) => setError((e as { message?: string }).message ?? "Eroare."),
  });
  const update = useMutation({
    mutationFn: (input: UpdateContactInput) => api.contacts.update(contact!.id, input),
    onSuccess: onSaved,
    onError: (e) => setError((e as { message?: string }).message ?? "Eroare."),
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
    if (!form.legalName.trim()) { setError("Denumirea este obligatorie."); return; }
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
    };
    if (isEdit) {
      const { companyId: _cid, ...updateInput } = input;
      update.mutate(updateInput);
    } else {
      create.mutate(input);
    }
  };

  return (
    <div className="palette-scrim" style={{ alignItems: "center", paddingTop: 0 }} onClick={onClose}>
      <div
        style={{ width: 420, background: "var(--bg-content)", border: "1px solid var(--border-strong)", boxShadow: "0 4px 24px rgba(0,0,0,0.12)", padding: "20px 24px 18px", maxHeight: "90vh", overflowY: "auto" }}
        onClick={(e) => e.stopPropagation()}
      >
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 16 }}>
          <h3 style={{ fontSize: 14, fontWeight: 700, margin: 0 }}>
            {isEdit ? `Editează: ${contact.legalName}` : "Contact nou"}
          </h3>
          <button type="button" className="btn-icon" onClick={onClose}><Icon name="x" size={14} /></button>
        </div>

        <form onSubmit={handleSubmit} style={{ display: "flex", flexDirection: "column", gap: 9 }}>
          <div style={{ display: "flex", gap: 9 }}>
            <MField label="Tip *" style={{ flex: 1 }}>
              <select className="field" value={form.contactType} onChange={(e) => setForm((f) => ({ ...f, contactType: e.target.value as ContactType }))}>
                <option value="CUSTOMER">Client</option>
                <option value="SUPPLIER">Furnizor</option>
                <option value="BOTH">Client/Furnizor</option>
              </select>
            </MField>
            <MField label="CUI" style={{ flex: 1 }}>
              <input className="field" placeholder="ex. RO12345678" style={{ fontFamily: "var(--font-mono)" }} {...field("cui")} />
            </MField>
          </div>

          <MField label="Denumire legală *">
            <input className="field" placeholder="S.C. Exemplu S.R.L." {...field("legalName")} />
          </MField>

          <div style={{ display: "flex", gap: 9 }}>
            <MField label="Localitate" style={{ flex: 2 }}>
              <input className="field" placeholder="Cluj-Napoca" {...field("city")} />
            </MField>
            <MField label="Județ" style={{ flex: 1 }}>
              <input className="field" placeholder="CJ" maxLength={2} style={{ fontFamily: "var(--font-mono)", textTransform: "uppercase" }} {...field("county")} />
            </MField>
          </div>

          <MField label="Adresă">
            <input className="field" placeholder="Str. Exemplu nr. 1" {...field("address")} />
          </MField>

          <div style={{ display: "flex", gap: 9 }}>
            <MField label="Email" style={{ flex: 1 }}>
              <input className="field" type="email" placeholder="office@firma.ro" {...field("email")} />
            </MField>
            <MField label="Telefon" style={{ flex: 1 }}>
              <input className="field" placeholder="+40 722..." {...field("phone")} />
            </MField>
          </div>

          <div style={{ display: "flex", alignItems: "center", gap: 8, paddingTop: 2 }}>
            <input
              id="m-vatPayer"
              type="checkbox"
              className="cbx"
              checked={form.vatPayer as boolean}
              onChange={(e) => setForm((f) => ({ ...f, vatPayer: e.target.checked }))}
            />
            <label htmlFor="m-vatPayer" style={{ fontSize: 12, cursor: "pointer", userSelect: "none" }}>
              Plătitor de TVA
            </label>
          </div>

          {error && (
            <div style={{ padding: "6px 10px", background: "#FEE2E2", border: "1px solid #FECACA", fontSize: 11, color: "#991B1B" }}>
              {error}
            </div>
          )}

          <div style={{ display: "flex", gap: 8, justifyContent: "flex-end", marginTop: 4 }}>
            <button type="button" className="btn" onClick={onClose}>Anulează</button>
            <button type="submit" className="btn primary" disabled={isPending}>
              {isPending ? "Se salvează…" : isEdit ? "Salvează" : "Adaugă"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

function MField({
  label,
  children,
  style,
}: {
  label: string;
  children: React.ReactNode;
  style?: React.CSSProperties;
}) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 3, ...style }}>
      <label style={{ fontSize: 11, fontWeight: 600, color: "var(--text-muted)" }}>{label}</label>
      {children}
    </div>
  );
}
