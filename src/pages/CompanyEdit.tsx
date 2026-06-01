/**
 * Editare companie — re-skinned to rf kit (Wave 3).
 * Multi-step wizard style (same as CompanyNew).
 * Preserves: api.companies.update(id, input) + pre-fill from api.companies.get(id).
 * CUI is shown read-only (not editable).
 */

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate, useParams } from "@tanstack/react-router";
import { zodResolver } from "@hookform/resolvers/zod";
import { useForm, type FieldErrors, type UseFormRegister } from "react-hook-form";
import { z } from "zod";

import { Icon } from "@/components/shared/Icon";
import {
  Btn, Card, Field, Input, Banner,
} from "@/components/rf";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { UpdateCompanyInput } from "@/types";

// ─── Schema ───────────────────────────────────────────────────────────────────

const IBAN_REGEX = /^[A-Z]{2}\d{2}[A-Z0-9]{1,30}$/i;

const schema = z.object({
  legalName: z.string().min(2, "Introduceți numele complet."),
  tradeName: z.string().optional(),
  registryNumber: z.string().optional(),
  address: z.string().min(2, "Adresa e obligatorie."),
  city: z.string().min(2, "Localitatea e obligatorie."),
  county: z.string().min(2, "Județul e obligatoriu."),
  postalCode: z.string().optional(),
  email: z.email("Email invalid.").optional().or(z.literal("")),
  phone: z.string().optional(),
  iban: z.string().regex(IBAN_REGEX, "IBAN invalid.").optional().or(z.literal("")),
  bankName: z.string().optional(),
  invoiceSeries: z.string().min(1, "Seria e obligatorie."),
  vatPayer: z.boolean(),
});

type FormValues = z.infer<typeof schema>;

const STEPS = ["Date firmă", "Facturare", "Confirmare"];

// ─── Page ──────────────────────────────────────────────────────────────────────

