import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import type { AnafCompanyData, AppErrorPayload, CreateCompanyInput } from "@/types";

type Step = 1 | 2 | 3 | 4 | 5 | 6;

/** Step 1 mode: trial or activate */
type LicenseMode = "trial" | "activate";

interface WizardFormState {
  cui: string;
  legalName: string;
  address: string;
  city: string;
  county: string;
  invoiceSeries: string;
  vatPayer: boolean;
  email: string;
  phone: string;
  iban: string;
  bankName: string;
}

const INITIAL_FORM: WizardFormState = {
  cui: "",
  legalName: "",
  address: "",
  city: "",
  county: "",
  invoiceSeries: "RO",
  vatPayer: false,
  email: "",
  phone: "",
  iban: "",
  bankName: "",
};

function StepDots({ current }: { current: Step }) {
  return (
    <div
      style={{
        display: "flex",
        gap: 8,
        justifyContent: "center",
        marginBottom: 28,
      }}
    >
      {([1, 2, 3, 4, 5, 6] as Step[]).map((s) => (
        <div
          key={s}
          style={{
            width: s === current ? 20 : 8,
            height: 8,
            borderRadius: 4,
            background: s === current ? "var(--accent)" : s < current ? "var(--accent-border)" : "var(--border)",
            transition: "all 0.15s ease",
          }}
        />
      ))}
    </div>
  );
}

export function OnboardingWizard() {
  const [step, setStep] = useState<Step>(1);
  const [form, setForm] = useState<WizardFormState>(INITIAL_FORM);
  const [formError, setFormError] = useState<string | null>(null);
  const [createdName, setCreatedName] = useState("");
  const [createdCompanyId, setCreatedCompanyId] = useState<string>("");

  const queryClient = useQueryClient();
  const setActiveCompanyId = useAppStore((s) => s.setActiveCompanyId);

  /** Called from Step2 right after trial/license activation so the validity
   *  cache is fresh when handleFinish eventually invalidates companies. */
  const handleLicenseActivated = () => {
    void queryClient.invalidateQueries({ queryKey: queryKeys.licenseValidity });
  };

  const create = useMutation({
    mutationFn: (input: CreateCompanyInput) => api.companies.create(input),
    onSuccess: (company) => {
      setActiveCompanyId(company.id);
      setCreatedName(company.legalName);
      setCreatedCompanyId(company.id);
      setStep(4);
    },
    onError: (err) => {
      const payload = err as unknown as AppErrorPayload;
      setFormError(payload?.message ?? "Eroare necunoscută.");
    },
  });

  const field = (key: keyof Omit<WizardFormState, "vatPayer">) => ({
    value: form[key] as string,
    onChange: (e: React.ChangeEvent<HTMLInputElement>) =>
      setForm((f) => ({ ...f, [key]: e.target.value })),
  });

  const handleStep3Submit = (e: React.FormEvent) => {
    e.preventDefault();
    setFormError(null);
    if (!form.cui.trim()) { setFormError("CUI este obligatoriu."); return; }
    if (!form.legalName.trim()) { setFormError("Denumirea legală este obligatorie."); return; }
    if (!form.city.trim()) { setFormError("Localitatea este obligatorie."); return; }
    if (!form.county.trim()) { setFormError("Județul este obligatoriu."); return; }
    if (!form.address.trim()) { setFormError("Adresa este obligatorie."); return; }

    create.mutate({
      cui: form.cui.trim(),
      legalName: form.legalName.trim(),
      address: form.address.trim(),
      city: form.city.trim(),
      county: form.county.trim().toUpperCase(),
      invoiceSeries: form.invoiceSeries.trim() || "RO",
      vatPayer: form.vatPayer,
      email: form.email.trim() || undefined,
      phone: form.phone.trim() || undefined,
      iban: form.iban.trim() || undefined,
      bankName: form.bankName.trim() || undefined,
    });
  };

  const handleFinish = async () => {
    await api.settings.set("first_run_completed", "1");
    // Invalidate both companies AND license validity so OnboardingGate re-checks
    // correctly after trial has been started (license was false before trial creation).
    void queryClient.invalidateQueries({ queryKey: queryKeys.companies.list() });
    void queryClient.invalidateQueries({ queryKey: queryKeys.licenseValidity });
  };

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "var(--bg)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 9999,
      }}
    >
      <div
        style={{
          width: 480,
          background: "var(--bg-content)",
          border: "1px solid var(--border-strong)",
          boxShadow: "0 4px 24px rgba(0,0,0,0.12)",
          padding: "32px 36px 28px",
        }}
      >
        <StepDots current={step} />

        {step === 1 && (
          <Step1 onNext={() => setStep(2)} />
        )}

        {step === 2 && (
          <Step2
            onStartTrial={() => { handleLicenseActivated(); setStep(3); }}
            onActivate={() => { handleLicenseActivated(); setStep(3); }}
          />
        )}

        {step === 3 && (
          <Step3
            form={form}
            field={field}
            error={formError}
            isPending={create.isPending}
            onCheckChange={(checked) => setForm((f) => ({ ...f, vatPayer: checked }))}
            onSubmit={handleStep3Submit}
            onAnafFill={(data) =>
              setForm((f) => ({
                ...f,
                legalName: data.legalName,
                address: data.address,
                city: data.city,
                county: data.county.slice(0, 2).toUpperCase(),
                vatPayer: data.vatPayer,
              }))
            }
          />
        )}

        {step === 4 && (
          <Step4Company
            companyName={createdName}
            onContinue={() => setStep(5)}
            onSkip={() => setStep(6)}
          />
        )}

        {step === 5 && (
          <Step5Anaf
            companyId={createdCompanyId}
            companyName={createdName}
            onNext={() => setStep(6)}
          />
        )}

        {step === 6 && (
          <Step6Summary companyName={createdName} onFinish={handleFinish} />
        )}
      </div>
    </div>
  );
}

