/**
 * Detaliu companie — pattern SAGA: tabs cu field-rows (key-value table)
 * pentru info, fără carduri cu shadow.
 */

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { Link, useParams, useNavigate } from "@tanstack/react-router";
import { ArrowLeft, CheckCircle2, ExternalLink, XCircle } from "lucide-react";

import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Section, FieldRow, FieldGroup } from "@/components/shared/Section";
import {
  PageContent,
  PageHeader,
  Toolbar,
} from "@/components/shared/PageHeader";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { notify } from "@/lib/toasts";
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
      <div className="content">
        <PageHeader title="Se încarcă..." />
        <PageContent>
          <Skeleton className="h-48 w-full" />
        </PageContent>
      </div>
    );
  }

  if (error || !data) {
    return (
      <div className="content">
        <PageHeader title="Companie inexistentă" />
        <PageContent>
          <Alert variant="destructive">
            <AlertDescription className="text-xs">
              Compania cu ID-ul <code className="font-mono">{id}</code> nu a
              fost găsită.{" "}
              <Link to="/companies" className="underline">
                Înapoi la listă
              </Link>
            </AlertDescription>
          </Alert>
        </PageContent>
      </div>
    );
  }

  return (
    <div className="content">
      <PageHeader
        title={data.legalName}
        meta={
          <>
            <span className="font-mono">{data.cui}</span>
            <span className="text-muted-foreground/40">·</span>
            <span>{data.city}, {data.county}</span>
            <span className="text-muted-foreground/40">·</span>
            <span className="font-mono">
              {data.invoiceSeries}-
              {String(data.lastInvoiceNumber).padStart(4, "0")}
            </span>
          </>
        }
      />

      <Toolbar>
        <button
          type="button"
          onClick={() => navigate({ to: "/companies" })}
          className="flex h-7 items-center gap-1.5 rounded-sm border border-border bg-background px-2 text-[11px] hover:bg-muted/60"
        >
          <ArrowLeft className="h-3 w-3" />
          <span>Înapoi la listă</span>
        </button>
      </Toolbar>

      <PageContent>
        <Tabs defaultValue="info" className="space-y-3">
          <TabsList className="h-8 rounded-sm bg-muted/40 p-0.5">
            <TabsTrigger value="info" className="h-7 rounded-sm text-[12px]">
              Informații
            </TabsTrigger>
            <TabsTrigger value="invoices" disabled className="h-7 rounded-sm text-[12px]">
              Facturi
            </TabsTrigger>
            <TabsTrigger value="contacts" disabled className="h-7 rounded-sm text-[12px]">
              Contacte
            </TabsTrigger>
            <TabsTrigger value="spv" className="h-7 rounded-sm text-[12px]">
              SPV
            </TabsTrigger>
          </TabsList>

          <TabsContent value="info" className="space-y-3">
            <CompanyInfoSections company={data} />
          </TabsContent>

          <TabsContent value="spv">
            <CompanySpvSection company={data} />
            <CertificatesSection companyId={data.id} />
          </TabsContent>
        </Tabs>
      </PageContent>
    </div>
  );
}

// ─── Sections ─────────────────────────────────────────────────────────────

function CompanyInfoSections({ company }: { company: Company }) {
  return (
    <>
      <Section title="Identificare">
        <FieldGroup>
          <FieldRow label="CUI" mono>
            {company.cui}
          </FieldRow>
          <FieldRow label="Denumire legală">{company.legalName}</FieldRow>
          {company.tradeName && (
            <FieldRow label="Denumire comercială">{company.tradeName}</FieldRow>
          )}
          <FieldRow label="Nr. registru comerț" mono>
            {company.registryNumber ?? "—"}
          </FieldRow>
          <FieldRow label="Plătitor TVA">{company.vatPayer ? "Da" : "Nu"}</FieldRow>
        </FieldGroup>
      </Section>

      <Section title="Adresă">
        <FieldGroup>
          <FieldRow label="Adresă">{company.address}</FieldRow>
          <FieldRow label="Localitate">{company.city}</FieldRow>
          <FieldRow label="Județ">{company.county}</FieldRow>
          <FieldRow label="Cod poștal">{company.postalCode ?? "—"}</FieldRow>
          <FieldRow label="Țară">{company.country}</FieldRow>
        </FieldGroup>
      </Section>

      <Section title="Contact și plată">
        <FieldGroup>
          <FieldRow label="Email">{company.email ?? "—"}</FieldRow>
          <FieldRow label="Telefon">{company.phone ?? "—"}</FieldRow>
          <FieldRow label="IBAN" mono>
            {company.iban ?? "—"}
          </FieldRow>
          <FieldRow label="Bancă">{company.bankName ?? "—"}</FieldRow>
        </FieldGroup>
      </Section>

      <Section title="Facturare">
        <FieldGroup>
          <FieldRow label="Serie facturi" mono>
            {company.invoiceSeries}
          </FieldRow>
          <FieldRow label="Ultimul număr emis" mono>
            {String(company.lastInvoiceNumber).padStart(4, "0")}
          </FieldRow>
        </FieldGroup>
      </Section>
    </>
  );
}

