/**
 * Users page — admin-only.
 * Lists all users with role, active status, last login.
 * Admin can: create user, change role, deactivate/activate, reset password.
 *
 * Hidden from the sidebar for non-admin users (sidebar filters by role).
 * Accessible at /users.
 */

import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { api } from "@/lib/tauri";
import { queryKeys } from "@/lib/queries";
import { useAuthStore } from "@/lib/auth-store";
import { formatError } from "@/lib/error-mapper";
import { notify } from "@/lib/toasts";
import type { UserRole, CreateUserInput } from "@/types";

// ─── Role label helper ──────────────────────────────────────────────────────

function RoleChip({ role }: { role: UserRole }) {
  const { t } = useTranslation();
  const colors: Record<UserRole, string> = {
    admin: "var(--rf-accent)",
    contabil: "#4caf50",
    operator: "#ff9800",
    viewer: "var(--rf-text-dim)",
  };
  return (
    <span
      style={{
        display: "inline-block",
        padding: "2px 8px",
        borderRadius: 12,
        fontSize: 12,
        fontWeight: 600,
        background: colors[role] + "22",
        color: colors[role],
        border: `1px solid ${colors[role]}44`,
      }}
    >
      {t(`auth.roles.${role}`, role)}
    </span>
  );
}

// ─── Create user modal ───────────────────────────────────────────────────────

function CreateUserModal({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [role, setRole] = useState<UserRole>("operator");

  const mutation = useMutation({
    mutationFn: (input: CreateUserInput) => api.auth.createUser(input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.auth.users });
      notify.success(t("auth.users.createBtn"));
      onClose();
    },
    onError: (err) => notify.error(formatError(err)),
  });

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    mutation.mutate({ username: username.trim(), password, role });
  };

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "rgba(0,0,0,0.5)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 1000,
      }}
      onClick={onClose}
    >
      <div
        style={{
          background: "var(--rf-card)",
          border: "1px solid var(--rf-border)",
          borderRadius: 8,
          padding: "24px 28px",
          width: "100%",
          maxWidth: 360,
          margin: 16,
        }}
        onClick={(e) => e.stopPropagation()}
      >
        <h2 style={{ color: "var(--rf-text)", fontSize: 16, fontWeight: 700, margin: "0 0 16px" }}>
          {t("auth.users.createTitle")}
        </h2>
        <form onSubmit={handleSubmit} style={{ display: "flex", flexDirection: "column", gap: 10 }}>
          <input
            type="text"
            placeholder={t("auth.login.usernamePlaceholder")}
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            required
            style={inputStyle}
          />
          <input
            type="password"
            placeholder={t("auth.setup.passwordPlaceholder")}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            required
            style={inputStyle}
          />
          <select
            value={role}
            onChange={(e) => setRole(e.target.value as UserRole)}
            style={inputStyle}
          >
            {(["admin", "contabil", "operator", "viewer"] as UserRole[]).map((r) => (
              <option key={r} value={r}>{t(`auth.roles.${r}`, r)}</option>
            ))}
          </select>
          {mutation.error && (
            <p style={{ color: "var(--rf-danger)", fontSize: 12, margin: 0 }}>
              {formatError(mutation.error)}
            </p>
          )}
          <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
            <button type="button" onClick={onClose} style={secondaryBtnStyle}>
              Anulare
            </button>
            <button type="submit" disabled={mutation.isPending} style={btnStyle}>
              {mutation.isPending ? t("auth.users.creating") : t("auth.users.createBtn")}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

// ─── Reset password modal ────────────────────────────────────────────────────

