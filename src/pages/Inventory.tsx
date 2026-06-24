/**
 * Inventariere — pagina principală.
 *
 * Implementează OMFP 2861/2009 + OMFP 2634/2015 form 14-3-12 (Listă de inventariere).
 *
 * Flux:
 *   1. Creează sesiune (dată, an fiscal, tip, gestiune, comisie)
 *   2. «Pre-completează din stoc» — populează liniile din stock_qty + avg_cost
 *   3. User introduce qty_faptic per linie + cauza diferenței
 *   4. «Finalizează» → FINALIZED (snapshot în registru-inventar)
 *   5. Opțional «Postează diferențele în GL» (neimputabil D607/stoc, imputabil → manual)
 */

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { confirm } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { fmtRON, parseDec } from "@/lib/utils";
import type {
  CreateInventorySessionInput,
  InventoryLine,
  InventorySession,
  InventoryDiffCause,
  UpdateInventoryLineFapticInput,
} from "@/types";

// ─── Helpers ─────────────────────────────────────────────────────────────────

const today = () => new Date().toISOString().slice(0, 10);
const currentYear = () => new Date().getFullYear();

function diffColor(diff: string) {
  const v = parseDec(diff);
  if (v < 0) return "var(--red)";
  if (v > 0) return "var(--green)";
  return "var(--text-2)";
}

const SESSION_TYPES = [
  "ANUAL",
  "INCEPERE",
  "INCETARE",
  "PREDARE_GESTIUNE",
  "CALAMITATE",
] as const;

const DIFF_CAUSES: Array<InventoryDiffCause | ""> = [
  "",
  "perisabilitati",
  "imputabil",
  "neimputabil",
  "depreciere",
  "altele",
];

// ─── CreateSessionModal ───────────────────────────────────────────────────────

interface CreateModalProps {
  companyId: string;
  onClose: () => void;
  onCreated: (s: InventorySession) => void;
}

