/**
 * Form companie nouă — layout SAGA: label-left în field-rows, secțiuni
 * bordurate fără shadow, footer toolbar pentru acțiuni.
 */

import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { zodResolver } from "@hookform/resolvers/zod";
import { useForm, type FieldErrors, type UseFormRegister } from "react-hook-form";
import { z } from "zod";
import { ArrowLeft } from "lucide-react";

import { Input } from "@/components/ui/input";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Section } from "@/components/shared/Section";
import {
  PageContent,
  PageHeader,
  Toolbar,
} from "@/components/shared/PageHeader";
import { cn } from "@/lib/utils";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import type { AppErrorPayload, CreateCompanyInput } from "@/types";

// ─── Schemă validare ──────────────────────────────────────────────────────

const CUI_REGEX = /^(RO)?\d{2,10}$/i;
const IBAN_REGEX = /^[A-Z]{2}\d{2}[A-Z0-9]{1,30}$/i;

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
  iban: z.string().regex(IBAN_REGEX, "IBAN invalid.").optional().or(z.literal("")),
  bankName: z.string().optional(),
  invoiceSeries: z.string().min(1, "Seria e obligatorie."),
});

type FormValues = z.infer<typeof schema>;

// ─── Page ─────────────────────────────────────────────────────────────────

export function CompanyNewPage() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();

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
    },
  });

  const [cuiLookupLoading, setCuiLookupLoading] = useState(false);
  const [cuiLookupError, setCuiLookupError] = useState<string | null>(null);

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
      navigate({ to: "/companies/$id", params: { id: company.id } });
    },
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

  const error = create.error as unknown as AppErrorPayload | undefined;

  return (
    <form onSubmit={form.handleSubmit(onSubmit)} className="content">
      <PageHeader title="Companie nouă" />

      <Toolbar>
        <button
          type="button"
          onClick={() => navigate({ to: "/companies" })}
          className="flex h-7 items-center gap-1.5 rounded-sm border border-border bg-background px-2 text-[11px] hover:bg-muted/60"
        >
          <ArrowLeft className="h-3 w-3" />
          <span>Înapoi</span>
        </button>

        <div className="ml-auto flex items-center gap-1.5">
          <button
            type="button"
            onClick={() => navigate({ to: "/companies" })}
            className="h-7 rounded-sm border border-border bg-background px-3 text-[11px] hover:bg-muted/60"
          >
            Anulează
          </button>
          <button
            type="submit"
            disabled={create.isPending}
            className="h-7 rounded-sm bg-primary px-3 text-[11px] font-medium text-primary-foreground hover:bg-primary/90 disabled:opacity-60"
          >
            {create.isPending ? "Se salvează..." : "Salvează"}
          </button>
        </div>
      </Toolbar>

      <PageContent className="flex-1 space-y-3 overflow-auto">
        <div className="mx-auto max-w-3xl space-y-3">
          <Section title="Identificare">
            <FormRow
              id="cui"
              label="CUI *"
              placeholder="RO12345678"
              register={form.register}
              errors={form.formState.errors}
              mono
            />
            <div className="flex border-b border-border last:border-b-0">
              <div className="flex w-[180px] shrink-0 items-center border-r border-border bg-muted/20 px-3 py-1.5 text-[11px] font-medium text-muted-foreground" />
              <div className="flex min-w-0 flex-1 items-center gap-2 px-2 py-1">
                <button
                  type="button"
                  className="h-7 rounded-sm border border-border bg-background px-2.5 text-[11px] hover:bg-muted/60 disabled:opacity-50"
                  disabled={cuiLookupLoading}
                  onClick={() => { void handleCuiLookup(); }}
                >
                  {cuiLookupLoading ? "Se caută…" : "Caută în ANAF ↗"}
                </button>
                {cuiLookupError && (
                  <span className="text-[11px] text-destructive">{cuiLookupError}</span>
                )}
              </div>
            </div>
            <FormRow
              id="legalName"
              label="Denumire legală *"
              register={form.register}
              errors={form.formState.errors}
            />
            <FormRow
              id="tradeName"
              label="Denumire comercială"
              register={form.register}
              errors={form.formState.errors}
            />
            <FormRow
              id="registryNumber"
              label="Nr. registru comerț"
              placeholder="J40/1234/2020"
              register={form.register}
              errors={form.formState.errors}
              mono
            />
          </Section>

          <Section title="Adresă">
            <FormRow
              id="address"
              label="Adresă *"
              register={form.register}
              errors={form.formState.errors}
            />
            <FormRow
              id="city"
              label="Localitate *"
              register={form.register}
              errors={form.formState.errors}
            />
            <FormRow
              id="county"
              label="Județ *"
              register={form.register}
              errors={form.formState.errors}
            />
            <FormRow
              id="postalCode"
              label="Cod poștal"
              register={form.register}
              errors={form.formState.errors}
              mono
            />
          </Section>

          <Section title="Contact și plată">
            <FormRow
              id="email"
              label="Email"
              type="email"
              register={form.register}
              errors={form.formState.errors}
            />
            <FormRow
              id="phone"
              label="Telefon"
              register={form.register}
              errors={form.formState.errors}
            />
            <FormRow
              id="iban"
              label="IBAN"
              placeholder="RO49AAAA..."
              register={form.register}
              errors={form.formState.errors}
              mono
            />
            <FormRow
              id="bankName"
              label="Bancă"
              register={form.register}
              errors={form.formState.errors}
            />
          </Section>

          <Section title="Facturare">
            <FormRow
              id="invoiceSeries"
              label="Serie facturi *"
              hint="Facturile vor fi numerotate ex: FACT-0001, FACT-0002..."
              register={form.register}
              errors={form.formState.errors}
              mono
              uppercase
            />
          </Section>

          {error && (
            <Alert variant="destructive">
              <AlertDescription className="text-xs">
                {error.message}
              </AlertDescription>
            </Alert>
          )}
        </div>
      </PageContent>
    </form>
  );
}

// ─── Label-left form row ──────────────────────────────────────────────────

interface FormRowProps {
  id: keyof FormValues;
  label: string;
  placeholder?: string;
  type?: string;
  hint?: string;
  mono?: boolean;
  uppercase?: boolean;
  register: UseFormRegister<FormValues>;
  errors: FieldErrors<FormValues>;
}

function FormRow({
  id,
  label,
  placeholder,
  type,
  hint,
  mono,
  uppercase,
  register,
  errors,
}: FormRowProps) {
  const error = errors[id]?.message as string | undefined;

  return (
    <div className="flex border-b border-border last:border-b-0">
      <label
        htmlFor={id}
        className="flex w-[180px] shrink-0 items-center border-r border-border bg-muted/20 px-3 py-1.5 text-[11px] font-medium text-muted-foreground"
      >
        {label}
      </label>
      <div className="min-w-0 flex-1 px-2 py-1">
        <Input
          id={id}
          type={type}
          placeholder={placeholder}
          className={cn(
            "h-7 rounded-sm border-0 bg-transparent px-1 text-[12px] shadow-none focus-visible:ring-1",
            mono && "font-mono",
            uppercase && "uppercase",
          )}
          {...register(id)}
        />
        {hint && !error && (
          <p className="px-1 pb-0.5 text-[10px] text-muted-foreground">{hint}</p>
        )}
        {error && (
          <p className="px-1 pb-0.5 text-[10px] text-destructive">{error}</p>
        )}
      </div>
    </div>
  );
}
