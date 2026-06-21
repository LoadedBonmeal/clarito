/**
 * LoginScreen — shown when users exist but no session is active.
 * Calls auth_login (PUBLIC command) then transitions to the app.
 */

import { useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { api } from "@/lib/tauri";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";

interface LoginScreenProps {
  onSuccess: () => void;
}

export function LoginScreen({ onSuccess }: LoginScreenProps) {
  const { t } = useTranslation();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");

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
          maxWidth: 380,
          padding: "40px 32px",
          background: "var(--rf-card)",
          border: "1px solid var(--rf-border)",
          borderRadius: 8,
          margin: 16,
        }}
      >
        {/* Logo mark */}
        <div
          style={{
            width: 40,
            height: 40,
            background: "var(--rf-accent)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            borderRadius: 6,
            color: "#fff",
            fontWeight: 700,
            fontSize: 16,
            marginBottom: 20,
          }}
        >
          C
        </div>

        <h1
          style={{
            color: "var(--rf-text)",
            fontSize: 18,
            fontWeight: 700,
            margin: "0 0 6px",
          }}
        >
          {t("auth.login.title")}
        </h1>
        <p style={{ color: "var(--rf-text-dim)", fontSize: 13, margin: "0 0 24px" }}>
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
  padding: "9px 12px",
  background: "var(--rf-input-bg, var(--rf-bg))",
  border: "1px solid var(--rf-border)",
  borderRadius: 5,
  color: "var(--rf-text)",
  fontSize: 14,
  outline: "none",
  boxSizing: "border-box",
};

const btnStyle: React.CSSProperties = {
  width: "100%",
  padding: "10px 16px",
  background: "var(--rf-accent)",
  border: "none",
  borderRadius: 5,
  color: "#fff",
  fontSize: 14,
  fontWeight: 600,
  cursor: "pointer",
  marginTop: 4,
};
