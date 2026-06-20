/**
 * D390View — declarația recapitulativă (VIES) intra-UE: operațiuni grupate pe
 * partener + tip (L/T/A/P/S/R). Aggregated from sales/received vat_category='K' lines.
 * Embedded in the Reports page — Claude-Design classes (.scr-card / .scr-table / .chip / .banner).
 */

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";

import { Ic } from "@/components/shared/Ic";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { useOpenXml } from "@/hooks/use-open-xml";

interface Props {
  dateFrom: string;
  dateTo: string;
}

const TIP_KEY: Record<string, string> = {
  L: "l", T: "t", A: "a", P: "p", S: "s", R: "r",
};

// Warn triangle — not in the Ic set, inlined verbatim from the prototype.
const IC_WARN =
  '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>';

const fmtLei = (n: number) => n.toLocaleString("ro-RO");

/**
 * EU member-state VAT prefixes recognised by VIES (27 member states + XI).
 * EL = Greece (its fiscal prefix, not the ISO `GR`). XI = Northern Ireland (post-Brexit VAT
 * arrangement). GB is intentionally excluded — the UK left VIES after Brexit, matching the D390
 * backend generator's prefix set.
 */
const EU_VAT_PREFIXES = new Set([
  "AT","BE","BG","CY","CZ","DE","DK","EE","EL","ES","FI","FR",
  "HR","HU","IE","IT","LT","LU","LV","MT","NL","PL","PT","RO","SE","SI",
  "SK","XI",
]);

interface VatIssue {
  denO: string;
  codO: string;
  tara: string;
  tip: string;
  issue: "missing" | "invalid";
}

/**
 * Validate each D390 operation's partner VAT id for a plausible EU format.
 * A valid id has a 2-letter EU prefix + at least one alphanumeric character.
 */
function findVatIssues(ops: import("@/types").D390Op[]): VatIssue[] {
  const seen = new Map<string, VatIssue>();
  for (const op of ops) {
    const key = `${op.tara}:${op.codO}:${op.denO}`;
    if (seen.has(key)) continue;
    const fullCode = `${op.tara}${op.codO}`.toUpperCase().replace(/\s/g, "");
    if (!op.codO || op.codO.trim() === "") {
      seen.set(key, { ...op, issue: "missing" });
    } else if (
      !EU_VAT_PREFIXES.has(op.tara.toUpperCase()) ||
      !/^[A-Z0-9]+$/.test(op.codO.replace(/\s/g, ""))
    ) {
      seen.set(key, { ...op, issue: "invalid" });
    } else {
      // Full code sanity check: must start with a recognised EU prefix.
      const prefix = fullCode.slice(0, 2);
      if (!EU_VAT_PREFIXES.has(prefix)) {
        seen.set(key, { ...op, issue: "invalid" });
      }
    }
    // Note: every D390 operation — supply (L/T/P/R) AND acquisition (A/S) — needs a valid partner
    // VAT id for VIES, so the missing/invalid check above is applied uniformly to all op types.
  }
  return Array.from(seen.values());
}

