/* ----------------------------------------------------------------------
   App shell — wires everything together
   ---------------------------------------------------------------------- */

const TWEAK_DEFAULTS = /*EDITMODE-BEGIN*/{
  "density": "compact",
  "accent": "navy",
  "dark": false,
  "lang": "ro"
}/*EDITMODE-END*/;

// accent presets: oklch params [l, c, h]
const ACCENT_PRESETS = {
  navy:    { l: 0.40, c: 0.14, h: 245, swatch: "#2848A1" },
  emerald: { l: 0.45, c: 0.13, h: 165, swatch: "#0F7A55" },
  burgundy:{ l: 0.42, c: 0.14, h:  18, swatch: "#9B2A2A" },
  teal:    { l: 0.45, c: 0.10, h: 200, swatch: "#1F6E8C" },
  graphite:{ l: 0.34, c: 0.02, h: 260, swatch: "#3F4451" },
};

function App() {
  const [t, setTweak] = useTweaks(TWEAK_DEFAULTS);

  const [screen, setScreen] = React.useState("dashboard");
  const [activeCompanyId, setActiveCompanyId] = React.useState("c1");
  const [openInvoice, setOpenInvoice] = React.useState(null);
  const [showPalette, setShowPalette] = React.useState(false);
  const [showCompanySwitcher, setShowCompanySwitcher] = React.useState(false);

  const companies = window.DATA.COMPANIES;
  const activeCompany = companies.find(c => c.id === activeCompanyId) || companies[0];

  // apply tweak classes to root
  const rootClass =
    "app density-" + t.density + (t.dark ? " dark" : "");

  // accent
  const accent = ACCENT_PRESETS[t.accent] || ACCENT_PRESETS.navy;
  const rootStyle = {
    "--accent-l": accent.l,
    "--accent-c": accent.c,
    "--accent-h": accent.h,
  };

  // global keyboard shortcuts
  React.useEffect(() => {
    const onKey = (e) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setShowPalette(true);
      }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "n" && !e.shiftKey) {
        e.preventDefault();
        navigate("factura-noua");
      }
      if (e.key === "F5") { e.preventDefault(); /* refresh sync */ }
      if (e.key === "Escape") {
        if (showPalette) setShowPalette(false);
        else if (showCompanySwitcher) setShowCompanySwitcher(false);
        else if (screen === "factura-detaliu") { setOpenInvoice(null); setScreen("facturi-emise"); }
        else if (screen === "factura-noua") setScreen("facturi-emise");
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [showPalette, showCompanySwitcher, screen]);

  const navigate = (s) => {
    if (s === "facturi-emise" || s === "dashboard" || s === "companii" || s === "facturi-primite" || s === "factura-noua") {
      setOpenInvoice(null);
    }
    setScreen(s);
  };

  const openInvoiceFn = (inv) => {
    setOpenInvoice(inv);
    setScreen("factura-detaliu");
  };

  return (
    <div className={rootClass} style={rootStyle}>
      <MenuBar
        activeCompany={activeCompany}
        onOpenCompanySwitcher={() => setShowCompanySwitcher(s => !s)}
        anafStatus="ok"
      />
      {showCompanySwitcher && (
        <CompanySwitcher
          companies={companies}
          activeId={activeCompanyId}
          onPick={setActiveCompanyId}
          onClose={() => setShowCompanySwitcher(false)}
        />
      )}
      <Ribbon
        activeScreen={screen}
        onNavigate={navigate}
        onOpenPalette={() => setShowPalette(true)}
      />
      <div className="workspace">
        <Sidebar activeScreen={screen} onNavigate={navigate} />

        {screen === "dashboard" && (
          <Dashboard
            activeCompany={activeCompany}
            companies={companies}
            invoices={window.DATA.INVOICES_OUT}
            onOpenInvoice={openInvoiceFn}
            onNavigate={navigate}
          />
        )}

        {screen === "facturi-emise" && (
          <FacturiEmise
            invoices={window.DATA.INVOICES_OUT}
            onOpenInvoice={openInvoiceFn}
            onNew={() => navigate("factura-noua")}
          />
        )}

        {screen === "facturi-primite" && (
          <FacturiPrimite invoices={window.DATA.INVOICES_IN} />
        )}

        {screen === "companii" && (
          <Companii
            companies={companies}
            activeCompanyId={activeCompanyId}
            onPick={setActiveCompanyId}
          />
        )}

        {screen === "factura-noua" && (
          <FacturaNoua
            activeCompany={activeCompany}
            onCancel={() => navigate("facturi-emise")}
            onSave={() => navigate("facturi-emise")}
          />
        )}

        {screen === "factura-detaliu" && (
          <FacturaDetaliu
            activeCompany={activeCompany}
            invoice={openInvoice || window.DATA.INVOICES_OUT[0]}
            onClose={() => navigate("facturi-emise")}
            onNavigate={navigate}
          />
        )}

        {/* fallback for sidebar items that don't have a screen built */}
        {!["dashboard","facturi-emise","facturi-primite","companii","factura-noua","factura-detaliu"].includes(screen) && (
          <UnbuiltStub screen={screen} onBack={() => navigate("dashboard")} />
        )}
      </div>
      <StatusBar activeCompany={activeCompany} companyCount={companies.length} />

      {showPalette && (
        <Palette
          onClose={() => setShowPalette(false)}
          onAction={(item) => {
            if (item.label.startsWith("Mergi la: Dashboard"))            navigate("dashboard");
            else if (item.label.startsWith("Mergi la: Facturi emise"))    navigate("facturi-emise");
            else if (item.label.startsWith("Mergi la: Facturi primite"))  navigate("facturi-primite");
            else if (item.label.startsWith("Mergi la: Companii"))         navigate("companii");
            else if (item.label === "Factură nouă")                       navigate("factura-noua");
            else if (item.label.startsWith("Schimbă"))                    setShowCompanySwitcher(true);
          }}
        />
      )}

      <TweaksPanel title="Tweaks">
        <TweakSection label="Densitate" />
        <TweakRadio  label="Spațiere rânduri"
                     value={t.density}
                     options={["compact", "cozy", "comfy"]}
                     onChange={(v) => setTweak("density", v)} />

        <TweakSection label="Temă" />
        <TweakColor  label="Accent"
                     value={t.accent}
                     options={["navy", "emerald", "burgundy", "teal", "graphite"]}
                     onChange={(v) => setTweak("accent", v)} />
        <TweakToggle label="Mod întunecat"
                     value={t.dark}
                     onChange={(v) => setTweak("dark", v)} />

        <TweakSection label="Limbă" />
        <TweakRadio  label="Limbă interfață"
                     value={t.lang}
                     options={["ro", "en"]}
                     onChange={(v) => setTweak("lang", v)} />

        <TweakSection label="Navigare rapidă" />
        <TweakButton label="Deschide paleta (Ctrl+K)" onClick={() => setShowPalette(true)} />
        <TweakButton label="Schimbă companie activă"  onClick={() => setShowCompanySwitcher(true)} />
      </TweaksPanel>
    </div>
  );
}