function Step1({ onNext }: { onNext: () => void }) {
  return (
    <div style={{ textAlign: "center" }}>
      <div
        style={{
          display: "inline-flex",
          alignItems: "center",
          justifyContent: "center",
          width: 56,
          height: 56,
          background: "var(--accent)",
          color: "var(--on-accent)",
          fontSize: 22,
          fontWeight: 700,
          fontFamily: "var(--font-mono)",
          marginBottom: 20,
          letterSpacing: "-1px",
        }}
      >
        eF
      </div>
      <h1
        style={{
          fontSize: 22,
          fontWeight: 700,
          margin: "0 0 6px",
          fontFamily: "var(--font-ui)",
          letterSpacing: "-0.3px",
        }}
      >
        RoFactura
      </h1>
      <p
        style={{
          fontSize: 13,
          color: "var(--text-muted)",
          margin: "0 0 32px",
          fontFamily: "var(--font-ui)",
        }}
      >
        Facturare electronică CIUS-RO
      </p>
      <p
        style={{
          fontSize: 12,
          color: "var(--text-muted)",
          margin: "0 0 28px",
          lineHeight: 1.6,
        }}
      >
        Bine ați venit! Activați o licență sau porniți perioada de probă gratuită
        de 14 zile pentru a începe.
      </p>
      <button
        type="button"
        className="btn primary"
        style={{ width: "100%", justifyContent: "center", height: 34, fontSize: 12 }}
        onClick={onNext}
      >
        Începe configurarea →
      </button>
    </div>
  );
}

interface Step2Props {
  onStartTrial: () => void;
  onActivate: () => void;
}

