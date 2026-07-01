/**
 * Setări — verbatim port of the design "Setari.html":
 *   .page-head (title + sub + btn-dark "Salvează modificările") → .cols-2-even
 *   Left: Conectare ANAF SPV (.set-row rows + chips + tabs Test/Producție +
 *   configurare avansată OAuth) · Companie activă · Cote TVA (istoric legislativ)
 *   · Backup & restaurare · Date personale (GDPR) · Jurnal activitate.
 *   Right: Șablon factură PDF (preset/accent/antet/subsol/toggles + modal
 *   previzualizare LIVE .pdf-sheet) · Integrări (SmartBill) · Licență & aspect
 *   (temă + densitate) · Notificări · Suport și feedback · Informații aplicație
 *   · Sistem · Dezvoltare (DEV).
 *
 * ALL wiring preserved: api.anaf.authorize/logout/isAuthenticated,
 * api.anaf.setOauthClientSecret/hasOauthClientSecret + anaf_oauth_* settings,
 * use_anaf_test_env, api.certificates.list, invoice_template_* settings +
 * api.ubl.previewInvoiceTemplate + openPath, set_smartbill_credentials,
 * notif_pref_* + quiet_hours, api.license.get/activate + purchase_url,
 * api.archive.exportZip/getSize/verifyIntegrity/changeArchiveLocation,
 * api.system.exportBackup/openArchiveFolder + importBackup,
 * api.gdpr.exportAll/wipeAll (cu confirmare retenție L82/1991),
 * api.feedback.gather/mailto, api.system.appInfo/getAutostart/setAutostart/
 * getActivityLog/exportActivityLogCsv/devSeed, plugin-updater check.
 */

import { useQuery, useQueries, useMutation, useQueryClient } from "@tanstack/react-query";
import { useState, useEffect, useCallback } from "react";
import { open, save, confirm } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useOpenPdf } from "@/hooks/use-open-pdf";
import { invoke } from "@tauri-apps/api/core";
import { useNavigate } from "@tanstack/react-router";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { useAnimatedClose } from "@/hooks/use-animated-close";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore, type ThemeMode, type DensityMode } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { Company, EffectiveAccountMapping, ProductGroup, ProductType, SetAccountMappingInput } from "@/types";
import type { PayrollAccountMap, SetPayrollConfigInput } from "@/lib/tauri";

// ─── helpers ──────────────────────────────────────────────────────────────────

const RO_MON = ["ian", "feb", "mar", "apr", "mai", "iun", "iul", "aug", "sep", "oct", "nov", "dec"];

/** Unix seconds → `14 mar 2027`. */
function fmtRoUnix(ts: number): string {
  if (!ts) return "—";
  const d = new Date(ts * 1000);
  return `${String(d.getDate()).padStart(2, "0")} ${RO_MON[d.getMonth()]} ${d.getFullYear()}`;
}

function fmtBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

// Prototype icons not in Ic.tsx — inlined verbatim.
const CIRCLE_CHECK = '<path d="M9 12.75 11.25 15 15 9.75M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z"/>';
const WARN_TRI =
  '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>';

/** Small circle-check used inside chips (verbatim prototype path). */
function ChipCheck() {
  return <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: CIRCLE_CHECK }} />;
}

/** One design `.set-row`: title + optional description + right-aligned control. */
function SetRow({
  title,
  desc,
  descNum,
  danger,
  children,
}: {
  title: string;
  desc?: React.ReactNode;
  descNum?: boolean;
  danger?: boolean;
  children?: React.ReactNode;
}) {
  return (
    <div className="set-row">
      <div>
        <div className="s1" style={danger ? { color: "var(--red)" } : undefined}>{title}</div>
        {desc != null && <div className={`s2${descNum ? " num" : ""}`}>{desc}</div>}
      </div>
      <div className="end">{children}</div>
    </div>
  );
}

/** Design `.field` (label + control + optional hint). */
function Fld({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="field">
      <label>{label}</label>
      {children}
      {hint && <span className="hint">{hint}</span>}
    </div>
  );
}

const HEX_RE = /^#[0-9a-fA-F]{6}$/;

// ─── Notification type definitions ────────────────────────────────────────────
// Labels live in the locale files: settings.notifications.types.<key>

const NOTIF_KEYS = ["validated", "rejected", "received", "cert_expiring", "cert_expired"] as const;
type NotifKey = typeof NOTIF_KEYS[number];

// ─── Main page ────────────────────────────────────────────────────────────────

