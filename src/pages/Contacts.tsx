/**
 * Contacte (clienți / furnizori) — re-skinned to rf kit (Wave 3).
 * Preserves 100% of wiring: api.contacts.list({companyId}),
 * type filter Toți/Clienți/Furnizori, search, create/edit modal
 * → api.contacts.create / api.contacts.update(id, companyId, input),
 * delete confirm → api.contacts.delete(id, companyId),
 * Import CSV → CsvImportModal, multi-select.
 */

import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { confirm } from "@tauri-apps/plugin-dialog";

import { Icon } from "@/components/shared/Icon";
import { CsvImportModal } from "@/components/shared/CsvImportModal";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import {
  PageHeader, Btn, IconBtn, Badge, Card, Field, Input, Select,
  Segmented, SearchInput, Empty, Modal, Banner,
} from "@/components/rf";
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
  BOTH: "Client/Furnizor",
};

const TYPE_VARIANT: Record<ContactType, "info" | "neutral" | "warning"> = {
  CUSTOMER: "info",
  SUPPLIER: "neutral",
  BOTH: "warning",
};

export function ContactsPage() {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const queryClient = useQueryClient();

  const [query, setQuery] = useState("");
  const [typeFilter, setTypeFilter] = useState<TypeFilter>("all");
  const [selected, setSelected] = useState<Set<string>>(new Set());
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

  const customers = contacts.filter(
    (c) => c.contactType === "CUSTOMER" || c.contactType === "BOTH",
  ).length;
  const suppliers = contacts.filter(
    (c) => c.contactType === "SUPPLIER" || c.contactType === "BOTH",
  ).length;

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

  const toggleAll = () => {
    setSelected(
      selected.size === list.length ? new Set() : new Set(list.map((c) => c.id)),
    );
  };
  const toggleOne = (id: string) => {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    setSelected(next);
  };

  const filterOptions = [
    { value: "all" as TypeFilter, label: `Toți (${contacts.length})` },
    { value: "CUSTOMER" as TypeFilter, label: `Clienți (${customers})` },
    { value: "SUPPLIER" as TypeFilter, label: `Furnizori (${suppliers})` },
  ];

  if (!activeCompanyId) {
    return (
      <div className="rf-page">
        <PageHeader title="Clienți & Furnizori" />
        <div className="rf-page-body">
          <Card pad>
            <Empty icon="users" title="Selectați o companie activă pentru a vedea contactele." />
          </Card>
        </div>
      </div>
    );
  }

  return (
    <div className="rf-page">
      <PageHeader
        title="Clienți & Furnizori"
        sub={<Badge variant="neutral" dot={false}>{contacts.length} contacte</Badge>}
        actions={
          <>
            <Btn
              variant="secondary"
              icon="upload"
              size="sm"
              onClick={() => setShowImportModal(true)}
            >
              Import CSV
            </Btn>
            <Btn
              variant="primary"
              icon="plus"
              size="sm"
              onClick={() => setModal("create")}
            >
              Contact nou
            </Btn>
          </>
        }
      />

      <div className="rf-page-body">
        <Card>
          {/* Toolbar */}
          <div className="rf-toolbar-row" style={{ padding: "10px 16px", borderBottom: "1px solid var(--rf-border)" }}>
            <SearchInput
              placeholder="Caută după nume, CUI sau localitate…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              style={{ width: 300 }}
            />
            <Segmented
              options={filterOptions}
              value={typeFilter}
              onChange={(v) => setTypeFilter(v)}
            />
            <div style={{ marginLeft: "auto", display: "flex", gap: 6, alignItems: "center" }}>
              {selected.size > 0 && (
                <span style={{ fontSize: 12, fontWeight: 600, color: "var(--rf-text-muted)" }}>
                  {selected.size} selectate
                </span>
              )}
              <IconBtn
                icon="refresh"
                title="Reîmprospătează"
                onClick={() =>
                  void queryClient.invalidateQueries({ queryKey: queryKeys.contacts.all })
                }
              />
            </div>
          </div>

          {/* Table */}
          <div className="rf-tbl-wrap">
            {isLoading ? (
              <Empty icon="users" title="Se încarcă…" />
            ) : contactsError ? (
              <QueryErrorBanner
                error={contactsErr}
                label="contactele"
                onRetry={() => void refetchContacts()}
              />
            ) : list.length === 0 ? (
              <Empty
                icon="users"
                title={
                  contacts.length === 0
                    ? "Niciun contact"
                    : "Niciun rezultat pentru filtrele aplicate"
                }
              >
                {contacts.length === 0 &&
                  "Adaugă primul client sau furnizor cu butonul \"Contact nou\"."}
              </Empty>
            ) : (
              <table className="rf-tbl">
                <thead>
                  <tr>
                    <th className="rf-ck">
                      <input
                        type="checkbox"
                        className="rf-cbx"
                        checked={selected.size === list.length && list.length > 0}
                        onChange={toggleAll}
                      />
                    </th>
                    <th style={{ width: 120 }}>CUI</th>
                    <th>Denumire</th>
                    <th style={{ width: 110 }}>Tip</th>
                    <th style={{ width: 140 }}>Localitate</th>
                    <th style={{ width: 50 }}>Județ</th>
                    <th style={{ width: 60, textAlign: "center" }}>TVA</th>
                    <th style={{ width: 180 }}>Email</th>
                    <th style={{ width: 90 }}></th>
                  </tr>
                </thead>
                <tbody>
                  {list.map((c: Contact) => (
                    <tr key={c.id}>
                      <td className="rf-ck" onClick={(e) => e.stopPropagation()}>
                        <input
                          type="checkbox"
                          className="rf-cbx"
                          checked={selected.has(c.id)}
                          onChange={() => toggleOne(c.id)}
                        />
                      </td>
                      <td className="mono">{c.cui ?? <span className="rf-dim">—</span>}</td>
                      <td style={{ fontWeight: 500 }}>{c.legalName}</td>
                      <td>
                        <Badge variant={TYPE_VARIANT[c.contactType]} dot={false}>
                          {TYPE_LABELS[c.contactType]}
                        </Badge>
                      </td>
                      <td style={{ color: "var(--rf-text-muted)" }}>
                        {c.city ?? <span className="rf-dim">—</span>}
                      </td>
                      <td className="mono" style={{ color: "var(--rf-text-muted)" }}>
                        {c.county ?? <span className="rf-dim">—</span>}
                      </td>
                      <td style={{ textAlign: "center" }}>
                        {c.vatPayer ? (
                          <Icon name="check" size={14} style={{ color: "var(--rf-success)" }} />
                        ) : (
                          <span className="rf-dim">
                            <Icon name="x" size={14} />
                          </span>
                        )}
                      </td>
                      <td style={{ fontSize: 12, color: "var(--rf-text-muted)" }}>
                        {c.email ?? <span className="rf-dim">—</span>}
                      </td>
                      <td onClick={(e) => e.stopPropagation()}>
                        <div className="rf-cell-actions">
                          <IconBtn
                            icon="pen"
                            title="Editează"
                            size={14}
                            onClick={() => setModal({ edit: c })}
                          />
                          <IconBtn
                            icon="trash"
                            title="Șterge"
                            size={14}
                            onClick={() => void handleDelete(c)}
                          />
                        </div>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>

          {/* Footer */}
          <div className="rf-tbl-footer">
            <span>Total: <b>{list.length}</b> contacte</span>
            <span>Clienți: <b>{customers}</b></span>
            <span>Furnizori: <b>{suppliers}</b></span>
          </div>
        </Card>
      </div>

      {/* Contact modal */}
      {modal !== null && (
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

      {/* CSV Import */}
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

// ─── ContactModal ─────────────────────────────────────────────────────────────

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

  return (
    <Modal
      open
      onOpenChange={(open) => { if (!open) onClose(); }}
      title={isEdit ? `Editează: ${contact.legalName}` : "Contact nou"}
      width={560}
      footer={
        <>
          <Btn variant="secondary" onClick={onClose} disabled={isPending}>
            Anulează
          </Btn>
          <Btn
            variant="primary"
            icon="check"
            disabled={isPending}
            onClick={(e) => {
              e.preventDefault();
              void handleSubmit(e);
            }}
          >
            {isPending ? "Se salvează…" : isEdit ? "Salvează" : "Adaugă"}
          </Btn>
        </>
      }
    >
      <form
        id="contact-form"
        onSubmit={handleSubmit}
        style={{ display: "flex", flexDirection: "column", gap: 14 }}
      >
        {/* Tip + CUI */}
        <div className="rf-grid-2">
          <Field label="Tip" required>
            <Select
              value={form.contactType}
              onChange={(e) =>
                setForm((f) => ({
                  ...f,
                  contactType: e.target.value as ContactType,
                }))
              }
            >
              <option value="CUSTOMER">Client</option>
              <option value="SUPPLIER">Furnizor</option>
              <option value="BOTH">Client/Furnizor</option>
            </Select>
          </Field>
          <Field label="CUI">
            <div style={{ display: "flex", gap: 6 }}>
              <Input
                placeholder="ex. RO12345678"
                className="mono"
                style={{ flex: 1 }}
                {...field("cui")}
                onBlur={triggerAnafLookup}
              />
              <Btn
                variant="secondary"
                size="sm"
                disabled={anafLookup.isPending || (form.isIndividual as boolean)}
                onClick={(e) => {
                  e.preventDefault();
                  setLastLookedUp("");
                  triggerAnafLookup();
                }}
                title="Preia datele firmei din ANAF după CUI"
              >
                {anafLookup.isPending ? "…" : "ANAF ↓"}
              </Btn>
            </div>
          </Field>
        </div>

        {/* Denumire */}
        <Field label="Denumire legală" required>
          <Input placeholder="S.C. Exemplu S.R.L." {...field("legalName")} autoFocus />
        </Field>

        {/* Localitate + Județ */}
        <div className="rf-grid-2">
          <Field label="Localitate">
            <Input placeholder="Cluj-Napoca" {...field("city")} />
          </Field>
          <Field label="Județ">
            <Input
              placeholder="CJ"
              maxLength={2}
              className="mono"
              style={{ textTransform: "uppercase" }}
              {...field("county")}
            />
          </Field>
        </div>

        {/* Adresă */}
        <Field label="Adresă">
          <Input placeholder="Str. Exemplu nr. 1" {...field("address")} />
        </Field>

        {/* Țară + Monedă */}
        <div className="rf-grid-2">
          <Field label="Țară">
            <Select value={form.country ?? "RO"} onChange={(e) => setForm((f) => ({ ...f, country: e.target.value }))}>
              {COUNTRIES.map((c) => (
                <option key={c.code} value={c.code}>
                  {c.name} ({c.code})
                </option>
              ))}
            </Select>
          </Field>
          <Field label="Monedă">
            <Select value={currency} onChange={(e) => setCurrency(e.target.value)}>
              {CURRENCIES.map((c) => (
                <option key={c} value={c}>
                  {c}
                </option>
              ))}
            </Select>
          </Field>
        </div>

        {/* Email + Telefon */}
        <div className="rf-grid-2">
          <Field label="Email">
            <Input type="email" placeholder="office@firma.ro" {...field("email")} />
          </Field>
          <Field label="Telefon">
            <Input placeholder="+40 722..." {...field("phone")} />
          </Field>
        </div>

        {/* Plătitor TVA */}
        <label
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            fontSize: 13,
            cursor: "pointer",
          }}
        >
          <input
            type="checkbox"
            className="rf-cbx"
            checked={form.vatPayer as boolean}
            onChange={(e) => setForm((f) => ({ ...f, vatPayer: e.target.checked }))}
          />
          Plătitor de TVA
        </label>

        <label
          style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 13, cursor: "pointer" }}
        >
          <input
            type="checkbox"
            className="rf-cbx"
            checked={form.isIndividual as boolean}
            onChange={(e) => setForm((f) => ({ ...f, isIndividual: e.target.checked }))}
          />
          Persoană fizică (consumator) — B2C, fără CUI
        </label>

        <label
          style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 13, cursor: "pointer" }}
        >
          <input
            type="checkbox"
            className="rf-cbx"
            checked={form.cashVat as boolean}
            onChange={(e) => setForm((f) => ({ ...f, cashVat: e.target.checked }))}
          />
          TVA la încasare (cash-VAT)
        </label>

        {anafInfo?.inactive && (
          <Banner variant="error">
            Contribuabil <b>INACTIV</b> la ANAF — facturile primite au deductibilitate restricționată
            pentru cheltuieli și TVA (art. 11 Cod fiscal). Verificați înainte de a înregistra
            achiziții.
          </Banner>
        )}
        {anafInfo?.cashVat && (
          <Banner variant="warning">
            Furnizor cu <b>TVA la încasare</b> — TVA deductibilă se amână până la plata facturii
            (art. 297 Cod fiscal).
          </Banner>
        )}

        {error && (
          <div className="rf-banner rf-banner--error">
            <Icon name="xCircle" size={16} />
            <span>{error}</span>
          </div>
        )}
      </form>
    </Modal>
  );
}
