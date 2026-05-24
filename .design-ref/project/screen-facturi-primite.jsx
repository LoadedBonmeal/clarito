/* ----------------------------------------------------------------------
   Facturi primite — received invoices (from SPV ANAF)
   Table with issuer, amount, status; inline approve/reject on hover
   ---------------------------------------------------------------------- */

const FacturiPrimite = ({ invoices }) => {
  const [query, setQuery] = React.useState("");
  const [filter, setFilter] = React.useState("all");
  const [selected, setSelected] = React.useState(new Set());
  const [hoverId, setHoverId] = React.useState(null);

  const q = query.trim().toLowerCase();
  const list = invoices
    .filter(i => !q || i.no.toLowerCase().includes(q) || i.issuer.name.toLowerCase().includes(q) || i.issuer.cui.toLowerCase().includes(q))
    .filter(i => filter === "all" ? true : i.status === filter);

  const counts = {
    all:      invoices.length,
    new:      invoices.filter(i => i.status === "new").length,
    reviewed: invoices.filter(i => i.status === "reviewed").length,
    approved: invoices.filter(i => i.status === "approved").length,
    rejected: invoices.filter(i => i.status === "rejected").length,
    archived: invoices.filter(i => i.status === "archived").length,
  };

  const totalSum = list.reduce((s, i) => s + i.total, 0);

  const toggleOne = (id) => {
    const next = new Set(selected);
    next.has(id) ? next.delete(id) : next.add(id);
    setSelected(next);
  };

  return (
    <div className="content">
      <div className="content-titlebar">
        <span className="content-title">
          <span className="crumb">e-Factura</span>
          Facturi primite
        </span>
        <span className="muted" style={{ fontSize: 11 }}>{list.length} facturi · {fmtRON(totalSum)} RON</span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
          <button className="btn"><Icon name="upload" size={12} /> Import XML</button>
          <button className="btn"><Icon name="download" size={12} /> Export selecție</button>
          <button className="btn primary"><Icon name="cloudDn" size={12} /> Descarcă din SPV <span className="kbd" style={{ marginLeft: 6, background: "rgba(255,255,255,0.18)", border: "1px solid rgba(255,255,255,0.3)", color: "#fff" }}>F5</span></button>
        </span>
      </div>

      {/* Saved views row */}
      <div className="views-bar">
        <span className={"view-tab " + (filter === "all"      ? "active" : "")} onClick={() => setFilter("all")}>Toate <span className="count">{counts.all}</span></span>
        <span className={"view-tab " + (filter === "new"      ? "active" : "")} onClick={() => setFilter("new")}>Noi <span className="count" style={{ color: "var(--accent)" }}>{counts.new}</span></span>
        <span className={"view-tab " + (filter === "reviewed" ? "active" : "")} onClick={() => setFilter("reviewed")}>De revizuit <span className="count">{counts.reviewed}</span></span>
        <span className={"view-tab " + (filter === "approved" ? "active" : "")} onClick={() => setFilter("approved")}>Aprobate <span className="count">{counts.approved}</span></span>
        <span className={"view-tab " + (filter === "rejected" ? "active" : "")} onClick={() => setFilter("rejected")}>Respinse <span className="count" style={{ color: "#DC2626" }}>{counts.rejected}</span></span>
        <span className={"view-tab " + (filter === "archived" ? "active" : "")} onClick={() => setFilter("archived")}>Arhivate <span className="count">{counts.archived}</span></span>
        <span className="view-tab" style={{ color: "var(--accent)", borderRight: 0 }}>
          <Icon name="plus" size={11} /> Salvează vizualizarea
        </span>
      </div>

      {/* Toolbar */}
      <div className="content-toolbar">
        <div className="search">
          <Icon name="search" size={13} />
          <input placeholder="Caută după nr., CUI emitent sau denumire…" value={query} onChange={(e) => setQuery(e.target.value)} />
          <span className="kbd-hint">Ctrl F</span>
        </div>
        <span className="divider-v" style={{ margin: "0 4px" }} />
        <span className="chip">Perioadă: Mai 2026 <Icon name="caret" size={10} /></span>
        <span className="chip">Categorie: toate <Icon name="caret" size={10} /></span>
        <span className="chip">Sumă: orice <Icon name="caret" size={10} /></span>
        <span className="chip on">Sortare: data ▾ <Icon name="caret" size={10} className="x" /></span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6, alignItems: "center" }}>
          {selected.size > 0 ? (
            <>
              <span style={{ fontSize: 11, fontWeight: 600 }}>{selected.size} selectate</span>
              <button className="btn compact primary"><Icon name="check" size={11} /> Aprobă toate</button>
              <button className="btn compact"><Icon name="tag" size={11} /> Categorie</button>
              <button className="btn compact"><Icon name="bookmark" size={11} /> Arhivează</button>
              <button className="btn compact danger" style={{ height: 22 }}><Icon name="x" size={11} /> Respinge</button>
              <span className="divider-v" style={{ margin: "0 4px" }} />
            </>
          ) : (
            <span style={{ fontSize: 10.5, color: "var(--text-dim)" }}>
              Treci cu mouse-ul peste un rând pentru aprobare/respingere rapidă
            </span>
          )}
          <button className="btn-icon" title="Coloane"><Icon name="filter" size={14} /></button>
          <button className="btn-icon" title="Mai multe"><Icon name="more" size={14} /></button>
        </span>
      </div>

      <div className="content-body">
        <table className="dt">
          <thead>
            <tr>
              <th className="ck">
                <input type="checkbox" className="cbx"
                       checked={selected.size === list.length && list.length > 0}
                       onChange={() => setSelected(selected.size === list.length ? new Set() : new Set(list.map(i => i.id)))} />
              </th>
              <th style={{ width: 130 }}>Nr. document</th>
              <th style={{ width: 96 }} className="sortable sorted">Data ↓</th>
              <th style={{ width: 110 }}>CUI emitent</th>
              <th>Emitent</th>
              <th style={{ width: 110 }}>Categorie</th>
              <th className="num" style={{ width: 110 }}>Net (RON)</th>
              <th className="num" style={{ width: 90 }}>TVA</th>
              <th className="num" style={{ width: 120 }}>Total</th>
              <th style={{ width: 100 }}>Scadență</th>
              <th style={{ width: 130 }}>Status</th>
              <th style={{ width: 150 }}>Acțiuni</th>
            </tr>
          </thead>
          <tbody>
            {list.map(i => {
              const isHover = hoverId === i.id;
              return (
                <tr key={i.id}
                    onMouseEnter={() => setHoverId(i.id)}
                    onMouseLeave={() => setHoverId(null)}
                    className={selected.has(i.id) ? "selected" : ""}>
                  <td className="ck" onClick={(e) => e.stopPropagation()}>
                    <input type="checkbox" className="cbx" checked={selected.has(i.id)} onChange={() => toggleOne(i.id)} />
                  </td>
                  <td className="mono"><b>{i.no}</b></td>
                  <td className="muted">{i.date}</td>
                  <td className="mono">{i.issuer.cui}</td>
                  <td>{i.issuer.name}
                    {i.note && (
                      <span style={{ marginLeft: 8, color: "#DC2626", fontSize: 10.5 }}>
                        <Icon name="alert" size={10} /> {i.note}
                      </span>
                    )}
                  </td>
                  <td className="muted">{i.category}</td>
                  <td className="num tnum muted">{fmtRON(i.net)}</td>
                  <td className="num tnum dim">{fmtRON(i.vat)}</td>
                  <td className="num tnum"><b>{fmtRON(i.total)}</b></td>
                  <td className="muted">{i.due}</td>
                  <td><StatusBadge status={i.status} /></td>
                  <td onClick={(e) => e.stopPropagation()}>
                    {isHover ? (
                      <div style={{ display: "flex", gap: 2 }}>
                        {(i.status === "new" || i.status === "reviewed") && (
                          <>
                            <button className="btn compact primary" title="Aprobă">
                              <Icon name="check" size={11} /> Aprobă
                            </button>
                            <button className="btn compact" style={{ borderColor: "#FCA5A5", color: "#B91C1C" }} title="Respinge">
                              <Icon name="x" size={11} /> Respinge
                            </button>
                          </>
                        )}
                        {i.status === "approved" && (
                          <button className="btn compact">
                            <Icon name="bookmark" size={11} /> Arhivează
                          </button>
                        )}
                        {i.status === "rejected" && (
                          <button className="btn compact">
                            <Icon name="refresh" size={11} /> Reanalizează
                          </button>
                        )}
                        <button className="btn-icon" title="Detalii"><Icon name="eye" size={13} /></button>
                        <button className="btn-icon" title="Descarcă XML"><Icon name="download" size={13} /></button>
                      </div>
                    ) : (
                      <div className="dim" style={{ display: "flex", gap: 2, opacity: 0.6 }}>
                        <button className="btn-icon"><Icon name="eye" size={13} /></button>
                        <button className="btn-icon"><Icon name="download" size={13} /></button>
                        <button className="btn-icon"><Icon name="more" size={13} /></button>
                      </div>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      <div style={{ padding: "6px 14px", borderTop: "1px solid var(--border)", background: "var(--bg)", display: "flex", gap: 16, fontSize: 11, color: "var(--text-muted)" }}>
        <span>Total: <b style={{ color: "var(--text)" }}>{list.length}</b> facturi</span>
        <span>Suma totală: <b style={{ color: "var(--text)" }} className="tnum">{fmtRON(totalSum)} RON</b></span>
        <span>De aprobat: <b style={{ color: "var(--accent)" }}>{counts.new + counts.reviewed}</b></span>
        <span style={{ marginLeft: "auto" }}>
          <span className="kbd">A</span> aprobă · <span className="kbd">R</span> respinge · <span className="kbd">↑↓</span> navighează
        </span>
      </div>
    </div>
  );
};

window.FacturiPrimite = FacturiPrimite;
