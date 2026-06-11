/**
 * PreflightPanel — renders pre-export validation findings above export buttons.
 *
 * Advisory only: errors are shown prominently but do NOT block export.
 * Returns null when there are no issues (nothing rendered).
 */

import { Banner } from "@/components/shared/Banner";
import type { PreflightIssue } from "@/lib/tauri";

interface Props {
  issues: PreflightIssue[];
}

export function PreflightPanel({ issues }: Props) {
  if (issues.length === 0) return null;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
      <div
        style={{
          fontSize: 11.5,
          fontWeight: 700,
          textTransform: "uppercase",
          letterSpacing: "0.06em",
          color: "var(--text-2)",
          paddingBottom: 2,
        }}
      >
        Verificare înainte de export
      </div>
      {issues.map((issue, idx) => (
        <Banner
          key={`${issue.code}-${idx}`}
          variant={issue.severity === "error" ? "error" : "warning"}
        >
          <b>{issue.message}</b>
          {issue.hint && (
            <div
              style={{
                marginTop: 4,
                fontSize: 12,
                color: "var(--text-2)",
              }}
            >
              {issue.hint}
            </div>
          )}
        </Banner>
      ))}
    </div>
  );
}
