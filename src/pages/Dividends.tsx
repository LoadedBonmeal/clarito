/**
 * Dividende — repartizare + impozit pe dividende (Legea 141/2025: 16% de la 2026, 10% tranzitoriu
 * pentru situații interimare 2025). Înregistrarea calculează cota + impozitul, postează nota
 * 117/457/446 în GL și afișează termenul de declarare (decl. 100, 25 a lunii următoare plății).
 */
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { confirm, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import { Ic } from "@/components/shared/Ic";
import { PreflightPanel } from "@/components/shared/PreflightPanel";
import { api } from "@/lib/tauri";
import type { PreflightIssue } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { fmtRON } from "@/lib/utils";

const todayISO = () => new Date().toISOString().slice(0, 10);

export function Dividends() {
  const { t } = useTranslation();
  const companyId = useAppStore((s) => s.activeCompanyId);
  const qc = useQueryClient();

  const [distributionDate, setDistributionDate] = useState(todayISO());
  const [paymentDate, setPaymentDate] = useState("");
  const [grossAmount, setGrossAmount] = useState("");
  const [interim2025, setInterim2025] = useState(false);
  const [shareholder, setShareholder] = useState("");
  const [beneficiaryCnp, setBeneficiaryCnp] = useState("");
  const [beneficiaryResident, setBeneficiaryResident] = useState(true);
  const [d205Year, setD205Year] = useState(new Date().getFullYear() - 1);
  const [exportingD205, setExportingD205] = useState(false);
  const [dukBlock, setDukBlock] = useState<PreflightIssue[] | null>(null);

  const { data: list = [] } = useQuery({
    queryKey: ["dividends", companyId ?? ""],
    queryFn: () => api.dividends.list(companyId!),
    enabled: !!companyId,
  });

  const add = useMutation({
    mutationFn: () => {
      if (!companyId) throw new Error(t("dividends.selectCompany"));
      return api.dividends.create({
        companyId,
        distributionDate,
        paymentDate: paymentDate || null,
        grossAmount: grossAmount || "0",
        interim2025,
        shareholder: shareholder || null,
        beneficiaryCnp: beneficiaryCnp.trim() || null,
        beneficiaryResident,
      });
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["dividends", companyId ?? ""] });
      void qc.invalidateQueries({ queryKey: ["gl"] });
      setGrossAmount("");
      setShareholder("");
      setBeneficiaryCnp("");
      notify.success(t("dividends.saved"));
    },
    onError: (e) => notify.error(formatError(e, t("dividends.saveFailed"))),
  });

  const del = useMutation({
    mutationFn: (id: string) => api.dividends.delete(id, companyId!),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["dividends", companyId ?? ""] }),
    onError: (e) => notify.error(formatError(e, t("dividends.deleteFailed"))),
  });

  /** Export D205 (informativă anuală, pe beneficiar) cu gate DUK + override. */
  const runD205Export = async (override = false) => {
    if (!companyId) return;
    const dest = await saveDialog({
      title: t("dividends.d205.saveTitle"),
      defaultPath: `d205-${d205Year}.xml`,
      filters: [{ name: "XML", extensions: ["xml"] }],
    });
    if (!dest) return;
    setExportingD205(true);
    try {
      const res = await api.dividends.exportD205Official(companyId, d205Year, dest, override);
      if (!res.written) {
        setDukBlock(res.issues);
        notify.error(t("declarations.notify.dukErrors"));
        return;
      }
      setDukBlock(null);
      notify.success(
        res.dukAvailable
          ? t("dividends.d205.savedDuk", { path: res.path })
          : t("dividends.d205.savedNoDuk", { path: res.path }),
      );
    } catch (e) {
      notify.error(formatError(e, t("dividends.d205.exportFailed")));
    } finally {
      setExportingD205(false);
    }
  };

  if (!companyId) {
    return (
      <div className="main-inner">
        <div className="page-head"><div><h1>{t("dividends.title")}</h1></div></div>
        <p style={{ fontSize: 13, color: "var(--text-2)" }}>{t("dividends.selectCompany")}</p>
      </div>
    );
  }

  return (
    <div className="main-inner wide">
      <div className="page-head">
        <div>
          <h1>{t("dividends.title")}</h1>
          <div className="sub">{t("dividends.sub")}</div>
        </div>
      </div>

      {/* Entry form */}
      <div className="card" style={{ padding: 16, marginBottom: 16 }}>
        <div className="fgrid">
          <div className="field">
            <label>{t("dividends.distributionDate")}</label>
            <input className="input" type="date" value={distributionDate} onChange={(e) => setDistributionDate(e.target.value)} />
          </div>
          <div className="field">
            <label>{t("dividends.gross")}</label>
            <input className="input num num-r" inputMode="decimal" placeholder="10000" value={grossAmount} onChange={(e) => setGrossAmount(e.target.value)} />
          </div>
          <div className="field">
            <label>{t("dividends.paymentDate")}</label>
            <input className="input" type="date" value={paymentDate} onChange={(e) => setPaymentDate(e.target.value)} />
          </div>
          <div className="field">
            <label>{t("dividends.shareholder")}</label>
            <input className="input" value={shareholder} onChange={(e) => setShareholder(e.target.value)} />
          </div>
          <div className="field">
            <label>{t("dividends.beneficiaryCnp")}</label>
            <input
              className="input num"
              inputMode="numeric"
              maxLength={13}
              placeholder="1960101410019"
              value={beneficiaryCnp}
              onChange={(e) => setBeneficiaryCnp(e.target.value.replace(/\D/g, ""))}
            />
            <div className="hint">{t("dividends.beneficiaryCnpHint")}</div>
          </div>
          <div className="field span2">
            <label className="chk-row">
              <input type="checkbox" checked={beneficiaryResident} onChange={(e) => setBeneficiaryResident(e.target.checked)} />
              <span>{t("dividends.beneficiaryResident")}</span>
            </label>
            <div className="hint">{t("dividends.beneficiaryResidentHint")}</div>
          </div>
          <div className="field span2">
            <label className="chk-row">
              <input type="checkbox" checked={interim2025} onChange={(e) => setInterim2025(e.target.checked)} />
              <span>{t("dividends.interim2025")}</span>
            </label>
            <div className="hint">{t("dividends.interim2025Hint")}</div>
          </div>
        </div>
        <button className="btn-dark" style={{ marginTop: 12 }} disabled={add.isPending || !grossAmount} onClick={() => add.mutate()}>
          <Ic name="plus" />{t("dividends.add")}
        </button>
      </div>

      {/* Export D205 — declarația informativă anuală, pe beneficiar (capitol dividende), validată DUK */}
      <div className="card" style={{ padding: 16, marginBottom: 16 }}>
        <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 4 }}>{t("dividends.d205.title")}</div>
        <div className="hint" style={{ marginBottom: 12 }}>{t("dividends.d205.hint")}</div>
        <div style={{ display: "flex", alignItems: "flex-end", gap: 12, flexWrap: "wrap" }}>
          <div className="field" style={{ width: 140 }}>
            <label>{t("dividends.d205.year")}</label>
            <input
              className="input num"
              inputMode="numeric"
              maxLength={4}
              value={d205Year}
              onChange={(e) => setD205Year(Number(e.target.value.replace(/\D/g, "")) || d205Year)}
            />
          </div>
          <button className="btn-dark" disabled={exportingD205} onClick={() => void runD205Export()}>
            <Ic name="code" />{exportingD205 ? t("dividends.d205.exporting") : t("dividends.d205.export")}
          </button>
        </div>
        {dukBlock && (
          <div style={{ marginTop: 12 }}>
            <PreflightPanel issues={dukBlock} />
            <button
              className="pill-btn"
              style={{ marginTop: 8, color: "var(--red)", borderColor: "rgba(220,38,38,.35)" }}
              disabled={exportingD205}
              onClick={() => void runD205Export(true)}
            >
              {t("declarations.common.exportAnyway")}
            </button>
          </div>
        )}
      </div>

      {/* List */}
      <div className="card">
        <table className="scr-table">
          <thead>
            <tr>
              <th>{t("dividends.col.distribution")}</th>
              <th className="r">{t("dividends.col.gross")}</th>
              <th className="r">{t("dividends.col.rate")}</th>
              <th className="r">{t("dividends.col.tax")}</th>
              <th className="r">{t("dividends.col.net")}</th>
              <th>{t("dividends.col.deadline")}</th>
              <th>{t("dividends.col.shareholder")}</th>
              <th className="r w-del"></th>
            </tr>
          </thead>
          <tbody>
            {list.length === 0 ? (
              <tr><td colSpan={8} style={{ padding: "32px 16px", textAlign: "center", color: "var(--text-2)" }}>{t("dividends.empty")}</td></tr>
            ) : (
              list.map((d) => (
                <tr key={d.id}>
                  <td>{d.distributionDate}</td>
                  <td className="r num">{fmtRON(d.grossAmount)}</td>
                  <td className="r num">{d.taxRate}%</td>
                  <td className="r num">{fmtRON(d.taxAmount)}</td>
                  <td className="r num">{fmtRON(d.netAmount)}</td>
                  <td>{d.taxDeadline}</td>
                  <td>{d.shareholder ?? "—"}</td>
                  <td className="r w-del">
                    <button
                      className="icon-btn"
                      title={t("dividends.delete")}
                      onClick={async () => { if (await confirm(t("dividends.confirmDelete"))) del.mutate(d.id); }}
                    >
                      <Ic name="xMark" />
                    </button>
                  </td>
                </tr>
              ))
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
