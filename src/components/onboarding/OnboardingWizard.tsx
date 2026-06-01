/**
 * Wizard de configurare inițială — Wave 6 re-skin.
 *
 * Visual: prototype rail stâng (indigo) + card alb la dreapta (5 pași vizibili).
 * Wiring: 100% preserved — aceleași step-uri (1–6), aceleași api.* calls,
 * aceeași logică (company create → ANAF authorize → license start/activate → finish).
 *
 * Step mapping (intern):
 *  1 = Bun venit
 *  2 = Licență (trial / activate)
 *  3 = Date companie (create)
 *  4 = Companie creată (bridge → SPV)
 *  5 = ANAF / SPV authorize
 *  6 = Gata (summary + finish)
 *
 * Rail shows the 5 visual steps: Bun venit / Companie / ANAF/SPV / Licență / Gata
 * mapped to internal steps for progress indication.
 */

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { formatError } from "@/lib/error-mapper";
import { Icon } from "@/components/shared/Icon";
import { BrandMark } from "@/components/shared/BrandMark";
import { Btn, Banner } from "@/components/rf";
import type { AnafCompanyData, AppErrorPayload, CreateCompanyInput } from "@/types";

// ─── Types ────────────────────────────────────────────────────────────────────

type Step = 1 | 2 | 3 | 4 | 5 | 6;
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

// ─── Rail step definitions ─────────────────────────────────────────────────────

const RAIL_STEPS = [
  { key: "welcome",  label: "Bun venit" },
  { key: "company",  label: "Companie" },
  { key: "anaf",     label: "ANAF / SPV" },
  { key: "license",  label: "Licență" },
  { key: "done",     label: "Gata" },
] as const;

/** Map internal step → rail index (0-based) for progress indicator */
function stepToRailIndex(step: Step): number {
  if (step === 1) return 0;
  if (step === 2) return 3; // License step
  if (step === 3) return 1; // Company form
  if (step === 4) return 1; // Company created (still on Company rail)
  if (step === 5) return 2; // ANAF
  return 4;                  // Done
}

// ─── Rail ─────────────────────────────────────────────────────────────────────

function Rail({ currentStep }: { currentStep: Step }) {
  const railIdx = stepToRailIndex(currentStep);
  return (
    <div className="onb-rail">
      {/* Brand */}
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <BrandMark size={32} />
        <span style={{ color: "#fff", fontWeight: 700, fontSize: 15, letterSpacing: "-0.01em" }}>
          Clarito
        </span>
      </div>

      {/* Steps */}
      <div style={{ marginTop: 48, display: "flex", flexDirection: "column", gap: 4 }}>
        {RAIL_STEPS.map((s, i) => {
          const done = i < railIdx;
          const active = i === railIdx;
          return (
            <div
              key={s.key}
              className="onb-step"
              style={{ opacity: active ? 1 : done ? 0.85 : 0.5 }}
            >
              <span
                className="onb-step-dot"
                style={{
                  background: done
                    ? "#fff"
                    : active
                    ? "rgba(255,255,255,0.2)"
                    : "transparent",
                  borderColor: done || active ? "#fff" : "rgba(255,255,255,0.35)",
                  color: done ? "var(--rf-accent)" : "#fff",
                }}
              >
                {done ? <Icon name="check" size={12} stroke={3} /> : i + 1}
              </span>
              <span style={{ fontWeight: active ? 700 : 500, fontSize: 13.5 }}>{s.label}</span>
            </div>
          );
        })}
      </div>

      {/* Footer note */}
      <div className="onb-rail-foot">
        <Icon name="shield" size={15} />
        <span>Datele dvs. sunt stocate local și criptate. Conexiunea ANAF folosește OAuth securizat.</span>
      </div>
    </div>
  );
}

// ─── StepTitle helper ─────────────────────────────────────────────────────────