function Step2({ onStartTrial, onActivate }: Step2Props) {
  const [mode, setMode] = useState<LicenseMode | null>(null);
  const [email, setEmail] = useState("");
  const [licenseKey, setLicenseKey] = useState("");
  const [licenseEmail, setLicenseEmail] = useState("");
  const [error, setError] = useState<string | null>(null);

  // If a license already exists in DB (e.g. app crashed after trial start but
  // before company creation), skip this step automatically.
  const { data: existingLicense, isLoading: licenseCheckLoading } = useQuery({
    queryKey: queryKeys.licenseExisting,
    queryFn: () => api.license.get(),
    staleTime: 0,
  });

  // Auto-advance past Step 2 when an active license already exists
  if (!licenseCheckLoading && existingLicense) {
    return (
      <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
        <h2 style={{ fontSize: 15, fontWeight: 700, margin: "0 0 4px", fontFamily: "var(--font-ui)" }}>
          Licență
        </h2>
        <div style={{ padding: "10px 12px", background: "#D1FAE5", border: "1px solid #A7F3D0", fontSize: 12, color: "#065F46", display: "flex", alignItems: "center", gap: 8 }}>
          <span>✓</span>
          <span>
            Licență activă: <strong>{existingLicense.tier}</strong>
            {existingLicense.email ? ` — ${existingLicense.email}` : ""}
          </span>
        </div>
        <button
          type="button"
          className="btn primary"
          style={{ width: "100%", justifyContent: "center", height: 34, fontSize: 12 }}
          onClick={onStartTrial}
        >
          Continuă →
        </button>
      </div>
    );
  }

  const trialMutation = useMutation({
    mutationFn: (trialEmail: string) => api.license.startTrial(trialEmail),
    onSuccess: () => { onStartTrial(); },
    onError: (err) => {
      const payload = err as unknown as AppErrorPayload;
      setError(payload?.message ?? "Eroare la activarea perioadei de probă.");
    },
  });

  const activateMutation = useMutation({
    mutationFn: ({ key, actEmail }: { key: string; actEmail: string }) =>
      api.license.activate(key, actEmail),
    onSuccess: () => { onActivate(); },
    onError: (err) => {
      const payload = err as unknown as AppErrorPayload;
      setError(payload?.message ?? "Licența nu a putut fi activată.");
    },
  });

  const isPending = trialMutation.isPending || activateMutation.isPending;

  return (
    <div>
      <h2
        style={{
          fontSize: 15,
          fontWeight: 700,
          margin: "0 0 8px",
          fontFamily: "var(--font-ui)",
        }}
      >
        Licență
      </h2>
      <p style={{ fontSize: 12, color: "var(--text-muted)", margin: "0 0 20px" }}>
        Alegeți o opțiune pentru a continua:
      </p>

      {!mode && (
        <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
          <button
            type="button"
            className="btn primary"
            style={{ width: "100%", justifyContent: "center", height: 38, fontSize: 12 }}
            onClick={() => setMode("trial")}
          >
            Perioadă de probă gratuită — 14 zile
          </button>
          <button
            type="button"
            className="btn"
            style={{ width: "100%", justifyContent: "center", height: 38, fontSize: 12 }}
            onClick={() => setMode("activate")}
          >
            Am deja o licență — Activează
          </button>
        </div>
      )}

      {mode === "trial" && (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            setError(null);
            if (!email.trim()) { setError("Adresa de email este obligatorie."); return; }
            trialMutation.mutate(email.trim());
          }}
          style={{ display: "flex", flexDirection: "column", gap: 10 }}
        >
          <WField label="Adresă email *" id="trial-email">
            <input
              id="trial-email"
              className="field"
              type="email"
              placeholder="office@firma.ro"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
            />
          </WField>
          {error && (
            <div style={{ padding: "7px 10px", background: "#FEE2E2", border: "1px solid #FECACA", fontSize: 11, color: "#991B1B" }}>
              {error}
            </div>
          )}
          <button
            type="submit"
            disabled={isPending}
            className="btn primary"
            style={{ width: "100%", justifyContent: "center", height: 34, fontSize: 12 }}
          >
            {isPending ? "Se activează…" : "Pornește perioada de probă →"}
          </button>
          <button
            type="button"
            className="btn"
            style={{ width: "100%", justifyContent: "center", height: 30, fontSize: 11 }}
            onClick={() => { setMode(null); setError(null); }}
          >
            ← Înapoi
          </button>
        </form>
      )}

      {mode === "activate" && (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            setError(null);
            if (!licenseKey.trim()) { setError("Cheia de licență este obligatorie."); return; }
            if (!licenseEmail.trim()) { setError("Adresa de email este obligatorie."); return; }
            activateMutation.mutate({ key: licenseKey.trim(), actEmail: licenseEmail.trim() });
          }}
          style={{ display: "flex", flexDirection: "column", gap: 10 }}
        >
          <WField label="Cheie licență *" id="license-key">
            <input
              id="license-key"
              className="field"
              placeholder="XXXX-XXXX-XXXX-XXXX"
              style={{ fontFamily: "var(--font-mono)", textTransform: "uppercase" }}
              value={licenseKey}
              onChange={(e) => setLicenseKey(e.target.value)}
            />
          </WField>
          <WField label="Email achiziție *" id="license-email">
            <input
              id="license-email"
              className="field"
              type="email"
              placeholder="office@firma.ro"
              value={licenseEmail}
              onChange={(e) => setLicenseEmail(e.target.value)}
            />
          </WField>
          {error && (
            <div style={{ padding: "7px 10px", background: "#FEE2E2", border: "1px solid #FECACA", fontSize: 11, color: "#991B1B" }}>
              {error}
            </div>
          )}
          <button
            type="submit"
            disabled={isPending}
            className="btn primary"
            style={{ width: "100%", justifyContent: "center", height: 34, fontSize: 12 }}
          >
            {isPending ? "Se activează…" : "Activează licența →"}
          </button>
          <button
            type="button"
            className="btn"
            style={{ width: "100%", justifyContent: "center", height: 30, fontSize: 11 }}
            onClick={() => { setMode(null); setError(null); }}
          >
            ← Înapoi
          </button>
        </form>
      )}
    </div>
  );
}

