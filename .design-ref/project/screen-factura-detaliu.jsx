/* ----------------------------------------------------------------------
   Detaliu factură — 60/40 split: PDF preview + metadata/timeline
   ---------------------------------------------------------------------- */

const FacturaDetaliu = ({ activeCompany, invoice, onClose, onNavigate }) => {
  if (!invoice) return null;
  const lines = [
    { code: "SRV-001",     desc: "Servicii consultanță IT — mai 2026",        qty: 80, um: "ore", price: 150.00, vat: 19 },
    { code: "LIC-OFFICE",  desc: "Licență anuală Microsoft 365 Business",     qty:  5, um: "buc", price: 285.00, vat: 19 },
    { code: "TRSP",        desc: "Decont transport delegație Brașov",         qty:  1, um: "buc", price: 420.00, vat: 19 },
  ];
  const net = lines.reduce((s, l) => s + l.qty * l.price, 0);
  const vat = net * 0.19;
  const total = net + vat;

  return (
    <div className="content">
      <div className="content-titlebar">
        <span className="content-title">
          <span className="crumb">e-Factura</span>
          <span className="crumb" onClick={() => onNavigate("facturi-emise")} style={{ cursor: "pointer" }}>Facturi emise</span>
          <span className="mono">{invoice.no}</span>
        </span>
        <span style={{ marginLeft: 12 }}>
          <StatusBadge status={invoice.status} />
        </span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
          <button className="btn" onClick={onClose}><Icon name="chevronLeft" size={12} /> Înapoi</button>
          <button className="btn"><Icon name="chevronLeft" size={12} /> Anterioara</button>
          <button className="btn">Următoarea <Icon name="chevronRight" size={12} /></button>
          <span className="divider-v" style={{ margin: "0 4px" }} />
          <button className="btn"><Icon name="copy" size={12} /> Duplică</button>
          <button className="btn"><Icon name="storno" size={12} /> Storno</button>
          <button className="btn"><Icon name="mail" size={12} /> Trimite email</button>
          <button className="btn"><Icon name="printer" size={12} /> Tipărește</button>
        </span>
      </div>

      <div className="detail-actions">
        <button className="btn primary"><Icon name="refresh" size={12} /> Verifică status ANAF <span className="kbd" style={{ marginLeft: 6, background: "rgba(255,255,255,0.18)", border: "1px solid rgba(255,255,255,0.3)", color: "#fff" }}>F10</span></button>
        <button className="btn"><Icon name="download" size={12} /> Descarcă XML semnat</button>
        <button className="btn"><Icon name="download" size={12} /> Descarcă PDF</button>
        <button className="btn"><Icon name="external" size={12} /> Deschide în SPV ANAF</button>
        <span style={{ marginLeft: "auto", display: "inline-flex", alignItems: "center", gap: 6, fontSize: 11, color: "var(--text-muted)" }}>
          <Icon name="clock" size={12} />
          Ultima sincronizare cu ANAF: <b style={{ color: "var(--text)" }}>12:34:09</b> · acum 14 secunde
        </span>
      </div>

      <div className="split-60-40">
        {/* PDF preview */}
        <div className="invoice-preview">
          <div className="invoice-paper">
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start" }}>
              <div>
                <h1>Factură fiscală</h1>
                <div style={{ fontSize: 10, color: "#777", letterSpacing: "0.06em" }}>
                  {invoice.no} · {invoice.date}
                </div>
              </div>
              <div style={{ textAlign: "right" }}>
                <div className="seal">e-Factura · validată</div>
                <div style={{ fontSize: 9.5, color: "#777", marginTop: 4 }}>
                  Index ANAF: <span style={{ fontFamily: "var(--font-mono)" }}>{invoice.anafId || "—"}</span>
                </div>
              </div>
            </div>

            <div className="ip-grid">
              <div>
                <div className="ip-label">Furnizor</div>
                <div style={{ fontWeight: 700, marginTop: 4 }}>{activeCompany.name}</div>
                <div style={{ fontFamily: "var(--font-mono)", fontSize: 10 }}>{activeCompany.cui} · {activeCompany.regCom}</div>
                <div style={{ fontSize: 10.5, color: "#555" }}>Str. Lipscani nr. 18, et. 4<br />{activeCompany.city}, jud. {activeCompany.county}</div>
                <div style={{ fontFamily: "var(--font-mono)", fontSize: 10, marginTop: 4 }}>RO12 BTRL 0240 1202 0000 6789 · BT</div>
              </div>
              <div>
                <div className="ip-label">Cumpărător</div>
                <div style={{ fontWeight: 700, marginTop: 4 }}>{invoice.client.name}</div>
                <div style={{ fontFamily: "var(--font-mono)", fontSize: 10 }}>{invoice.client.cui}</div>
                <div style={{ fontSize: 10.5, color: "#555" }}>Str. Industriilor nr. 42, et. 3<br />{invoice.client.city}, jud. {invoice.client.county}</div>
                <div style={{ fontSize: 10, color: "#555", marginTop: 4 }}>Reprezentant: Andrei Marinescu</div>
              </div>
            </div>

            <table>
              <thead>
                <tr>
                  <th style={{ width: 24 }}>#</th>
                  <th>Descriere</th>
                  <th style={{ width: 50, textAlign: "right" }}>UM</th>
                  <th style={{ width: 50, textAlign: "right" }}>Cant.</th>
                  <th style={{ width: 70, textAlign: "right" }}>Preț</th>
                  <th style={{ width: 40, textAlign: "right" }}>TVA</th>
                  <th style={{ width: 80, textAlign: "right" }}>Valoare</th>
                </tr>
              </thead>
              <tbody>
                {lines.map((l, i) => (
                  <tr key={i}>
                    <td style={{ color: "#999" }}>{i + 1}</td>
                    <td>
                      <div style={{ fontWeight: 600 }}>{l.desc}</div>
                      <div style={{ fontSize: 9, color: "#888", fontFamily: "var(--font-mono)" }}>cod: {l.code}</div>
                    </td>
                    <td style={{ textAlign: "right" }}>{l.um}</td>
                    <td style={{ textAlign: "right" }}>{l.qty}</td>
                    <td style={{ textAlign: "right" }}>{fmtRON(l.price)}</td>
                    <td style={{ textAlign: "right" }}>{l.vat}%</td>
                    <td style={{ textAlign: "right", fontWeight: 600 }}>{fmtRON(l.qty * l.price)}</td>
                  </tr>
                ))}
              </tbody>
            </table>

            <div className="totals">
              <div className="row"><span>Subtotal net</span><span>{fmtRON(net)} RON</span></div>
              <div className="row"><span>TVA 19%</span><span>{fmtRON(vat)} RON</span></div>
              <div className="row grand"><span>Total de plată</span><span>{fmtRON(total)} RON</span></div>
            </div>

            <div style={{ marginTop: 26, fontSize: 9.5, color: "#777", borderTop: "1px solid #DDD", paddingTop: 8 }}>
              Plata se va efectua în 30 zile de la data emiterii. Tip fiscal: TVA la încasare. Document generat și validat prin sistemul național e-Factura (RO_CIUS 1.0.1).
            </div>
          </div>

          <div style={{ marginTop: 8, fontSize: 10.5, color: "var(--text-muted)" }}>
            Pagina 1 din 1 · <span className="kbd">+</span> mărește · <span className="kbd">−</span> micșorează
          </div>
        </div>

        {/* METADATA + TIMELINE */}
        <div className="invoice-meta">
          <div className="invoice-meta-section">
            <h3>Metadate factură</h3>
            <dl className="invoice-meta-kv">
              <dt>Număr</dt>          <dd><span className="mono"><b>{invoice.no}</b></span></dd>
              <dt>Data emiterii</dt>  <dd>{invoice.date}</dd>
              <dt>Data scadenței</dt> <dd>{invoice.due}</dd>
              <dt>Tip fiscal</dt>     <dd>TVA la încasare</dd>
              <dt>Serie</dt>          <dd className="mono">{activeCompany.serie}</dd>
              <dt>Monedă</dt>         <dd>RON · curs BNR 4,9750 EUR</dd>
            </dl>
          </div>

          <div className="invoice-meta-section">
            <h3>Status ANAF</h3>
            <div style={{ display: "flex", gap: 6, alignItems: "center", marginBottom: 6 }}>
              <StatusBadge status={invoice.status} />
              <span className="mono dim" style={{ fontSize: 10.5 }}>{invoice.anafId}</span>
            </div>
            <dl className="invoice-meta-kv">
              <dt>Trimisă la</dt>     <dd className="mono">12:33:42 · 19.05.2026</dd>
              <dt>Validată la</dt>    <dd className="mono">12:34:09 · 19.05.2026</dd>
              <dt>Timp procesare</dt> <dd>27 secunde</dd>
              <dt>Semnătură XML</dt>  <dd className="mono dim">SHA256: 4f1c…b8d2</dd>
            </dl>
          </div>

          <div className="invoice-meta-section">
            <h3>Evenimente · jurnal</h3>
            <div className="timeline">
              {window.DATA.ANAF_EVENTS.map((e, i) => (
                <div key={i} className={"timeline-row " + e.kind}>
                  <span className="dot" />
                  <span className="time">{e.time}</span>
                  <span className="what">{e.label}<span className="meta">{e.detail}</span></span>
                </div>
              ))}
            </div>
          </div>

          <div className="invoice-meta-section">
            <h3>Atașamente · 2</h3>
            <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 8, padding: 6, border: "1px solid var(--border-soft)", background: "var(--bg)", fontSize: 11.5 }}>
                <Icon name="file" size={14} style={{ color: "var(--text-muted)" }} />
                <span style={{ flex: 1 }}>{invoice.no}.xml</span>
                <span className="dim mono" style={{ fontSize: 10.5 }}>4,2 KB</span>
                <button className="btn-icon"><Icon name="download" size={12} /></button>
              </div>
              <div style={{ display: "flex", alignItems: "center", gap: 8, padding: 6, border: "1px solid var(--border-soft)", background: "var(--bg)", fontSize: 11.5 }}>
                <Icon name="file" size={14} style={{ color: "var(--text-muted)" }} />
                <span style={{ flex: 1 }}>{invoice.no}.pdf</span>
                <span className="dim mono" style={{ fontSize: 10.5 }}>118 KB</span>
                <button className="btn-icon"><Icon name="download" size={12} /></button>
              </div>
              <div style={{ marginTop: 4 }}>
                <button className="btn compact"><Icon name="plus" size={11} /> Atașează document</button>
              </div>
            </div>
          </div>

          <div className="invoice-meta-section" style={{ borderBottom: 0 }}>
            <h3>Acțiuni contextuale</h3>
            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 6 }}>
              <button className="btn"><Icon name="storno" size={12} /> Storno</button>
              <button className="btn"><Icon name="copy" size={12} /> Duplică</button>
              <button className="btn"><Icon name="mail" size={12} /> Email PDF</button>
              <button className="btn"><Icon name="link" size={12} /> Link public</button>
              <button className="btn"><Icon name="receipt" size={12} /> Chitanță</button>
              <button className="btn"><Icon name="history" size={12} /> Audit log</button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};

window.FacturaDetaliu = FacturaDetaliu;
