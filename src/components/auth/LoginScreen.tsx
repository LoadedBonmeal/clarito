/**
 * LoginScreen — shown when users exist but no session is active.
 * Calls auth_login (PUBLIC command) then transitions to the app.
 *
 * Also shown after an idle-timeout expiry: in that case
 * `sessionExpiredSignal.get()` is true and a notice banner is rendered.
 */

import { useState, useEffect } from "react";
import { useMutation } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { api, sessionExpiredSignal } from "@/lib/tauri";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";
import { BrandMark } from "@/components/shared/BrandMark";

interface LoginScreenProps {
  onSuccess: () => void;
}

export function LoginScreen({ onSuccess }: LoginScreenProps) {
  const { t } = useTranslation();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [wasSessionExpired, setWasSessionExpired] = useState(false);

  // Consume the session-expired signal on mount so it shows once, then clears.
  useEffect(() => {
    if (sessionExpiredSignal.get()) {
      setWasSessionExpired(true);
      sessionExpiredSignal.set(false);
    }
  }, []);

  const mutation = useMutation({
    mutationFn: ({ u, p }: { u: string; p: string }) => api.auth.login(u, p),
    onSuccess: () => {
      onSuccess();
    },
    onError: (err) => {
      notify.error(formatError(err));
    },
  });

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    mutation.mutate({ u: username.trim(), p: password });
  };

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: "var(--rf-bg)",
        fontFamily: "system-ui, sans-serif",
      }}
    >
      <div
        style={{
          width: "100%",
          maxWidth: 400,
          padding: "36px 32px",
          background: "var(--rf-card)",
          border: "1px solid var(--rf-border)",
          borderRadius: 16,
          boxShadow: "0 1px 2px rgba(0,0,0,.04), 0 18px 40px -24px rgba(0,0,0,.25)",
          margin: 16,
        }}
      >
        {/* Brand mark — real Clarito logo, centered */}
        <div style={{ display: "flex", justifyContent: "center", marginBottom: 18 }}>
          <BrandMark size={48} />
        </div>

        {/* Session-expired notice — shown once after an idle-timeout expiry */}
        {wasSessionExpired && (
          <div
            style={{
              padding: "10px 12px",
              marginBottom: 16,
              background: "var(--rf-warning-bg, rgba(234,179,8,0.12))",
              border: "1px solid var(--rf-warning-border, rgba(234,179,8,0.4))",
              borderRadius: 5,
              color: "var(--rf-warning-text, #ca8a04)",
              fontSize: 13,
              lineHeight: 1.5,
            }}
            role="alert"
          >
            {t("auth.login.sessionExpiredNotice")}
          </div>
        )}

        <h1
          style={{
            color: "var(--rf-text)",
            fontSize: 20,
            fontWeight: 600,
            letterSpacing: "-0.01em",
            margin: "0 0 6px",
            textAlign: "center",
          }}
        >
          {t("auth.login.title")}
        </h1>
        <p style={{ color: "var(--rf-text-dim)", fontSize: 13, margin: "0 0 24px", textAlign: "center" }}>
          {t("auth.login.sub")}
        </p>

        <form onSubmit={handleSubmit} style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <input
            type="text"
            autoComplete="username"
            placeholder={t("auth.login.usernamePlaceholder")}
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            required
            autoFocus
            style={inputStyle}
          />
          <input
            type="password"
            autoComplete="current-password"
            placeholder={t("auth.login.passwordPlaceholder")}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
            style={inputStyle}
          />

          {mutation.error && (
            <p style={{ color: "var(--rf-danger)", fontSize: 13, margin: "4px 0 0" }}>
              {formatError(mutation.error)}
            </p>
          )}

          <button
            type="submit"
            disabled={mutation.isPending}
            style={btnStyle}
          >
            {mutation.isPending ? t("auth.login.submitting") : t("auth.login.submitBtn")}
          </button>
        </form>
      </div>
    </div>
  );
}

const inputStyle: React.CSSProperties = {
  width: "100%",
  padding: "10px 12px",
  background: "var(--rf-input-bg, var(--rf-bg))",
  border: "1px solid var(--rf-border)",
  borderRadius: 9,
  color: "var(--rf-text)",
  fontSize: 14,
  outline: "none",
  boxSizing: "border-box",
};

const btnStyle: React.CSSProperties = {
  width: "100%",
  padding: "11px 16px",
  background: "var(--rf-accent)",
  border: "none",
  borderRadius: 9,
  color: "#fff",
  fontSize: 14,
  fontWeight: 600,
  cursor: "pointer",
  marginTop: 4,
};
