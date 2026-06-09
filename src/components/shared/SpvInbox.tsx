/**
 * SpvInbox — the general SPV inbox (SPVWS2): declaration recipise, notificări, somații, decizii.
 * Distinct from the e-Factura received-invoice sync. Read-only — ANAF provides no
 * declaration-submission API (D300/D394/D406 are uploaded manually in the SPV portal), so this
 * surfaces the responses + notifications. Requires a connected ANAF account.
 */

import { useState } from "react";
import { useMutation } from "@tanstack/react-query";

import { SectionCard, Btn, Badge, Banner } from "@/components/rf";
import { api } from "@/lib/tauri";
import { useAppStore } from "@/lib/store";
import { notify } from "@/lib/toasts";
import { formatError } from "@/lib/error-mapper";
import type { SpvInboxItem } from "@/types";

const CATEGORY_LABEL: Record<string, string> = {
  recipisa: "Recipisă",
  notificare: "Notificare",
  somatie: "Somație",
  decizie: "Decizie",
  factura: "Factură",
  altele: "Altele",
};

function categoryBadge(category: string) {
  const variant =
    category === "somatie"
      ? "error"
      : category === "recipisa"
        ? "success"
        : category === "decizie" || category === "notificare"
          ? "warning"
          : "neutral";
  return <Badge variant={variant}>{CATEGORY_LABEL[category] ?? category}</Badge>;
}

export function SpvInbox() {
  const activeCompanyId = useAppStore((s) => s.activeCompanyId);
  const [days, setDays] = useState(30);

  const load = useMutation({
    mutationFn: (): Promise<SpvInboxItem[]> => {
      if (!activeCompanyId) throw new Error("Selectați o companie activă.");
      return api.anaf.listSpvInbox(activeCompanyId, days);
    },
    onError: (err) => notify.error(formatError(err, "Nu s-a putut încărca inbox-ul SPV.")),
  });

  const items = load.data ?? [];
  const somatii = items.filter((i) => i.category === "somatie").length;

  return (
    <SectionCard
      icon="fileIn"
      title="Inbox SPV (recipise, notificări, somații)"
      actions={
        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <select
            className="rf-select"
            value={days}
            onChange={(e) => setDays(Number(e.target.value))}
            style={{ width: 110 }}
          >
            <option value={7}>7 zile</option>
            <option value={30}>30 zile</option>
            <option value={60}>60 zile</option>
          </select>
          <Btn
            variant="secondary"
            size="sm"
            icon="refresh"
            disabled={load.isPending || !activeCompanyId}
            onClick={() => load.mutate()}
          >
            {load.isPending ? "Se încarcă…" : "Încarcă mesajele SPV"}
          </Btn>
        </div>
      }
    >
      <div style={{ padding: "0 16px 12px" }}>
        <Banner variant="info">
          Depunerea declarațiilor D300/D394/D406 nu are API ANAF — se face manual în portalul SPV
          (PDF inteligent validat cu DUKIntegrator). Aici vedeți <b>răspunsurile</b> ANAF:
          recipise, notificări și somații.
        </Banner>
      </div>

      {somatii > 0 && (
        <div style={{ padding: "0 16px 12px" }}>
          <Banner variant="warning">
            <b>{somatii}</b> {somatii === 1 ? "somație necesită" : "somații necesită"} atenție.
          </Banner>
        </div>
      )}

      {load.isSuccess && items.length === 0 ? (
        <div style={{ padding: "12px 16px", fontSize: 12.5, color: "var(--rf-text-muted)" }}>
          Niciun mesaj SPV în perioada selectată.
        </div>
      ) : items.length > 0 ? (
        <div className="rf-tbl-wrap">
          <table className="rf-tbl">
            <thead>
              <tr>
                <th>Data</th>
                <th>Categorie</th>
                <th>Tip</th>
                <th>Detalii</th>
              </tr>
            </thead>
            <tbody>
              {items.map((m) => (
                <tr key={m.id}>
                  <td className="rf-mono">{m.dataCreare}</td>
                  <td>{categoryBadge(m.category)}</td>
                  <td>{m.tip}</td>
                  <td style={{ color: "var(--rf-text-muted)" }}>{m.detalii ?? "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : null}
    </SectionCard>
  );
}
