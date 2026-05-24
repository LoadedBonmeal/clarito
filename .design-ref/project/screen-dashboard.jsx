/* ----------------------------------------------------------------------
   Dashboard — Privire generală
   - Inline text summary (NOT stat tiles with colored bubbles)
   - Compact 4-cell KPI strip (no rounded corners, no icons-in-bubbles)
   - Companies administered table
   - Recent invoices table with status badges
   - ANAF live activity panel
   ---------------------------------------------------------------------- */

const fmtRON = (n) => n.toLocaleString("ro-RO", { minimumFractionDigits: 2, maximumFractionDigits: 2 });

const StatusBadge = ({ status }) => {
  const map = {
    draft:     "DRAFT",
    pending:   "ÎN AȘTEPTARE",
    submitted: "TRIMISĂ",
    validated: "VALIDATĂ",
    rejected:  "RESPINSĂ",
    archived:  "ARHIVATĂ",
    new:       "NOUĂ",
    reviewed:  "REVIZUITĂ",
    approved:  "APROBATĂ",
  };
  return <span className={"badge " + status}><span className="dot" />{map[status] || status}</span>;
};

const DashSummary = () => (
  <div className="dash-summary">
    Bună dimineața, <span className="b">Dorin</span>. Astăzi, <span className="b">19 mai 2026</span>, ai{" "}
    <span className="pill"><Icon name="bell" size={11} />3 mesaje SPV neprocesate</span> și{" "}
    <span className="pill"><Icon name="alert" size={11} />1 factură respinsă de ANAF</span> care necesită atenție.
    În luna curentă ai emis <span className="b">187 facturi</span> totalizând <span className="b tnum">1.247.890,50 RON</span>{" "}
    <span className="pos">▲ 12,4% față de aprilie</span>, dintre care{" "}
    <span className="b">184 au fost validate</span> de ANAF (<span className="pos">98,4%</span>) și{" "}
    <span className="neg">3 respinse</span> — toate au cauza{" "}
    <a href="#">CUI cumpărător invalid sau neînregistrat ca plătitor TVA</a>.
    Sincronizarea cu SPV s-a făcut acum 14 minute. Următoarea declarație de TVA (<span className="b">D300</span>) este scadentă în{" "}
    <span className="b">7 zile</span>.
  </div>
);

const KpiStrip = () => (
  <div className="kpi-strip">
    <div className="kpi-cell">
      <span className="lbl">Vânzări — Mai 2026</span>
      <span className="val tnum">1.247.890,50</span>
      <span className="delta up">▲ 12,4% vs aprilie · RON net</span>
    </div>
    <div className="kpi-cell">
      <span className="lbl">TVA colectată</span>
      <span className="val tnum">237.099,20</span>
      <span className="sub">19% (×142) · 9% (×31) · 5% (×14)</span>
    </div>
    <div className="kpi-cell">
      <span className="lbl">Facturi emise · Mai</span>
      <span className="val tnum">187</span>
      <span className="sub">184 validate · 3 respinse · 9 schițe</span>
    </div>
    <div className="kpi-cell">
      <span className="lbl">De încasat · Restanțe</span>
      <span className="val tnum">84.220,30</span>
      <span className="delta down">▼ 11 facturi cu termen depășit</span>
    </div>
  </div>
);

const Sparkline = ({ values, highlight = -1 }) => (
  <div className="spark">
    {values.map((v, i) => (
      <span key={i}
            className={i === highlight ? "" : (i < values.length - 7 ? "muted" : "")}
            style={{ height: `${Math.max(4, v * 100)}%` }} />
    ))}
  </div>
);

