/* ----------------------------------------------------------------------
   Companii — list of administered SRLs
   Dense data table, toolbar with search/filters/add button
   ---------------------------------------------------------------------- */

const Companii = ({ companies, activeCompanyId, onPick }) => {
  const [query, setQuery] = React.useState("");
  const [filterSPV, setFilterSPV] = React.useState("all"); // all | yes | no
  const [selected, setSelected] = React.useState(new Set());

  const q = query.trim().toLowerCase();
  const list = companies
    .filter(c => !q || c.name.toLowerCase().includes(q) || c.cui.toLowerCase().includes(q) || c.city.toLowerCase().includes(q))
    .filter(c => filterSPV === "all" ? true : filterSPV === "yes" ? c.spv : !c.spv);

  const toggleAll = () => {
    if (selected.size === list.length) setSelected(new Set());
    else setSelected(new Set(list.map(c => c.id)));
  };
  const toggleOne = (id) => {
    const next = new Set(selected);
    next.has(id) ? next.delete(id) : next.add(id);
    setSelected(next);
  };

  return (
    <div className="content">
      <div className="content-titlebar">
        <span className="content-title">
          <span className="crumb">Date</span>
          Companii administrate
        </span>
        <span className="muted" style={{ fontSize: 11 }}>{list.length} din {companies.length} companii</span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
          <button className="btn"><Icon name="upload" size={12} /> Import CSV</button>
          <button className="btn"><Icon name="download" size={12} /> Export</button>
          <button className="btn primary"><Icon name="plus" size={12} /> Adaugă companie</button>
        </span>
      </div>

      {/* Saved views */}
      <div className="views-bar">
        <span className="view-tab active">Toate <span className="count">{companies.length}</span></span>
        <span className="view-tab">Cu SPV activ <span className="count">{companies.filter(c => c.spv).length}</span></span>
        <span className="view-tab">Fără SPV <span className="count">{companies.filter(c => !c.spv).length}</span></span>
        <span className="view-tab">Alerte ANAF <span className="count">3</span></span>
        <span className="view-tab">Recent active <span className="count">8</span></span>
        <span className="view-tab" style={{ color: "var(--accent)", borderRight: 0 }}>
          <Icon name="plus" size={11} /> Salvează vizualizarea
        </span>
      </div>

      {/* Toolbar */}
      <div className="content-toolbar">
        <div className="search">
          <Icon name="search" size={13} />
          <input placeholder="Caută după nume, CUI sau localitate…" value={query} onChange={(e) => setQuery(e.target.value)} />
          <span className="kbd-hint">Ctrl F</span>
        </div>
        <span className="divider-v" style={{ margin: "0 4px" }} />
        <span style={{ fontSize: 11, color: "var(--text-muted)" }}>SPV:</span>
        <div className="seg">
          <span className={"seg-item " + (filterSPV === "all" ? "active" : "")} onClick={() => setFilterSPV("all")}>Toate</span>
          <span className={"seg-item " + (filterSPV === "yes" ? "active" : "")} onClick={() => setFilterSPV("yes")}>Activ</span>
          <span className={"seg-item " + (filterSPV === "no"  ? "active" : "")} onClick={() => setFilterSPV("no")}>Inactiv</span>
        </div>
        <span className="chip">Județ: toate <Icon name="caret" size={10} className="x" /></span>
        <span className="chip">Tip: SRL/SA <Icon name="caret" size={10} className="x" /></span>
        <span className="chip on">Mai 2026 <Icon name="x" size={11} className="x" /></span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6, alignItems: "center" }}>
          {selected.size > 0 && (
            <>
              <span style={{ fontSize: 11, fontWeight: 600 }}>{selected.size} selectate</span>
              <button className="btn compact"><Icon name="cloudUp" size={11} /> Sync SPV</button>
              <button className="btn compact"><Icon name="tag" size={11} /> Etichetează</button>
              <button className="btn compact danger" style={{ height: 22 }}><Icon name="trash" size={11} /> Arhivează</button>
              <span className="divider-v" style={{ margin: "0 4px" }} />
            </>
          )}
          <button className="btn-icon" title="Coloane"><Icon name="filter" size={14} /></button>
          <button className="btn-icon" title="Reîmprospătează"><Icon name="refresh" size={14} /></button>
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
                       onChange={toggleAll} />
              </th>
              <th style={{ width: 110 }} className="sortable sorted">CUI <span className="sort">▾</span></th>
              <th className="sortable">Denumire <span className="sort">▴▾</span></th>
              <th style={{ width: 140 }}>Localitate</th>
              <th style={{ width: 60 }}>Județ</th>
              <th style={{ width: 64 }} className="num">SPV</th>
              <th style={{ width: 84 }}>Serie</th>
              <th style={{ width: 80 }} className="num">Nr. ultim</th>
              <th style={{ width: 140 }}>Reg. Comerțului</th>
              <th style={{ width: 110 }}>Acțiuni</th>
            </tr>
          </thead>
          <tbody>
            {list.map((c) => (
              <tr key={c.id}
                  className={(c.id === activeCompanyId ? "selected" : "")}
                  onClick={() => onPick(c.id)}
                  style={{ cursor: "pointer" }}>
                <td className="ck" onClick={(e) => e.stopPropagation()}>
                  <input type="checkbox" className="cbx" checked={selected.has(c.id)} onChange={() => toggleOne(c.id)} />
                </td>
                <td className="mono">{c.cui}</td>
                <td>
                  <span style={{ display: "inline-block", width: 6, height: 6, background: c.color, marginRight: 6, verticalAlign: "middle" }} />
                  <b>{c.name}</b>
                  {c.id === activeCompanyId && <span className="kbd" style={{ marginLeft: 8 }}>activă</span>}
                </td>
                <td>{c.city}</td>
                <td className="mono">{c.county}</td>
                <td className="num">
                  {c.spv
                    ? <span style={{ color: "#16A34A", display: "inline-flex", alignItems: "center", gap: 3 }}><Icon name="check" size={13} /></span>
                    : <span className="dim"><Icon name="x" size={13} /></span>}
                </td>
                <td className="mono">{c.serie}</td>
                <td className="num tnum">{c.lastNo.toLocaleString("ro-RO")}</td>
                <td className="mono muted">{c.regCom}</td>
                <td onClick={(e) => e.stopPropagation()}>
                  <button className="btn-icon" title="Deschide"><Icon name="external" size={13} /></button>
                  <button className="btn-icon" title="Editează"><Icon name="pen" size={13} /></button>
                  <button className="btn-icon" title="Sync ANAF"><Icon name="refresh" size={13} /></button>
                  <button className="btn-icon" title="Mai multe"><Icon name="more" size={13} /></button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <div style={{ padding: "6px 14px", borderTop: "1px solid var(--border)", background: "var(--bg)", display: "flex", gap: 16, fontSize: 11, color: "var(--text-muted)" }}>
        <span>Total: <b style={{ color: "var(--text)" }}>{list.length}</b> companii</span>
        <span>Cu SPV: <b style={{ color: "var(--text)" }}>{list.filter(c => c.spv).length}</b></span>
        <span>Cu alertă: <b style={{ color: "#DC2626" }}>3</b></span>
        <span style={{ marginLeft: "auto" }}>
          Click pe rând pentru a activa compania · <span className="kbd">Ctrl K Ctrl C</span> deschide selector rapid
        </span>
      </div>
    </div>
  );
};

window.Companii = Companii;
