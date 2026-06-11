/**
 * SpvInbox — the general SPV inbox (SPVWS2): declaration recipise, notificări, somații, decizii.
 * Distinct from the e-Factura received-invoice sync. Read-only — ANAF provides no
 * declaration-submission API (D300/D394/D406 are uploaded manually in the SPV portal), so this
 * surfaces the responses + notifications. Requires a connected ANAF account.
 *
 * Design re-skin: .scr-card + .scr-toolbar (select zile + .pill-btn încărcare) +
 * .banner info/warn + .msg list (pattern din src/pages/Notifications.tsx).
 */

import { useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { SpvInboxItem } from "@/types";

/** Known inbox categories with an i18n label under shared.spvInbox.cat.* */
const KNOWN_CATEGORIES = new Set([
  "recipisa", "notificare", "somatie", "decizie", "factura", "altele",
]);

/** Category → design .chip variant (same severity mapping as the old rf Badge). */
function categoryChipCls(category: string): string {
  return category === "somatie"
    ? "late"
    : category === "recipisa"
      ? "paid"
      : category === "decizie" || category === "notificare"
        ? "wait"
        : "sent";
}

export function SpvInbox() {
  const { t } = useTranslation();
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [days, setDays] = useState(30);

  const load = useMutation({
    mutationFn: (): Promise<SpvInboxItem[]> => {
      if (!activeCompanyId) throw new Error(t("shared.spvInbox.noCompany"));
      return api.anaf.listSpvInbox(activeCompanyId, days);
    },
    onError: (err) => notify.error(formatError(err, t("shared.spvInbox.loadError"))),
  });

  const items = load.data ?? [];
  const somatii = items.filter((i) => i.category === "somatie").length;

  return (
    <div className="scr-card">
      <div className="scr-toolbar">
        <Ic name="docDown" />
        <span className="tt">{t("shared.spvInbox.title")}</span>
        <div className="spacer" />
        <select
          className="select"
          value={days}
          onChange={(e) => setDays(Number(e.target.value))}
          style={{ width: 110, height: 32 }}
        >
          <option value={7}>{t("shared.spvInbox.days", { n: 7 })}</option>
          <option value={30}>{t("shared.spvInbox.days", { n: 30 })}</option>
          <option value={60}>{t("shared.spvInbox.days", { n: 60 })}</option>
        </select>
        <button
          className="pill-btn"
          disabled={load.isPending || !activeCompanyId}
          style={load.isPending || !activeCompanyId ? { opacity: 0.6, cursor: "default" } : undefined}
          onClick={() => load.mutate()}
        >
          <Ic name="sync" />
          {load.isPending ? t("shared.spvInbox.loading") : t("shared.spvInbox.loadBtn")}
        </button>
      </div>

      <div style={{ padding: "12px 14px 0" }}>
        <div className="banner" style={{ marginBottom: somatii > 0 ? 10 : 12 }}>
          <Ic name="docText" />
          <span>
            {t("shared.spvInbox.infoPre")} <b>{t("shared.spvInbox.infoBold")}</b>{" "}
            {t("shared.spvInbox.infoPost")}
          </span>
        </div>

        {somatii > 0 && (
          <div className="banner warn" style={{ marginBottom: 12 }}>
            <svg
              className="ic"
              viewBox="0 0 24 24"
              dangerouslySetInnerHTML={{ __html: '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>' }}
            />
            <span>
              <b>{somatii}</b> {t("shared.spvInbox.somatii", { count: somatii })}
            </span>
          </div>
        )}
      </div>

      {load.isSuccess && items.length === 0 ? (
        <div style={{ padding: "4px 16px 14px", fontSize: 12.5, color: "var(--text-2)" }}>
          {t("shared.spvInbox.empty")}
        </div>
      ) : items.length > 0 ? (
        <div className="msg-list" style={{ borderTop: "1px solid var(--line)" }}>
          {items.map((m) => (
            <div className="msg" key={m.id}>
              <div className="msg-main">
                <div className="msg-top">
                  <span className="msg-from">{m.tip}</span>
                  <span className="msg-time num">{m.dataCreare}</span>
                </div>
                <div className="msg-sub">{m.detalii ?? "—"}</div>
              </div>
              <span className="msg-tag">
                <span className={`chip ${categoryChipCls(m.category)}`}>
                  {KNOWN_CATEGORIES.has(m.category)
                    ? t(`shared.spvInbox.cat.${m.category}`)
                    : m.category}
                </span>
              </span>
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}