export function D390View({ dateFrom, dateTo }: Props) {
  const { t } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [exporting, setExporting] = useState(false);
  const [previewing, setPreviewing] = useState(false);
  const [isRectificative, setIsRectificative] = useState(false);
  const openXml = useOpenXml();

  const tipLabel = (tip: string): string =>
    TIP_KEY[tip] ? t(`declarations.d390.types.${TIP_KEY[tip]}`) : tip;

  const {
    data: doc,
    isLoading,
    isError,
    error,
    refetch,
  } = useQuery({
    queryKey: ["d390", activeCompanyId ?? "", dateFrom, dateTo],
    queryFn: () => api.d390.compute(activeCompanyId!, dateFrom, dateTo),
    enabled: !!activeCompanyId && !!dateFrom && !!dateTo,
    staleTime: 60_000,
  });

  const ops = doc?.operations ?? [];
  const totalBaza = ops.reduce((s, o) => s + o.baza, 0);
  const vatIssues = useMemo(() => findVatIssues(ops), [ops]);

  const handleExport = async () => {
    if (!activeCompanyId) {
      notify.warn(t("declarations.notify.selectCompany"));
      return;
    }
    if (ops.length === 0) {
      notify.info(t("declarations.d390.notify.noOps"));
      return;
    }
    const savePath = await saveDialog({
      title: t("declarations.dialogs.saveD390"),
      defaultPath: `d390-${dateFrom}-${dateTo}.xml`,
      filters: [{ name: "XML", extensions: ["xml"] }],
    });
    if (!savePath) return;
    setExporting(true);
    try {
      const saved = await api.d390.export(activeCompanyId, dateFrom, dateTo, savePath, { dRec: isRectificative });
      notify.success(t("declarations.d390.notify.saved", { path: saved }));
      try {
        await openPath(saved);
      } catch {
        /* reveal best-effort */
      }
    } catch (err) {
      notify.error(formatError(err, t("declarations.d390.notify.exportFailed")));
    } finally {
      setExporting(false);
    }
  };

  /** Construiește XML-ul D390 și îl deschide în vizualizatorul/editorul XML (doar citire — fără DUK). */
  const handlePreview = async () => {
    if (!activeCompanyId) {
      notify.warn(t("declarations.notify.selectCompany"));
      return;
    }
    if (ops.length === 0) {
      notify.info(t("declarations.d390.notify.noOps"));
      return;
    }
    setPreviewing(true);
    try {
      const xml = await api.d390.previewD390Xml(activeCompanyId, dateFrom, dateTo, { dRec: isRectificative });
      openXml({ xml, name: `d390-${dateFrom}-${dateTo}.xml` });
    } catch (err) {
      notify.error(formatError(err, t("declarations.d390.previewFailed")));
    } finally {
      setPreviewing(false);
    }
  };

  return (
    <div className="scr-card">
      <div className="scr-toolbar">
        <div className="tt">{t("declarations.d390.title")}</div>
        <div className="spacer" />
        {/* Declarație rectificativă — toggle vizibil în toolbar lângă butoanele export/preview. */}
        <label className="chk-row" style={{ fontSize: 13, userSelect: "none" }} title={t("declarations.d390.rectificativeHint")}>
          <input
            type="checkbox"
            checked={isRectificative}
            onChange={(e) => setIsRectificative(e.target.checked)}
          />
          <span>{t("declarations.d390.rectificative")}</span>
        </label>
        <button
          className="btn-dark"
          disabled={exporting || !activeCompanyId || ops.length === 0}
          onClick={() => void handleExport()}
          title={t("declarations.d390.exportTitle")}
        >
          <Ic name="dl" />
          {exporting ? t("declarations.common.exporting") : t("declarations.common.exportXml")}
        </button>
        <button
          className="pill-btn"
          disabled={previewing || !activeCompanyId || ops.length === 0}
          onClick={() => void handlePreview()}
          title={t("declarations.d390.previewXml")}
        >
          <Ic name="eye" />
          {previewing ? t("declarations.d390.previewing") : t("declarations.d390.previewXml")}
        </button>
      </div>

      {isLoading ? (
        <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>{t("declarations.common.loading")}</div>
      ) : isError ? (
        <div style={{ padding: 16 }}>
          <QueryErrorBanner error={error} label={t("declarations.d390.reportLabel")} onRetry={() => void refetch()} />
        </div>
      ) : ops.length === 0 ? (
        <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
          {t("declarations.d390.empty")}
        </div>
      ) : (
        <>
          {(doc?.dropped ?? 0) > 0 && (
            <div style={{ padding: "14px 16px 0" }}>
              <div className="banner warn">
                <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_WARN }} />
                <span>
                  <b>{doc!.dropped}</b>{" "}
                  {t("declarations.d390.dropped", { count: doc!.dropped })}{" "}
                  {t("declarations.d390.droppedRest")}
                </span>
              </div>
            </div>
          )}
          {vatIssues.length > 0 && (
            <div style={{ padding: "14px 16px 0" }}>
              <div className="banner warn">
                <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: IC_WARN }} />
                <span>
                  <b>{t("declarations.d390.vatCheck.warnTitle")}</b>
                  {" — "}
                  {t("declarations.d390.vatCheck.warnHint")}
                </span>
              </div>
              <table className="scr-table" style={{ marginTop: 8 }}>
                <thead>
                  <tr>
                    <th>{t("declarations.d390.vatCheck.colPartner")}</th>
                    <th style={{ width: 160 }}>{t("declarations.d390.vatCheck.colCode")}</th>
                    <th style={{ width: 280 }}>{t("declarations.d390.vatCheck.colIssue")}</th>
                    <th style={{ width: 140 }}></th>
                  </tr>
                </thead>
                <tbody>
                  {vatIssues.map((vi, i) => (
                    <tr key={i}>
                      <td style={{ fontWeight: 500 }}>{vi.denO}</td>
                      <td className="doc">{vi.tara}{vi.codO || <span className="muted">—</span>}</td>
                      <td style={{ color: "var(--amber)", fontSize: 12.5 }}>
                        {vi.issue === "missing"
                          ? t("declarations.d390.vatCheck.missingCode")
                          : t("declarations.d390.vatCheck.invalidFormat")}
                      </td>
                      <td>
                        {vi.tara && vi.codO && (
                          <button
                            type="button"
                            className="pill-btn"
                            style={{ height: 28, fontSize: 12 }}
                            title={t("declarations.d390.vatCheck.verifyVies")}
                            onClick={() => {
                              if (!activeCompanyId) return;
                              void api.companies.validateVies(vi.tara, vi.codO).then((r) => {
                                if (r.valid) {
                                  notify.success(
                                    `VIES: ${vi.denO} — ${t("contacts.notify.viesValid") ?? "valid"}`
                                  );
                                } else {
                                  notify.warn(
                                    `VIES: ${vi.denO} — ${t("contacts.notify.viesInvalid") ?? "invalid"}`
                                  );
                                }
                              }).catch((e: unknown) => {
                                notify.error(formatError(e, "VIES error"));
                              });
                            }}
                          >
                            {t("declarations.d390.vatCheck.verifyVies")}
                          </button>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
          <table className="scr-table">
            <thead>
              <tr>
                <th>{t("declarations.d390.headers.type")}</th>
                <th>{t("declarations.d390.headers.country")}</th>
                <th>{t("declarations.d390.headers.code")}</th>
                <th>{t("declarations.d390.headers.name")}</th>
                <th className="r">{t("declarations.d390.headers.base")}</th>
              </tr>
            </thead>
            <tbody>
              {ops.map((o, i) => (
                <tr key={i}>
                  <td>
                    <span className="chip sent">{tipLabel(o.tip)}</span>
                  </td>
                  <td className="doc">{o.tara}</td>
                  <td className="doc">{o.codO}</td>
                  <td style={{ fontWeight: 500 }}>{o.denO}</td>
                  <td className="r num">{fmtLei(o.baza)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="tot-foot">
            <span>
              {t("declarations.d390.totalFoot", { count: ops.length })} <b className="num">{fmtLei(totalBaza)}</b> lei
            </span>
          </div>
        </>
      )}
    </div>
  );
}
