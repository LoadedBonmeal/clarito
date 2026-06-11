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
  const { id } = useParams({ from: "/companies/$id" });
  const navigate = useNavigate();

  const { data, isLoading, error } = useQuery({
    queryKey: queryKeys.companies.detail(id),
    queryFn: () => api.companies.get(id),
  });

  if (isLoading) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>Companie</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          Se încarcă datele companiei…
        </div>
      </div>
    );
  }

  if (error || !data) {
    return (
      <div className="main-inner wide">
        <div className="page-head"><div><h1>Companie inexistentă</h1></div></div>
        <div className="banner danger">
          <Ic name="xMark" />
          <span>
            Compania cu ID-ul <span className="num">{id}</span> nu a fost găsită.{" "}
            <a className="link" style={{ cursor: "pointer" }} onClick={() => void navigate({ to: "/companies" })}>
              Înapoi la listă
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
            <a onClick={() => void navigate({ to: "/companies" })} style={{ cursor: "pointer" }}>Companii</a>
            <span className="sep">›</span>
            <span>{data.legalName}</span>
          </div>
          <div className="head-title">
            <h1>{data.legalName}</h1>
            {data.spvEnabled ? (
              <span className="chip paid">
                <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: OK_CIRCLE_PATH }} />
                SPV activ
              </span>
            ) : (
              <span className="chip sent"><Ic name="dot" cls="sic" />SPV inactiv</span>
            )}
            <span className="chip sent">
              {data.taxRegime === "profit" ? "Profit · 16%" : "Micro · 1%"}
            </span>
          </div>
          <p className="sub">
            <span className="num">{data.cui}</span> · {data.city}, {data.county} · serie{" "}
            <span className="num">{data.invoiceSeries}-{String(data.lastInvoiceNumber).padStart(4, "0")}</span>
          </p>
        </div>
        <div className="head-actions">
          <button className="pill-btn" onClick={() => void navigate({ to: "/companies" })}>
            Înapoi
          </button>
          <button
            className="btn-dark"
            onClick={() => void navigate({ to: "/companies/$id/edit", params: { id: data.id } })}
          >
            <Ic name="pen" />Editează
          </button>
        </div>
      </div>

      <div className="cols-2">
        <div>
          {/* Identificare */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">Identificare</div>
              <div className="spacer" />
              <Ic name="building" cls="ic" />
            </div>
            <div className="card-pad">
              <dl className="kv">
                <dt>CUI</dt><dd className="num">{data.cui}</dd>
                <dt>Denumire legală</dt><dd>{data.legalName}</dd>
                {data.tradeName && (
                  <><dt>Denumire comercială</dt><dd>{data.tradeName}</dd></>
                )}
                <dt>Nr. Reg. Comerțului</dt>
                <dd>{data.registryNumber ? <span className="num">{data.registryNumber}</span> : "—"}</dd>
                <dt>Plătitor TVA</dt>
                <dd>{data.vatPayer ? <span className="pos">✓ Da</span> : "Nu"}</dd>
                <dt>Regim fiscal</dt>
                <dd>{data.taxRegime === "profit" ? "Impozit pe profit (16%)" : "Microîntreprindere (impozit pe venit 1%)"}</dd>
              </dl>
            </div>
          </div>

          {/* Adresă */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar"><div className="tt">Adresă</div></div>
            <div className="card-pad">
              <dl className="kv">
                <dt>Adresă</dt><dd>{data.address}</dd>
                <dt>Localitate</dt><dd>{data.city}</dd>
                <dt>Județ</dt><dd>{data.county}</dd>
                <dt>Cod poștal</dt>
                <dd>{data.postalCode ? <span className="num">{data.postalCode}</span> : "—"}</dd>
                <dt>Țară</dt><dd>{data.country}</dd>
              </dl>
            </div>
          </div>

          {/* Contact și plată */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">Contact și plată</div>
              <div className="spacer" />
              <Ic name="mail" cls="ic" />
            </div>
            <div className="card-pad">
              <dl className="kv">
                <dt>Email</dt><dd>{data.email ?? "—"}</dd>
                <dt>Telefon</dt>
                <dd>{data.phone ? <span className="num">{data.phone}</span> : "—"}</dd>
                <dt>IBAN</dt>
                <dd>{data.iban ? <span className="num">{data.iban}</span> : "—"}</dd>
                <dt>Bancă</dt><dd>{data.bankName ?? "—"}</dd>
              </dl>
            </div>
          </div>
        </div>

        <div>
          {/* Facturare */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">Facturare</div>
              <div className="spacer" />
              <Ic name="docText" cls="ic" />
            </div>
            <div className="card-pad">
              <dl className="kv" style={{ gridTemplateColumns: "130px 1fr", fontSize: 12.5 }}>
                <dt>Serie facturi</dt><dd className="num">{data.invoiceSeries}</dd>
                <dt>Ultimul număr emis</dt>
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
        notify.success("SPV conectat cu succes.");
      } else {
        notify.error("Autorizarea SPV a fost anulată sau a eșuat. Reîncercați.");
      }
    },
    onError: (e) => notify.error(formatError(e, "Conectarea la SPV a eșuat.")),
  });

  return (
    <div className="scr-card" style={{ marginBottom: 14 }}>
      <div className="scr-toolbar">
        <div className="tt">Sistem ANAF SPV</div>
        <div className="spacer" />
        {!company.spvEnabled && (
          <button
            className="pill-btn send-btn"
            disabled={connectSpv.isPending}
            onClick={() => connectSpv.mutate()}
          >
            <Ic name="shield" />
            {connectSpv.isPending ? "Se autorizează…" : "Conectează SPV"}
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
              <div style={{ fontSize: 13, fontWeight: 600 }}>SPV conectat</div>
              <div style={{ fontSize: 12, color: "var(--text-2)", marginTop: 2 }}>
                Această companie poate trimite facturi electronice direct către ANAF.
              </div>
            </div>
          </div>
        ) : (
          <div style={{ display: "flex", gap: 10, alignItems: "flex-start" }}>
            <Ic name="xMark" cls="ic" />
            <div>
              <div style={{ fontSize: 13, fontWeight: 600 }}>SPV neconectat</div>
              <div style={{ fontSize: 12, color: "var(--text-2)", marginTop: 2 }}>
                Pentru a trimite facturi electronice, conectați certificatul digital pentru această
                companie.
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
      notify.success("Certificat reautorizat.");
    },
    onError: (e) =>
      notify.error(formatError(e, "Reautorizarea certificatului a eșuat.")),
  });

  const revokeCert = useMutation({
    mutationFn: (_certId: string) => api.certificates.revoke(companyId),
    onSuccess: () => {
      void queryClient.invalidateQueries({
        queryKey: queryKeys.certificates.list(companyId),
      });
      notify.success("Certificat revocat.");
    },
    onError: (e) =>
      notify.error(formatError(e, "Revocarea certificatului a eșuat.")),
  });

  return (
    <div className="scr-card">
      <div className="scr-toolbar">
        <div className="tt">Certificate SPV / ANAF OAuth</div>
        <div className="spacer" />
        <button
          className="pill-btn"
          disabled={refreshCert.isPending}
          onClick={() => refreshCert.mutate()}
        >
          <Ic name="sync" />
          {refreshCert.isPending ? "Autorizare…" : "Reautorizare SPV"}
        </button>
      </div>
      {isLoading ? (
        <div style={{ padding: "22px 14px", textAlign: "center", fontSize: 12.5, color: "var(--text-2)" }}>
          Se încarcă…
        </div>
      ) : certs.length === 0 ? (
        <div style={{ padding: "22px 14px", textAlign: "center", fontSize: 12.5, color: "var(--text-2)" }}>
          Niciun certificat activ.
        </div>
      ) : (
        <table className="scr-table">
          <thead>
            <tr>
              <th>Emis</th>
              <th>Expiră</th>
              <th>Status</th>
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
                    <span className="chip paid"><Ic name="check" cls="sic" />Activ</span>
                  ) : (
                    <span className="chip sent"><Ic name="dot" cls="sic" />Inactiv</span>
                  )}
                </td>
                <td className="r">
                  <button
                    className="pill-btn"
                    style={{ height: 28 }}
                    disabled={revokeCert.isPending}
                    onClick={() => revokeCert.mutate(cert.id)}
                  >
                    Revocă
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