interface Step3Props {
  form: WizardFormState;
  field: (key: keyof Omit<WizardFormState, "vatPayer">) => {
    value: string;
    onChange: (e: React.ChangeEvent<HTMLInputElement>) => void;
  };
  error: string | null;
  isPending: boolean;
  onCheckChange: (checked: boolean) => void;
  onSubmit: (e: React.FormEvent) => void;
  onAnafFill: (data: AnafCompanyData) => void;
}

function Step3({ form, field, error, isPending, onCheckChange, onSubmit, onAnafFill }: Step3Props) {
  const [cuiLookupLoading, setCuiLookupLoading] = useState(false);
  const [cuiLookupError, setCuiLookupError] = useState<string | null>(null);

  const handleCuiLookup = async () => {
    if (!form.cui.trim()) return;
    setCuiLookupLoading(true);
    setCuiLookupError(null);
    try {
      const data = await api.companies.fetchAnafData(form.cui.trim());
      onAnafFill(data);
    } catch {
      setCuiLookupError("CUI-ul nu a fost găsit în baza ANAF.");
    } finally {
      setCuiLookupLoading(false);
    }
  };

  return (
    <form onSubmit={onSubmit}>
      <h2
        style={{
          fontSize: 15,
          fontWeight: 700,
          margin: "0 0 20px",
          fontFamily: "var(--font-ui)",
        }}
      >
        Date companie
      </h2>

      <div style={{ display: "flex", flexDirection: "column", gap: 10, marginBottom: 16 }}>
        <WField label="CUI *" id="w-cui">
          <input
            id="w-cui"
            className="field"
            placeholder="ex. RO12345678"
            style={{ fontFamily: "var(--font-mono)", fontSize: 12 }}
            {...field("cui")}
          />
          <button
            type="button"
            className="btn"
            disabled={cuiLookupLoading}
            onClick={() => { void handleCuiLookup(); }}
            style={{ marginTop: 4, fontSize: 11 }}
          >
            {cuiLookupLoading ? "Se caută…" : "Caută în ANAF ↗"}
          </button>
          {cuiLookupError && (
            <span style={{ fontSize: 11, color: "#DC2626", marginTop: 2 }}>
              {cuiLookupError}
            </span>
          )}
        </WField>

        <WField label="Denumire legală *" id="w-legalName">
          <input
            id="w-legalName"
            className="field"
            placeholder="S.C. Exemplu S.R.L."
            {...field("legalName")}
          />
        </WField>

        <div style={{ display: "flex", gap: 10 }}>
          <WField label="Localitate *" id="w-city" style={{ flex: 2 }}>
            <input
              id="w-city"
              className="field"
              placeholder="Cluj-Napoca"
              {...field("city")}
            />
          </WField>
          <WField label="Județ *" id="w-county" style={{ flex: 1 }}>
            <input
              id="w-county"
              className="field"
              placeholder="CJ"
              maxLength={2}
              style={{ fontFamily: "var(--font-mono)", textTransform: "uppercase" }}
              {...field("county")}
            />
          </WField>
        </div>

        <WField label="Adresă *" id="w-address">
          <input
            id="w-address"
            className="field"
            placeholder="Str. Exemplu nr. 1"
            {...field("address")}
          />
        </WField>

        <WField label="Serie factură" id="w-series">
          <input
            id="w-series"
            className="field"
            placeholder="RO"
            style={{ fontFamily: "var(--font-mono)", textTransform: "uppercase" }}
            {...field("invoiceSeries")}
          />
        </WField>

        <div
          style={{
            display: "flex",
            alignItems: "center",
            gap: 8,
            padding: "6px 0 2px",
          }}
        >
          <input
            id="w-vatPayer"
            type="checkbox"
            className="cbx"
            checked={form.vatPayer}
            onChange={(e) => onCheckChange(e.target.checked)}
          />
          <label
            htmlFor="w-vatPayer"
            style={{ fontSize: 12, cursor: "pointer", userSelect: "none" }}
          >
            Plătitor de TVA
          </label>
        </div>

        <details style={{ marginTop: 4 }}>
          <summary
            style={{
              fontSize: 11,
              color: "var(--text-muted)",
              cursor: "pointer",
              userSelect: "none",
              listStyle: "none",
              display: "flex",
              alignItems: "center",
              gap: 4,
            }}
          >
            ▸ Date opționale (email, telefon, IBAN, bancă)
          </summary>
          <div style={{ display: "flex", flexDirection: "column", gap: 10, marginTop: 10 }}>
            <WField label="Email" id="w-email">
              <input
                id="w-email"
                className="field"
                type="email"
                placeholder="office@firma.ro"
                {...field("email")}
              />
            </WField>
            <WField label="Telefon" id="w-phone">
              <input
                id="w-phone"
                className="field"
                placeholder="+40 722 000 000"
                {...field("phone")}
              />
            </WField>
            <WField label="IBAN" id="w-iban">
              <input
                id="w-iban"
                className="field"
                placeholder="RO49AAAA..."
                style={{ fontFamily: "var(--font-mono)" }}
                {...field("iban")}
              />
            </WField>
            <WField label="Bancă" id="w-bank">
              <input
                id="w-bank"
                className="field"
                placeholder="Banca Transilvania"
                {...field("bankName")}
              />
            </WField>
          </div>
        </details>
      </div>

      {error && (
        <div
          style={{
            padding: "7px 10px",
            background: "#FEE2E2",
            border: "1px solid #FECACA",
            fontSize: 11,
            color: "#991B1B",
            marginBottom: 14,
          }}
        >
          {error}
        </div>
      )}

      <button
        type="submit"
        disabled={isPending}
        className="btn primary"
        style={{ width: "100%", justifyContent: "center", height: 34, fontSize: 12 }}
      >
        {isPending ? "Se salvează…" : "Salvează și continuă →"}
      </button>
    </form>
  );
}

