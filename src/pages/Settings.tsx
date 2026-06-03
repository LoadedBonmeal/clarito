/**
 * Setări aplicație — temă, densitate, companie activă, ANAF, licență,
 * integrări, notificări, arhivă, GDPR, sistem, jurnal activitate.
 *
 * Wave 6 re-skin: rf SectionCard + Field/Input/Select/Segmented/Toggle/Banner/Btn.
 * Wiring: 100% preserved from original — same api.* calls + setting keys.
 */

import { useQuery, useQueries, useMutation, useQueryClient } from "@tanstack/react-query";
import { useState, useEffect, useId } from "react";
import { open, save, confirm } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";

import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore, type ThemeMode, type DensityMode } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { Company } from "@/types";

import {
  Btn,
  Badge,
  SectionCard,
  Field,
  Input,
  Select,
  Textarea,
  Segmented,
  Toggle,
  Banner,
  PageHeader,
} from "@/components/rf";
import { Icon } from "@/components/shared/Icon";

// ─── helpers ──────────────────────────────────────────────────────────────────

function fmtBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

/** One row in a settings section: label + optional description + control */
function SettingRow({
  label,
  desc,
  last,
  children,
}: {
  label: string;
  desc?: string;
  last?: boolean;
  children: React.ReactNode;
}) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        gap: 20,
        padding: "13px 0",
        borderBottom: last ? "none" : "1px solid var(--rf-border)",
      }}
    >
      <div>
        <div style={{ fontSize: 13.5, fontWeight: 500 }}>{label}</div>
        {desc && (
          <div className="rf-text-muted" style={{ fontSize: 12.5, marginTop: 2 }}>
            {desc}
          </div>
        )}
      </div>
      <div style={{ flexShrink: 0 }}>{children}</div>
    </div>
  );
}

// ─── Notification type definitions ────────────────────────────────────────────

const NOTIF_TYPES = [
  { key: "validated",     label: "Factură validată ANAF" },
  { key: "rejected",      label: "Factură respinsă ANAF" },
  { key: "received",      label: "Facturi noi primite SPV" },
  { key: "cert_expiring", label: "Certificat SPV expiră" },
  { key: "cert_expired",  label: "Certificat SPV expirat" },
] as const;

const NOTIF_KEYS = ["validated", "rejected", "received", "cert_expiring", "cert_expired"] as const;
type NotifKey = typeof NOTIF_KEYS[number];

// ─── Tier display map ──────────────────────────────────────────────────────────

const TIER_LABELS: Record<string, string> = {
  TRIAL: "Probă gratuită",
  SOLO: "Solo",
  ACCOUNTANT: "Contabil",
  FIRM: "Firmă",
};

// ─── Main page ────────────────────────────────────────────────────────────────

