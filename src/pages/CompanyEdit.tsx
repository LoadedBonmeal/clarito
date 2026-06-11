/**
 * Editare companie — design-system form page (no dedicated prototype; follows
 * the same .fgrid/.field conventions as CompanyNew / Contacts modal):
 *   .page-head (.crumb "Companii › {denumire} › Editează" + h1 + sub CUI/serie +
 *   .head-actions Renunță / btn-dark "Salvează") → .scr-card "Identificare"
 *   (CUI read-only, denumiri, Reg. Com., plătitor TVA) → "Adresă" →
 *   "Contact & bancă" → "Facturare & regim fiscal" (serie facturi cu confirmare
 *   la schimbare după facturi emise, regim micro/profit).
 *
 * ALL wiring preserved: react-hook-form + Zod (IBAN mod-97), pre-fill din
 * api.companies.get(id), api.companies.update(id, input) → invalidate +
 * navigate, confirm() Tauri la schimbarea seriei când există facturi emise
 * (continuitatea numerotării). CUI rămâne needitabil.
 */

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate, useParams } from "@tanstack/react-router";
import { zodResolver } from "@hookform/resolvers/zod";
import { useForm, type FieldErrors, type UseFormRegister } from "react-hook-form";
import { z } from "zod";
import { confirm } from "@tauri-apps/plugin-dialog";

import { Ic } from "@/components/shared/Ic";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { UpdateCompanyInput } from "@/types";

// ─── Schema ───────────────────────────────────────────────────────────────────

/**
 * IBAN mod-97 validation (ISO 13616) — same logic as CompanyNew.
 * Empty string is accepted (IBAN is optional).
 */
function validateIban(raw: string): boolean {
  const s = raw.replace(/\s+/g, "").toUpperCase();
  if (s.length === 0) return true;
  if (s.length < 15 || s.length > 34) return false;
  if (!/^[A-Z]{2}[0-9]{2}[A-Z0-9]+$/.test(s)) return false;
  const rearranged = s.slice(4) + s.slice(0, 4);
  const numeric = rearranged
    .split("")
    .map((c) => (c >= "A" && c <= "Z" ? String(c.charCodeAt(0) - 55) : c))
    .join("");
  let remainder = BigInt(0);
  for (const ch of numeric) {
    remainder = (remainder * BigInt(10) + BigInt(ch)) % BigInt(97);
  }
  return remainder === BigInt(1);
}

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
  iban: z
    .string()
    .refine((v) => validateIban(v), "IBAN invalid (checksum incorect sau format greșit).")
    .optional()
    .or(z.literal("")),
  bankName: z.string().optional(),
  invoiceSeries: z.string().min(1, "Seria e obligatorie."),
  vatPayer: z.boolean(),
  taxRegime: z.enum(["micro", "profit"]),
});

type FormValues = z.infer<typeof schema>;

// ─── Page ──────────────────────────────────────────────────────────────────────

