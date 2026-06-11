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
import { useState, useEffect } from "react";
import { open, save, confirm } from "@tauri-apps/plugin-dialog";
import { openPath, openUrl } from "@tauri-apps/plugin-opener";
import { invoke } from "@tauri-apps/api/core";
import { useNavigate } from "@tanstack/react-router";

import { Ic } from "@/components/shared/Ic";
import { queryKeys } from "@/lib/queries";
import { api } from "@/lib/tauri";
import { useAppStore, type ThemeMode, type DensityMode } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { Company } from "@/types";

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
  const navigate = useNavigate();

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
      if (tokenToSave) setSmartbillConfigured(true);
      setSmartbillSaved(true);
      setTimeout(() => setSmartbillSaved(false), 3000);
    } catch {
      notify.error("Eroare la salvare credențiale SmartBill.");
    } finally {
      setSavingSmartbill(false);
    }
  };

  const handlePreviewTemplate = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
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
      await openPath(path);
    } catch (e) {
      notify.error(formatError(e, "Nu s-a putut genera previzualizarea."));
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
      notify.error(formatError(e, "Eroare la salvarea șablonului de factură."));
    } finally {
      setSavingTemplate(false);
    }
  };

  /** Head action "Salvează modificările" — saves all form-based sections at once. */
  const handleSaveAll = async () => {
    await handleSaveTemplate();
    await handleSaveAnafAdvanced();
    if (activeCompanyId) await handleSaveSmartbill();
    notify.success("Modificările au fost salvate.");
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

  const handleCheckUpdate = async () => {
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
  };

  // ── Archive handlers ──────────────────────────────────────────────────────────

  const handleExportArchiveZip = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    try {
      const path = await api.archive.exportZip(activeCompanyId);
      notify.success(`Arhivă exportată: ${path}`);
    } catch (err) {
      notify.error(formatError(err, "Exportul arhivei a eșuat."));
    }
  };

  const handleOpenArchiveFolder = () => {
    api.system.openArchiveFolder().catch((e) =>
      notify.error(formatError(e, "Nu s-a putut deschide folderul arhivei."))
    );
  };

  const handleChangeArchiveLocation = async () => {
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
      notify.success(`Backup salvat: ${path}`);
    } catch (e) {
      notify.error(formatError(e, "Exportul backup-ului a eșuat."));
    }
  };

  const handleVerifyIntegrity = async () => {
    if (!activeCompanyId) { notify.warn("Selectați o companie activă."); return; }
    try {
      const result = await api.archive.verifyIntegrity(activeCompanyId);
      if (result.ok) {
        notify.success(`Arhiva este integră. ${result.checked} fișiere verificate.`);
      } else {
        notify.error(
          `Fișiere lipsă (${result.missing.length} din ${result.checked}` +
          (result.missingUnderRetention > 0
            ? `, ${result.missingUnderRetention} sub termenul legal de păstrare de 5 ani — L82/1991`
            : "") +
          `): ` +
          result.missing.slice(0, 5).join(", ") +
          (result.missing.length > 5 ? " …" : "")
        );
      }
    } catch (e) {
      notify.error(formatError(e, "Verificarea integrității a eșuat."));
    }
  };

  const handleRestoreBackup = async () => {
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
  };

  // ── GDPR handlers ─────────────────────────────────────────────────────────────

  const handleGdprExport = async () => {
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
          msg + "\n\nConfirmați că ați exportat/arhivat documentele și vă asumați " +
          "răspunderea pentru respectarea termenului legal de păstrare?",
          { title: "Termen legal de păstrare (L82/1991)", kind: "warning" }
        );
        if (!ack) return;
        await api.gdpr.wipeAll(true);
      }
      setGdprOpen(false);
      notify.success("Toate datele dvs. au fost șterse. Aplicația va reporni.");
      setTimeout(() => { window.location.reload(); }, 2000);
    } catch (e) {
      notify.error(formatError(e, "Ștergerea datelor a eșuat."));
    } finally {
      setGdprWiping(false);
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

  const noCompanyWarn = () => notify.warn("Selectați o companie activă.");

  return (
    <div className="main-inner wide">
      {/* page head */}
      <div className="page-head">
        <div>
          <h1>Setări</h1>
          <p className="sub">
            {activeCompany ? activeCompany.legalName : "Nicio companie activă"}
            {appInfo ? ` · versiune ${appInfo.version}` : ""}
          </p>
        </div>
        <div className="head-actions">
          <button className="btn-dark" onClick={() => void handleSaveAll()}>
            <Ic name="check" />Salvează modificările
          </button>
        </div>
      </div>

      <div className="cols-2-even">
        {/* ════════ left column ════════ */}
        <div>
          {/* ANAF SPV */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">Conectare ANAF SPV</div>
              <div className="spacer" />
              {activeCompanyId ? (
                isAnafAuthenticated ? (
                  <span className="chip paid"><Ic name="checkC" cls="sic" />Conectat</span>
                ) : (
                  <span className="chip sent"><Ic name="dot" cls="sic" />Neconectat</span>
                )
              ) : (
                <span className="muted" style={{ fontSize: 12 }}>fără companie activă</span>
              )}
            </div>
            <SetRow
              title="Autorizare OAuth ANAF"
              desc={
                isAnafAuthenticated
                  ? "token valabil · reînnoire automată"
                  : "autorizați aplicația în SPV pentru trimitere și sincronizare e-Factura"
              }
            >
              {isAnafAuthenticated ? (
                <button
                  className="pill-btn"
                  style={{ color: "var(--red)", borderColor: "rgba(220,38,38,.3)" }}
                  disabled={logoutAnaf.isPending}
                  onClick={() => { if (!activeCompanyId) { noCompanyWarn(); return; } logoutAnaf.mutate(); }}
                >
                  {logoutAnaf.isPending ? "Se deconectează…" : "Deconectează"}
                </button>
              ) : (
                <button
                  className="pill-btn"
                  disabled={authorizeAnaf.isPending}
                  onClick={() => { if (!activeCompanyId) { noCompanyWarn(); return; } authorizeAnaf.mutate(); }}
                >
                  <Ic name="shield" />
                  {authorizeAnaf.isPending ? "Se autorizează…" : "Conectează"}
                </button>
              )}
            </SetRow>
            <SetRow
              title="Client secret OAuth"
              desc="stocat în keychain-ul sistemului · valoarea nu este afișată niciodată"
            >
              {anafHasSecret ? (
                <span className="chip paid"><ChipCheck />Configurat</span>
              ) : (
                <span className="chip wait"><Ic name="clock" cls="sic" />Lipsește</span>
              )}
              <button className="pill-btn" onClick={() => setAnafAdvancedOpen(true)}>Schimbă</button>
            </SetRow>
            <SetRow
              title="Mediu ANAF"
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
                  Test
                </div>
                <div
                  className={`tab${anafTestMode ? "" : " active"}`}
                  onClick={() => void handleTestModeChange(false)}
                >
                  Producție
                </div>
              </div>
            </SetRow>
            <SetRow
              title="Certificat calificat"
              descNum
              desc={
                activeCert
                  ? `valabil până la ${fmtRoUnix(activeCert.expiresAt)} · reînnoibil până la ${fmtRoUnix(activeCert.refreshableUntil)}`
                  : "niciun certificat înregistrat — se reține automat la prima autorizare SPV"
              }
            >
              {activeCert ? (
                certValid ? (
                  <span className="chip paid"><ChipCheck />Valabil</span>
                ) : (
                  <span className="chip late"><Ic name="xMark" cls="sic" />Expirat</span>
                )
              ) : (
                <span className="muted">—</span>
              )}
            </SetRow>
            <SetRow title="Sincronizare automată SPV" desc="la fiecare 6 ore · mesaje + facturi primite">
              {/* propunere — neimplementat: interval de sincronizare configurabil */}
              <span className="tog on" onClick={() => notify.info("În curând.")} />
            </SetRow>
            <SetRow
              title="Configurare avansată OAuth"
              desc="Client ID, Redirect URI și URL-urile ANAF pentru aplicația OAuth proprie"
            >
              <button className="pill-btn" onClick={() => setAnafAdvancedOpen((v) => !v)}>
                {anafAdvancedOpen ? "Ascunde" : "Configurează"}
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
                    Pentru conectarea la SPV, ANAF cere o aplicație OAuth proprie: în SPV → „Gestionare
                    profil OAuth” înregistrați aplicația (cu Redirect URI-ul de mai jos) și veți primi un
                    <b> Client ID</b> și un <b>Client Secret</b>. Completați-le aici. Restul câmpurilor pot
                    rămâne goale (valori implicite).
                  </span>
                </div>
                <div className="fgrid">
                  <Fld label="Client ID" hint="Generat de ANAF la înregistrarea aplicației OAuth">
                    <input
                      className="input num"
                      placeholder="client_id de la ANAF"
                      value={anafClientId}
                      onChange={(e) => setAnafClientId(e.target.value)}
                    />
                  </Fld>
                  <Fld
                    label="Client Secret"
                    hint={anafHasSecret ? "Salvat în keychain — lăsați gol pentru a-l păstra" : "Generat de ANAF; stocat securizat în keychain"}
                  >
                    <input
                      type="password"
                      className="input num"
                      placeholder={anafHasSecret ? "•••••••• (configurat)" : "client_secret de la ANAF"}
                      value={anafClientSecret}
                      onChange={(e) => setAnafClientSecret(e.target.value)}
                    />
                  </Fld>
                  <Fld label="Redirect URI">
                    <input
                      className="input num"
                      placeholder="http://localhost:8787/callback (implicit)"
                      value={anafRedirectUri}
                      onChange={(e) => setAnafRedirectUri(e.target.value)}
                    />
                  </Fld>
                  <Fld label="Port callback">
                    <input
                      className="input num"
                      placeholder="8787 (implicit)"
                      value={anafCallbackPort}
                      onChange={(e) => setAnafCallbackPort(e.target.value)}
                    />
                  </Fld>
                  <Fld label="URL autorizare">
                    <input
                      className="input num"
                      placeholder="https://logincert.anaf.ro/anaf-oauth2/v1/authorize"
                      value={anafAuthorizeUrl}
                      onChange={(e) => setAnafAuthorizeUrl(e.target.value)}
                    />
                  </Fld>
                  <Fld label="URL token">
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
                    {anafAdvancedSaving ? "Se salvează…" : "Salvează configurarea"}
                  </button>
                  {anafAdvancedSaved && <span className="okk">Salvat</span>}
                </div>
              </div>
            )}
          </div>

          {/* companie activă */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">Companie activă</div>
              <div className="spacer" />
              <span className="muted" style={{ fontSize: 12 }}>
                {activeCompany ? activeCompany.legalName : "neselectată"}
              </span>
            </div>
            <SetRow
              title="Compania de lucru"
              desc="seria de facturare și numerotarea se configurează pe companie (Companii › Editează)"
            >
              {companies.length === 0 ? (
                <span className="muted" style={{ fontSize: 12.5 }}>Nicio companie configurată</span>
              ) : (
                <select
                  className="select"
                  style={{ width: 230 }}
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
              <button className="pill-btn" onClick={() => void navigate({ to: "/companies" })}>
                Gestionează
              </button>
            </SetRow>
            {activeCompany && (
              <>
                <SetRow
                  title="Serie facturi · ultimul număr"
                  descNum
                  desc={`${activeCompany.invoiceSeries} · ${String(activeCompany.lastInvoiceNumber).padStart(4, "0")} (următorul: ${String(activeCompany.lastInvoiceNumber + 1).padStart(4, "0")})`}
                >
                  <button
                    className="pill-btn"
                    onClick={() => void navigate({ to: "/companies/$id/edit", params: { id: activeCompany.id } })}
                  >
                    Editează compania
                  </button>
                </SetRow>
                <SetRow title="CUI" descNum desc={activeCompany.cui}>
                  <span className="muted" style={{ fontSize: 12.5 }}>
                    {activeCompany.vatPayer ? "plătitor de TVA" : "neplătitor de TVA"}
                  </span>
                </SetRow>
                <SetRow title="SPV activat" desc="compania trimite și primește documente prin SPV">
                  {activeCompany.spvEnabled ? (
                    <span className="chip paid"><ChipCheck />Da</span>
                  ) : (
                    <span className="chip sent"><Ic name="dot" cls="sic" />Nu</span>
                  )}
                </SetRow>
              </>
            )}
          </div>

          {/* cote TVA */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">Cote TVA — istoric legislativ (Legea 141/2025)</div>
              <div className="spacer" />
              <button
                className="see-all"
                style={{ height: "auto", padding: 0, border: 0, background: "transparent" }}
                onClick={() => void navigate({ to: "/vat-rates" })}
              >
                Catalog complet<Ic name="chevR" cls="ic" />
              </button>
            </div>
            <table className="scr-table">
              <thead>
                <tr>
                  <th>Perioadă</th>
                  <th className="r">Standard</th>
                  <th className="r">Redusă</th>
                  <th className="r">Redusă 2</th>
                  <th>Status</th>
                </tr>
              </thead>
              <tbody>
                <tr>
                  <td className="num">până la 31 iul 2025</td>
                  <td className="r num">19%</td>
                  <td className="r num">9%</td>
                  <td className="r num">5%</td>
                  <td><span className="chip sent">istoric</span></td>
                </tr>
                <tr>
                  <td className="num">de la 01 aug 2025</td>
                  <td className="r num"><b>21%</b></td>
                  <td className="r num"><b>11%</b></td>
                  <td className="r num">—</td>
                  <td><span className="chip paid"><ChipCheck />în vigoare</span></td>
                </tr>
              </tbody>
            </table>
            <div className="pager">
              <span>Facturile și stornările pentru perioade anterioare folosesc automat cotele valabile la data faptului generator.</span>
              <span></span>
            </div>
          </div>

          {/* backup & restaurare */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">Backup &amp; restaurare</div>
              <div className="spacer" />
              <span className="muted num" style={{ fontSize: 12 }}>arhivă: {fmtBytes(archiveSize ?? 0)}</span>
            </div>
            <SetRow
              title="Backup complet (ZIP)"
              desc="bază de date + arhivă de documente — alegeți unde se salvează fișierul"
            >
              <button className="pill-btn" onClick={() => void handleExportBackup()}>
                <Ic name="dl" />Descarcă
              </button>
            </SetRow>
            <SetRow
              title="Export arhivă companie (XML + PDF)"
              desc="exportă fișierele XML și PDF ale companiei active într-un ZIP"
            >
              <button
                className="pill-btn"
                style={!activeCompanyId ? { opacity: 0.5 } : undefined}
                onClick={() => void handleExportArchiveZip()}
              >
                <Ic name="dl" />Exportă
              </button>
            </SetRow>
            <SetRow title="Folder arhivă" desc="deschide locația arhivei sau mută arhiva în alt folder">
              <button className="pill-btn" onClick={handleOpenArchiveFolder}>Deschide folder</button>
              <button className="pill-btn" onClick={() => void handleChangeArchiveLocation()}>
                Schimbă locația
              </button>
            </SetRow>
            <SetRow
              title="Verificare integritate"
              desc="verifică dacă toate fișierele arhivate ale companiei active există pe disc"
            >
              <button
                className="pill-btn"
                style={!activeCompanyId ? { opacity: 0.5 } : undefined}
                onClick={() => void handleVerifyIntegrity()}
              >
                Verifică
              </button>
            </SetRow>
            <SetRow
              title="Restaurează din ZIP"
              desc="restaurarea suprascrie datele curente și repornește aplicația"
            >
              <button className="pill-btn" onClick={() => void handleRestoreBackup()}>
                <Ic name="docUp" />Încarcă ZIP
              </button>
            </SetRow>
          </div>

          {/* GDPR */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar"><div className="tt">Date personale (GDPR)</div></div>
            <SetRow
              title="Export date personale"
              desc="arhivă ZIP cu baza de date + arhiva de documente (export_all_my_data)"
            >
              <button className="pill-btn" onClick={() => void handleGdprExport()}>Exportă</button>
            </SetRow>
            <SetRow
              title="Politica de confidențialitate"
              desc="lucaris.ro/privacy — cum sunt stocate și folosite datele dvs."
            >
              <button
                className="pill-btn"
                onClick={() => { openUrl("https://lucaris.ro/privacy").catch(() => {}); }}
              >
                Deschide
              </button>
            </SetRow>
            <SetRow
              title="Ștergere totală cont și date"
              danger
              desc={
                <>
                  documentele fiscale (facturi, jurnale, declarații) rămân arhivate <b>5 ani</b> conform
                  obligației legale de păstrare — se șterg doar datele personale neobligatorii
                </>
              }
            >
              <button
                className="pill-btn"
                style={{ color: "var(--red)", borderColor: "rgba(220,38,38,.3)" }}
                onClick={() => { setGdprConfirmText(""); setGdprOpen(true); }}
              >
                Șterge tot
              </button>
            </SetRow>
          </div>

          {/* jurnal activitate */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar">
              <div className="tt">Jurnal activitate</div>
              <div className="spacer" />
              <button
                className="pill-btn"
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
                <Ic name="dl" />Export CSV
              </button>
            </div>
            {activityLog.length === 0 ? (
              <div style={{ padding: "20px 16px", textAlign: "center", fontSize: 12.5, color: "var(--text-2)" }}>
                Nicio activitate înregistrată.
              </div>
            ) : (
              <table className="scr-table">
                <thead>
                  <tr>
                    <th style={{ width: 150 }}>Timp</th>
                    <th>Sarcină</th>
                    <th>Rezultat</th>
                  </tr>
                </thead>
                <tbody>
                  {activityLog.slice(0, 20).map((entry) => (
                    <tr key={entry.id}>
                      <td className="num" style={{ fontSize: 11.5, color: "var(--text-2)" }}>
                        {new Date(entry.createdAt * 1000).toLocaleString("ro-RO")}
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
              <div className="tt">Șablon factură (PDF)</div>
              <div className="spacer" />
              <span className="muted" style={{ fontSize: 12 }}>aspectul vizual al PDF-ului generat</span>
            </div>
            <div className="card-pad">
              <div className="fgrid" style={{ gridTemplateColumns: "1fr" }}>
                <Fld label="Preset" hint="Definește elementele colorate în PDF.">
                  <select
                    className="select"
                    value={templatePreset}
                    onChange={(e) => setTemplatePreset(e.target.value)}
                  >
                    <option value="clasic">Clasic (negru, implicit)</option>
                    <option value="modern">Modern (accent pe titlu, secțiuni, linii)</option>
                    <option value="minimal">Minimal (accent doar pe titlu)</option>
                  </select>
                </Fld>
                <Fld label="Culoare accent" hint="Aplicată conform preset-ului ales (#RRGGBB).">
                  <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                    <input
                      type="color"
                      value={accentValid ? templateAccent : "#000000"}
                      onChange={(e) => setTemplateAccent(e.target.value.toUpperCase())}
                      style={{ width: 46, height: 34, padding: 2, border: "1px solid var(--line)", borderRadius: 8, cursor: "pointer", flex: "none" }}
                      title="Selectați culoarea accent"
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
                <Fld label="Antet personalizat" hint="Sub data emiterii — slogan / mențiuni legale (max 2 rânduri).">
                  <textarea
                    className="input"
                    rows={2}
                    maxLength={240}
                    value={templateHeaderNote}
                    onChange={(e) => setTemplateHeaderNote(e.target.value)}
                    placeholder="Capital social: 200 lei · J12/345/2020"
                  />
                </Fld>
                <Fld label="Subsol personalizat" hint="La finalul facturii — mulțumiri / termeni de plată (max 3 rânduri).">
                  <textarea
                    className="input"
                    rows={3}
                    maxLength={400}
                    value={templateFooterNote}
                    onChange={(e) => setTemplateFooterNote(e.target.value)}
                    placeholder={"Vă mulțumim pentru colaborare!\nPlata în 15 zile de la emitere."}
                  />
                </Fld>
              </div>
            </div>
            <SetRow title="Suma în litere" desc="Afișează totalul scris în cuvinte sub TOTAL.">
              <span
                className={`tog${templateShowWords ? " on" : ""}`}
                onClick={() => setTemplateShowWords((v) => !v)}
              />
            </SetRow>
            <SetRow title="Detaliu TVA" desc="Afișează tabelul cu baze impozabile pe cote.">
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
                {savingTemplate ? "Se salvează…" : "Salvează"}
              </button>
              <button className="pill-btn" onClick={() => setTplPreviewOpen(true)}>
                <Ic name="eye" />Previzualizează PDF demo
              </button>
              {templateSaved && <span className="okk">Salvat</span>}
            </div>
          </div>

          {/* integrări */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar"><div className="tt">Integrări</div></div>
            <SetRow
              title="SmartBill — trimitere facturi"
              desc="trimite factura curentă în contul SmartBill (per factură) · necesită user + token API"
            >
              {smartbillConfigured ? (
                <span className="chip paid"><ChipCheck />Configurat</span>
              ) : (
                <span className="chip sent"><Ic name="dot" cls="sic" />Neconfigurat</span>
              )}
            </SetRow>
            <div className="card-pad" style={{ borderTop: "1px solid var(--line)" }}>
              <div className="fgrid">
                <Fld label="Utilizator (email)">
                  <input
                    className="input"
                    type="email"
                    placeholder="email@firma.ro"
                    value={smartbillUser}
                    onChange={(e) => setSmartbillUser(e.target.value)}
                  />
                </Fld>
                <Fld label="Token API" hint="SmartBill → Setări → Cont → Token API">
                  <input
                    className="input num"
                    type="password"
                    placeholder="Token din contul SmartBill"
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
                  {savingSmartbill ? "Se salvează…" : "Salvează credențiale"}
                </button>
                {smartbillSaved && <span className="okk">Salvat</span>}
              </div>
            </div>
          </div>

          {/* licență + temă */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar"><div className="tt">Licență &amp; aspect</div></div>
            <SetRow
              title={
                licenseLoading
                  ? "Licență"
                  : license
                    ? `Plan ${TIER_LABELS[license.tier] ?? license.tier} — licență activă`
                    : "Nicio licență activă"
              }
              descNum
              desc={
                license
                  ? `${license.email ?? "—"} · expiră ${fmtRoUnix(license.expiresAt)} · ${licenseDaysLeft} zile rămase · legată de acest dispozitiv`
                  : "porniți trial-ul gratuit din meniul Ajutor sau activați o cheie"
              }
            >
              {license && (
                license.isExpired ? (
                  <span className="chip late"><Ic name="xMark" cls="sic" />Expirată</span>
                ) : (
                  <span className="chip paid"><ChipCheck />Activă</span>
                )
              )}
              <button
                className="pill-btn"
                onClick={() => { setShowLicenseActivate((v) => !v); setLicenseActivateError(null); }}
              >
                {license ? "Activează altă cheie" : "Activează cheie"}
              </button>
              <button className="pill-btn" onClick={() => void openPurchase()}>Cumpără licență</button>
            </SetRow>
            {showLicenseActivate && (
              <div className="card-pad" style={{ borderTop: "1px solid var(--line)" }}>
                <div className="fgrid">
                  <Fld label="Cheie licență">
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
                  <Fld label="Email achiziție">
                    <input
                      className="input"
                      type="email"
                      placeholder="office@firma.ro"
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
                      if (!licenseKeyInput.trim()) { setLicenseActivateError("Introduceți cheia de licență."); return; }
                      if (!licenseEmailInput.trim()) { setLicenseActivateError("Introduceți emailul de achiziție."); return; }
                      licenseActivateMutation.mutate();
                    }}
                  >
                    {licenseActivateMutation.isPending ? "Se activează…" : "Activează"}
                  </button>
                  <button
                    className="pill-btn"
                    onClick={() => { setShowLicenseActivate(false); setLicenseActivateError(null); }}
                  >
                    Anulează
                  </button>
                </div>
              </div>
            )}
            <SetRow title="Temă interfață" desc="comutați între tema luminoasă, întunecată sau sistem">
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
            <SetRow title="Densitate rânduri" desc="înălțimea rândurilor în tabele și liste">
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
            <div className="scr-toolbar"><div className="tt">Notificări</div></div>
            {NOTIF_TYPES.map(({ key, label }) => (
              <SetRow key={key} title={label}>
                <select
                  className="select"
                  style={{ width: 170, height: 30, fontSize: 12.5 }}
                  value={notifPrefMap[key]}
                  onChange={(e) => void handleNotifPrefChange(key, e.target.value)}
                >
                  <option value="os">Desktop + In-app</option>
                  <option value="inapp">Doar in-app</option>
                  <option value="off">Dezactivat</option>
                </select>
              </SetRow>
            ))}
            <SetRow title="Ore liniștite" desc="dezactivează notificările OS între 22:00 și 07:00">
              <span
                className={`tog${(quietHoursSetting ?? "0") === "1" ? " on" : ""}`}
                onClick={() => void handleNotifToggle("quiet_hours", (quietHoursSetting ?? "0") !== "1")}
              />
            </SetRow>
          </div>

          {/* suport și feedback */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar"><div className="tt">Suport și feedback</div></div>
            <div className="card-pad">
              <Fld label="Mesaj (opțional)">
                <textarea
                  className="input"
                  rows={4}
                  value={feedbackMsg}
                  onChange={(e) => setFeedbackMsg(e.target.value)}
                  placeholder="Descrieți problema sau sugestia (diagnosticul se atașează automat)…"
                />
              </Fld>
              <div style={{ marginTop: 10, padding: "8px 10px", border: "1px dashed var(--line)", borderRadius: 8, fontSize: 12, color: "var(--text-2)", lineHeight: 1.5 }}>
                <b>Atașăm automat:</b> versiunea {appInfo?.version ?? "—"}, sistemul de operare, machine ID
                anonimizat, ultimele 50 linii log. La click se deschide clientul dvs. de email.
              </div>
              <div style={{ display: "flex", gap: 8, marginTop: 12 }}>
                <button
                  className="btn-dark"
                  style={{ height: 34 }}
                  disabled={feedbackSending}
                  onClick={() => void sendFeedback()}
                >
                  <Ic name="mail" />
                  {feedbackSending ? "Pregătesc…" : "Trimite feedback"}
                </button>
                <button className="pill-btn" onClick={() => void openPurchase()}>Cumpără licență</button>
              </div>
            </div>
          </div>

          {/* informații aplicație */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar"><div className="tt">Informații aplicație</div></div>
            {appInfoLoading ? (
              <div style={{ padding: "16px", fontSize: 12.5, color: "var(--text-2)" }}>Se încarcă…</div>
            ) : appInfo ? (
              <>
                <SetRow title="Versiune" descNum desc={appInfo.version} />
                <SetRow
                  title="Director date"
                  descNum
                  desc={<span style={{ wordBreak: "break-all" }}>{appInfo.appDataDir}</span>}
                />
                <SetRow
                  title="Bază de date"
                  descNum
                  desc={<span style={{ wordBreak: "break-all" }}>{appInfo.dbPath}</span>}
                />
                <SetRow title="Actualizări" desc={updateStatus ?? "verificați dacă există o versiune nouă"}>
                  <button
                    className="pill-btn"
                    disabled={checkingUpdate}
                    onClick={() => void handleCheckUpdate()}
                  >
                    <Ic name="sync" />
                    {checkingUpdate ? "Se verifică…" : "Verifică actualizări"}
                  </button>
                </SetRow>
              </>
            ) : null}
          </div>

          {/* sistem */}
          <div className="scr-card" style={{ marginBottom: 14 }}>
            <div className="scr-toolbar"><div className="tt">Sistem</div></div>
            <SetRow
              title="Pornire automată la login"
              desc="pornește aplicația automat la autentificarea în sistem"
            >
              <span
                className={`tog${autostartEnabled ? " on" : ""}`}
                onClick={async () => {
                  try {
                    await api.system.setAutostart(!autostartEnabled);
                    void queryClient.invalidateQueries({ queryKey: queryKeys.system.autostart });
                  } catch (err) {
                    notify.error(formatError(err, "Nu s-a putut modifica setarea de pornire automată."));
                  }
                }}
              />
            </SetRow>
          </div>

          {/* dezvoltare (DEV only) */}
          {import.meta.env.DEV && (
            <div className="scr-card" style={{ marginBottom: 14 }}>
              <div className="scr-toolbar"><div className="tt">Dezvoltare</div></div>
              <SetRow title="Date demo" desc="populează baza de date cu date de test (doar DB gol)">
                <button className="pill-btn" onClick={() => void handleDevSeed()}>
                  Populează DB cu date demo
                </button>
              </SetRow>
            </div>
          )}
        </div>
      </div>

      {/* modal GDPR — ștergere totală (type STERGE to confirm) */}
      {gdprOpen && (
        <div
          className="modal-back show"
          style={{ position: "fixed" }}
          onMouseDown={(e) => { if (e.target === e.currentTarget) setGdprOpen(false); }}
        >
          <div className="modal">
            <div className="modal-head">
              <div>
                <div className="mt" style={{ color: "var(--red)" }}>Ștergere totală — confirmare</div>
                <div className="ms">Acțiunea este ireversibilă</div>
              </div>
              <button className="modal-x" onClick={() => setGdprOpen(false)}>
                <Ic name="xMark" />
              </button>
            </div>
            <div className="modal-body">
              <div className="banner danger" style={{ marginBottom: 14 }}>
                <svg className="ic" viewBox="0 0 24 24" dangerouslySetInnerHTML={{ __html: WARN_TRI }} />
                <span>
                  <b>Retenție legală:</b> facturile, jurnalele și declarațiile depuse NU pot fi șterse —
                  legea impune păstrarea lor <b>5 ani</b> de la sfârșitul exercițiului financiar. Acestea
                  rămân arhivate în mod read-only. Se șterg definitiv: contul de utilizator, preferințele,
                  contactele nefacturate și datele de marketing.
                </span>
              </div>
              <div className="field">
                <label>Tastați <b>STERGE</b> pentru confirmare</label>
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
              <button className="pill-btn" onClick={() => setGdprOpen(false)}>Renunță</button>
              <button
                className="btn-dark"
                style={{
                  background: "var(--red)",
                  opacity: gdprConfirmText.trim().toUpperCase() === "STERGE" && !gdprWiping ? 1 : 0.5,
                }}
                disabled={gdprConfirmText.trim().toUpperCase() !== "STERGE" || gdprWiping}
                onClick={() => void handleGdprWipe()}
              >
                {gdprWiping ? "Se șterge…" : "Șterge definitiv"}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* modal previzualizare șablon PDF — mock live (reacționează instant la setări) */}
      {tplPreviewOpen && (
        <div
          className="modal-back show"
          style={{ position: "fixed" }}
          onMouseDown={(e) => { if (e.target === e.currentTarget) setTplPreviewOpen(false); }}
        >
          <div className="modal pdfwide">
            <div className="modal-head">
              <div>
                <div className="mt">Previzualizare PDF demo</div>
                <div className="ms num">
                  {(activeCompany?.invoiceSeries ?? "FAC") + "-DEMO-0001"} · reacționează live la setările șablonului
                </div>
              </div>
              <button className="modal-x" onClick={() => setTplPreviewOpen(false)}>
                <Ic name="xMark" />
              </button>
            </div>
            <div className="modal-body" style={{ background: "#F4F4F5" }}>
              <div className="pdf-sheet" style={{ "--acc": mockSecAcc } as React.CSSProperties}>
                <div className="pdf-top">
                  <div style={{ display: "flex", gap: 11 }}>
                    <div className="pdf-logo">SIGLA</div>
                    <div>
                      <div className="pdf-co">{activeCompany?.legalName ?? "Compania mea SRL"}</div>
                      <div className="pdf-co-meta">
                        {activeCompany ? `${activeCompany.vatPayer ? "RO " : ""}${activeCompany.cui}` : "RO 00000000"}
                        {activeCompany?.registryNumber ? ` · ${activeCompany.registryNumber}` : ""}
                        <br />
                        {activeCompany ? `${activeCompany.address}, ${activeCompany.city}` : "Str. Exemplu nr. 1, București"}
                      </div>
                    </div>
                  </div>
                  <div>
                    <div className="pdf-title" style={{ color: mockTitleAcc }}>FACTURĂ</div>
                    <div className="pdf-sub num">
                      Nr. {(activeCompany?.invoiceSeries ?? "FAC") + "-DEMO-0001"} · {fmtRoUnix(Math.floor(Date.now() / 1000))}
                    </div>
                    {mockHeaderNote && <div className="pdf-note">{mockHeaderNote}</div>}
                  </div>
                </div>

                <div className="pdf-sec">Cumpărător</div>
                {mockShowRules && <div className="pdf-rule" />}
                <div style={{ fontSize: 10.5, marginTop: 5 }}>Mavericks SRL · RO 22418890 · Cluj-Napoca, jud. Cluj</div>

                <div className="pdf-sec">Articole</div>
                {mockShowRules && <div className="pdf-rule" />}
                <table className="pdf-tbl">
                  <thead>
                    <tr>
                      <th>Denumire</th>
                      <th className="r">Cant.</th>
                      <th className="r">Preț</th>
                      <th className="r">TVA</th>
                      <th className="r">Valoare</th>
                    </tr>
                  </thead>
                  <tbody>
                    <tr><td>Servicii consultanță</td><td className="r">10</td><td className="r">100,00</td><td className="r">21%</td><td className="r">1.000,00</td></tr>
                    <tr><td>Materiale tipărite</td><td className="r">5</td><td className="r">40,00</td><td className="r">11%</td><td className="r">200,00</td></tr>
                  </tbody>
                </table>

                {templateShowVatDetail && (
                  <div>
                    <div className="pdf-sec">Detaliu TVA</div>
                    {mockShowRules && <div className="pdf-rule" />}
                    <table className="pdf-tbl">
                      <thead>
                        <tr><th>Cotă</th><th className="r">Bază</th><th className="r">TVA</th></tr>
                      </thead>
                      <tbody>
                        <tr><td>21%</td><td className="r">1.000,00</td><td className="r">210,00</td></tr>
                        <tr><td>11%</td><td className="r">200,00</td><td className="r">22,00</td></tr>
                      </tbody>
                    </table>
                  </div>
                )}

                <div className="pdf-tot">
                  <div className="row"><span>Subtotal</span><b>1.200,00 RON</b></div>
                  <div className="row"><span>TVA</span><b>232,00 RON</b></div>
                  <div className="row grand"><span>TOTAL</span><span>1.432,00 RON</span></div>
                </div>
                {templateShowWords && (
                  <div className="pdf-words">(una mie patru sute treizeci și doi lei)</div>
                )}

                {mockFooterNote && <div className="pdf-foot-note">{mockFooterNote}</div>}
              </div>
            </div>
            <div className="modal-foot">
              <span className="left muted" style={{ fontSize: 12 }}>
                Mock live — PDF-ul real se generează identic local
              </span>
              <button
                className="pill-btn"
                disabled={previewingTemplate || !activeCompanyId}
                style={!activeCompanyId ? { opacity: 0.5 } : undefined}
                title="Generează PDF-ul demo real cu identitatea companiei și îl deschide"
                onClick={() => void handlePreviewTemplate()}
              >
                <Ic name="eye" />
                {previewingTemplate ? "Se generează…" : "Deschide PDF real"}
              </button>
              <button className="pill-btn" onClick={() => setTplPreviewOpen(false)}>Închide</button>
              <button
                className="btn-dark"
                disabled={savingTemplate}
                onClick={async () => { await handleSaveTemplate(); setTplPreviewOpen(false); }}
              >
                <Ic name="check" />Salvează șablonul
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
