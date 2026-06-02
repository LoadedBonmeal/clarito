/**
 * Companie nouă — re-skinned to rf kit (Wave 3).
 * Multi-step wizard (Date firmă → SPV/ANAF → Logo) within the existing route.
 * Preserves: react-hook-form/Zod, api.companies.create,
 * CUI lookup → api.companies.fetchAnafData(cui) (auto-fill).
 * All fields: legalName/tradeName/registryNumber/cui/address/city/county/
 * postalCode/email/phone/iban/bankName/invoiceSeries/vatPayer/spv.
 */

import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
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
import type { CreateCompanyInput } from "@/types";

// ─── Schema ───────────────────────────────────────────────────────────────────

const CUI_REGEX = /^(RO)?\d{2,10}$/i;

/**
 * IBAN mod-97 validation (ISO 13616).
 * Strips spaces, uppercases, checks format and the mod-97 checksum.
 * Empty string is accepted (field is optional).
 */
function validateIban(raw: string): boolean {
  const s = raw.replace(/\s+/g, "").toUpperCase();
  if (s.length === 0) return true; // optional field
  if (s.length < 15 || s.length > 34) return false;
  if (!/^[A-Z]{2}[0-9]{2}[A-Z0-9]+$/.test(s)) return false;
  // Move first 4 chars to end, convert letters A-Z → 10-35, compute mod 97.
  const rearranged = s.slice(4) + s.slice(0, 4);
  const numeric = rearranged
    .split("")
    .map((c) => (c >= "A" && c <= "Z" ? String(c.charCodeAt(0) - 55) : c))
    .join("");
  // BigInt mod-97 for large numbers.
  let remainder = BigInt(0);
  for (const ch of numeric) {
    remainder = (remainder * BigInt(10) + BigInt(ch)) % BigInt(97);
  }
  return remainder === BigInt(1);
}

const schema = z.object({
  cui: z.string().regex(CUI_REGEX, "Format: 2-10 cifre, opțional cu RO."),
  legalName: z.string().min(2, "Introduceți numele complet."),
  tradeName: z.string().optional(),
  registryNumber: z.string().optional(),
  address: z.string().min(2, "Adresa e obligatorie."),
  city: z.string().min(2, "Localitatea e obligatorie."),
  county: z.string().min(2, "Județul e obligatoriu."),
  postalCode: z.string().optional(),
  email: z.email("Email invalid.").optional().or(z.literal("")),
  phone: z.string().optional(),
  iban: z
    .string()
    .refine((v) => validateIban(v), "IBAN invalid (checksum incorect sau format greșit).")
    .optional()
    .or(z.literal("")),
  bankName: z.string().optional(),
  invoiceSeries: z.string().min(1, "Seria e obligatorie."),
  vatPayer: z.boolean().optional(),
});

type FormValues = z.infer<typeof schema>;

const STEPS = ["Date firmă", "SPV / ANAF", "Logo"];

// ─── Page ──────────────────────────────────────────────────────────────────────

