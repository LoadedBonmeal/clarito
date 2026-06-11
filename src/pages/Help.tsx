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

import { Ic } from "@/components/shared/Ic";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";

const EFACTURA_DOCS_URL = "https://mfinante.gov.ro/ro/web/efactura/informatii-tehnice";
const SUPPORT_EMAIL = "support@lucaris.ro";

// ── Ghid de pornire — pași legați de rutele reale ─────────────────────────────

const GUIDE_STEPS: Array<{ cls: "done" | "curr" | "todo"; label: string; desc: string; to: string }> = [
  { cls: "done", label: "Configurează firma",   desc: "CUI + ANAF",          to: "/companies/new" },
  { cls: "done", label: "Conectează SPV",       desc: "OAuth + certificat",  to: "/settings" },
  { cls: "curr", label: "Emite prima factură",  desc: "CIUS-RO",             to: "/invoices/new" },
  { cls: "todo", label: "Trimite D300",         desc: "scadență 25",         to: "/reports" },
];

// ── FAQ — conținutul prototipului, cu diacritice corecte ──────────────────────

const FAQ_ITEMS: Array<{ q: string; a: string }> = [
  {
    q: "Cum trimit o factură prin e-Factura?",
    a: "Creezi factura, o validezi (CIUS-RO), apoi „Validează și trimite la ANAF”. Recipisa apare în Mesaje SPV și se arhivează automat.",
  },
  {
    q: "De ce ANAF păstrează mesajele doar 60 de zile?",
    a: "SPV șterge mesajele după 60 de zile. Clarito sincronizează și arhivează local, așa că nu pierzi recipise sau notificări.",
  },
  {
    q: "Ce plafoane monitorizează aplicația?",
    a: "Micro 100.000 EUR (curs 5,0985 = 509.850 lei), înregistrare TVA 395.000 lei (art. 310) și TVA la încasare 5.000.000 lei.",
  },
  {
    q: "D112 — modelul nou din iulie 2026?",
    a: "Pentru lunile de raportare ≥ iulie 2026 se folosește modelul OPANAF 605/2026; Clarito comută automat.",
  },
  {
    q: "Cum stornez o factură?",
    a: "Din factura validată → „Storno”, introduci motivul; se creează o notă de credit cu valori negative, trimisă separat la ANAF.",
  },
];

// ── Scurtături — scurtăturile reale ale aplicației ────────────────────────────

const SHORTCUTS: Array<{ label: string; keys: string }> = [
  { label: "Căutare",          keys: "⌘ K" },
  { label: "Factură nouă",     keys: "⌘ N" },
  { label: "Reîmprospătare",   keys: "F5" },
  { label: "Scurtături",       keys: "⌘ /" },
  { label: "Salvare ciornă",   keys: "⌘ S" },
  { label: "Trimite",          keys: "Ctrl ↵" },
];

/** Căutare insensibilă la diacritice (Factură ↔ factura). */
const fold = (s: string) =>
  s.toLowerCase().normalize("NFD").replace(/[\u0300-\u036f]/g, "");

// ── HelpPage ──────────────────────────────────────────────────────────────────

