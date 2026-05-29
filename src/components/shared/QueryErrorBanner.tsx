interface QueryErrorBannerProps {
  error: unknown;
  label?: string;
  onRetry?: () => void;
}

export function QueryErrorBanner({ error, label = "date", onRetry }: QueryErrorBannerProps) {
  const message = error instanceof Error
    ? error.message
    : typeof error === "object" && error !== null && "message" in error
      ? String((error as { message: unknown }).message)
      : "Eroare necunoscută";

  return (
    <div
      role="alert"
      style={{
        display: "flex",
        alignItems: "center",
        gap: 12,
        padding: "10px 16px",
        margin: "8px 0",
        background: "var(--error-bg, #fef2f2)",
        border: "1px solid var(--error-border, #fca5a5)",
        borderRadius: 6,
        color: "var(--error-fg, #b91c1c)",
        fontSize: 13,
      }}
    >
      <span style={{ flex: 1 }}>
        ⚠️ Nu s-au putut încărca {label}. {message}
      </span>
      {onRetry && (
        <button
          type="button"
          onClick={onRetry}
          style={{
            padding: "4px 10px",
            background: "transparent",
            border: "1px solid currentColor",
            borderRadius: 4,
            cursor: "pointer",
            color: "inherit",
            fontSize: 12,
          }}
        >
          Reîncearcă
        </button>
      )}
    </div>
  );
}