/** Step 4 — Company created; ask user if they want to set up ANAF SPV now */
function Step4Company({
  companyName,
  onContinue,
  onSkip,
}: {
  companyName: string;
  onContinue: () => void;
  onSkip: () => void;
}) {
  return (
    <div style={{ textAlign: "center" }}>
      <div
        style={{
          display: "inline-flex",
          alignItems: "center",
          justifyContent: "center",
          width: 48,
          height: 48,
          background: "#D1FAE5",
          color: "#065F46",
          fontSize: 22,
          marginBottom: 20,
        }}
      >
        ✓
      </div>
      <h2
        style={{
          fontSize: 17,
          fontWeight: 700,
          margin: "0 0 8px",
          fontFamily: "var(--font-ui)",
        }}
      >
        Companie creată
      </h2>
      <p style={{ fontSize: 12, color: "var(--text-muted)", margin: "0 0 4px" }}>
        A fost adăugată cu succes:
      </p>
      <p style={{ fontSize: 13, fontWeight: 600, margin: "0 0 20px", color: "var(--text)" }}>
        {companyName}
      </p>
      <p style={{ fontSize: 12, color: "var(--text-muted)", margin: "0 0 20px", lineHeight: 1.6 }}>
        Configurați acum autentificarea <strong>SPV ANAF</strong> pentru a putea
        transmite facturi electronic. Puteți face asta și mai târziu din Setări.
      </p>
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        <button
          type="button"
          className="btn primary"
          style={{ width: "100%", justifyContent: "center", height: 36, fontSize: 12 }}
          onClick={onContinue}
        >
          Configurează SPV ANAF →
        </button>
        <button
          type="button"
          className="btn"
          style={{ width: "100%", justifyContent: "center", height: 30, fontSize: 11 }}
          onClick={onSkip}
        >
          Mai târziu — sari →
        </button>
      </div>
    </div>
  );
}

