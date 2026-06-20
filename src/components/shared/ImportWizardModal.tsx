/**
 * ImportWizardModal — multi-step wizard pentru migrarea datelor din alte programe.
 *
 * Pași:
 *   1. Sursă        — alege sursa (SmartBill XML/REST, SAGA XML/DBF, WinMentor TXT)
 *   2. Fișiere/API  — pick fișier(e) sau verificare credențiale SmartBill REST
 *   3. Mapare coloane (doar SAGA DBF) — confirmare câmpuri detectate în DBF
 *   4. Previzualizare — stage + preview: rollup per entitate + eșantion rânduri
 *   5. Confirmare   — commit + CommitReport
 *
 * Paralel cu CsvImportModal (același shell: modal-back/modal/modal-head/modal-body),
 * dar cu navigare multi-step proprie.
 */

import { useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";

import { Ic } from "@/components/shared/Ic";
import { useAnimatedClose } from "@/hooks/use-animated-close";
import { api } from "@/lib/tauri";
import type {
  DetectedColumn,
  BatchCounts,
  PreviewResult,
  CommitReport,
} from "@/lib/tauri";
import { formatError } from "@/lib/error-mapper";

// ─── Types ────────────────────────────────────────────────────────────────────

type SourceId =
  | "SMARTBILL_XML"
  | "SMARTBILL_REST"
  | "SAGA_XML"
  | "SAGA_DBF"
  | "WINMENTOR_TXT";

interface SourceDef {
  id: SourceId;
  labelKey: string;
  descKey: string;
  extensions?: string[];
  isRest?: boolean;
}

const SOURCES: SourceDef[] = [
  {
    id: "SMARTBILL_XML",
    labelKey: "shared.import.sources.smartbillXml.label",
    descKey: "shared.import.sources.smartbillXml.desc",
    extensions: ["xml"],
  },
  {
    id: "SMARTBILL_REST",
    labelKey: "shared.import.sources.smartbillRest.label",
    descKey: "shared.import.sources.smartbillRest.desc",
    isRest: true,
  },
  {
    id: "SAGA_XML",
    labelKey: "shared.import.sources.sagaXml.label",
    descKey: "shared.import.sources.sagaXml.desc",
    extensions: ["xml"],
  },
  {
    id: "SAGA_DBF",
    labelKey: "shared.import.sources.sagaDbf.label",
    descKey: "shared.import.sources.sagaDbf.desc",
    extensions: ["dbf"],
  },
  {
    id: "WINMENTOR_TXT",
    labelKey: "shared.import.sources.winmentorTxt.label",
    descKey: "shared.import.sources.winmentorTxt.desc",
    extensions: ["txt"],
  },
];

// Internal field options for column mapping (SAGA DBF).
const COLUMN_TARGET_FIELDS = [
  "cui",
  "name",
  "address",
  "city",
  "county",
  "country",
  "email",
  "phone",
  "code",
  "barcode",
  "unit",
  "vatRate",
  "price",
  "(skip)",
];

type Step = "source" | "input" | "columns" | "preview" | "commit";

interface ImportWizardModalProps {
  companyId: string;
  onClose: () => void;
  onSuccess: () => void;
}

// ─── Helper: step index for the progress bar ──────────────────────────────────

const STEP_ORDER: Step[] = ["source", "input", "columns", "preview", "commit"];

function stepIndex(step: Step) {
  return STEP_ORDER.indexOf(step);
}

// ─── Helper: resolution badge color ──────────────────────────────────────────

function resColor(key: string): string {
  switch (key) {
    case "new":
      return "#1d4ed8";
    case "matched":
      return "#059669";
    case "review":
      return "#d97706";
    case "error":
    case "dupInBatch":
      return "#dc2626";
    default:
      return "var(--text-muted)";
  }
}

// ─── Helper: render BatchCounts rollup table ──────────────────────────────────

function CountsTable({
  counts,
  t,
}: {
  counts: BatchCounts;
  t: (k: string) => string;
}) {
  const entities = [
    { key: "contacts", label: t("shared.import.preview.contacts") },
    { key: "products", label: t("shared.import.preview.products") },
    { key: "accounts", label: t("shared.import.preview.accounts") },
    { key: "invoices", label: t("shared.import.preview.invoices") },
  ] as const;

  const cols = [
    { key: "new", label: t("shared.import.preview.new") },
    { key: "matched", label: t("shared.import.preview.matched") },
    { key: "review", label: t("shared.import.preview.review") },
    { key: "error", label: t("shared.import.preview.error") },
  ] as const;

  return (
    <table
      style={{
        width: "100%",
        borderCollapse: "collapse",
        fontSize: 11,
        marginBottom: 12,
      }}
    >
      <thead>
        <tr style={{ background: "var(--bg)", color: "var(--text-muted)" }}>
          <th
            style={{
              textAlign: "left",
              padding: "4px 8px",
              border: "1px solid var(--border)",
              fontWeight: 600,
            }}
          >
            {t("shared.import.preview.entity")}
          </th>
          {cols.map((c) => (
            <th
              key={c.key}
              style={{
                textAlign: "right",
                padding: "4px 8px",
                border: "1px solid var(--border)",
                fontWeight: 600,
                color: resColor(c.key),
              }}
            >
              {c.label}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {entities.map((ent) => {
          const row = counts[ent.key];
          return (
            <tr key={ent.key}>
              <td
                style={{
                  padding: "4px 8px",
                  border: "1px solid var(--border)",
                  fontWeight: 600,
                }}
              >
                {ent.label}
              </td>
              {cols.map((c) => (
                <td
                  key={c.key}
                  style={{
                    padding: "4px 8px",
                    border: "1px solid var(--border)",
                    textAlign: "right",
                    color: row[c.key] > 0 ? resColor(c.key) : "var(--text-muted)",
                  }}
                >
                  {row[c.key]}
                </td>
              ))}
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

// ─── Helper: commit report table ──────────────────────────────────────────────

function CommitReportTable({
  report,
  t,
}: {
  report: CommitReport;
  t: (k: string) => string;
}) {
  const entities = [
    { key: "contacts", label: t("shared.import.preview.contacts") },
    { key: "products", label: t("shared.import.preview.products") },
    { key: "accounts", label: t("shared.import.preview.accounts") },
    { key: "invoices", label: t("shared.import.preview.invoices") },
  ] as const;

  return (
    <table
      style={{
        width: "100%",
        borderCollapse: "collapse",
        fontSize: 11,
        marginBottom: 12,
      }}
    >
      <thead>
        <tr style={{ background: "var(--bg)", color: "var(--text-muted)" }}>
          <th
            style={{
              textAlign: "left",
              padding: "4px 8px",
              border: "1px solid var(--border)",
              fontWeight: 600,
            }}
          >
            {t("shared.import.preview.entity")}
          </th>
          {(
            [
              ["created", t("shared.import.commit.created")],
              ["matched", t("shared.import.commit.matched")],
              ["skipped", t("shared.import.commit.skipped")],
              ["errors", t("shared.import.commit.errors")],
            ] as [string, string][]
          ).map(([k, label]) => (
            <th
              key={k}
              style={{
                textAlign: "right",
                padding: "4px 8px",
                border: "1px solid var(--border)",
                fontWeight: 600,
                color:
                  k === "created"
                    ? "#1d4ed8"
                    : k === "matched"
                    ? "#059669"
                    : k === "errors"
                    ? "#dc2626"
                    : "var(--text-muted)",
              }}
            >
              {label}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {entities.map((ent) => {
          const row = report[ent.key];
          return (
            <tr key={ent.key}>
              <td
                style={{
                  padding: "4px 8px",
                  border: "1px solid var(--border)",
                  fontWeight: 600,
                }}
              >
                {ent.label}
              </td>
              {(["created", "matched", "skipped", "errors"] as const).map((k) => (
                <td
                  key={k}
                  style={{
                    padding: "4px 8px",
                    border: "1px solid var(--border)",
                    textAlign: "right",
                    color:
                      row[k] > 0
                        ? k === "created"
                          ? "#1d4ed8"
                          : k === "matched"
                          ? "#059669"
                          : k === "errors"
                          ? "#dc2626"
                          : "var(--text-muted)"
                        : "var(--text-muted)",
                  }}
                >
                  {row[k]}
                </td>
              ))}
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

// ─── Modal ────────────────────────────────────────────────────────────────────

export function ImportWizardModal({
  companyId,
  onClose,
  onSuccess,
}: ImportWizardModalProps) {
  const { t } = useTranslation();
  const { closing, close } = useAnimatedClose(onClose, 140, true);

  // ── Wizard state ─────────────────────────────────────────────────────────
  const [step, setStep] = useState<Step>("source");
  const [source, setSource] = useState<SourceId | null>(null);
  const [filePaths, setFilePaths] = useState<string[]>([]);
  const [sbCreds, setSbCreds] = useState<{ user: string; token: string } | null>(
    null
  );
  const [detectedCols, setDetectedCols] = useState<DetectedColumn[]>([]);
  const [columnMap, setColumnMap] = useState<Record<string, string>>({});
  const [batchId, setBatchId] = useState<string | null>(null);
  const [preview, setPreview] = useState<PreviewResult | null>(null);
  const [commitReport, setCommitReport] = useState<CommitReport | null>(null);

  // ── Loading / error state ────────────────────────────────────────────────
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const sourceDef = SOURCES.find((s) => s.id === source);
  const isDbf = source === "SAGA_DBF";

  // ── Step helpers ──────────────────────────────────────────────────────────

  function clearError() {
    setError(null);
  }

  function goTo(s: Step) {
    clearError();
    setStep(s);
  }

  function goBack() {
    clearError();
    const cur = stepIndex(step);
    if (cur <= 0) return;
    // Skip "columns" step when going back if not DBF or no cols.
    if (step === "preview" && (!isDbf || detectedCols.length === 0)) {
      goTo("input");
      return;
    }
    goTo(STEP_ORDER[cur - 1]);
  }

  // ── Step 2: pick files ────────────────────────────────────────────────────

  async function handlePickFiles() {
    if (!sourceDef || !sourceDef.extensions) return;
    try {
      const selected = await open({
        multiple: true,
        filters: [
          {
            name: sourceDef.extensions.map((e) => e.toUpperCase()).join("/"),
            extensions: sourceDef.extensions,
          },
        ],
      });
      if (selected) {
        const paths = Array.isArray(selected) ? selected : [selected];
        setFilePaths(paths);
      }
    } catch (e) {
      setError(formatError(e, t("shared.import.errors.filePicker")));
    }
  }

  async function handleLoadSmartBillCreds() {
    setLoading(true);
    clearError();
    try {
      const creds = await api.integrations.getSmartbillCredentials(companyId);
      if (!creds.configured) {
        setError(t("shared.import.errors.smartbillNotConfigured"));
        return;
      }
      setSbCreds({ user: creds.user, token: "***" });
    } catch (e) {
      setError(formatError(e, t("shared.import.errors.smartbillCreds")));
    } finally {
      setLoading(false);
    }
  }

  function canProceedFromInput(): boolean {
    if (sourceDef?.isRest) return !!sbCreds;
    return filePaths.length > 0;
  }

  // ── Step 2→3/4: detect columns (SAGA DBF) or go straight to preview ───────

  async function handleNextFromInput() {
    clearError();
    if (isDbf && filePaths.length > 0) {
      setLoading(true);
      try {
        const cols = await api.importWaveC.detectColumns(
          companyId,
          source!,
          filePaths
        );
        setDetectedCols(cols);
        // Pre-fill columnMap from adapter's default synonym resolution (best-effort).
        const initial: Record<string, string> = {};
        cols.forEach((c) => {
          initial[c.name] = "(skip)";
        });
        setColumnMap(initial);
        if (cols.length > 0) {
          goTo("columns");
        } else {
          await doStageAndPreview();
        }
      } catch (e) {
        setError(formatError(e, t("shared.import.errors.detectColumns")));
      } finally {
        setLoading(false);
      }
    } else {
      await doStageAndPreview();
    }
  }

  // ── Stage + Preview ───────────────────────────────────────────────────────

  async function doStageAndPreview() {
    setLoading(true);
    clearError();
    try {
      const paths = sourceDef?.isRest ? [] : filePaths;
      const cm = isDbf && Object.keys(columnMap).length > 0 ? columnMap : undefined;
      const stageRes = await api.importWaveC.stage(
        companyId,
        source!,
        paths,
        cm
      );
      setBatchId(stageRes.batchId);

      const pvRes = await api.importWaveC.preview(stageRes.batchId);
      setPreview(pvRes);
      goTo("preview");
    } catch (e) {
      setError(formatError(e, t("shared.import.errors.stage")));
    } finally {
      setLoading(false);
    }
  }

  // ── Commit ────────────────────────────────────────────────────────────────

  async function handleCommit() {
    if (!batchId) return;
    setLoading(true);
    clearError();
    try {
      const report = await api.importWaveC.commit(batchId);
      setCommitReport(report);
      goTo("commit");
      onSuccess();
    } catch (e) {
      setError(formatError(e, t("shared.import.errors.commit")));
    } finally {
      setLoading(false);
    }
  }

  // ── Progress bar ──────────────────────────────────────────────────────────

  // Only show the columns step in progress when DBF detected real columns.
  const visibleSteps: { id: Step; label: string }[] = [
    { id: "source", label: t("shared.import.steps.source") },
    { id: "input", label: t("shared.import.steps.input") },
    ...(isDbf && detectedCols.length > 0
      ? [{ id: "columns" as Step, label: t("shared.import.steps.columns") }]
      : []),
    { id: "preview", label: t("shared.import.steps.preview") },
    { id: "commit", label: t("shared.import.steps.confirm") },
  ];
  const curVisIdx = visibleSteps.findIndex((s) => s.id === step);

  // ── Render ────────────────────────────────────────────────────────────────

  return createPortal(
    <div
      className={`modal-back ${closing ? "closing" : "show"}`}
      style={{ position: "fixed" }}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) close();
      }}
    >
      <div
        className="modal"
        style={{
          width: 580,
          maxHeight: "88vh",
          display: "flex",
          flexDirection: "column",
        }}
      >
        {/* ── Header ── */}
        <div className="modal-head">
          <div>
            <div className="mt">{t("shared.import.title")}</div>
            <div className="ms">{t("shared.import.subtitle")}</div>
          </div>
          <button
            type="button"
            className="modal-x"
            aria-label={t("shared.common.close")}
            onClick={close}
          >
            <Ic name="xMark" />
          </button>
        </div>

        {/* ── Progress bar ── */}
        <div
          style={{
            display: "flex",
            gap: 0,
            borderBottom: "1px solid var(--border)",
            padding: "0 20px",
            overflowX: "auto",
          }}
        >
          {visibleSteps.map((s, i) => (
            <div
              key={s.id}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 4,
                paddingBottom: 8,
                paddingTop: 8,
                fontSize: 11,
                fontWeight: i === curVisIdx ? 700 : 500,
                color:
                  i < curVisIdx
                    ? "#059669"
                    : i === curVisIdx
                    ? "var(--text)"
                    : "var(--text-muted)",
                flex: "0 0 auto",
              }}
            >
              {i > 0 && (
                <Ic
                  name="chevR"
                  cls="ic"
                  // tiny inline-size for the separator
                />
              )}
              <span
                style={{
                  width: 18,
                  height: 18,
                  borderRadius: "50%",
                  background:
                    i < curVisIdx
                      ? "#059669"
                      : i === curVisIdx
                      ? "var(--text)"
                      : "var(--border)",
                  color: i <= curVisIdx ? "#fff" : "var(--text-muted)",
                  display: "inline-flex",
                  alignItems: "center",
                  justifyContent: "center",
                  fontSize: 10,
                  fontWeight: 700,
                  flexShrink: 0,
                }}
              >
                {i < curVisIdx ? "✓" : i + 1}
              </span>
              <span style={{ marginLeft: 2, whiteSpace: "nowrap" }}>
                {s.label}
              </span>
            </div>
          ))}
        </div>

        {/* ── Body ── */}
        <div className="modal-body" style={{ overflowY: "auto", flex: 1 }}>
          {/* Error banner */}
          {error && (
            <div
              style={{
                padding: "7px 10px",
                background: "#FEE2E2",
                border: "1px solid #FECACA",
                fontSize: 11,
                color: "#991B1B",
                marginBottom: 12,
                borderRadius: 4,
              }}
            >
              {error}
            </div>
          )}

          {/* ── Step 1: Source ── */}
          {step === "source" && (
            <div>
              <div
                style={{
                  fontSize: 11.5,
                  fontWeight: 600,
                  marginBottom: 10,
                  color: "var(--text-muted)",
                }}
              >
                {t("shared.import.sourceStep.prompt")}
              </div>
              <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                {SOURCES.map((s) => (
                  <button
                    key={s.id}
                    type="button"
                    onClick={() => {
                      setSource(s.id);
                      setFilePaths([]);
                      setSbCreds(null);
                      setDetectedCols([]);
                      setColumnMap({});
                      setBatchId(null);
                      setPreview(null);
                      setCommitReport(null);
                    }}
                    style={{
                      textAlign: "left",
                      padding: "10px 14px",
                      border: `1.5px solid ${
                        source === s.id ? "var(--text)" : "var(--border)"
                      }`,
                      borderRadius: 6,
                      background:
                        source === s.id ? "var(--bg)" : "transparent",
                      cursor: "pointer",
                      transition: "border-color .12s",
                    }}
                  >
                    <div
                      style={{
                        fontWeight: 600,
                        fontSize: 12,
                        marginBottom: 2,
                      }}
                    >
                      {t(s.labelKey)}
                    </div>
                    <div
                      style={{ fontSize: 10.5, color: "var(--text-muted)" }}
                    >
                      {t(s.descKey)}
                    </div>
                  </button>
                ))}
              </div>
            </div>
          )}

          {/* ── Step 2: Input ── */}
          {step === "input" && sourceDef && (
            <div>
              <div
                style={{
                  fontSize: 11.5,
                  fontWeight: 600,
                  marginBottom: 10,
                  color: "var(--text-muted)",
                }}
              >
                {t(sourceDef.labelKey)}
              </div>

              {/* File sources */}
              {!sourceDef.isRest && (
                <div>
                  <button
                    type="button"
                    className="pill-btn"
                    onClick={() => void handlePickFiles()}
                  >
                    <Ic name="docUp" />
                    {t("shared.import.inputStep.pickFiles")}
                  </button>

                  {filePaths.length > 0 && (
                    <div
                      style={{
                        marginTop: 10,
                        padding: "7px 10px",
                        background: "var(--bg)",
                        border: "1px solid var(--border)",
                        fontSize: 11,
                        borderRadius: 4,
                      }}
                    >
                      {filePaths.map((p) => (
                        <div
                          key={p}
                          style={{
                            fontFamily: "var(--font-mono)",
                            fontSize: 10.5,
                            color: "var(--text-muted)",
                            marginBottom: 2,
                          }}
                        >
                          {p}
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              )}

              {/* SmartBill REST */}
              {sourceDef.isRest && (
                <div>
                  {sbCreds ? (
                    <div
                      style={{
                        padding: "8px 12px",
                        background: "#F0FDF4",
                        border: "1px solid #BBF7D0",
                        fontSize: 11,
                        color: "#15803D",
                        borderRadius: 4,
                        marginBottom: 10,
                      }}
                    >
                      <Ic name="checkC" cls="ic" />
                      &nbsp;
                      {t("shared.import.inputStep.sbConnected", { user: sbCreds.user })}
                    </div>
                  ) : (
                    <div>
                      <div
                        style={{
                          fontSize: 11,
                          color: "var(--text-muted)",
                          marginBottom: 8,
                        }}
                      >
                        {t("shared.import.inputStep.sbHint")}
                      </div>
                      <button
                        type="button"
                        className="pill-btn"
                        disabled={loading}
                        onClick={() => void handleLoadSmartBillCreds()}
                      >
                        {loading
                          ? t("shared.import.loading")
                          : t("shared.import.inputStep.sbCheck")}
                      </button>
                    </div>
                  )}
                </div>
              )}
            </div>
          )}

          {/* ── Step 3: Column mapping (SAGA DBF only) ── */}
          {step === "columns" && (
            <div>
              <div
                style={{
                  fontSize: 11,
                  color: "var(--text-muted)",
                  marginBottom: 10,
                }}
              >
                {t("shared.import.columnsStep.hint")}
              </div>
              <table
                style={{
                  width: "100%",
                  borderCollapse: "collapse",
                  fontSize: 11,
                }}
              >
                <thead>
                  <tr
                    style={{
                      background: "var(--bg)",
                      color: "var(--text-muted)",
                    }}
                  >
                    <th
                      style={{
                        textAlign: "left",
                        padding: "4px 8px",
                        border: "1px solid var(--border)",
                        fontWeight: 600,
                      }}
                    >
                      {t("shared.import.columnsStep.dbfCol")}
                    </th>
                    <th
                      style={{
                        textAlign: "left",
                        padding: "4px 8px",
                        border: "1px solid var(--border)",
                        fontWeight: 600,
                      }}
                    >
                      {t("shared.import.columnsStep.sample")}
                    </th>
                    <th
                      style={{
                        textAlign: "left",
                        padding: "4px 8px",
                        border: "1px solid var(--border)",
                        fontWeight: 600,
                      }}
                    >
                      {t("shared.import.columnsStep.mapsTo")}
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {detectedCols.map((col) => (
                    <tr key={col.name}>
                      <td
                        style={{
                          padding: "4px 8px",
                          border: "1px solid var(--border)",
                          fontFamily: "var(--font-mono)",
                          fontSize: 10.5,
                        }}
                      >
                        {col.name}
                      </td>
                      <td
                        style={{
                          padding: "4px 8px",
                          border: "1px solid var(--border)",
                          color: "var(--text-muted)",
                          maxWidth: 120,
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                          whiteSpace: "nowrap",
                        }}
                      >
                        {col.sample}
                      </td>
                      <td
                        style={{
                          padding: "4px 8px",
                          border: "1px solid var(--border)",
                        }}
                      >
                        <select
                          value={columnMap[col.name] ?? "(skip)"}
                          onChange={(e) =>
                            setColumnMap((prev) => ({
                              ...prev,
                              [col.name]: e.target.value,
                            }))
                          }
                          style={{
                            fontSize: 11,
                            border: "1px solid var(--border)",
                            background: "var(--bg)",
                            padding: "2px 4px",
                            borderRadius: 3,
                            width: "100%",
                          }}
                        >
                          {COLUMN_TARGET_FIELDS.map((f) => (
                            <option key={f} value={f}>
                              {f}
                            </option>
                          ))}
                        </select>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}

          {/* ── Step 4: Preview ── */}
          {step === "preview" && preview && (
            <div>
              {/* Warnings */}
              {preview.counts && (
                <div>
                  <div
                    style={{
                      fontSize: 11.5,
                      fontWeight: 600,
                      marginBottom: 6,
                    }}
                  >
                    {t("shared.import.preview.rollupTitle")}
                  </div>
                  <CountsTable counts={preview.counts} t={t} />
                </div>
              )}

              {/* Skip-note */}
              <div
                style={{
                  padding: "7px 10px",
                  background: "#FFFBEB",
                  border: "1px solid #FDE68A",
                  fontSize: 10.5,
                  color: "#92400E",
                  marginBottom: 10,
                  borderRadius: 4,
                }}
              >
                {t("shared.import.preview.skipNote")}
              </div>

              {/* Sample rows */}
              {preview.sampleContacts.length > 0 && (
                <SampleRows
                  label={t("shared.import.preview.contacts")}
                  rows={preview.sampleContacts}
                />
              )}
              {preview.sampleProducts.length > 0 && (
                <SampleRows
                  label={t("shared.import.preview.products")}
                  rows={preview.sampleProducts}
                />
              )}
              {preview.sampleAccounts.length > 0 && (
                <SampleRows
                  label={t("shared.import.preview.accounts")}
                  rows={preview.sampleAccounts}
                />
              )}
              {preview.sampleInvoices.length > 0 && (
                <SampleRows
                  label={t("shared.import.preview.invoices")}
                  rows={preview.sampleInvoices}
                />
              )}
            </div>
          )}

          {/* ── Step 5: Commit result ── */}
          {step === "commit" && commitReport && (
            <div>
              <div
                style={{
                  padding: "8px 12px",
                  background: "#F0FDF4",
                  border: "1px solid #BBF7D0",
                  fontSize: 11,
                  color: "#15803D",
                  marginBottom: 12,
                  borderRadius: 4,
                  fontWeight: 600,
                }}
              >
                {t("shared.import.commit.success")}
              </div>

              <CommitReportTable report={commitReport} t={t} />

              {/* Draft invoice note */}
              <div
                style={{
                  padding: "7px 10px",
                  background: "#EFF6FF",
                  border: "1px solid #BFDBFE",
                  fontSize: 10.5,
                  color: "#1d4ed8",
                  marginBottom: 10,
                  borderRadius: 4,
                }}
              >
                {t("shared.import.commit.draftNote")}
              </div>

              {/* Errors list */}
              {commitReport.errors.length > 0 && (
                <div
                  style={{
                    padding: "7px 10px",
                    background: "#FEF2F2",
                    border: "1px solid #FECACA",
                    fontSize: 10.5,
                    color: "#991B1B",
                    borderRadius: 4,
                  }}
                >
                  <div style={{ fontWeight: 600, marginBottom: 4 }}>
                    {t("shared.import.commit.errorList")}
                  </div>
                  {commitReport.errors.slice(0, 10).map((e, i) => (
                    <div key={i}>• {e}</div>
                  ))}
                  {commitReport.errors.length > 10 && (
                    <div style={{ marginTop: 2 }}>
                      {t("shared.import.commit.moreErrors", {
                        count: commitReport.errors.length - 10,
                      })}
                    </div>
                  )}
                </div>
              )}
            </div>
          )}
        </div>

        {/* ── Footer actions ── */}
        <div
          style={{
            display: "flex",
            gap: 8,
            justifyContent: "flex-end",
            padding: "12px 20px",
            borderTop: "1px solid var(--border)",
          }}
        >
          {/* Close / Back */}
          {step === "source" || step === "commit" ? (
            <button type="button" className="pill-btn" onClick={close}>
              {t("shared.common.close")}
            </button>
          ) : (
            <button
              type="button"
              className="pill-btn"
              disabled={loading}
              onClick={goBack}
            >
              {t("shared.import.nav.back")}
            </button>
          )}

          {/* Primary action */}
          {step === "source" && (
            <button
              type="button"
              className="btn-dark"
              disabled={!source}
              onClick={() => goTo("input")}
            >
              {t("shared.import.nav.next")}
            </button>
          )}

          {step === "input" && (
            <button
              type="button"
              className="btn-dark"
              disabled={!canProceedFromInput() || loading}
              onClick={() => void handleNextFromInput()}
            >
              {loading ? t("shared.import.loading") : t("shared.import.nav.next")}
            </button>
          )}

          {step === "columns" && (
            <button
              type="button"
              className="btn-dark"
              disabled={loading}
              onClick={() => void doStageAndPreview()}
            >
              {loading ? t("shared.import.loading") : t("shared.import.nav.next")}
            </button>
          )}

          {step === "preview" && (
            <button
              type="button"
              className="btn-dark"
              disabled={loading || !batchId}
              onClick={() => void handleCommit()}
            >
              {loading
                ? t("shared.import.loading")
                : t("shared.import.nav.commit")}
            </button>
          )}

          {/* Commit step: only close */}
        </div>
      </div>
    </div>,
    document.body
  );
}

// ─── SampleRows sub-component ────────────────────────────────────────────────

function SampleRows({
  label,
  rows,
}: {
  label: string;
  rows: Record<string, string | null>[];
}) {
  if (rows.length === 0) return null;
  const cols = Object.keys(rows[0]);

  return (
    <div style={{ marginBottom: 12 }}>
      <div
        style={{
          fontSize: 11,
          fontWeight: 600,
          marginBottom: 4,
          color: "var(--text-muted)",
        }}
      >
        {label}
      </div>
      <div style={{ overflowX: "auto" }}>
        <table
          style={{
            borderCollapse: "collapse",
            fontSize: 10.5,
            minWidth: "100%",
          }}
        >
          <thead>
            <tr>
              {cols.map((c) => (
                <th
                  key={c}
                  style={{
                    padding: "3px 7px",
                    border: "1px solid var(--border)",
                    background: "var(--bg)",
                    fontWeight: 600,
                    textAlign: "left",
                    whiteSpace: "nowrap",
                  }}
                >
                  {c}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((row, i) => (
              <tr key={i}>
                {cols.map((c) => (
                  <td
                    key={c}
                    style={{
                      padding: "3px 7px",
                      border: "1px solid var(--border)",
                      color:
                        c === "resolution"
                          ? resColor(row[c]?.toLowerCase() ?? "")
                          : undefined,
                      fontWeight: c === "resolution" ? 600 : undefined,
                      whiteSpace: "nowrap",
                      maxWidth: 180,
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                    }}
                  >
                    {row[c] ?? "—"}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
