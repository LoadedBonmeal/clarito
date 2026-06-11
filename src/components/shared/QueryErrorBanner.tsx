/**
 * QueryErrorBanner — eroare de încărcare query, redată ca design .banner.danger
 * cu buton de reîncercare .pill-btn.
 */

import { useTranslation } from "react-i18next";

interface QueryErrorBannerProps {
  error: unknown;
  label?: string;
  onRetry?: () => void;
}

export function QueryErrorBanner({ error, label, onRetry }: QueryErrorBannerProps) {
  const { t } = useTranslation();
  const message = error instanceof Error
    ? error.message
    : typeof error === "object" && error !== null && "message" in error
      ? String((error as { message: unknown }).message)
      : t("shared.queryError.unknown");

  return (
    <div className="banner danger" role="alert">
      <svg
        className="ic"
        viewBox="0 0 24 24"
        dangerouslySetInnerHTML={{ __html: '<path d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126ZM12 15.75h.007v.008H12v-.008Z"/>' }}
      />
      <span style={{ flex: 1 }}>
        {t("shared.queryError.message", {
          label: label ?? t("shared.queryError.defaultLabel"),
          message,
        })}
      </span>
      {onRetry && (
        <button
          type="button"
          className="pill-btn"
          onClick={onRetry}
          style={{ height: 28, padding: "0 10px", fontSize: 12, flex: "none" }}
        >
          {t("shared.queryError.retry")}
        </button>
      )}
    </div>
  );
}
