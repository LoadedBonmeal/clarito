/**
 * SetupAdminScreen — shown on first launch when no users exist.
 * Calls auth_setup_admin (PUBLIC command) then transitions to the app.
 */

import { useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { api } from "@/lib/tauri";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";
import { BrandMark } from "@/components/shared/BrandMark";

interface SetupAdminScreenProps {
  onSuccess: () => void;
}

export function SetupAdminScreen({ onSuccess }: SetupAdminScreenProps) {
  const { t } = useTranslation();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [validationErr, setValidationErr] = useState<string | null>(null);

  const mutation = useMutation({
    mutationFn: ({ u, p }: { u: string; p: string }) =>
      api.auth.setupAdmin(u, p),
    onSuccess: () => {
      notify.success(t("auth.setup.success"));
      onSuccess();
    },
    onError: (err) => {
      notify.error(formatError(err));
    },
  });

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    setValidationErr(null);
    if (password !== confirm) {
      setValidationErr(t("auth.setup.passwordMismatch"));
      return;
    }
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
          {t("auth.setup.title")}
        </h1>
        <p style={{ color: "var(--rf-text-dim)", fontSize: 13, margin: "0 0 24px", textAlign: "center" }}>
          {t("auth.setup.sub")}
        </p>

        <form onSubmit={handleSubmit} style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <input
            type="text"
            autoComplete="username"
            placeholder={t("auth.setup.usernamePlaceholder")}
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            required
            style={inputStyle}
          />
          <input
            type="password"
            autoComplete="new-password"
            placeholder={t("auth.setup.passwordPlaceholder")}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
            style={inputStyle}
          />
          <input
            type="password"
            autoComplete="new-password"
            placeholder={t("auth.setup.confirmPlaceholder")}
            value={confirm}
            onChange={(e) => setConfirm(e.target.value)}
            required
            style={inputStyle}
          />

          {(validationErr ?? mutation.error) && (
            <p style={{ color: "var(--rf-danger)", fontSize: 13, margin: "4px 0 0" }}>
              {validationErr ?? formatError(mutation.error)}
            </p>
          )}

          <button
            type="submit"
            disabled={mutation.isPending}
            style={btnStyle}
          >
            {mutation.isPending ? t("auth.setup.submitting") : t("auth.setup.submitBtn")}
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
  color: "var(--on-accent)",
  fontSize: 14,
  fontWeight: 600,
  cursor: "pointer",
  marginTop: 4,
};