export function SettingsPage() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();

  // Store
  const theme = useAppStore((s) => s.theme);
  const setTheme = useAppStore((s) => s.setTheme);
  const density = useAppStore((s) => s.density);
  const setDensity = useAppStore((s) => s.setDensity);
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const setActiveCompanyId = useAppStore((s) => s.setActiveCompanyId);

  // ── Queries ──────────────────────────────────────────────────────────────────

  const { data: appInfo, isLoading: appInfoLoading } = useQuery({
    queryKey: queryKeys.appInfo,
    queryFn: () => api.system.appInfo(),
  });

  const { data: companies = [] } = useQuery({
    queryKey: queryKeys.companies.list(),
    queryFn: () => api.companies.list(),
  });

  const { data: license, isLoading: licenseLoading } = useQuery({
    queryKey: queryKeys.license,
    queryFn: () => api.license.get(),
  });

  const { data: archiveSize } = useQuery({
    queryKey: queryKeys.system.archiveSize,
    queryFn: () => api.archive.getSize(),
    staleTime: 60_000,
  });

  const { data: autostartEnabled } = useQuery({
    queryKey: queryKeys.system.autostart,
    queryFn: () => api.system.getAutostart(),
    staleTime: Infinity,
  });

  const { data: activityLog = [] } = useQuery({
    queryKey: queryKeys.system.activityLog,
    queryFn: () => api.system.getActivityLog(),
    refetchInterval: 30_000,
  });

  const { data: testModeSetting } = useQuery({
    queryKey: queryKeys.anaf.testMode,
    queryFn: () => api.settings.get("use_anaf_test_env"),
  });

  const anafTestMode = testModeSetting === "1";

  const { data: isAnafAuthenticated, refetch: refetchAnafAuth } = useQuery({
    queryKey: queryKeys.anaf.auth(activeCompanyId ?? ""),
    queryFn: () => api.anaf.isAuthenticated(activeCompanyId!),
    enabled: !!activeCompanyId,
  });

  // Notification preferences
  const notifResults = useQueries({
    queries: NOTIF_KEYS.map((key) => ({
      queryKey: queryKeys.settings.get(`notif_pref_${key}`),
      queryFn: () => api.settings.get(`notif_pref_${key}`),
    })),
  });

  const notifPrefMap = Object.fromEntries(
    NOTIF_KEYS.map((key, i) => [key, notifResults[i].data ?? "os"])
  ) as Record<NotifKey, string>;

  const { data: quietHoursSetting } = useQuery({
    queryKey: queryKeys.settings.get("quiet_hours"),
    queryFn: () => api.settings.get("quiet_hours"),
  });

  // ── Local state ───────────────────────────────────────────────────────────────

  // ANAF advanced OAuth config
  const [anafAdvancedOpen, setAnafAdvancedOpen] = useState(false);
  const [anafClientId, setAnafClientId] = useState("");
  const [anafClientSecret, setAnafClientSecret] = useState("");
  const [anafHasSecret, setAnafHasSecret] = useState(false);
  const [anafRedirectUri, setAnafRedirectUri] = useState("");
  const [anafCallbackPort, setAnafCallbackPort] = useState("");
  const [anafAuthorizeUrl, setAnafAuthorizeUrl] = useState("");
  const [anafTokenUrl, setAnafTokenUrl] = useState("");
  const [anafAdvancedSaving, setAnafAdvancedSaving] = useState(false);
  const [anafAdvancedSaved, setAnafAdvancedSaved] = useState(false);
  const [anafError, setAnafError] = useState<string | null>(null);

  const [updateStatus, setUpdateStatus] = useState<string | null>(null);
  const [checkingUpdate, setCheckingUpdate] = useState(false);

  // SmartBill
  const [smartbillUser, setSmartbillUser] = useState("");
  const [smartbillToken, setSmartbillToken] = useState("");
  const [savingSmartbill, setSavingSmartbill] = useState(false);
  const [smartbillSaved, setSmartbillSaved] = useState(false);

  // License activation in Settings
  const [showLicenseActivate, setShowLicenseActivate] = useState(false);
  const [licenseKeyInput, setLicenseKeyInput] = useState("");
  const [licenseEmailInput, setLicenseEmailInput] = useState("");
  const [licenseActivateError, setLicenseActivateError] = useState<string | null>(null);

  // Feedback
  const feedbackMsgId = useId();
  const [feedbackMsg, setFeedbackMsg] = useState("");
  const [feedbackSending, setFeedbackSending] = useState(false);

  // ── Effects ───────────────────────────────────────────────────────────────────

  // Load advanced ANAF settings on mount
  useEffect(() => {
    void (async () => {
      const [cid, ruri, port, aurl, turl] = await Promise.all([
        api.settings.get("anaf_oauth_client_id"),
        api.settings.get("anaf_oauth_redirect_uri"),
        api.settings.get("anaf_oauth_callback_port"),
        api.settings.get("anaf_oauth_authorize_url"),
        api.settings.get("anaf_oauth_token_url"),
      ]);
      setAnafClientId(cid ?? "");
      setAnafRedirectUri(ruri ?? "");
      setAnafCallbackPort(port ?? "");
      setAnafAuthorizeUrl(aurl ?? "");
      setAnafTokenUrl(turl ?? "");
      try {
        setAnafHasSecret(await api.anaf.hasOauthClientSecret());
      } catch {
        setAnafHasSecret(false);
      }
    })();
  }, []);

  // Load SmartBill credentials on active company change
  useEffect(() => {
    if (!activeCompanyId) return;
    api.integrations.getSmartbillCredentials(activeCompanyId)
      .then((creds) => {
        setSmartbillUser(creds.user);
        setSmartbillToken(creds.configured ? "••••••••" : "");
      })
      .catch(() => {});
  }, [activeCompanyId]);

  // ── Mutations ─────────────────────────────────────────────────────────────────

  const authorizeAnaf = useMutation({
    mutationFn: () => api.anaf.authorize(activeCompanyId!),
    onSuccess: () => { void refetchAnafAuth(); setAnafError(null); },
    onError: (e) => setAnafError(formatError(e, "Eroare autorizare ANAF.")),
  });

  const logoutAnaf = useMutation({
    mutationFn: () => api.anaf.logout(activeCompanyId!),
    onSuccess: () => { void refetchAnafAuth(); setAnafError(null); },
  });

  const licenseActivateMutation = useMutation({
    mutationFn: () => api.license.activate(licenseKeyInput.trim(), licenseEmailInput.trim()),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.license });
      void queryClient.invalidateQueries({ queryKey: queryKeys.licenseValidity });
      setShowLicenseActivate(false);
      setLicenseKeyInput("");
      setLicenseEmailInput("");
      notify.success("Licența a fost activată cu succes.");
    },
    onError: (e) => setLicenseActivateError(formatError(e, "Licența nu a putut fi activată.")),
  });

  // ── Handlers ──────────────────────────────────────────────────────────────────

  const handleSaveAnafAdvanced = async () => {
    setAnafAdvancedSaving(true);
    try {
      await Promise.all([
        api.settings.set("anaf_oauth_client_id", anafClientId),
        api.settings.set("anaf_oauth_redirect_uri", anafRedirectUri),
        api.settings.set("anaf_oauth_callback_port", anafCallbackPort),
        api.settings.set("anaf_oauth_authorize_url", anafAuthorizeUrl),
        api.settings.set("anaf_oauth_token_url", anafTokenUrl),
      ]);
      // client_secret-ul merge în keychain, doar dacă utilizatorul a introdus o valoare nouă.
      if (anafClientSecret.trim() !== "") {
        await api.anaf.setOauthClientSecret(anafClientSecret.trim());
        setAnafClientSecret("");
        setAnafHasSecret(true);
      }
      setAnafAdvancedSaved(true);
      setTimeout(() => setAnafAdvancedSaved(false), 3000);
    } catch (e) {
      notify.error(formatError(e, "Eroare la salvarea configurației ANAF avansate."));
    } finally {
      setAnafAdvancedSaving(false);
    }
  };

  const handleTestModeChange = async (enabled: boolean) => {
    await api.settings.set("use_anaf_test_env", enabled ? "1" : "0");
    void queryClient.invalidateQueries({ queryKey: queryKeys.anaf.testMode });
  };

  const handleSaveSmartbill = async () => {
    if (!activeCompanyId) return;
    setSavingSmartbill(true);
    try {
      const tokenToSave =
        smartbillToken && !smartbillToken.startsWith("•") ? smartbillToken : undefined;
      await invoke("set_smartbill_credentials", {
        companyId: activeCompanyId,
        user: smartbillUser,
        token: tokenToSave ?? null,
      });
      // Scrub any legacy plaintext token in settings DB
      try {
        await api.settings.set(`smartbill_token_${activeCompanyId}`, "");
      } catch {
        // ignore — old key may not exist
      }
      setSmartbillSaved(true);
      setTimeout(() => setSmartbillSaved(false), 3000);
    } catch {
      notify.error("Eroare la salvare credențiale SmartBill.");
    } finally {
      setSavingSmartbill(false);
    }
  };

  const handleNotifPrefChange = async (key: string, value: string) => {
    await api.settings.set(`notif_pref_${key}`, value);
    void queryClient.invalidateQueries({ queryKey: queryKeys.settings.get(`notif_pref_${key}`) });
  };

  const handleNotifToggle = async (key: string, checked: boolean) => {
    await api.settings.set(key, checked ? "1" : "0");
    void queryClient.invalidateQueries({ queryKey: queryKeys.settings.get(key) });
  };

  const sendFeedback = async () => {
    setFeedbackSending(true);
    try {
      const report = await api.feedback.gather();
      const url = await api.feedback.mailto(report, feedbackMsg || undefined);
      await openUrl(url);
      notify.success("Email pregătit în clientul dvs. de email");
    } catch (e) {
      notify.error(
        formatError(e, "Nu pot deschide clientul de email — trimite manual la support@lucaris.ro"),
      );
    } finally {
      setFeedbackSending(false);
    }
  };

  const openPurchase = async () => {
    try {
      const purchase = await api.settings.get("purchase_url");
      const url = purchase || "https://lucaris.ro/rofactura#pret";
      await openUrl(url);
    } catch (e) {
      notify.error(formatError(e, "Nu pot deschide pagina de cumpărare."));
    }
  };

  const handleDevSeed = async () => {
    const ok = await confirm("Populați baza de date cu date de test? Funcționează doar dacă DB-ul este gol.", {
      title: "Date de test",
      kind: "info",
    });
    if (!ok) return;
    try {
      await api.system.devSeed();
      void queryClient.invalidateQueries({ queryKey: queryKeys.companies.all });
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
      notify.success("Date de test adăugate cu succes.");
    } catch {
      notify.error("Seed-ul a eșuat sau DB-ul nu este gol.");
    }
  };

  // ─────────────────────────────────────────────────────────────────────────────

  const THEME_OPTIONS: { value: ThemeMode; label: string }[] = [
    { value: "light", label: "Luminos" },
    { value: "dark", label: "Întunecat" },
    { value: "system", label: "Sistem" },
  ];

  const DENSITY_OPTIONS: { value: DensityMode; label: string }[] = [
    { value: "compact", label: "Compact" },
    { value: "comfortable", label: "Confortabil" },
    { value: "relaxed", label: "Lejer" },
  ];

  const activeCompany = companies.find((c: Company) => c.id === activeCompanyId);

  return (
    <>
      <PageHeader title={t("settings.title")} />

      <div
        className="rf-page-body"
        style={{ maxWidth: 860, width: "100%", alignSelf: "center", margin: "0 auto" }}
      >
        {/* ── Temă și afișare ── */}
        <SectionCard icon="settings" title="Temă și afișare" subtitle="Personalizați aspectul aplicației">
          <div style={{ padding: "0 24px 16px" }}>
            <SettingRow label="Densitate rânduri" desc="Înălțimea rândurilor în tabele și liste.">
              <Segmented
                value={density ?? "comfortable"}
                onChange={(v) => setDensity(v)}
                options={DENSITY_OPTIONS}
              />
            </SettingRow>
            <SettingRow label="Temă interfață" desc="Comutați între tema luminoasă, întunecată sau sistem.">
              <Segmented
                value={theme}
                onChange={(v) => setTheme(v)}
                options={THEME_OPTIONS}
              />
            </SettingRow>
          </div>
        </SectionCard>

        {/* ── Companie activă ── */}
        <SectionCard icon="building" title="Companie activă" subtitle={activeCompany ? activeCompany.legalName : "Selectați compania de lucru"}>
          <div style={{ padding: "0 24px 16px" }}>
            <SettingRow label="Companie curentă" last={!activeCompany}>
              {companies.length === 0 ? (
                <span className="rf-text-muted" style={{ fontSize: 13 }}>Nicio companie configurată</span>
              ) : (
                <Select
                  value={activeCompanyId ?? ""}
                  onChange={(e) => setActiveCompanyId(e.target.value || null)}
                  style={{ minWidth: 280 }}
                >
                  <option value="">— Selectați compania —</option>
                  {companies.map((c: Company) => (
                    <option key={c.id} value={c.id}>
                      {c.legalName} ({c.cui})
                    </option>
                  ))}
                </Select>
              )}
            </SettingRow>
            {activeCompany && (
              <>
                <SettingRow label="CUI">
                  <span className="rf-mono" style={{ fontSize: 13 }}>{activeCompany.cui}</span>
                </SettingRow>
                <SettingRow label="Serie facturi">
                  <span className="rf-mono" style={{ fontSize: 13 }}>{activeCompany.invoiceSeries}</span>
                </SettingRow>
                <SettingRow label="Plătitor TVA">
                  <span style={{ fontSize: 13 }}>{activeCompany.vatPayer ? "Da" : "Nu"}</span>
                </SettingRow>
                <SettingRow label="SPV activat" last>
                  <span style={{ fontSize: 13 }}>{activeCompany.spvEnabled ? "Da" : "Nu"}</span>
                </SettingRow>
              </>
            )}
          </div>
        </SectionCard>

        {/* ── ANAF / SPV ── */}
        <SectionCard
          icon="shield"
          title="ANAF / SPV"
          subtitle="Configurare OAuth pentru conectarea la SPV"
          actions={
            activeCompanyId && isAnafAuthenticated ? (
              <Badge variant="success" dot>Conectat</Badge>
            ) : activeCompanyId ? (
              <Badge variant="neutral" dot>Neconectat</Badge>
            ) : undefined
          }
        >
          <div style={{ padding: "0 24px 16px" }}>
            <SettingRow label="Mediu ANAF" desc="Test (sandbox ANAF) sau Producție.">
              <Segmented
                value={anafTestMode ? "test" : "prod"}
                onChange={(v) => void handleTestModeChange(v === "test")}
                options={[
                  { value: "test", label: "Test" },
                  { value: "prod", label: "Producție" },
                ]}
              />
            </SettingRow>
            <SettingRow label="URL API ANAF">
              <span className="rf-mono" style={{ fontSize: 12 }}>
                {anafTestMode
                  ? "https://api.anaf.ro/test/FCTEL/rest/"
                  : "https://api.anaf.ro/prod/FCTEL/rest/"}
              </span>
            </SettingRow>

            {activeCompanyId && (
              <SettingRow label="Conectare SPV">
                <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                  {isAnafAuthenticated ? (
                    <Btn
                      variant="danger"
                      size="sm"
                      icon="link"
                      disabled={logoutAnaf.isPending}
                      onClick={() => logoutAnaf.mutate()}
                    >
                      {logoutAnaf.isPending ? "Se deconectează…" : "Deconectează"}
                    </Btn>
                  ) : (
                    <Btn
                      variant="primary"
                      size="sm"
                      icon="shield"
                      disabled={authorizeAnaf.isPending}
                      onClick={() => authorizeAnaf.mutate()}
                    >
                      {authorizeAnaf.isPending ? "Se autorizează…" : "Conectează"}
                    </Btn>
                  )}
                </div>
              </SettingRow>
            )}

            {anafError && (
              <div style={{ margin: "8px 0" }}>
                <Banner variant="error">{anafError}</Banner>
              </div>
            )}

            {/* Configurare avansată — collapsible */}
            <div style={{ paddingTop: 12, borderTop: "1px solid var(--rf-border)", marginTop: 8 }}>
              <button
                type="button"
                className="rf-btn rf-btn--ghost rf-btn--sm"
                onClick={() => setAnafAdvancedOpen((v) => !v)}
              >
                <Icon name="chevDown" size={14} style={{ transform: anafAdvancedOpen ? "rotate(180deg)" : undefined }} />
                Configurare avansată ANAF
              </button>
            </div>

            {anafAdvancedOpen && (
              <div style={{ display: "flex", flexDirection: "column", gap: 12, paddingTop: 14 }}>
                <Banner variant="info">
                  Pentru conectarea la SPV, ANAF cere o aplicație OAuth proprie: în SPV → „Gestionare
                  profil OAuth" înregistrați aplicația (cu Redirect URI-ul de mai jos) și veți primi un
                  <b> Client ID</b> și un <b>Client Secret</b>. Completați-le aici. Restul câmpurilor pot
                  rămâne goale (valori implicite).
                </Banner>
                <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
                  <Field label="Client ID" help="Generat de ANAF la înregistrarea aplicației OAuth">
                    <Input
                      className="rf-mono"
                      placeholder="client_id de la ANAF"
                      value={anafClientId}
                      onChange={(e) => setAnafClientId(e.target.value)}
                    />
                  </Field>
                  <Field
                    label="Client Secret"
                    help={anafHasSecret ? "Salvat în keychain — lăsați gol pentru a-l păstra" : "Generat de ANAF; stocat securizat în keychain"}
                  >
                    <Input
                      type="password"
                      className="rf-mono"
                      placeholder={anafHasSecret ? "•••••••• (configurat)" : "client_secret de la ANAF"}
                      value={anafClientSecret}
                      onChange={(e) => setAnafClientSecret(e.target.value)}
                    />
                  </Field>
                  <Field label="Redirect URI">
                    <Input
                      className="rf-mono"
                      placeholder="http://localhost:8787/callback (implicit)"
                      value={anafRedirectUri}
                      onChange={(e) => setAnafRedirectUri(e.target.value)}
                    />
                  </Field>
                  <Field label="Port callback">
                    <Input
                      className="rf-mono"
                      placeholder="8787 (implicit)"
                      value={anafCallbackPort}
                      onChange={(e) => setAnafCallbackPort(e.target.value)}
                    />
                  </Field>
                  <Field label="URL autorizare">
                    <Input
                      className="rf-mono"
                      placeholder="https://logincert.anaf.ro/anaf-oauth2/v1/authorize"
                      value={anafAuthorizeUrl}
                      onChange={(e) => setAnafAuthorizeUrl(e.target.value)}
                    />
                  </Field>
                  <div style={{ gridColumn: "span 2" }}>
                    <Field label="URL token">
                      <Input
                        className="rf-mono"
                        placeholder="https://logincert.anaf.ro/anaf-oauth2/v1/token"
                        value={anafTokenUrl}
                        onChange={(e) => setAnafTokenUrl(e.target.value)}
                      />
                    </Field>
                  </div>
                </div>
                <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                  <Btn
                    variant="primary"
                    size="sm"
                    disabled={anafAdvancedSaving}
                    onClick={() => void handleSaveAnafAdvanced()}
                  >
                    {anafAdvancedSaving ? "Se salvează…" : "Salvează configurare avansată"}
                  </Btn>
                  {anafAdvancedSaved && (
                    <span style={{ fontSize: 12, color: "var(--rf-success)" }}>
                      <Icon name="checkCircle" size={14} /> Salvat
                    </span>
                  )}
                </div>
              </div>
            )}
          </div>
        </SectionCard>

        {/* ── Integrări (SmartBill) ── */}
        <SectionCard icon="link" title="Integrări" subtitle="Conectați servicii externe">
          <div style={{ padding: "0 24px 16px" }}>
            <div style={{ fontSize: 12, fontWeight: 600, color: "var(--rf-text-muted)", textTransform: "uppercase", letterSpacing: "0.05em", padding: "8px 0 4px" }}>
              SmartBill
            </div>
            <SettingRow label="Utilizator (email)">
              <Input
                type="email"
                placeholder="email@firma.ro"
                value={smartbillUser}
                onChange={(e) => setSmartbillUser(e.target.value)}
                style={{ width: 240 }}
              />
            </SettingRow>
            <SettingRow label="Token API">
              <Input
                type="password"
                placeholder="Token din contul SmartBill"
                className="rf-mono"
                value={smartbillToken}
                onChange={(e) => setSmartbillToken(e.target.value)}
                style={{ width: 240 }}
              />
            </SettingRow>
            <SettingRow label="Documentație" last>
              <span className="rf-text-muted" style={{ fontSize: 12.5 }}>
                SmartBill → Setări → Cont → Token API
              </span>
            </SettingRow>
            <div style={{ paddingTop: 12, display: "flex", gap: 8, alignItems: "center" }}>
              <Btn
                variant="primary"
                size="sm"
                disabled={savingSmartbill || !activeCompanyId}
                onClick={() => void handleSaveSmartbill()}
              >
                {savingSmartbill ? "Se salvează…" : "Salvează credențiale"}
              </Btn>
              {smartbillSaved && (
                <span style={{ fontSize: 12, color: "var(--rf-success)" }}>
                  <Icon name="checkCircle" size={14} /> Salvat
                </span>
              )}
            </div>
          </div>
        </SectionCard>

        {/* ── Notificări ── */}
        <SectionCard icon="bell" title={t("settings.sections.notifications")} subtitle="Configurați tipul și orarul notificărilor">
          <div style={{ padding: "0 24px 16px" }}>
            {NOTIF_TYPES.map(({ key, label }, idx) => (
              <SettingRow
                key={key}
                label={label}
                last={idx === NOTIF_TYPES.length - 1 && (quietHoursSetting === undefined)}
              >
                <Select
                  value={notifPrefMap[key]}
                  onChange={(e) => void handleNotifPrefChange(key, e.target.value)}
                  style={{ width: 180 }}
                >
                  <option value="os">Desktop + In-app</option>
                  <option value="inapp">Doar in-app</option>
                  <option value="off">Dezactivat</option>
                </Select>
              </SettingRow>
            ))}
            <SettingRow label="Ore liniștite" desc="Dezactivează notificările OS între 22:00 și 07:00." last>
              <Toggle
                checked={(quietHoursSetting ?? "0") === "1"}
                onChange={(checked) => void handleNotifToggle("quiet_hours", checked)}
                aria-label="Ore liniștite"
              />
            </SettingRow>
          </div>
        </SectionCard>

        {/* ── Licență ── */}
        <SectionCard
          icon="shield"
          title="Licență"
          actions={
            license ? (
              <Badge variant="info">{TIER_LABELS[license.tier] ?? license.tier}</Badge>
            ) : undefined
          }
        >
          <div style={{ padding: "0 24px 16px" }}>
            {licenseLoading ? (
              <div style={{ padding: "12px 0", color: "var(--rf-text-muted)", fontSize: 13 }}>Se încarcă…</div>
            ) : license ? (
              <>
                <div style={{ paddingTop: 8 }}>
                  <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
                    <span className="rf-mono" style={{ fontSize: 28, fontWeight: 700 }}>
                      {Math.max(0, Math.floor((license.expiresAt - Date.now() / 1000) / 86400))}
                    </span>
                    <span className="rf-text-muted">zile rămase</span>
                  </div>
                  <div className="rf-text-muted" style={{ fontSize: 12.5, marginTop: 4 }}>
                    {license.email ?? ""} · expiră {license.expiresAt ? new Date(license.expiresAt * 1000).toLocaleDateString("ro-RO") : "—"}
                  </div>
                  {license.licenseKey && (
                    <div className="rf-mono rf-text-muted" style={{ fontSize: 11.5, marginTop: 4 }}>
                      {license.licenseKey}
                    </div>
                  )}
                </div>
                <div style={{ marginTop: 14, display: "flex", gap: 8 }}>
                  <Btn variant="secondary" size="sm" onClick={() => { setShowLicenseActivate((v) => !v); setLicenseActivateError(null); }}>
                    Activează cheie
                  </Btn>
                  <Btn variant="ghost" size="sm" onClick={() => void openPurchase()}>
                    Cumpără licență →
                  </Btn>
                </div>
              </>
            ) : (
              <div style={{ padding: "12px 0", fontSize: 13, color: "var(--rf-text-muted)" }}>
                Nicio licență activă. Porniți trial-ul gratuit din meniul Ajutor.
                <div style={{ marginTop: 10 }}>
                  <Btn variant="primary" size="sm" onClick={() => { setShowLicenseActivate(true); setLicenseActivateError(null); }}>
                    Activează cheie
                  </Btn>
                </div>
              </div>
            )}

            {showLicenseActivate && (
              <div style={{ marginTop: 14, padding: 16, background: "var(--rf-accent-tint)", borderRadius: 10, display: "flex", flexDirection: "column", gap: 10 }}>
                <Field label="Cheie licență" required>
                  <Input
                    className="rf-mono"
                    placeholder="XXXX-XXXX-XXXX-XXXX"
                    value={licenseKeyInput}
                    onChange={(e) => setLicenseKeyInput(e.target.value.toUpperCase())}
                    style={{ textTransform: "uppercase" }}
                    autoComplete="off"
                    spellCheck={false}
                  />
                </Field>
                <Field label="Email achiziție" required>
                  <Input
                    type="email"
                    placeholder="office@firma.ro"
                    value={licenseEmailInput}
                    onChange={(e) => setLicenseEmailInput(e.target.value)}
                  />
                </Field>
                {licenseActivateError && <Banner variant="error">{licenseActivateError}</Banner>}
                <div style={{ display: "flex", gap: 8 }}>
                  <Btn
                    variant="primary"
                    size="sm"
                    disabled={licenseActivateMutation.isPending}
                    onClick={() => {
                      setLicenseActivateError(null);
                      if (!licenseKeyInput.trim()) { setLicenseActivateError("Introduceți cheia de licență."); return; }
                      if (!licenseEmailInput.trim()) { setLicenseActivateError("Introduceți emailul de achiziție."); return; }
                      licenseActivateMutation.mutate();
                    }}
                  >
                    {licenseActivateMutation.isPending ? "Se activează…" : "Activează"}
                  </Btn>
                  <Btn variant="ghost" size="sm" onClick={() => { setShowLicenseActivate(false); setLicenseActivateError(null); }}>
                    Anulează
                  </Btn>
                </div>
              </div>
            )}
          </div>
        </SectionCard>

        {/* ── Suport și feedback ── */}
        <SectionCard icon="help" title="Suport și feedback" subtitle="Trimiteți feedback sau raportați o problemă">
          <div style={{ padding: "0 24px 16px" }}>
            <Field label="Mesaj (opțional)" className="rf-field-mt">
              <Textarea
                id={feedbackMsgId}
                rows={4}
                value={feedbackMsg}
                onChange={(e) => setFeedbackMsg(e.target.value)}
                placeholder="Descrieți problema sau sugestia (diagnosticul se atașează automat)…"
              />
            </Field>
            <div style={{ marginTop: 4, padding: "8px 10px", border: "1px dashed var(--rf-border)", borderRadius: 8, fontSize: 12, color: "var(--rf-text-muted)", lineHeight: 1.5 }}>
              <strong>Atașăm automat:</strong> versiunea {appInfo?.version ?? "—"}, sistemul de operare, machine ID anonimizat, ultimele 50 linii log. La click se deschide clientul dvs. de email.
            </div>
            <div style={{ display: "flex", gap: 8, marginTop: 12 }}>
              <Btn
                variant="secondary"
                icon="mail"
                disabled={feedbackSending}
                onClick={() => void sendFeedback()}
              >
                {feedbackSending ? "Pregătesc…" : "Trimite feedback"}
              </Btn>
              <Btn variant="primary" onClick={() => void openPurchase()}>
                Cumpără licență
              </Btn>
            </div>
          </div>
        </SectionCard>

        {/* ── Informații aplicație ── */}
        <SectionCard icon="info" title="Informații aplicație" subtitle="Versiune, director și actualizări">
          <div style={{ padding: "0 24px 16px" }}>
            {appInfoLoading ? (
              <div style={{ padding: "12px 0", color: "var(--rf-text-muted)", fontSize: 13 }}>Se încarcă…</div>
            ) : appInfo ? (
              <>
                <SettingRow label="Versiune">
                  <span className="rf-mono" style={{ fontSize: 13 }}>{appInfo.version}</span>
                </SettingRow>
                <SettingRow label="Director date">
                  <span className="rf-mono" style={{ fontSize: 11.5, wordBreak: "break-all", maxWidth: 320 }}>{appInfo.appDataDir}</span>
                </SettingRow>
                <SettingRow label="Bază de date">
                  <span className="rf-mono" style={{ fontSize: 11.5, wordBreak: "break-all", maxWidth: 320 }}>{appInfo.dbPath}</span>
                </SettingRow>
                <SettingRow label="Actualizări" last>
                  <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                    <Btn
                      variant="secondary"
                      size="sm"
                      icon="refresh"
                      disabled={checkingUpdate}
                      onClick={async () => {
                        setCheckingUpdate(true);
                        setUpdateStatus(null);
                        try {
                          const { check } = await import("@tauri-apps/plugin-updater");
                          const update = await check();
                          if (update?.available) {
                            setUpdateStatus(`Versiune nouă disponibilă: ${update.version}. Descărcați de pe site.`);
                          } else {
                            setUpdateStatus("Aplicația este la zi.");
                          }
                        } catch {
                          setUpdateStatus("Nu s-a putut verifica actualizările (server indisponibil).");
                        } finally {
                          setCheckingUpdate(false);
                        }
                      }}
                    >
                      {checkingUpdate ? "Se verifică…" : "Verifică actualizări"}
                    </Btn>
                    {updateStatus && (
                      <span style={{ fontSize: 12, color: "var(--rf-text-muted)" }}>{updateStatus}</span>
                    )}
                  </div>
                </SettingRow>
              </>
            ) : null}
          </div>
        </SectionCard>

        {/* ── Arhivă (only when company selected) ── */}
        {activeCompanyId && (
          <SectionCard icon="archive" title={t("settings.sections.archive")} subtitle="Export, backup și restaurare date">
            <div style={{ padding: "0 24px 16px" }}>
              <SettingRow label="Dimensiune arhivă">
                <span className="rf-mono" style={{ fontSize: 13 }}>{fmtBytes(archiveSize ?? 0)}</span>
              </SettingRow>

              <SettingRow label="Export arhivă ZIP">
                <Btn
                  variant="secondary"
                  size="sm"
                  icon="download"
                  onClick={async () => {
                    try {
                      const path = await api.archive.exportZip(activeCompanyId);
                      notify.success(`Arhivă exportată: ${path}`);
                    } catch (err) {
                      notify.error(formatError(err, "Exportul arhivei a eșuat."));
                    }
                  }}
                >
                  Export XML + PDF (ZIP)
                </Btn>
              </SettingRow>

              <SettingRow label="Folder arhivă">
                <div style={{ display: "flex", gap: 6 }}>
                  <Btn
                    variant="secondary"
                    size="sm"
                    icon="database"
                    onClick={() => {
                      api.system.openArchiveFolder().catch((e) =>
                        notify.error(formatError(e, "Nu s-a putut deschide folderul arhivei."))
                      );
                    }}
                  >
                    Deschide folder
                  </Btn>
                  <Btn
                    variant="ghost"
                    size="sm"
                    icon="folder"
                    onClick={async () => {
                      const dir = await open({ directory: true, title: "Selectează noua locație arhivă" });
                      if (dir && typeof dir === "string") {
                        const ok = await confirm(
                          `Schimbi locația arhivei în:\n${dir}\n\nFișierele existente vor fi copiate. Continuați?`,
                          { title: "Schimbare locație arhivă", kind: "warning" }
                        );
                        if (ok) {
                          await api.archive.changeArchiveLocation(dir);
                          notify.success("Locație arhivă schimbată cu succes.");
                        }
                      }
                    }}
                  >
                    Schimbă locația
                  </Btn>
                </div>
              </SettingRow>

              <SettingRow label="Backup complet">
                <Btn
                  variant="secondary"
                  size="sm"
                  icon="download"
                  onClick={async () => {
                    try {
                      const defaultName = `efactura_backup_${new Date().toISOString().slice(0, 10).replace(/-/g, "")}.zip`;
                      const destPath = await save({
                        filters: [{ name: "ZIP", extensions: ["zip"] }],
                        defaultPath: defaultName,
                      });
                      if (!destPath) return;
                      const path = await api.system.exportBackup(destPath as string);
                      notify.success(`Backup salvat: ${path}`);
                    } catch (e) {
                      notify.error(formatError(e, "Exportul backup-ului a eșuat."));
                    }
                  }}
                >
                  Exportă backup (DB + arhivă)
                </Btn>
              </SettingRow>

              <SettingRow label="Verificare integritate">
                <Btn
                  variant="secondary"
                  size="sm"
                  icon="checkCircle"
                  onClick={async () => {
                    try {
                      const result = await api.archive.verifyIntegrity();
                      if (result.ok) {
                        notify.success(`Arhiva este integră. ${result.checked} fișiere verificate.`);
                      } else {
                        notify.error(
                          `Fișiere lipsă (${result.missing.length} din ${result.checked}): ` +
                          result.missing.slice(0, 5).join(", ") +
                          (result.missing.length > 5 ? " …" : "")
                        );
                      }
                    } catch (e) {
                      notify.error(formatError(e, "Verificarea integrității a eșuat."));
                    }
                  }}
                >
                  Verifică integritate arhivă
                </Btn>
              </SettingRow>

              <SettingRow label="Restaurare backup" last>
                <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                  <Btn
                    variant="secondary"
                    size="sm"
                    icon="upload"
                    onClick={async () => {
                      try {
                        const file = await open({
                          filters: [{ name: "ZIP", extensions: ["zip"] }],
                        });
                        if (file) {
                          const ok = await confirm(
                            "Aceasta va înlocui baza de date curentă cu backup-ul selectat. Operațiunea nu poate fi anulată.",
                            { title: "⚠️ Restaurare bază de date", kind: "warning" }
                          );
                          if (ok) {
                            await api.archive.importBackup(file as string);
                          }
                        }
                      } catch (e) {
                        notify.error(formatError(e, "Restaurarea backup-ului a eșuat."));
                      }
                    }}
                  >
                    Selectează backup ZIP
                  </Btn>
                  <Banner variant="warning">
                    Restaurarea va reporni aplicația și va înlocui toate datele curente.
                  </Banner>
                </div>
              </SettingRow>
            </div>
          </SectionCard>
        )}

        {/* ── Confidențialitate (GDPR) ── */}
        <SectionCard icon="shield" title="Confidențialitate (GDPR)" subtitle="Gestionați datele personale stocate local">
          <div style={{ padding: "0 24px 16px" }}>
            <Banner variant="info" className="rf-banner-mt">
              Gestionați datele personale stocate local. Operațiunile de ștergere sunt ireversibile.
            </Banner>
            <div style={{ marginTop: 14, display: "flex", flexDirection: "column", gap: 8 }}>
              <Btn
                variant="secondary"
                icon="download"
                block
                onClick={async () => {
                  try {
                    const dest = await save({
                      defaultPath: `rofactura-date-${new Date().toISOString().slice(0, 10)}.zip`,
                      filters: [{ name: "ZIP", extensions: ["zip"] }],
                      title: "Alegeți locul pentru exportul datelor dvs.",
                    });
                    if (!dest) return;
                    const result = await api.gdpr.exportAll(dest);
                    notify.success(`Date exportate cu succes: ${result.path}`);
                  } catch (e) {
                    notify.error(formatError(e, "Exportul datelor a eșuat."));
                  }
                }}
              >
                Exportă toate datele mele (ZIP)
              </Btn>
              <Btn
                variant="secondary"
                block
                onClick={async () => {
                  try {
                    await openUrl("https://lucaris.ro/privacy");
                  } catch { /* ignore */ }
                }}
              >
                lucaris.ro/privacy →
              </Btn>
              <Btn
                variant="danger"
                icon="trash"
                block
                onClick={async () => {
                  const step1 = await confirm(
                    "Această acțiune va șterge ireversibil TOATE datele dvs. din aplicație:\n" +
                    "• Toate facturile, companiile și contactele\n" +
                    "• Toate fișierele XML și PDF din arhivă\n" +
                    "• Toate setările și licența\n\n" +
                    "Datele NU pot fi recuperate după această operațiune.\n\nDoriți să continuați?",
                    { title: "Atenție: Ștergere toate datele", kind: "warning" }
                  );
                  if (!step1) return;

                  const step2 = await confirm(
                    "Confirmare finală: sunteți absolut sigur că doriți să ștergeți TOATE datele dvs.?\n\n" +
                    "Această operațiune este ireversibilă.",
                    { title: "Confirmare finală ștergere date", kind: "warning" }
                  );
                  if (!step2) return;

                  try {
                    await api.gdpr.wipeAll();
                    notify.success("Toate datele dvs. au fost șterse. Aplicația va reporni.");
                    setTimeout(() => { window.location.reload(); }, 2000);
                  } catch (e) {
                    notify.error(formatError(e, "Ștergerea datelor a eșuat."));
                  }
                }}
              >
                Șterge toate datele (ireversibil)
              </Btn>
            </div>
          </div>
        </SectionCard>

        {/* ── Sistem ── */}
        <SectionCard icon="settings" title={t("settings.sections.system")} subtitle="Opțiuni de sistem și pornire automată">
          <div style={{ padding: "0 24px 16px" }}>
            <SettingRow label="Pornire automată la login" desc="Pornește aplicația automat la autentificarea în sistem." last>
              <Toggle
                checked={autostartEnabled ?? false}
                onChange={async (checked) => {
                  try {
                    await api.system.setAutostart(checked);
                    void queryClient.invalidateQueries({ queryKey: queryKeys.system.autostart });
                  } catch (err) {
                    notify.error(formatError(err, "Nu s-a putut modifica setarea de pornire automată."));
                  }
                }}
                aria-label="Pornire automată"
              />
            </SettingRow>
          </div>
        </SectionCard>

        {/* ── Jurnal activitate ── */}
        <SectionCard icon="list" title={t("settings.sections.activityLog")} subtitle="Ultimele operațiuni înregistrate">
          {activityLog.length === 0 ? (
            <div style={{ padding: "12px 24px", fontSize: 13, color: "var(--rf-text-muted)" }}>
              Nicio activitate înregistrată.
            </div>
          ) : (
            <div style={{ overflowX: "auto" }}>
              <table className="rf-tbl">
                <thead>
                  <tr>
                    <th style={{ width: 160 }}>Timp</th>
                    <th>Sarcină</th>
                    <th>Rezultat</th>
                  </tr>
                </thead>
                <tbody>
                  {activityLog.slice(0, 20).map((entry) => (
                    <tr key={entry.id}>
                      <td className="rf-mono rf-text-muted" style={{ fontSize: 11.5 }}>
                        {new Date(entry.createdAt * 1000).toLocaleString("ro-RO")}
                      </td>
                      <td style={{ fontSize: 13 }}>{entry.entityId || <span style={{ color: "var(--rf-text-muted)" }}>—</span>}</td>
                      <td style={{ fontSize: 12, color: "var(--rf-text-muted)" }}>
                        {entry.metadata || <span style={{ color: "var(--rf-text-muted)" }}>—</span>}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
          <div style={{ display: "flex", justifyContent: "flex-end", padding: "8px 24px 12px" }}>
            <Btn
              variant="ghost"
              size="sm"
              icon="download"
              onClick={async () => {
                const csv = await api.system.exportActivityLogCsv();
                const blob = new Blob([csv], { type: "text/csv" });
                const url = URL.createObjectURL(blob);
                const a = document.createElement("a");
                a.href = url;
                a.download = "jurnal-activitate.csv";
                a.click();
                URL.revokeObjectURL(url);
              }}
            >
              Export CSV
            </Btn>
          </div>
        </SectionCard>

        {/* ── Development (DEV only) ── */}
        {import.meta.env.DEV && (
          <SectionCard icon="code" title="Dezvoltare">
            <div style={{ padding: "12px 24px" }}>
              <Btn variant="secondary" size="sm" onClick={() => void handleDevSeed()}>
                Populează DB cu date demo
              </Btn>
            </div>
          </SectionCard>
        )}
      </div>
    </>
  );
}
