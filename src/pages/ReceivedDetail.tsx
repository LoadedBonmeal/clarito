/**
 * Detaliu factură primită — vizualizare completă pentru o ReceivedInvoice.
 *
 * Ruta: /received/$id
 */

import { useState } from "react";
import { useParams, useNavigate } from "@tanstack/react-router";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { openPath } from "@tauri-apps/plugin-opener";

import { Icon } from "@/components/shared/Icon";
import { StatusBadge } from "@/components/shared/StatusBadge";
import { notify } from "@/lib/toasts";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { fmtRON } from "@/lib/utils";
import type { ReceivedStatus } from "@/types";

function fmtTime(unix: number): string {
  return new Date(unix * 1000).toLocaleString("ro-RO");
}

export function ReceivedDetailPage() {
  const { id } = useParams({ from: "/received/$id" });
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [successMsg, setSuccessMsg] = useState<string | null>(null);

  const { data: inv, isLoading, isError } = useQuery({
    queryKey: queryKeys.received.detail(id),
    queryFn: () => api.received.get(id),
  });

  const { mutate: updateStatus, isPending } = useMutation({
    mutationFn: (status: ReceivedStatus) =>
      api.received.updateStatus(id, status),
    onSuccess: (_data, status) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.received.all });
      void queryClient.invalidateQueries({ queryKey: queryKeys.received.detail(id) });
      const labels: Record<ReceivedStatus, string> = {
        NEW: "nouă",
        REVIEWED: "revizuită",
        APPROVED: "aprobată",
        REJECTED: "respinsă",
        ARCHIVED: "arhivată",
      };
      setSuccessMsg(`Factura a fost marcată ca ${labels[status]}.`);
      setTimeout(() => setSuccessMsg(null), 3000);
    },
  });

  return (
    <div className="content">
      <div className="content-titlebar">
        <span className="content-title">
          <span className="crumb">Facturi primite</span>
          {inv
            ? inv.series && inv.number
              ? `${inv.series}-${inv.number}`
              : inv.anafDownloadId
            : "Detaliu factură"}
        </span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
          <button
            type="button"
            className="btn"
            onClick={() => navigate({ to: "/received" })}
          >
            <Icon name="arrowLeft" size={12} /> Înapoi
          </button>
        </span>
      </div>

      <div className="content-body" style={{ padding: 20 }}>
        {isLoading && (
          <div style={{ fontSize: 12, color: "var(--text-muted)" }}>
            Se încarcă…
          </div>
        )}

        {isError && (
          <div style={{ fontSize: 12, color: "#DC2626" }}>
            Eroare la încărcarea facturii.
          </div>
        )}

        {successMsg && (
          <div
            style={{
              marginBottom: 16,
              padding: "8px 14px",
              background: "var(--accent-soft)",
              border: "1px solid var(--accent)",
              borderRadius: 4,
              fontSize: 12,
              color: "var(--accent)",
              display: "flex",
              alignItems: "center",
              gap: 8,
            }}
          >
            <Icon name="check" size={13} />
            {successMsg}
          </div>
        )}

        {inv && (
          <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
            {/* Header — emitent + docnum + total */}
            <div className="panel">
              <div className="panel-header">Informații factură</div>
              <div style={{ padding: "12px 14px", display: "grid", gridTemplateColumns: "1fr 1fr", gap: "10px 24px" }}>
                <div>
                  <div style={{ fontSize: 10, textTransform: "uppercase", color: "var(--text-muted)", marginBottom: 2 }}>
                    Emitent
                  </div>
                  <div style={{ fontWeight: 600 }}>{inv.issuerName}</div>
                  <div className="mono muted" style={{ fontSize: 11 }}>{inv.issuerCui}</div>
                </div>
                <div>
                  <div style={{ fontSize: 10, textTransform: "uppercase", color: "var(--text-muted)", marginBottom: 2 }}>
                    Nr. document
                  </div>
                  <div className="mono" style={{ fontWeight: 600 }}>
                    {inv.series && inv.number
                      ? `${inv.series}-${inv.number}`
                      : inv.anafDownloadId}
                  </div>
                </div>
                <div>
                  <div style={{ fontSize: 10, textTransform: "uppercase", color: "var(--text-muted)", marginBottom: 2 }}>
                    Dată emitere
                  </div>
                  <div>{inv.issueDate}</div>
                </div>
                <div>
                  <div style={{ fontSize: 10, textTransform: "uppercase", color: "var(--text-muted)", marginBottom: 2 }}>
                    Total
                  </div>
                  <div className="tnum" style={{ fontWeight: 700, fontSize: 15 }}>
                    {fmtRON(inv.totalAmount)} {inv.currency}
                  </div>
                </div>
              </div>
            </div>

            {/* Status + Acțiuni */}
            <div className="panel">
              <div className="panel-header">Status & Acțiuni</div>
              <div style={{ padding: "12px 14px", display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
                <StatusBadge status={inv.status} />
                <span style={{ flex: 1 }} />
                {(inv.status === "NEW" || inv.status === "REVIEWED") && (
                  <>
                    <button
                      type="button"
                      className="btn primary"
                      disabled={isPending}
                      title="Marchează factura ca aprobată în evidența locală. Nu trimite niciun răspuns la ANAF/SPV."
                      onClick={() => updateStatus("APPROVED")}
                    >
                      <Icon name="check" size={12} /> Aprobă local
                    </button>
                    <button
                      type="button"
                      className="btn"
                      disabled={isPending}
                      style={{ borderColor: "#FCA5A5", color: "#B91C1C" }}
                      title="Marchează factura ca respinsă în evidența locală. Nu trimite niciun răspuns la ANAF/SPV."
                      onClick={() => updateStatus("REJECTED")}
                    >
                      <Icon name="x" size={12} /> Respinge local
                    </button>
                    <div style={{ flexBasis: "100%", fontSize: 10.5, color: "var(--text-muted)", marginTop: 2 }}>
                      Status intern — nu trimite răspuns la ANAF/SPV.
                    </div>
                  </>
                )}
                {inv.status === "APPROVED" && (
                  <button
                    type="button"
                    className="btn"
                    disabled={isPending}
                    onClick={() => updateStatus("ARCHIVED")}
                  >
                    <Icon name="bookmark" size={12} /> Arhivează
                  </button>
                )}
                {inv.status === "REJECTED" && (
                  <button
                    type="button"
                    className="btn"
                    disabled={isPending}
                    onClick={() => updateStatus("REVIEWED")}
                  >
                    <Icon name="refresh" size={12} /> Reanalizează
                  </button>
                )}
              </div>
            </div>

            {/* ANAF info */}
            <div className="panel">
              <div className="panel-header">Informații ANAF/SPV</div>
              <div style={{ padding: "12px 14px", display: "grid", gridTemplateColumns: "1fr 1fr", gap: "10px 24px" }}>
                <div>
                  <div style={{ fontSize: 10, textTransform: "uppercase", color: "var(--text-muted)", marginBottom: 2 }}>
                    Index ANAF
                  </div>
                  <div className="mono">{inv.anafIndex || "—"}</div>
                </div>
                <div>
                  <div style={{ fontSize: 10, textTransform: "uppercase", color: "var(--text-muted)", marginBottom: 2 }}>
                    ID descărcare ANAF
                  </div>
                  <div className="mono">{inv.anafDownloadId}</div>
                </div>
                <div>
                  <div style={{ fontSize: 10, textTransform: "uppercase", color: "var(--text-muted)", marginBottom: 2 }}>
                    Descărcat la
                  </div>
                  <div className="muted" style={{ fontSize: 11 }}>{fmtTime(inv.downloadedAt)}</div>
                </div>
                <div>
                  <div style={{ fontSize: 10, textTransform: "uppercase", color: "var(--text-muted)", marginBottom: 2 }}>
                    Creat la
                  </div>
                  <div className="muted" style={{ fontSize: 11 }}>{fmtTime(inv.createdAt)}</div>
                </div>
              </div>
            </div>

            {/* Fișiere */}
            <div className="panel">
              <div className="panel-header">Fișiere</div>
              <div style={{ padding: "12px 14px", display: "flex", flexDirection: "column", gap: 8 }}>
                <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                  <Icon name="file" size={14} />
                  <span style={{ fontSize: 12, fontWeight: 600 }}>XML</span>
                  <span
                    className="mono muted"
                    style={{ fontSize: 10, flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
                  >
                    {inv.xmlPath}
                  </span>
                  <button
                    type="button"
                    className="btn"
                    disabled={!inv?.xmlPath}
                    onClick={async () => {
                      if (!inv?.xmlPath) { notify.error("XML indisponibil"); return; }
                      try { await openPath(inv.xmlPath); } catch (e) { notify.error(`Eroare: ${e}`); }
                    }}
                  >
                    <Icon name="download" size={12} /> Deschide XML
                  </button>
                </div>
                {inv.pdfPath && (
                  <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                    <Icon name="file" size={14} />
                    <span style={{ fontSize: 12, fontWeight: 600 }}>PDF</span>
                    <span
                      className="mono muted"
                      style={{ fontSize: 10, flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
                    >
                      {inv.pdfPath}
                    </span>
                    <button
                      type="button"
                      className="btn"
                      disabled={!inv?.pdfPath}
                      onClick={async () => {
                        if (!inv?.pdfPath) { notify.error("PDF indisponibil"); return; }
                        try { await openPath(inv.pdfPath); } catch (e) { notify.error(`Eroare: ${e}`); }
                      }}
                    >
                      <Icon name="download" size={12} /> Deschide PDF
                    </button>
                  </div>
                )}
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
