/**
 * Bank.tsx — "Bancă & Casă" placeholder page.
 *
 * Static "În curând" teaser — no backend calls (bank module not yet implemented).
 * Design re-skin: .main-inner + .page-head + centered muted .scr-card.
 */

import { useTranslation } from "react-i18next";

import { Ic } from "@/components/shared/Ic";

export function BankPage() {
  const { t } = useTranslation();
  return (
    <div className="main-inner">
      <div className="page-head">
        <div>
          <h1>{t("bank.title")}</h1>
          <p className="sub">{t("bank.sub")}</p>
        </div>
        <div className="head-actions">
          <span className="chip wait">
            <Ic name="clock" cls="sic" />
            {t("bank.soon")}
          </span>
        </div>
      </div>

      <div className="scr-card">
        <div style={{ padding: "56px 24px", textAlign: "center" }}>
          <div
            style={{
              width: 44,
              height: 44,
              margin: "0 auto 14px",
              borderRadius: 12,
              background: "var(--fill)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
            }}
          >
            <Ic name="card" />
          </div>
          <div style={{ fontSize: 14, fontWeight: 600, color: "var(--text)" }}>{t("bank.soon")}</div>
          <div
            style={{
              fontSize: 12.5,
              color: "var(--text-2)",
              marginTop: 6,
              maxWidth: 420,
              marginLeft: "auto",
              marginRight: "auto",
              lineHeight: 1.5,
            }}
          >
            {t("bank.desc")}
          </div>
        </div>
      </div>
    </div>
  );
}
