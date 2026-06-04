/**
 * Detaliu factură primită — re-skinned to rf kit (Wave 4).
 * Ruta: /received/$id
 * Preserves: api.received.get(id, companyId), status buttons → api.received.updateStatus,
 * supplier/sums display, XML/PDF source → openPath, Recalculează TVA → reparseVat.
 */

import { useState } from "react";
import { useParams, useNavigate } from "@tanstack/react-router";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { openPath } from "@tauri-apps/plugin-opener";

import { Icon } from "@/components/shared/Icon";
import { StatusBadge } from "@/components/shared/StatusBadge";
import { QueryErrorBanner } from "@/components/shared/QueryErrorBanner";
import {
  PageHeader, Btn, Badge, Card, SectionCard, Banner, Empty,
} from "@/components/rf";
import { notify } from "@/lib/toasts";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { fmtRON } from "@/lib/utils";
import { formatError } from "@/lib/error-mapper";
import type { ReceivedStatus } from "@/types";

function fmtTime(unix: number): string {
  return new Date(unix * 1000).toLocaleString("ro-RO");
}

const STATUS_LABELS: Record<ReceivedStatus, string> = {
  NEW: "nouă",
  REVIEWED: "revizuită",
  APPROVED: "aprobată",
  REJECTED: "respinsă",
  ARCHIVED: "arhivată",
};