// Override TweakColor swatches — accent presets are names, not hex
// (we resolve to actual swatch hex inline via a thin wrapper)
const TweakColorOriginal = window.TweakColor;
window.TweakColor = function PatchedTweakColor({ label, value, options, onChange }) {
  // expand named accents to swatch hex strings before passing through
  if (options && options.every(o => typeof o === "string" && ACCENT_PRESETS[o])) {
    const hexOptions = options.map(o => ACCENT_PRESETS[o].swatch);
    const valueHex   = ACCENT_PRESETS[value]?.swatch;
    return (
      <TweakColorOriginal
        label={label}
        value={valueHex}
        options={hexOptions}
        onChange={(hex) => {
          const name = Object.keys(ACCENT_PRESETS).find(k => ACCENT_PRESETS[k].swatch === hex);
          if (name) onChange(name);
        }}
      />
    );
  }
  return <TweakColorOriginal label={label} value={value} options={options} onChange={onChange} />;
};

/* ----------------------------------------------------------------------
   Facturi emise — full list (used by sidebar "Facturi emise" item)
   ---------------------------------------------------------------------- */

function FacturiEmise({ invoices, onOpenInvoice, onNew }) {
  const [query, setQuery] = React.useState("");
  const [filter, setFilter] = React.useState("all");
  const [selected, setSelected] = React.useState(new Set());

  const q = query.trim().toLowerCase();
  const list = invoices
    .filter(i => !q || i.no.toLowerCase().includes(q) || i.client.name.toLowerCase().includes(q) || i.client.cui.toLowerCase().includes(q))
    .filter(i => filter === "all" ? true : i.status === filter);

  const counts = {
    all: invoices.length,
    validated: invoices.filter(i => i.status === "validated").length,
    submitted: invoices.filter(i => i.status === "submitted").length,
    rejected: invoices.filter(i => i.status === "rejected").length,
    draft: invoices.filter(i => i.status === "draft").length,
    pending: invoices.filter(i => i.status === "pending").length,
  };

  const toggleOne = (no) => {
    const next = new Set(selected);
    next.has(no) ? next.delete(no) : next.add(no);
    setSelected(next);
  };

  return (
    <div className="content">
      <div className="content-titlebar">
        <span className="content-title">
          <span className="crumb">e-Factura</span>
          Facturi emise
        </span>
        <span className="muted" style={{ fontSize: 11 }}>{list.length} din 1.247 facturi</span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
          <button className="btn"><Icon name="download" size={12} /> Export</button>
          <button className="btn"><Icon name="upload" size={12} /> Import XML</button>
          <button className="btn primary" onClick={onNew}>
            <Icon name="plus" size={12} /> Factură nouă
            <span className="kbd" style={{ marginLeft: 6, background: "rgba(255,255,255,0.18)", border: "1px solid rgba(255,255,255,0.3)", color: "#fff" }}>Ctrl N</span>
          </button>
        </span>
      </div>

      <div className="views-bar">
        {window.DATA.SAVED_VIEWS.map(v => (
          <span key={v.id} className={"view-tab " + (v.active ? "active" : "")}>
            {v.label} <span className="count">{v.count.toLocaleString("ro-RO")}</span>
          </span>
        ))}
        <span className="view-tab" style={{ color: "var(--accent)", borderRight: 0 }}>
          <Icon name="plus" size={11} /> Salvează vizualizarea
        </span>
      </div>

      <div className="content-toolbar">
        <div className="search">
          <Icon name="search" size={13} />
          <input placeholder="Caută după nr., CUI cumpărător sau denumire…" value={query} onChange={(e) => setQuery(e.target.value)} />
          <span className="kbd-hint">Ctrl F</span>
        </div>
        <span className="divider-v" style={{ margin: "0 4px" }} />
        <span style={{ fontSize: 11, color: "var(--text-muted)" }}>Status:</span>
        <div className="seg">
          <span className={"seg-item " + (filter === "all" ? "active" : "")}       onClick={() => setFilter("all")}>Toate</span>
          <span className={"seg-item " + (filter === "validated" ? "active" : "")} onClick={() => setFilter("validated")}>Validate</span>
          <span className={"seg-item " + (filter === "submitted" ? "active" : "")} onClick={() => setFilter("submitted")}>Trimise</span>
          <span className={"seg-item " + (filter === "rejected" ? "active" : "")}  onClick={() => setFilter("rejected")}>Respinse</span>
          <span className={"seg-item " + (filter === "draft" ? "active" : "")}     onClick={() => setFilter("draft")}>Schițe</span>
        </div>
        <span className="chip">Perioadă: Mai 2026 <Icon name="caret" size={10} /></span>
        <span className="chip">Cumpărător: oricine <Icon name="caret" size={10} /></span>
        <span style={{ marginLeft: "auto", display: "flex", gap: 6, alignItems: "center" }}>
          {selected.size > 0 && (
            <>
              <span style={{ fontSize: 11, fontWeight: 600 }}>{selected.size} selectate</span>
              <button className="btn compact primary"><Icon name="cloudUp" size={11} /> Trimite la ANAF</button>
              <button className="btn compact"><Icon name="download" size={11} /> Export XML</button>
              <button className="btn compact"><Icon name="printer" size={11} /> Tipărește</button>
              <span className="divider-v" style={{ margin: "0 4px" }} />
            </>
          )}
          <button className="btn-icon"><Icon name="filter" size={14} /></button>
          <button className="btn-icon"><Icon name="more" size={14} /></button>
        </span>
      </div>

      <div className="content-body">
        <table className="dt">
          <thead>
            <tr>
              <th className="ck">
                <input type="checkbox" className="cbx"
                       checked={selected.size === list.length && list.length > 0}
                       onChange={() => setSelected(selected.size === list.length ? new Set() : new Set(list.map(i => i.no)))} />
              </th>
              <th style={{ width: 134 }} className="sortable sorted">Nr. factură <span className="sort">▾</span></th>
              <th style={{ width: 92 }}>Data</th>
              <th>Cumpărător</th>
              <th style={{ width: 100 }}>CUI</th>
              <th className="num" style={{ width: 110 }}>Net (RON)</th>
              <th className="num" style={{ width: 90 }}>TVA</th>
              <th className="num" style={{ width: 120 }}>Total</th>
              <th style={{ width: 100 }}>Scadență</th>
              <th style={{ width: 124 }}>Status ANAF</th>
              <th style={{ width: 110 }}>Index ANAF</th>
              <th style={{ width: 24 }}></th>
            </tr>
          </thead>
          <tbody>
            {list.map(inv => (
              <tr key={inv.no}
                  onClick={() => onOpenInvoice(inv)}
                  className={selected.has(inv.no) ? "selected" : ""}
                  style={{ cursor: "pointer" }}>
                <td className="ck" onClick={(e) => e.stopPropagation()}>
                  <input type="checkbox" className="cbx" checked={selected.has(inv.no)} onChange={() => toggleOne(inv.no)} />
                </td>
                <td className="mono"><b>{inv.no}</b></td>
                <td className="muted">{inv.date}</td>
                <td>{inv.client.name}</td>
                <td className="mono muted">{inv.client.cui}</td>
                <td className="num tnum muted">{fmtRON(inv.net)}</td>
                <td className="num tnum dim">{fmtRON(inv.vat)}</td>
                <td className="num tnum"><b>{fmtRON(inv.total)}</b></td>
                <td className="muted">{inv.due}</td>
                <td><StatusBadge status={inv.status} /></td>
                <td className="mono dim">{inv.anafId || "—"}</td>
                <td><Icon name="chevronRight" size={12} style={{ color: "var(--text-dim)" }} /></td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <div style={{ padding: "6px 14px", borderTop: "1px solid var(--border)", background: "var(--bg)", display: "flex", gap: 16, fontSize: 11, color: "var(--text-muted)" }}>
        <span>Validate: <b style={{ color: "#16A34A" }}>{counts.validated}</b></span>
        <span>Trimise: <b style={{ color: "#1E40AF" }}>{counts.submitted}</b></span>
        <span>Respinse: <b style={{ color: "#DC2626" }}>{counts.rejected}</b></span>
        <span>Schițe: <b>{counts.draft}</b></span>
        <span style={{ marginLeft: "auto" }}>
          <span className="kbd">↑↓</span> selectează · <span className="kbd">Enter</span> deschide · <span className="kbd">Space</span> bifează
        </span>
      </div>
    </div>
  );
}

/* placeholder for sidebar items without a built screen */
function UnbuiltStub({ screen, onBack }) {
  const titles = {
    "spv": "Mesaje SPV",
    "stornate": "Facturi stornate",
    "contacte": "Clienți & Furnizori",
    "stocuri": "Articole & Stocuri",
    "banca": "Bancă & Casă",
    "rapoarte": "Rapoarte",
    "declaratii": "Declarații ANAF",
    "audit": "Jurnal de modificări",
  };
  return (
    <div className="content">
      <div className="content-titlebar">
        <span className="content-title">
          <span className="crumb">Efactura</span>
          {titles[screen] || screen}
        </span>
        <span style={{ marginLeft: "auto" }}>
          <button className="btn" onClick={onBack}><Icon name="chevronLeft" size={12} /> Înapoi la Dashboard</button>
        </span>
      </div>
      <div style={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "center", color: "var(--text-muted)", fontSize: 12, padding: 40, textAlign: "center" }}>
        <div>
          <div style={{ fontSize: 11, letterSpacing: 0.12, textTransform: "uppercase", color: "var(--text-dim)", marginBottom: 6 }}>Modul în dezvoltare</div>
          <div>
            Ecran demonstrativ — modulul <b>{titles[screen] || screen}</b> va folosi același sistem de tabele,<br />
            filtre salvate, acțiuni bulk și validare live ANAF ca celelalte module deja construite.
          </div>
        </div>
      </div>
    </div>
  );
}

window.FacturiEmise = FacturiEmise;
window.App = App;

ReactDOM.createRoot(document.getElementById("root")).render(<App />);
