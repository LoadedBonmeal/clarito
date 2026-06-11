/**
 * Detaliu companie — design-system detail page (no dedicated prototype; follows
 * the InvoiceDetail .page-head crumb + .cols-2 / .kv card conventions):
 *   .page-head (.crumb "Companii › {denumire}" · .head-title h1 + chip SPV +
 *   chip regim fiscal · sub CUI/localitate/serie · .head-actions Înapoi /
 *   btn-dark Editează) → .cols-2: left = Identificare (.kv) + Adresă (.kv) +
 *   Contact și plată (.kv), right = Facturare (.kv) + ANAF SPV (status +
 *   Conectează SPV) + Certificate SPV (.scr-table Emis/Expiră/Status/Revocă +
 *   Reautorizare în toolbar).
 *
 * ALL wiring preserved: api.companies.get(id), Editează → /companies/$id/edit,
 * SPV connect → api.anaf.authorize (granted check + invalidations + toasts),
 * certificate → api.certificates.list/refresh/revoke.
 */

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useParams, useNavigate } from "@tanstack/react-router";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { Certificate, Company } from "@/types";

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];
/** Unix seconds → "03 iun 2026" (design dd lll yyyy format). */
const fmtRoDateU = (unixSec: number) => {
  const d = new Date(unixSec * 1000);
  return `${String(d.getDate()).padStart(2, "0")} ${RO_MON[d.getMonth()]} ${d.getFullYear()}`;
};

/** Check-circle icon — not in Ic's set; inlined verbatim from the prototype. */
const OK_CIRCLE_PATH = '<path d="M9 12.75 11.25 15 15 9.75M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z"/>';

