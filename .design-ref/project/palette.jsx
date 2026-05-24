/* ----------------------------------------------------------------------
   Cmd+K command palette overlay
   ---------------------------------------------------------------------- */

const Palette = ({ onClose, onAction }) => {
  const [query, setQuery] = React.useState("");
  const [active, setActive] = React.useState(0);
  const rootRef = React.useRef(null);

  // flatten + filter
  const q = query.trim().toLowerCase();
  const sections = window.DATA.PALETTE_ITEMS.map(s => ({
    ...s,
    items: s.items.filter(i => !q || i.label.toLowerCase().includes(q) || (i.kbd || "").toLowerCase().includes(q)),
  })).filter(s => s.items.length > 0);

  const flat = sections.flatMap(s => s.items);
  const safeActive = Math.min(active, Math.max(0, flat.length - 1));

  React.useEffect(() => {
    const onKey = (e) => {
      if (e.key === "Escape") { e.preventDefault(); onClose(); return; }
      if (e.key === "ArrowDown") { e.preventDefault(); setActive(a => Math.min(a + 1, flat.length - 1)); return; }
      if (e.key === "ArrowUp")   { e.preventDefault(); setActive(a => Math.max(a - 1, 0)); return; }
      if (e.key === "Enter")     { e.preventDefault(); onAction(flat[safeActive]); onClose(); return; }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [flat.length, safeActive]);

  let idx = -1;
  return (
    <div className="palette-scrim" onMouseDown={onClose}>
      <div className="palette" ref={rootRef} onMouseDown={(e) => e.stopPropagation()}>
        <div className="palette-input">
          <Icon name="command" size={16} />
          <input
            autoFocus
            placeholder="Caută acțiune, navigare sau companie…"
            value={query}
            onChange={(e) => { setQuery(e.target.value); setActive(0); }}
          />
          <span className="kbd">ESC</span>
        </div>
        <div className="palette-list">
          {sections.map((s, si) => (
            <div key={si}>
              <div className="palette-section">{s.section}</div>
              {s.items.map((it, ii) => {
                idx++;
                const isActive = idx === safeActive;
                const myIdx = idx;
                return (
                  <div
                    key={ii}
                    className={"palette-row" + (isActive ? " active" : "")}
                    onMouseEnter={() => setActive(myIdx)}
                    onClick={() => { onAction(it); onClose(); }}
                  >
                    <span className="ico"><Icon name={it.ico} size={14} /></span>
                    <span>{it.label}</span>
                    {it.kbd && <span className="kbd">{it.kbd}</span>}
                  </div>
                );
              })}
            </div>
          ))}
          {flat.length === 0 && (
            <div className="palette-section" style={{ padding: "30px 14px", textAlign: "center" }}>
              Nicio acțiune nu corespunde cu "{query}"
            </div>
          )}
        </div>
        <div className="palette-footer">
          <span><span className="kbd">↑</span><span className="kbd">↓</span> navighează</span>
          <span><span className="kbd">↵</span> execută</span>
          <span><span className="kbd">ESC</span> închide</span>
          <span style={{ marginLeft: "auto" }}>Tastează <span className="kbd">?</span> pentru ajutor</span>
        </div>
      </div>
    </div>
  );
};

window.Palette = Palette;