export function HelpPage() {
  const navigate = useNavigate();
  const [faqQuery, setFaqQuery] = useState("");
  const [openFaq, setOpenFaq] = useState<number | null>(0);
  const [reportSending, setReportSending] = useState(false);

  const { data: appInfo } = useQuery({
    queryKey: queryKeys.appInfo,
    queryFn: () => api.system.appInfo(),
  });

  const filteredFaq = useMemo(() => {
    const q = fold(faqQuery.trim());
    if (!q) return FAQ_ITEMS.map((item, i) => ({ ...item, idx: i }));
    return FAQ_ITEMS.map((item, i) => ({ ...item, idx: i })).filter(
      (item) => fold(item.q).includes(q) || fold(item.a).includes(q),
    );
  }, [faqQuery]);

  async function handleOpenDocs() {
    try {
      await openUrl(EFACTURA_DOCS_URL);
    } catch (e) {
      notify.error(formatError(e, "Nu pot deschide documentația e-Factura."));
    }
  }

  async function handleEmailSupport() {
    try {
      await openUrl(`mailto:${SUPPORT_EMAIL}`);
    } catch (e) {
      notify.error(formatError(e, `Nu pot deschide clientul de email — scrieți la ${SUPPORT_EMAIL}.`));
    }
  }

  async function handleSendReport() {
    setReportSending(true);
    try {
      const report = await api.feedback.gather();
      const url = await api.feedback.mailto(report);
      await openUrl(url);
      notify.success("Email pregătit în clientul dvs. de email.");
    } catch (e) {
      notify.error(formatError(e, `Nu pot deschide clientul de email — trimiteți manual la ${SUPPORT_EMAIL}.`));
    } finally {
      setReportSending(false);
    }
  }

  return (
    <div className="main-inner">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>Ajutor &amp; suport</h1>
          <p className="sub">
            Ghiduri, întrebări frecvente, scurtături și contact
            {appInfo ? ` · versiunea ${appInfo.version}` : ""}
          </p>
        </div>
        <div className="head-actions">
          <button className="pill-btn" onClick={() => void handleOpenDocs()}>
            <Ic name="book" />Documentație e-Factura
          </button>
        </div>
      </div>

      {/* ghid de pornire */}
      <div className="scr-card" style={{ marginBottom: 16 }}>
        <div className="scr-toolbar"><div className="tt">Ghid de pornire</div></div>
        <div className="card-pad">
          <div className="steps">
            {GUIDE_STEPS.map((s) => (
              <div
                key={s.label}
                className={`step ${s.cls} link`}
                role="button"
                tabIndex={0}
                onClick={() => void navigate({ to: s.to })}
                onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); void navigate({ to: s.to }); } }}
              >
                <div className="dot">
                  {(s.cls === "done" || s.cls === "curr") && <Ic name="check" cls="sic" />}
                </div>
                <div className="sl">{s.label}</div>
                <div className="sd">{s.desc}</div>
              </div>
            ))}
          </div>
        </div>
      </div>

      <div className="cols-2" style={{ alignItems: "start" }}>
        {/* FAQ căutabil */}
        <div className="scr-card">
          <div className="scr-toolbar">
            <div className="tt">Întrebări frecvente</div>
            <div className="spacer" />
            <div className="scr-search" style={{ maxWidth: 220 }}>
              <Ic name="lens" />
              <input
                type="text"
                placeholder="Caută în întrebări…"
                value={faqQuery}
                onChange={(e) => setFaqQuery(e.target.value)}
              />
            </div>
          </div>
          <div id="faq">
            {filteredFaq.length === 0 ? (
              <div style={{ padding: "32px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
                Nicio întrebare găsită pentru „{faqQuery.trim()}”.
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
            <div className="scr-toolbar"><div className="tt">Scurtături</div></div>
            <table className="scr-table">
              <tbody>
                {SHORTCUTS.map((s) => (
                  <tr key={s.label}>
                    <td>{s.label}</td>
                    <td className="r"><span className="kbd">{s.keys}</span></td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          {/* contact suport */}
          <div className="scr-card">
            <div className="scr-toolbar">
              <div className="tt">Contact suport</div>
              <div className="spacer" />
              <span className="chip paid">
                <svg
                  className="sic"
                  viewBox="0 0 24 24"
                  dangerouslySetInnerHTML={{ __html: '<path d="M9 12.75 11.25 15 15 9.75M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z"/>' }}
                />
                Sistem operațional
              </span>
            </div>
            <div className="set-row">
              <div>
                <div className="s1">Email</div>
                <div className="s2">{SUPPORT_EMAIL} · Lu–Vi 09:00–18:00</div>
              </div>
              <div className="end">
                <button className="pill-btn" onClick={() => void handleEmailSupport()}>Scrie-ne</button>
              </div>
            </div>
            <div className="set-row">
              <div>
                <div className="s1">Diagnostic</div>
                <div className="s2">trimite un raport tehnic către suport</div>
              </div>
              <div className="end">
                <button
                  className="pill-btn"
                  disabled={reportSending}
                  style={reportSending ? { opacity: 0.6 } : undefined}
                  onClick={() => void handleSendReport()}
                >
                  {reportSending ? "Se pregătește…" : "Trimite raport"}
                </button>
              </div>
            </div>
            <div className="set-row">
              <div>
                <div className="s1">Versiune aplicație</div>
                <div className="s2 num">{appInfo ? `Clarito v${appInfo.version}` : "—"}</div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
