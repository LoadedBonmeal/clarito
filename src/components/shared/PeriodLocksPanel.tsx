/**
 * PeriodLocksPanel — afișează perioadele fiscale blocate ale firmei active
 * și permite deblocarea manuală (cu confirmare).
 *
 * Utilizare: randat în Setări (cardul „Perioade fiscale blocate"), care îi dă
 * titlul — componenta redă doar lista + acțiunea de deblocare. Poate fi inclus
 * în orice pagină cu acces la api.gl.listPeriodLocks / unlockPeriod.
 */

import { useCallback, useEffect, useState } from "react";
import { confirm } from "@tauri-apps/plugin-dialog";

import { api } from "@/lib/tauri";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { PeriodLock } from "@/types";

interface Props {
  companyId: string;
}

// Formatare sursă în text lizibil
function fmtSource(source: string): string {
  if (source.startsWith("declaration:")) {
    return `Declarație ${source.slice("declaration:".length)}`;
  }
  if (source === "manual") return "Manual (contabil)";
  return source;
}

// Formatare timestamp Unix → dată română
function fmtUnix(ts: number): string {
  const d = new Date(ts * 1000);
  const zi = String(d.getDate()).padStart(2, "0");
  const luna = String(d.getMonth() + 1).padStart(2, "0");
  const an = d.getFullYear();
  return `${zi}.${luna}.${an}`;
}

export function PeriodLocksPanel({ companyId }: Props) {
  const [locks, setLocks] = useState<PeriodLock[]>([]);
  const [loading, setLoading] = useState(false);
  const [unlocking, setUnlocking] = useState<string | null>(null); // period being unlocked

  const reload = useCallback(async () => {
    if (!companyId) return;
    setLoading(true);
    try {
      const data = await api.gl.listPeriodLocks(companyId);
      setLocks(data);
    } catch (err) {
      notify.error(formatError(err, "Eroare la încărcarea perioadelor blocate."));
    } finally {
      setLoading(false);
    }
  }, [companyId]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const handleUnlock = async (lock: PeriodLock) => {
    const ok = await confirm(
      `Deblocați perioada ${lock.period}?\n\nAceastă acțiune permite re-generarea notelor GL și modificarea facturilor din această perioadă. Procedați doar dacă depuneți o declarație rectificativă.`,
      { title: "Deblocare perioadă", kind: "warning" },
    );
    if (!ok) return;

    setUnlocking(lock.period);
    try {
      await api.gl.unlockPeriod(companyId, lock.period);
      notify.success(`Perioada ${lock.period} a fost deblocată.`);
      await reload();
    } catch (err) {
      notify.error(formatError(err, `Eroare la deblocarea perioadei ${lock.period}.`));
    } finally {
      setUnlocking(null);
    }
  };

  if (!companyId) return null;

  return (
    <div>
      {loading && (
        <div style={{ color: "var(--text-2)", fontSize: 13 }}>Se încarcă…</div>
      )}

      {!loading && locks.length === 0 && (
        <div
          style={{
            color: "var(--text-2)",
            fontSize: 13,
            padding: "10px 0",
            fontStyle: "italic",
          }}
        >
          Nicio perioadă blocată.
        </div>
      )}

      {!loading && locks.length > 0 && (
        <div
          style={{
            border: "1px solid var(--border)",
            borderRadius: 6,
            overflow: "hidden",
          }}
        >
          <table
            style={{
              width: "100%",
              borderCollapse: "collapse",
              fontSize: 13,
            }}
          >
            <thead>
              <tr
                style={{
                  background: "var(--surface-2)",
                  borderBottom: "1px solid var(--border)",
                }}
              >
                <th style={thStyle}>Perioadă</th>
                <th style={thStyle}>Sursă</th>
                <th style={thStyle}>Data blocării</th>
                <th style={{ ...thStyle, textAlign: "right" }}>Acțiuni</th>
              </tr>
            </thead>
            <tbody>
              {locks.map((lock, i) => (
                <tr
                  key={lock.id}
                  style={{
                    borderBottom:
                      i < locks.length - 1 ? "1px solid var(--border)" : "none",
                    background: "var(--surface-1)",
                  }}
                >
                  <td style={tdStyle}>
                    <span style={{ fontWeight: 600, fontVariantNumeric: "tabular-nums" }}>
                      {lock.period}
                    </span>
                  </td>
                  <td style={{ ...tdStyle, color: "var(--text-2)" }}>
                    {fmtSource(lock.source)}
                    {lock.note && (
                      <span style={{ color: "var(--text-3)", marginLeft: 6, fontSize: 12 }}>
                        ({lock.note})
                      </span>
                    )}
                  </td>
                  <td style={{ ...tdStyle, color: "var(--text-2)", fontVariantNumeric: "tabular-nums" }}>
                    {fmtUnix(lock.lockedAt)}
                  </td>
                  <td style={{ ...tdStyle, textAlign: "right" }}>
                    <button
                      onClick={() => void handleUnlock(lock)}
                      disabled={unlocking === lock.period}
                      style={{
                        fontSize: 12,
                        padding: "3px 10px",
                        borderRadius: 4,
                        border: "1px solid var(--border)",
                        background: "transparent",
                        color: "var(--danger, #c0392b)",
                        cursor: unlocking === lock.period ? "not-allowed" : "pointer",
                        opacity: unlocking === lock.period ? 0.6 : 1,
                      }}
                    >
                      {unlocking === lock.period ? "Se deblochează…" : "Deblochează"}
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

const thStyle: React.CSSProperties = {
  textAlign: "left",
  padding: "7px 12px",
  fontSize: 12,
  fontWeight: 600,
  color: "var(--text-2)",
};

const tdStyle: React.CSSProperties = {
  padding: "8px 12px",
  verticalAlign: "middle",
};
