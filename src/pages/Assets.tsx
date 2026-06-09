/**
 * Assets — mijloace fixe (registru) + amortizare liniară lunară.
 * Lista mijloacelor fixe (cost + durată + cont 21x) + rularea lunară care calculează amortizarea
 * (OMFP 1802/2014, începe din luna următoare PIF) și postează nota 6811 = 281x în GL.
 */

import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { confirm } from "@tauri-apps/plugin-dialog";

import {
  PageHeader, Btn, IconBtn, Card, Field, Input, Modal, Banner, Segmented, Empty,
} from "@/components/rf";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { fmtRON } from "@/lib/utils";
import type { FixedAsset, FixedAssetInput, DepreciationRun } from "@/types";

const MONTHS = ["Ian","Feb","Mar","Apr","Mai","Iun","Iul","Aug","Sep","Oct","Nov","Dec"];

export function AssetsPage() {
  const companyId = useAppStore((s) => s.activeCompanyId);
  const qc = useQueryClient();
  const now = new Date();
  const [year, setYear] = useState(now.getFullYear());
  const [month, setMonth] = useState(now.getMonth() + 1);
  const [modal, setModal] = useState<"create" | { edit: FixedAsset } | null>(null);
  const [disposing, setDisposing] = useState<FixedAsset | null>(null);
  const [run, setRun] = useState<DepreciationRun | null>(null);

  const { data: assets = [] } = useQuery({
    queryKey: ["assets", companyId],
    queryFn: () => api.assets.list(companyId!),
    enabled: !!companyId,
  });

  const period = useMemo(() => {
    const mm = String(month).padStart(2, "0");
    const last = new Date(year, month, 0).getDate();
    return { from: `${year}-${mm}-01`, to: `${year}-${mm}-${String(last).padStart(2, "0")}` };
  }, [year, month]);

  const runMut = useMutation({
    mutationFn: () => api.assets.runDepreciation(companyId!, period.from, period.to),
    onSuccess: (r) => {
      setRun(r);
      r.posted
        ? notify.success(`Amortizare postată — total ${r.totalAmount} lei (6811 = 281x).`)
        : notify.info("Nimic de amortizat în această lună.");
    },
    onError: (e) => notify.error(formatError(e, "Nu s-a putut rula amortizarea.")),
  });

  const del = useMutation({
    mutationFn: (id: string) => api.assets.delete(id, companyId!),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["assets", companyId] }),
    onError: (e) => notify.error(formatError(e, "Eroare la ștergere.")),
  });

  const dispose = useMutation({
    mutationFn: ({ id, date }: { id: string; date: string }) =>
      api.assets.dispose(companyId!, id, date),
    onSuccess: () => {
      notify.success("Mijloc fix scos din funcțiune (281x + 6583 / 21x).");
      setDisposing(null);
      void qc.invalidateQueries({ queryKey: ["assets", companyId] });
    },
    onError: (e) => notify.error(formatError(e, "Eroare la scoaterea din funcțiune.")),
  });

  return (
    <div className="rf-page">
      <PageHeader
        title="Mijloace fixe"
        actions={
          <Btn variant="primary" icon="plus" disabled={!companyId} onClick={() => setModal("create")}>
            Mijloc fix nou
          </Btn>
        }
      />
      <div className="rf-page-body">
        <Banner variant="info">
          Amortizare liniară (OMFP 1802/2014) — începe din luna următoare punerii în funcțiune
          (art. 28 Cod fiscal). Rularea lunară postează nota 6811 = 281x în jurnal.
        </Banner>

        <Card>
          <div style={{ display: "flex", gap: 12, alignItems: "center", flexWrap: "wrap", padding: 12 }}>
            <Segmented
              options={MONTHS.map((l, i) => ({ value: String(i + 1), label: l }))}
              value={String(month)}
              onChange={(v) => setMonth(Number(v))}
            />
            <Segmented
              options={[year - 1, year, year + 1].map((y) => ({ value: String(y), label: String(y) }))}
              value={String(year)}
              onChange={(v) => setYear(Number(v))}
            />
            <Btn variant="secondary" icon="ledger" disabled={runMut.isPending || !companyId} onClick={() => runMut.mutate()}>
              {runMut.isPending ? "Calculez…" : "Rulează amortizarea"}
            </Btn>
          </div>
        </Card>

        <Card>
          {assets.length === 0 ? (
            <Empty icon="buildings" title="Niciun mijloc fix — adăugați mijloace fixe pentru a calcula amortizarea." />
          ) : (
            <table className="rf-tbl">
              <thead>
                <tr><th>Cod</th><th>Descriere</th><th>Cont</th><th>PIF</th><th className="right">Cost</th><th className="right">Durată (luni)</th><th></th></tr>
              </thead>
              <tbody>
                {assets.map((a) => (
                  <tr key={a.id} style={{ opacity: a.active ? 1 : 0.5 }}>
                    <td className="mono">{a.assetCode}</td>
                    <td style={{ fontWeight: 500 }}>{a.description}</td>
                    <td className="mono">{a.accountId}</td>
                    <td className="mono">{a.startUpDate}</td>
                    <td className="right rf-mono">{fmtRON(a.acquisitionCost)}</td>
                    <td className="right rf-mono">{a.lifeMonths}</td>
                    <td className="right">
                      <IconBtn icon="edit" onClick={() => setModal({ edit: a })} title="Editează" />
                      {a.active && (
                        <IconBtn icon="archive" onClick={() => setDisposing(a)} title="Scoate din funcțiune" />
                      )}
                      <IconBtn icon="trash" onClick={async () => {
                        if (await confirm(`Ștergeți mijlocul fix "${a.description}"?`, { kind: "warning" })) del.mutate(a.id);
                      }} title="Șterge" />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </Card>

        {run && run.states.length > 0 && (
          <Card>
            <div style={{ padding: "10px 12px", fontWeight: 600 }}>
              Amortizare {MONTHS[month - 1]} {year}
            </div>
            <table className="rf-tbl">
              <thead>
                <tr><th>Cod</th><th>Descriere</th><th className="right">Amortizare lună</th><th className="right">Cumulat</th><th className="right">Valoare rămasă</th><th>Notă</th></tr>
              </thead>
              <tbody>
                {run.states.map((s) => (
                  <tr key={s.assetId}>
                    <td className="mono">{s.assetCode}</td>
                    <td>{s.description}</td>
                    <td className="right rf-mono" style={{ fontWeight: 600 }}>{fmtRON(s.monthlyCharge)}</td>
                    <td className="right rf-mono">{fmtRON(s.accumulated)}</td>
                    <td className="right rf-mono">{fmtRON(s.bookValue)}</td>
                    <td className="mono">{s.expenseAcct} = {s.amortAcct}</td>
                  </tr>
                ))}
              </tbody>
              <tfoot>
                <tr style={{ fontWeight: 700, borderTop: "2px solid var(--rf-border)" }}>
                  <td colSpan={2}>TOTAL</td>
                  <td className="right rf-mono">{fmtRON(run.totalAmount)}</td>
                  <td colSpan={3}></td>
                </tr>
              </tfoot>
            </table>
          </Card>
        )}
      </div>

      {modal && companyId && (
        <AssetModal
          companyId={companyId}
          asset={modal === "create" ? null : modal.edit}
          onClose={() => setModal(null)}
          onSaved={() => {
            setModal(null);
            void qc.invalidateQueries({ queryKey: ["assets", companyId] });
          }}
        />
      )}

      {disposing && (
        <DisposeModal
          asset={disposing}
          defaultDate={period.to}
          busy={dispose.isPending}
          onClose={() => setDisposing(null)}
          onConfirm={(date) => dispose.mutate({ id: disposing.id, date })}
        />
      )}
    </div>
  );
}

/** Scoatere din funcțiune — alegerea datei, înlocuiește window.prompt (no-op în WebView-ul Tauri). */
function DisposeModal({
  asset, defaultDate, busy, onClose, onConfirm,
}: {
  asset: FixedAsset;
  defaultDate: string;
  busy: boolean;
  onClose: () => void;
  onConfirm: (date: string) => void;
}) {
  const [date, setDate] = useState(defaultDate);
  const valid = /^\d{4}-\d{2}-\d{2}$/.test(date.trim());

  return (
    <Modal open onOpenChange={(o) => { if (!o) onClose(); }} title="Scoatere din funcțiune" width={420}
      footer={
        <>
          <Btn variant="secondary" onClick={onClose} disabled={busy}>Anulează</Btn>
          <Btn variant="primary" icon="archive" disabled={busy || !valid}
            onClick={() => { if (valid) onConfirm(date.trim()); }}>
            {busy ? "Se procesează…" : "Scoate din funcțiune"}
          </Btn>
        </>
      }
    >
      <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
        <Banner variant="info">
          Mijlocul fix «{asset.description}» ({asset.assetCode}) va fi scos din funcțiune la data
          aleasă (notă contabilă 281x + 6583 / 21x).
        </Banner>
        <Field label="Data scoaterii din funcțiune (AAAA-LL-ZZ)" required>
          <Input className="mono" placeholder="2026-06-30" value={date} onChange={(e) => setDate(e.target.value)} autoFocus />
        </Field>
      </div>
    </Modal>
  );
}

function AssetModal({
  companyId, asset, onClose, onSaved,
}: {
  companyId: string;
  asset: FixedAsset | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const isEdit = asset !== null;
  const [form, setForm] = useState({
    assetCode: asset?.assetCode ?? "",
    description: asset?.description ?? "",
    accountId: asset?.accountId ?? "213",
    dateOfAcquisition: asset?.dateOfAcquisition ?? "",
    startUpDate: asset?.startUpDate ?? "",
    acquisitionCost: asset?.acquisitionCost ?? "",
    lifeMonths: asset ? String(asset.lifeMonths) : "",
  });
  const [error, setError] = useState<string | null>(null);

  const save = useMutation({
    mutationFn: () => {
      if (!form.description.trim()) throw new Error("Descrierea e obligatorie.");
      const input: FixedAssetInput = {
        assetCode: form.assetCode.trim() || "MF",
        description: form.description.trim(),
        accountId: form.accountId.trim() || "213",
        dateOfAcquisition: form.dateOfAcquisition || form.startUpDate,
        startUpDate: form.startUpDate || form.dateOfAcquisition,
        acquisitionCost: form.acquisitionCost.trim() || "0",
        lifeMonths: Number(form.lifeMonths) || 0,
        depreciationMethod: "liniara",
      };
      return isEdit ? api.assets.update(asset!.id, companyId, input) : api.assets.create(companyId, input);
    },
    onSuccess: onSaved,
    onError: (e) => setError(formatError(e, "Eroare la salvare.")),
  });

  const field = (k: keyof typeof form) => ({
    value: form[k],
    onChange: (e: React.ChangeEvent<HTMLInputElement>) => setForm((f) => ({ ...f, [k]: e.target.value })),
  });

  return (
    <Modal
      open
      onOpenChange={(o) => { if (!o) onClose(); }}
      title={isEdit ? `Editează: ${asset.description}` : "Mijloc fix nou"}
      width={520}
      footer={
        <>
          <Btn variant="secondary" onClick={onClose} disabled={save.isPending}>Anulează</Btn>
          <Btn variant="primary" icon="check" disabled={save.isPending} onClick={() => save.mutate()}>
            {save.isPending ? "Se salvează…" : isEdit ? "Salvează" : "Adaugă"}
          </Btn>
        </>
      }
    >
      <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
        <Field label="Descriere" required><Input placeholder="Laptop Dell" {...field("description")} autoFocus /></Field>
        <div className="rf-grid-2">
          <Field label="Cod"><Input placeholder="MF-001" className="mono" {...field("assetCode")} /></Field>
          <Field label="Cont (21x)"><Input placeholder="213" className="mono" {...field("accountId")} /></Field>
        </div>
        <div className="rf-grid-2">
          <Field label="Data achiziției"><Input type="date" {...field("dateOfAcquisition")} /></Field>
          <Field label="Data punerii în funcțiune (PIF)"><Input type="date" {...field("startUpDate")} /></Field>
        </div>
        <div className="rf-grid-2">
          <Field label="Cost (lei)"><Input inputMode="decimal" placeholder="5000" {...field("acquisitionCost")} /></Field>
          <Field label="Durată (luni)"><Input inputMode="numeric" placeholder="36" {...field("lifeMonths")} /></Field>
        </div>
        {error && <Banner variant="error">{error}</Banner>}
      </div>
    </Modal>
  );
}
