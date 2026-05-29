/**
 * Setări aplicație — temă, companie activă, ANAF, licență, informații sistem.
 */

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useState, useEffect } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";

import { Skeleton } from "@/components/ui/skeleton";
import { Section, FieldRow, FieldGroup } from "@/components/shared/Section";
import { PageContent, PageHeader } from "@/components/shared/PageHeader";
import { Icon } from "@/components/shared/Icon";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore, type ThemeMode } from "@/lib/store";
import { notify } from "@/lib/toasts";
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

  // Notification preferences: per-type ("os" | "inapp" | "off") + quiet hours
  const NOTIF_TYPES = [
    { key: "validated",     label: "Factură validată ANAF" },
    { key: "rejected",      label: "Factură respinsă ANAF" },
    { key: "received",      label: "Facturi noi primite SPV" },
    { key: "cert_expiring", label: "Certificat SPV expiră" },
    { key: "cert_expired",  label: "Certificat SPV expirat" },
  ];

  const { data: notifPref_validated } = useQuery({
    queryKey: queryKeys.settings.get("notif_pref_validated"),
    queryFn: () => api.settings.get("notif_pref_validated"),
  });
  const { data: notifPref_rejected } = useQuery({
    queryKey: queryKeys.settings.get("notif_pref_rejected"),
    queryFn: () => api.settings.get("notif_pref_rejected"),
  });
  const { data: notifPref_received } = useQuery({
    queryKey: queryKeys.settings.get("notif_pref_received"),
    queryFn: () => api.settings.get("notif_pref_received"),
  });
  const { data: notifPref_cert_expiring } = useQuery({
    queryKey: queryKeys.settings.get("notif_pref_cert_expiring"),
    queryFn: () => api.settings.get("notif_pref_cert_expiring"),
  });
  const { data: notifPref_cert_expired } = useQuery({
    queryKey: queryKeys.settings.get("notif_pref_cert_expired"),
    queryFn: () => api.settings.get("notif_pref_cert_expired"),
  });

  const notifPrefs = [
    { key: "validated",     pref: notifPref_validated     ?? "os" },
    { key: "rejected",      pref: notifPref_rejected      ?? "os" },
    { key: "received",      pref: notifPref_received      ?? "os" },
    { key: "cert_expiring", pref: notifPref_cert_expiring ?? "os" },
    { key: "cert_expired",  pref: notifPref_cert_expired  ?? "os" },
  ];

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
      await api.settings.set(`smartbill_user_${activeCompanyId}`, smartbillUser);
      if (smartbillToken && !smartbillToken.startsWith("•")) {
        await api.settings.set(`smartbill_token_${activeCompanyId}`, smartbillToken);
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
    onError: (e) => setAnafError((e as unknown as { message?: string }).message ?? "Eroare autorizare ANAF."),
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
    if (!window.confirm("Populați baza de date cu date de test? Funcționează doar dacă DB-ul este gol.")) return;
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
              <FieldRow label="Mod test ANAF">
                <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                  <input
                    id="anaf-test-mode"
                    type="checkbox"
                    className="cbx"
                    checked={anafTestMode}
                    onChange={handleTestModeToggle}
                  />
                  <label htmlFor="anaf-test-mode" style={{ fontSize: 11, cursor: "pointer" }}>
                    Folosește endpointurile de test (api.anaf.ro/test)
                  </label>
                </div>
              </FieldRow>
              <FieldRow label="URL ANAF prod" mono>
                <span style={{ fontSize: 10.5 }}>https://api.anaf.ro/prod/FCTEL/rest/</span>
              </FieldRow>
              {activeCompanyId && (
                <FieldRow label="Status OAuth2">
                  <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                    {isAnafAuthenticated ? (
                      <>
                        <span style={{ fontSize: 11, color: "#16A34A", fontWeight: 600 }}>✓ Autentificat</span>
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
                        <span style={{ fontSize: 11, color: "var(--text-muted)" }}>Neautentificat</span>
                        <button
                          type="button"
                          className="btn primary"
                          disabled={authorizeAnaf.isPending}
                          onClick={() => authorizeAnaf.mutate()}
                        >
                          {authorizeAnaf.isPending ? "Se autorizează…" : "Autorizează ANAF"}
                        </button>
                      </>
                    )}
                  </div>
                </FieldRow>
              )}
              {anafError && (
                <FieldRow label="">
                  <span style={{ fontSize: 11, color: "#DC2626" }}>{anafError}</span>
                </FieldRow>
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
                        notify.error("Export eșuat: " + String(err));
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
                        api.system.openArchiveFolder().catch((e) => notify.error(String(e)));
                      }}
                    >
                      <Icon name="database" size={12} /> Deschide folder arhivă
                    </button>
                    <button
                      type="button"
                      className="btn compact"
                      onClick={async () => {
                        const { open } = await import("@tauri-apps/plugin-dialog");
                        const dir = await open({ directory: true, title: "Selectează noua locație arhivă" });
                        if (dir && typeof dir === "string") {
                          if (confirm(`Schimbi locația arhivei în:\n${dir}\n\nFișierele existente vor fi copiate. Continuați?`)) {
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
                        notify.error("Eroare backup: " + String(e));
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
                        notify.error("Eroare verificare: " + String(e));
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
                            if (confirm("Aceasta va înlocui baza de date curentă. Continuați?")) {
                              await api.archive.importBackup(file as string);
                            }
                          }
                        } catch (e) {
                          notify.error("Eroare restaurare: " + String(e));
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
                        notify.error("Eroare setare autostart: " + String(err));
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
