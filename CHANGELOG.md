# Changelog

Toate modificările notabile ale Clarito (fost RoFactura). Format: [Keep a Changelog](https://keepachangelog.com), versionare [SemVer](https://semver.org).

## [0.5.0] - 2026-06-02

Audit complet pe funcționalități contabile + remediere bug-uri raportate (plan-mode + sub-agenți + QA + verificare adversarială; gate verde: 338 teste cargo + 129 teste vitest + clippy `-D warnings`).

### Fixed
- **Izolare date multi-companie (P0)**: când nicio companie nu e activă, paginile Facturi emise/primite, Contacte, Rapoarte și Dashboard nu mai afișează datele tuturor firmelor amestecate (gardă în UI + backend respinge `company_id` nul).
- **Reconciliere stornare (P0)**: o factură stornată nu mai dispare din D300/D394/SAF-T — originalul rămâne contabilizat în perioada lui, iar nota de credit (negativă) compensează în perioada ei; jurnalul de vânzări nu mai dublează; după stornare ești dus la nota de credit cu banner „trimite la ANAF".
- **TVA pe categorie autoritar (P0)**: liniile cu taxare inversă/scutit (AE/E/Z/O/K/G) au acum TVA 0 și cotă 0 (e-factură validă) — înainte aplicau 19/21% cu cod de scutire contradictoriu.
- **Trunchiere liste (P0)**: listele se încarcă complet (nu doar 50 de rânduri), cu avertisment când sunt mai multe; totalurile din subsol nu mai adună RON+EUR la un loc.
- **Chitanțe**: numerotare per-serie (fără goluri ilegale), validare sumă pozitivă + plătitor obligatoriu, selector de factură (nu UUID), PDF cu numărul real al facturii + separator zecimal ro-RO + diacritice.
- **Declarații ANAF**: SAF-T D406 exportă luna selectată; D394 grupează pe (partener, categorie TVA) cu conversie valutară deterministă; codurile TVA derivă din categorie, nu din cotă (cota redusă 5/9/11% → „AA").
- **Validare catalog**: cifră de control CUI (mod-11), prevenire duplicate CUI contact/cod produs, validare IBAN (mod-97); o companie ștearsă (soft-delete) poate fi re-adăugată (reactivare).
- **e-Factura/backup/licență**: crash la verificarea integrității arhivei remediat; „Export selecție" (facturi primite) și butonul „Adaugă plată" sunt acum funcționale; reîncercare la token expirat (401) pe verificarea stării; backup consistent (VACUUM INTO) + alegere locație; email-ul de probă normalizat (case).
- **Densitate rânduri** funcțională în toată aplicația, inclusiv tabelele virtualizate (Facturi emise) și Stornate.
- Bug-uri raportate: crash „Rendered fewer hooks" la ștergerea ultimei companii pe plan Solo; clase CSS nedefinite (spațierea din detaliile companiei); meniul de rând tăiat pe rândurile de jos; spațiere consistentă între pagini; animații one-shot pe iconițe la apăsare (afișate din prima apăsare).

### Security
- Întărire flux OAuth pe callback-ul loopback (validare cale `/callback` + `state` CSRF; portul se eliberează la timeout); închidere injecție în URL-ul SQLite la import backup; encodare procentuală a parametrilor ANAF; permisiuni `0600` pe baza de date (PII).

### Changed
- Exporturile CSV au BOM UTF-8 (diacritice corecte în Excel); exportul SAGA/WinMentor folosește moneda + cursul real al facturii; datele folosesc fusul local (EET) pentru scadență/perioadă/recurente; cod mort eliminat (animații icon mutate pe Web Animations API).

## [0.4.0] - 2026-06-02

### Changed
- **Rebranding „Clarito"**: aplicația a fost redenumită din RoFactura în Clarito (nume afișat, titlul ferestrei, installer, marca logo). Identificatorii tehnici (bundle id `com.lucaris.efactura`, folderul de date, cheile de licență, keychain) rămân neschimbați — fără pierdere de date sau licențe la actualizare.
- **Reproiectare UI completă** — interfață modernă în stil fintech-SaaS (sidebar alb + accent indigo + spațiere aerisită), care înlocuiește shell-ul stil Windows (ribbon + bară de meniu) cu: bară laterală grupată + bară de sus subțire (căutare globală ⌘K, „+ Nou", status ANAF·SPV + Sincronizează, notificări, profil) + paletă de comenzi (⌘K). Set nou de componente („rf"), mod întunecat + densitate reglabilă (Compact/Confortabil/Lejer), animații fluide discrete (cu suport `prefers-reduced-motion`), layout responsiv la redimensionarea ferestrei.
- Pagini aliniate la designul de referință: **Facturi emise** (toolbar curat — filtru status, perioadă, „Filtre", meniuri Export/Import, acțiuni la hover), **pagină dedicată Stornate**, coloane **Net/TVA** pe Facturi primite, listă curată **Mesaje SPV**, comutator de companie rotunjit, sidebar comprimat centrat.
- Toate cele 124+ funcționalități backend rămân conectate la noul UI (nicio funcție pierdută — exporturi, filtre, bulk, acțiuni de rând relocate, nu eliminate).

### Fixed
- Audit complet (securitate/bugs/cod, plan-mode + sub-agenți + QA): curățare listener „click-outside" pe meniul de rând (fără listeneri reziduali), filtru status „În coadă" (QUEUED) adăugat în lista de facturi, gardă „companie activă" pe Facturi primite (updateStatus/reparseVat) și pe importul CSV, encodare `mailto`, eliminare indiciu de tastatură înșelător.
- Cod curat: eliminare cod mort (icoană/CSS/variabile nefolosite după reproiectare), culori hardcodate înlocuite cu tokeni de temă (light + dark).

### Security
- Backend Rust neatins funcțional (doar redenumiri de text afișat în tray/email feedback/anteturi export/EULA); **salt-ul de licență + bundle identifier rămân intacte** (licențele și datele existente continuă să funcționeze); CSP strict, fără CDN extern; izolarea pe companie verificată pe toate paginile. Auditul nu a găsit probleme de securitate.

## [0.3.1] - 2026-05-31

### Added
- **Multi-monedă (FX)**: facturi non-RON funcționale — câmp curs valutar + buton „Preia curs BNR" (curs oficial BNR), UBL compliant EN16931 (emite `TaxCurrencyCode` + TVA în RON), normalizare RON în D300/D394/SAF-T/raport TVA, parsare curs din facturile primite, coloană Moneda în jurnalul de vânzări
- **Module noi**: Articole/Stocuri (catalog produse + selector în liniile de factură pentru facturare rapidă/reutilizabilă), Cote TVA editabile (catalog din DB ce alimentează dropdown-ul TVA), Chitanțe (document încasare numerar + PDF cu suma în litere, numerotare per-companie), Plan de conturi (catalog conturi + seed plan RO standard); butonul „Plată" conectat la pagina de plăți — toate cele 5 item-uri de meniu moarte sunt acum funcționale
- Rapoarte ca view-uri distincte (`/reports?view=`): D394, D406 SAF-T, jurnal vânzări, jurnal cumpărări, export contabil + bară de tab-uri
- Parsare TVA din XML-ul facturilor primite (net/TVA pe cotă) + backfill „Recalculează TVA din XML"
- D300 + D394 partea de achiziții reală din facturile primite (înlocuiește placeholder-ele)
- Jurnal cumpărări cu coloane reale Net/TVA

### Security
- **Izolare multi-companie completă**: TOATE comenzile care citesc/scriu/generează date de companie sunt scope pe `company_id` — facturi (get/update draft/storno/duplicate/delete/status/validate), generare UBL XML/PDF, push SmartBill, submit ANAF (claim DRAFT→QUEUED scope), contacts (inclusiv get_contact), recurring, received
- Hardening audit R16: neutralizare CSV formula-injection în export-uri; GDPR șterge și PDF-urile de chitanță; CAS pe schimbarea statusului facturii (anti-TOCTOU); anti-dublă-depunere ANAF la eșec post-upload
- GDPR: ștergerea totală șterge token-urile ANAF + SmartBill din keychain + `data.db.bak`/backup + log-urile aplicației
- SmartBill: token stocat în keychain (nu în DB plaintext) + curățare token vechi
- Secrete HMAC build.rs din variabile de mediu (fallback identic → licențe valide); fingerprint licență aplicat la toate tier-urile
- Eliminat capability `http:default` nefolosit; `redirect_uri` OAuth validat ca loopback; refresh token single-flight (fără race)

### Fixed
- Export contabil (SAGA/WinMentor) + SAF-T D406: doar facturi VALIDATED (fără DRAFT/REJECTED/STORNED)
- SAF-T: tip 381 pentru storno (credit note); cotele 9%/11% mapate ca redus (nu standard)
- UBL: categoria `Z` (cotă zero) nu mai emite cod VATEX de export; BR-RO-017 (prefix RO) se aplică doar cumpărătorilor RO (facturile către clienți UE nu mai sunt blocate)
- Deducere categorie TVA: rezolvă țara cumpărătorului întâi; neplătitor intern → `O` (nu `AE`)
- ANAF OAuth: `client_id` configurat folosit la refresh/revoke (inclusiv task-uri background) + callback percent-decode
- Arhivă: „mută arhiva" funcțională (cheia de setări corectă); `import_backup` rescrie căile XML la restore cross-machine
- Rapoarte: export permis pentru perioade doar-achiziții (D300/D394); statistici + guard SAF-T pe VALIDATED; QueryErrorBanner pe erori
- PDF facturi: paginare pentru facturi cu multe linii (inclusiv footer-ul TVA/totaluri)
- Erori arhivă/backup cu mesaj RO generic (fără scurgere de căi); `AppErrorPayload` aliniat 1:1 cu Rust
- Stări de eroare (QueryErrorBanner) pe paginile Facturi primite + Plăți
- Etichete scurtături native macOS/Windows în tooltip-uri (fmtShortcut); Storno din meniu funcțional; Ctrl+S pe editarea facturii
- Recuperare la mutex de log otrăvit (fără crash); mesaj path log specific platformei
- Audit R16 (fiscal/UBL): CountrySubentity emis ca cod ISO 3166-2:RO (RO-CJ etc.); validare cod mod de plată (UNCL4461); cantitate stocată la 6 zecimale; jurnal cumpărări exclude facturile RESPINSE; rotunjire per-linie în UI = backend; cotele dezactivate rămân vizibile la editarea facturilor vechi
- Audit R16 (robustețe): factură primită cu monedă non-RON parsată corect; extragere ZIP rezilientă; cap la paginarea SPV; import CSV nu mai contopește clienții fără CUI; recurring nu mai creează facturi goale; export D300 respectă TVA deductibilă introdusă manual
- Audit R16 (UX): guard anti-dublu-submit (Enter/Ctrl+S); ecran „factura nu a fost găsită"; tab „Active" funcțional în Articole/Plan conturi; id-uri ARIA unice pe combobox-uri

## [0.3.0] - 2026-05-31

### Added
- Declarația D300 (decont TVA, partea de vânzări) + pagină Declarații
- Editor șabloane recurente: creare + editare + „Salvează ca șablon" din factură
- Dialog scurtături tastatură (Ctrl+/) cu etichete native macOS/Windows
- GDPR: export complet date + ștergere totală (Setări → Confidențialitate)
- license-gen CLI (workspace crate separat) pentru emiterea cheilor de licență
- Configurare ANAF avansată (client_id, redirect, port, URL-uri) + mediu de test
- Coloană vatCategory în editor linii factură + LineItemsEditor wired în Recurring/InvoiceEdit
- Backup complet (inclusiv fișiere arhivă) + cap recurring loop
- Trial status surface + căutare în company switcher din sidebar
- Dashboard redesenat: segmente perioadă, acțiune Corectează, etichete panel
- Suport și feedback: secțiune dedicată + wiring mailto diagnostic gather
- Teste de integrare backend (migrații + contract schemă) și teste unitare frontend

### Fixed
- Feedback + Cumpără licență: deschidere corectă client email / browser (openUrl)
- ANAF SPV: mediu de test OAuth + configurare avansată + mesaje de eroare clare
- Rapoarte: export robust la runtime + reîmprospătare vizibilă pe dashboard
- Șabloane: preserve vatCategory la „Salvează ca șablon" (defect QA#1)
- CSV import transacțional + mesaj clar la licență expirată la activare
- Modals Storno + CsvImport + company switcher wrapped în Radix Dialog (a11y)
- Bundler: license-gen extras în workspace crate propriu (fix CI real)
- Bundler: set default-run pentru ca Tauri să bundleze efactura-desktop, nu license-gen

### Security
- Eliminat tauri-plugin-sql; fs scoped la directoarele app/user (SEC-R7-01/02)
- HMAC-SHA256 (RFC 2104) pentru cheile de licență + hardening cale import
- async-FS în comenzi (non-blocking I/O)

---

## [0.2.0] - 2026-05-30

### Added
- Selectare TVA (vatCategory) + clienți UE (câmp țară/monedă) în factură
- Single-instance focus (cross-platform) — o singură instanță a aplicației
- Anti-rollback: toleranță drift ceas ±30 zile pentru cheile de licență
- license-gen binar CLI (primul draft, ulterior mutat în workspace crate)
- Workflow CI: build NSIS-only pe Windows, workflow_dispatch, ubuntu diagnostic
- README: secțiuni Support, Cumpărare licență (Stripe), Troubleshooting

### Fixed
- Storno: atomic submit/storno claims + guards + migrație 0011
- Storno: storno_of_invoice_id FK + validare dată chrono + eliminat heuristica series='S'
- Import CSV: câmpuri cu ghilimele (crate csv în loc de split(';'))
- Formatters: formatOptionalRon folosește parseDec pentru consistență
- Programare DST-safe + audit log error logging + propagare coloane SAF-T
- Dashboard: redesenat conform Claude Design (segmente, Corectează, etichete)

### Changed
- Refactorizare background: mod.rs de 1242 linii împărțit în submodule focusate
- Centralizare query keys în factory queryKeys
- Politică CI: format/clippy/test/typecheck înainte de build

### Security
- Obfuscare secrete licență via build.rs XOR cycle (SEC-05)
- Hostname real via OS API + checksum 32-bit (SEC-09/10)
- Validare integritate DB backup înainte de restore
- Narrowing Tauri capabilities — eliminat process:default
- Redactare body răspuns ANAF din loguri, OsRng pentru PKCE

---

## [0.1.0] - 2026-05 (versiune inițială)

### Added
- Versiune inițială — aplicație desktop Tauri pentru e-Factura ANAF
- Multi-companie, validare RO_CIUS, arhivare locală
- Facturare, storno, import/export XML ANAF, trimitere SPV
- Plăți, facturi recurente, export SAF-T D406
- Design system v2, ribbon, meniu aplicație, shortcut-uri OS-aware
- Branding complet: icon, DMG, NSIS installer cu EULA română
- Sistem licențiere (TRIAL/SOLO) cu fingerprint hardware
- Securitate: CSP strict, FS scope, pipeline CI/CD