/** Step 5 — ANAF SPV OAuth authorize */
function Step5Anaf({
  companyId,
  companyName,
  onNext,
}: {
  companyId: string;
  companyName: string;
  onNext: () => void;
}) {
  const [isAuthenticating, setIsAuthenticating] = useState(false);
  const [authError, setAuthError] = useState<string | null>(null);
  const [isAuthenticated, setIsAuthenticated] = useState(false);

  const handleAuthorize = async () => {
    setIsAuthenticating(true);
    setAuthError(null);
    try {
      await api.anaf.authorize(companyId);
      const authed = await api.anaf.isAuthenticated(companyId);
      setIsAuthenticated(authed);
      if (authed) {
        setTimeout(onNext, 1200);
      } else {
        setAuthError("Autorizarea nu s-a finalizat. Încercați din nou sau sari peste.");
      }
    } catch (e) {
      const err = e as unknown as AppErrorPayload;
      setAuthError(err?.message ?? "Autorizarea a eșuat. Verificați conexiunea și reîncercați.");
    } finally {
      setIsAuthenticating(false);
    }
  };

  return (
    <div>
      <h2
        style={{
          fontSize: 15,
          fontWeight: 700,
          margin: "0 0 8px",
          fontFamily: "var(--font-ui)",
        }}
      >
        Conectare SPV ANAF
      </h2>
      <p style={{ fontSize: 12, color: "var(--text-muted)", margin: "0 0 20px", lineHeight: 1.6 }}>
        Autorizați <strong>{companyName}</strong> pentru autentificare electronică.
        Veți fi redirecționat în browser pentru a vă loga cu certificatul digital.
      </p>

      {isAuthenticated ? (
        <div
          style={{
            padding: "10px 12px",
            background: "#D1FAE5",
            border: "1px solid #A7F3D0",
            fontSize: 12,
            color: "#065F46",
            marginBottom: 16,
            display: "flex",
            alignItems: "center",
            gap: 8,
          }}
        >
          <span>✓</span>
          <span>Autentificare SPV reușită! Se continuă…</span>
        </div>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
          {authError && (
            <div
              style={{
                padding: "7px 10px",
                background: "#FEE2E2",
                border: "1px solid #FECACA",
                fontSize: 11,
                color: "#991B1B",
              }}
            >
              {authError}
            </div>
          )}
          <button
            type="button"
            className="btn primary"
            disabled={isAuthenticating}
            style={{ width: "100%", justifyContent: "center", height: 36, fontSize: 12 }}
            onClick={() => { void handleAuthorize(); }}
          >
            {isAuthenticating ? "Se autorizează…" : "Autorizează în SPV ANAF →"}
          </button>
          <button
            type="button"
            className="btn"
            disabled={isAuthenticating}
            style={{ width: "100%", justifyContent: "center", height: 30, fontSize: 11 }}
            onClick={onNext}
          >
            Mai târziu — sari →
          </button>
        </div>
      )}
    </div>
  );
}

