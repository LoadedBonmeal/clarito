/**
 * Ajutor & suport — verbatim port of the design "Ajutor.html":
 *   .page-head (title + sub + head-actions "Documentație e-Factura")
 *   .scr-card "Ghid de pornire" → .steps (done/curr/todo, clicabile spre rute reale)
 *   .cols-2 → FAQ accordion căutabil (.faq-item/.faq-q/.faq-a) ·
 *   "Scurtături" .scr-table (scurtăturile reale ale aplicației) ·
 *   "Contact suport" .set-row (Email mailto + Diagnostic raport tehnic).
 *
 * Wiring real: api.system.appInfo() (versiune în sub), openUrl pentru
 * documentația e-Factura + mailto suport, api.feedback.gather/mailto pentru
 * raportul de diagnostic, useNavigate pentru pașii ghidului.
 */

import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";

const EFACTURA_DOCS_URL = "https://mfinante.gov.ro/ro/web/efactura/informatii-tehnice";
const SUPPORT_EMAIL = "support@lucaris.ro";

// ── Ghid de pornire — pași legați de rutele reale (etichete în help.guide.*) ──

const GUIDE_STEPS: Array<{ cls: "done" | "curr" | "todo"; key: string; to: string }> = [
  { cls: "done", key: "step1", to: "/companies/new" },
  { cls: "done", key: "step2", to: "/settings" },
  { cls: "curr", key: "step3", to: "/invoices/new" },
  { cls: "todo", key: "step4", to: "/reports" },
];

/** FAQ — conținutul în help.faq.q1..q5 / a1..a5. */
const FAQ_COUNT = 5;

// ── Scurtături — scurtăturile reale ale aplicației (etichete în help.shortcuts.*) ──

const SHORTCUTS: Array<{ key: string; keys: string }> = [
  { key: "search",     keys: "⌘ K" },
  { key: "newInvoice", keys: "⌘ N" },
  { key: "refresh",    keys: "F5" },
  { key: "shortcuts",  keys: "⌘ /" },
  { key: "saveDraft",  keys: "⌘ S" },
  { key: "send",       keys: "Ctrl ↵" },
];

/** Căutare insensibilă la diacritice (Factură ↔ factura). */
const fold = (s: string) =>
  s.toLowerCase().normalize("NFD").replace(/[\u0300-\u036f]/g, "");

// ── HelpPage ──────────────────────────────────────────────────────────────────