function CertificatesSection({ companyId }: { companyId: string }) {
  const queryClient = useQueryClient();

  const { data: certs = [], isLoading } = useQuery({
    queryKey: queryKeys.certificates.list(companyId),
    queryFn: () => api.certificates.list(companyId),
  });

  const refreshCert = useMutation({
    mutationFn: () => api.certificates.refresh(companyId),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.certificates.list(companyId) });
    },
    onError: (e) => notify.error("Eroare reautorizare: " + String(e)),
  });

  const revokeCert = useMutation({
    mutationFn: (_certId: string) => api.certificates.revoke(companyId),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.certificates.list(companyId) });
    },
    onError: (e) => notify.error("Eroare revocare: " + String(e)),
  });

  const fmt = (unix: number) => new Date(unix * 1000).toLocaleDateString("ro-RO");

  return (
    <div className="panel" style={{ marginTop: 16 }}>
      <div
        className="panel-header"
        style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "8px 14px", borderBottom: "1px solid var(--border)" }}
      >
        <span style={{ fontSize: 12, fontWeight: 600 }}>Certificate SPV / ANAF OAuth</span>
        <button
          type="button"
          className="btn primary"
          disabled={refreshCert.isPending}
          onClick={() => refreshCert.mutate()}
        >
          {refreshCert.isPending ? "Autorizare…" : "Reautorizare SPV"}
        </button>
      </div>
      <div style={{ padding: "8px 14px" }}>
        {isLoading ? (
          <span style={{ fontSize: 11, color: "var(--text-muted)" }}>Se încarcă…</span>
        ) : certs.length === 0 ? (
          <span style={{ fontSize: 11, color: "var(--text-muted)" }}>Niciun certificat activ.</span>
        ) : (
          <table style={{ width: "100%", fontSize: 11, borderCollapse: "collapse" }}>
            <thead>
              <tr style={{ color: "var(--text-muted)", textAlign: "left" }}>
                <th style={{ padding: "4px 8px 4px 0", fontWeight: 600 }}>Emis</th>
                <th style={{ padding: "4px 8px", fontWeight: 600 }}>Expiră</th>
                <th style={{ padding: "4px 8px", fontWeight: 600 }}>Status</th>
                <th style={{ padding: "4px 0", fontWeight: 600 }} />
              </tr>
            </thead>
            <tbody>
              {certs.map((cert: Certificate) => (
                <tr key={cert.id} style={{ borderTop: "1px solid var(--border)" }}>
                  <td style={{ padding: "5px 8px 5px 0" }}>{fmt(cert.issuedAt)}</td>
                  <td style={{ padding: "5px 8px" }}>{fmt(cert.expiresAt)}</td>
                  <td style={{ padding: "5px 8px" }}>
                    {cert.isActive ? (
                      <span style={{ color: "#16A34A", fontWeight: 600 }}>Activ</span>
                    ) : (
                      <span style={{ color: "var(--text-muted)" }}>Inactiv</span>
                    )}
                  </td>
                  <td style={{ padding: "5px 0", textAlign: "right" }}>
                    <button
                      type="button"
                      className="btn"
                      style={{ fontSize: 10 }}
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
    </div>
  );
}

function CompanySpvSection({ company }: { company: Company }) {
  const queryClient = useQueryClient();

  const connectSpv = useMutation({
    mutationFn: () => api.anaf.authorize(company.id),
    onSuccess: (granted) => {
      if (granted) {
        void queryClient.invalidateQueries({ queryKey: queryKeys.companies.detail(company.id) });
        void queryClient.invalidateQueries({ queryKey: queryKeys.certificates.list(company.id) });
      } else {
        notify.error("Autorizarea SPV a fost anulată sau a eșuat. Reîncercați.");
      }
    },
    onError: (e) => notify.error("Eroare conectare SPV: " + String(e)),
  });

  return (
    <Section title="Sistem ANAF SPV">
      <div className="p-4">
        {company.spvEnabled ? (
          <div className="flex items-start gap-3">
            <CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0 text-success" />
            <div>
              <p className="text-[12px] font-medium">SPV conectat</p>
              <p className="mt-0.5 text-[11px] text-muted-foreground">
                Această companie poate trimite facturi electronice direct
                către ANAF.
              </p>
            </div>
          </div>
        ) : (
          <div className="flex items-start gap-3">
            <XCircle className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
            <div>
              <p className="text-[12px] font-medium">SPV neconectat</p>
              <p className="mt-0.5 text-[11px] text-muted-foreground">
                Pentru a trimite facturi electronice, conectează certificatul
                digital pentru această companie.
              </p>
              <button
                type="button"
                disabled={connectSpv.isPending}
                className="mt-3 inline-flex h-7 items-center gap-1.5 rounded-sm border border-border bg-background px-2.5 text-[11px] font-medium disabled:opacity-50"
                onClick={() => connectSpv.mutate()}
              >
                <ExternalLink className="h-3 w-3" />
                <span>{connectSpv.isPending ? "Se autorizează…" : "Conectează SPV"}</span>
              </button>
            </div>
          </div>
        )}
      </div>
    </Section>
  );
}
