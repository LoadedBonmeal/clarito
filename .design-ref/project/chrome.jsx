/* ----------------------------------------------------------------------
   Chrome: MenuBar, Ribbon, Sidebar, StatusBar, CompanySwitcher
   ---------------------------------------------------------------------- */

const { useState, useRef, useEffect } = React;

/* ============================================================
   MenuBar — Windows-style File / Edit / ... with dropdowns
   ============================================================ */

const MENUS = {
  "Fișier": [
    { type: "row", icon: "plus",      label: "Factură nouă",                kbd: "Ctrl+N" },
    { type: "row", icon: "invoiceIn", label: "Înregistrare factură primită", kbd: "Ctrl+Shift+N" },
    { type: "row", icon: "users",     label: "Contact nou (client/furnizor)", kbd: "Ctrl+Alt+C" },
    { type: "sep" },
    { type: "row", icon: "save",      label: "Salvează",                    kbd: "Ctrl+S" },
    { type: "row", icon: "copy",      label: "Salvează ca…",                kbd: "Ctrl+Shift+S" },
    { type: "sep" },
    { type: "section", label: "Import / Export" },
    { type: "row", icon: "upload",    label: "Importă XML e-Factura…",      kbd: "" },
    { type: "row", icon: "download",  label: "Exportă SAF-T (D406)…",       kbd: "" },
    { type: "row", icon: "printer",   label: "Tipărește factura curentă",   kbd: "Ctrl+P" },
    { type: "sep" },
    { type: "row", icon: "x",         label: "Ieșire",                      kbd: "Alt+F4" },
  ],
  "Editare": [
    { type: "row", icon: "pen",       label: "Anulează",                    kbd: "Ctrl+Z" },
    { type: "row", icon: "pen",       label: "Refă",                        kbd: "Ctrl+Y" },
    { type: "sep" },
    { type: "row", icon: "copy",      label: "Decupează",                   kbd: "Ctrl+X" },
    { type: "row", icon: "copy",      label: "Copiază",                     kbd: "Ctrl+C" },
    { type: "row", icon: "copy",      label: "Lipește",                     kbd: "Ctrl+V" },
    { type: "sep" },
    { type: "row", icon: "search",    label: "Caută…",                      kbd: "Ctrl+F" },
    { type: "row", icon: "command",   label: "Paleta de comenzi",           kbd: "Ctrl+K" },
  ],
  "Operațiuni": [
    { type: "section", label: "e-Factura" },
    { type: "row", icon: "cloudUp",   label: "Trimite factura la ANAF",     kbd: "F9" },
    { type: "row", icon: "refresh",   label: "Verifică status mesaje",      kbd: "F10" },
    { type: "row", icon: "storno",    label: "Storno factură",              kbd: "Ctrl+F9" },
    { type: "sep" },
    { type: "section", label: "Bancă & casă" },
    { type: "row", icon: "bank",      label: "Punctare extras bancar",      kbd: "" },
    { type: "row", icon: "receipt",   label: "Înregistrare chitanță",       kbd: "" },
    { type: "sep" },
    { type: "section", label: "Bulk" },
    { type: "row", icon: "check",     label: "Trimite selecția la ANAF",    kbd: "" },
    { type: "row", icon: "tag",       label: "Aplică categorie pe selecție", kbd: "" },
  ],
  "Date": [
    { type: "row", icon: "buildings", label: "Companii administrate",       kbd: "G C" },
    { type: "row", icon: "users",     label: "Clienți",                     kbd: "" },
    { type: "row", icon: "users",     label: "Furnizori",                   kbd: "" },
    { type: "row", icon: "stock",     label: "Articole / Stocuri",          kbd: "" },
    { type: "sep" },
    { type: "row", icon: "database",  label: "Plan de conturi",             kbd: "" },
    { type: "row", icon: "tag",       label: "Cote TVA și taxe",            kbd: "" },
    { type: "row", icon: "history",   label: "Audit & jurnal modificări",   kbd: "" },
  ],
  "Rapoarte": [
    { type: "section", label: "Declarații ANAF" },
    { type: "row", icon: "reports",   label: "D300 — Decont TVA",           kbd: "" },
    { type: "row", icon: "reports",   label: "D394 — Livrări/Achiziții",     kbd: "" },
    { type: "row", icon: "reports",   label: "D406 — SAF-T",                kbd: "" },
    { type: "sep" },
    { type: "section", label: "Operative" },
    { type: "row", icon: "reports",   label: "Jurnal de vânzări",           kbd: "" },
    { type: "row", icon: "reports",   label: "Jurnal de cumpărări",         kbd: "" },
    { type: "row", icon: "reports",   label: "Cartea mare",                 kbd: "" },
    { type: "row", icon: "reports",   label: "Balanță de verificare",       kbd: "" },
  ],
  "Vizualizare": [
    { type: "row", icon: "view",      label: "Reîncarcă datele",            kbd: "F5" },
    { type: "row", icon: "view",      label: "Mărește densitatea (compact)", kbd: "Ctrl+−" },
    { type: "row", icon: "view",      label: "Micșorează densitatea",       kbd: "Ctrl+=" },
    { type: "sep" },
    { type: "row", icon: "view",      label: "Mod întunecat",               kbd: "Ctrl+Shift+D" },
    { type: "row", icon: "view",      label: "Arată coloane ascunse…",      kbd: "" },
  ],
  "Ajutor": [
    { type: "row", icon: "help",      label: "Documentație e-Factura",      kbd: "F1" },
    { type: "row", icon: "keyboard",  label: "Scurtături tastatură",        kbd: "Ctrl+/" },
    { type: "sep" },
    { type: "row", icon: "info",      label: "Despre Efactura • v0.1.0",    kbd: "" },
  ],
};