export function CompanyDetailPage() {
  const { t } = useTranslation();
  const { id } = useParams({ from: "/companies/$id" });
  const navigate = useNavigate();

  const { data, isLoading, error } = useQuery({
    queryKey: queryKeys.companies.detail(id),
    queryFn: () => api.companies.get(id),
  });

  if (isLoading) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>{t("companies.detail.loadingTitle")}</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          {t("companies.loadingCompany")}
        </div>
      </div>
    );
  }

  if (error || !data) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>{t("companies.notFound.title")}</h1></div></div>
        <div className="banner danger">
          <Ic name="xMark" />
          <span>
            {t("companies.notFound.pre")} <span className="num">{id}</span> {t("companies.notFound.post")}{" "}
            <a className="link" style={{ cursor: "pointer" }} onClick={() => void navigate({ to: "/companies" })}>
              {t("companies.notFound.backToList")}
            </a>
          </span>
        </div>
      </div>
    );
  }

  return (
    <div className="main-inner wide">
      {/* page head */}
      <div className="page-head">
        <div>
          <div className="crumb">
            <a onClick={() => void navigate({ to: "/companies" })} style={{ cursor: "pointer" }}>{t("companies.title")}</a>
            <span className="sep">›</span>
            <span>{data.legalName}</span>
          </div>
          <div className="head-title">
            <h1>{data.legalName}</h1>
            {data.spvEnabled ? (
              <span className="chip paid">
                <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: OK_CIRCLE_PATH }} />
                {t("companies.detail.spvActive")}
              </span>
            ) : (
              <span className="chip sent"><Ic name="dot" cls="sic" />{t("companies.detail.spvInactive")}</span>
            )}
            <span className="chip sent">
              {data.taxRegime === "profit" ? t("companies.regime.profit") : t("companies.regime.micro")}
            </span>
          </div>
          <p className="sub">
            <span className="num">{data.cui}</span> · {data.city}, {data.county} · {t("companies.form.subSeries")}{" "}
            <span className="num">{data.invoiceSeries}-{String(data.lastInvoiceNumber).padStart(4, "0")}</span>
          </p>
        </div>
        <div className="head-actions">
          <button className="pill-btn" onClick={() => void navigate({ to: "/companies" })}>
            {t("companies.detail.back")}
          </button>
          <button
            className="btn-dark"
            onClick={() => void navigate({ to: "/companies/$id/edit", params: { id: data.id } })}
          >
            <Ic name="pen" />{t("companies.actions.edit")}
          </button>
        </div>
      </div>

      <div className="cols-2">
        <div>
          {/* Identificare */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">{t("companies.form.sections.identification")}</div>
              <div className="spacer" />
              <Ic name="building" cls="ic" />
            </div>
            <div className="card-pad">
              <dl className="kv">
                <dt>{t("companies.form.fields.cui")}</dt><dd className="num">{data.cui}</dd>
                <dt>{t("companies.form.fields.legalName")}</dt><dd>{data.legalName}</dd>
                {data.tradeName && (
                  <><dt>{t("companies.form.fields.tradeName")}</dt><dd>{data.tradeName}</dd></>
                )}
                <dt>{t("companies.form.fields.regCom")}</dt>
                <dd>{data.registryNumber ? <span className="num">{data.registryNumber}</span> : "—"}</dd>
                <dt>{t("companies.form.fields.vatPayer")}</dt>
                <dd>{data.vatPayer ? <span className="pos">✓ {t("companies.form.yes")}</span> : t("companies.form.no")}</dd>
                <dt>{t("companies.form.fields.taxRegime")}</dt>
                <dd>{data.taxRegime === "profit" ? t("companies.form.regime.profit") : t("companies.form.regime.micro")}</dd>
              </dl>
            </div>
          </div>

          {/* Adresă */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar"><div className="tt">{t("companies.form.sections.address")}</div></div>
            <div className="card-pad">
              <dl className="kv">
                <dt>{t("companies.form.fields.address")}</dt><dd>{data.address}</dd>
                <dt>{t("companies.form.fields.city")}</dt><dd>{data.city}</dd>
                <dt>{t("companies.form.fields.county")}</dt><dd>{data.county}</dd>
                <dt>{t("companies.form.fields.postalCode")}</dt>
                <dd>{data.postalCode ? <span className="num">{data.postalCode}</span> : "—"}</dd>
                <dt>{t("companies.detail.kv.country")}</dt><dd>{data.country}</dd>
              </dl>
            </div>
          </div>

          {/* Contact și plată */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">{t("companies.detail.sections.contactPay")}</div>
              <div className="spacer" />
              <Ic name="mail" cls="ic" />
            </div>
            <div className="card-pad">
              <dl className="kv">
                <dt>{t("companies.form.fields.email")}</dt><dd>{data.email ?? "—"}</dd>
                <dt>{t("companies.form.fields.phone")}</dt>
                <dd>{data.phone ? <span className="num">{data.phone}</span> : "—"}</dd>
                <dt>{t("companies.form.fields.iban")}</dt>
                <dd>{data.iban ? <span className="num">{data.iban}</span> : "—"}</dd>
                <dt>{t("companies.form.fields.bank")}</dt><dd>{data.bankName ?? "—"}</dd>
              </dl>
            </div>
          </div>
        </div>

        <div>
          {/* Facturare */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">{t("companies.detail.sections.billing")}</div>
              <div className="spacer" />
              <Ic name="docText" cls="ic" />
            </div>
            <div className="card-pad">
              <dl className="kv" style={{ gridTemplateColumns: "130px 1fr", fontSize: 12.5 }}>
                <dt>{t("companies.form.fields.invoiceSeries")}</dt><dd className="num">{data.invoiceSeries}</dd>
                <dt>{t("companies.detail.kv.lastNumber")}</dt>
                <dd className="num">{String(data.lastInvoiceNumber).padStart(4, "0")}</dd>
              </dl>
            </div>
          </div>

          {/* SPV */}
          <CompanySpvSection company={data} />

          {/* Certificate */}
          <CertificatesSection companyId={data.id} />
        </div>
      </div>
    </div>
  );
}

// ─── CompanySpvSection — ANAF SPV status + connect (design card) ─────────────

function CompanySpvSection({ company }: { company: Company }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const connectSpv = useMutation({
    mutationFn: () => api.anaf.authorize(company.id),
    onSuccess: (granted) => {
      if (granted) {
        void queryClient.invalidateQueries({
          queryKey: queryKeys.companies.detail(company.id),
        });
        void queryClient.invalidateQueries({
          queryKey: queryKeys.certificates.list(company.id),
        });
        notify.success(t("companies.detail.spv.notifyConnected"));
      } else {
        notify.error(t("companies.detail.spv.notifyCancelled"));
      }
    },
    onError: (e) => notify.error(formatError(e, t("companies.detail.spv.notifyFailed"))),
  });

  return (
    <div className="scr-card" style={{ marginBottom: 14 }}>
      <div className="scr-toolbar">
        <div className="tt">{t("companies.detail.spv.title")}</div>
        <div className="spacer" />
        {!company.spvEnabled && (
          <button
            className="pill-btn send-btn"
            disabled={connectSpv.isPending}
            onClick={() => connectSpv.mutate()}
          >
            <Ic name="shield" />
            {connectSpv.isPending ? t("companies.detail.spv.authorizing") : t("companies.detail.spv.connect")}
          </button>
        )}
      </div>
      <div className="card-pad">
        {company.spvEnabled ? (
          <div style={{ display: "flex", gap: 10, alignItems: "flex-start" }}>
            <svg
              className="ic"
              viewBox="0 0 24 24"
              style={{ stroke: "var(--green)", flex: "none", marginTop: 1 }}
              dangerouslySetInnerHTML={{ __html: OK_CIRCLE_PATH }}
            />
            <div>
              <div style={{ fontSize: 13, fontWeight: 600 }}>{t("companies.detail.spv.connectedTitle")}</div>
              <div style={{ fontSize: 12, color: "var(--text-2)", marginTop: 2 }}>
                {t("companies.detail.spv.connectedDesc")}
              </div>
            </div>
          </div>
        ) : (
          <div style={{ display: "flex", gap: 10, alignItems: "flex-start" }}>
            <Ic name="xMark" cls="ic" />
            <div>
              <div style={{ fontSize: 13, fontWeight: 600 }}>{t("companies.detail.spv.notConnectedTitle")}</div>
              <div style={{ fontSize: 12, color: "var(--text-2)", marginTop: 2 }}>
                {t("companies.detail.spv.notConnectedDesc")}
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

// ─── CertificatesSection — certificate SPV/OAuth (design .scr-table) ─────────

function CertificatesSection({ companyId }: { companyId: string }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();

  const { data: certs = [], isLoading } = useQuery({
    queryKey: queryKeys.certificates.list(companyId),
    queryFn: () => api.certificates.list(companyId),
  });

  const refreshCert = useMutation({
    mutationFn: () => api.certificates.refresh(companyId),
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: queryKeys.certificates.list(companyId),
      });
      notify.success(t("companies.detail.certs.notifyRefreshed"));
    },
    onError: (e) =>
      notify.error(formatError(e, t("companies.detail.certs.notifyRefreshError"))),
  });

  const revokeCert = useMutation({
    mutationFn: (_certId: string) => api.certificates.revoke(companyId),
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: queryKeys.certificates.list(companyId),
      });
      notify.success(t("companies.detail.certs.notifyRevoked"));
    },
    onError: (e) =>
      notify.error(formatError(e, t("companies.detail.certs.notifyRevokeError"))),
  });

  return (
    <div className="scr-card">
      <div className="scr-toolbar">
        <div className="tt">{t("companies.detail.certs.title")}</div>
        <div className="spacer" />
        <button
          className="pill-btn"
          disabled={refreshCert.isPending}
          onClick={() => refreshCert.mutate()}
        >
          <Ic name="sync" />
          {refreshCert.isPending ? t("companies.detail.certs.authorizing") : t("companies.detail.certs.reauth")}
        </button>
      </div>
      {isLoading ? (
        <div style={{ padding: "22px 14px", textAlign: "center", fontSize: 12.5, color: "var(--text-2)" }}>
          {t("companies.loading")}
        </div>
      ) : certs.length === 0 ? (
        <div style={{ padding: "22px 14px", textAlign: "center", fontSize: 12.5, color: "var(--text-2)" }}>
          {t("companies.detail.certs.empty")}
        </div>
      ) : (
        <table className="scr-table">
          <thead>
            <tr>
              <th>{t("companies.detail.certs.issued")}</th>
              <th>{t("companies.detail.certs.expires")}</th>
              <th>{t("companies.detail.certs.status")}</th>
              <th className="r" style={{ width: 90 }}></th>
            </tr>
          </thead>
          <tbody>
            {certs.map((cert: Certificate) => (
              <tr key={cert.id}>
                <td className="num">{fmtRoDateU(cert.issuedAt)}</td>
                <td className="num">{fmtRoDateU(cert.expiresAt)}</td>
                <td>
                  {cert.isActive ? (
                    <span className="chip paid"><Ic name="check" cls="sic" />{t("companies.detail.certs.active")}</span>
                  ) : (
                    <span className="chip sent"><Ic name="dot" cls="sic" />{t("companies.detail.certs.inactive")}</span>
                  )}
                </td>
                <td className="r">
                  <button
                    className="pill-btn"
                    style={{ height: 28 }}
                    disabled={revokeCert.isPending}
                    onClick={() => revokeCert.mutate(cert.id)}
                  >
                    {t("companies.detail.certs.revoke")}
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
