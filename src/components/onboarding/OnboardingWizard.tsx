/**
 * Wizard de configurare inițială — verbatim port of the design "Instalare.html":
 *   pre-login centered card (.wiz-wrap → .wiz), NO app sidebar; brand top
 *   (BrandMark + word), slim 6-segment progress (.prog .seg done/cur),
 *   .step-meta kind + "n / 6", .wiz-body steps, .wiz-foot Înapoi/Continuă.
 *
 * Steps (visual = prototype): Bun venit → Licență (Trial/Solo/Contabil/Firmă
 * selectable .opt cards) → Compania ta (CUI + "Caută la ANAF" autofill + regim
 * fiscal) → Conectare ANAF SPV (OAuth + skip) → Serie și numerotare → Gata.
 *
 * ALL wiring preserved from the previous wizard:
 *   api.license.get/startTrial/activate, api.companies.fetchAnafData/create/
 *   update (serie), api.anaf.authorize/isAuthenticated,
 *   api.settings.set("first_run_completed"), query invalidations, store
 *   setActiveCompanyId.
 */

import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";
import { BrandMark } from "@/components/shared/BrandMark";
import { Ic } from "@/components/shared/Ic";
import type { AnafCompanyData, AppErrorPayload, CreateCompanyInput } from "@/types";

// ─── Constants (prototype) ────────────────────────────────────────────────────

const TOTAL = 6;
const KINDS = ["Bun venit", "Licență", "Compania", "ANAF SPV", "Numerotare", "Gata"];

type Plan = "trial" | "solo" | "contabil" | "firma";

const PLAN_LABEL: Record<Plan, string> = {
  trial: "Trial · 3 companii",
  solo: "Solo · 1 companie",
  contabil: "Contabil · 15 companii",
  firma: "Firmă · nelimitat",
};

const PLAN_OPTS: { val: Plan; ot: string; od: string }[] = [
  { val: "trial", ot: "Trial", od: "3 companii · 14 zile" },
  { val: "solo", ot: "Solo", od: "1 companie" },
  { val: "contabil", ot: "Contabil", od: "15 companii" },
  { val: "firma", ot: "Firmă", od: "companii nelimitate" },
];

const TIER_LABEL: Record<string, string> = {
  TRIAL: "Trial · 3 companii",
  SOLO: "Solo · 1 companie",
  ACCOUNTANT: "Contabil · 15 companii",
  FIRM: "Firmă · nelimitat",
};

// Prototype inline icons not in the Ic set (verbatim paths).
const CheckSvg = () => (
  <svg viewBox="0 0 24 24">
    <path d="M4.5 12.75 10 18l9.5-11.5" />
  </svg>
);
const ArrowIc = ({ back }: { back?: boolean }) => (
  <svg className="ic arrow" viewBox="0 0 24 24" style={back ? { transform: "scaleX(-1)" } : undefined}>
    <path d="M13.5 4.5 21 12m0 0-7.5 7.5M21 12H3" />
  </svg>
);
const ExtLinkIc = () => (
  <svg className="ic" viewBox="0 0 24 24">
    <path d="M13.5 6H5.25A2.25 2.25 0 0 0 3 8.25v10.5A2.25 2.25 0 0 0 5.25 21h10.5A2.25 2.25 0 0 0 18 18.75V10.5m-10.5 6L21 3m0 0h-5.25M21 3v5.25" />
  </svg>
);
const HomeIc = () => (
  <svg className="ic" viewBox="0 0 24 24">
    <path d="M2.25 12l8.954-8.955c.44-.439 1.152-.439 1.591 0L21.75 12M4.5 9.75v10.125c0 .621.504 1.125 1.125 1.125H9.75v-4.875c0-.621.504-1.125 1.125-1.125h2.25c.621 0 1.125.504 1.125 1.125V21h4.125c.621 0 1.125-.504 1.125-1.125V9.75M8.25 21h8.25" />
  </svg>
);
const InfoSvg = () => (
  <svg viewBox="0 0 24 24">
    <path d="M11.25 11.25l.041-.02a.75.75 0 0 1 1.063.852l-.708 2.836a.75.75 0 0 0 1.063.853l.041-.021M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Zm-9-3.75h.008v.008H12V8.25Z" />
  </svg>
);