export function HelpPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [faqQuery, setFaqQuery] = useState("");
  const [openFaq, setOpenFaq] = useState<number | null>(0);
  const [reportSending, setReportSending] = useState(false);

  const { data: appInfo } = useQuery({
    queryKey: queryKeys.appInfo,
    queryFn: () => api.system.appInfo(),
  });

  const faqItems = useMemo(
    () =>
      Array.from({ length: FAQ_COUNT }, (_, i) => ({
        q: t(`help.faq.q${i + 1}`),
        a: t(`help.faq.a${i + 1}`),
        idx: i,
      })),
    [t],
  );

  const filteredFaq = useMemo(() => {
    const q = fold(faqQuery.trim());
    if (!q) return faqItems;
    return faqItems.filter(
      (item) => fold(item.q).includes(q) || fold(item.a).includes(q),
    );
  }, [faqQuery, faqItems]);

  async function handleOpenDocs() {
    try {
      await openUrl(EFACTURA_DOCS_URL);
    } catch (e) {
      notify.error(formatError(e, t("help.notify.docsError")));
    }
  }

  async function handleEmailSupport() {
    try {
      await openUrl(`mailto:${SUPPORT_EMAIL}`);
    } catch (e) {
      notify.error(formatError(e, t("help.notify.mailError", { email: SUPPORT_EMAIL })));
    }
  }

  async function handleSendReport() {
    setReportSending(true);
    try {
      const report = await api.feedback.gather();
      const url = await api.feedback.mailto(report);
      await openUrl(url);
      notify.success(t("help.notify.reportReady"));
    } catch (e) {
      notify.error(formatError(e, t("help.notify.reportError", { email: SUPPORT_EMAIL })));
    } finally {
      setReportSending(false);
    }
  }

  return (
    <div className="main-inner">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>{t("help.title")}</h1>
          <p className="sub">
            {t("help.sub")}
            {appInfo ? t("help.subVersion", { v: appInfo.version }) : ""}
          </p>
        </div>
        <div className="head-actions">
          <button className="pill-btn" onClick={() => void handleOpenDocs()}>
            <Ic name="book" />{t("help.docsBtn")}
          </button>
        </div>
      </div>

      {/* ghid de pornire */}
      <div className="scr-card" style={{ marginBottom: 16 }}>
        <div className="scr-toolbar"><div className="tt">{t("help.guide.title")}</div></div>
        <div className="card-pad">
          <div className="steps">
            {GUIDE_STEPS.map((s) => (
              <div
                key={s.key}
                className={`step ${s.cls} link`}
                role="button"
                tabIndex={0}
                onClick={() => void navigate({ to: s.to })}
                onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); void navigate({ to: s.to }); } }}
              >
                <div className="dot">
                  {(s.cls === "done" || s.cls === "curr") && <Ic name="check" cls="sic" />}
                </div>
                <div className="sl">{t(`help.guide.${s.key}`)}</div>
                <div className="sd">{t(`help.guide.${s.key}Desc`)}</div>
              </div>
            ))}
          </div>
        </div>
      </div>

      <div className="cols-2" style={{ alignItems: "start" }}>
        {/* FAQ căutabil */}
        <div className="scr-card">
          <div className="scr-toolbar">
            <div className="tt">{t("help.faq.title")}</div>
            <div className="spacer" />
            <div className="scr-search" style={{ maxWidth: 220 }}>
              <Ic name="lens" />
              <input
                type="text"
                placeholder={t("help.faq.search")}
                value={faqQuery}
                onChange={(e) => setFaqQuery(e.target.value)}
              />
            </div>
          </div>
          <div id="faq">
            {filteredFaq.length === 0 ? (
              <div style={{ padding: "32px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
                {t("help.faq.empty", { q: faqQuery.trim() })}
              </div>
            ) : (
              filteredFaq.map((item) => (
                <div key={item.idx} className={`faq-item${openFaq === item.idx ? " open" : ""}`}>
                  <button
                    className="faq-q"
                    type="button"
                    onClick={() => setOpenFaq(openFaq === item.idx ? null : item.idx)}
                  >
                    <span>{item.q}</span>
                    <Ic name="chevD" cls="ic faq-chev" />
                  </button>
                  <div className="faq-a"><p>{item.a}</p></div>
                </div>
              ))
            )}
          </div>
        </div>

        <div>
          {/* scurtături */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar"><div className="tt">{t("help.shortcuts.title")}</div></div>
            <table className="scr-table">
              <tbody>
                {SHORTCUTS.map((s) => (
                  <tr key={s.key}>
                    <td>{t(`help.shortcuts.${s.key}`)}</td>
                    <td className="r"><span className="kbd">{s.keys}</span></td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          {/* contact suport */}
          <div className="scr-card">
            <div className="scr-toolbar">
              <div className="tt">{t("help.contact.title")}</div>
              <div className="spacer" />
              <span className="chip paid">
                <svg
                  className="sic"
                  viewBox="0 0 24 24"
                  dangerouslySetInnerHTML={{ __html: '<path d="M9 12.75 11.25 15 15 9.75M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z"/>' }}
                />
                {t("help.contact.systemOk")}
              </span>
            </div>
            <div className="set-row">
              <div>
                <div className="s1">{t("help.contact.email")}</div>
                <div className="s2">{SUPPORT_EMAIL} · {t("help.contact.hours")}</div>
              </div>
              <div className="end">
                <button className="pill-btn" onClick={() => void handleEmailSupport()}>{t("help.contact.writeUs")}</button>
              </div>
            </div>
            <div className="set-row">
              <div>
                <div className="s1">{t("help.contact.diagnostic")}</div>
                <div className="s2">{t("help.contact.diagnosticDesc")}</div>
              </div>
              <div className="end">
                <button
                  className="pill-btn"
                  disabled={reportSending}
                  style={reportSending ? { opacity: 0.6 } : undefined}
                  onClick={() => void handleSendReport()}
                >
                  {reportSending ? t("help.contact.preparing") : t("help.contact.sendReport")}
                </button>
              </div>
            </div>
            <div className="set-row">
              <div>
                <div className="s1">{t("help.contact.appVersion")}</div>
                <div className="s2 num">{appInfo ? `Clarito v${appInfo.version}` : "—"}</div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