export function ReceivedDetailPage() {
  const { id } = useParams({ from: "/received/$id" });
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [successMsg, setSuccessMsg] = useState<string | null>(null);

  const { data: inv, isLoading, isError, error, refetch } = useQuery({
    queryKey: queryKeys.received.detail(id),
    queryFn: () => {
      if (!activeCompanyId) return Promise.reject(new Error("Nicio companie activă selectată."));
      return api.received.get(id, activeCompanyId);
    },
    enabled: !!activeCompanyId,
  });

  const { mutate: updateStatus, isPending } = useMutation({
    mutationFn: (status: ReceivedStatus) => {
      if (!activeCompanyId) {
        notify.warn("Nicio companie activă selectată.");
        return Promise.reject(new Error("Nicio companie activă selectată."));
      }
      return api.received.updateStatus(id, activeCompanyId, status);
    },
    onSuccess: (_data, status) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.received.all });
      void queryClient.invalidateQueries({ queryKey: queryKeys.received.detail(id) });
      setSuccessMsg(`Factura a fost marcată ca ${STATUS_LABELS[status]}.`);
      setTimeout(() => setSuccessMsg(null), 3000);
    },
    onError: (e) => notify.error(formatError(e, "Nu s-a putut actualiza statusul.")),
  });

  const { mutate: reparseVat, isPending: isReparsing } = useMutation({
    mutationFn: () => api.received.reparseVat(activeCompanyId ?? undefined),
    onSuccess: (count) => {
      queryClient.invalidateQueries({ queryKey: queryKeys.received.all });
      void queryClient.invalidateQueries({ queryKey: queryKeys.received.detail(id) });
      notify.success(`TVA recalculat pentru ${count} facturi.`);
    },
    onError: (e) => notify.error(formatError(e, "Eroare recalculare TVA.")),
  });

  const docTitle = inv
    ? inv.series && inv.number
      ? `${inv.series}-${inv.number}`
      : inv.anafDownloadId
    : "Detaliu factură";

  return (
    <div className="rf-page">
      <PageHeader
        title={docTitle}
        sub={inv ? <StatusBadge status={inv.status} /> : undefined}
        actions={
          <>
            <Btn
              variant="ghost"
              icon="chevLeft"
              size="sm"
              onClick={() => navigate({ to: "/received" })}
            >
              Înapoi
            </Btn>
            <Btn
              variant="secondary"
              icon="refresh"
              size="sm"
              disabled={isReparsing}
              onClick={() => reparseVat()}
            >
              Recalculează TVA
            </Btn>
          </>
        }
      />

      <div className="rf-page-body">
        {isLoading ? (
          <Empty icon="fileIn" title="Se încarcă…" />
        ) : isError ? (
          <QueryErrorBanner error={error} label="factura primită" onRetry={() => void refetch()} />
        ) : inv ? (
          <>
            {successMsg && (
              <div style={{ marginBottom: 12 }}>
                <Banner variant="success">{successMsg}</Banner>
              </div>
            )}

            <div style={{ display: "grid", gridTemplateColumns: "1fr 300px", gap: 20, alignItems: "start" }}>
              {/* Left column */}
              <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
                {/* Invoice info */}
                <Card pad>
                  <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "14px 24px" }}>
                    <div>
                      <div className="rf-sec-title">Emitent</div>
                      <div style={{ fontWeight: 600, marginTop: 4 }}>{inv.issuerName}</div>
                      <div className="mono" style={{ fontSize: 12, color: "var(--rf-text-muted)" }}>{inv.issuerCui}</div>
                    </div>
                    <div>
                      <div className="rf-sec-title">Nr. document</div>
                      <div className="mono" style={{ fontWeight: 600, marginTop: 4 }}>
                        {inv.series && inv.number ? `${inv.series}-${inv.number}` : inv.anafDownloadId}
                      </div>
                    </div>
                    <div>
                      <div className="rf-sec-title">Dată emitere</div>
                      <div style={{ marginTop: 4 }}>{inv.issueDate}</div>
                    </div>
                    <div>
                      <div className="rf-sec-title">Total</div>
                      <div className="mono" style={{ fontWeight: 700, fontSize: 16, marginTop: 4, color: "var(--rf-accent)" }}>
                        {fmtRON(inv.totalAmount)} {inv.currency}
                      </div>
                    </div>
                  </div>
                </Card>

                {/* Defalcare TVA — net/TVA/total + avertisment dacă lipsește din XML */}
                <SectionCard icon="receipt" title="Defalcare TVA">
                  {inv.netAmount != null ? (
                    <div style={{ padding: "0 16px 16px" }}>
                      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr", gap: "12px 24px" }}>
                        <div>
                          <div className="rf-sec-title">Bază impozabilă</div>
                          <div className="mono" style={{ fontWeight: 600, marginTop: 4 }}>
                            {fmtRON(inv.netAmount)} {inv.currency}
                          </div>
                        </div>
                        <div>
                          <div className="rf-sec-title">TVA</div>
                          <div className="mono" style={{ fontWeight: 600, marginTop: 4 }}>
                            {inv.vatAmount != null ? `${fmtRON(inv.vatAmount)} ${inv.currency}` : "—"}
                          </div>
                        </div>
                        <div>
                          <div className="rf-sec-title">Total</div>
                          <div className="mono" style={{ fontWeight: 700, marginTop: 4, color: "var(--rf-accent)" }}>
                            {fmtRON(inv.totalAmount)} {inv.currency}
                          </div>
                        </div>
                      </div>
                      <div style={{ marginTop: 10 }}>
                        {inv.vatAmount != null ? (
                          <Badge variant="success">Defalcare parsată din XML</Badge>
                        ) : (
                          <Badge variant="warning">TVA lipsă din XML — verificați factura</Badge>
                        )}
                      </div>
                    </div>
                  ) : (
                    <div style={{ padding: "0 16px 16px" }}>
                      <Banner variant="warning">
                        <b>Defalcare TVA indisponibilă</b> — această factură nu are baza și TVA extrase din
                        XML, deci <b>nu contribuie la TVA deductibilă</b> în D300/D394. Apăsați
                        «Recalculează TVA» (în antet) pentru a re-parsa din fișierul XML.
                      </Banner>
                    </div>
                  )}
                </SectionCard>

                {/* ANAF/SPV info */}
                <SectionCard icon="cloud" title="Informații ANAF/SPV">
                  <div style={{ padding: "0 16px 16px", display: "grid", gridTemplateColumns: "1fr 1fr", gap: "12px 24px" }}>
                    <div>
                      <div className="rf-sec-title">Index ANAF</div>
                      <div className="mono" style={{ marginTop: 4 }}>{inv.anafIndex || "—"}</div>
                    </div>
                    <div>
                      <div className="rf-sec-title">ID descărcare ANAF</div>
                      <div className="mono" style={{ marginTop: 4 }}>{inv.anafDownloadId}</div>
                    </div>
                    <div>
                      <div className="rf-sec-title">Descărcat la</div>
                      <div style={{ marginTop: 4, fontSize: 12, color: "var(--rf-text-muted)" }}>{fmtTime(inv.downloadedAt)}</div>
                    </div>
                    <div>
                      <div className="rf-sec-title">Creat la</div>
                      <div style={{ marginTop: 4, fontSize: 12, color: "var(--rf-text-muted)" }}>{fmtTime(inv.createdAt)}</div>
                    </div>
                  </div>
                </SectionCard>

                {/* Files */}
                <SectionCard icon="file" title="Fișiere">
                  <div style={{ padding: "0 16px 16px", display: "flex", flexDirection: "column", gap: 10 }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                      <Icon name="file" size={16} style={{ color: "var(--rf-text-muted)" }} />
                      <span style={{ fontWeight: 600, fontSize: 13 }}>XML</span>
                      <span
                        className="mono"
                        style={{ fontSize: 11, color: "var(--rf-text-muted)", flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
                      >
                        {inv.xmlPath}
                      </span>
                      <Btn
                        variant="secondary"
                        icon="download"
                        size="sm"
                        disabled={!inv.xmlPath}
                        onClick={async () => {
                          if (!inv.xmlPath) { notify.error("XML indisponibil"); return; }
                          try { await openPath(inv.xmlPath); } catch (e) { notify.error(formatError(e, "Eroare deschidere XML.")); }
                        }}
                      >
                        Deschide XML
                      </Btn>
                    </div>
                    {inv.pdfPath && (
                      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                        <Icon name="file" size={16} style={{ color: "var(--rf-text-muted)" }} />
                        <span style={{ fontWeight: 600, fontSize: 13 }}>PDF</span>
                        <span
                          className="mono"
                          style={{ fontSize: 11, color: "var(--rf-text-muted)", flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
                        >
                          {inv.pdfPath}
                        </span>
                        <Btn
                          variant="secondary"
                          icon="download"
                          size="sm"
                          onClick={async () => {
                            if (!inv.pdfPath) { notify.error("PDF indisponibil"); return; }
                            try { await openPath(inv.pdfPath); } catch (e) { notify.error(formatError(e, "Eroare deschidere PDF.")); }
                          }}
                        >
                          Deschide PDF
                        </Btn>
                      </div>
                    )}
                  </div>
                </SectionCard>
              </div>

              {/* Right column — status & actions */}
              <div style={{ position: "sticky", top: 0 }}>
                <SectionCard icon="check" title="Status & Acțiuni">
                  <div style={{ padding: "0 16px 16px", display: "flex", flexDirection: "column", gap: 8 }}>
                    <div style={{ marginBottom: 8 }}>
                      <StatusBadge status={inv.status} />
                    </div>

                    {(inv.status === "NEW" || inv.status === "REVIEWED") && (
                      <>
                        <Btn
                          variant="primary"
                          icon="check"
                          className="btn--block"
                          disabled={isPending}
                          title="Marchează factura ca aprobată în evidența locală. Nu trimite niciun răspuns la ANAF/SPV."
                          onClick={() => updateStatus("APPROVED")}
                        >
                          Aprobă local
                        </Btn>
                        <Btn
                          variant="danger"
                          icon="x"
                          className="btn--block"
                          disabled={isPending}
                          title="Marchează factura ca respinsă în evidența locală. Nu trimite niciun răspuns la ANAF/SPV."
                          onClick={() => updateStatus("REJECTED")}
                        >
                          Respinge local
                        </Btn>
                        <p style={{ fontSize: 11, color: "var(--rf-text-muted)", margin: "4px 0 0" }}>
                          Status intern — nu trimite răspuns la ANAF/SPV.
                        </p>
                      </>
                    )}

                    {inv.status === "APPROVED" && (
                      <Btn
                        variant="secondary"
                        icon="bookmark"
                        className="btn--block"
                        disabled={isPending}
                        onClick={() => updateStatus("ARCHIVED")}
                      >
                        Arhivează
                      </Btn>
                    )}

                    {inv.status === "REJECTED" && (
                      <Btn
                        variant="secondary"
                        icon="refresh"
                        className="btn--block"
                        disabled={isPending}
                        onClick={() => updateStatus("REVIEWED")}
                      >
                        Reanalizează
                      </Btn>
                    )}

                    {inv.status === "ARCHIVED" && (
                      <Badge variant="neutral">Factură arhivată</Badge>
                    )}
                  </div>
                </SectionCard>
              </div>
            </div>
          </>
        ) : null}
      </div>
    </div>
  );
}