const Dashboard = ({ onOpenInvoice, onNavigate, activeCompany, companies, invoices }) => {
  const sortedInvoices = invoices.slice(0, 10);

  return (
    <div className="content">
      <div className="content-titlebar">
        <span className="content-title">
          <span className="crumb">Efactura</span>
          Privire generală
        </span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6, alignItems: "center" }}>
          <span style={{ fontSize: 11, color: "var(--text-muted)" }}>Perioadă:</span>
          <div className="seg">
            <span className="seg-item">Astăzi</span>
            <span className="seg-item">Săptămâna</span>
            <span className="seg-item active">Mai 2026</span>
            <span className="seg-item">YTD</span>
          </div>
          <button className="btn"><Icon name="download" size={12} /> Export</button>
          <button className="btn"><Icon name="refresh" size={12} /> Reîmprospătează <span className="kbd" style={{ marginLeft: 4 }}>F5</span></button>
        </span>
      </div>

      {/* inline ANAF callout */}
      <div className="callout" style={{ margin: "10px 14px 0", borderColor: "#FCD34D", background: "#FFFBEB", color: "#854D0E", borderLeftColor: "#D97706" }}>
        <Icon name="alert" size={15} />
        <span>
          Factura <b>ACME-0001244</b> către <b>Wayne Logistics SRL</b> a fost respinsă de ANAF:{" "}
          <i>TVA-ul cumpărătorului nu corespunde cu registrul ANAF la data emiterii.</i>
        </span>
        <button className="fix" style={{ background: "#D97706" }}>
          <Icon name="pen" size={11} /> Corectează
        </button>
        <button className="fix" style={{ background: "transparent", color: "#854D0E", border: "1px solid #D97706" }}>
          <Icon name="eye" size={11} /> Vezi factura
        </button>
      </div>

      <div className="dash">
        <DashSummary />
        <KpiStrip />

        {/* Two-column row: Companies + ANAF activity */}
        <div className="dash-row">
          {/* Companies administered table */}
          <div className="panel">
            <div className="panel-header">
              <span>Companii administrate · 15 SRL</span>
              <span style={{ display: "flex", gap: 6 }}>
                <button className="btn compact" onClick={() => onNavigate("companii")}>
                  Vezi toate <Icon name="arrowRight" size={11} />
                </button>
              </span>
            </div>
            <div style={{ maxHeight: 240, overflow: "auto" }}>
              <table className="dt">
                <thead>
                  <tr>
                    <th style={{ width: 96 }}>CUI</th>
                    <th>Denumire</th>
                    <th style={{ width: 110 }}>Localitate</th>
                    <th className="num" style={{ width: 56 }}>SPV</th>
                    <th className="num" style={{ width: 84 }}>Vânzări mai</th>
                    <th className="num" style={{ width: 56 }}>Alertă</th>
                  </tr>
                </thead>
                <tbody>
                  {companies.slice(0, 8).map((c, i) => (
                    <tr key={c.id} className={c.id === activeCompany.id ? "selected" : ""}>
                      <td><span className="mono">{c.cui}</span></td>
                      <td>
                        <span style={{ display: "inline-block", width: 6, height: 6, background: c.color, marginRight: 6, verticalAlign: "middle" }} />
                        {c.name}
                      </td>
                      <td className="muted">{c.city}</td>
                      <td className="num">
                        {c.spv
                          ? <Icon name="check" size={13} style={{ color: "#16A34A" }} />
                          : <Icon name="x"     size={13} style={{ color: "var(--text-dim)" }} />}
                      </td>
                      <td className="num tnum">{fmtRON([47280, 88100, 14290, 28100, 110450, 4220, 192480, 8400][i] || 0)}</td>
                      <td className="num">
                        {[0,3,0,1,0,0,2,0][i] > 0
                          ? <span style={{ color: "#DC2626", fontWeight: 600 }}>{[0,3,0,1,0,0,2,0][i]}</span>
                          : <span className="dim">—</span>}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>

          {/* ANAF live activity timeline */}
          <div className="panel">
            <div className="panel-header">
              <span>Activitate ANAF · live</span>
              <span style={{ display: "flex", gap: 6, alignItems: "center" }}>
                <span style={{ display: "inline-flex", alignItems: "center", gap: 4, fontSize: 10.5, color: "#16A34A", textTransform: "none", letterSpacing: 0 }}>
                  <span style={{ width: 6, height: 6, background: "#16A34A", borderRadius: "50%" }} /> conectat
                </span>
                <button className="btn-icon"><Icon name="refresh" size={13} /></button>
              </span>
            </div>
            <div className="panel-body" style={{ padding: "4px 12px 8px" }}>
              <div className="timeline">
                {window.DATA.ANAF_EVENTS.map((e, i) => (
                  <div key={i} className={"timeline-row " + e.kind}>
                    <span className="dot" />
                    <span className="time">{e.time}</span>
                    <span className="what">
                      {e.label}
                      <span className="meta">{e.detail}</span>
                    </span>
                  </div>
                ))}
                <div className="timeline-row info">
                  <span className="dot" />
                  <span className="time">11:42:18</span>
                  <span className="what">Sincronizare automată mesaje SPV
                    <span className="meta">3 mesaje noi descărcate · 0 erori</span>
                  </span>
                </div>
              </div>
            </div>
          </div>
        </div>

        {/* Recent invoices table */}
        <div className="panel">
          <div className="panel-header">
            <span>Facturi recente · ultimele 10</span>
            <span style={{ display: "flex", gap: 6 }}>
              <button className="btn compact" onClick={() => onNavigate("factura-noua")}>
                <Icon name="plus" size={12} /> Factură nouă <span className="kbd" style={{ marginLeft: 6 }}>Ctrl N</span>
              </button>
              <button className="btn compact" onClick={() => onNavigate("facturi-emise")}>
                Vezi toate (1.247) <Icon name="arrowRight" size={11} />
              </button>
            </span>
          </div>
          <table className="dt">
            <thead>
              <tr>
                <th style={{ width: 130 }}>Nr. factură</th>
                <th style={{ width: 92 }}>Data</th>
                <th>Cumpărător</th>
                <th style={{ width: 100 }}>CUI</th>
                <th className="num" style={{ width: 110 }}>Net (RON)</th>
                <th className="num" style={{ width: 90 }}>TVA</th>
                <th className="num" style={{ width: 120 }}>Total</th>
                <th style={{ width: 120 }}>Status ANAF</th>
                <th style={{ width: 110 }}>Index ANAF</th>
                <th style={{ width: 24 }}></th>
              </tr>
            </thead>
            <tbody>
              {sortedInvoices.map((inv, i) => (
                <tr key={inv.no} onClick={() => onOpenInvoice(inv)} style={{ cursor: "pointer" }}>
                  <td className="mono"><b>{inv.no}</b></td>
                  <td className="muted">{inv.date}</td>
                  <td>{inv.client.name}</td>
                  <td className="mono muted">{inv.client.cui}</td>
                  <td className="num tnum">{fmtRON(inv.net)}</td>
                  <td className="num tnum muted">{fmtRON(inv.vat)}</td>
                  <td className="num tnum"><b>{fmtRON(inv.total)}</b></td>
                  <td><StatusBadge status={inv.status} /></td>
                  <td className="mono dim">{inv.anafId || "—"}</td>
                  <td><Icon name="chevronRight" size={12} style={{ color: "var(--text-dim)" }} /></td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        <div style={{ fontSize: 10.5, color: "var(--text-dim)", padding: "4px 2px 20px" }}>
          Datele se actualizează automat la fiecare 60 secunde. Apasă <span className="kbd">F5</span> pentru reîmprospătare manuală.
          Toate sumele sunt în <b>RON</b> conform cursului BNR din data emiterii.
        </div>
      </div>
    </div>
  );
};

Object.assign(window, { Dashboard, StatusBadge, fmtRON });