/** Step 6 — Final summary + finish */
function Step6Summary({
  companyName,
  onFinish,
}: {
  companyName: string;
  onFinish: () => Promise<void>;
}) {
  const [finishing, setFinishing] = useState(false);

  const handleClick = async () => {
    setFinishing(true);
    await onFinish();
  };

  return (
    <div style={{ textAlign: "center" }}>
      <div
        style={{
          display: "inline-flex",
          alignItems: "center",
          justifyContent: "center",
          width: 52,
          height: 52,
          background: "var(--accent)",
          color: "var(--on-accent)",
          fontSize: 22,
          fontWeight: 700,
          fontFamily: "var(--font-mono)",
          marginBottom: 20,
          letterSpacing: "-1px",
        }}
      >
        eF
      </div>
      <h2
        style={{
          fontSize: 17,
          fontWeight: 700,
          margin: "0 0 8px",
          fontFamily: "var(--font-ui)",
        }}
      >
        Totul configurat!
      </h2>
      <p style={{ fontSize: 12, color: "var(--text-muted)", margin: "0 0 16px", lineHeight: 1.6 }}>
        Sunteți gata să emiteți facturi electronice CIUS-RO.
      </p>
      <div
        style={{
          padding: "10px 14px",
          background: "var(--bg)",
          border: "1px solid var(--border-soft)",
          fontSize: 11.5,
          textAlign: "left",
          marginBottom: 20,
          display: "flex",
          flexDirection: "column",
          gap: 6,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span style={{ color: "#16A34A" }}>✓</span>
          <span>Licență activată</span>
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span style={{ color: "#16A34A" }}>✓</span>
          <span>Companie: <strong>{companyName}</strong></span>
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span style={{ color: "var(--text-muted)" }}>○</span>
          <span style={{ color: "var(--text-muted)" }}>
            SPV ANAF — configurabil din Setări
          </span>
        </div>
      </div>
      <button
        type="button"
        className="btn primary"
        disabled={finishing}
        style={{ width: "100%", justifyContent: "center", height: 34, fontSize: 12 }}
        onClick={() => { void handleClick(); }}
      >
        {finishing ? "Se finalizează…" : "Deschide RoFactura →"}
      </button>
    </div>
  );
}

function WField({
  label,
  id,
  children,
  style,
}: {
  label: string;
  id: string;
  children: React.ReactNode;
  style?: React.CSSProperties;
}) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 3, ...style }}>
      <label
        htmlFor={id}
        style={{ fontSize: 11, fontWeight: 600, color: "var(--text-muted)" }}
      >
        {label}
      </label>
      {children}
    </div>
  );
}
