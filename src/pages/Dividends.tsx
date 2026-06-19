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
import type { Dividend, PreflightIssue } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { fmtRON } from "@/lib/utils";
import { useOpenXml } from "@/hooks/use-open-xml";

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
  const [beneficiaryType, setBeneficiaryType] = useState<"PF" | "PJ">("PF");
  const [d205Year, setD205Year] = useState(new Date().getFullYear() - 1);
  const [exportingD205, setExportingD205] = useState(false);
  const [dukBlock, setDukBlock] = useState<PreflightIssue[] | null>(null);
  // D207 — declarația informativă pentru veniturile nerezidenților (capitol dividende).
  const [d207Year, setD207Year] = useState(new Date().getFullYear() - 1);
  const [exportingD207, setExportingD207] = useState(false);
  const [previewingD207, setPreviewingD207] = useState(false);

  const { data: list = [] } = useQuery({
    queryKey: ["dividends", companyId ?? ""],
    queryFn: () => api.dividends.list(companyId!),
    enabled: !!companyId,
  });

  // Dividendele către NEREZIDENȚI sunt excluse din D205 (se raportează separat în D207, emis mai jos) —
  // le semnalăm explicit ca să nu fie raportate „tăcut" în nicio declarație.
  const nonResidentCount = list.filter((d) => !d.beneficiaryResident).length;

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
        beneficiaryType,
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

  // DIV-01: editare in-place a beneficiarului (CNP/nume/rezidență/tip/dată plată) — fără a atinge
  // sumele (brut/impozit postează GL). Permite corectarea unui CNP lipsă/greșit fără delete + recreate.
  const [editing, setEditing] = useState<Dividend | null>(null);
  const [eName, setEName] = useState("");
  const [eCnp, setECnp] = useState("");
  const [eResident, setEResident] = useState(true);
  const [eType, setEType] = useState<"PF" | "PJ">("PF");
  const [ePayment, setEPayment] = useState("");
  // D207: pentru beneficiari nerezidenți — țara de rezidență (Stat_R) + NIF-ul fiscal străin (cifS).
  const [eCountry, setECountry] = useState("");
  const [eForeign, setEForeign] = useState("");

  const startEdit = (d: Dividend) => {
    setEditing(d);
    setEName(d.shareholder ?? "");
    setECnp(d.beneficiaryCnp ?? "");
    setEResident(d.beneficiaryResident);
    setEType((d.beneficiaryType as "PF" | "PJ") ?? "PF");
    setEPayment(d.paymentDate ?? "");
    setECountry(d.beneficiaryCountry ?? "");
    setEForeign(d.beneficiaryForeignTaxId ?? "");
  };

  const updateBen = useMutation({
    mutationFn: () => {
      if (!editing || !companyId) throw new Error(t("dividends.selectCompany"));
      return api.dividends.updateBeneficiary({
        id: editing.id,
        companyId,
        paymentDate: ePayment || null,
        shareholder: eName || null,
        beneficiaryCnp: eCnp.trim() || null,
        beneficiaryResident: eResident,
        beneficiaryType: eType,
        // Nerezidenții poartă țara + NIF străin (D207); rezidenții le trimit goale.
        beneficiaryCountry: eResident ? null : eCountry.trim().toUpperCase() || null,
        beneficiaryForeignTaxId: eResident ? null : eForeign.trim() || null,
      });
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["dividends", companyId ?? ""] });
      setEditing(null);
      notify.success(t("dividends.updated"));
    },
    onError: (e) => notify.error(formatError(e, t("dividends.updateFailed"))),
  });

  const openXml = useOpenXml();
  const [previewingD205, setPreviewingD205] = useState(false);

  /** Construiește XML-ul D205 și îl deschide în vizualizatorul/editorul XML (cu re-validare DUK). */
  const runD205Preview = async () => {
    if (!companyId) return;
    setPreviewingD205(true);
    try {
      const xml = await api.dividends.previewD205Xml(companyId, d205Year);
      openXml({ xml, name: `d205-${d205Year}.xml`, declKind: "D205" });
    } catch (e) {
      notify.error(formatError(e, t("dividends.d205.previewFailed")));
    } finally {
      setPreviewingD205(false);
    }
  };

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

  /** Construiește XML-ul D207 (venituri nerezidenți) și îl deschide în vizualizatorul XML. */
  const runD207Preview = async () => {
    if (!companyId) return;
    setPreviewingD207(true);
    try {
      const xml = await api.dividends.previewD207Xml(companyId, d207Year);
      // D207 nu are validator DUK propriu (validat XSD în backend) → docKey, nu declKind.
      openXml({ xml, name: `d207-${d207Year}.xml`, docKey: "D207" });
    } catch (e) {
      notify.error(formatError(e, t("dividends.d207.previewFailed")));
    } finally {
      setPreviewingD207(false);
    }
  };

  /** Export D207 (informativă anuală, venituri nerezidenți — validată XSD ANAF). */
  const runD207Export = async () => {
    if (!companyId) return;
    const dest = await saveDialog({
      title: t("dividends.d207.saveTitle"),
      defaultPath: `d207-${d207Year}.xml`,
      filters: [{ name: "XML", extensions: ["xml"] }],
    });
    if (!dest) return;
    setExportingD207(true);
    try {
      const res = await api.dividends.exportD207Official(companyId, d207Year, dest);
      if (!res.written) {
        notify.error(formatError(res.issues?.[0]?.message ?? "", t("dividends.d207.exportFailed")));
        return;
      }
      notify.success(t("dividends.d207.saved", { path: res.path }));
    } catch (e) {
      notify.error(formatError(e, t("dividends.d207.exportFailed")));
    } finally {
      setExportingD207(false);
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
            <input className="input num num-r" inputMode="decimal" placeholder="10000" value={grossAmount} onChange={(e) => setGrossAmount(e.target.value.replace(/[^0-9.]/g, ""))} />
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
            <label>{t("dividends.beneficiaryType")}</label>
            <select
              className="input"
              value={beneficiaryType}
              onChange={(e) => setBeneficiaryType(e.target.value as "PF" | "PJ")}
            >
              <option value="PF">{t("dividends.beneficiaryTypePF")}</option>
              <option value="PJ">{t("dividends.beneficiaryTypePJ")}</option>
            </select>
            <div className="hint">{t("dividends.beneficiaryTypeHint")}</div>
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
          <button className="pill-btn" disabled={previewingD205} onClick={() => void runD205Preview()}>
            <Ic name="eye" />{previewingD205 ? t("dividends.d205.previewing") : t("dividends.d205.preview")}
          </button>
        </div>
        {nonResidentCount > 0 && (
          <div
            className="hint"
            style={{ marginTop: 10, color: "var(--red)", display: "flex", gap: 6, alignItems: "flex-start" }}
          >
            <Ic name="shield" />
            <span>{t("dividends.d205.nonResidentWarn", { count: nonResidentCount })}</span>
          </div>
        )}
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

      {/* Export D207 — informativă anuală pentru veniturile nerezidenților (capitol dividende), validată XSD ANAF.
          Apare doar când există beneficiari nerezidenți (altfel declarația nu se depune). */}
      {nonResidentCount > 0 && (
        <div className="card" style={{ padding: 16, marginBottom: 16 }}>
          <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 4 }}>{t("dividends.d207.title")}</div>
          <div className="hint" style={{ marginBottom: 12 }}>{t("dividends.d207.hint")}</div>
          <div style={{ display: "flex", alignItems: "flex-end", gap: 12, flexWrap: "wrap" }}>
            <div className="field" style={{ width: 140 }}>
              <label>{t("dividends.d207.year")}</label>
              <input
                className="input num"
                inputMode="numeric"
                maxLength={4}
                value={d207Year}
                onChange={(e) => setD207Year(Number(e.target.value.replace(/\D/g, "")) || d207Year)}
              />
            </div>
            <button className="btn-dark" disabled={exportingD207} onClick={() => void runD207Export()}>
              <Ic name="code" />{exportingD207 ? t("dividends.d207.exporting") : t("dividends.d207.export")}
            </button>
            <button className="pill-btn" disabled={previewingD207} onClick={() => void runD207Preview()}>
              <Ic name="eye" />{previewingD207 ? t("dividends.d207.previewing") : t("dividends.d207.preview")}
            </button>
          </div>
          {/* DIV-02: the emitter reports every non-resident dividend as fully taxable at the domestic
              rate (cod venit 01). Treaty-reduced / EU parent-subsidiary-exempt cases aren't modelled yet. */}
          <div
            className="hint"
            style={{ marginTop: 10, color: "var(--amber, #b45309)", display: "flex", gap: 6, alignItems: "flex-start" }}
          >
            <Ic name="shield" />
            <span>{t("dividends.d207.treatyNote")}</span>
          </div>
        </div>
      )}

      {/* DIV-01: editor beneficiar in-place (corectare CNP / nume / rezidență / tip / dată plată). */}
      {editing && (
        <div className="card" style={{ padding: 16, marginBottom: 16 }}>
          <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 4 }}>
            {t("dividends.editTitle")} — {editing.distributionDate} · {fmtRON(editing.grossAmount)}
          </div>
          <div className="hint" style={{ marginBottom: 12 }}>{t("dividends.editHint")}</div>
          <div className="fgrid">
            <div className="field">
              <label>{t("dividends.paymentDate")}</label>
              <input className="input" type="date" value={ePayment} onChange={(e) => setEPayment(e.target.value)} />
            </div>
            <div className="field">
              <label>{t("dividends.shareholder")}</label>
              <input className="input" value={eName} onChange={(e) => setEName(e.target.value)} />
            </div>
            <div className="field">
              <label>{t("dividends.beneficiaryType")}</label>
              <select className="input" value={eType} onChange={(e) => setEType(e.target.value as "PF" | "PJ")}>
                <option value="PF">{t("dividends.beneficiaryTypePF")}</option>
                <option value="PJ">{t("dividends.beneficiaryTypePJ")}</option>
              </select>
            </div>
            <div className="field">
              <label>{t("dividends.beneficiaryCnp")}</label>
              <input className="input num" inputMode="numeric" maxLength={13} placeholder="1960101410019" value={eCnp} onChange={(e) => setECnp(e.target.value.replace(/\D/g, ""))} />
            </div>
            <div className="field span2">
              <label className="chk-row">
                <input type="checkbox" checked={eResident} onChange={(e) => setEResident(e.target.checked)} />
                <span>{t("dividends.beneficiaryResident")}</span>
              </label>
            </div>
            {/* D207: nerezidenții poartă țara de rezidență (Stat_R) + NIF-ul fiscal străin (cifS). */}
            {!eResident && (
              <>
                <div className="field">
                  <label>{t("dividends.beneficiaryCountry")}</label>
                  <input className="input" maxLength={2} placeholder="GB" value={eCountry} onChange={(e) => setECountry(e.target.value.replace(/[^a-zA-Z]/g, "").toUpperCase())} />
                  <div className="hint">{t("dividends.beneficiaryCountryHint")}</div>
                </div>
                <div className="field">
                  <label>{t("dividends.beneficiaryForeignTaxId")}</label>
                  <input className="input" maxLength={40} placeholder="GB123456789" value={eForeign} onChange={(e) => setEForeign(e.target.value)} />
                  <div className="hint">{t("dividends.beneficiaryForeignTaxIdHint")}</div>
                </div>
              </>
            )}
          </div>
          <div style={{ display: "flex", gap: 8, marginTop: 12 }}>
            <button className="btn-dark" disabled={updateBen.isPending} onClick={() => updateBen.mutate()}>
              <Ic name="check" />{t("dividends.save")}
            </button>
            <button className="pill-btn" disabled={updateBen.isPending} onClick={() => setEditing(null)}>
              {t("dividends.cancel")}
            </button>
          </div>
        </div>
      )}

      {/* List — .scr-card (not .card) so the table is clipped to the rounded corners (overflow:hidden). */}
      <div className="scr-card">
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
                  <td>
                    {d.shareholder ?? "—"}
                    {!d.beneficiaryResident && (
                      <span
                        title={t("dividends.col.nonResidentTitle")}
                        style={{
                          marginLeft: 6,
                          fontSize: 10.5,
                          fontWeight: 600,
                          color: "var(--red)",
                          border: "1px solid rgba(220,38,38,.3)",
                          borderRadius: 999,
                          padding: "1px 7px",
                          whiteSpace: "nowrap",
                        }}
                      >
                        {t("dividends.col.nonResident")}
                      </span>
                    )}
                  </td>
                  <td className="r w-del">
                    <div style={{ display: "inline-flex", gap: 2 }}>
                      <button
                        className="icon-btn"
                        title={t("dividends.edit")}
                        onClick={() => startEdit(d)}
                      >
                        <Ic name="pen" />
                      </button>
                      <button
                        className="icon-btn"
                        title={t("dividends.delete")}
                        onClick={async () => { if (await confirm(t("dividends.confirmDelete"))) del.mutate(d.id); }}
                      >
                        <Ic name="xMark" />
                      </button>
                    </div>
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
