/**
 * BankImport.tsx — Import extras bancar (jurnal de bancă).
 *
 * Supports MT940, CAMT.053 and CSV formats.
 * The importer SUGGESTS invoice matches; the user must explicitly confirm
 * each match before a payment is recorded. No auto-posting.
 *
 * Flow:
 *   1. Pick format + file → parse → preview statement header + transaction list
 *   2. Each UNMATCHED transaction shows suggested invoice matches with confidence
 *   3. User can: Confirm match (records payment), Ignore (bank fee / internal), or leave
 *   4. Summary counts: matched / unmatched / ignored
 */

import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { api, BankStatement, BankTransaction } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { parseDec, fmtRON } from "@/lib/utils";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import { isDemoMode } from "@/lib/demo";

// ─── Helpers ─────────────────────────────────────────────────────────────────

const fmtAmt = (s: string) => {
  const d = parseDec(s);
  return d >= 0 ? `+${fmtRON(d)}` : fmtRON(d);
};

const amtCls = (s: string) =>
  parseDec(s) >= 0 ? "num" : "num text-red";

const STATUS_BADGE: Record<string, string> = {
  UNMATCHED: "badge badge-warn",
  MATCHED:   "badge badge-ok",
  IGNORED:   "badge badge-muted",
};

// ─── StatementHeader ─────────────────────────────────────────────────────────

function StatementHeader({
  stmt,
  integrityOk,
  warnings,
}: {
  stmt: BankStatement;
  integrityOk: boolean | null;
  warnings: string[];
}) {
  const { t } = useTranslation();

  return (
    <div className="scr-card" style={{ marginBottom: 12 }}>
      <div style={{ display: "flex", gap: 24, flexWrap: "wrap", padding: "12px 16px" }}>
        <div>
          <div style={{ fontSize: 11, color: "var(--text-2)", marginBottom: 2 }}>
            {t("bankImport.ref")}
          </div>
          <div style={{ fontWeight: 600, fontSize: 13 }}>
            {stmt.statementRef || "—"}
          </div>
        </div>
        <div>
          <div style={{ fontSize: 11, color: "var(--text-2)", marginBottom: 2 }}>
            {t("bankImport.date")}
          </div>
          <div style={{ fontSize: 13 }}>{stmt.statementDate || "—"}</div>
        </div>
        <div>
          <div style={{ fontSize: 11, color: "var(--text-2)", marginBottom: 2 }}>
            {t("bankImport.opening")}
          </div>
          <div className="num" style={{ fontSize: 13 }}>
            {fmtRON(parseDec(stmt.openingBalance))} {stmt.sourceFormat === "CSV" ? "" : "RON"}
          </div>
        </div>
        <div>
          <div style={{ fontSize: 11, color: "var(--text-2)", marginBottom: 2 }}>
            {t("bankImport.closing")}
          </div>
          <div className="num" style={{ fontSize: 13 }}>
            {fmtRON(parseDec(stmt.closingBalance))} {stmt.sourceFormat === "CSV" ? "" : "RON"}
          </div>
        </div>
        <div>
          <div style={{ fontSize: 11, color: "var(--text-2)", marginBottom: 2 }}>
            {t("bankImport.integrity")}
          </div>
          <div style={{ fontSize: 13 }}>
            {integrityOk === null
              ? "N/A"
              : integrityOk
              ? <span style={{ color: "var(--accent-green)" }}>✓ OK</span>
              : <span style={{ color: "var(--accent-red)" }}>⚠ {t("bankImport.integrityFail")}</span>}
          </div>
        </div>
      </div>
      {warnings.length > 0 && (
        <div style={{ padding: "8px 16px", borderTop: "1px solid var(--border)", background: "var(--warn-bg)" }}>
          {warnings.map((w, i) => (
            <div key={i} style={{ fontSize: 12, color: "var(--warn-text)" }}>⚠ {w}</div>
          ))}
        </div>
      )}
    </div>
  );
}

// ─── TxnRow ──────────────────────────────────────────────────────────────────

