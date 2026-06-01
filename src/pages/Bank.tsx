/**
 * Bank.tsx — "Bancă & Casă" placeholder page.
 *
 * Static visual teaser — no backend calls (bank module not yet implemented).
 * Clearly marked "În curând" with a Banner and disabled-looking UI.
 */

import { Banner, Badge, Card, PageHeader, StatCard, Segmented } from "@/components/rf";
import { useState } from "react";

// ─── Static demo rows ─────────────────────────────────────────────────────────

interface TxRow {
  date: string;
  desc: string;
  doc: string;
  type: "in" | "out";
  amount: number;
  bal: number;
}

const BANCA_ROWS: TxRow[] = [
  { date: "29 mai 2026", desc: "Încasare factură ACME-0001247", doc: "OP 4471", type: "in", amount: 14994.0, bal: 248120.5 },
  { date: "28 mai 2026", desc: "Plată furnizor Umbrella Distribution", doc: "OP 4468", type: "out", amount: 7996.8, bal: 233126.5 },
  { date: "27 mai 2026", desc: "Încasare factură ACME-0001245 (parțial)", doc: "OP 4460", type: "in", amount: 3000.0, bal: 241123.3 },
  { date: "26 mai 2026", desc: "Comision administrare cont", doc: "Extras 05", type: "out", amount: 45.0, bal: 238123.3 },
  { date: "24 mai 2026", desc: "Plată salarii mai 2026", doc: "OP 4452", type: "out", amount: 38200.0, bal: 238168.3 },
  { date: "22 mai 2026", desc: "Încasare factură ACME-0001241", doc: "OP 4441", type: "in", amount: 11424.0, bal: 276368.3 },
];

const CASA_ROWS: TxRow[] = [
  { date: "28 mai 2026", desc: "Chitanță CH-0042 — Wayne Logistics", doc: "CH-0042", type: "in", amount: 3000.0, bal: 8420.0 },
  { date: "24 mai 2026", desc: "Chitanță CH-0041 — Pied Piper", doc: "CH-0041", type: "in", amount: 1500.0, bal: 5420.0 },
  { date: "23 mai 2026", desc: "Decont deplasare", doc: "DC 12", type: "out", amount: 380.0, bal: 3920.0 },
  { date: "20 mai 2026", desc: "Chitanță CH-0040 — Persoană fizică", doc: "CH-0040", type: "in", amount: 420.0, bal: 4300.0 },
];

// ─── Helpers ─────────────────────────────────────────────────────────────────

function fmtMoney(n: number): string {
  return n.toLocaleString("ro-RO", { minimumFractionDigits: 2, maximumFractionDigits: 2 });
}

// ─── Page ────────────────────────────────────────────────────────────────────

export function BankPage() {
  const [tab, setTab] = useState<"banca" | "casa">("banca");
  const rows = tab === "banca" ? BANCA_ROWS : CASA_ROWS;

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", background: "var(--rf-app-bg)" }}>
      <PageHeader
        screen="Operativ › Bancă & Casă"
        title="Bancă & Casă"
        actions={
          <span
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 6,
              padding: "4px 12px",
              background: "var(--rf-accent-tint)",
              color: "var(--rf-accent)",
              borderRadius: 20,
              fontSize: 12,
              fontWeight: 600,
            }}
          >
            În curând
          </span>
        }
      />

      <div style={{ flex: 1, overflow: "auto", padding: "0 32px 32px" }}>
        {/* "Coming soon" banner */}
        <Banner variant="info" title="Modul în curs de dezvoltare">
          Modulul Bancă &amp; Casă nu este încă disponibil. Această pagină este un previzualizare statică a funcționalităților viitoare.
        </Banner>

        {/* Disabled stat cards */}
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(4, 1fr)",
            gap: 16,
            marginBottom: 20,
            opacity: 0.5,
            pointerEvents: "none",
          }}
        >
          <StatCard icon="bank" label="Sold bancă" value="248.120,50" unit="RON" ctx="RO49 RNCB · BCR" />
          <StatCard icon="wallet" label="Sold casă (numerar)" value="8.420,00" unit="RON" ctx="Registru de casă" />
          <StatCard icon="arrowDown" label="Încasări (mai)" value="29.418,00" unit="RON" delta="6,2%" deltaDir="up" ctx="vs. aprilie" />
          <StatCard icon="arrowUp" label="Plăți (mai)" value="46.241,80" unit="RON" ctx="furnizori + salarii" />
        </div>

        {/* Disabled table card */}
        <div style={{ opacity: 0.5, pointerEvents: "none" }}>
          <Card>
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 12,
                padding: "12px 16px",
                borderBottom: "1px solid var(--rf-border)",
              }}
            >
              <Segmented
                value={tab}
                onChange={(v) => setTab(v as "banca" | "casa")}
                options={[
                  { value: "banca", label: "Bancă" },
                  { value: "casa", label: "Casă" },
                ]}
              />
            </div>

            <div style={{ overflowX: "auto" }}>
              <table className="rf-tbl">
                <thead>
                  <tr>
                    <th>Data</th>
                    <th>Descriere</th>
                    <th>Document</th>
                    <th>Tip</th>
                    <th className="right">Sumă</th>
                    <th className="right">Sold</th>
                  </tr>
                </thead>
                <tbody>
                  {rows.map((r, i) => (
                    <tr key={i}>
                      <td style={{ color: "var(--rf-text-dim)", whiteSpace: "nowrap" }}>{r.date}</td>
                      <td style={{ fontWeight: 500 }}>{r.desc}</td>
                      <td style={{ fontFamily: "var(--rf-mono)", color: "var(--rf-text-muted)" }}>{r.doc}</td>
                      <td>
                        {r.type === "in" ? (
                          <Badge variant="success">Încasare</Badge>
                        ) : (
                          <Badge variant="neutral">Plată</Badge>
                        )}
                      </td>
                      <td
                        className="right"
                        style={{
                          fontFamily: "var(--rf-mono)",
                          fontWeight: 600,
                          color: r.type === "in" ? "var(--rf-success)" : "var(--rf-text)",
                        }}
                      >
                        {r.type === "in" ? "+" : "−"}
                        {fmtMoney(r.amount)}
                      </td>
                      <td
                        className="right"
                        style={{ fontFamily: "var(--rf-mono)", color: "var(--rf-text-muted)" }}
                      >
                        {fmtMoney(r.bal)}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </Card>
        </div>
      </div>
    </div>
  );
}