function CreateSessionModal({ companyId, onClose, onCreated }: CreateModalProps) {
  const { t } = useTranslation();
  const [referenceDate, setReferenceDate] = useState(today());
  const [fiscalYear, setFiscalYear] = useState(String(currentYear()));
  const [type, setType] = useState<string>("ANUAL");
  const [gestiune, setGestiune] = useState("");
  const [comisie, setComisie] = useState("");
  const [notes, setNotes] = useState("");
  const [loading, setLoading] = useState(false);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setLoading(true);
    try {
      // Comisia de inventariere (OMFP 2861/2009) — nume separate prin virgulă → JSON array.
      const members = comisie
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean);
      const input: CreateInventorySessionInput = {
        companyId,
        referenceDate,
        fiscalYear: Number(fiscalYear),
        type: type as CreateInventorySessionInput["type"],
        gestiune: gestiune || undefined,
        comisieMembers: members.length ? JSON.stringify(members) : undefined,
        notes: notes || undefined,
      };
      const s = await api.inventory.createSession(input);
      notify.success(t("inventory.messages.sessionCreated"));
      onCreated(s);
    } catch (err) {
      notify.error(formatError(err));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="modal-back" onClick={onClose}>
      <div className="modal" style={{ maxWidth: 480 }} onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <span>{t("inventory.newSession")}</span>
          <button className="ic-btn" onClick={onClose}><Ic name="x" /></button>
        </div>
        <form onSubmit={(e) => void handleSubmit(e)}>
          <div className="modal-body" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            <label className="field">
              <span className="flabel">{t("inventory.form.referenceDate")}</span>
              <input type="date" className="finput" value={referenceDate}
                onChange={(e) => setReferenceDate(e.target.value)} required />
            </label>
            <label className="field">
              <span className="flabel">{t("inventory.form.fiscalYear")}</span>
              <input type="number" className="finput" value={fiscalYear}
                onChange={(e) => setFiscalYear(e.target.value)} min={2000} max={2100} required />
            </label>
            <label className="field">
              <span className="flabel">{t("inventory.form.type")}</span>
              <select className="fsel" value={type} onChange={(e) => setType(e.target.value)}>
                {SESSION_TYPES.map((tp) => (
                  <option key={tp} value={tp}>{t(`inventory.sessionType.${tp}`)}</option>
                ))}
              </select>
            </label>
            <label className="field">
              <span className="flabel">{t("inventory.form.gestiune")}</span>
              <input className="finput" value={gestiune} onChange={(e) => setGestiune(e.target.value)}
                placeholder={t("inventory.form.gestiuonePlaceholder")} />
            </label>
            <label className="field">
              <span className="flabel">{t("inventory.form.comisieMembers")}</span>
              <input className="finput" value={comisie} onChange={(e) => setComisie(e.target.value)}
                placeholder={t("inventory.form.comisieMembersPlaceholder")} />
            </label>
            <label className="field">
              <span className="flabel">{t("inventory.form.notes")}</span>
              <textarea className="finput" value={notes} onChange={(e) => setNotes(e.target.value)}
                rows={2} style={{ resize: "vertical" }} />
            </label>
          </div>
          <div className="modal-foot">
            <button type="button" className="btn" onClick={onClose}>{t("inventory.form.cancel")}</button>
            <button type="submit" className="btn-dark" disabled={loading}>
              {loading ? "…" : t("inventory.form.create")}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

// ─── SessionDetail ────────────────────────────────────────────────────────────

interface SessionDetailProps {
  session: InventorySession;
  companyId: string;
  onBack: () => void;
  onSessionChanged: () => void;
}

function SessionDetail({ session, companyId, onBack, onSessionChanged }: SessionDetailProps) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [pendingFaptic, setPendingFaptic] = useState<Record<string, string>>({});
  const [pendingCause, setPendingCause] = useState<Record<string, string>>({});

  const { data: lines = [], refetch: refetchLines } = useQuery({
    queryKey: ["inventory-lines", session.id],
    queryFn: () => api.inventory.listLines(session.id, companyId),
  });

  const prefillMut = useMutation({
    mutationFn: () => api.inventory.prefillSession(session.id, companyId),
    onSuccess: () => {
      notify.success(t("inventory.messages.prefillDone"));
      void refetchLines();
    },
    onError: (err) => notify.error(formatError(err)),
  });

  const updateLineMut = useMutation({
    mutationFn: (input: UpdateInventoryLineFapticInput) => api.inventory.updateLineFaptic(input),
    onSuccess: () => void refetchLines(),
    onError: (err) => notify.error(formatError(err)),
  });

  const finalizeMut = useMutation({
    mutationFn: () => api.inventory.finalizeSession(session.id, companyId),
    onSuccess: () => {
      notify.success(t("inventory.messages.finalized"));
      void qc.invalidateQueries({ queryKey: ["inventory-sessions"] });
      onSessionChanged();
    },
    onError: (err) => notify.error(formatError(err)),
  });

  const postDiffsMut = useMutation({
    mutationFn: () => api.inventory.postDiffs(session.id, companyId),
    onSuccess: () => notify.success(t("inventory.messages.glPosted")),
    onError: (err) => notify.error(formatError(err)),
  });

  const isFinalized = session.status === "FINALIZED";

  const handleBlurFaptic = (line: InventoryLine) => {
    const raw = pendingFaptic[line.id];
    if (raw === undefined) return;
    const cause = (pendingCause[line.id] ?? line.diffCause ?? "") as InventoryDiffCause | undefined;
    void updateLineMut.mutateAsync({
      lineId: line.id,
      sessionId: session.id,
      companyId,
      qtyFaptic: raw || line.qtyFaptic,
      diffCause: cause || undefined,
      imputable: cause === "imputabil",
    });
  };

  const handleFinalize = async () => {
    const ok = await confirm(t("inventory.messages.confirmFinalize"), { kind: "warning" });
    if (ok) finalizeMut.mutate();
  };

  const handlePostDiffs = async () => {
    const ok = await confirm(t("inventory.messages.confirmPostDiffs"), { kind: "warning" });
    if (ok) postDiffsMut.mutate();
  };

  // Totals
  const totalContabila = lines.reduce((s, l) => s + parseDec(l.valueContabila), 0);
  const totalInventar = lines.reduce((s, l) => s + parseDec(l.valueInventar), 0);
  const totalDiff = lines.reduce((s, l) => s + parseDec(l.diffValue), 0);

  return (
    <div>
      {/* Sub-header */}
      <div className="page-head" style={{ marginBottom: 16 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <button className="ic-btn" onClick={onBack} title="Înapoi"><Ic name="chevL" /></button>
          <div>
            <h1 className="page-title">{t("inventory.sessionOf", { date: session.referenceDate })}</h1>
            <div className="page-sub">
              {t(`inventory.sessionType.${session.type}`)}
              {session.gestiune ? ` — ${session.gestiune}` : ""}
              {" · "}
              <span className={`pill-new ${isFinalized ? "" : "draft"}`}
                style={{ background: isFinalized ? "var(--green)" : "var(--fill)", color: isFinalized ? "#fff" : "var(--text-2)" }}>
                {t(`inventory.sessionStatus.${session.status}`)}
              </span>
            </div>
          </div>
        </div>
        <div style={{ display: "flex", gap: 8 }}>
          {!isFinalized && (
            <button className="btn" onClick={() => prefillMut.mutate()} disabled={prefillMut.isPending}>
              {prefillMut.isPending ? "…" : t("inventory.prefill")}
            </button>
          )}
          {!isFinalized && lines.length > 0 && (
            <button className="btn-dark" onClick={() => void handleFinalize()} disabled={finalizeMut.isPending}>
              {finalizeMut.isPending ? "…" : t("inventory.finalize")}
            </button>
          )}
          {isFinalized && (
            <button className="btn" onClick={() => void handlePostDiffs()} disabled={postDiffsMut.isPending}>
              {postDiffsMut.isPending ? "…" : t("inventory.postDiffs")}
            </button>
          )}
        </div>
      </div>

      {/* GL deferred note */}
      {isFinalized && (
        <div className="scr-card" style={{ marginBottom: 12, padding: "10px 16px", background: "var(--fill)", borderLeft: "3px solid var(--orange, #f59e0b)" }}>
          <span style={{ fontSize: 12, color: "var(--text-2)" }}>{t("inventory.messages.glDeferredNote")}</span>
        </div>
      )}

      {/* Lines table */}
      {lines.length === 0 ? (
        <div className="scr-card" style={{ padding: 32, textAlign: "center", color: "var(--text-2)" }}>
          {t("inventory.noLines")}
        </div>
      ) : (
        <div className="scr-card">
          <div style={{ overflowX: "auto" }}>
            <table className="scr-table" style={{ minWidth: 900, fontSize: 13 }}>
              <thead>
                <tr>
                  <th>{t("inventory.table.account")}</th>
                  <th>{t("inventory.table.itemName")}</th>
                  <th>{t("inventory.table.um")}</th>
                  <th className="num">{t("inventory.table.qtyScriptic")}</th>
                  <th className="num">{t("inventory.table.qtyFaptic")}</th>
                  <th className="num">{t("inventory.table.unitPrice")}</th>
                  <th className="num">{t("inventory.table.valueContabila")}</th>
                  <th className="num">{t("inventory.table.valueInventar")}</th>
                  <th className="num">{t("inventory.table.diffValue")}</th>
                  <th>{t("inventory.table.diffCause")}</th>
                </tr>
              </thead>
              <tbody>
                {lines.map((line) => (
                  <tr key={line.id}>
                    <td><span className="code num">{line.accountCode}</span></td>
                    <td>{line.itemName}</td>
                    <td>{line.um}</td>
                    <td className="num">{parseDec(line.qtyScriptic).toFixed(3)}</td>
                    <td className="num">
                      {isFinalized ? (
                        <span>{parseDec(line.qtyFaptic).toFixed(3)}</span>
                      ) : (
                        <input
                          type="number"
                          step="0.001"
                          min="0"
                          className="finput num"
                          style={{ width: 80, textAlign: "right", padding: "2px 4px" }}
                          defaultValue={pendingFaptic[line.id] ?? parseDec(line.qtyFaptic).toFixed(3)}
                          onChange={(e) =>
                            setPendingFaptic((prev) => ({ ...prev, [line.id]: e.target.value }))
                          }
                          onBlur={() => handleBlurFaptic(line)}
                        />
                      )}
                    </td>
                    <td className="num">{fmtRON(line.unitPrice)}</td>
                    <td className="num">{fmtRON(line.valueContabila)}</td>
                    <td className="num">{fmtRON(line.valueInventar)}</td>
                    <td className="num" style={{ color: diffColor(line.diffValue), fontWeight: 600 }}>
                      {parseDec(line.diffValue) !== 0
                        ? (parseDec(line.diffValue) > 0 ? "+" : "") + fmtRON(line.diffValue)
                        : "—"}
                    </td>
                    <td>
                      {isFinalized ? (
                        <span>{line.diffCause ? t(`inventory.diffCause.${line.diffCause}`) : "—"}</span>
                      ) : (
                        <select
                          className="fsel"
                          style={{ fontSize: 12, padding: "2px 4px" }}
                          value={pendingCause[line.id] ?? (line.diffCause ?? "")}
                          onChange={(e) => {
                            setPendingCause((prev) => ({ ...prev, [line.id]: e.target.value }));
                            // Immediately persist cause change.
                            void updateLineMut.mutateAsync({
                              lineId: line.id,
                              sessionId: session.id,
                              companyId,
                              qtyFaptic: pendingFaptic[line.id] ?? line.qtyFaptic,
                              diffCause: (e.target.value as InventoryDiffCause) || undefined,
                              imputable: e.target.value === "imputabil",
                            });
                          }}
                        >
                          {DIFF_CAUSES.map((c) => (
                            <option key={c} value={c}>{t(`inventory.diffCause.${c}`)}</option>
                          ))}
                        </select>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
              <tfoot>
                <tr style={{ fontWeight: 700 }}>
                  <td colSpan={6} style={{ textAlign: "right" }}>TOTAL</td>
                  <td className="num">{fmtRON(totalContabila)}</td>
                  <td className="num">{fmtRON(totalInventar)}</td>
                  <td className="num" style={{ color: diffColor(String(totalDiff)) }}>
                    {totalDiff !== 0 ? (totalDiff > 0 ? "+" : "") + fmtRON(totalDiff) : "—"}
                  </td>
                  <td />
                </tr>
              </tfoot>
            </table>
          </div>
        </div>
      )}
    </div>
  );
}

// ─── Main page ────────────────────────────────────────────────────────────────

export function InventoryPage() {
  const { t } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const qc = useQueryClient();
  const [showCreate, setShowCreate] = useState(false);
  const [activeSession, setActiveSession] = useState<InventorySession | null>(null);
  const [filterYear, setFilterYear] = useState<number | undefined>(currentYear());

  const { data: sessions = [] } = useQuery({
    queryKey: ["inventory-sessions", activeCompanyId, filterYear],
    queryFn: () =>
      activeCompanyId
        ? api.inventory.listSessions(activeCompanyId, filterYear)
        : Promise.resolve([]),
    enabled: !!activeCompanyId,
  });

  const deleteMut = useMutation({
    mutationFn: (id: string) => api.inventory.deleteSession(id, activeCompanyId!),
    onSuccess: () => {
      notify.success(t("inventory.messages.deleted"));
      void qc.invalidateQueries({ queryKey: ["inventory-sessions"] });
    },
    onError: (err) => notify.error(formatError(err)),
  });

  const handleDelete = async (s: InventorySession) => {
    const ok = await confirm(t("inventory.messages.confirmDelete"), { kind: "warning" });
    if (ok) deleteMut.mutate(s.id);
  };

  if (!activeCompanyId) {
    return (
      <div className="main-inner" style={{ padding: 32, color: "var(--text-2)" }}>
        Selectați o companie activă.
      </div>
    );
  }

  if (activeSession) {
    return (
      <div className="main-inner">
        <SessionDetail
          session={activeSession}
          companyId={activeCompanyId}
          onBack={() => setActiveSession(null)}
          onSessionChanged={() => {
            void qc.invalidateQueries({ queryKey: ["inventory-sessions"] });
            setActiveSession(null);
          }}
        />
      </div>
    );
  }

  return (
    <div className="main-inner">
      {showCreate && (
        <CreateSessionModal
          companyId={activeCompanyId}
          onClose={() => setShowCreate(false)}
          onCreated={(s) => {
            setShowCreate(false);
            void qc.invalidateQueries({ queryKey: ["inventory-sessions"] });
            setActiveSession(s);
          }}
        />
      )}

      {/* Page header */}
      <div className="page-head">
        <div>
          <h1 className="page-title">{t("inventory.pageTitle")}</h1>
          <div className="page-sub">{t("inventory.pageSubtitle")}</div>
        </div>
        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
          {/* Year filter */}
          <select
            className="fsel"
            value={filterYear ?? ""}
            onChange={(e) =>
              setFilterYear(e.target.value ? Number(e.target.value) : undefined)
            }
            style={{ minWidth: 90 }}
          >
            <option value="">{t("inventory.registru.yearPicker")} (toate)</option>
            {Array.from({ length: 6 }, (_, i) => currentYear() - i).map((y) => (
              <option key={y} value={y}>{y}</option>
            ))}
          </select>
          <button className="btn-dark" onClick={() => setShowCreate(true)}>
            <Ic name="plus" /> {t("inventory.newSession")}
          </button>
        </div>
      </div>

      {/* Sessions list */}
      {sessions.length === 0 ? (
        <div className="state-row muted">{t("inventory.empty")}</div>
      ) : (
        <div className="scr-card">
          <table className="scr-table">
            <thead>
              <tr>
                <th>Data de referință</th>
                <th>An fiscal</th>
                <th>Tip</th>
                <th>Gestiune</th>
                <th>Status</th>
                <th style={{ width: 80 }} />
              </tr>
            </thead>
            <tbody>
              {sessions.map((s) => (
                <tr key={s.id} className="row-click" onClick={() => setActiveSession(s)}>
                  <td className="num">{s.referenceDate}</td>
                  <td className="num">{s.fiscalYear}</td>
                  <td>{t(`inventory.sessionType.${s.type}`)}</td>
                  <td>{s.gestiune ?? "—"}</td>
                  <td>
                    <span
                      className="pill-new"
                      style={{
                        background: s.status === "FINALIZED" ? "var(--green)" : "var(--fill)",
                        color: s.status === "FINALIZED" ? "#fff" : "var(--text-2)",
                      }}
                    >
                      {t(`inventory.sessionStatus.${s.status}`)}
                    </span>
                  </td>
                  <td onClick={(e) => e.stopPropagation()}>
                    {s.status === "DRAFT" && (
                      <button
                        className="ic-btn"
                        title={t("inventory.deleteSession")}
                        onClick={() => void handleDelete(s)}
                      >
                        <Ic name="trash" />
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