export function CompanyNewPage() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [step, setStep] = useState(1);
  const [cuiLookupLoading, setCuiLookupLoading] = useState(false);
  const [cuiLookupError, setCuiLookupError] = useState<string | null>(null);

  const form = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: {
      cui: "",
      legalName: "",
      tradeName: "",
      registryNumber: "",
      address: "",
      city: "",
      county: "",
      postalCode: "",
      email: "",
      phone: "",
      iban: "",
      bankName: "",
      invoiceSeries: "FACT",
      vatPayer: false,
    },
  });

  const handleCuiLookup = async () => {
    const cui = form.getValues("cui").trim();
    if (!cui) return;
    setCuiLookupLoading(true);
    setCuiLookupError(null);
    try {
      const data = await api.companies.fetchAnafData(cui);
      form.setValue("legalName", data.legalName);
      form.setValue("address", data.address);
      form.setValue("city", data.city);
      form.setValue("county", data.county);
      if (data.registryNumber) form.setValue("registryNumber", data.registryNumber);
      notify.success("Date completate din ANAF.");
    } catch {
      setCuiLookupError("CUI-ul nu a fost găsit în baza ANAF.");
    } finally {
      setCuiLookupLoading(false);
    }
  };

  const create = useMutation({
    mutationFn: (input: CreateCompanyInput) => api.companies.create(input),
    onSuccess: (company) => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.companies.all });
      void navigate({ to: "/companies/$id", params: { id: company.id } });
    },
    onError: (e) => notify.error(formatError(e, "Eroare la crearea companiei.")),
  });

  const onSubmit = (v: FormValues) => {
    create.mutate({
      cui: v.cui,
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
    });
  };

  // Step 1: validate required date firmă fields before advancing
  const advanceStep = async () => {
    if (step === 1) {
      const ok = await form.trigger(["cui", "legalName", "address", "city", "county", "invoiceSeries"]);
      if (!ok) return;
    }
    setStep((s) => Math.min(s + 1, 3));
  };

  return (
    <div className="rf-page">
      {/* Page header */}
      <div className="rf-page-head">
        <div>
          <h1 className="rf-page-title">Companie nouă</h1>
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
            {/* ── Step 1: Date firmă ── */}
            {step === 1 && (
              <form
                id="company-form"
                onSubmit={form.handleSubmit(onSubmit)}
                style={{ display: "flex", flexDirection: "column", gap: 14 }}
              >
                <div className="rf-grid-2">
                  <Field
                    label="CUI"
                    required
                    error={form.formState.errors.cui?.message}
                  >
                    <div style={{ display: "flex", gap: 6 }}>
                      <Input
                        className="mono"
                        placeholder="RO12345678"
                        {...form.register("cui")}
                        error={!!form.formState.errors.cui}
                        style={{ flex: 1 }}
                      />
                      <Btn
                        variant="secondary"
                        size="sm"
                        disabled={cuiLookupLoading}
                        onClick={(e) => {
                          e.preventDefault();
                          void handleCuiLookup();
                        }}
                      >
                        {cuiLookupLoading ? "Se caută…" : "ANAF ↗"}
                      </Btn>
                    </div>
                    {cuiLookupError && (
                      <span className="rf-help rf-help--err">{cuiLookupError}</span>
                    )}
                  </Field>
                  <FormField
                    id="registryNumber"
                    label="Nr. Reg. Comerțului"
                    placeholder="J40/1234/2020"
                    register={form.register}
                    errors={form.formState.errors}
                    mono
                  />
                </div>

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
                <FormField
                  id="invoiceSeries"
                  label="Serie facturi"
                  required
                  placeholder="FACT"
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
              </form>
            )}

            {/* ── Step 2: SPV / ANAF ── */}
            {step === 2 && (
              <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
                <Banner variant="info">
                  Conexiunea SPV/ANAF OAuth se poate configura după salvarea companiei din
                  pagina de detalii. Puteți continua fără configurarea SPV.
                </Banner>
                <p style={{ fontSize: 13, color: "var(--rf-text-muted)", margin: 0 }}>
                  Datele de autorizare OAuth (Client ID, secret) se completează în pasul de
                  configurare SPV din pagina companiei, după salvare.
                </p>
              </div>
            )}

            {/* ── Step 3: Logo ── */}
            {step === 3 && (
              <div
                style={{
                  display: "flex",
                  flexDirection: "column",
                  alignItems: "center",
                  gap: 14,
                  padding: "10px 0",
                }}
              >
                <div
                  style={{
                    width: 110,
                    height: 110,
                    borderRadius: 12,
                    border: "2px dashed var(--rf-border-strong)",
                    display: "grid",
                    placeItems: "center",
                    color: "var(--rf-text-dim)",
                  }}
                >
                  <Icon name="upload" size={28} />
                </div>
                <p style={{ fontSize: 12, color: "var(--rf-text-muted)", margin: 0, textAlign: "center" }}>
                  Logo-ul poate fi adăugat ulterior din pagina de setări a companiei.
                  <br />PNG sau SVG, recomandat 400×400 px. Apare pe facturile PDF.
                </p>
                {create.error && (
                  <Banner variant="error">
                    {formatError(create.error, "Eroare la crearea companiei.")}
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
            <Btn
              variant="secondary"
              onClick={() => void navigate({ to: "/companies" })}
            >
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
                disabled={create.isPending}
                onClick={() => void form.handleSubmit(onSubmit)()}
              >
                {create.isPending ? "Se salvează…" : "Salvează compania"}
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
