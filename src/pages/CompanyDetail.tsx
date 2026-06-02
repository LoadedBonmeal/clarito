/**
 * Detaliu companie — re-skinned to rf kit (Wave 3).
 * Preserves: api.companies.get(id), info display, Editează → /companies/$id/edit,
 * SPV section with api.anaf.authorize, certificates with api.certificates.*
 */

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { Link, useParams, useNavigate } from "@tanstack/react-router";

import { Icon } from "@/components/shared/Icon";
import {
  PageHeader, Btn, SectionCard, Badge, Banner,
} from "@/components/rf";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { Certificate, Company } from "@/types";

export function CompanyDetailPage() {
  const { id } = useParams({ from: "/companies/$id" });
  const navigate = useNavigate();

  const { data, isLoading, error } = useQuery({
    queryKey: queryKeys.companies.detail(id),
    queryFn: () => api.companies.get(id),
  });

  if (isLoading) {
    return (
      <div className="rf-page">
        <PageHeader title="Se încarcă…" />
        <div className="rf-page-body">
          <div style={{ padding: 40, color: "var(--rf-text-muted)", fontSize: 13 }}>Se încarcă datele companiei…</div>
        </div>
      </div>
    );
  }

  if (error || !data) {
    return (
      <div className="rf-page">
        <PageHeader title="Companie inexistentă" />
        <div className="rf-page-body">
          <Banner variant="error">
            Compania cu ID-ul <code>{id}</code> nu a fost găsită.{" "}
            <Link to="/companies" style={{ textDecoration: "underline" }}>
              Înapoi la listă
            </Link>
          </Banner>
        </div>
      </div>
    );
  }

  return (
    <div className="rf-page">
      <PageHeader
        title={data.legalName}
        sub={
          <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap", marginTop: 2 }}>
            <span className="mono" style={{ fontSize: 13, color: "var(--rf-text-muted)" }}>{data.cui}</span>
            <span style={{ color: "var(--rf-border)" }}>·</span>
            <span style={{ fontSize: 13, color: "var(--rf-text-muted)" }}>{data.city}, {data.county}</span>
            <span style={{ color: "var(--rf-border)" }}>·</span>
            <span className="mono" style={{ fontSize: 13, color: "var(--rf-text-muted)" }}>
              {data.invoiceSeries}-{String(data.lastInvoiceNumber).padStart(4, "0")}
            </span>
            {data.spvEnabled ? (
              <Badge variant="success" dot={false}>SPV activ</Badge>
            ) : (
              <Badge variant="neutral" dot={false}>SPV inactiv</Badge>
            )}
          </div>
        }
        actions={
          <>
            <Btn
              variant="secondary"
              icon="arrowLeft"
              size="sm"
              onClick={() => void navigate({ to: "/companies" })}
            >
              Înapoi
            </Btn>
            <Btn
              variant="primary"
              icon="pen"
              size="sm"
              onClick={() =>
                void navigate({ to: "/companies/$id/edit", params: { id: data.id } })
              }
            >
              Editează
            </Btn>
          </>
        }
      />

      <div className="rf-page-body">
        <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
          {/* Identificare */}
          <SectionCard icon="building" title="Identificare">
            <div className="rf-kv-list">
              <KvRow label="CUI" mono>{data.cui}</KvRow>
              <KvRow label="Denumire legală">{data.legalName}</KvRow>
              {data.tradeName && <KvRow label="Denumire comercială">{data.tradeName}</KvRow>}
              <KvRow label="Nr. registru comerț" mono>{data.registryNumber ?? "—"}</KvRow>
              <KvRow label="Plătitor TVA">{data.vatPayer ? "Da" : "Nu"}</KvRow>
            </div>
          </SectionCard>

          {/* Adresă */}
          <SectionCard icon="map" title="Adresă">
            <div className="rf-kv-list">
              <KvRow label="Adresă">{data.address}</KvRow>
              <KvRow label="Localitate">{data.city}</KvRow>
              <KvRow label="Județ">{data.county}</KvRow>
              <KvRow label="Cod poștal">{data.postalCode ?? "—"}</KvRow>
              <KvRow label="Țară">{data.country}</KvRow>
            </div>
          </SectionCard>

          {/* Contact și plată */}
          <SectionCard icon="mail" title="Contact și plată">
            <div className="rf-kv-list">
              <KvRow label="Email">{data.email ?? "—"}</KvRow>
              <KvRow label="Telefon">{data.phone ?? "—"}</KvRow>
              <KvRow label="IBAN" mono>{data.iban ?? "—"}</KvRow>
              <KvRow label="Bancă">{data.bankName ?? "—"}</KvRow>
            </div>
          </SectionCard>

          {/* Facturare */}
          <SectionCard icon="file" title="Facturare">
            <div className="rf-kv-list">
              <KvRow label="Serie facturi" mono>{data.invoiceSeries}</KvRow>
              <KvRow label="Ultimul număr emis" mono>
                {String(data.lastInvoiceNumber).padStart(4, "0")}
              </KvRow>
            </div>
          </SectionCard>

          {/* SPV */}
          <CompanySpvSection company={data} />

          {/* Certificates */}
          <CertificatesSection companyId={data.id} />
        </div>
      </div>
    </div>
  );
}