function ResetPasswordModal({ userId, onClose }: { userId: string; onClose: () => void }) {
  const { t } = useTranslation();
  const [newPassword, setNewPassword] = useState("");

  const mutation = useMutation({
    mutationFn: (pwd: string) => api.auth.resetPassword(userId, pwd),
    onSuccess: () => {
      notify.success(t("auth.users.resetBtn"));
      onClose();
    },
    onError: (err) => notify.error(formatError(err)),
  });

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    mutation.mutate(newPassword);
  };

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "rgba(0,0,0,0.5)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 1000,
      }}
      onClick={onClose}
    >
      <div
        style={{
          background: "var(--rf-card)",
          border: "1px solid var(--rf-border)",
          borderRadius: 8,
          padding: "24px 28px",
          width: "100%",
          maxWidth: 340,
          margin: 16,
        }}
        onClick={(e) => e.stopPropagation()}
      >
        <h2 style={{ color: "var(--rf-text)", fontSize: 16, fontWeight: 700, margin: "0 0 16px" }}>
          {t("auth.users.newPassword")}
        </h2>
        <form onSubmit={handleSubmit} style={{ display: "flex", flexDirection: "column", gap: 10 }}>
          <input
            type="password"
            placeholder={t("auth.users.newPasswordPlaceholder")}
            value={newPassword}
            onChange={(e) => setNewPassword(e.target.value)}
            required
            autoFocus
            style={inputStyle}
          />
          {mutation.error && (
            <p style={{ color: "var(--rf-danger)", fontSize: 12, margin: 0 }}>
              {formatError(mutation.error)}
            </p>
          )}
          <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
            <button type="button" onClick={onClose} style={secondaryBtnStyle}>
              Anulare
            </button>
            <button type="submit" disabled={mutation.isPending} style={btnStyle}>
              {mutation.isPending ? t("auth.users.resetting") : t("auth.users.resetBtn")}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

// ─── Main page ───────────────────────────────────────────────────────────────