function StepTitle({ icon, title, sub }: { icon: string; title: string; sub: string }) {
  return (
    <div style={{ display: "flex", gap: 13, alignItems: "flex-start", marginBottom: 22 }}>
      <span
        style={{
          width: 42,
          height: 42,
          borderRadius: 11,
          background: "var(--rf-accent-tint)",
          color: "var(--rf-accent)",
          display: "grid",
          placeItems: "center",
          flexShrink: 0,
        }}
      >
        <Icon name={icon} size={20} />
      </span>
      <div>
        <h1 style={{ fontSize: 21, fontWeight: 650, letterSpacing: "-0.01em", margin: 0 }}>{title}</h1>
        <p className="rf-text-muted" style={{ fontSize: 13.5, margin: "4px 0 0", lineHeight: 1.5 }}>{sub}</p>
      </div>
    </div>
  );
}

// ─── WField — wizard form field ────────────────────────────────────────────────

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
        style={{ fontSize: 11.5, fontWeight: 600, color: "var(--rf-text-muted)" }}
      >
        {label}
      </label>
      {children}
    </div>
  );
}

// ─── Step components ──────────────────────────────────────────────────────────

function Step1() {
  return (
    <div>
      <div style={{ marginBottom: 20 }}>
        <BrandMark size={64} />
      </div>
      <h1 style={{ fontSize: 28, fontWeight: 700, letterSpacing: "-0.02em", margin: "0 0 8px" }}>
        Bun venit în Clarito
      </h1>
      <p className="rf-text-muted" style={{ fontSize: 15, lineHeight: 1.6, margin: "0 0 28px" }}>
        Soluția completă de e-Factura și contabilitate pentru firmele din România. Hai să configurăm aplicația în câțiva pași.
      </p>
      <div style={{ display: "grid", gap: 12, marginBottom: 28 }}>
        {([
          ["fileOut", "Facturare electronică", "Emite și trimite facturi la ANAF în câteva secunde."],
          ["download", "Sincronizare SPV", "Descarcă automat facturile primite din Spațiul Privat Virtual."],
          ["chart", "Raportare fiscală", "D300, D394, SAF-T și jurnale, generate automat."],
        ] as const).map(([ic, t, d]) => (
          <div key={t} style={{ display: "flex", gap: 13, alignItems: "flex-start" }}>
            <span
              style={{
                width: 38,
                height: 38,
                borderRadius: 10,
                background: "var(--rf-accent-tint)",
                color: "var(--rf-accent)",
                display: "grid",
                placeItems: "center",
                flexShrink: 0,
              }}
            >
              <Icon name={ic} size={18} />
            </span>
            <div>
              <div style={{ fontWeight: 600, fontSize: 13.5 }}>{t}</div>
              <div className="rf-text-muted" style={{ fontSize: 12.5 }}>{d}</div>
            </div>
          </div>
        ))}
      </div>
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

  // Auto-skip if a license already exists
  const { data: existingLicense, isLoading: licenseCheckLoading } = useQuery({
    queryKey: queryKeys.licenseExisting,
    queryFn: () => api.license.get(),
    staleTime: 0,
  });

  const trialMutation = useMutation({
    mutationFn: (trialEmail: string) => api.license.startTrial(trialEmail),
    onSuccess: () => onStartTrial(),
    onError: (err) => {
      const payload = err as unknown as AppErrorPayload;
      setError(payload?.message ?? "Eroare la activarea perioadei de probă.");
    },
  });

  const activateMutation = useMutation({
    mutationFn: ({ key, actEmail }: { key: string; actEmail: string }) =>
      api.license.activate(key, actEmail),
    onSuccess: () => onActivate(),
    onError: (err) => {
      const payload = err as unknown as AppErrorPayload;
      setError(payload?.message ?? "Licența nu a putut fi activată.");
    },
  });

  const isPending = trialMutation.isPending || activateMutation.isPending;

  if (!licenseCheckLoading && existingLicense) {
    return (
      <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
        <StepTitle icon="shield" title="Licență" sub="Licența dvs. este activă." />
        <Banner variant="success">
          Licență activă: <strong>{existingLicense.tier}</strong>
          {existingLicense.email ? ` — ${existingLicense.email}` : ""}
        </Banner>
        <Btn variant="primary" iconRight="chevRight" block onClick={onStartTrial}>
          Continuă
        </Btn>
      </div>
    );
  }

  return (
    <div>
      <StepTitle icon="shield" title="Activare licență" sub="Începeți cu o perioadă de probă gratuită sau activați o licență existentă." />

      {!mode && (
        <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 14 }}>
          {/* Trial card */}
          <div
            style={{
              border: "2px solid var(--rf-accent)",
              borderRadius: 12,
              padding: 18,
              background: "var(--rf-accent-tint)",
              cursor: "pointer",
            }}
            role="button"
            onClick={() => setMode("trial")}
          >
            <span
              style={{
                display: "inline-block",
                padding: "2px 8px",
                borderRadius: 6,
                background: "var(--rf-accent)",
                color: "#fff",
                fontSize: 11,
                fontWeight: 600,
                marginBottom: 8,
              }}
            >
              Recomandat
            </span>
            <div style={{ fontSize: 17, fontWeight: 700 }}>Probă gratuită</div>
            <div className="rf-text-muted" style={{ fontSize: 12.5, marginTop: 2 }}>14 zile, toate funcțiile Pro</div>
            <div className="rf-mono" style={{ fontSize: 26, fontWeight: 700, margin: "12px 0 2px" }}>
              0 <span style={{ fontSize: 13, color: "var(--rf-text-muted)" }}>RON</span>
            </div>
            <div className="rf-text-muted" style={{ fontSize: 12 }}>fără card de credit</div>
          </div>

          {/* Activate card */}
          <div
            style={{
              border: "1px solid var(--rf-border)",
              borderRadius: 12,
              padding: 18,
              cursor: "pointer",
            }}
            role="button"
            onClick={() => setMode("activate")}
          >
            <div style={{ fontSize: 17, fontWeight: 700 }}>Am o licență</div>
            <div className="rf-text-muted" style={{ fontSize: 12.5, marginTop: 2 }}>Activați cu cheia primită</div>
            <div className="rf-mono" style={{ fontSize: 22, fontWeight: 700, margin: "12px 0 2px" }}>
              →
            </div>
            <div className="rf-text-muted" style={{ fontSize: 12 }}>introduceți cheia</div>
          </div>
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
          style={{ display: "flex", flexDirection: "column", gap: 12 }}
        >
          <WField label="Adresă email *" id="trial-email">
            <input
              id="trial-email"
              className="rf-input"
              type="email"
              placeholder="office@firma.ro"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
            />
          </WField>
          {error && <Banner variant="error">{error}</Banner>}
          <Btn type="submit" variant="primary" disabled={isPending} block>
            {isPending ? "Se activează…" : "Pornește perioada de probă →"}
          </Btn>
          <Btn type="button" variant="ghost" block onClick={() => { setMode(null); setError(null); }}>
            ← Înapoi
          </Btn>
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
          style={{ display: "flex", flexDirection: "column", gap: 12 }}
        >
          <WField label="Cheie licență *" id="license-key">
            <input
              id="license-key"
              className="rf-input rf-mono"
              placeholder="XXXX-XXXX-XXXX-XXXX"
              style={{ textTransform: "uppercase", letterSpacing: "0.05em" }}
              value={licenseKey}
              onChange={(e) => setLicenseKey(e.target.value)}
              autoComplete="off"
              spellCheck={false}
            />
          </WField>
          <WField label="Email achiziție *" id="license-email">
            <input
              id="license-email"
              className="rf-input"
              type="email"
              placeholder="office@firma.ro"
              value={licenseEmail}
              onChange={(e) => setLicenseEmail(e.target.value)}
            />
          </WField>
          {error && <Banner variant="error">{error}</Banner>}
          <Btn type="submit" variant="primary" disabled={isPending} block>
            {isPending ? "Se activează…" : "Activează licența →"}
          </Btn>
          <Btn type="button" variant="ghost" block onClick={() => { setMode(null); setError(null); }}>
            ← Înapoi
          </Btn>
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
      <StepTitle icon="building" title="Datele companiei" sub="Completați datele firmei pentru care veți emite facturi." />

      <div style={{ display: "flex", flexDirection: "column", gap: 10, marginBottom: 16 }}>
        <WField label="CUI *" id="w-cui">
          <input
            id="w-cui"
            className="rf-input rf-mono"
            placeholder="ex. RO12345678"
            {...field("cui")}
          />
          <div style={{ display: "flex", gap: 8, marginTop: 4, alignItems: "center" }}>
            <Btn
              type="button"
              variant="ghost"
              size="sm"
              disabled={cuiLookupLoading}
              onClick={() => { void handleCuiLookup(); }}
            >
              {cuiLookupLoading ? "Se caută…" : "Caută în ANAF ↗"}
            </Btn>
            {cuiLookupError && (
              <span style={{ fontSize: 12, color: "var(--rf-error)" }}>{cuiLookupError}</span>
            )}
          </div>
        </WField>

        <WField label="Denumire legală *" id="w-legalName">
          <input id="w-legalName" className="rf-input" placeholder="S.C. Exemplu S.R.L." {...field("legalName")} />
        </WField>

        <div style={{ display: "flex", gap: 10 }}>
          <WField label="Localitate *" id="w-city" style={{ flex: 2 }}>
            <input id="w-city" className="rf-input" placeholder="Cluj-Napoca" {...field("city")} />
          </WField>
          <WField label="Județ *" id="w-county" style={{ flex: 1 }}>
            <input
              id="w-county"
              className="rf-input rf-mono"
              placeholder="CJ"
              maxLength={2}
              style={{ textTransform: "uppercase" }}
              {...field("county")}
            />
          </WField>
        </div>

        <WField label="Adresă *" id="w-address">
          <input id="w-address" className="rf-input" placeholder="Str. Exemplu nr. 1" {...field("address")} />
        </WField>

        <WField label="Serie factură" id="w-series">
          <input
            id="w-series"
            className="rf-input rf-mono"
            placeholder="RO"
            style={{ textTransform: "uppercase" }}
            {...field("invoiceSeries")}
          />
        </WField>

        <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
          <input
            type="checkbox"
            className="rf-cbx"
            checked={form.vatPayer}
            onChange={(e) => onCheckChange(e.target.checked)}
          />
          <span style={{ fontSize: 13 }}>Plătitor de TVA</span>
        </label>

        <details style={{ marginTop: 4 }}>
          <summary
            style={{
              fontSize: 12,
              color: "var(--rf-text-muted)",
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
              <input id="w-email" className="rf-input" type="email" placeholder="office@firma.ro" {...field("email")} />
            </WField>
            <WField label="Telefon" id="w-phone">
              <input id="w-phone" className="rf-input" placeholder="+40 722 000 000" {...field("phone")} />
            </WField>
            <WField label="IBAN" id="w-iban">
              <input id="w-iban" className="rf-input rf-mono" placeholder="RO49AAAA..." {...field("iban")} />
            </WField>
            <WField label="Bancă" id="w-bank">
              <input id="w-bank" className="rf-input" placeholder="Banca Transilvania" {...field("bankName")} />
            </WField>
          </div>
        </details>
      </div>

      {error && <Banner variant="error" className="mb-3">{error}</Banner>}

      <Btn type="submit" variant="primary" disabled={isPending} block>
        {isPending ? "Se salvează…" : "Salvează și continuă →"}
      </Btn>
    </form>
  );
}

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
          width: 72,
          height: 72,
          borderRadius: "50%",
          background: "var(--rf-success-bg)",
          color: "var(--rf-success)",
          display: "grid",
          placeItems: "center",
          margin: "0 auto 20px",
        }}
      >
        <Icon name="check" size={36} stroke={2.4} />
      </div>
      <h2 style={{ fontSize: 21, fontWeight: 700, margin: "0 0 8px", letterSpacing: "-0.01em" }}>
        Companie creată
      </h2>
      <p className="rf-text-muted" style={{ fontSize: 13, margin: "0 0 4px" }}>
        A fost adăugată cu succes:
      </p>
      <p style={{ fontSize: 15, fontWeight: 600, margin: "0 0 20px" }}>{companyName}</p>
      <p className="rf-text-muted" style={{ fontSize: 13, margin: "0 0 16px", lineHeight: 1.6 }}>
        Configurați acum autentificarea <strong>SPV ANAF</strong> pentru a putea transmite facturi electronic.
        Puteți face asta și mai târziu din Setări.
      </p>

      <div
        style={{
          padding: "10px 14px",
          background: "var(--rf-accent-tint)",
          borderRadius: 10,
          fontSize: 12,
          color: "var(--rf-text-muted)",
          lineHeight: 1.6,
          marginBottom: 20,
          textAlign: "left",
          display: "flex",
          flexDirection: "column",
          gap: 6,
        }}
      >
        <div><strong style={{ color: "var(--rf-text)" }}>e-Factura</strong> — sistemul ANAF de facturare electronică obligatoriu pentru B2B și B2G în România (CIUS-RO / UBL 2.1).</div>
        <div><strong style={{ color: "var(--rf-text)" }}>SPV</strong> (Spațiul Privat Virtual) — portalul ANAF prin care se transmit și se recepționează facturile electronice.</div>
        <div><strong style={{ color: "var(--rf-text)" }}>Certificat digital</strong> — pentru autorizare este necesar un token/certificat calificat emis de o autoritate de certificare.</div>
      </div>

      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        <Btn variant="primary" iconRight="chevRight" block onClick={onContinue}>
          Configurează SPV ANAF →
        </Btn>
        <Btn variant="ghost" block onClick={onSkip}>
          Mai târziu — sari →
        </Btn>
      </div>
    </div>
  );
}

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
      setAuthError(formatError(e, "Autorizarea a eșuat. Verificați conexiunea și reîncercați."));
    } finally {
      setIsAuthenticating(false);
    }
  };

  return (
    <div>
      <StepTitle icon="shield" title="Conectare ANAF / SPV" sub="Conectați aplicația la Spațiul Privat Virtual pentru a trimite și primi facturi electronice." />

      <p className="rf-text-muted" style={{ fontSize: 13, lineHeight: 1.6, marginBottom: 14 }}>
        Autorizați <strong>{companyName}</strong> în SPV. Veți fi redirecționat în browser — autentificarea
        se face cu <strong>certificatul digital calificat</strong> (token USB sau soft-cert).
      </p>

      <div
        style={{
          padding: "10px 14px",
          background: "var(--rf-accent-tint)",
          borderRadius: 10,
          fontSize: 12,
          color: "var(--rf-text-muted)",
          lineHeight: 1.55,
          marginBottom: 16,
        }}
      >
        <strong style={{ color: "var(--rf-text)" }}>Ce este necesar:</strong>
        <ul style={{ margin: "4px 0 0", paddingLeft: 18, lineHeight: 1.7 }}>
          <li>Un <strong>certificat digital calificat</strong> (token fizic USB sau certificat soft) emis de o autoritate acreditată (ex. certSIGN, DigiSign, Trans Sped) — instalat și activ în browser.</li>
          <li>Portul <strong>8787</strong> disponibil pe calculator (nu ocupat de altă aplicație). Dacă primiți eroare de port, configurați un alt port în Setări → ANAF → Configurare avansată.</li>
        </ul>
      </div>

      {isAuthenticated ? (
        <Banner variant="success">Autentificare SPV reușită! Se continuă…</Banner>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
          {authError && <Banner variant="error">{authError}</Banner>}
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              border: "1px solid var(--rf-border)",
              borderRadius: 10,
              padding: "16px 18px",
            }}
          >
            <div style={{ display: "flex", gap: 13, alignItems: "center" }}>
              <span
                style={{
                  width: 40,
                  height: 40,
                  borderRadius: 10,
                  background: "var(--rf-accent-tint)",
                  color: "var(--rf-accent)",
                  display: "grid",
                  placeItems: "center",
                }}
              >
                <Icon name="link" size={19} />
              </span>
              <div>
                <div style={{ fontWeight: 600, fontSize: 13.5 }}>Autorizare prin OAuth</div>
                <div className="rf-text-muted" style={{ fontSize: 12.5 }}>Veți fi redirecționat către portalul ANAF pentru a autoriza accesul.</div>
              </div>
            </div>
            <Btn
              variant="primary"
              size="sm"
              icon="shield"
              disabled={isAuthenticating}
              onClick={() => { void handleAuthorize(); }}
            >
              {isAuthenticating ? "Se autorizează…" : "Conectează"}
            </Btn>
          </div>
          <Btn
            variant="ghost"
            block
            disabled={isAuthenticating}
            onClick={onNext}
          >
            Nu am certificat încă — configurez mai târziu din Setări
          </Btn>
        </div>
      )}
    </div>
  );
}

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
    <div style={{ textAlign: "center", padding: "20px 0" }}>
      <div
        style={{
          width: 76,
          height: 76,
          borderRadius: "50%",
          background: "var(--rf-success-bg)",
          color: "var(--rf-success)",
          display: "grid",
          placeItems: "center",
          margin: "0 auto",
        }}
      >
        <Icon name="check" size={40} stroke={2.4} />
      </div>
      <h1 style={{ fontSize: 26, fontWeight: 700, letterSpacing: "-0.02em", margin: "22px 0 8px" }}>
        Totul este pregătit!
      </h1>
      <p className="rf-text-muted" style={{ fontSize: 15, lineHeight: 1.6, margin: "0 auto", maxWidth: 380 }}>
        Compania <strong style={{ color: "var(--rf-text)" }}>{companyName}</strong> este configurată.
        Puteți începe să emiteți facturi electronice.
      </p>

      <div
        style={{
          padding: "12px 16px",
          background: "var(--rf-accent-tint)",
          borderRadius: 10,
          fontSize: 12.5,
          textAlign: "left",
          margin: "20px 0",
          display: "flex",
          flexDirection: "column",
          gap: 8,
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <Icon name="checkCircle" size={15} style={{ color: "var(--rf-success)" }} />
          <span>Licență activată</span>
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <Icon name="checkCircle" size={15} style={{ color: "var(--rf-success)" }} />
          <span>Companie: <strong>{companyName}</strong></span>
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <Icon name="dot" size={15} style={{ color: "var(--rf-text-muted)", opacity: 0.4 }} />
          <span style={{ color: "var(--rf-text-muted)" }}>SPV ANAF — configurabil din Setări</span>
        </div>
      </div>

      <Btn
        variant="primary"
        icon="dashboard"
        block
        disabled={finishing}
        onClick={() => { void handleClick(); }}
      >
        {finishing ? "Se finalizează…" : "Intră în aplicație"}
      </Btn>
    </div>
  );
}