// ─── KvRow ────────────────────────────────────────────────────────────────────

function KvRow({
  label,
  mono,
  children,
}: {
  label: string;
  mono?: boolean;
  children: React.ReactNode;
}) {
  return (
    <div className="rf-kv-row">
      <span className="rf-kv-label">{label}</span>
      <span className={["rf-kv-value", mono && "mono"].filter(Boolean).join(" ")}>
        {children}
      </span>
    </div>
  );
}

// ─── CompanySpvSection ───────────────────────────────────────────────────────

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
    <SectionCard
      icon="cloudUp"
      title="Sistem ANAF SPV"
      actions={
        !company.spvEnabled && (
          <Btn
            variant="primary"
            size="sm"
            icon="external"
            disabled={connectSpv.isPending}
            onClick={() => connectSpv.mutate()}
          >
            {connectSpv.isPending ? "Se autorizează…" : "Conectează SPV"}
          </Btn>
        )
      }
    >
      <div style={{ padding: "4px 16px 16px" }}>
        {company.spvEnabled ? (
          <div style={{ display: "flex", gap: 10, alignItems: "flex-start" }}>
            <Icon name="checkCircle" size={18} style={{ color: "var(--rf-success)", flexShrink: 0, marginTop: 1 }} />
            <div>
              <div style={{ fontSize: 13, fontWeight: 600 }}>SPV conectat</div>
              <div style={{ fontSize: 12, color: "var(--rf-text-muted)", marginTop: 2 }}>
                Această companie poate trimite facturi electronice direct către ANAF.
              </div>
            </div>
          </div>
        ) : (
          <div style={{ display: "flex", gap: 10, alignItems: "flex-start" }}>
            <Icon name="xCircle" size={18} style={{ color: "var(--rf-text-muted)", flexShrink: 0, marginTop: 1 }} />
            <div>
              <div style={{ fontSize: 13, fontWeight: 600 }}>SPV neconectat</div>
              <div style={{ fontSize: 12, color: "var(--rf-text-muted)", marginTop: 2 }}>
                Pentru a trimite facturi electronice, conectează certificatul digital pentru această companie.
              </div>
            </div>
          </div>
        )}
      </div>
    </SectionCard>
  );
}

// ─── CertificatesSection ──────────────────────────────────────────────────────

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

  const fmt = (unix: number) =>
    new Date(unix * 1000).toLocaleDateString("ro-RO");

  return (
    <SectionCard
      icon="key"
      title="Certificate SPV / ANAF OAuth"
      actions={
        <Btn
          variant="secondary"
          size="sm"
          disabled={refreshCert.isPending}
          onClick={() => refreshCert.mutate()}
        >
          {refreshCert.isPending ? "Autorizare…" : "Reautorizare SPV"}
        </Btn>
      }
    >
      <div className="rf-tbl-wrap">
        {isLoading ? (
          <span style={{ fontSize: 12, color: "var(--rf-text-muted)" }}>Se încarcă…</span>
        ) : certs.length === 0 ? (
          <span style={{ fontSize: 12, color: "var(--rf-text-muted)" }}>
            Niciun certificat activ.
          </span>
        ) : (
          <table className="rf-tbl">
            <thead>
              <tr>
                <th>Emis</th>
                <th>Expiră</th>
                <th>Status</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {certs.map((cert: Certificate) => (
                <tr key={cert.id}>
                  <td>{fmt(cert.issuedAt)}</td>
                  <td>{fmt(cert.expiresAt)}</td>
                  <td>
                    {cert.isActive ? (
                      <Badge variant="success" dot={false}>Activ</Badge>
                    ) : (
                      <Badge variant="neutral" dot={false}>Inactiv</Badge>
                    )}
                  </td>
                  <td onClick={(e) => e.stopPropagation()}>
                    <Btn
                      variant="secondary"
                      size="sm"
                      disabled={revokeCert.isPending}
                      onClick={() => revokeCert.mutate(cert.id)}
                    >
                      Revocă
                    </Btn>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </SectionCard>
  );
}