export function UsersPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const currentUser = useAuthStore((s) => s.currentUser);
  const [showCreate, setShowCreate] = useState(false);
  const [resetUserId, setResetUserId] = useState<string | null>(null);

  const { data: users = [], isLoading } = useQuery({
    queryKey: queryKeys.auth.users,
    queryFn: () => api.auth.listUsers(),
  });

  const updateMutation = useMutation({
    mutationFn: ({ userId, isActive }: { userId: string; isActive: boolean }) =>
      api.auth.updateUser(userId, { isActive }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.auth.users });
    },
    onError: (err) => notify.error(formatError(err)),
  });

  const changeRoleMutation = useMutation({
    mutationFn: ({ userId, role }: { userId: string; role: UserRole }) =>
      api.auth.updateUser(userId, { role }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.auth.users });
    },
    onError: (err) => notify.error(formatError(err)),
  });

  const handleToggleActive = (userId: string, currentlyActive: boolean) => {
    if (userId === currentUser?.id) {
      notify.error(t("auth.users.adminCannotDeactivateSelf"));
      return;
    }
    updateMutation.mutate({ userId, isActive: !currentlyActive });
  };

  const formatLastLogin = (ts: number | null) => {
    if (!ts) return t("auth.users.never");
    return new Date(ts * 1000).toLocaleDateString("ro-RO", {
      day: "2-digit",
      month: "2-digit",
      year: "numeric",
    });
  };

  return (
    <div className="cl-page">
      <div className="page-head">
        <div>
          <h1 className="page-title">{t("auth.users.title")}</h1>
          <p className="page-sub">{t("auth.users.sub")}</p>
        </div>
        <button
          type="button"
          className="btn btn-primary"
          onClick={() => setShowCreate(true)}
          style={{ alignSelf: "flex-start" }}
        >
          + {t("auth.users.addUser")}
        </button>
      </div>

      {isLoading ? (
        <p style={{ color: "var(--rf-text-dim)", padding: "16px 0", fontSize: 13 }}>
          {t("common.loading", "Se încarcă…")}
        </p>
      ) : users.length === 0 ? (
        <p style={{ color: "var(--rf-text-dim)", padding: "16px 0", fontSize: 13 }}>
          {t("auth.users.noUsers")}
        </p>
      ) : (
        <div className="scr-card" style={{ marginTop: 16 }}>
          <table className="scr-table" style={{ width: "100%" }}>
            <thead>
              <tr>
                <th>{t("auth.users.username")}</th>
                <th>{t("auth.users.role")}</th>
                <th>{t("auth.users.status")}</th>
                <th>{t("auth.users.lastLogin")}</th>
                <th>{t("auth.users.actions")}</th>
              </tr>
            </thead>
            <tbody>
              {users.map((user) => (
                <tr key={user.id}>
                  <td style={{ fontWeight: 600, color: "var(--rf-text)" }}>
                    {user.username}
                    {user.id === currentUser?.id && (
                      <span
                        style={{
                          marginLeft: 6,
                          fontSize: 11,
                          color: "var(--rf-accent)",
                          fontWeight: 400,
                        }}
                      >
                        (tu)
                      </span>
                    )}
                  </td>
                  <td>
                    <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                      <RoleChip role={user.role} />
                      {/* Role change inline select (not self) */}
                      {user.id !== currentUser?.id && (
                        <select
                          value={user.role}
                          onChange={(e) =>
                            changeRoleMutation.mutate({
                              userId: user.id,
                              role: e.target.value as UserRole,
                            })
                          }
                          style={{
                            fontSize: 12,
                            padding: "2px 4px",
                            background: "var(--rf-bg)",
                            color: "var(--rf-text-dim)",
                            border: "1px solid var(--rf-border)",
                            borderRadius: 4,
                          }}
                        >
                          {(["admin", "contabil", "operator", "viewer"] as UserRole[]).map((r) => (
                            <option key={r} value={r}>
                              {t(`auth.roles.${r}`, r)}
                            </option>
                          ))}
                        </select>
                      )}
                    </div>
                  </td>
                  <td>
                    {user.lockedUntil && user.lockedUntil > Date.now() / 1000 ? (
                      <span style={{ color: "var(--rf-danger)", fontSize: 12 }}>
                        {t("auth.users.locked")}
                      </span>
                    ) : user.isActive ? (
                      <span style={{ color: "#4caf50", fontSize: 12 }}>
                        {t("auth.users.active")}
                      </span>
                    ) : (
                      <span style={{ color: "var(--rf-text-dim)", fontSize: 12 }}>
                        {t("auth.users.inactive")}
                      </span>
                    )}
                  </td>
                  <td style={{ color: "var(--rf-text-dim)", fontSize: 13 }}>
                    {formatLastLogin(user.lastLogin)}
                  </td>
                  <td>
                    <div style={{ display: "flex", gap: 8 }}>
                      <button
                        type="button"
                        onClick={() => setResetUserId(user.id)}
                        style={actionBtnStyle}
                      >
                        {t("auth.users.resetPwd")}
                      </button>
                      {user.id !== currentUser?.id && (
                        <button
                          type="button"
                          onClick={() => handleToggleActive(user.id, user.isActive)}
                          disabled={updateMutation.isPending}
                          style={{
                            ...actionBtnStyle,
                            color: user.isActive ? "var(--rf-danger)" : "#4caf50",
                          }}
                        >
                          {user.isActive
                            ? t("auth.users.deactivate")
                            : t("auth.users.activate")}
                        </button>
                      )}
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {showCreate && <CreateUserModal onClose={() => setShowCreate(false)} />}
      {resetUserId && (
        <ResetPasswordModal userId={resetUserId} onClose={() => setResetUserId(null)} />
      )}
    </div>
  );
}

// ─── Styles ──────────────────────────────────────────────────────────────────

const inputStyle: React.CSSProperties = {
  width: "100%",
  padding: "8px 11px",
  background: "var(--rf-input-bg, var(--rf-bg))",
  border: "1px solid var(--rf-border)",
  borderRadius: 5,
  color: "var(--rf-text)",
  fontSize: 13,
  outline: "none",
  boxSizing: "border-box",
};

const btnStyle: React.CSSProperties = {
  flex: 1,
  padding: "8px 14px",
  background: "var(--rf-accent)",
  border: "none",
  borderRadius: 5,
  color: "#fff",
  fontSize: 13,
  fontWeight: 600,
  cursor: "pointer",
};

const secondaryBtnStyle: React.CSSProperties = {
  ...btnStyle,
  background: "var(--rf-bg)",
  color: "var(--rf-text-dim)",
  border: "1px solid var(--rf-border)",
};

const actionBtnStyle: React.CSSProperties = {
  padding: "4px 10px",
  background: "transparent",
  border: "1px solid var(--rf-border)",
  borderRadius: 4,
  color: "var(--rf-text-dim)",
  fontSize: 12,
  cursor: "pointer",
};