function TxnRow({
  txn,
  companyId,
  onRefresh,
}: {
  txn: BankTransaction;
  companyId: string;
  onRefresh: () => void;
}) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [expanded, setExpanded] = useState(false);

  const handleMatch = async (invoiceId: string, direction: string) => {
    setBusy(true);
    try {
      await api.bankImport.matchTxn({
        txnId: txn.id,
        companyId,
        invoiceId,
        direction,
      });
      notify.success(t("bankImport.matchedOk"));
      onRefresh();
    } catch (e) {
      notify.error(formatError(e, t("bankImport.matchError")));
    } finally {
      setBusy(false);
    }
  };

  const handleUnmatch = async () => {
    setBusy(true);
    try {
      await api.bankImport.unmatchTxn(txn.id, companyId);
      notify.success(t("bankImport.unmatchedOk"));
      onRefresh();
    } catch (e) {
      notify.error(formatError(e, t("bankImport.unmatchError")));
    } finally {
      setBusy(false);
    }
  };

  const handleIgnore = async () => {
    setBusy(true);
    try {
      await api.bankImport.ignoreTxn(txn.id, companyId);
      notify.success(t("bankImport.ignoredOk"));
      onRefresh();
    } catch (e) {
      notify.error(formatError(e, t("bankImport.ignoreError")));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <tr
        className={`clickable${txn.status === "MATCHED" ? " row-matched" : txn.status === "IGNORED" ? " row-ignored" : ""}`}
        onClick={() => setExpanded((e) => !e)}
        style={{ opacity: busy ? 0.5 : 1 }}
      >
        <td className="num" style={{ width: 96 }}>{txn.bookingDate}</td>
        <td>{txn.counterpartyName || <span className="muted">—</span>}</td>
        <td style={{ maxWidth: 220, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
          <span title={txn.reference ?? ""} style={{ fontSize: 12, color: "var(--text-2)" }}>
            {txn.reference || "—"}
          </span>
        </td>
        <td className={amtCls(txn.amount)} style={{ textAlign: "right" }}>
          {fmtAmt(txn.amount)} {txn.currency}
        </td>
        <td style={{ width: 90 }}>
          <span className={STATUS_BADGE[txn.status] ?? "badge"}>
            {t(`bankImport.status.${txn.status}`, txn.status)}
          </span>
        </td>
        <td style={{ width: 100, textAlign: "right" }}>
          {txn.status === "UNMATCHED" && txn.suggestions.length > 0 && (
            <span style={{ fontSize: 11, color: "var(--accent)" }}>
              {txn.suggestions.length} {t("bankImport.suggestions")}
            </span>
          )}
          {txn.status === "MATCHED" && (
            <button
              className="pill-btn xs"
              disabled={busy}
              onClick={(e) => { e.stopPropagation(); void handleUnmatch(); }}
            >
              {t("bankImport.unmatch")}
            </button>
          )}
        </td>
      </tr>

      {/* Expanded detail: suggestions + actions */}
      {expanded && txn.status === "UNMATCHED" && (
        <tr>
          <td colSpan={6} style={{ background: "var(--fill)", padding: "8px 16px" }}>
            <div style={{ display: "flex", gap: 8, flexWrap: "wrap", alignItems: "flex-start" }}>
              {txn.suggestions.length > 0 ? (
                <div style={{ flex: 1 }}>
                  <div style={{ fontSize: 11, fontWeight: 600, marginBottom: 6, color: "var(--text-2)" }}>
                    {t("bankImport.suggestTitle")}
                  </div>
                  <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                    {txn.suggestions.map((s) => (
                      <div
                        key={s.invoiceId}
                        style={{
                          display: "flex", gap: 8, alignItems: "center",
                          padding: "6px 10px", borderRadius: 6,
                          background: "var(--bg-card)",
                          border: s.confidence === "HIGH"
                            ? "1px solid var(--accent)"
                            : "1px solid var(--border)",
                        }}
                      >
                        <span style={{ fontSize: 11, color: "var(--text-2)" }}>
                          {s.direction === "issued" ? "4111" : "401"}
                        </span>
                        <span style={{ fontSize: 13, fontWeight: 600 }}>
                          {s.invoiceNumber || s.invoiceId.slice(0, 8)}
                        </span>
                        {s.partnerName && (
                          <span style={{ fontSize: 12, color: "var(--text-2)" }}>{s.partnerName}</span>
                        )}
                        <span className="num" style={{ fontSize: 12, marginLeft: "auto" }}>
                          {fmtRON(parseDec(s.outstanding))}
                        </span>
                        <span style={{
                          fontSize: 10, fontWeight: 700,
                          color: s.confidence === "HIGH" ? "var(--accent)" : "var(--text-3)",
                        }}>
                          {s.confidence}
                        </span>
                        <button
                          className="pill-btn xs primary"
                          disabled={busy}
                          onClick={(e) => {
                            e.stopPropagation();
                            void handleMatch(s.invoiceId, s.direction);
                          }}
                        >
                          {t("bankImport.confirm")}
                        </button>
                      </div>
                    ))}
                  </div>
                </div>
              ) : (
                <div style={{ fontSize: 12, color: "var(--text-2)", flexShrink: 0 }}>
                  {t("bankImport.noSuggestions")}
                </div>
              )}

              {/* Ignore button */}
              <button
                className="pill-btn xs"
                disabled={busy}
                style={{ alignSelf: "flex-start", marginTop: txn.suggestions.length > 0 ? 20 : 0 }}
                onClick={(e) => { e.stopPropagation(); void handleIgnore(); }}
              >
                <Ic name="x" />
                {t("bankImport.ignore")}
              </button>
            </div>
          </td>
        </tr>
      )}
    </>
  );
}

// ─── StatementView ────────────────────────────────────────────────────────────

function StatementView({
  stmt,
  companyId,
  integrityOk,
  warnings,
}: {
  stmt: BankStatement;
  companyId: string;
  integrityOk: boolean | null;
  warnings: string[];
}) {
  const { t } = useTranslation();
  const [txns, setTxns] = useState<BankTransaction[] | null>(null);
  const [loading, setLoading] = useState(false);

  const loadTxns = async () => {
    setLoading(true);
    try {
      const rows = await api.bankImport.listTransactions(stmt.id, companyId);
      setTxns(rows);
    } catch (e) {
      notify.error(formatError(e, t("bankImport.loadError")));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { void loadTxns(); }, [stmt.id]);

  const matched  = txns?.filter((t) => t.status === "MATCHED").length ?? 0;
  const ignored  = txns?.filter((t) => t.status === "IGNORED").length ?? 0;
  const unmatched = txns?.filter((t) => t.status === "UNMATCHED").length ?? 0;

  return (
    <div>
      <StatementHeader stmt={stmt} integrityOk={integrityOk} warnings={warnings} />

      {/* Summary counts */}
      {txns && (
        <div style={{ display: "flex", gap: 16, marginBottom: 12 }}>
          <div className="stat-chip">
            <span className="badge badge-ok">{matched}</span>
            <span style={{ fontSize: 12 }}>{t("bankImport.matched")}</span>
          </div>
          <div className="stat-chip">
            <span className="badge badge-warn">{unmatched}</span>
            <span style={{ fontSize: 12 }}>{t("bankImport.unmatched")}</span>
          </div>
          <div className="stat-chip">
            <span className="badge badge-muted">{ignored}</span>
            <span style={{ fontSize: 12 }}>{t("bankImport.ignored")}</span>
          </div>
        </div>
      )}

      <div className="scr-card">
        {loading ? (
          <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>
            {t("gl.common.loading")}
          </div>
        ) : txns && txns.length === 0 ? (
          <div style={{ padding: 32, textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
            {t("bankImport.noTxns")}
          </div>
        ) : (
          <table className="scr-table">
            <thead>
              <tr>
                <th>{t("bankImport.colDate")}</th>
                <th>{t("bankImport.colCounterparty")}</th>
                <th>{t("bankImport.colReference")}</th>
                <th className="r">{t("bankImport.colAmount")}</th>
                <th>{t("bankImport.colStatus")}</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {txns?.map((txn) => (
                <TxnRow
                  key={txn.id}
                  txn={txn}
                  companyId={companyId}
                  onRefresh={loadTxns}
                />
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}

// ─── BankImportPage ───────────────────────────────────────────────────────────

export function BankImportPage() {
  const { t } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);

  const [format, setFormat] = useState<"MT940" | "CAMT053" | "CSV">("MT940");
  const [importing, setImporting] = useState(false);
  const [statements, setStatements] = useState<BankStatement[]>([]);
  const [loadingStmts, setLoadingStmts] = useState(false);
  const [activeStmt, setActiveStmt] = useState<BankStatement | null>(null);
  const [activeIntegrity, setActiveIntegrity] = useState<boolean | null>(null);
  const [activeWarnings, setActiveWarnings] = useState<string[]>([]);

  const fileInputRef = useRef<HTMLInputElement>(null);

  const loadStatements = async (cid: string) => {
    setLoadingStmts(true);
    try {
      const rows = await api.bankImport.listStatements(cid);
      setStatements(rows);
    } catch (e) {
      notify.error(formatError(e, t("bankImport.loadError")));
    } finally {
      setLoadingStmts(false);
    }
  };

  useEffect(() => {
    if (!activeCompanyId) return;
    void loadStatements(activeCompanyId);
  }, [activeCompanyId]);

  const handleFileSelect = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file || !activeCompanyId) return;

    if (isDemoMode()) {
      notify.info(t("bankImport.demoNotice"));
      return;
    }

    setImporting(true);
    try {
      const buffer = await file.arrayBuffer();
      const fileBytes = Array.from(new Uint8Array(buffer));
      const result = await api.bankImport.importStatement(
        activeCompanyId,
        format,
        fileBytes,
      );

      if (result.duplicate) {
        notify.warn(t("bankImport.duplicate"));
      } else {
        notify.success(
          t("bankImport.importedOk", { count: result.importedTxns }),
        );
      }

      setActiveStmt(result.statement);
      setActiveIntegrity(result.integrityOk);
      setActiveWarnings(result.warnings);
      await loadStatements(activeCompanyId);
    } catch (err) {
      notify.error(formatError(err, t("bankImport.importError")));
    } finally {
      setImporting(false);
      // Reset file input so the same file can be re-selected
      if (fileInputRef.current) fileInputRef.current.value = "";
    }
  };

  if (!activeCompanyId) {
    return (
      <div className="main-inner">
        <div className="page-head">
          <div><h1>{t("bankImport.title")}</h1></div>
        </div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          {t("bankImport.noCompany")}
        </div>
      </div>
    );
  }

  return (
    <div className="main-inner">
      {/* Page header */}
      <div className="page-head">
        <div>
          <h1>{t("bankImport.title")}</h1>
          <p className="sub">{t("bankImport.sub")}</p>
        </div>
        <div className="head-actions">
          {/* Format selector */}
          <select
            className="pill-btn"
            value={format}
            onChange={(e) => setFormat(e.target.value as typeof format)}
            style={{ padding: "4px 10px", cursor: "pointer" }}
          >
            <option value="MT940">MT940 (SWIFT)</option>
            <option value="CAMT053">CAMT.053 (ISO 20022)</option>
            <option value="CSV">CSV</option>
          </select>

          {/* File picker */}
          <label
            className={`pill-btn primary${importing ? " disabled" : ""}`}
            style={{ cursor: importing ? "default" : "pointer" }}
          >
            <input
              ref={fileInputRef}
              type="file"
              accept=".txt,.sta,.mt940,.xml,.csv"
              style={{ display: "none" }}
              disabled={importing}
              onChange={(e) => void handleFileSelect(e)}
            />
            <Ic name="dl" />
            {importing ? t("bankImport.importing") : t("bankImport.pickFile")}
          </label>
        </div>
      </div>

      {/* Active statement view */}
      {activeStmt && activeCompanyId && (
        <StatementView
          stmt={activeStmt}
          companyId={activeCompanyId}
          integrityOk={activeIntegrity}
          warnings={activeWarnings}
        />
      )}

      {/* Previous statements list */}
      {!activeStmt && (
        <div className="scr-card">
          {loadingStmts ? (
            <div style={{ padding: 24, fontSize: 13, color: "var(--text-2)" }}>
              {t("gl.common.loading")}
            </div>
          ) : statements.length === 0 ? (
            <div style={{ padding: "44px 16px", textAlign: "center", fontSize: 13, color: "var(--text-2)" }}>
              {t("bankImport.empty")}
            </div>
          ) : (
            <table className="scr-table">
              <thead>
                <tr>
                  <th>{t("bankImport.colDate")}</th>
                  <th>{t("bankImport.ref")}</th>
                  <th>{t("bankImport.format")}</th>
                  <th className="r">{t("bankImport.opening")}</th>
                  <th className="r">{t("bankImport.closing")}</th>
                  <th></th>
                </tr>
              </thead>
              <tbody>
                {statements.map((s) => (
                  <tr key={s.id} className="clickable" onClick={() => {
                    setActiveStmt(s);
                    setActiveIntegrity(null);
                    setActiveWarnings([]);
                  }}>
                    <td className="num">{s.statementDate || "—"}</td>
                    <td>{s.statementRef || "—"}</td>
                    <td><span className="badge">{s.sourceFormat}</span></td>
                    <td className="num r">{fmtRON(parseDec(s.openingBalance))}</td>
                    <td className="num r">{fmtRON(parseDec(s.closingBalance))}</td>
                    <td>
                      <button
                        className="pill-btn xs"
                        onClick={(e) => {
                          e.stopPropagation();
                          setActiveStmt(s);
                          setActiveIntegrity(null);
                          setActiveWarnings([]);
                        }}
                      >
                        {t("bankImport.view")}
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      )}

      {/* Back to list when viewing a statement */}
      {activeStmt && (
        <div style={{ marginTop: 12 }}>
          <button
            className="pill-btn"
            onClick={() => { setActiveStmt(null); setActiveWarnings([]); }}
          >
            ← {t("bankImport.backToList")}
          </button>
        </div>
      )}
    </div>
  );
}