export function CompanyEditPage() {
  const { id } = useParams({ from: "/companies/$id/edit" });
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [step, setStep] = useState(1);

  const { data, isLoading, error: loadError } = useQuery({
    queryKey: queryKeys.companies.detail(id),
    queryFn: () => api.companies.get(id),
  });

  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    values: data
      ? {
          legalName: data.legalName,
          tradeName: data.tradeName ?? "",
          registryNumber: data.registryNumber ?? "",
          address: data.address,
          city: data.city,
          county: data.county,
          postalCode: data.postalCode ?? "",
          email: data.email ?? "",
          phone: data.phone ?? "",
          iban: data.iban ?? "",
          bankName: data.bankName ?? "",
          invoiceSeries: data.invoiceSeries,
          vatPayer: data.vatPayer,
        }
      : undefined,
  });

  const update = useMutation({
    mutationFn: (input: UpdateCompanyInput) => api.companies.update(id, input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.companies.all });
      void navigate({ to: "/companies" });
      notify.success("Companie salvată.");
    },
    onError: (e) => notify.error(formatError(e, "Eroare la salvarea companiei.")),
  });

  const onSubmit = (v: FormValues) => {
    update.mutate({
      legalName: v.legalName,
      tradeName: v.tradeName || undefined,
      registryNumber: v.registryNumber || undefined,
      address: v.address,
      city: v.city,
      county: v.county,
      postalCode: v.postalCode || undefined,
      email: v.email || undefined,
      phone: v.phone || undefined,
      iban: v.iban || undefined,
      bankName: v.bankName || undefined,
      invoiceSeries: v.invoiceSeries,
      vatPayer: v.vatPayer,
    });
  };

  const advanceStep = async () => {
    if (step === 1) {
      const ok = await form.trigger(["legalName", "address", "city", "county"]);
      if (!ok) return;
    }
    setStep((s) => Math.min(s + 1, 3));
  };

  if (isLoading) {
    return (
      <div className="rf-page">
        <div className="rf-page-head">
          <h1 className="rf-page-title">Se încarcă…</h1>
        </div>
        <div className="rf-page-body">
          <div style={{ padding: 40, color: "var(--rf-text-muted)", fontSize: 13 }}>
            Se încarcă datele companiei…
          </div>
        </div>
      </div>
    );
  }

  if (loadError || !data) {
    return (
      <div className="rf-page">
        <div className="rf-page-head">
          <h1 className="rf-page-title">Companie inexistentă</h1>
        </div>
        <div className="rf-page-body">
          <Banner variant="error">
            Compania cu ID-ul <code>{id}</code> nu a fost găsită.
          </Banner>
        </div>
      </div>
    );
  }

  return (
    <div className="rf-page">
      {/* Page header */}
      <div className="rf-page-head">
        <div>
          <h1 className="rf-page-title">Editează: {data.legalName}</h1>
          <div style={{ fontSize: 13, color: "var(--rf-text-muted)", marginTop: 2 }}>
            CUI: <span className="mono">{data.cui}</span>
          </div>
        </div>
        <div className="rf-toolbar-row" style={{ flexShrink: 0 }}>
          <Btn
            variant="secondary"
            icon="arrowLeft"
            size="sm"
            onClick={() => void navigate({ to: "/companies" })}
          >
            Înapoi
          </Btn>
        </div>
      </div>

      <div className="rf-page-body">
        <div style={{ maxWidth: 640, width: "100%", margin: "0 auto" }}>
          {/* Step indicator */}
          <WizardSteps current={step} steps={STEPS} />

          <Card pad>
            {/* ── Step 1: Identificare + Adresă + Contact ── */}
            {step === 1 && (
              <form
                id="company-edit-form"
                onSubmit={form.handleSubmit(onSubmit)}
                style={{ display: "flex", flexDirection: "column", gap: 14 }}
              >
                {/* CUI read-only */}
                <Field label="CUI">
                  <div
                    className="rf-input mono"
                    style={{
                      background: "var(--rf-bg-muted, var(--rf-border))",
                      color: "var(--rf-text-muted)",
                      cursor: "not-allowed",
                      userSelect: "none",
                    }}
                  >
                    {data.cui}
                  </div>
                </Field>

                <FormField
                  id="legalName"
                  label="Denumire legală"
                  required
                  register={form.register}
                  errors={form.formState.errors}
                />
                <FormField
                  id="tradeName"
                  label="Denumire comercială"
                  register={form.register}
                  errors={form.formState.errors}
                />
                <FormField
                  id="registryNumber"
                  label="Nr. registru comerț"
                  placeholder="J40/1234/2020"
                  register={form.register}
                  errors={form.formState.errors}
                  mono
                />

                <div className="rf-grid-2">
                  <FormField
                    id="city"
                    label="Localitate"
                    required
                    register={form.register}
                    errors={form.formState.errors}
                  />
                  <FormField
                    id="county"
                    label="Județ"
                    required
                    register={form.register}
                    errors={form.formState.errors}
                  />
                </div>

                <FormField
                  id="address"
                  label="Adresă"
                  required
                  register={form.register}
                  errors={form.formState.errors}
                />
                <FormField
                  id="postalCode"
                  label="Cod poștal"
                  register={form.register}
                  errors={form.formState.errors}
                  mono
                />

                <div className="rf-grid-2">
                  <FormField
                    id="email"
                    label="Email"
                    type="email"
                    register={form.register}
                    errors={form.formState.errors}
                  />
                  <FormField
                    id="phone"
                    label="Telefon"
                    register={form.register}
                    errors={form.formState.errors}
                  />
                </div>

                <FormField
                  id="iban"
                  label="IBAN"
                  placeholder="RO49AAAA..."
                  register={form.register}
                  errors={form.formState.errors}
                  mono
                />
                <FormField
                  id="bankName"
                  label="Bancă"
                  register={form.register}
                  errors={form.formState.errors}
                />
              </form>
            )}

            {/* ── Step 2: Facturare ── */}
            {step === 2 && (
              <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
                <FormField
                  id="invoiceSeries"
                  label="Serie facturi"
                  required
                  register={form.register}
                  errors={form.formState.errors}
                  mono
                  uppercase
                />
                <label
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 8,
                    fontSize: 13,
                    cursor: "pointer",
                  }}
                >
                  <input
                    type="checkbox"
                    className="rf-cbx"
                    {...form.register("vatPayer")}
                  />
                  Plătitor TVA
                </label>
              </div>
            )}

            {/* ── Step 3: Confirmare ── */}
            {step === 3 && (
              <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
                <Banner variant="info">
                  Verificați datele înainte de salvare. Apăsați "Salvează" pentru a confirma modificările.
                </Banner>
                <div className="rf-kv-list">
                  <div className="rf-kv-row">
                    <span className="rf-kv-label">Denumire</span>
                    <span className="rf-kv-value">{form.getValues("legalName")}</span>
                  </div>
                  <div className="rf-kv-row">
                    <span className="rf-kv-label">Localitate</span>
                    <span className="rf-kv-value">{form.getValues("city")}, {form.getValues("county")}</span>
                  </div>
                  <div className="rf-kv-row">
                    <span className="rf-kv-label">Serie facturi</span>
                    <span className="rf-kv-value mono">{form.getValues("invoiceSeries")}</span>
                  </div>
                  <div className="rf-kv-row">
                    <span className="rf-kv-label">Plătitor TVA</span>
                    <span className="rf-kv-value">{form.getValues("vatPayer") ? "Da" : "Nu"}</span>
                  </div>
                </div>
                {update.error && (
                  <Banner variant="error">
                    {formatError(update.error, "Eroare la salvarea companiei.")}
                  </Banner>
                )}
              </div>
            )}
          </Card>

          {/* Wizard footer */}
          <div
            style={{
              display: "flex",
              gap: 8,
              justifyContent: "flex-end",
              marginTop: 16,
              alignItems: "center",
            }}
          >
            {step > 1 && (
              <Btn variant="ghost" icon="arrowLeft" onClick={() => setStep((s) => s - 1)}>
                Înapoi
              </Btn>
            )}
            <Btn variant="secondary" onClick={() => void navigate({ to: "/companies" })}>
              Anulează
            </Btn>
            {step < 3 ? (
              <Btn variant="primary" iconRight="arrowRight" onClick={() => void advanceStep()}>
                Continuă
              </Btn>
            ) : (
              <Btn
                variant="primary"
                icon="check"
                disabled={update.isPending}
                onClick={() => void form.handleSubmit(onSubmit)()}
              >
                {update.isPending ? "Se salvează…" : "Salvează"}
              </Btn>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

// ─── WizardSteps ──────────────────────────────────────────────────────────────

function WizardSteps({ current, steps }: { current: number; steps: string[] }) {
  return (
    <div style={{ display: "flex", gap: 8, marginBottom: 20 }}>
      {steps.map((s, i) => {
        const idx = i + 1;
        const done = current > idx;
        const active = current === idx;
        return (
          <div
            key={s}
            style={{ flex: 1, display: "flex", alignItems: "center", gap: 8 }}
          >
            <span
              style={{
                width: 26,
                height: 26,
                borderRadius: "50%",
                display: "grid",
                placeItems: "center",
                fontSize: 12,
                fontWeight: 700,
                flexShrink: 0,
                background: done
                  ? "var(--rf-success)"
                  : active
                  ? "var(--rf-accent)"
                  : "var(--rf-neutral-bg, var(--rf-border))",
                color: done || active ? "#fff" : "var(--rf-text-muted)",
              }}
            >
              {done ? <Icon name="check" size={13} /> : idx}
            </span>
            <span
              style={{
                fontSize: 13,
                fontWeight: active ? 600 : 400,
                color: active ? "var(--rf-text)" : "var(--rf-text-muted)",
              }}
            >
              {s}
            </span>
            {i < steps.length - 1 && (
              <div
                style={{
                  flex: 1,
                  height: 1,
                  background: "var(--rf-border)",
                  marginLeft: 4,
                }}
              />
            )}
          </div>
        );
      })}
    </div>
  );
}

// ─── FormField helper ─────────────────────────────────────────────────────────

interface FormFieldProps {
  id: keyof FormValues;
  label: string;
  required?: boolean;
  placeholder?: string;
  type?: string;
  mono?: boolean;
  uppercase?: boolean;
  register: UseFormRegister<FormValues>;
  errors: FieldErrors<FormValues>;
}

function FormField({
  id,
  label,
  required,
  placeholder,
  type,
  mono,
  uppercase,
  register,
  errors,
}: FormFieldProps) {
  const error = errors[id]?.message as string | undefined;
  return (
    <Field label={label} required={required} error={error}>
      <Input
        id={id}
        type={type}
        placeholder={placeholder}
        error={!!error}
        className={[mono && "mono", uppercase && "uppercase"].filter(Boolean).join(" ")}
        style={uppercase ? { textTransform: "uppercase" } : undefined}
        {...register(id)}
      />
    </Field>
  );
}
