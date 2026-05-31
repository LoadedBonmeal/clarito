/**
 * Setări aplicație — temă, companie activă, ANAF, licență, informații sistem.
 */

import { useQuery, useQueries, useMutation, useQueryClient } from "@tanstack/react-query";
import { useState, useEffect, useId } from "react";
import { open, save, confirm } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";

import { Skeleton } from "@/components/ui/skeleton";
import { Section, FieldRow, FieldGroup } from "@/components/shared/Section";
import { PageContent, PageHeader } from "@/components/shared/PageHeader";
import { Icon } from "@/components/shared/Icon";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore, type ThemeMode } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { Company } from "@/types";

function fmtBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

const THEME_OPTIONS: { value: ThemeMode; label: string }[] = [
  { value: "light", label: "Luminos" },
  { value: "dark", label: "Întunecat" },
  { value: "system", label: "Sistem (automat)" },
];

export function SettingsPage() {
  const queryClient = useQueryClient();
  const { t } = useTranslation();
  const theme = useAppStore((s) => s.theme);
  const setTheme = useAppStore((s) => s.setTheme);
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const setActiveCompanyId = useAppStore((s) => s.setActiveCompanyId);

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

  // Advanced ANAF OAuth config
  const [anafAdvancedOpen, setAnafAdvancedOpen] = useState(false);
  const [anafClientId, setAnafClientId] = useState("");
  const [anafRedirectUri, setAnafRedirectUri] = useState("");
  const [anafCallbackPort, setAnafCallbackPort] = useState("");
  const [anafAuthorizeUrl, setAnafAuthorizeUrl] = useState("");
  const [anafTokenUrl, setAnafTokenUrl] = useState("");
  const [anafAdvancedSaving, setAnafAdvancedSaving] = useState(false);
  const [anafAdvancedSaved, setAnafAdvancedSaved] = useState(false);

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
    })();
  }, []);

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
      setAnafAdvancedSaved(true);
      setTimeout(() => setAnafAdvancedSaved(false), 3000);
    } catch (e) {
      notify.error(formatError(e, "Eroare la salvarea configurației ANAF avansate."));
    } finally {
      setAnafAdvancedSaving(false);
    }
  };

  // Notification preferences: per-type ("os" | "inapp" | "off") + quiet hours
  const NOTIF_TYPES = [
    { key: "validated",     label: "Factură validată ANAF" },
    { key: "rejected",      label: "Factură respinsă ANAF" },
    { key: "received",      label: "Facturi noi primite SPV" },
    { key: "cert_expiring", label: "Certificat SPV expiră" },
    { key: "cert_expired",  label: "Certificat SPV expirat" },
  ];

  const NOTIF_KEYS = ["validated", "rejected", "received", "cert_expiring", "cert_expired"] as const;
  type NotifKey = typeof NOTIF_KEYS[number];

  const notifResults = useQueries({
    queries: NOTIF_KEYS.map((key) => ({
      queryKey: queryKeys.settings.get(`notif_pref_${key}`),
      queryFn: () => api.settings.get(`notif_pref_${key}`),
    })),
  });

  const notifPrefMap = Object.fromEntries(
    NOTIF_KEYS.map((key, i) => [key, notifResults[i].data ?? "os"])
  ) as Record<NotifKey, string>;

  const notifPrefs = NOTIF_KEYS.map((key) => ({ key, pref: notifPrefMap[key] }));

  const { data: quietHoursSetting } = useQuery({
    queryKey: queryKeys.settings.get("quiet_hours"),
    queryFn: () => api.settings.get("quiet_hours"),
  });

  const handleNotifToggle = async (key: string, checked: boolean) => {
    await api.settings.set(key, checked ? "1" : "0");
    void queryClient.invalidateQueries({ queryKey: queryKeys.settings.get(key) });
  };

  const { data: isAnafAuthenticated, refetch: refetchAnafAuth } = useQuery({
    queryKey: queryKeys.anaf.auth(activeCompanyId ?? ""),
    queryFn: () => api.anaf.isAuthenticated(activeCompanyId!),
    enabled: !!activeCompanyId,
  });

  const [anafError, setAnafError] = useState<string | null>(null);
  const [updateStatus, setUpdateStatus] = useState<string | null>(null);
  const [checkingUpdate, setCheckingUpdate] = useState(false);

  const [smartbillUser, setSmartbillUser] = useState("");
  const [smartbillToken, setSmartbillToken] = useState("");
  const [savingSmartbill, setSavingSmartbill] = useState(false);
  const [smartbillSaved, setSmartbillSaved] = useState(false);

  useEffect(() => {
    if (!activeCompanyId) return;
    api.integrations.getSmartbillCredentials(activeCompanyId)
      .then((creds) => {
        setSmartbillUser(creds.user);
        setSmartbillToken(creds.configured ? "••••••••" : "");
      })
      .catch(() => {});
  }, [activeCompanyId]);

  const handleSaveSmartbill = async () => {
    if (!activeCompanyId) return;
    setSavingSmartbill(true);
    try {
      // Save username + token via the keychain-backed command (set_smartbill_credentials).
      // Token is stored in the OS keychain, NOT in the plaintext settings DB.
      // If the token field still shows the placeholder mask ("••••…"), omit it so
      // only the username is updated and the existing keychain token is preserved.
      const tokenToSave =
        smartbillToken && !smartbillToken.startsWith("•") ? smartbillToken : undefined;
      await invoke("set_smartbill_credentials", {
        companyId: activeCompanyId,
        user: smartbillUser,
        token: tokenToSave ?? null,
      });

      // Scrub any legacy plaintext token that may have been written to the settings DB
      // by an older version of this handler.  Best-effort: ignore errors.
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

  const authorizeAnaf = useMutation({
    mutationFn: () => api.anaf.authorize(activeCompanyId!),
    onSuccess: () => { void refetchAnafAuth(); setAnafError(null); },
    onError: (e) => setAnafError(formatError(e, "Eroare autorizare ANAF.")),
  });

  const logoutAnaf = useMutation({
    mutationFn: () => api.anaf.logout(activeCompanyId!),
    onSuccess: () => { void refetchAnafAuth(); setAnafError(null); },
  });

  const handleTestModeToggle = async (e: React.ChangeEvent<HTMLInputElement>) => {
    await api.settings.set("use_anaf_test_env", e.target.checked ? "1" : "0");
    void queryClient.invalidateQueries({ queryKey: queryKeys.anaf.testMode });
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

  const tierLabels: Record<string, string> = {
    TRIAL: "Probă gratuită",
    SOLO: "Solo",
    ACCOUNTANT: "Contabil",
    FIRM: "Firmă",
  };

  // Feedback section state
  const feedbackMsgId = useId();
  const [feedbackMsg, setFeedbackMsg] = useState("");
  const [feedbackSending, setFeedbackSending] = useState(false);

  const sendFeedback = async () => {
    setFeedbackSending(true);
    try {
      const report = await api.feedback.gather();
      const url = await api.feedback.mailto(report, feedbackMsg || undefined);
      await openUrl(url);
      notify.success("Email pregătit în clientul dvs. de email");
    } catch (e) {
      notify.error(
        formatError(
          e,
          "Nu pot deschide clientul de email — trimite manual la support@lucaris.ro",
        ),
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

  return (
    <>
      <PageHeader title={t('settings.title')} />
      <PageContent className="space-y-3" style={{ flex: 1, overflowY: "auto" }}>
        <div style={{ maxWidth: 680, display: "flex", flexDirection: "column", gap: 12 }}>

          {/* Aspect */}
          <Section title="Aspect">
            <FieldGroup>
              <FieldRow label="Temă interfață">
                <div className="seg" style={{ fontSize: 11 }}>
                  {THEME_OPTIONS.map((opt) => (
                    <span
                      key={opt.value}
                      className={"seg-item " + (theme === opt.value ? "active" : "")}
                      onClick={() => setTheme(opt.value)}
                    >
                      {opt.label}
                    </span>
                  ))}
                </div>
              </FieldRow>
            </FieldGroup>
          </Section>

          {/* Companie activă */}
          <Section title="Companie activă">
            <FieldGroup>
              <FieldRow label="Companie curentă">
                {companies.length === 0 ? (
                  <span className="muted" style={{ fontSize: 11 }}>Nicio companie configurată</span>
                ) : (
                  <select
                    className="field"
                    style={{ width: 280, fontSize: 11 }}
                    value={activeCompanyId ?? ""}
                    onChange={(e) => setActiveCompanyId(e.target.value || null)}
                  >
                    <option value="">— Selectați compania —</option>
                    {companies.map((c: Company) => (
                      <option key={c.id} value={c.id}>
                        {c.legalName} ({c.cui})
                      </option>
                    ))}
                  </select>
                )}
              </FieldRow>
              {activeCompanyId && (() => {
                const ac = companies.find((c) => c.id === activeCompanyId);
                if (!ac) return null;
                return (
                  <>
                    <FieldRow label="CUI" mono>{ac.cui}</FieldRow>
                    <FieldRow label="Serie facturi" mono>{ac.invoiceSeries}</FieldRow>
                    <FieldRow label="Plătitor TVA">{ac.vatPayer ? "Da" : "Nu"}</FieldRow>
                    <FieldRow label="SPV activat">{ac.spvEnabled ? "Da" : "Nu"}</FieldRow>
                  </>
                );
              })()}
            </FieldGroup>
          </Section>

          {/* ANAF */}
          <Section title="ANAF / SPV">
            <FieldGroup>
              <FieldRow label="Mediu ANAF">
                <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                  <span
                    style={{
                      fontSize: 11,
                      fontWeight: 600,
                      padding: "2px 8px",
                      borderRadius: 3,
                      background: anafTestMode ? "#FEF3C7" : "#D1FAE5",
                      color: anafTestMode ? "#92400E" : "#065F46",
                      border: `1px solid ${anafTestMode ? "#FCD34D" : "#A7F3D0"}`,
                    }}
                  >
                    {anafTestMode ? "Test" : "Producție"}
                  </span>
                  <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                    <input
                      id="anaf-test-mode"
                      type="checkbox"
                      className="cbx"
                      checked={anafTestMode}
                      onChange={handleTestModeToggle}
                    />
                    <label htmlFor="anaf-test-mode" style={{ fontSize: 11, cursor: "pointer" }}>
                      Mod test ANAF (API facturare test)
                    </label>
                  </div>
                </div>
              </FieldRow>
              <FieldRow label="URL API ANAF" mono>
                <span style={{ fontSize: 10.5 }}>
                  {anafTestMode
                    ? "https://api.anaf.ro/test/FCTEL/rest/"
                    : "https://api.anaf.ro/prod/FCTEL/rest/"}
                </span>
              </FieldRow>
              {activeCompanyId && (
                <FieldRow label="Conectare SPV ANAF">
                  <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                    <div style={{ fontSize: 10.5, color: "var(--text-muted)", lineHeight: 1.5 }}>
                      Pentru conectare aveți nevoie de un <strong>certificat digital calificat</strong> instalat
                      în browser (token USB sau soft-cert). Clientul OAuth trebuie înregistrat ca SPV la ANAF
                      (client_id — configurabil în secțiunea avansată de mai jos).
                    </div>
                    <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                      {isAnafAuthenticated ? (
                        <>
                          <span style={{ fontSize: 11, color: "#16A34A", fontWeight: 600 }}>✓ Conectat la SPV ANAF</span>
                          <button
                            type="button"
                            className="btn"
                            disabled={logoutAnaf.isPending}
                            onClick={() => logoutAnaf.mutate()}
                          >
                            Deconectare
                          </button>
                        </>
                      ) : (
                        <>
                          <span style={{ fontSize: 11, color: "var(--text-muted)" }}>Neconectat</span>
                          <button
                            type="button"
                            className="btn primary"
                            disabled={authorizeAnaf.isPending}
                            onClick={() => authorizeAnaf.mutate()}
                          >
                            {authorizeAnaf.isPending ? "Se autorizează…" : "Conectează la SPV ANAF →"}
                          </button>
                        </>
                      )}
                    </div>
                  </div>
                </FieldRow>
              )}
              {anafError && (
                <FieldRow label="">
                  <div
                    style={{
                      padding: "7px 10px",
                      background: "#FEE2E2",
                      border: "1px solid #FECACA",
                      fontSize: 11,
                      color: "#991B1B",
                      lineHeight: 1.5,
                    }}
                  >
                    {anafError}
                  </div>
                </FieldRow>
              )}
              {/* Configurare avansată ANAF — collapsible */}
              <FieldRow label="">
                <button
                  type="button"
                  className="btn compact"
                  onClick={() => setAnafAdvancedOpen((v) => !v)}
                  style={{ fontSize: 10.5 }}
                >
                  {anafAdvancedOpen ? "▲ Ascunde configurare avansată ANAF" : "▼ Configurare avansată ANAF"}
                </button>
              </FieldRow>
              {anafAdvancedOpen && (
                <>
                  <FieldRow label="">
                    <div style={{ fontSize: 10.5, color: "var(--text-muted)", lineHeight: 1.5, marginBottom: 4 }}>
                      Lăsați gol pentru valorile implicite. Modificați doar dacă știți ce faceți
                      (ex. mediu de test OAuth, client_id propriu înregistrat la ANAF).
                    </div>
                  </FieldRow>
                  <FieldRow label="Client ID">
                    <input
                      className="field"
                      style={{ width: 280, fontSize: 10.5, fontFamily: "var(--font-mono)" }}
                      placeholder="efactura-desktop (implicit)"
                      value={anafClientId}
                      onChange={(e) => setAnafClientId(e.target.value)}
                    />
                  </FieldRow>
                  <FieldRow label="Redirect URI">
                    <input
                      className="field"
                      style={{ width: 280, fontSize: 10.5, fontFamily: "var(--font-mono)" }}
                      placeholder="http://localhost:8787/callback (implicit)"
                      value={anafRedirectUri}
                      onChange={(e) => setAnafRedirectUri(e.target.value)}
                    />
                  </FieldRow>
                  <FieldRow label="Port callback">
                    <input
                      className="field"
                      style={{ width: 120, fontSize: 10.5, fontFamily: "var(--font-mono)" }}
                      placeholder="8787 (implicit)"
                      value={anafCallbackPort}
                      onChange={(e) => setAnafCallbackPort(e.target.value)}
                    />
                  </FieldRow>
                  <FieldRow label="URL autorizare">
                    <input
                      className="field"
                      style={{ width: 280, fontSize: 10.5, fontFamily: "var(--font-mono)" }}
                      placeholder="https://logincert.anaf.ro/anaf-oauth2-server/authorize"
                      value={anafAuthorizeUrl}
                      onChange={(e) => setAnafAuthorizeUrl(e.target.value)}
                    />
                  </FieldRow>
                  <FieldRow label="URL token">
                    <input
                      className="field"
                      style={{ width: 280, fontSize: 10.5, fontFamily: "var(--font-mono)" }}
                      placeholder="https://logincert.anaf.ro/anaf-oauth2-server/token"
                      value={anafTokenUrl}
                      onChange={(e) => setAnafTokenUrl(e.target.value)}
                    />
                  </FieldRow>
                  <FieldRow label="">
                    <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                      <button
                        type="button"
                        className="btn primary"
                        disabled={anafAdvancedSaving}
                        onClick={() => void handleSaveAnafAdvanced()}
                      >
                        {anafAdvancedSaving ? "Se salvează…" : "Salvează configurare avansată"}
                      </button>
                      {anafAdvancedSaved && (
                        <span style={{ fontSize: 11, color: "#16A34A" }}>✓ Salvat</span>
                      )}
                    </div>
                  </FieldRow>
                </>
              )}
            </FieldGroup>
          </Section>

          {/* Integrări contabile */}
          <Section title="Integrări contabile">
            <FieldGroup>
              <div style={{ padding: "8px 14px 4px", fontSize: 11, fontWeight: 600, color: "var(--text-muted)", textTransform: "uppercase", letterSpacing: "0.05em" }}>
                SmartBill
              </div>
              <FieldRow label="Utilizator (email)">
                <input
                  className="field"
                  style={{ width: 240, fontSize: 11 }}
                  type="email"
                  placeholder="email@firma.ro"
                  value={smartbillUser}
                  onChange={(e) => setSmartbillUser(e.target.value)}
                />
              </FieldRow>
              <FieldRow label="Token API">
                <input
                  className="field"
                  style={{ width: 240, fontSize: 11, fontFamily: "var(--font-mono)" }}
                  type="password"
                  placeholder="Token din contul SmartBill"
                  value={smartbillToken}
                  onChange={(e) => setSmartbillToken(e.target.value)}
                />
              </FieldRow>
              <FieldRow label="">
                <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                  <button
                    type="button"
                    className="btn primary"
                    disabled={savingSmartbill || !activeCompanyId}
                    onClick={handleSaveSmartbill}
                  >
                    {savingSmartbill ? "Se salvează…" : "Salvează credențiale"}
                  </button>
                  {smartbillSaved && (
                    <span style={{ fontSize: 11, color: "#16A34A" }}>✓ Salvat</span>
                  )}
                </div>
              </FieldRow>
              <FieldRow label="Documentație">
                <span style={{ fontSize: 11, color: "var(--text-muted)" }}>
                  Obțineți tokenul din SmartBill → Setări → Cont → Token API
                </span>
              </FieldRow>
            </FieldGroup>
          </Section>

          {/* Notificări */}
          <Section title={t('settings.sections.notifications')}>
            <FieldGroup>
              {notifPrefs.map(({ key, pref }) => {
                const label = NOTIF_TYPES.find((t) => t.key === key)?.label ?? key;
                return (
                  <FieldRow key={key} label={label}>
                    <select
                      className="input"
                      style={{ fontSize: 11, width: 160 }}
                      value={pref}
                      onChange={async (e) => {
                        await api.settings.set(`notif_pref_${key}`, e.target.value);
                        void queryClient.invalidateQueries({ queryKey: queryKeys.settings.get(`notif_pref_${key}`) });
                      }}
                    >
                      <option value="os">Desktop + In-app</option>
                      <option value="inapp">Doar in-app</option>
                      <option value="off">Dezactivat</option>
                    </select>
                  </FieldRow>
                );
              })}
              <FieldRow label="Ore liniștite">
                <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                  <input
                    id="quiet-hours"
                    type="checkbox"
                    className="cbx"
                    checked={(quietHoursSetting ?? "0") === "1"}
                    onChange={(e) => void handleNotifToggle("quiet_hours", e.target.checked)}
                  />
                  <label htmlFor="quiet-hours" style={{ fontSize: 11, cursor: "pointer" }}>
                    Ore liniștite (22:00–07:00)
                  </label>
                </div>
              </FieldRow>
            </FieldGroup>
          </Section>

          {/* Licență */}
          <Section title="Licență">
            {licenseLoading ? (
              <Skeleton className="m-3 h-16" />
            ) : license ? (
              <FieldGroup>
                <FieldRow label="Tip">{tierLabels[license.tier] ?? license.tier}</FieldRow>
                <FieldRow label="Email" mono>{license.email ?? "—"}</FieldRow>
                <FieldRow label="Expiră">
                  {new Date(license.expiresAt * 1000).toLocaleDateString("ro-RO")}
                </FieldRow>
                <FieldRow label="Cheie licență" mono>
                  {license.licenseKey ? (
                    <span style={{ fontSize: 10.5 }}>{license.licenseKey}</span>
                  ) : (
                    <span className="muted">—</span>
                  )}
                </FieldRow>
              </FieldGroup>
            ) : (
              <div style={{ padding: "12px 14px", fontSize: 11, color: "var(--text-muted)" }}>
                Nicio licență activă. Porniți trial-ul gratuit din meniul Ajutor.
              </div>
            )}
          </Section>

          {/* Suport și feedback */}
          <Section
            title="Suport și feedback"
            highlight
            badge="NOU"
          >
            <FieldGroup>
              <FieldRow label="Mesajul dvs. (opțional)" htmlFor={feedbackMsgId}>
                <textarea
                  id={feedbackMsgId}
                  className="input"
                  rows={4}
                  value={feedbackMsg}
                  onChange={(e) => setFeedbackMsg(e.target.value)}
                  placeholder="Descrie problema sau sugestia (nu e obligatoriu — diagnosticul se atașează automat)..."
                  style={{ width: "100%", resize: "vertical", fontSize: 11, fontFamily: "var(--font-ui)" }}
                />
              </FieldRow>
              <FieldRow label="">
                <div style={{ display: "flex", gap: 8 }}>
                  <button
                    type="button"
                    className="btn primary"
                    onClick={() => void sendFeedback()}
                    disabled={feedbackSending}
                  >
                    <Icon name="mail" size={12} />
                    {feedbackSending ? "Pregătesc…" : "Trimite feedback prin email"}
                  </button>
                  <button type="button" className="btn" onClick={() => void openPurchase()}>
                    <Icon name="arrowRight" size={12} />
                    Cumpără licență →
                  </button>
                </div>
              </FieldRow>
              <FieldRow label="">
                <div
                  style={{
                    padding: "8px 10px",
                    border: "1px dashed var(--border)",
                    fontSize: 11,
                    color: "var(--text-muted)",
                    lineHeight: 1.5,
                  }}
                >
                  <strong>Atașăm automat:</strong> versiunea{" "}
                  {appInfo?.version ?? "0.2.0"}, sistemul de operare, machine ID
                  anonimizat, ultimele 50 linii log. La click se deschide clientul
                  dvs. de email — nu trimitem nimic fără dumneavoastră.
                </div>
              </FieldRow>
            </FieldGroup>
          </Section>

          {/* Informații aplicație */}
          <Section title="Informații aplicație">
            {appInfoLoading ? (
              <Skeleton className="m-3 h-20" />
            ) : appInfo ? (
              <FieldGroup>
                <FieldRow label="Versiune" mono>{appInfo.version}</FieldRow>
                <FieldRow label="Director date" mono>
                  <span style={{ fontSize: 10.5, wordBreak: "break-all" }}>{appInfo.appDataDir}</span>
                </FieldRow>
                <FieldRow label="Bază de date" mono>
                  <span style={{ fontSize: 10.5, wordBreak: "break-all" }}>{appInfo.dbPath}</span>
                </FieldRow>
                <FieldRow label="Actualizări">
                  <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                    <button
                      type="button"
                      className="btn"
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
                      <Icon name="refresh" size={12} />
                      {checkingUpdate ? "Se verifică…" : "Verifică actualizări"}
                    </button>
                    {updateStatus && (
                      <span style={{ fontSize: 11, color: "var(--text-muted)" }}>{updateStatus}</span>
                    )}
                  </div>
                </FieldRow>
              </FieldGroup>
            ) : null}
          </Section>

          {/* Date & Export */}
          {activeCompanyId && (
            <Section title={t('settings.sections.archive')}>
              <FieldGroup>
                <FieldRow label="Dimensiune arhivă">
                  <span style={{ fontSize: 11, fontFamily: "var(--font-mono)" }}>
                    {fmtBytes(archiveSize ?? 0)}
                  </span>
                </FieldRow>
                <FieldRow label="Export arhivă ZIP">
                  <button
                    type="button"
                    className="btn"
                    onClick={async () => {
                      try {
                        const path = await api.archive.exportZip(activeCompanyId);
                        notify.success(`Arhivă exportată: ${path}`);
                      } catch (err) {
                        notify.error(formatError(err, 'Exportul arhivei a eșuat.'));
                      }
                    }}
                  >
                    Export XML + PDF (ZIP)
                  </button>
                </FieldRow>
                <FieldRow label="Folder arhivă">
                  <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                    <button
                      type="button"
                      className="btn"
                      onClick={() => {
                        api.system.openArchiveFolder().catch((e) => notify.error(formatError(e, 'Nu s-a putut deschide folderul arhivei.')));
                      }}
                    >
                      <Icon name="database" size={12} /> Deschide folder arhivă
                    </button>
                    <button
                      type="button"
                      className="btn compact"
                      onClick={async () => {
                        const dir = await open({ directory: true, title: "Selectează noua locație arhivă" });
                        if (dir && typeof dir === "string") {
                          const ok = await confirm(`Schimbi locația arhivei în:\n${dir}\n\nFișierele existente vor fi copiate. Continuați?`, {
                            title: "Schimbare locație arhivă",
                            kind: "warning",
                          });
                          if (ok) {
                            await api.archive.changeArchiveLocation(dir);
                            notify.success("Locație arhivă schimbată cu succes.");
                          }
                        }
                      }}
                    >
                      <Icon name="folder" size={11} /> Schimbă locația arhivei
                    </button>
                  </div>
                </FieldRow>
                <FieldRow label="Backup complet">
                  <button
                    type="button"
                    className="btn"
                    onClick={async () => {
                      try {
                        const path = await api.system.exportBackup();
                        notify.success(`Backup salvat: ${path}`);
                      } catch (e) {
                        notify.error(formatError(e, 'Exportul backup-ului a eșuat.'));
                      }
                    }}
                  >
                    <Icon name="download" size={12} /> Exportă backup (DB + arhivă)
                  </button>
                </FieldRow>
                <FieldRow label="Verificare integritate">
                  <button
                    type="button"
                    className="btn"
                    onClick={async () => {
                      try {
                        const result = await api.archive.verifyIntegrity();
                        if (result.ok) {
                          notify.success(`Arhiva este integră. ${result.totalChecked} fișiere verificate.`);
                        } else {
                          notify.error(
                            `Fișiere lipsă (${result.missingFiles.length} din ${result.totalChecked}): ` +
                            result.missingFiles.slice(0, 5).join(", ") +
                            (result.missingFiles.length > 5 ? " …" : "")
                          );
                        }
                      } catch (e) {
                        notify.error(formatError(e, 'Verificarea integrității a eșuat.'));
                      }
                    }}
                  >
                    Verifică integritate arhivă
                  </button>
                </FieldRow>
                <FieldRow label="Restaurare backup">
                  <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                    <button
                      type="button"
                      className="btn"
                      onClick={async () => {
                        try {
                          const file = await open({
                            filters: [{ name: "ZIP", extensions: ["zip"] }],
                          });
                          if (file) {
                            const ok = await confirm("Aceasta va înlocui baza de date curentă cu backup-ul selectat. Operațiunea nu poate fi anulată.", {
                              title: "⚠️ Restaurare bază de date",
                              kind: "warning",
                            });
                            if (ok) {
                              await api.archive.importBackup(file as string);
                            }
                          }
                        } catch (e) {
                          notify.error(formatError(e, 'Restaurarea backup-ului a eșuat.'));
                        }
                      }}
                    >
                      Selectează backup ZIP
                    </button>
                    <span style={{ fontSize: 11, color: "#DC2626" }}>
                      ⚠️ Restaurarea va reporni aplicația și va înlocui toate datele curente.
                    </span>
                  </div>
                </FieldRow>
              </FieldGroup>
            </Section>
          )}

          {/* Confidențialitate (GDPR) */}
          <Section title="Confidențialitate (GDPR)">
            <FieldGroup>
              <FieldRow label="Exportați datele dvs.">
                <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                  <button
                    type="button"
                    className="btn"
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
                    <Icon name="download" size={12} /> Exportă toate datele mele (ZIP)
                  </button>
                  <span style={{ fontSize: 11, color: "var(--text-muted)" }}>
                    Exportă baza de date și toate fișierele XML + PDF într-un singur arhivă ZIP.
                  </span>
                </div>
              </FieldRow>
              <FieldRow label="Politică de confidențialitate">
                <button
                  type="button"
                  className="btn"
                  style={{ fontFamily: "var(--font-ui)" }}
                  onClick={async () => {
                    try {
                      await openUrl("https://lucaris.ro/privacy");
                    } catch {
                      /* ignore */
                    }
                  }}
                >
                  lucaris.ro/privacy →
                </button>
              </FieldRow>
              <FieldRow label="Ștergeți toate datele">
                <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                  <button
                    type="button"
                    className="btn"
                    style={{ color: "#DC2626", borderColor: "#DC2626" }}
                    onClick={async () => {
                      const step1 = await confirm(
                        "Această acțiune va șterge ireversibil TOATE datele dvs. din aplicație:\n" +
                        "• Toate facturile, companiile și contactele\n" +
                        "• Toate fișierele XML și PDF din arhivă\n" +
                        "• Toate setările și licența\n\n" +
                        "Datele NU pot fi recuperate după această operațiune.\n\n" +
                        "Doriți să continuați?",
                        {
                          title: "Atenție: Ștergere toate datele",
                          kind: "warning",
                        }
                      );
                      if (!step1) return;

                      const step2 = await confirm(
                        "Confirmare finală: sunteți absolut sigur că doriți să ștergeți TOATE datele dvs.?\n\n" +
                        "Această operațiune este ireversibilă.",
                        {
                          title: "Confirmare finală ștergere date",
                          kind: "warning",
                        }
                      );
                      if (!step2) return;

                      try {
                        await api.gdpr.wipeAll();
                        notify.success("Toate datele dvs. au fost șterse. Aplicația va reporni.");
                        // Give the toast time to show, then reload
                        setTimeout(() => {
                          window.location.reload();
                        }, 2000);
                      } catch (e) {
                        notify.error(formatError(e, "Ștergerea datelor a eșuat."));
                      }
                    }}
                  >
                    <Icon name="trash" size={12} /> Șterge toate datele (ireversibil)
                  </button>
                  <span style={{ fontSize: 11, color: "#DC2626" }}>
                    ⚠️ Această acțiune este ireversibilă. Exportați datele dvs. înainte de a continua.
                  </span>
                </div>
              </FieldRow>
            </FieldGroup>
          </Section>

          {/* Sistem */}
          <Section title={t('settings.sections.system')}>
            <FieldGroup>
              <FieldRow label="Pornire automată la login">
                <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                  <input
                    id="autostart-toggle"
                    type="checkbox"
                    className="cbx"
                    checked={autostartEnabled ?? false}
                    onChange={async (e) => {
                      try {
                        await api.system.setAutostart(e.target.checked);
                        void queryClient.invalidateQueries({ queryKey: queryKeys.system.autostart });
                      } catch (err) {
                        notify.error(formatError(err, 'Nu s-a putut modifica setarea de pornire automată.'));
                      }
                    }}
                  />
                  <label htmlFor="autostart-toggle" style={{ fontSize: 11, cursor: "pointer" }}>
                    Pornește aplicația automat la autentificarea în sistem
                  </label>
                </div>
              </FieldRow>
            </FieldGroup>
          </Section>

          {/* Jurnal activitate */}
          <Section title={t('settings.sections.activityLog')}>
            {activityLog.length === 0 ? (
              <div style={{ padding: "10px 14px", fontSize: 11, color: "var(--text-muted)" }}>
                Nicio activitate înregistrată.
              </div>
            ) : (
              <div style={{ overflowX: "auto" }}>
                <table className="dt">
                  <thead>
                    <tr>
                      <th style={{ width: 140 }}>Timp</th>
                      <th>Sarcină</th>
                      <th>Rezultat</th>
                    </tr>
                  </thead>
                  <tbody>
                    {activityLog.slice(0, 20).map((entry) => (
                      <tr key={entry.id}>
                        <td className="mono muted" style={{ fontSize: 10.5 }}>
                          {new Date(entry.createdAt * 1000).toLocaleString("ro-RO")}
                        </td>
                        <td>{entry.entityId || <span className="dim">—</span>}</td>
                        <td style={{ fontSize: 11, color: "var(--text-muted)" }}>
                          {entry.metadata || <span className="dim">—</span>}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
            <div style={{ display: "flex", justifyContent: "flex-end", marginTop: 8, padding: "0 14px 10px" }}>
              <button
                className="btn compact"
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
                <Icon name="download" size={11} /> Export CSV
              </button>
            </div>
          </Section>

          {/* Development */}
          {import.meta.env.DEV && (
            <Section title="Dezvoltare">
              <FieldGroup>
                <FieldRow label="Date de test">
                  <button type="button" className="btn" onClick={handleDevSeed}>
                    Populează DB cu date demo
                  </button>
                </FieldRow>
              </FieldGroup>
            </Section>
          )}
        </div>
      </PageContent>
    </>
  );
}