const MenuBar = ({ activeCompany, onOpenCompanySwitcher, anafStatus = "ok" }) => {
  const [open, setOpen] = useState(null);
  const ref = useRef(null);

  useEffect(() => {
    const onDoc = (e) => { if (ref.current && !ref.current.contains(e.target)) setOpen(null); };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, []);

  return (
    <div className="menubar" ref={ref}>
      <div className="menubar-brand">
        <span className="menubar-brand-mark">eF</span>
        <span>Efactura</span>
      </div>
      {Object.keys(MENUS).map((name) => (
        <div
          key={name}
          className={"menubar-item" + (open === name ? " open" : "")}
          onMouseDown={(e) => { e.preventDefault(); setOpen(open === name ? null : name); }}
          onMouseEnter={() => { if (open) setOpen(name); }}
        >
          <u>{name[0]}</u>{name.slice(1)}
          {open === name && (
            <div className="menu-dropdown" onMouseDown={(e) => e.stopPropagation()}>
              {MENUS[name].map((row, i) => {
                if (row.type === "sep")     return <div key={i} className="menu-sep" />;
                if (row.type === "section") return <div key={i} className="menu-section">{row.label}</div>;
                return (
                  <div key={i} className="menu-row">
                    <span className="menu-icon"><Icon name={row.icon} size={13} /></span>
                    <span>{row.label}</span>
                    <span className="menu-kbd">{row.kbd}</span>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      ))}
      <div className="menubar-spacer" />
      <span className="menubar-anaf" title="Status conexiune ANAF / SPV">
        <span className="pulse-dot" />
        ANAF · SPV {anafStatus === "ok" ? "conectat" : anafStatus.toUpperCase()}
      </span>
      <button className="menubar-company" onClick={onOpenCompanySwitcher} title="Schimbă compania activă (Ctrl+K Ctrl+C)">
        <span className="swatch" style={{ background: activeCompany.color }} />
        <span style={{ fontWeight: 600 }}>{activeCompany.name}</span>
        <span className="cui">· {activeCompany.cui}</span>
        <Icon name="caret" size={11} />
      </button>
    </div>
  );
};

/* ============================================================
   Ribbon — grouped icon+label toolbar
   ============================================================ */

const Ribbon = ({ activeScreen, onNavigate, onOpenPalette }) => {
  const BtnBig = ({ icon, label, primary, active, onClick, hint }) => (
    <button className={"ribbon-btn" + (primary ? " primary" : "") + (active ? " active" : "")} onClick={onClick} title={hint}>
      <span className="ico"><Icon name={icon} size={22} /></span>
      <span className="lbl">{label}</span>
      {hint && <span className="caret"><Icon name="caret" size={8} /></span>}
    </button>
  );

  return (
    <div className="ribbon">
      {/* OPERAȚIUNI */}
      <div className="ribbon-group">
        <div className="ribbon-group-buttons">
          <BtnBig icon="plus"      label="Factură nouă"    primary onClick={() => onNavigate("factura-noua")} hint="Ctrl+N" />
          <BtnBig icon="invoiceIn" label="Primită nouă"            hint="Ctrl+Shift+N" />
          <BtnBig icon="storno"    label="Storno"                  hint="Ctrl+F9" />
          <BtnBig icon="receipt"   label="Chitanță" />
          <BtnBig icon="bank"      label="Plată" />
          <BtnBig icon="users"     label="Contact" />
        </div>
        <div className="ribbon-group-label">Operațiuni</div>
      </div>

      {/* SINCRONIZARE */}
      <div className="ribbon-group">
        <div className="ribbon-group-buttons">
          <BtnBig icon="cloudUp"  label="Trimite ANAF"  hint="F9" />
          <BtnBig icon="cloudDn"  label="Descarcă SPV"  hint="Ctrl+D" />
          <BtnBig icon="refresh"  label="Verifică status" hint="F10" />
          <BtnBig icon="anaf"     label="Mesaje SPV" />
          <BtnBig icon="download" label="Export XML" />
          <BtnBig icon="upload"   label="Import XML" />
        </div>
        <div className="ribbon-group-label">Sincronizare ANAF</div>
      </div>

      {/* DATE */}
      <div className="ribbon-group">
        <div className="ribbon-group-buttons">
          <BtnBig icon="buildings" label="Companii"
                  active={activeScreen === "companii"}
                  onClick={() => onNavigate("companii")} />
          <BtnBig icon="users"     label="Contacte" />
          <BtnBig icon="stock"     label="Articole" />
          <BtnBig icon="database"  label="Plan conturi" />
          <BtnBig icon="tag"       label="Cote TVA" />
          <BtnBig icon="history"   label="Audit log" />
        </div>
        <div className="ribbon-group-label">Date</div>
      </div>

      {/* RAPOARTE */}
      <div className="ribbon-group">
        <div className="ribbon-group-buttons">
          <BtnBig icon="reports" label="D300 TVA" />
          <BtnBig icon="reports" label="D394"     />
          <BtnBig icon="reports" label="D406 SAF-T" />
          <BtnBig icon="reports" label="Jurnal vânzări" />
          <BtnBig icon="reports" label="Jurnal cumpărări" />
          <BtnBig icon="reports" label="Balanță" />
        </div>
        <div className="ribbon-group-label">Rapoarte & Declarații</div>
      </div>

      {/* INSTRUMENTE */}
      <div className="ribbon-group" style={{ flex: 1 }}>
        <div className="ribbon-group-buttons" style={{ justifyContent: "flex-start" }}>
          <BtnBig icon="command"  label="Comenzi" onClick={onOpenPalette} hint="Ctrl+K" />
          <BtnBig icon="keyboard" label="Scurtături" hint="Ctrl+/" />
          <BtnBig icon="settings" label="Setări" />
        </div>
        <div className="ribbon-group-label">Instrumente</div>
      </div>
    </div>
  );
};

/* ============================================================
   Sidebar — flat module list with color bars
   ============================================================ */

const SIDEBAR_MODULES = [
  { section: "Tablou de bord" },
  { id: "dashboard",       label: "Privire generală", ico: "data",       color: "#2848A1", badge: null },
  { section: "e-Factura" },
  { id: "facturi-emise",   label: "Facturi emise",     ico: "invoice",    color: "var(--color-facturi)", badge: 1247 },
  { id: "facturi-primite", label: "Facturi primite",   ico: "invoiceIn",  color: "var(--color-primite)", badge: 12 },
  { id: "spv",             label: "Mesaje SPV",        ico: "anaf",       color: "var(--color-primite)", badge: 3 },
  { id: "stornate",        label: "Stornate",          ico: "storno",     color: "var(--color-rapoarte)", badge: null },
  { section: "Operativ" },
  { id: "companii",        label: "Companii",          ico: "buildings",  color: "var(--color-companii)", badge: 15 },
  { id: "contacte",        label: "Contacte",          ico: "users",      color: "var(--color-contacte)", badge: 487 },
  { id: "stocuri",         label: "Articole & Stocuri", ico: "stock",     color: "var(--color-stocuri)", badge: null },
  { id: "banca",           label: "Bancă & Casă",      ico: "bank",       color: "var(--color-banca)",   badge: null },
  { section: "Raportare" },
  { id: "rapoarte",        label: "Rapoarte",          ico: "reports",    color: "var(--color-rapoarte)", badge: null },
  { id: "declaratii",      label: "Declarații ANAF",   ico: "anaf",       color: "var(--color-rapoarte)", badge: 2 },
  { id: "audit",           label: "Jurnal modificări", ico: "history",    color: "#8A857A", badge: null },
];

const Sidebar = ({ activeScreen, onNavigate }) => {
  const screenMap = {
    "dashboard": "dashboard",
    "facturi-emise": "facturi-emise",
    "facturi-noua": "facturi-emise",
    "factura-noua": "facturi-emise",
    "factura-detaliu": "facturi-emise",
    "facturi-primite": "facturi-primite",
    "companii": "companii",
  };
  const activeKey = screenMap[activeScreen] || activeScreen;

  return (
    <div className="sidebar">
      {SIDEBAR_MODULES.map((m, i) => {
        if (m.section) return <div key={"s" + i} className="sidebar-section">{m.section}</div>;
        const isActive = activeKey === m.id;
        const screenTarget = m.id === "facturi-emise" ? "facturi-emise" : m.id;
        return (
          <div
            key={m.id}
            className={"sidebar-item" + (isActive ? " active" : "")}
            style={{ "--module-color": m.color }}
            onClick={() => onNavigate(screenTarget)}
          >
            <span className="bar" />
            <span className="ico"><Icon name={m.ico} size={15} /></span>
            <span>{m.label}</span>
            {m.badge != null && <span className="badge">{m.badge}</span>}
          </div>
        );
      })}
      <div className="sidebar-footer">
        <Icon name="user" size={14} />
        <span style={{ flex: 1 }}>D. Popescu</span>
        <span className="kbd">Ctrl K</span>
      </div>
    </div>
  );
};

/* ============================================================
   StatusBar — informative, pulsating ANAF live dot
   ============================================================ */

const StatusBar = ({ activeCompany, companyCount = 15 }) => (
  <div className="statusbar">
    <span className="statusbar-chip">
      <span className="pulse-dot" />
      <span><b>ANAF · SPV</b> conectat</span>
    </span>
    <span className="statusbar-chip">
      <Icon name="clock" size={12} />
      <span className="label-dim">Ultima sincronizare</span>
      <b>12:34:09</b>
      <span className="label-dim">· acum 14s</span>
    </span>
    <span className="statusbar-chip">
      <Icon name="invoice" size={12} />
      <span className="label-dim">Astăzi</span>
      <b>14 facturi</b>
      <span className="label-dim">· {`118.420,80 RON`}</span>
    </span>
    <span className="statusbar-chip">
      <Icon name="cloudDn" size={12} />
      <span className="label-dim">Mesaje SPV</span>
      <b>3 noi</b>
    </span>
    <span className="statusbar-chip">
      <span style={{ width: 8, height: 8, background: activeCompany.color, display: "inline-block", borderRadius: 2 }} />
      <span className="label-dim">Companie activă</span>
      <b>{activeCompany.name}</b>
      <span className="mono label-dim" style={{ fontSize: 10.5 }}>· {activeCompany.cui}</span>
    </span>
    <span className="statusbar-spacer" />
    <span className="statusbar-chip"><span className="label-dim">{companyCount} companii administrate</span></span>
    <span className="statusbar-chip"><span className="label-dim">RO_CIUS 1.0.1 · RON · ro-RO</span></span>
    <span className="statusbar-chip"><span className="label-dim">v0.1.0</span></span>
  </div>
);

/* ============================================================
   Company switcher popover
   ============================================================ */

const CompanySwitcher = ({ companies, activeId, onPick, onClose }) => {
  const ref = useRef(null);
  const [query, setQuery] = useState("");
  useEffect(() => {
    const onDoc = (e) => { if (ref.current && !ref.current.contains(e.target)) onClose(); };
    const onEsc = (e) => { if (e.key === "Escape") onClose(); };
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onEsc);
    return () => { document.removeEventListener("mousedown", onDoc); document.removeEventListener("keydown", onEsc); };
  }, []);
  const q = query.trim().toLowerCase();
  const filtered = companies.filter(c =>
    !q || c.name.toLowerCase().includes(q) || c.cui.toLowerCase().includes(q) || c.city.toLowerCase().includes(q)
  );
  return (
    <div className="popover" ref={ref}>
      <div className="popover-search">
        <div className="search">
          <Icon name="search" size={13} />
          <input autoFocus placeholder="Caută după nume, CUI, oraș…" value={query} onChange={(e) => setQuery(e.target.value)} />
          <span className="kbd-hint">ESC</span>
        </div>
      </div>
      <div className="popover-list">
        <div className="palette-section">Companii administrate · {filtered.length}/{companies.length}</div>
        {filtered.map(c => (
          <div key={c.id}
               className={"popover-row" + (c.id === activeId ? " current" : "")}
               onClick={() => { onPick(c.id); onClose(); }}>
            <span style={{ width: 10, height: 10, background: c.color, display: "inline-block" }} />
            <span>
              <div style={{ fontWeight: 600 }}>{c.name}</div>
              <div className="sub">{c.city} · {c.county} · {c.regCom}</div>
            </span>
            <span className="cui">{c.cui}</span>
          </div>
        ))}
      </div>
      <div className="palette-footer">
        <span><span className="kbd">↑</span> <span className="kbd">↓</span> navigare</span>
        <span><span className="kbd">Enter</span> selectează</span>
        <span><span className="kbd">Ctrl K Ctrl C</span> deschide din orice ecran</span>
      </div>
    </div>
  );
};

Object.assign(window, { MenuBar, Ribbon, Sidebar, StatusBar, CompanySwitcher });
