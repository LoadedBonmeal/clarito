import React from "react";

interface State { hasError: boolean; error: Error | null }

export class ErrorBoundary extends React.Component<
  React.PropsWithChildren<{ fallback?: React.ReactNode }>,
  State
> {
  constructor(props: React.PropsWithChildren<{ fallback?: React.ReactNode }>) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    console.error("[ErrorBoundary]", error, info.componentStack);
  }

  render() {
    if (this.state.hasError) {
      return this.props.fallback ?? (
        <div style={{
          display: "flex", flexDirection: "column", alignItems: "center",
          justifyContent: "center", height: "100vh", gap: 12,
          fontFamily: "var(--font-sans, system-ui)", color: "#991B1B",
          background: "var(--rf-error-bg)"
        }}>
          <span style={{ fontSize: 32 }}>⚠️</span>
          <strong style={{ fontSize: 14 }}>A apărut o eroare neașteptată</strong>
          <span style={{ fontSize: 12, color: "var(--rf-text-muted)" }}>
            {this.state.error?.message ?? "Eroare necunoscută"}
          </span>
          <button
            style={{ marginTop: 8, padding: "6px 16px", fontSize: 12, cursor: "pointer" }}
            onClick={() => window.location.reload()}
          >
            Reîncarcă aplicația
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}
