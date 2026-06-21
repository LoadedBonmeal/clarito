/**
 * Gestiuni (depozite/magazii) — management page.
 * CRUD: cod, denumire, tip, metoda_evaluare, cont_stoc, adresa, dispersata_teritorial.
 * Gestiunea principala (is_default=1) nu poate fi stearsa.
 */

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";
import type { Gestiune, GestiuneInput } from "@/types";

export function GestiuniPage() {
  const { t } = useTranslation();
  const companyId = useAppStore((s) => s.activeCompanyId);
  const qc = useQueryClient();

  const { data: gestiuni = [], isLoading, error } = useQuery({
    queryKey: ["gestiuni", companyId],
    queryFn: () => api.gestiuni.list(companyId!),
    enabled: !!companyId,
  });

  const [editing, setEditing] = useState<Gestiune | null>(null);
  const [creating, setCreating] = useState(false);

  const deleteMut = useMutation({
    mutationFn: (id: string) => api.gestiuni.delete(id, companyId!),
    onSuccess: () => {
      notify.success(t("gestiuni.deleted"));
      void qc.invalidateQueries({ queryKey: ["gestiuni", companyId] });
    },
    onError: (e) => notify.error(formatError(e, t("gestiuni.deleteError"))),
  });

  if (!companyId) {
    return (
      <div className="page">
        <div className="page-head">
          <div className="page-title">{t("gestiuni.title")}</div>
        </div>
        <div className="scr-card" style={{ padding: 24, color: "var(--text-2)" }}>
          {t("gestiuni.selectCompany")}
        </div>
      </div>
    );
  }

  return (
    <div className="page">
      <div className="page-head">
        <div>
          <div className="page-title">{t("gestiuni.title")}</div>
          <div className="page-sub">{t("gestiuni.sub")}</div>
        </div>
        <div className="spacer" />
        <button className="btn-dark" onClick={() => setCreating(true)}>
          + {t("gestiuni.new")}
        </button>
      </div>

      {error && <QueryErrorBanner error={error} label={t("gestiuni.errorLabel")} />}

      <div className="scr-card">
        <table className="scr-table">
          <thead>
            <tr>
              <th>{t("gestiuni.colCod")}</th>
              <th>{t("gestiuni.colDenumire")}</th>
              <th>{t("gestiuni.colTip")}</th>
              <th>{t("gestiuni.colMetoda")}</th>
              <th>{t("gestiuni.colCont")}</th>
              <th>{t("gestiuni.colStatus")}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {isLoading && (
              <tr>
                <td colSpan={7} style={{ textAlign: "center", padding: 24 }}>
                  {t("gestiuni.loading")}
                </td>
              </tr>
            )}
            {!isLoading && gestiuni.length === 0 && (
              <tr>
                <td colSpan={7} style={{ textAlign: "center", padding: 24, color: "var(--text-2)" }}>
                  {t("gestiuni.empty")}
                </td>
              </tr>
            )}
            {gestiuni.map((g) => (
              <tr key={g.id}>
                <td>
                  <span className="doc">{g.cod}</span>
                </td>
                <td>
                  {g.denumire}
                  {g.isDefault === 1 && (
                    <span className="chip sent" style={{ marginLeft: 8 }}>
                      {t("gestiuni.default")}
                    </span>
                  )}
                </td>
                <td>
                  <span className="chip">
                    {g.tip === "cantitativ_valorica" ? t("gestiuni.tipCV") : t("gestiuni.tipGV")}
                  </span>
                </td>
                <td>
                  <span className="chip">{g.metodaEvaluare}</span>
                </td>
                <td>
                  <span className="doc">{g.contStoc}</span>
                </td>
                <td>
                  <span className={`chip ${g.activ === 1 ? "sent" : "late"}`}>
                    {g.activ === 1 ? t("gestiuni.active") : t("gestiuni.inactive")}
                  </span>
                </td>
                <td style={{ display: "flex", gap: 6, justifyContent: "flex-end" }}>
                  <button className="pill-btn" onClick={() => setEditing(g)}>
                    {t("gestiuni.edit")}
                  </button>
                  {g.isDefault !== 1 && (
                    <button
                      className="pill-btn"
                      onClick={() => deleteMut.mutate(g.id)}
                      disabled={deleteMut.isPending}
                    >
                      {t("gestiuni.delete")}
                    </button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {(creating || editing) && (
        <GestiuneModal
          companyId={companyId}
          gestiune={editing}
          onClose={() => {
            setCreating(false);
            setEditing(null);
          }}
          onSaved={() => {
            setCreating(false);
            setEditing(null);
            void qc.invalidateQueries({ queryKey: ["gestiuni", companyId] });
          }}
        />
      )}
    </div>
  );
}

function GestiuneModal({
  companyId,
  gestiune,
  onClose,
  onSaved,
}: {
  companyId: string;
  gestiune: Gestiune | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation();
  const [cod, setCod] = useState(gestiune?.cod ?? "");
  const [denumire, setDenumire] = useState(gestiune?.denumire ?? "");
  const [tip, setTip] = useState(gestiune?.tip ?? "cantitativ_valorica");
  const [metoda, setMetoda] = useState(gestiune?.metodaEvaluare ?? "CMP");
  const [cont, setCont] = useState(gestiune?.contStoc ?? "371");
  const [adresa, setAdresa] = useState(gestiune?.adresa ?? "");
  const [dispersata, setDispersata] = useState((gestiune?.dispersataTeritorila ?? 0) === 1);

  const mut = useMutation({
    mutationFn: () => {
      const input: GestiuneInput = {
        cod,
        denumire,
        tip,
        metodaEvaluare: metoda,
        contStoc: cont,
        adresa: adresa || undefined,
        dispersataTeritorila: dispersata,
      };
      return gestiune
        ? api.gestiuni.update(gestiune.id, companyId, input)
        : api.gestiuni.create(companyId, input);
    },
    onSuccess: () => {
      notify.success(gestiune ? t("gestiuni.saved") : t("gestiuni.added"));
      onSaved();
    },
    onError: (e) => notify.error(formatError(e, t("gestiuni.saveError"))),
  });

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()} style={{ maxWidth: 520 }}>
        <div className="modal-head">
          <div className="modal-title">
            {gestiune
              ? t("gestiuni.editTitle", { cod: gestiune.cod })
              : t("gestiuni.newTitle")}
          </div>
          <button className="modal-close" onClick={onClose}>
            &times;
          </button>
        </div>
        <div className="modal-body" style={{ display: "flex", flexDirection: "column", gap: 14 }}>
          <div className="field">
            <label>{t("gestiuni.fieldCod")}</label>
            <input
              className="input"
              value={cod}
              onChange={(e) => setCod(e.target.value)}
              placeholder="PRINCIPALA"
            />
          </div>
          <div className="field">
            <label>{t("gestiuni.fieldDenumire")}</label>
            <input
              className="input"
              value={denumire}
              onChange={(e) => setDenumire(e.target.value)}
              placeholder={t("gestiuni.denumirePlaceholder")}
            />
          </div>
          <div className="field">
            <label>{t("gestiuni.fieldTip")}</label>
            <select
              className="select"
              value={tip}
              onChange={(e) => setTip(e.target.value)}
            >
              <option value="cantitativ_valorica">{t("gestiuni.tipCV")}</option>
              <option value="global_valorica">{t("gestiuni.tipGV")}</option>
            </select>
          </div>
          <div className="field">
            <label>{t("gestiuni.fieldMetoda")}</label>
            <select
              className="select"
              value={metoda}
              onChange={(e) => setMetoda(e.target.value)}
            >
              <option value="CMP">{t("gestiuni.metodaCMP")}</option>
              <option value="FIFO">FIFO</option>
              <option value="LIFO">LIFO</option>
            </select>
            <small className="hint">{t("gestiuni.metodaInfo")}</small>
          </div>
          <div className="field">
            <label>{t("gestiuni.fieldCont")}</label>
            <input
              className="input num"
              value={cont}
              onChange={(e) => setCont(e.target.value)}
              placeholder="371"
            />
          </div>
          <div className="field">
            <label>{t("gestiuni.fieldAdresa")}</label>
            <input
              className="input"
              value={adresa}
              onChange={(e) => setAdresa(e.target.value)}
              placeholder={t("gestiuni.adresaPlaceholder")}
            />
          </div>
          <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
            <input
              type="checkbox"
              checked={dispersata}
              onChange={(e) => setDispersata(e.target.checked)}
            />
            {t("gestiuni.fieldDispersata")}
          </label>
        </div>
        <div className="modal-footer">
          <button className="btn-ghost" onClick={onClose}>
            {t("gestiuni.cancel")}
          </button>
          <button
            className="btn-dark"
            disabled={mut.isPending}
            onClick={() => mut.mutate()}
          >
            {mut.isPending
              ? t("gestiuni.saving")
              : gestiune
              ? t("gestiuni.save")
              : t("gestiuni.add")}
          </button>
        </div>
      </div>
    </div>
  );
}