// ─── Main wizard ──────────────────────────────────────────────────────────────

export function OnboardingWizard() {
  const [step, setStep] = useState<Step>(1);
  const [form, setForm] = useState<WizardFormState>(INITIAL_FORM);
  const [formError, setFormError] = useState<string | null>(null);
  const [createdName, setCreatedName] = useState("");
  const [createdCompanyId, setCreatedCompanyId] = useState<string>("");

  const queryClient = useQueryClient();
  const setActiveCompanyId = useAppStore((s) => s.setActiveCompanyId);

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
    void queryClient.invalidateQueries({ queryKey: queryKeys.companies.list() });
    void queryClient.invalidateQueries({ queryKey: queryKeys.licenseValidity });
  };

  // ── Footer nav ────────────────────────────────────────────────────────────────

  const canShowBack = step > 1 && step !== 4; // No back from company-created bridge
  const isLastStep = step === 6;
  const isFormStep = step === 3; // Form handles its own submit

  // Footer for non-form steps (form steps have their own button)
  const footer = (
    <div className="onb-foot">
      {canShowBack ? (
        <Btn variant="ghost" icon="chevLeft" onClick={() => setStep((s) => Math.max(1, s - 1) as Step)}>
          Înapoi
        </Btn>
      ) : (
        <div />
      )}
      <div style={{ flex: 1 }} />
      <span className="rf-text-muted" style={{ fontSize: 12.5 }}>
        Pasul {stepToRailIndex(step) + 1} din {RAIL_STEPS.length}
      </span>
      {!isFormStep && !isLastStep && (
        <Btn
          variant="primary"
          iconRight="chevRight"
          onClick={() => {
            if (step === 1) setStep(2);
            else if (step === 2) setStep(3); // shouldn't happen (Step2 navigates itself)
          }}
        >
          {step === 1 ? "Începe configurarea" : "Continuă"}
        </Btn>
      )}
    </div>
  );

  return (
    <div className="onb-overlay">
      <Rail currentStep={step} />

      <div className="onb-content">
        <div className="onb-card">
          {step === 1 && <Step1 />}

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

          {/* Footer nav — only for steps that don't fully own their buttons */}
          {(step === 1) && footer}
        </div>
      </div>
    </div>
  );
}