export function SettingsPage() {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const navigate = useNavigate();

  // Store
  const theme = useAppStore((s) => s.theme);
  const setTheme = useAppStore((s) => s.setTheme);
  const density = useAppStore((s) => s.density);
  const setDensity = useAppStore((s) => s.setDensity);
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const setActiveCompanyId = useAppStore((s) => s.setActiveCompanyId);
  const openPdf = useOpenPdf();

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
    queryKey: [...queryKeys.system.activityLog, activeCompanyId ?? ""],
    queryFn: () => api.system.getActivityLog(activeCompanyId!),
    enabled: !!activeCompanyId,
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

  // Certificat calificat SPV (real data — prototype shows a static row)
  const { data: certificates = [] } = useQuery({
    queryKey: queryKeys.certificates.list(activeCompanyId ?? ""),
    queryFn: () => api.certificates.list(activeCompanyId!),
    enabled: !!activeCompanyId,
  });
  const activeCert = certificates.find((c) => c.isActive) ?? certificates[0];
  const certValid = !!activeCert && activeCert.expiresAt * 1000 > Date.now();

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

  // Invoice template
  const [templatePreset, setTemplatePreset] = useState("clasic");
  const [templateAccent, setTemplateAccent] = useState("#000000");
  const [templateHeaderNote, setTemplateHeaderNote] = useState("");
  const [templateFooterNote, setTemplateFooterNote] = useState("");
  const [templateShowWords, setTemplateShowWords] = useState(true);
  const [templateShowVatDetail, setTemplateShowVatDetail] = useState(true);
  const [previewingTemplate, setPreviewingTemplate] = useState(false);
  const [savingTemplate, setSavingTemplate] = useState(false);
  const [templateSaved, setTemplateSaved] = useState(false);
  const [tplPreviewOpen, setTplPreviewOpen] = useState(false);

  // SmartBill
  const [smartbillUser, setSmartbillUser] = useState("");
  const [smartbillToken, setSmartbillToken] = useState("");
  const [smartbillConfigured, setSmartbillConfigured] = useState(false);
  const [savingSmartbill, setSavingSmartbill] = useState(false);
  const [smartbillSaved, setSmartbillSaved] = useState(false);

  // License activation in Settings
  const [showLicenseActivate, setShowLicenseActivate] = useState(false);
  const [licenseKeyInput, setLicenseKeyInput] = useState("");
  const [licenseEmailInput, setLicenseEmailInput] = useState("");
  const [licenseActivateError, setLicenseActivateError] = useState<string | null>(null);

  // Feedback
  const [feedbackMsg, setFeedbackMsg] = useState("");
  const [feedbackSending, setFeedbackSending] = useState(false);

  // GDPR wipe modal (design gdprModal — type STERGE to confirm)
  const [gdprOpen, setGdprOpen] = useState(false);
  const [gdprConfirmText, setGdprConfirmText] = useState("");
  const [gdprWiping, setGdprWiping] = useState(false);

  // Animated-exit close handlers for the two inline modals.
  const { closing: closing1, close: close1 } = useAnimatedClose(useCallback(() => setGdprOpen(false), []));
  const { closing: closing2, close: close2 } = useAnimatedClose(useCallback(() => setTplPreviewOpen(false), []));

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

  // Load invoice template settings on mount
  useEffect(() => {
    void (async () => {
      const [preset, accent, headerNote, footerNote, showWords, showVatDetail] = await Promise.all([
        api.settings.get("invoice_template_preset"),
        api.settings.get("invoice_template_accent"),
        api.settings.get("invoice_template_header_note"),
        api.settings.get("invoice_template_footer_note"),
        api.settings.get("invoice_template_show_words"),
        api.settings.get("invoice_template_show_vat_detail"),
      ]);
      if (preset) setTemplatePreset(preset);
      if (accent) setTemplateAccent(accent);
      if (headerNote) setTemplateHeaderNote(headerNote);
      if (footerNote) setTemplateFooterNote(footerNote);
      if (showWords != null) setTemplateShowWords(showWords !== "0");
      if (showVatDetail != null) setTemplateShowVatDetail(showVatDetail !== "0");
    })();
  }, []);

  // Load SmartBill credentials on active company change
  useEffect(() => {
    if (!activeCompanyId) return;
    api.integrations.getSmartbillCredentials(activeCompanyId)
      .then((creds) => {
        setSmartbillUser(creds.user);
        setSmartbillToken(creds.configured ? "••••••••" : "");
        setSmartbillConfigured(creds.configured);
      })
      .catch(() => {});
  }, [activeCompanyId]);

  // ── Mutations ─────────────────────────────────────────────────────────────────

  const authorizeAnaf = useMutation({
    mutationFn: () => api.anaf.authorize(activeCompanyId!),
    onSuccess: () => { void refetchAnafAuth(); setAnafError(null); },
    onError: (e) => setAnafError(formatError(e, t("settings.notify.anafAuthError"))),
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
      notify.success(t("settings.notify.licenseActivated"));
    },
    onError: (e) => setLicenseActivateError(formatError(e, t("settings.notify.licenseActivateFailed"))),
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
      notify.error(formatError(e, t("settings.notify.anafAdvancedSaveError")));
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
      if (tokenToSave) setSmartbillConfigured(true);
      setSmartbillSaved(true);
      setTimeout(() => setSmartbillSaved(false), 3000);
    } catch {
      notify.error(t("settings.notify.smartbillSaveError"));
    } finally {
      setSavingSmartbill(false);
    }
  };

  const handlePreviewTemplate = async () => {
    if (!activeCompanyId) { notify.warn(t("settings.notify.selectCompany")); return; }
    setPreviewingTemplate(true);
    try {
      // Previzualizează șablonul CURENT din formular (chiar nesalvat) pe o factură demo
      // cu identitatea reală a companiei (logo, IBAN, serie).
      const path = await api.ubl.previewInvoiceTemplate(activeCompanyId, {
        preset: templatePreset,
        accentHex: templateAccent,
        headerNote: templateHeaderNote,
        footerNote: templateFooterNote,
        showWords: templateShowWords,
        showVatDetail: templateShowVatDetail,
      });
      await openPdf(path, `${t("shared.pdfViewer.previewTitle")}.pdf`);
    } catch (e) {
      notify.error(formatError(e, t("settings.notify.previewFailed")));
    } finally {
      setPreviewingTemplate(false);
    }
  };

  const handleSaveTemplate = async () => {
    setSavingTemplate(true);
    try {
      await Promise.all([
        api.settings.set("invoice_template_preset", templatePreset),
        api.settings.set("invoice_template_accent", templateAccent),
        api.settings.set("invoice_template_header_note", templateHeaderNote),
        api.settings.set("invoice_template_footer_note", templateFooterNote),
        api.settings.set("invoice_template_show_words", templateShowWords ? "1" : "0"),
        api.settings.set("invoice_template_show_vat_detail", templateShowVatDetail ? "1" : "0"),
      ]);
      setTemplateSaved(true);
      setTimeout(() => setTemplateSaved(false), 3000);
    } catch (e) {
      notify.error(formatError(e, t("settings.notify.templateSaveError")));
    } finally {
      setSavingTemplate(false);
    }
  };

  /** Head action "Salvează modificările" — saves all form-based sections at once. */
  const handleSaveAll = async () => {
    await handleSaveTemplate();
    await handleSaveAnafAdvanced();
    if (activeCompanyId) await handleSaveSmartbill();
    notify.success(t("settings.notify.allSaved"));
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
      notify.success(t("settings.notify.emailReady"));
    } catch (e) {
      notify.error(
        formatError(e, t("settings.notify.emailFailed")),
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
      notify.error(formatError(e, t("settings.notify.purchaseFailed")));
    }
  };

  const handleCheckUpdate = async () => {
    setCheckingUpdate(true);
    setUpdateStatus(null);
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const update = await check();
      if (update?.available) {
        setUpdateStatus(t("settings.appInfo.updates.available", { v: update.version }));
      } else {
        setUpdateStatus(t("settings.appInfo.updates.upToDate"));
      }
    } catch {
      setUpdateStatus(t("settings.appInfo.updates.failed"));
    } finally {
      setCheckingUpdate(false);
    }
  };

  // ── Archive handlers ──────────────────────────────────────────────────────────

  const handleExportArchiveZip = async () => {
    if (!activeCompanyId) { notify.warn(t("settings.notify.selectCompany")); return; }
    try {
      const path = await api.archive.exportZip(activeCompanyId);
      notify.success(t("settings.notify.archiveExported", { path }));
    } catch (err) {
      notify.error(formatError(err, t("settings.notify.archiveExportFailed")));
    }
  };

  const handleOpenArchiveFolder = () => {
    api.system.openArchiveFolder().catch((e) =>
      notify.error(formatError(e, t("settings.notify.archiveFolderFailed")))
    );
  };

  const handleChangeArchiveLocation = async () => {
    const dir = await open({ directory: true, title: t("settings.dialogs.changeLocationPickTitle") });
    if (dir && typeof dir === "string") {
      const ok = await confirm(
        t("settings.dialogs.changeLocationMsg", { dir }),
        { title: t("settings.dialogs.changeLocationTitle"), kind: "warning" }
      );
      if (ok) {
        await api.archive.changeArchiveLocation(dir);
        notify.success(t("settings.notify.locationChanged"));
      }
    }
  };

  const handleExportBackup = async () => {
    try {
      const defaultName = `efactura_backup_${new Date().toISOString().slice(0, 10).replace(/-/g, "")}.zip`;
      const destPath = await save({
        filters: [{ name: "ZIP", extensions: ["zip"] }],
        defaultPath: defaultName,
      });
      if (!destPath) return;
      const path = await api.system.exportBackup(destPath as string);
      notify.success(t("settings.notify.backupSaved", { path }));
    } catch (e) {
      notify.error(formatError(e, t("settings.notify.backupExportFailed")));
    }
  };

  const handleVerifyIntegrity = async () => {
    if (!activeCompanyId) { notify.warn(t("settings.notify.selectCompany")); return; }
    try {
      const result = await api.archive.verifyIntegrity(activeCompanyId);
      if (result.ok) {
        notify.success(t("settings.notify.integrityOk", { count: result.checked }));
      } else {
        notify.error(
          t("settings.notify.integrityMissingPrefix", { missing: result.missing.length, checked: result.checked }) +
          (result.missingUnderRetention > 0
            ? t("settings.notify.integrityRetention", { n: result.missingUnderRetention })
            : "") +
          `): ` +
          result.missing.slice(0, 5).join(", ") +
          (result.missing.length > 5 ? " …" : "")
        );
      }
    } catch (e) {
      notify.error(formatError(e, t("settings.notify.integrityFailed")));
    }
  };

  const handleRestoreBackup = async () => {
    try {
      const file = await open({
        filters: [{ name: "ZIP", extensions: ["zip"] }],
      });
      if (file) {
        const ok = await confirm(
          t("settings.dialogs.restoreMsg"),
          { title: t("settings.dialogs.restoreTitle"), kind: "warning" }
        );
        if (ok) {
          await api.archive.importBackup(file as string);
        }
      }
    } catch (e) {
      notify.error(formatError(e, t("settings.notify.restoreFailed")));
    }
  };

  // ── GDPR handlers ─────────────────────────────────────────────────────────────

  const handleGdprExport = async () => {
    try {
      const dest = await save({
        defaultPath: `rofactura-date-${new Date().toISOString().slice(0, 10)}.zip`,
        filters: [{ name: "ZIP", extensions: ["zip"] }],
        title: t("settings.dialogs.gdprExportSaveTitle"),
      });
      if (!dest) return;
      const result = await api.gdpr.exportAll(dest);
      notify.success(t("settings.notify.gdprExported", { path: result.path }));
    } catch (e) {
      notify.error(formatError(e, t("settings.notify.gdprExportFailed")));
    }
  };

  /** Wipe total — gated by the gdprModal type-STERGE confirmation. */
  const handleGdprWipe = async () => {
    setGdprWiping(true);
    try {
      try {
        await api.gdpr.wipeAll();
      } catch (e) {
        // L82/1991: documente sub termenul legal de păstrare de 5 ani — cerem
        // confirmarea explicită a păstrării legale înainte de a forța ștergerea.
        const msg = formatError(e, "");
        if (!msg.includes("5 ani")) throw e;
        const ack = await confirm(
          msg + "\n\n" + t("settings.dialogs.gdprRetentionAck"),
          { title: t("settings.dialogs.gdprRetentionTitle"), kind: "warning" }
        );
        if (!ack) return;
        await api.gdpr.wipeAll(true);
      }
      setGdprOpen(false);
      notify.success(t("settings.notify.wiped"));
      setTimeout(() => { window.location.reload(); }, 2000);
    } catch (e) {
      notify.error(formatError(e, t("settings.notify.wipeFailed")));
    } finally {
      setGdprWiping(false);
    }
  };

  const handleDevSeed = async () => {
    const ok = await confirm(t("settings.dev.confirmMsg"), {
      title: t("settings.dev.confirmTitle"),
      kind: "info",
    });
    if (!ok) return;
    try {
      await api.system.devSeed();
      void queryClient.invalidateQueries({ queryKey: queryKeys.companies.all });
      void queryClient.invalidateQueries({ queryKey: queryKeys.invoices.all });
      notify.success(t("settings.dev.seeded"));
    } catch {
      notify.error(t("settings.dev.seedFailed"));
    }
  };

  // ─────────────────────────────────────────────────────────────────────────────

  const THEME_OPTIONS: { value: ThemeMode; label: string }[] = [
    { value: "light", label: t("settings.license.theme.light") },
    { value: "dark", label: t("settings.license.theme.dark") },
    { value: "system", label: t("settings.license.theme.system") },
  ];

  const DENSITY_OPTIONS: { value: DensityMode; label: string }[] = [
    { value: "compact", label: t("settings.license.density.compact") },
    { value: "comfortable", label: t("settings.license.density.comfortable") },
    { value: "relaxed", label: t("settings.license.density.relaxed") },
  ];

  const activeCompany = companies.find((c: Company) => c.id === activeCompanyId);

  const licenseDaysLeft = license
    ? Math.max(0, Math.floor((license.expiresAt - Date.now() / 1000) / 86400))
    : 0;

  // Live PDF mock — preset rules mirror pdf.rs: clasic=black; minimal=title only;
  // modern=title+sections+rules (same logic as the prototype's render()).
  const accentValid = HEX_RE.test(templateAccent);
  const mockAcc = accentValid ? templateAccent : "#000000";
  const mockTitleAcc = templatePreset === "modern" || templatePreset === "minimal" ? mockAcc : "#1D1D1F";
  const mockSecAcc = templatePreset === "modern" ? mockAcc : "#1D1D1F";
  const mockShowRules = templatePreset === "modern";
  const mockHeaderNote = templateHeaderNote.split("\n").slice(0, 2).join("\n").trim();
  const mockFooterNote = templateFooterNote.split("\n").slice(0, 3).join("\n").trim();

  const noCompanyWarn = () => notify.warn(t("settings.notify.selectCompany"));

  return (
    <div className="main-inner wide">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>{t("settings.title")}</h1>
          <p className="sub">
            {activeCompany ? activeCompany.legalName : t("settings.head.noCompany")}
            {appInfo ? ` · ${t("settings.head.version", { v: appInfo.version })}` : ""}
          </p>
        </div>
        <div className="head-actions">
          <button className="btn-dark" onClick={() => void handleSaveAll()}>
            <Ic name="check" />{t("settings.head.save")}
          </button>
        </div>
      </div>

      <div className="cols-2-even">
        {/* ════════ left column ════════ */}
        <div>
          {/* ANAF SPV */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">{t("settings.anaf.title")}</div>
              <div className="spacer" />
              {activeCompanyId ? (
                isAnafAuthenticated ? (
                  <span className="chip paid"><Ic name="checkC" cls="sic" />{t("settings.anaf.connected")}</span>
                ) : (
                  <span className="chip sent"><Ic name="dot" cls="sic" />{t("settings.anaf.notConnected")}</span>
                )
              ) : (
                <span className="muted" style={{ fontSize: 12 }}>{t("settings.anaf.noCompanyChip")}</span>
              )}
            </div>
            <SetRow
              title={t("settings.anaf.oauth.title")}
              desc={
                isAnafAuthenticated
                  ? t("settings.anaf.oauth.descConnected")
                  : t("settings.anaf.oauth.descDisconnected")
              }
            >
              {isAnafAuthenticated ? (
                <button
                  className="pill-btn"
                  style={{ color: "var(--red)", borderColor: "rgba(220,38,38,.3)" }}
                  disabled={logoutAnaf.isPending}
                  onClick={() => { if (!activeCompanyId) { noCompanyWarn(); return; } logoutAnaf.mutate(); }}
                >
                  {logoutAnaf.isPending ? t("settings.anaf.oauth.disconnecting") : t("settings.anaf.oauth.disconnect")}
                </button>
              ) : (
                <button
                  className="pill-btn"
                  disabled={authorizeAnaf.isPending}
                  onClick={() => { if (!activeCompanyId) { noCompanyWarn(); return; } authorizeAnaf.mutate(); }}
                >
                  <Ic name="shield" />
                  {authorizeAnaf.isPending ? t("settings.anaf.oauth.authorizing") : t("settings.anaf.oauth.connect")}
                </button>
              )}
            </SetRow>
            <SetRow
              title={t("settings.anaf.secret.title")}
              desc={t("settings.anaf.secret.desc")}
            >
              {anafHasSecret ? (
                <span className="chip paid"><ChipCheck />{t("settings.anaf.secret.configured")}</span>
              ) : (
                <span className="chip wait"><Ic name="clock" cls="sic" />{t("settings.anaf.secret.missing")}</span>
              )}
              <button className="pill-btn" onClick={() => setAnafAdvancedOpen(true)}>{t("settings.anaf.secret.change")}</button>
            </SetRow>
            <SetRow
              title={t("settings.anaf.env.title")}
              desc={
                <span className="num">
                  {anafTestMode
                    ? "https://api.anaf.ro/test/FCTEL/rest/"
                    : "https://api.anaf.ro/prod/FCTEL/rest/"}
                </span>
              }
            >
              <div className="tabs">
                <div
                  className={`tab${anafTestMode ? " active" : ""}`}
                  onClick={() => void handleTestModeChange(true)}
                >
                  {t("settings.anaf.env.test")}
                </div>
                <div
                  className={`tab${anafTestMode ? "" : " active"}`}
                  onClick={() => void handleTestModeChange(false)}
                >
                  {t("settings.anaf.env.prod")}
                </div>
              </div>
            </SetRow>
            <SetRow
              title={t("settings.anaf.cert.title")}
              descNum
              desc={
                activeCert
                  ? t("settings.anaf.cert.descValid", { until: fmtRoUnix(activeCert.expiresAt), refresh: fmtRoUnix(activeCert.refreshableUntil) })
                  : t("settings.anaf.cert.descNone")
              }
            >
              {activeCert ? (
                certValid ? (
                  <span className="chip paid"><ChipCheck />{t("settings.anaf.cert.valid")}</span>
                ) : (
                  <span className="chip late"><Ic name="xMark" cls="sic" />{t("settings.anaf.cert.expired")}</span>
                )
              ) : (
                <span className="muted">—</span>
              )}
            </SetRow>
            <SetRow title={t("settings.anaf.sync.title")} desc={t("settings.anaf.sync.desc")}>
              {/* propunere — neimplementat: interval de sincronizare configurabil */}
              <span className="tog on" onClick={() => notify.info(t("settings.notify.comingSoon"))} />
            </SetRow>
            <SetRow
              title={t("settings.anaf.advanced.title")}
              desc={t("settings.anaf.advanced.desc")}
            >
              <button className="pill-btn" onClick={() => setAnafAdvancedOpen((v) => !v)}>
                {anafAdvancedOpen ? t("settings.anaf.advanced.hide") : t("settings.anaf.advanced.configure")}
                <Ic name="chevD" cls="ic" />
              </button>
            </SetRow>
            {anafError && (
              <div style={{ padding: "0 16px 12px" }}>
                <div className="banner danger" style={{ marginBottom: 0 }}>
                  <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: WARN_TRI }} />
                  <span>{anafError}</span>
                </div>
              </div>
            )}
            {anafAdvancedOpen && (
              <div className="card-pad" style={{ borderTop: "1px solid var(--line)" }}>
                <div className="banner">
                  <Ic name="shield" />
                  <span>
                    {t("settings.anaf.advanced.bannerP1")}
                    <b> Client ID</b> {t("settings.anaf.advanced.bannerP2")} <b>Client Secret</b>
                    {t("settings.anaf.advanced.bannerP3")}
                  </span>
                </div>
                <div className="fgrid">
                  <Fld label={t("settings.anaf.advanced.clientId")} hint={t("settings.anaf.advanced.clientIdHint")}>
                    <input
                      className="input num"
                      placeholder={t("settings.anaf.advanced.clientIdPh")}
                      value={anafClientId}
                      onChange={(e) => setAnafClientId(e.target.value)}
                    />
                  </Fld>
                  <Fld
                    label={t("settings.anaf.advanced.clientSecret")}
                    hint={anafHasSecret ? t("settings.anaf.advanced.secretHintSet") : t("settings.anaf.advanced.secretHintNew")}
                  >
                    <input
                      type="password"
                      className="input num"
                      placeholder={anafHasSecret ? t("settings.anaf.advanced.secretPhSet") : t("settings.anaf.advanced.secretPhNew")}
                      value={anafClientSecret}
                      onChange={(e) => setAnafClientSecret(e.target.value)}
                    />
                  </Fld>
                  <Fld label={t("settings.anaf.advanced.redirectUri")}>
                    <input
                      className="input num"
                      placeholder={t("settings.anaf.advanced.redirectPh")}
                      value={anafRedirectUri}
                      onChange={(e) => setAnafRedirectUri(e.target.value)}
                    />
                  </Fld>
                  <Fld label={t("settings.anaf.advanced.callbackPort")}>
                    <input
                      className="input num"
                      placeholder={t("settings.anaf.advanced.portPh")}
                      value={anafCallbackPort}
                      onChange={(e) => setAnafCallbackPort(e.target.value)}
                    />
                  </Fld>
                  <Fld label={t("settings.anaf.advanced.authorizeUrl")}>
                    <input
                      className="input num"
                      placeholder="https://logincert.anaf.ro/anaf-oauth2/v1/authorize"
                      value={anafAuthorizeUrl}
                      onChange={(e) => setAnafAuthorizeUrl(e.target.value)}
                    />
                  </Fld>
                  <Fld label={t("settings.anaf.advanced.tokenUrl")}>
                    <input
                      className="input num"
                      placeholder="https://logincert.anaf.ro/anaf-oauth2/v1/token"
                      value={anafTokenUrl}
                      onChange={(e) => setAnafTokenUrl(e.target.value)}
                    />
                  </Fld>
                </div>
                <div style={{ display: "flex", gap: 8, alignItems: "center", marginTop: 13 }}>
                  <button
                    className="btn-dark"
                    style={{ height: 34 }}
                    disabled={anafAdvancedSaving}
                    onClick={() => void handleSaveAnafAdvanced()}
                  >
                    <Ic name="check" />
                    {anafAdvancedSaving ? t("settings.common.saving") : t("settings.anaf.advanced.save")}
                  </button>
                  {anafAdvancedSaved && <span className="okk">{t("settings.common.saved")}</span>}
                </div>
              </div>
            )}
          </div>

          {/* companie activă */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">{t("settings.company.title")}</div>
              <div className="spacer" />
              <span className="muted" style={{ fontSize: 12 }}>
                {activeCompany ? activeCompany.legalName : t("settings.company.notSelected")}
              </span>
            </div>
            <SetRow
              title={t("settings.company.work.title")}
              desc={t("settings.company.work.desc")}
            >
              {companies.length === 0 ? (
                <span className="muted" style={{ fontSize: 12.5 }}>{t("settings.company.none")}</span>
              ) : (
                <select
                  className="select"
                  style={{ width: 230 }}
                  value={activeCompanyId ?? ""}
                  onChange={(e) => setActiveCompanyId(e.target.value || null)}
                >
                  <option value="">{t("settings.company.selectPlaceholder")}</option>
                  {companies.map((c: Company) => (
                    <option key={c.id} value={c.id}>
                      {c.legalName} ({c.cui})
                    </option>
                  ))}
                </select>
              )}
              <button className="pill-btn" onClick={() => void navigate({ to: "/companies" })}>
                {t("settings.company.manage")}
              </button>
            </SetRow>
            {activeCompany && (
              <>
                <SetRow
                  title={t("settings.company.series.title")}
                  descNum
                  desc={t("settings.company.series.desc", {
                    series: activeCompany.invoiceSeries,
                    last: String(activeCompany.lastInvoiceNumber).padStart(4, "0"),
                    next: String(activeCompany.lastInvoiceNumber + 1).padStart(4, "0"),
                  })}
                >
                  <button
                    className="pill-btn"
                    onClick={() => void navigate({ to: "/companies/$id/edit", params: { id: activeCompany.id } })}
                  >
                    {t("settings.company.edit")}
                  </button>
                </SetRow>
                <SetRow title={t("settings.company.cui")} descNum desc={activeCompany.cui}>
                  <span className="muted" style={{ fontSize: 12.5 }}>
                    {activeCompany.vatPayer ? t("settings.company.vatPayer") : t("settings.company.nonVatPayer")}
                  </span>
                </SetRow>
                <SetRow title={t("settings.company.spv.title")} desc={t("settings.company.spv.desc")}>
                  {activeCompany.spvEnabled ? (
                    <span className="chip paid"><ChipCheck />{t("settings.company.spv.yes")}</span>
                  ) : (
                    <span className="chip sent"><Ic name="dot" cls="sic" />{t("settings.company.spv.no")}</span>
                  )}
                </SetRow>
              </>
            )}
          </div>

          {/* cote TVA */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">{t("settings.vat.title")}</div>
              <div className="spacer" />
              <button
                className="see-all"
                style={{ height: "auto", padding: 0, border: 0, background: "transparent" }}
                onClick={() => void navigate({ to: "/vat-rates" })}
              >
                {t("settings.vat.catalog")}<Ic name="chevR" cls="ic" />
              </button>
            </div>
            <table className="scr-table">
              <thead>
                <tr>
                  <th>{t("settings.vat.th.period")}</th>
                  <th className="r">{t("settings.vat.th.standard")}</th>
                  <th className="r">{t("settings.vat.th.reduced")}</th>
                  <th className="r">{t("settings.vat.th.reduced2")}</th>
                  <th>{t("settings.vat.th.status")}</th>
                </tr>
              </thead>
              <tbody>
                <tr>
                  <td className="num">{t("settings.vat.until")}</td>
                  <td className="r num">19%</td>
                  <td className="r num">9%</td>
                  <td className="r num">5%</td>
                  <td><span className="chip sent">{t("settings.vat.historic")}</span></td>
                </tr>
                <tr>
                  <td className="num">{t("settings.vat.from")}</td>
                  <td className="r num"><b>21%</b></td>
                  <td className="r num"><b>11%</b></td>
                  <td className="r num">—</td>
                  <td><span className="chip paid"><ChipCheck />{t("settings.vat.inForce")}</span></td>
                </tr>
              </tbody>
            </table>
            <div className="pager">
              <span>{t("settings.vat.note")}</span>
              <span></span>
            </div>
          </div>

          {/* P2 Wave 1: conturi implicite pe tip produs */}
          {activeCompanyId && (
            <AccountMappingPanel companyId={activeCompanyId} />
          )}

          {/* P2 Wave 1: grupe de articole */}
          {activeCompanyId && (
            <ProductGroupsPanel companyId={activeCompanyId} />
          )}

          {/* P2 Wave 7: configurare salarizare (conturi GL + diurnă + rate 2026) */}
          {activeCompanyId && (
            <PayrollConfigPanel companyId={activeCompanyId} />
          )}

          {/* backup & restaurare */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">{t("settings.backup.title")}</div>
              <div className="spacer" />
              <span className="muted num" style={{ fontSize: 12 }}>{t("settings.backup.archiveSize", { size: fmtBytes(archiveSize ?? 0) })}</span>
            </div>
            <SetRow
              title={t("settings.backup.full.title")}
              desc={t("settings.backup.full.desc")}
            >
              <button className="pill-btn" onClick={() => void handleExportBackup()}>
                <Ic name="dl" />{t("settings.backup.full.download")}
              </button>
            </SetRow>
            <SetRow
              title={t("settings.backup.export.title")}
              desc={t("settings.backup.export.desc")}
            >
              <button
                className="pill-btn"
                style={!activeCompanyId ? { opacity: 0.5 } : undefined}
                onClick={() => void handleExportArchiveZip()}
              >
                <Ic name="dl" />{t("settings.common.export")}
              </button>
            </SetRow>
            <SetRow title={t("settings.backup.folder.title")} desc={t("settings.backup.folder.desc")}>
              <button className="pill-btn" onClick={handleOpenArchiveFolder}>{t("settings.backup.folder.open")}</button>
              <button className="pill-btn" onClick={() => void handleChangeArchiveLocation()}>
                {t("settings.backup.folder.changeLocation")}
              </button>
            </SetRow>
            <SetRow
              title={t("settings.backup.integrity.title")}
              desc={t("settings.backup.integrity.desc")}
            >
              <button
                className="pill-btn"
                style={!activeCompanyId ? { opacity: 0.5 } : undefined}
                onClick={() => void handleVerifyIntegrity()}
              >
                {t("settings.backup.integrity.verify")}
              </button>
            </SetRow>
            <SetRow
              title={t("settings.backup.restore.title")}
              desc={t("settings.backup.restore.desc")}
            >
              <button className="pill-btn" onClick={() => void handleRestoreBackup()}>
                <Ic name="docUp" />{t("settings.backup.restore.upload")}
              </button>
            </SetRow>
          </div>

          {/* GDPR */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar"><div className="tt">{t("settings.gdpr.title")}</div></div>
            <SetRow
              title={t("settings.gdpr.export.title")}
              desc={t("settings.gdpr.export.desc")}
            >
              <button className="pill-btn" onClick={() => void handleGdprExport()}>{t("settings.common.export")}</button>
            </SetRow>
            <SetRow
              title={t("settings.gdpr.privacy.title")}
              desc={t("settings.gdpr.privacy.desc")}
            >
              <button
                className="pill-btn"
                onClick={() => { openUrl("https://lucaris.ro/privacy").catch(() => {}); }}
              >
                {t("settings.common.open")}
              </button>
            </SetRow>
            <SetRow
              title={t("settings.gdpr.wipe.title")}
              danger
              desc={
                <>
                  {t("settings.gdpr.wipe.descPre")} <b>{t("settings.gdpr.wipe.years")}</b>{" "}
                  {t("settings.gdpr.wipe.descPost")}
                </>
              }
            >
              <button
                className="pill-btn"
                style={{ color: "var(--red)", borderColor: "rgba(220,38,38,.3)" }}
                onClick={() => { setGdprConfirmText(""); setGdprOpen(true); }}
              >
                {t("settings.gdpr.wipe.button")}
              </button>
            </SetRow>
          </div>

          {/* jurnal activitate */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">{t("settings.activity.title")}</div>
              <div className="spacer" />
              <button
                className="pill-btn"
                onClick={async () => {
                  if (!activeCompanyId) return;
                  try {
                    // Native save dialog + fs write instead of a browser blob/anchor
                    // download — the latter is a silent no-op in Tauri's macOS
                    // WKWebView (no download manager to catch the anchor click).
                    const { save } = await import("@tauri-apps/plugin-dialog");
                    const path = await save({
                      filters: [{ name: "CSV", extensions: ["csv"] }],
                      defaultPath: "jurnal-activitate.csv",
                    });
                    if (!path) return;
                    const csv = await api.system.exportActivityLogCsv(activeCompanyId);
                    const { writeTextFile } = await import("@tauri-apps/plugin-fs");
                    await writeTextFile(path, csv);
                    notify.success(t("settings.activity.exported", { path }));
                  } catch (err) {
                    notify.error(formatError(err, t("settings.activity.exportFailed")));
                  }
                }}
              >
                <Ic name="dl" />{t("settings.activity.exportCsv")}
              </button>
            </div>
            {activityLog.length === 0 ? (
              <div style={{ padding: "20px 16px", textAlign: "center", fontSize: 12.5, color: "var(--text-2)" }}>
                {t("settings.activity.empty")}
              </div>
            ) : (
              <table className="scr-table">
                <thead>
                  <tr>
                    <th style={{ width: 150 }}>{t("settings.activity.th.time")}</th>
                    <th>{t("settings.activity.th.task")}</th>
                    <th>{t("settings.activity.th.result")}</th>
                  </tr>
                </thead>
                <tbody>
                  {activityLog.slice(0, 20).map((entry) => (
                    <tr key={entry.id}>
                      <td className="num" style={{ fontSize: 11.5, color: "var(--text-2)" }}>
                        {new Date(entry.createdAt * 1000).toLocaleString(i18n.language)}
                      </td>
                      <td>{entry.entityId || <span className="muted">—</span>}</td>
                      <td style={{ fontSize: 12, color: "var(--text-2)" }}>
                        {entry.metadata || <span className="muted">—</span>}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        </div>

        {/* ════════ right column ════════ */}
        <div>
          {/* șablon factură PDF */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">{t("settings.template.title")}</div>
              <div className="spacer" />
              <span className="muted" style={{ fontSize: 12 }}>{t("settings.template.subtitle")}</span>
            </div>
            <div className="card-pad">
              <div className="fgrid" style={{ gridTemplateColumns: "1fr" }}>
                <Fld label={t("settings.template.preset.label")} hint={t("settings.template.preset.hint")}>
                  <select
                    className="select"
                    value={templatePreset}
                    onChange={(e) => setTemplatePreset(e.target.value)}
                  >
                    <option value="clasic">{t("settings.template.preset.clasic")}</option>
                    <option value="modern">{t("settings.template.preset.modern")}</option>
                    <option value="minimal">{t("settings.template.preset.minimal")}</option>
                  </select>
                </Fld>
                <Fld label={t("settings.template.accent.label")} hint={t("settings.template.accent.hint")}>
                  <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                    <input
                      type="color"
                      value={accentValid ? templateAccent : "#000000"}
                      onChange={(e) => setTemplateAccent(e.target.value.toUpperCase())}
                      style={{ width: 46, height: 34, padding: 2, border: "1px solid var(--line)", borderRadius: 8, cursor: "pointer", flex: "none" }}
                      title={t("settings.template.accent.pickerTitle")}
                    />
                    <input
                      className="input num"
                      type="text"
                      value={templateAccent}
                      onChange={(e) => setTemplateAccent(e.target.value)}
                      style={{ width: 120 }}
                      placeholder="#000000"
                    />
                  </div>
                </Fld>
                <Fld label={t("settings.template.header.label")} hint={t("settings.template.header.hint")}>
                  <textarea
                    className="input"
                    rows={2}
                    maxLength={240}
                    value={templateHeaderNote}
                    onChange={(e) => setTemplateHeaderNote(e.target.value)}
                    placeholder={t("settings.template.header.ph")}
                  />
                </Fld>
                <Fld label={t("settings.template.footer.label")} hint={t("settings.template.footer.hint")}>
                  <textarea
                    className="input"
                    rows={3}
                    maxLength={400}
                    value={templateFooterNote}
                    onChange={(e) => setTemplateFooterNote(e.target.value)}
                    placeholder={t("settings.template.footer.ph")}
                  />
                </Fld>
              </div>
            </div>
            <SetRow title={t("settings.template.words.title")} desc={t("settings.template.words.desc")}>
              <span
                className={`tog${templateShowWords ? " on" : ""}`}
                onClick={() => setTemplateShowWords((v) => !v)}
              />
            </SetRow>
            <SetRow title={t("settings.template.vatDetail.title")} desc={t("settings.template.vatDetail.desc")}>
              <span
                className={`tog${templateShowVatDetail ? " on" : ""}`}
                onClick={() => setTemplateShowVatDetail((v) => !v)}
              />
            </SetRow>
            <div className="card-pad" style={{ paddingTop: 12, display: "flex", gap: 8, alignItems: "center" }}>
              <button
                className="btn-dark"
                style={{ height: 34 }}
                disabled={savingTemplate}
                onClick={() => void handleSaveTemplate()}
              >
                <Ic name="check" />
                {savingTemplate ? t("settings.common.saving") : t("settings.template.save")}
              </button>
              <button className="pill-btn" onClick={() => setTplPreviewOpen(true)}>
                <Ic name="eye" />{t("settings.template.preview")}
              </button>
              {templateSaved && <span className="okk">{t("settings.common.saved")}</span>}
            </div>
          </div>

          {/* integrări */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar"><div className="tt">{t("settings.smartbill.cardTitle")}</div></div>
            <SetRow
              title={t("settings.smartbill.row.title")}
              desc={t("settings.smartbill.row.desc")}
            >
              {smartbillConfigured ? (
                <span className="chip paid"><ChipCheck />{t("settings.smartbill.configured")}</span>
              ) : (
                <span className="chip sent"><Ic name="dot" cls="sic" />{t("settings.smartbill.notConfigured")}</span>
              )}
            </SetRow>
            <div className="card-pad" style={{ borderTop: "1px solid var(--line)" }}>
              <div className="fgrid">
                <Fld label={t("settings.smartbill.user.label")}>
                  <input
                    className="input"
                    type="email"
                    placeholder={t("settings.smartbill.user.ph")}
                    value={smartbillUser}
                    onChange={(e) => setSmartbillUser(e.target.value)}
                  />
                </Fld>
                <Fld label={t("settings.smartbill.token.label")} hint={t("settings.smartbill.token.hint")}>
                  <input
                    className="input num"
                    type="password"
                    placeholder={t("settings.smartbill.token.ph")}
                    value={smartbillToken}
                    onChange={(e) => setSmartbillToken(e.target.value)}
                  />
                </Fld>
              </div>
              <div style={{ display: "flex", gap: 8, alignItems: "center", marginTop: 13 }}>
                <button
                  className="btn-dark"
                  style={{ height: 34, ...(!activeCompanyId ? { opacity: 0.5 } : null) }}
                  disabled={savingSmartbill || !activeCompanyId}
                  onClick={() => void handleSaveSmartbill()}
                >
                  <Ic name="check" />
                  {savingSmartbill ? t("settings.common.saving") : t("settings.smartbill.save")}
                </button>
                {smartbillSaved && <span className="okk">{t("settings.common.saved")}</span>}
              </div>
            </div>
          </div>

          {/* licență + temă */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar"><div className="tt">{t("settings.license.cardTitle")}</div></div>
            <SetRow
              title={
                licenseLoading
                  ? t("settings.license.loadingTitle")
                  : license
                    ? t("settings.license.planActive", { plan: t(`settings.license.tiers.${license.tier}`, { defaultValue: license.tier }) })
                    : t("settings.license.none")
              }
              descNum
              desc={
                license
                  ? t("settings.license.desc", {
                      email: license.email ?? "—",
                      date: fmtRoUnix(license.expiresAt),
                      days: t("settings.license.daysLeft", { count: licenseDaysLeft }),
                    })
                  : t("settings.license.noneDesc")
              }
            >
              {license && (
                license.isExpired ? (
                  <span className="chip late"><Ic name="xMark" cls="sic" />{t("settings.license.expired")}</span>
                ) : (
                  <span className="chip paid"><ChipCheck />{t("settings.license.active")}</span>
                )
              )}
              <button
                className="pill-btn"
                onClick={() => { setShowLicenseActivate((v) => !v); setLicenseActivateError(null); }}
              >
                {license ? t("settings.license.activateOther") : t("settings.license.activate")}
              </button>
              <button className="pill-btn" onClick={() => void openPurchase()}>{t("settings.common.buy")}</button>
            </SetRow>
            {showLicenseActivate && (
              <div className="card-pad" style={{ borderTop: "1px solid var(--line)" }}>
                <div className="fgrid">
                  <Fld label={t("settings.license.key.label")}>
                    <input
                      className="input num"
                      placeholder="XXXX-XXXX-XXXX-XXXX"
                      value={licenseKeyInput}
                      onChange={(e) => setLicenseKeyInput(e.target.value.toUpperCase())}
                      style={{ textTransform: "uppercase" }}
                      autoComplete="off"
                      spellCheck={false}
                    />
                  </Fld>
                  <Fld label={t("settings.license.email.label")}>
                    <input
                      className="input"
                      type="email"
                      placeholder={t("settings.license.email.ph")}
                      value={licenseEmailInput}
                      onChange={(e) => setLicenseEmailInput(e.target.value)}
                    />
                  </Fld>
                </div>
                {licenseActivateError && (
                  <div className="banner danger" style={{ marginTop: 12, marginBottom: 0 }}>
                    <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: WARN_TRI }} />
                    <span>{licenseActivateError}</span>
                  </div>
                )}
                <div style={{ display: "flex", gap: 8, alignItems: "center", marginTop: 13 }}>
                  <button
                    className="btn-dark"
                    style={{ height: 34 }}
                    disabled={licenseActivateMutation.isPending}
                    onClick={() => {
                      setLicenseActivateError(null);
                      if (!licenseKeyInput.trim()) { setLicenseActivateError(t("settings.license.errors.keyRequired")); return; }
                      if (!licenseEmailInput.trim()) { setLicenseActivateError(t("settings.license.errors.emailRequired")); return; }
                      licenseActivateMutation.mutate();
                    }}
                  >
                    {licenseActivateMutation.isPending ? t("settings.license.activating") : t("settings.license.activateBtn")}
                  </button>
                  <button
                    className="pill-btn"
                    onClick={() => { setShowLicenseActivate(false); setLicenseActivateError(null); }}
                  >
                    {t("settings.common.cancel")}
                  </button>
                </div>
              </div>
            )}
            <SetRow title={t("settings.license.theme.title")} desc={t("settings.license.theme.desc")}>
              <div className="tabs">
                {THEME_OPTIONS.map((o) => (
                  <div
                    key={o.value}
                    className={`tab${theme === o.value ? " active" : ""}`}
                    onClick={() => setTheme(o.value)}
                  >
                    {o.label}
                  </div>
                ))}
              </div>
            </SetRow>
            <SetRow title={t("settings.license.density.title")} desc={t("settings.license.density.desc")}>
              <div className="tabs">
                {DENSITY_OPTIONS.map((o) => (
                  <div
                    key={o.value}
                    className={`tab${(density ?? "comfortable") === o.value ? " active" : ""}`}
                    onClick={() => setDensity(o.value)}
                  >
                    {o.label}
                  </div>
                ))}
              </div>
            </SetRow>
          </div>

          {/* notificări */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar"><div className="tt">{t("settings.notifications.title")}</div></div>
            {NOTIF_KEYS.map((key) => (
              <SetRow key={key} title={t(`settings.notifications.types.${key}`)}>
                <select
                  className="select"
                  style={{ width: 170, height: 30, fontSize: 12.5 }}
                  value={notifPrefMap[key]}
                  onChange={(e) => void handleNotifPrefChange(key, e.target.value)}
                >
                  <option value="os">{t("settings.notifications.options.os")}</option>
                  <option value="inapp">{t("settings.notifications.options.inapp")}</option>
                  <option value="off">{t("settings.notifications.options.off")}</option>
                </select>
              </SetRow>
            ))}
            <SetRow title={t("settings.notifications.quiet.title")} desc={t("settings.notifications.quiet.desc")}>
              <span
                className={`tog${(quietHoursSetting ?? "0") === "1" ? " on" : ""}`}
                onClick={() => void handleNotifToggle("quiet_hours", (quietHoursSetting ?? "0") !== "1")}
              />
            </SetRow>
          </div>

          {/* suport și feedback */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar"><div className="tt">{t("settings.feedback.title")}</div></div>
            <div className="card-pad">
              <Fld label={t("settings.feedback.message.label")}>
                <textarea
                  className="input"
                  rows={4}
                  value={feedbackMsg}
                  onChange={(e) => setFeedbackMsg(e.target.value)}
                  placeholder={t("settings.feedback.message.ph")}
                />
              </Fld>
              <div style={{ marginTop: 10, padding: "8px 10px", border: "1px dashed var(--line)", borderRadius: 8, fontSize: 12, color: "var(--text-2)", lineHeight: 1.5 }}>
                <b>{t("settings.feedback.autoBold")}</b> {t("settings.feedback.autoRest", { v: appInfo?.version ?? "—" })}
              </div>
              <div style={{ display: "flex", gap: 8, marginTop: 12 }}>
                <button
                  className="btn-dark"
                  style={{ height: 34 }}
                  disabled={feedbackSending}
                  onClick={() => void sendFeedback()}
                >
                  <Ic name="mail" />
                  {feedbackSending ? t("settings.feedback.preparing") : t("settings.feedback.send")}
                </button>
                <button className="pill-btn" onClick={() => void openPurchase()}>{t("settings.common.buy")}</button>
              </div>
            </div>
          </div>

          {/* informații aplicație */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar"><div className="tt">{t("settings.appInfo.title")}</div></div>
            {appInfoLoading ? (
              <div style={{ padding: "16px", fontSize: 12.5, color: "var(--text-2)" }}>{t("settings.appInfo.loading")}</div>
            ) : appInfo ? (
              <>
                <SetRow title={t("settings.appInfo.version")} descNum desc={appInfo.version} />
                <SetRow
                  title={t("settings.appInfo.dataDir")}
                  descNum
                  desc={<span style={{ wordBreak: "break-all" }}>{appInfo.appDataDir}</span>}
                />
                <SetRow
                  title={t("settings.appInfo.db")}
                  descNum
                  desc={<span style={{ wordBreak: "break-all" }}>{appInfo.dbPath}</span>}
                />
                <SetRow title={t("settings.appInfo.updates.title")} desc={updateStatus ?? t("settings.appInfo.updates.prompt")}>
                  <button
                    className="pill-btn"
                    disabled={checkingUpdate}
                    onClick={() => void handleCheckUpdate()}
                  >
                    <Ic name="sync" />
                    {checkingUpdate ? t("settings.appInfo.updates.checking") : t("settings.appInfo.updates.check")}
                  </button>
                </SetRow>
              </>
            ) : null}
          </div>

          {/* sistem */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar"><div className="tt">{t("settings.system.title")}</div></div>
            <SetRow
              title={t("settings.system.autostart.title")}
              desc={t("settings.system.autostart.desc")}
            >
              <span
                className={`tog${autostartEnabled ? " on" : ""}`}
                onClick={async () => {
                  try {
                    await api.system.setAutostart(!autostartEnabled);
                    void queryClient.invalidateQueries({ queryKey: queryKeys.system.autostart });
                  } catch (err) {
                    notify.error(formatError(err, t("settings.notify.autostartFailed")));
                  }
                }}
              />
            </SetRow>
          </div>

          {/* dezvoltare (DEV only) */}
          {import.meta.env.DEV && (
            <div className="scr-card" style={{ marginBottom: 14 }}>
              <div className="scr-toolbar"><div className="tt">{t("settings.dev.title")}</div></div>
              <SetRow title={t("settings.dev.demo.title")} desc={t("settings.dev.demo.desc")}>
                <button className="pill-btn" onClick={() => void handleDevSeed()}>
                  {t("settings.dev.demo.button")}
                </button>
              </SetRow>
            </div>
          )}
        </div>
      </div>

      {/* modal GDPR — ștergere totală (type STERGE to confirm) */}
      {gdprOpen && (
        <div
          className={`modal-back ${closing1 ? "closing" : "show"}`}
          style={{ position: "fixed" }}
          onMouseDown={(e) => { if (e.target === e.currentTarget) close1(); }}
        >
          <div className="modal">
            <div className="modal-head">
              <div>
                <div className="mt" style={{ color: "var(--red)" }}>{t("settings.gdpr.modal.title")}</div>
                <div className="ms">{t("settings.gdpr.modal.subtitle")}</div>
              </div>
              <button className="modal-x" onClick={() => close1()}>
                <Ic name="xMark" />
              </button>
            </div>
            <div className="modal-body">
              <div className="banner danger" style={{ marginBottom: 14 }}>
                <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: WARN_TRI }} />
                <span>
                  <b>{t("settings.gdpr.modal.bannerBold")}</b> {t("settings.gdpr.modal.bannerP1")}{" "}
                  <b>{t("settings.gdpr.modal.banner5y")}</b> {t("settings.gdpr.modal.bannerP2")}
                </span>
              </div>
              <div className="field">
                <label>{t("settings.gdpr.modal.typePre")} <b>STERGE</b> {t("settings.gdpr.modal.typePost")}</label>
                <input
                  className="input"
                  type="text"
                  placeholder="STERGE"
                  value={gdprConfirmText}
                  onChange={(e) => setGdprConfirmText(e.target.value)}
                  autoFocus
                />
              </div>
            </div>
            <div className="modal-foot">
              <button className="pill-btn" onClick={() => close1()}>{t("settings.gdpr.modal.cancel")}</button>
              <button
                className="btn-dark"
                style={{
                  background: "var(--red)",
                  opacity: gdprConfirmText.trim().toUpperCase() === "STERGE" && !gdprWiping ? 1 : 0.5,
                }}
                disabled={gdprConfirmText.trim().toUpperCase() !== "STERGE" || gdprWiping}
                onClick={() => void handleGdprWipe()}
              >
                {gdprWiping ? t("settings.gdpr.modal.wiping") : t("settings.gdpr.modal.confirm")}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* modal previzualizare șablon PDF — mock live (reacționează instant la setări) */}
      {tplPreviewOpen && (
        <div
          className={`modal-back ${closing2 ? "closing" : "show"}`}
          style={{ position: "fixed" }}
          onMouseDown={(e) => { if (e.target === e.currentTarget) close2(); }}
        >
          <div className="modal pdfwide">
            <div className="modal-head">
              <div>
                <div className="mt">{t("settings.template.modal.title")}</div>
                <div className="ms num">
                  {(activeCompany?.invoiceSeries ?? "FAC") + "-DEMO-0001"} · {t("settings.template.modal.live")}
                </div>
              </div>
              <button className="modal-x" onClick={() => close2()}>
                <Ic name="xMark" />
              </button>
            </div>
            <div className="modal-body" style={{ background: "var(--fill)" }}>
              <div className="pdf-sheet" style={{ "--acc": mockSecAcc } as React.CSSProperties}>
                <div className="pdf-top">
                  <div style={{ display: "flex", gap: 11 }}>
                    <div className="pdf-logo">{t("settings.template.modal.mock.logo")}</div>
                    <div>
                      <div className="pdf-co">{activeCompany?.legalName ?? t("settings.template.modal.mock.company")}</div>
                      <div className="pdf-co-meta">
                        {activeCompany ? `${activeCompany.vatPayer ? "RO " : ""}${activeCompany.cui}` : "RO 00000000"}
                        {activeCompany?.registryNumber ? ` · ${activeCompany.registryNumber}` : ""}
                        <br />
                        {activeCompany ? `${activeCompany.address}, ${activeCompany.city}` : t("settings.template.modal.mock.address")}
                      </div>
                    </div>
                  </div>
                  <div>
                    <div className="pdf-title" style={{ color: mockTitleAcc }}>{t("settings.template.modal.mock.invoiceTitle")}</div>
                    <div className="pdf-sub num">
                      {t("settings.template.modal.mock.no")} {(activeCompany?.invoiceSeries ?? "FAC") + "-DEMO-0001"} · {fmtRoUnix(Math.floor(Date.now() / 1000))}
                    </div>
                    {mockHeaderNote && <div className="pdf-note">{mockHeaderNote}</div>}
                  </div>
                </div>

                <div className="pdf-sec">{t("settings.template.modal.mock.buyer")}</div>
                {mockShowRules && <div className="pdf-rule" />}
                <div style={{ fontSize: 10.5, marginTop: 5 }}>{t("settings.template.modal.mock.buyerLine")}</div>

                <div className="pdf-sec">{t("settings.template.modal.mock.items")}</div>
                {mockShowRules && <div className="pdf-rule" />}
                <table className="pdf-tbl">
                  <thead>
                    <tr>
                      <th>{t("settings.template.modal.mock.name")}</th>
                      <th className="r">{t("settings.template.modal.mock.qty")}</th>
                      <th className="r">{t("settings.template.modal.mock.price")}</th>
                      <th className="r">{t("settings.template.modal.mock.vat")}</th>
                      <th className="r">{t("settings.template.modal.mock.amount")}</th>
                    </tr>
                  </thead>
                  <tbody>
                    <tr><td>{t("settings.template.modal.mock.row1")}</td><td className="r">10</td><td className="r">100,00</td><td className="r">21%</td><td className="r">1.000,00</td></tr>
                    <tr><td>{t("settings.template.modal.mock.row2")}</td><td className="r">5</td><td className="r">40,00</td><td className="r">11%</td><td className="r">200,00</td></tr>
                  </tbody>
                </table>

                {templateShowVatDetail && (
                  <div>
                    <div className="pdf-sec">{t("settings.template.modal.mock.vatDetail")}</div>
                    {mockShowRules && <div className="pdf-rule" />}
                    <table className="pdf-tbl">
                      <thead>
                        <tr><th>{t("settings.template.modal.mock.rate")}</th><th className="r">{t("settings.template.modal.mock.base")}</th><th className="r">{t("settings.template.modal.mock.vat")}</th></tr>
                      </thead>
                      <tbody>
                        <tr><td>21%</td><td className="r">1.000,00</td><td className="r">210,00</td></tr>
                        <tr><td>11%</td><td className="r">200,00</td><td className="r">22,00</td></tr>
                      </tbody>
                    </table>
                  </div>
                )}

                <div className="pdf-tot">
                  <div className="row"><span>{t("settings.template.modal.mock.subtotal")}</span><b>1.200,00 RON</b></div>
                  <div className="row"><span>{t("settings.template.modal.mock.vat")}</span><b>232,00 RON</b></div>
                  <div className="row grand"><span>{t("settings.template.modal.mock.total")}</span><span>1.432,00 RON</span></div>
                </div>
                {templateShowWords && (
                  <div className="pdf-words">{t("settings.template.modal.mock.words")}</div>
                )}

                {mockFooterNote && <div className="pdf-foot-note">{mockFooterNote}</div>}
              </div>
            </div>
            <div className="modal-foot">
              <span className="left muted" style={{ fontSize: 12 }}>
                {t("settings.template.modal.liveNote")}
              </span>
              <button
                className="pill-btn"
                disabled={previewingTemplate || !activeCompanyId}
                style={!activeCompanyId ? { opacity: 0.5 } : undefined}
                title={t("settings.template.modal.openRealTitle")}
                onClick={() => void handlePreviewTemplate()}
              >
                <Ic name="eye" />
                {previewingTemplate ? t("settings.template.modal.generating") : t("settings.template.modal.openReal")}
              </button>
              <button className="pill-btn" onClick={() => close2()}>{t("settings.common.close")}</button>
              <button
                className="btn-dark"
                disabled={savingTemplate}
                onClick={async () => { await handleSaveTemplate(); setTplPreviewOpen(false); }}
              >
                <Ic name="check" />{t("settings.template.modal.saveTemplate")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// ─── P2 Wave 1: AccountMappingPanel ───────────────────────────────────────────

function AccountMappingPanel({ companyId }: { companyId: string }) {
  const { t } = useTranslation();
  const qc = useQueryClient();

  const { data: rows = [], isLoading } = useQuery({
    queryKey: ["account-mapping", companyId],
    queryFn: () => api.accountMapping.list(companyId),
  });

  const [editingType, setEditingType] = useState<ProductType | null>(null);
  const [editForm, setEditForm] = useState<SetAccountMappingInput>({
    stockAccount: null,
    expenseAccount: null,
    incomeAccount: null,
    usesStock: true,
    retailCapable: false,
  });

  const setMut = useMutation({
    mutationFn: (args: { pt: ProductType; input: SetAccountMappingInput }) =>
      api.accountMapping.set(companyId, args.pt, args.input),
    onSuccess: () => {
      notify.success(t("products.accountMapping.saved"));
      void qc.invalidateQueries({ queryKey: ["account-mapping", companyId] });
      setEditingType(null);
    },
    onError: (e) => notify.error(formatError(e, t("products.accountMapping.saveError"))),
  });

  const resetMut = useMutation({
    mutationFn: (pt: ProductType) => api.accountMapping.reset(companyId, pt),
    onSuccess: () => {
      notify.success(t("products.accountMapping.resetDone"));
      void qc.invalidateQueries({ queryKey: ["account-mapping", companyId] });
    },
    onError: (e) => notify.error(formatError(e, t("products.accountMapping.saveError"))),
  });

  const startEdit = (row: EffectiveAccountMapping) => {
    setEditingType(row.productType as ProductType);
    setEditForm({
      stockAccount: row.stockAccount,
      expenseAccount: row.expenseAccount,
      incomeAccount: row.incomeAccount,
      usesStock: row.usesStock,
      retailCapable: row.retailCapable,
    });
  };

  return (
    <div className="scr-card" style={{ marginBottom: 14 }}>
      <div className="scr-toolbar">
        <div className="tt">{t("products.accountMapping.title")}</div>
      </div>
      <div className="card-pad" style={{ padding: "0 0 4px 0" }}>
        <p style={{ fontSize: 12, color: "var(--text-2)", padding: "8px 16px 0", margin: 0 }}>
          {t("products.accountMapping.subtitle")}
        </p>
      </div>
      {isLoading ? (
        <div style={{ padding: "12px 16px", fontSize: 12, color: "var(--text-2)" }}>…</div>
      ) : (
        <table className="scr-table">
          <thead>
            <tr>
              <th>{t("products.accountMapping.colType")}</th>
              <th className="r">{t("products.accountMapping.colStock")}</th>
              <th className="r">{t("products.accountMapping.colExpense")}</th>
              <th className="r">{t("products.accountMapping.colIncome")}</th>
              <th>{t("products.accountMapping.colUsesStock")}</th>
              <th></th>
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={row.productType}>
                <td>
                  {t(`products.productTypes.${row.productType}`)}
                  {row.isOverride && (
                    <span className="chip sent" style={{ marginLeft: 6, fontSize: 10 }}>
                      {t("products.accountMapping.overrideLabel")}
                    </span>
                  )}
                </td>
                <td className="r"><span className="doc">{row.stockAccount ?? "—"}</span></td>
                <td className="r"><span className="doc">{row.expenseAccount ?? "—"}</span></td>
                <td className="r"><span className="doc">{row.incomeAccount ?? "—"}</span></td>
                <td>{row.usesStock ? t("products.accountMapping.yes") : t("products.accountMapping.no")}</td>
                <td style={{ whiteSpace: "nowrap" }}>
                  <button
                    className="pill-btn"
                    style={{ marginRight: 4 }}
                    onClick={() => startEdit(row)}
                  >
                    {t("products.accountMapping.override")}
                  </button>
                  {row.isOverride && (
                    <button
                      className="pill-btn"
                      style={{ color: "var(--red)", borderColor: "rgba(220,38,38,.3)" }}
                      disabled={resetMut.isPending}
                      onClick={() => resetMut.mutate(row.productType as ProductType)}
                    >
                      {t("products.accountMapping.reset")}
                    </button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {/* inline edit form */}
      {editingType && (
        <div style={{ padding: "12px 16px", borderTop: "1px solid var(--line)" }}>
          <p style={{ fontSize: 12, fontWeight: 600, margin: "0 0 8px" }}>
            {t(`products.productTypes.${editingType}`)} — {t("products.accountMapping.override")}
          </p>
          <div className="fgrid" style={{ gridTemplateColumns: "repeat(3,1fr)" }}>
            {(["stockAccount", "expenseAccount", "incomeAccount"] as const).map((key) => (
              <div className="field" key={key}>
                <label>
                  {key === "stockAccount" ? t("products.accountMapping.colStock") :
                   key === "expenseAccount" ? t("products.accountMapping.colExpense") :
                   t("products.accountMapping.colIncome")}
                </label>
                <input
                  className="input num"
                  style={{ height: 30 }}
                  value={editForm[key] ?? ""}
                  onChange={(e) => setEditForm((f) => ({ ...f, [key]: e.target.value || null }))}
                  placeholder="—"
                />
              </div>
            ))}
          </div>
          <div style={{ display: "flex", gap: 8, marginTop: 8 }}>
            <button
              className="btn-dark"
              style={{ height: 30 }}
              disabled={setMut.isPending}
              onClick={() => setMut.mutate({ pt: editingType, input: editForm })}
            >
              <Ic name="check" />{t("products.accountMapping.override")}
            </button>
            <button
              className="pill-btn"
              style={{ height: 30 }}
              onClick={() => setEditingType(null)}
            >
              {t("settings.common.cancel", { defaultValue: "Anulează" })}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

// ─── P2 Wave 1: ProductGroupsPanel ────────────────────────────────────────────

function ProductGroupsPanel({ companyId }: { companyId: string }) {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [newName, setNewName] = useState("");

  const { data: groups = [], isLoading } = useQuery({
    queryKey: ["product-groups", companyId],
    queryFn: () => api.productGroups.list(companyId),
  });

  const createMut = useMutation({
    mutationFn: (name: string) => api.productGroups.create(companyId, { name }),
    onSuccess: () => {
      notify.success(t("products.groups.added"));
      setNewName("");
      void qc.invalidateQueries({ queryKey: ["product-groups", companyId] });
    },
    onError: (e) => notify.error(formatError(e, t("products.accountMapping.saveError"))),
  });

  const deleteMut = useMutation({
    mutationFn: (id: string) => api.productGroups.delete(id, companyId),
    onSuccess: () => {
      notify.success(t("products.groups.deleted"));
      void qc.invalidateQueries({ queryKey: ["product-groups", companyId] });
    },
    onError: (e) => notify.error(formatError(e, t("products.accountMapping.saveError"))),
  });

  return (
    <div className="scr-card" style={{ marginBottom: 14 }}>
      <div className="scr-toolbar">
        <div className="tt">{t("products.groups.title")}</div>
      </div>
      {isLoading ? (
        <div style={{ padding: "12px 16px", fontSize: 12, color: "var(--text-2)" }}>…</div>
      ) : (groups as ProductGroup[]).length === 0 ? (
        <div style={{ padding: "12px 16px", fontSize: 12, color: "var(--text-2)" }}>
          {t("products.groups.empty")}
        </div>
      ) : (
        <table className="scr-table">
          <tbody>
            {(groups as ProductGroup[]).map((g) => (
              <tr key={g.id}>
                <td>{g.name}</td>
                <td style={{ width: 48, textAlign: "right" }}>
                  <button
                    className="pill-btn"
                    style={{ color: "var(--red)", borderColor: "rgba(220,38,38,.3)" }}
                    disabled={deleteMut.isPending}
                    onClick={() => deleteMut.mutate(g.id)}
                    title={t("products.groups.deleteConfirm", { name: g.name })}
                  >
                    <svg className="sic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: '<path d="m14.74 9-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 0 1-2.244 2.077H8.084a2.25 2.25 0 0 1-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 0 0-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 0 1 3.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 0 0-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 0 0-7.5 0"/>' }} />
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <div style={{ padding: "10px 16px", display: "flex", gap: 8, alignItems: "center", borderTop: "1px solid var(--line)" }}>
        <input
          className="input"
          style={{ flex: 1, height: 30, fontSize: 12.5 }}
          placeholder={t("products.groups.namePlaceholder")}
          value={newName}
          onChange={(e) => setNewName(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter" && newName.trim()) createMut.mutate(newName.trim()); }}
        />
        <button
          className="btn-dark"
          style={{ height: 30 }}
          disabled={createMut.isPending || !newName.trim()}
          onClick={() => createMut.mutate(newName.trim())}
        >
          <Ic name="plus" />{t("products.groups.add")}
        </button>
      </div>
    </div>
  );
}

// ─── P2 Wave 7: PayrollConfigPanel ────────────────────────────────────────────

/** Standard code defaults — kept in sync with Rust consts for display fallback. */
const PAYROLL_DEFAULTS: Record<keyof Omit<SetPayrollConfigInput, never>, string> = {
  contCheltuieliSalarii: "641",
  contSalariiDatorate: "421",
  contCas: "4315",
  contCass: "4316",
  contImpozit: "444",
  contCheltuieliCam: "646",
  contCam: "436",
  contConcedii: "4373",
  contCheltuieliConcedii: "6458",
  contNetCasa: "5311",
  contNetBanca: "5121",
  diurnaInterna: "23.00",
  diurnaPlafonNeimpozabil: "57.50",
  diurnaCazare: "265.00",
};

type PayrollFormState = {
  contCheltuieliSalarii: string;
  contSalariiDatorate: string;
  contCas: string;
  contCass: string;
  contImpozit: string;
  contCheltuieliCam: string;
  contCam: string;
  contConcedii: string;
  contCheltuieliConcedii: string;
  contNetCasa: string;
  contNetBanca: string;
  diurnaInterna: string;
  diurnaPlafonNeimpozabil: string;
  diurnaCazare: string;
};

function mapToForm(cfg: PayrollAccountMap): PayrollFormState {
  return {
    contCheltuieliSalarii: cfg.cheltuieliSalarii,
    contSalariiDatorate: cfg.salariiDatorate,
    contCas: cfg.cas,
    contCass: cfg.cass,
    contImpozit: cfg.impozit,
    contCheltuieliCam: cfg.cheltuieliCam,
    contCam: cfg.cam,
    contConcedii: cfg.concedii,
    contCheltuieliConcedii: cfg.cheltuieliConcedii,
    contNetCasa: cfg.netCasa,
    contNetBanca: cfg.netBanca,
    diurnaInterna: cfg.diurnaInterna,
    diurnaPlafonNeimpozabil: cfg.diurnaPlafonNeimpozabil,
    diurnaCazare: cfg.diurnaCazare,
  };
}

function PayrollConfigPanel({ companyId }: { companyId: string }) {
  const { t } = useTranslation();
  const qc = useQueryClient();

  const { data: cfg, isLoading } = useQuery({
    queryKey: ["payroll-config", companyId],
    queryFn: () => api.payrollConfig.get(companyId),
  });

  const [form, setForm] = useState<PayrollFormState>(
    Object.fromEntries(
      Object.keys(PAYROLL_DEFAULTS).map((k) => [k, PAYROLL_DEFAULTS[k as keyof typeof PAYROLL_DEFAULTS]])
    ) as PayrollFormState
  );
  const [dirty, setDirty] = useState(false);

  // Sync form when server data arrives.
  useEffect(() => {
    if (cfg) {
      setForm(mapToForm(cfg));
      setDirty(false);
    }
  }, [cfg]);

  const setField = (key: keyof PayrollFormState, value: string) => {
    setForm((f) => ({ ...f, [key]: value }));
    setDirty(true);
  };

  const saveMut = useMutation({
    mutationFn: () => {
      // Any field equal to its code default is sent as null (→ removes override for that column).
      const input: SetPayrollConfigInput = {};
      (Object.keys(form) as (keyof PayrollFormState)[]).forEach((k) => {
        const v = form[k].trim() || null;
        const def = PAYROLL_DEFAULTS[k as keyof typeof PAYROLL_DEFAULTS];
        (input as Record<string, string | null>)[k] = (v === def || v === "") ? null : v;
      });
      return api.payrollConfig.set(companyId, input);
    },
    onSuccess: (updated) => {
      notify.success(t("settings.payrollConfig.saved"));
      setForm(mapToForm(updated));
      setDirty(false);
      void qc.invalidateQueries({ queryKey: ["payroll-config", companyId] });
    },
    onError: (e) => notify.error(formatError(e, t("settings.payrollConfig.saveError"))),
  });

  const resetMut = useMutation({
    mutationFn: () => api.payrollConfig.reset(companyId),
    onSuccess: (updated) => {
      notify.success(t("settings.payrollConfig.resetDone"));
      setForm(mapToForm(updated));
      setDirty(false);
      void qc.invalidateQueries({ queryKey: ["payroll-config", companyId] });
    },
    onError: (e) => notify.error(formatError(e, t("settings.payrollConfig.resetError"))),
  });

  /** Rows for the GL account map table. */
  const accountRows: { key: keyof PayrollFormState; labelKey: string; default: string }[] = [
    { key: "contCheltuieliSalarii",  labelKey: "settings.payrollConfig.accountMap.cheltuieliSalarii",  default: "641"  },
    { key: "contSalariiDatorate",    labelKey: "settings.payrollConfig.accountMap.salariiDatorate",    default: "421"  },
    { key: "contCas",                labelKey: "settings.payrollConfig.accountMap.cas",                default: "4315" },
    { key: "contCass",               labelKey: "settings.payrollConfig.accountMap.cass",               default: "4316" },
    { key: "contImpozit",            labelKey: "settings.payrollConfig.accountMap.impozit",            default: "444"  },
    { key: "contCheltuieliCam",      labelKey: "settings.payrollConfig.accountMap.cheltuieliCam",      default: "646"  },
    { key: "contCam",                labelKey: "settings.payrollConfig.accountMap.cam",                default: "436"  },
    { key: "contConcedii",           labelKey: "settings.payrollConfig.accountMap.concedii",           default: "4373" },
    { key: "contCheltuieliConcedii", labelKey: "settings.payrollConfig.accountMap.cheltuieliConcedii", default: "6458" },
    { key: "contNetCasa",            labelKey: "settings.payrollConfig.accountMap.netCasa",            default: "5311" },
    { key: "contNetBanca",           labelKey: "settings.payrollConfig.accountMap.netBanca",           default: "5121" },
  ];

  const diurnaRows: { key: keyof PayrollFormState; labelKey: string }[] = [
    { key: "diurnaInterna",           labelKey: "settings.payrollConfig.diurna.interna" },
    { key: "diurnaPlafonNeimpozabil", labelKey: "settings.payrollConfig.diurna.plafonNeimpozabil" },
    { key: "diurnaCazare",            labelKey: "settings.payrollConfig.diurna.cazare" },
  ];

  /** Read-only 2026 fiscal rates/ceilings card. */
  const rates2026 = [
    { label: t("settings.payrollConfig.rates2026.cas"),           value: "25%"             },
    { label: t("settings.payrollConfig.rates2026.cass"),          value: "10%"             },
    { label: t("settings.payrollConfig.rates2026.impozit"),       value: "10%"             },
    { label: t("settings.payrollConfig.rates2026.cam"),           value: "2,25%"           },
    // CCI 0,85% row removed — abolished OUG 79/2017; folded into CAM 2,25% since 1 Jan 2018.
    { label: t("settings.payrollConfig.rates2026.salariuMinimH1"), value: "4.050 lei"      },
    { label: t("settings.payrollConfig.rates2026.salariuMinimH2"), value: "4.325 lei"      },
    { label: t("settings.payrollConfig.rates2026.carveOutH1"),    value: "300 lei"         },
    { label: t("settings.payrollConfig.rates2026.carveOutH2"),    value: "200 lei"         },
    { label: t("settings.payrollConfig.rates2026.plafonScutireH1"), value: "4.300 lei"     },
    { label: t("settings.payrollConfig.rates2026.plafonScutireH2"), value: "4.600 lei"     },
    { label: t("settings.payrollConfig.rates2026.deducere"),
      value: t("settings.payrollConfig.rates2026.deducereFormula") },
  ];

  return (
    <div className="scr-card" style={{ marginBottom: 14 }}>
      <div className="scr-toolbar">
        <div className="tt">{t("settings.payrollConfig.title")}</div>
        {cfg?.isOverride && (
          <span className="chip sent" style={{ fontSize: 10, marginLeft: 8 }}>
            {t("settings.payrollConfig.override")}
          </span>
        )}
        <div className="spacer" />
        {dirty && (
          <button
            className="btn-dark"
            style={{ height: 28, fontSize: 12 }}
            disabled={saveMut.isPending}
            onClick={() => saveMut.mutate()}
          >
            {saveMut.isPending ? t("settings.payrollConfig.saving") : t("settings.payrollConfig.save")}
          </button>
        )}
        {cfg?.isOverride && (
          <button
            className="pill-btn"
            style={{ height: 28, fontSize: 12, color: "var(--red)", borderColor: "rgba(220,38,38,.3)" }}
            disabled={resetMut.isPending}
            onClick={() => resetMut.mutate()}
          >
            {t("settings.payrollConfig.reset")}
          </button>
        )}
      </div>

      {isLoading ? (
        <div style={{ padding: "12px 16px", fontSize: 12, color: "var(--text-2)" }}>…</div>
      ) : (
        <>
          {/* GL account map */}
          <div style={{ padding: "8px 16px 4px", fontSize: 11, fontWeight: 600, color: "var(--text-2)", textTransform: "uppercase", letterSpacing: "0.04em" }}>
            {t("settings.payrollConfig.accountMap.title")}
          </div>
          <p style={{ fontSize: 12, color: "var(--text-2)", padding: "0 16px 6px", margin: 0 }}>
            {t("settings.payrollConfig.subtitle")}
          </p>
          <table className="scr-table">
            <thead>
              <tr>
                <th>{t("settings.payrollConfig.accountMap.title")}</th>
                <th className="r" style={{ width: 160 }}>Cont</th>
              </tr>
            </thead>
            <tbody>
              {accountRows.map(({ key, labelKey, default: def }) => {
                const isCustom = form[key] !== def;
                return (
                  <tr key={key}>
                    <td style={{ fontSize: 12.5 }}>
                      {t(labelKey)}
                      <span className="doc" style={{ marginLeft: 8, fontSize: 11, color: "var(--text-2)" }}>
                        {def}
                      </span>
                    </td>
                    <td className="r">
                      <input
                        className="input num"
                        style={{ height: 26, width: 130, fontSize: 12, textAlign: "right",
                          borderColor: isCustom ? "var(--accent)" : undefined }}
                        value={form[key]}
                        onChange={(e) => setField(key, e.target.value)}
                        placeholder={def}
                      />
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>

          {/* Diurnă */}
          <div style={{ padding: "10px 16px 4px", fontSize: 11, fontWeight: 600, color: "var(--text-2)", textTransform: "uppercase", letterSpacing: "0.04em" }}>
            {t("settings.payrollConfig.diurna.title")}
          </div>
          <p style={{ fontSize: 12, color: "var(--text-2)", padding: "0 16px 6px", margin: 0 }}>
            {t("settings.payrollConfig.diurna.subtitle")}
          </p>
          <table className="scr-table">
            <tbody>
              {diurnaRows.map(({ key, labelKey }) => (
                <tr key={key}>
                  <td style={{ fontSize: 12.5 }}>{t(labelKey)}</td>
                  <td className="r">
                    <input
                      className="input num"
                      style={{ height: 26, width: 130, fontSize: 12, textAlign: "right" }}
                      value={form[key]}
                      onChange={(e) => setField(key, e.target.value)}
                    />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>

          {/* Read-only 2026 rates */}
          <div style={{ padding: "10px 16px 4px", fontSize: 11, fontWeight: 600, color: "var(--text-2)", textTransform: "uppercase", letterSpacing: "0.04em" }}>
            {t("settings.payrollConfig.rates2026.title")}
          </div>
          <table className="scr-table">
            <tbody>
              {rates2026.map(({ label, value }) => (
                <tr key={label}>
                  <td style={{ fontSize: 12.5 }}>{label}</td>
                  <td className="r">
                    <span className="doc" style={{ fontSize: 12.5, fontWeight: 600 }}>{value}</span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="pager">
            <span style={{ fontSize: 11, color: "var(--text-2)" }}>
              {t("settings.payrollConfig.rates2026.note")}
            </span>
          </div>
        </>
      )}
    </div>
  );
}