export function CompanyEditPage() {
  const { id } = useParams({ from: "/companies/$id/edit" });
  const navigate = useNavigate();
  const queryClient = useQueryClient();

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
          taxRegime: data.taxRegime === "profit" ? "profit" : "micro",
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

  const onSubmit = async (v: FormValues) => {
    // Task 7: warn before changing invoice series if invoices have already been issued.
    const seriesChanged = data && v.invoiceSeries.trim() !== data.invoiceSeries.trim();
    const hasIssuedInvoices = data && data.lastInvoiceNumber > 0;
    if (seriesChanged && hasIssuedInvoices) {
      const ok = await confirm(
        `Compania a emis deja ${data.lastInvoiceNumber} factur${data.lastInvoiceNumber === 1 ? "ă" : "i"} cu seria "${data.invoiceSeries}". ` +
          `Schimbarea seriei la "${v.invoiceSeries}" poate întrerupe continuitatea numerotării. ` +
          "Doriți să continuați?",
        { title: "Schimbare serie facturi", kind: "warning" },
      );
      if (!ok) return;
    }

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
      taxRegime: v.taxRegime,
    });
  };

  if (isLoading) {
    return (
      <div className="main-inner">
        <div className="page-head"><div><h1>Editare companie</h1></div></div>
        <div style={{ padding: "40px 0", textAlign: "center", color: "var(--text-2)", fontSize: 13 }}>
          Se încarcă datele companiei…
        </div>
      </div>
    );
  }

  if (loadError || !data) {
    return (
      <div className="main-inner">
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

  const { errors } = form.formState;
  const vatPayer = form.watch("vatPayer") ?? false;

  return (
    <div className="main-inner">
      {/* page head */}
      <div className="page-head">
        <div>
          <div className="crumb">
            <a onClick={() => void navigate({ to: "/companies" })} style={{ cursor: "pointer" }}>Companii</a>
            <span className="sep">›</span>
            <a
              onClick={() => void navigate({ to: "/companies/$id", params: { id: data.id } })}
              style={{ cursor: "pointer" }}
            >
              {data.legalName}
            </a>
            <span className="sep">›</span>
            <span>Editează</span>
          </div>
          <h1>Editează: {data.legalName}</h1>
          <p className="sub">
            CUI <span className="num">{data.cui}</span> · serie{" "}
            <span className="num">{data.invoiceSeries}-{String(data.lastInvoiceNumber).padStart(4, "0")}</span>
          </p>
        </div>
        <div className="head-actions">
          <button className="pill-btn" onClick={() => void navigate({ to: "/companies" })}>
            Renunță
          </button>
          <button
            className="btn-dark"
            type="submit"
            form="company-edit-form"
            disabled={update.isPending}
            style={update.isPending ? { opacity: 0.6 } : undefined}
          >
            <Ic name="check" />
            {update.isPending ? "Se salvează…" : "Salvează"}
          </button>
        </div>
      </div>

      <form id="company-edit-form" onSubmit={form.handleSubmit(onSubmit)}>
        {/* Identificare */}
        <div className="scr-card" style={{ marginBottom: 14 }}>
          <div className="scr-toolbar"><div className="tt">Identificare</div></div>
          <div className="card-pad">
            <div className="fgrid">
              <div className="field">
                <label>CUI</label>
                <input
                  className="input num"
                  type="text"
                  value={data.cui}
                  disabled
                  style={{ background: "var(--fill)", color: "var(--text-2)", cursor: "not-allowed" }}
                />
                <span className="hint">CUI-ul nu poate fi modificat</span>
              </div>
              <FormField
                id="registryNumber"
                label="Nr. Reg. Comerțului"
                placeholder="J40/1234/2020"
                num
                register={form.register}
                errors={errors}
              />
              <FormField
                id="legalName"
                label="Denumire legală"
                required
                span2
                register={form.register}
                errors={errors}
              />
              <FormField
                id="tradeName"
                label="Denumire comercială"
                register={form.register}
                errors={errors}
              />
              <div className="field">
                <label>Plătitor TVA</label>
                <select
                  className="select"
                  value={vatPayer ? "da" : "nu"}
                  onChange={(e) => form.setValue("vatPayer", e.target.value === "da")}
                >
                  <option value="da">Da</option>
                  <option value="nu">Nu</option>
                </select>
              </div>
            </div>
          </div>
        </div>

        {/* Adresă */}
        <div className="scr-card" style={{ marginBottom: 14 }}>
          <div className="scr-toolbar"><div className="tt">Adresă</div></div>
          <div className="card-pad">
            <div className="fgrid">
              <FormField
                id="address"
                label="Adresă"
                required
                span2
                register={form.register}
                errors={errors}
              />
              <FormField id="city" label="Localitate" required register={form.register} errors={errors} />
              <FormField id="county" label="Județ" required register={form.register} errors={errors} />
              <FormField id="postalCode" label="Cod poștal" num register={form.register} errors={errors} />
            </div>
          </div>
        </div>

        {/* Contact & bancă */}
        <div className="scr-card" style={{ marginBottom: 14 }}>
          <div className="scr-toolbar"><div className="tt">Contact &amp; bancă</div></div>
          <div className="card-pad">
            <div className="fgrid">
              <FormField id="email" label="Email" type="email" placeholder="opțional" register={form.register} errors={errors} />
              <FormField id="phone" label="Telefon" num placeholder="opțional" register={form.register} errors={errors} />
              <FormField
                id="iban"
                label="IBAN"
                span2
                num
                placeholder="RO49AAAA1B31007593840000"
                register={form.register}
                errors={errors}
              />
              <FormField id="bankName" label="Bancă" register={form.register} errors={errors} />
            </div>
          </div>
        </div>

        {/* Facturare & regim fiscal */}
        <div className="scr-card" style={{ marginBottom: 14 }}>
          <div className="scr-toolbar"><div className="tt">Facturare &amp; regim fiscal</div></div>
          <div className="card-pad">
            <div className="fgrid">
              <FormField
                id="invoiceSeries"
                label="Serie facturi"
                required
                num
                uppercase
                hint={
                  data.lastInvoiceNumber > 0
                    ? `s-au emis deja ${data.lastInvoiceNumber} facturi cu seria curentă — schimbarea cere confirmare`
                    : "apare pe toate facturile emise (ex. FACT-0001)"
                }
                register={form.register}
                errors={errors}
              />
              <div className="field">
                <label>Regim fiscal</label>
                <select className="select" {...form.register("taxRegime")}>
                  <option value="micro">Microîntreprindere (impozit pe venit 1%)</option>
                  <option value="profit">Impozit pe profit (16%)</option>
                </select>
                <span className="hint">
                  plafon micro 2026: 100.000 EUR (OUG 89/2025) — la depășire se trece la impozit pe
                  profit din trimestrul depășirii
                </span>
              </div>
            </div>
          </div>
        </div>
      </form>

      {update.isError && (
        <div className="banner danger">
          <Ic name="xMark" />
          <span>{formatError(update.error, "Eroare la salvarea companiei.")}</span>
        </div>
      )}

      {/* bottom actions (mirror of head-actions, for long-form ergonomics) */}
      <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
        <button className="pill-btn" onClick={() => void navigate({ to: "/companies" })}>
          Renunță
        </button>
        <button
          className="btn-dark"
          type="submit"
          form="company-edit-form"
          disabled={update.isPending}
          style={update.isPending ? { opacity: 0.6 } : undefined}
        >
          <Ic name="check" />
          {update.isPending ? "Se salvează…" : "Salvează"}
        </button>
      </div>
    </div>
  );
}

// ─── FormField helper — design .field/.input markup ──────────────────────────

interface FormFieldProps {
  id: keyof FormValues;
  label: string;
  required?: boolean;
  span2?: boolean;
  placeholder?: string;
  type?: string;
  /** Render with the monospaced .num input class. */
  num?: boolean;
  uppercase?: boolean;
  hint?: string;
  register: UseFormRegister<FormValues>;
  errors: FieldErrors<FormValues>;
}

function FormField({
  id,
  label,
  required,
  span2,
  placeholder,
  type,
  num,
  uppercase,
  hint,
  register,
  errors,
}: FormFieldProps) {
  const error = errors[id]?.message as string | undefined;
  return (
    <div className={`field${span2 ? " span2" : ""}`}>
      <label>
        {label}
        {required && <> <span className="req">*</span></>}
      </label>
      <input
        className={`input${num ? " num" : ""}`}
        type={type ?? "text"}
        placeholder={placeholder}
        style={uppercase ? { textTransform: "uppercase" } : undefined}
        {...register(id)}
      />
      {error && <span className="err">{error}</span>}
      {hint && !error && <span className="hint">{hint}</span>}
    </div>
  );
}
