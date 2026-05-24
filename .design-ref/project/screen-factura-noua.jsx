/* ----------------------------------------------------------------------
   Factură nouă — invoice editor
   Sections: Antet, Linii (editable inline table), Plată, Note
   Right-side validation panel with live RO_CIUS errors + [Corectează]
   ---------------------------------------------------------------------- */

const SAMPLE_LINES = [
  { id: 1, code: "SRV-001",     desc: "Servicii consultanță IT — mai 2026",        qty: 80,  um: "ore",  price: 150.00, vat: 19, total: 14280.00 },
  { id: 2, code: "LIC-OFFICE",  desc: "Licență anuală Microsoft 365 Business",     qty:  5,  um: "buc",  price: 285.00, vat: 19, total:  1695.75 },
  { id: 3, code: "—",           desc: "Decont transport delegație Brașov",         qty:  1,  um: "buc",  price:  420.00,vat: 19, total:   499.80 },
];

const FacturaNoua = ({ activeCompany, onCancel, onSave }) => {
  const [client, setClient] = React.useState("RO27543210");
  const [vatStatus, setVatStatus] = React.useState("invalid"); // pretend live ANAF check

  const lines = SAMPLE_LINES;
  const net = lines.reduce((s, l) => s + l.qty * l.price, 0);
  const vat = net * 0.19;
  const total = net + vat;

  return (
    <div className="content">
      <div className="content-titlebar">
        <span className="content-title">
          <span className="crumb">e-Factura</span>
          <span className="crumb">Facturi emise</span>
          Factură nouă · <span className="mono" style={{ fontWeight: 400, color: "var(--text-muted)" }}>{activeCompany.serie}-{String(activeCompany.lastNo + 1).padStart(7, "0")}</span>
        </span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
          <button className="btn" onClick={onCancel}>
            <Icon name="x" size={12} /> Renunță <span className="kbd" style={{ marginLeft: 6 }}>Esc</span>
          </button>
          <button className="btn">
            <Icon name="draft" size={12} /> Salvează ca schiță <span className="kbd" style={{ marginLeft: 6 }}>Ctrl S</span>
          </button>
          <button className="btn">
            <Icon name="eye" size={12} /> Previzualizare PDF
          </button>
          <button className="btn primary" disabled style={{ opacity: 0.5, cursor: "not-allowed" }}>
            <Icon name="cloudUp" size={12} /> Trimite la ANAF <span className="kbd" style={{ marginLeft: 6, opacity: 0.7 }}>F9</span>
          </button>
        </span>
      </div>

      <div className="editor-split">
        {/* MAIN editor area */}
        <div className="editor-main">

          {/* Antet */}
          <div className="panel" style={{ marginBottom: 12 }}>
            <div className="panel-header">
              <span>Antet factură · date generale</span>
              <span style={{ display: "flex", gap: 6 }}>
                <span className="kbd">Tab</span>
                <span style={{ textTransform: "none", letterSpacing: 0, fontWeight: 400, fontSize: 10.5 }}>pentru câmpul următor</span>
              </span>
            </div>
            <div className="panel-body">
              <div className="form-grid">
                <div className="form-section-title">Emitent</div>
                <label>Companie emitentă</label>
                <div className="field">
                  <input className="input" defaultValue={activeCompany.name} readOnly style={{ width: 320, background: "var(--bg)" }} />
                  <span className="mono muted" style={{ fontSize: 11 }}>CUI {activeCompany.cui} · {activeCompany.regCom}</span>
                </div>
                <label>Serie / Număr</label>
                <div className="field">
                  <input className="input mono" defaultValue={activeCompany.serie} style={{ width: 90 }} />
                  <input className="input mono" defaultValue={String(activeCompany.lastNo + 1).padStart(7, "0")} style={{ width: 120 }} />
                  <span className="dim" style={{ fontSize: 11 }}>auto-incrementat · ultima emisă: {activeCompany.serie}-{String(activeCompany.lastNo).padStart(7, "0")}</span>
                </div>
                <label>Data emiterii</label>
                <div className="field">
                  <input className="input" defaultValue="19.05.2026" style={{ width: 110 }} />
                  <Icon name="calendar" size={14} style={{ color: "var(--text-muted)" }} />
                  <span className="muted" style={{ fontSize: 11 }}>Curs BNR: <b>4,9750 EUR</b> · <b>4,5821 USD</b></span>
                </div>
                <label>Data scadenței</label>
                <div className="field">
                  <input className="input" defaultValue="18.06.2026" style={{ width: 110 }} />
                  <span className="muted" style={{ fontSize: 11 }}>30 zile · termen standard cumpărător</span>
                </div>

                <div className="form-section-title">Cumpărător</div>
                <label>CUI cumpărător</label>
                <div className="field">
                  <input
                    className={"input mono" + (vatStatus === "invalid" ? " invalid" : "")}
                    value={client}
                    onChange={(e) => setClient(e.target.value)}
                    style={{ width: 140 }}
                  />
                  <button className="btn compact"><Icon name="search" size={11} /> Caută ANAF</button>
                  {vatStatus === "invalid" && (
                    <span className="field-error">
                      <Icon name="alert" size={12} />
                      TVA neînregistrat la 19.05.2026
                      <button className="fix" onClick={() => setVatStatus("ok")}>Schimbă în neplătitor</button>
                    </span>
                  )}
                  {vatStatus === "ok" && (
                    <span style={{ display: "inline-flex", alignItems: "center", gap: 4, color: "#16A34A", fontSize: 11 }}>
                      <Icon name="check" size={12} /> Validat la ANAF · neplătitor TVA
                    </span>
                  )}
                </div>
                <label>Denumire</label>
                <div className="field">
                  <input className="input" defaultValue="Globex Distribuție SRL" style={{ width: 360 }} />
                  <button className="btn-icon" title="Sincronizează din ANAF"><Icon name="refresh" size={13} /></button>
                </div>
                <label>Adresă</label>
                <div className="field">
                  <input className="input" defaultValue="Str. Industriilor nr. 42, et. 3" style={{ width: 360 }} />
                </div>
                <label>Localitate / Județ</label>
                <div className="field">
                  <input className="input" defaultValue="București" style={{ width: 140 }} />
                  <input className="input invalid" defaultValue="" placeholder="Cod ISO (ex: B)" style={{ width: 110 }} />
                  <span className="field-error">
                    <Icon name="alert" size={12} />
                    Lipsește cod ISO 3166-2:RO
                    <button className="fix">Completează B</button>
                  </span>
                </div>
                <label>Reprezentant</label>
                <div className="field">
                  <input className="input" defaultValue="Andrei Marinescu" style={{ width: 200 }} />
                  <input className="input" defaultValue="andrei.marinescu@globex.ro" style={{ width: 240 }} />
                </div>
              </div>
            </div>
          </div>

          {/* Linii */}
          <div className="panel" style={{ marginBottom: 12 }}>
            <div className="panel-header">
              <span>Linii factură · {lines.length} articole</span>
              <span style={{ display: "flex", gap: 6, alignItems: "center" }}>
                <span style={{ fontSize: 10.5, fontWeight: 400, textTransform: "none", letterSpacing: 0, color: "var(--text-muted)" }}>
                  Tasta <span className="kbd">↓</span> pe ultima linie creează una nouă · <span className="kbd">F4</span> deschide catalog articole
                </span>
              </span>
            </div>
            <div className="line-items">
              <table>
                <thead>
                  <tr>
                    <th style={{ width: 28 }}>#</th>
                    <th style={{ width: 110 }}>Cod</th>
                    <th>Descriere</th>
                    <th style={{ width: 64 }} className="num">Cant.</th>
                    <th style={{ width: 56 }}>UM</th>
                    <th style={{ width: 100 }} className="num">Preț unitar</th>
                    <th style={{ width: 64 }} className="num">TVA %</th>
                    <th style={{ width: 110 }} className="num">Valoare net</th>
                    <th style={{ width: 110 }} className="num">Total cu TVA</th>
                    <th style={{ width: 28 }}></th>
                  </tr>
                </thead>
                <tbody>
                  {lines.map((l, i) => {
                    const lineNet = l.qty * l.price;
                    const lineTotal = lineNet * (1 + l.vat / 100);
                    return (
                      <tr key={l.id}>
                        <td style={{ textAlign: "center", color: "var(--text-dim)", fontFamily: "var(--font-mono)" }}>{i + 1}</td>
                        <td><input defaultValue={l.code} className="mono" /></td>
                        <td><input defaultValue={l.desc} style={{ background: i === 2 ? "var(--accent-soft)" : "transparent" }} /></td>
                        <td className="num"><input defaultValue={l.qty} className="num" /></td>
                        <td><input defaultValue={l.um} /></td>
                        <td className="num"><input defaultValue={l.price.toFixed(2)} className="num" /></td>
                        <td className="num"><input defaultValue={l.vat} className="num" /></td>
                        <td className="num"><input defaultValue={lineNet.toFixed(2)} className="num" readOnly style={{ color: "var(--text-muted)" }} /></td>
                        <td className="num"><input defaultValue={lineTotal.toFixed(2)} className="num" readOnly style={{ fontWeight: 600 }} /></td>
                        <td><button className="btn-icon"><Icon name="trash" size={12} /></button></td>
                      </tr>
                    );
                  })}
                  <tr className="line-add-row">
                    <td colSpan={10}><Icon name="plus" size={12} /> Adaugă linie · sau caută articol din catalog cu <span className="kbd">F4</span></td>
                  </tr>
                </tbody>
                <tfoot>
                  <tr>
                    <td colSpan={6} style={{ textAlign: "right", color: "var(--text-muted)" }}>Subtotal net</td>
                    <td className="num"></td>
                    <td className="num tnum">{fmtRON(net)}</td>
                    <td className="num"></td>
                    <td></td>
                  </tr>
                  <tr>
                    <td colSpan={6} style={{ textAlign: "right", color: "var(--text-muted)" }}>TVA 19%</td>
                    <td className="num"></td>
                    <td className="num tnum">{fmtRON(vat)}</td>
                    <td className="num"></td>
                    <td></td>
                  </tr>
                  <tr>
                    <td colSpan={6} style={{ textAlign: "right", textTransform: "uppercase", fontSize: 11, letterSpacing: 0.04 }}>Total de plată</td>
                    <td className="num"></td>
                    <td className="num"></td>
                    <td className="num tnum" style={{ fontSize: 14, color: "var(--accent)" }}>{fmtRON(total)} <span style={{ fontSize: 10.5, color: "var(--text-muted)" }}>RON</span></td>
                    <td></td>
                  </tr>
                </tfoot>
              </table>
            </div>
          </div>

          {/* Plată + Note */}
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
            <div className="panel">
              <div className="panel-header"><span>Modalitate de plată</span><span /></div>
              <div className="panel-body">
                <div className="form-grid" style={{ gridTemplateColumns: "120px 1fr" }}>
                  <label>Metodă</label>
                  <div className="field">
                    <select className="select" defaultValue="ot">
                      <option value="ot">Ordin de plată (OP)</option>
                      <option value="cash">Numerar</option>
                      <option value="card">Card bancar</option>
                      <option value="comp">Compensare</option>
                    </select>
                  </div>
                  <label>Cont bancar</label>
                  <div className="field">
                    <input className="input mono" defaultValue="RO12 BTRL 0240 1202 0000 6789" style={{ width: 250 }} />
                    <span className="muted" style={{ fontSize: 11 }}>Banca Transilvania</span>
                  </div>
                  <label>Referință</label>
                  <div className="field">
                    <input className="input" defaultValue="Plătiți în 30 zile de la data emiterii" />
                  </div>
                  <label>Tip fiscal</label>
                  <div className="field">
                    <div className="seg">
                      <span className="seg-item">Standard</span>
                      <span className="seg-item active">TVA la încasare</span>
                      <span className="seg-item">Intracom.</span>
                    </div>
                  </div>
                </div>
              </div>
            </div>

            <div className="panel">
              <div className="panel-header"><span>Note · clauze · referințe</span><span /></div>
              <div className="panel-body">
                <div className="form-grid" style={{ gridTemplateColumns: "120px 1fr" }}>
                  <label>Comandă / Contract</label>
                  <div className="field">
                    <input className="input" defaultValue="Contract C-2024-118" style={{ width: 200 }} />
                    <input className="input" defaultValue="anexa 7" style={{ width: 110 }} />
                  </div>
                  <label>Observații</label>
                  <div className="field" style={{ alignItems: "flex-start" }}>
                    <textarea className="input" style={{ width: "100%", height: 64, padding: 6, resize: "vertical" }}
                              defaultValue="Servicii prestate în baza contractului C-2024-118. Plata se va face în contul bancar menționat." />
                  </div>
                  <label>Etichete</label>
                  <div className="field" style={{ flexWrap: "wrap" }}>
                    <span className="chip on">consultanță</span>
                    <span className="chip on">recurent</span>
                    <span className="chip">+ adaugă</span>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>

        {/* VALIDATION PANEL */}
        <aside className="editor-validation">
          <div className="validation-summary">
            <h3>Validare RO_CIUS · live</h3>
            <div className="score">
              <span className="pct">{window.DATA.VALIDATION.score}%</span>
              <div className="validation-bar"><div className="fill" style={{ width: window.DATA.VALIDATION.score + "%" }} /></div>
            </div>
            <div style={{ fontSize: 11, color: "var(--text-muted)", marginTop: 4 }}>
              <Icon name="alert" size={11} style={{ color: "#DC2626" }} />{" "}
              <b style={{ color: "#DC2626" }}>2 erori</b> blochează trimiterea către ANAF · <b style={{ color: "#D97706" }}>1 avertisment</b>
            </div>
          </div>
          <div className="validation-items">
            {window.DATA.VALIDATION.items.map((it, i) => (
              <div key={i} className={"validation-item " + it.kind}>
                <span className="ico">
                  <Icon name={it.kind === "ok" ? "check" : it.kind === "err" ? "cancel" : "warning"} size={13} />
                </span>
                <span>
                  <div className="title">{it.title}</div>
                  <div className="desc">{it.desc}</div>
                  {it.fix && <button className="fix-btn"><Icon name="pen" size={10} /> {it.fix}</button>}
                </span>
              </div>
            ))}
          </div>

          <div style={{ marginTop: "auto", borderTop: "1px solid var(--border)", padding: "10px 12px", background: "var(--bg)", fontSize: 11, color: "var(--text-muted)" }}>
            <div style={{ fontSize: 10, textTransform: "uppercase", letterSpacing: 0.1, color: "var(--text-dim)", marginBottom: 4 }}>
              Validare automată
            </div>
            Schema: <b>CIUS-RO 1.0.1</b><br />
            Verificat acum 0,4s · <a href="#" style={{ color: "var(--accent)" }}>vezi log complet</a>
          </div>
        </aside>
      </div>
    </div>
  );
};

window.FacturaNoua = FacturaNoua;