// ─── Company form state ───────────────────────────────────────────────────────

interface WizardFormState {
  cui: string;
  legalName: string;
  address: string;
  city: string;
  county: string;
  vatPayer: boolean;
  taxRegime: string; // "micro" | "profit"
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
  vatPayer: false,
  taxRegime: "micro",
  email: "",
  phone: "",
  iban: "",
  bankName: "",
};

// ─── Main wizard ──────────────────────────────────────────────────────────────

export function OnboardingWizard() {
  const [step, setStep] = useState(0); // 0..5 (prototype data-step)

  // Licență
  const [plan, setPlan] = useState<Plan>("trial");
  const [trialEmail, setTrialEmail] = useState("");
  const [licenseKey, setLicenseKey] = useState("");
  const [licenseEmail, setLicenseEmail] = useState("");
  const [licenseError, setLicenseError] = useState<string | null>(null);

  // Compania
  const [form, setForm] = useState<WizardFormState>(INITIAL_FORM);
  const [formError, setFormError] = useState<string | null>(null);
  const [anafData, setAnafData] = useState<AnafCompanyData | null>(null);
  const [cuiLookupLoading, setCuiLookupLoading] = useState(false);
  const [cuiLookupError, setCuiLookupError] = useState<string | null>(null);
  const [showOptional, setShowOptional] = useState(false);
  const [createdName, setCreatedName] = useState("");
  const [createdCompanyId, setCreatedCompanyId] = useState("");

  // ANAF SPV
  const [isAuthenticating, setIsAuthenticating] = useState(false);
  const [spvConnected, setSpvConnected] = useState(false);
  const [spvError, setSpvError] = useState<string | null>(null);

  // Serie și numerotare
  const [serie, setSerie] = useState("FAC");
  const [nextNo, setNextNo] = useState("0001"); // propunere — neimplementat (numerotarea pornește mereu de la 0001 în backend)
  const [yearInNo, setYearInNo] = useState(false); // propunere — neimplementat (formatul real e SERIE-0001, fără an)
  const [serieError, setSerieError] = useState<string | null>(null);

  const [finishing, setFinishing] = useState(false);

  const queryClient = useQueryClient();
  const setActiveCompanyId = useAppStore((s) => s.setActiveCompanyId);

  // ── Licență wiring (preserved) ──────────────────────────────────────────────

  // Auto-detect an already-existing license (e.g. wizard re-run after reset)
  const { data: existingLicense, isLoading: licenseCheckLoading } = useQuery({
    queryKey: queryKeys.licenseExisting,
    queryFn: () => api.license.get(),
    staleTime: 0,
  });

  const handleLicenseActivated = () => {
    void queryClient.invalidateQueries({ queryKey: queryKeys.licenseValidity });
    void queryClient.invalidateQueries({ queryKey: queryKeys.licenseExisting });
    setStep(2);
  };

  const trialMutation = useMutation({
    mutationFn: (email: string) => api.license.startTrial(email),
    onSuccess: handleLicenseActivated,
    onError: (err) => {
      const payload = err as unknown as AppErrorPayload;
      setLicenseError(payload?.message ?? "Eroare la activarea perioadei de probă.");
    },
  });

  const activateMutation = useMutation({
    mutationFn: ({ key, email }: { key: string; email: string }) =>
      api.license.activate(key, email),
    onSuccess: handleLicenseActivated,
    onError: (err) => {
      const payload = err as unknown as AppErrorPayload;
      setLicenseError(payload?.message ?? "Licența nu a putut fi activată.");
    },
  });

  // ── Compania wiring (preserved) ─────────────────────────────────────────────

  const handleCuiLookup = async () => {
    if (!form.cui.trim()) return;
    setCuiLookupLoading(true);
    setCuiLookupError(null);
    try {
      const data = await api.companies.fetchAnafData(form.cui.trim());
      setAnafData(data);
      setForm((f) => ({
        ...f,
        legalName: data.legalName,
        address: data.address,
        city: data.city,
        county: data.county.slice(0, 2).toUpperCase(),
        vatPayer: data.vatPayer,
      }));
    } catch {
      setCuiLookupError("CUI-ul nu a fost găsit în baza ANAF.");
    } finally {
      setCuiLookupLoading(false);
    }
  };

  const create = useMutation({
    mutationFn: (input: CreateCompanyInput) => api.companies.create(input),
    onSuccess: (company) => {
      setActiveCompanyId(company.id);
      setCreatedName(company.legalName);
      setCreatedCompanyId(company.id);
      setStep(3);
    },
    onError: (err) => {
      const payload = err as unknown as AppErrorPayload;
      setFormError(payload?.message ?? "Eroare necunoscută.");
    },
  });

  const handleCompanySubmit = () => {
    setFormError(null);
    if (createdCompanyId) { setStep(3); return; } // already created (defensive)
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
      invoiceSeries: "FAC", // se actualizează la pasul „Serie și numerotare"
      vatPayer: form.vatPayer,
      taxRegime: form.taxRegime,
      email: form.email.trim() || undefined,
      phone: form.phone.trim() || undefined,
      iban: form.iban.trim() || undefined,
      bankName: form.bankName.trim() || undefined,
    });
  };

  // ── ANAF SPV wiring (preserved) ─────────────────────────────────────────────

  const handleAuthorize = async () => {
    setIsAuthenticating(true);
    setSpvError(null);
    try {
      await api.anaf.authorize(createdCompanyId);
      const authed = await api.anaf.isAuthenticated(createdCompanyId);
      setSpvConnected(authed);
      if (!authed) {
        setSpvError("Autorizarea nu s-a finalizat. Încercați din nou sau treceți peste.");
      }
    } catch (e) {
      setSpvError(formatError(e, "Autorizarea a eșuat. Verificați conexiunea și reîncercați."));
    } finally {
      setIsAuthenticating(false);
    }
  };

  // ── Serie wiring (real: update invoiceSeries on the created company) ────────

  const serieMutation = useMutation({
    mutationFn: () =>
      api.companies.update(createdCompanyId, {
        invoiceSeries: serie.trim().toUpperCase() || "FAC",
      }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.companies.list() });
      setStep(5);
    },
    onError: (err) => {
      setSerieError(formatError(err, "Seria nu a putut fi salvată."));
    },
  });

  const handleSerieSubmit = () => {
    setSerieError(null);
    if (!createdCompanyId) { setStep(5); return; }
    serieMutation.mutate();
  };

  // ── Finish (preserved) ──────────────────────────────────────────────────────

  const handleFinish = async () => {
    setFinishing(true);
    await api.settings.set("first_run_completed", "1");
    void queryClient.invalidateQueries({ queryKey: queryKeys.companies.list() });
    void queryClient.invalidateQueries({ queryKey: queryKeys.licenseValidity });
  };

  // ── Footer nav ──────────────────────────────────────────────────────────────

  const busy =
    trialMutation.isPending || activateMutation.isPending ||
    create.isPending || serieMutation.isPending;

  const handleNext = () => {
    if (step === 0) { setStep(1); return; }
    if (step === 1) {
      setLicenseError(null);
      if (existingLicense) { handleLicenseActivated(); return; }
      if (plan === "trial") {
        if (!trialEmail.trim()) { setLicenseError("Adresa de email este obligatorie."); return; }
        trialMutation.mutate(trialEmail.trim());
        return;
      }
      // Solo / Contabil / Firmă → activare cu cheia primită (tier-ul vine din cheie)
      if (!licenseKey.trim()) { setLicenseError("Cheia de licență este obligatorie."); return; }
      if (!licenseEmail.trim()) { setLicenseError("Adresa de email este obligatorie."); return; }
      activateMutation.mutate({ key: licenseKey.trim(), email: licenseEmail.trim() });
      return;
    }
    if (step === 2) { handleCompanySubmit(); return; }
    if (step === 3) { setStep(4); return; }
    if (step === 4) { handleSerieSubmit(); return; }
  };

  // No back into the company step once the company is committed (would re-create).
  const backDisabled = step === 0 || step === 3 || busy;
  const isLast = step === TOTAL - 1;

  // ── Serie preview (prototype #seriePrev) ────────────────────────────────────

  const seriePreview = (() => {
    const s = serie.trim().toUpperCase() || "FAC";
    const n = nextNo.trim() || "0001";
    return yearInNo ? `${s}-${new Date().getFullYear()}-${n}` : `${s}-${n}`;
  })();

  // ── Field helper ────────────────────────────────────────────────────────────

  const field = (key: keyof Omit<WizardFormState, "vatPayer">) => ({
    value: form[key] as string,
    onChange: (e: React.ChangeEvent<HTMLInputElement>) =>
      setForm((f) => ({ ...f, [key]: e.target.value })),
  });

  // ── Render ──────────────────────────────────────────────────────────────────

  return (
    <div className="wiz-wrap">
      <div className="wiz">
        {/* ── Top: brand + progress ── */}
        <div className="wiz-top">
          <div className="brand">
            <BrandMark size={34} />
            <span className="word">Clarito</span>
          </div>
          <div className="prog">
            {Array.from({ length: TOTAL }, (_, i) => (
              <div key={i} className={"seg" + (i < step ? " done" : i === step ? " cur" : "")}>
                <span />
              </div>
            ))}
          </div>
          <div className="step-meta">
            <span className="k">{KINDS[step]}</span>
            <span className="c"><span>{step + 1}</span> / {TOTAL}</span>
          </div>
        </div>

        {/* ── Body ── */}
        <div className="wiz-body">
          {/* 1 Bun venit */}
          {step === 0 && (
            <div className="step active">
              <h2>Bun venit la Clarito</h2>
              <p className="lead">
                Contabilitate și e-Factura pentru firma ta — facturi, SPV, declarații și jurnale,
                într-un singur loc. Hai să configurăm aplicația în câțiva pași.
              </p>
              <div className="toggle-note">
                <InfoSvg />
                <span>Datele se păstrează local pe acest dispozitiv. Poți conecta ANAF SPV acum sau mai târziu.</span>
              </div>
            </div>
          )}

          {/* 2 Licență */}
          {step === 1 && (
            <div className="step active">
              <h2>Licență</h2>
              {!licenseCheckLoading && existingLicense ? (
                <>
                  <p className="lead">Licența ta este deja activă. Poți continua configurarea.</p>
                  <div className="anaf-card">
                    <div className="anaf-row">
                      <div className="anaf-ic"><Ic name="shield" cls="" /></div>
                      <div style={{ flex: 1 }}>
                        <div className="at">{TIER_LABEL[existingLicense.tier] ?? existingLicense.tier}</div>
                        <div className="as">{existingLicense.email ?? "Licență locală"}</div>
                      </div>
                      <span className="chip ok"><svg className="sic" viewBox="0 0 24 24"><path d="M9 12.75 11.25 15 15 9.75M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z" /></svg>Activă</span>
                    </div>
                  </div>
                </>
              ) : (
                <>
                  <p className="lead">Alege planul. Îl poți schimba oricând din Setări.</p>
                  <div className="opts">
                    {PLAN_OPTS.map((o) => (
                      <div
                        key={o.val}
                        className={"opt" + (plan === o.val ? " sel" : "")}
                        role="button"
                        tabIndex={0}
                        onClick={() => { setPlan(o.val); setLicenseError(null); }}
                        onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { setPlan(o.val); setLicenseError(null); } }}
                      >
                        <div className="ot">{o.ot}</div>
                        <div className="od">{o.od}</div>
                        <span className="chk"><CheckSvg /></span>
                      </div>
                    ))}
                  </div>
                  {plan === "trial" ? (
                    <div className="field">
                      <label htmlFor="trial-email">Adresă email</label>
                      <input
                        id="trial-email"
                        className="input"
                        type="email"
                        placeholder="office@firma.ro"
                        value={trialEmail}
                        onChange={(e) => setTrialEmail(e.target.value)}
                      />
                      <span className="hint">14 zile gratuit, fără card. Trial-ul pornește la „Continuă".</span>
                    </div>
                  ) : (
                    <div className="row2" style={{ marginTop: 16 }}>
                      <div className="field" style={{ marginTop: 0 }}>
                        <label htmlFor="license-key">Cheie licență</label>
                        <input
                          id="license-key"
                          className="input num"
                          placeholder="XXXX-XXXX-XXXX-XXXX"
                          style={{ textTransform: "uppercase", letterSpacing: "0.05em" }}
                          value={licenseKey}
                          onChange={(e) => setLicenseKey(e.target.value)}
                          autoComplete="off"
                          spellCheck={false}
                        />
                      </div>
                      <div className="field" style={{ marginTop: 0 }}>
                        <label htmlFor="license-email">Email achiziție</label>
                        <input
                          id="license-email"
                          className="input"
                          type="email"
                          placeholder="office@firma.ro"
                          value={licenseEmail}
                          onChange={(e) => setLicenseEmail(e.target.value)}
                        />
                      </div>
                    </div>
                  )}
                  {licenseError && <p className="werr">{licenseError}</p>}
                </>
              )}
            </div>
          )}

          {/* 3 Compania */}
          {step === 2 && (
            <div className="step active">
              <h2>Compania ta</h2>
              <p className="lead">Introdu CUI-ul și completăm restul din registrul ANAF.</p>
              <div className="cui-row" style={{ marginTop: 16 }}>
                <div className="field">
                  <label htmlFor="cui">CUI / CIF</label>
                  <input id="cui" className="input num" type="text" placeholder="ex. RO12345678" {...field("cui")} />
                </div>
                <button
                  className="btn btn-ghost"
                  type="button"
                  disabled={cuiLookupLoading}
                  onClick={() => { void handleCuiLookup(); }}
                >
                  <Ic name="lens" cls="ic" />
                  {cuiLookupLoading ? "Se caută…" : "Caută la ANAF"}
                </button>
              </div>
              <div className={"fetched" + (anafData ? " show" : "")}>
                <CheckSvg />
                Date preluate din registrul ANAF
              </div>
              {cuiLookupError && <p className="werr">{cuiLookupError}</p>}

              {/* Real editable fields (the prototype shows them read-only in .kv;
                  the app keeps them editable so onboarding works and when ANAF
                  lookup fails) */}
              <div className="field">
                <label htmlFor="w-legalName">Denumire</label>
                <input id="w-legalName" className="input" placeholder="S.C. Exemplu S.R.L." {...field("legalName")} />
              </div>
              <div className="field">
                <label htmlFor="w-address">Adresă</label>
                <input id="w-address" className="input" placeholder="Str. Exemplu nr. 1" {...field("address")} />
              </div>
              <div className="row2" style={{ marginTop: 16 }}>
                <div className="field" style={{ marginTop: 0 }}>
                  <label htmlFor="w-city">Localitate</label>
                  <input id="w-city" className="input" placeholder="București" {...field("city")} />
                </div>
                <div className="field" style={{ marginTop: 0 }}>
                  <label htmlFor="w-county">Județ</label>
                  <input
                    id="w-county"
                    className="input num"
                    placeholder="B"
                    maxLength={2}
                    style={{ textTransform: "uppercase" }}
                    {...field("county")}
                  />
                </div>
              </div>
              <div className="row2" style={{ marginTop: 16 }}>
                <div className="field" style={{ marginTop: 0 }}>
                  <label htmlFor="w-vat">Plătitor TVA</label>
                  <select
                    id="w-vat"
                    className="select"
                    value={form.vatPayer ? "da" : "nu"}
                    onChange={(e) => setForm((f) => ({ ...f, vatPayer: e.target.value === "da" }))}
                  >
                    <option value="nu">Nu</option>
                    <option value="da">Da</option>
                  </select>
                </div>
                <div className="field" style={{ marginTop: 0 }}>
                  <label htmlFor="regim">Regim fiscal</label>
                  <select
                    id="regim"
                    className="select"
                    value={form.taxRegime}
                    onChange={(e) => setForm((f) => ({ ...f, taxRegime: e.target.value }))}
                  >
                    <option value="micro">Microîntreprindere · impozit pe venit 1%</option>
                    <option value="profit">Impozit pe profit · 16%</option>
                  </select>
                </div>
              </div>

              {anafData && (
                <dl className="kv">
                  <dt>TVA la încasare</dt><dd>{anafData.cashVat ? "Da" : "Nu"}</dd>
                  {anafData.registryNumber && (<><dt>Nr. Reg. Com.</dt><dd className="num">{anafData.registryNumber}</dd></>)}
                </dl>
              )}

              <button
                className="btn btn-link"
                type="button"
                onClick={() => setShowOptional((v) => !v)}
              >
                {showOptional ? "− Ascunde datele opționale" : "+ Date opționale (email, telefon, IBAN, bancă)"}
              </button>
              {showOptional && (
                <>
                  <div className="row2">
                    <div className="field" style={{ marginTop: 0 }}>
                      <label htmlFor="w-email">Email</label>
                      <input id="w-email" className="input" type="email" placeholder="office@firma.ro" {...field("email")} />
                    </div>
                    <div className="field" style={{ marginTop: 0 }}>
                      <label htmlFor="w-phone">Telefon</label>
                      <input id="w-phone" className="input" placeholder="+40 722 000 000" {...field("phone")} />
                    </div>
                  </div>
                  <div className="row2" style={{ marginTop: 12 }}>
                    <div className="field" style={{ marginTop: 0 }}>
                      <label htmlFor="w-iban">IBAN</label>
                      <input id="w-iban" className="input num" placeholder="RO49AAAA…" {...field("iban")} />
                    </div>
                    <div className="field" style={{ marginTop: 0 }}>
                      <label htmlFor="w-bank">Bancă</label>
                      <input id="w-bank" className="input" placeholder="Banca Transilvania" {...field("bankName")} />
                    </div>
                  </div>
                </>
              )}
              {formError && <p className="werr">{formError}</p>}
            </div>
          )}

          {/* 4 ANAF SPV */}
          {step === 3 && (
            <div className="step active">
              <h2>Conectare ANAF SPV</h2>
              <p className="lead">
                Conectează Spațiul Privat Virtual pentru e-Factura: trimitere, recipise și mesaje, automat.
              </p>
              <div className="anaf-card">
                <div className="anaf-row">
                  <div className="anaf-ic"><Ic name="shield" cls="" /></div>
                  <div style={{ flex: 1 }}>
                    <div className="at">Spațiul Privat Virtual</div>
                    <div className="as">
                      {spvConnected
                        ? "Certificat valabil · token reînnoit automat"
                        : "Autorizare OAuth · certificat calificat"}
                    </div>
                  </div>
                  {spvConnected ? (
                    <span className="chip ok">
                      <svg className="sic" viewBox="0 0 24 24"><path d="M9 12.75 11.25 15 15 9.75M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z" /></svg>
                      Conectat
                    </span>
                  ) : (
                    <span className="chip wait">
                      <svg className="sic" viewBox="0 0 24 24"><circle cx="12" cy="12" r="4.5" /></svg>
                      Neconectat
                    </span>
                  )}
                </div>
                {!spvConnected && (
                  <button
                    className="btn btn-dark"
                    type="button"
                    style={{ width: "100%", marginTop: 14 }}
                    disabled={isAuthenticating}
                    onClick={() => { void handleAuthorize(); }}
                  >
                    <ExtLinkIc />
                    {isAuthenticating ? "Se autorizează…" : "Conectează-te la ANAF"}
                  </button>
                )}
              </div>
              {spvError && <p className="werr">{spvError}</p>}
              <div className="toggle-note">
                <InfoSvg />
                <span>
                  Este necesar un certificat digital calificat (token USB sau soft-cert) instalat în
                  browser și portul 8787 liber. Portul se poate schimba din Setări → ANAF.
                </span>
              </div>
              {!spvConnected && (
                <button className="btn btn-link" type="button" onClick={() => setStep(4)}>
                  Poți face asta mai târziu →
                </button>
              )}
            </div>
          )}

          {/* 5 Serie */}
          {step === 4 && (
            <div className="step active">
              <h2>Serie și numerotare facturi</h2>
              <p className="lead">Stabilește seria și de la ce număr pornește numerotarea.</p>
              <div className="row2" style={{ marginTop: 18 }}>
                <div className="field" style={{ marginTop: 0 }}>
                  <label htmlFor="serie">Serie</label>
                  <input
                    id="serie"
                    className="input num"
                    type="text"
                    style={{ textTransform: "uppercase" }}
                    value={serie}
                    onChange={(e) => setSerie(e.target.value)}
                  />
                </div>
                <div className="field" style={{ marginTop: 0 }}>
                  <label htmlFor="nextno">Următorul număr</label>
                  {/* propunere — neimplementat: backend-ul pornește mereu de la 0001 */}
                  <input
                    id="nextno"
                    className="input num"
                    type="text"
                    value={nextNo}
                    onChange={(e) => setNextNo(e.target.value)}
                    onBlur={() => {
                      if (nextNo.trim() && nextNo.trim() !== "0001") {
                        notify.info("Număr de pornire personalizat — în curând. Numerotarea pornește de la 0001.");
                      }
                    }}
                  />
                </div>
              </div>
              <div className="field">
                <label htmlFor="anfmt">Anul în număr</label>
                {/* propunere — neimplementat: formatul real al numărului e SERIE-0001 */}
                <select
                  id="anfmt"
                  className="select"
                  value={yearInNo ? "da" : "nu"}
                  onChange={(e) => {
                    const withYear = e.target.value === "da";
                    setYearInNo(withYear);
                    if (withYear) notify.info("Formatul cu anul în număr — în curând.");
                  }}
                >
                  <option value="da">Da — FAC-{new Date().getFullYear()}-0001</option>
                  <option value="nu">Nu — FAC-0001</option>
                </select>
              </div>
              <div className="kv" style={{ gridTemplateColumns: "1fr" }}>
                <dt style={{ color: "var(--dim)" }}>Previzualizare</dt>
                <dd className="num" style={{ fontFamily: "var(--mono)", fontSize: 13, fontWeight: 600 }}>
                  {seriePreview}
                </dd>
              </div>
              {serieError && <p className="werr">{serieError}</p>}
            </div>
          )}

          {/* 6 Gata */}
          {step === 5 && (
            <div className="step active">
              <div className="done-wrap">
                <div className="done-ring"><CheckSvg /></div>
                <h2>Gata de pornire</h2>
                <p className="lead" style={{ maxWidth: 380, margin: "7px auto 0" }}>
                  Configurarea s-a încheiat. Poți emite prima factură și sincroniza SPV din tabloul de bord.
                </p>
              </div>
              <dl className="kv recap">
                <dt>Plan</dt>
                <dd>{existingLicense ? (TIER_LABEL[existingLicense.tier] ?? existingLicense.tier) : PLAN_LABEL[plan]}</dd>
                <dt>Companie</dt>
                <dd>{createdName}{form.cui.trim() ? ` · ${form.cui.trim()}` : ""}</dd>
                <dt>Regim fiscal</dt>
                <dd>{form.taxRegime === "micro" ? "Microîntreprindere 1%" : "Impozit pe profit 16%"}</dd>
                <dt>ANAF SPV</dt>
                <dd>{spvConnected ? "Conectat" : "Neconectat — se poate face din Setări"}</dd>
                <dt>Serie facturi</dt>
                <dd className="num">{seriePreview}</dd>
              </dl>
            </div>
          )}
        </div>

        {/* ── Footer nav ── */}
        <div className="wiz-foot">
          <button
            className="btn btn-ghost"
            type="button"
            disabled={backDisabled}
            onClick={() => setStep((s) => Math.max(0, s - 1))}
          >
            <ArrowIc back />
            Înapoi
          </button>
          {!isLast ? (
            <button className="btn btn-dark" type="button" disabled={busy} onClick={handleNext}>
              {busy
                ? "Se salvează…"
                : step === 0
                ? "Începe configurarea"
                : "Continuă"}
              <ArrowIc />
            </button>
          ) : (
            <button
              className="btn btn-dark"
              type="button"
              disabled={finishing}
              onClick={() => { void handleFinish(); }}
            >
              <HomeIc />
              {finishing ? "Se finalizează…" : "Intră în aplicație"}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
