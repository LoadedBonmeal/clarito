# Changelog

Toate modificările notabile ale Clarito (fost RoFactura). Format: [Keep a Changelog](https://keepachangelog.com), versionare [SemVer](https://semver.org).

## [0.7.5] - 2026-07-02

### Corectat
- e-Factura: facturile intracomunitare (K) emit din nou codul de TVA al cumpărătorului (BT-48, regresie v0.7.4); preflight pentru adresa cumpărătorului (BR-RO-080/090/110); categoriile deduse respectă întâi statutul de neplătitor (O); prefixe de țară normalizate la majuscule; județele Dâmbovița/Vâlcea recunoscute corect.
- e-Transport: câmpurile obligatorii (stradă, organizator) în UI + validare locală; atributele goale nu se mai emit (XSD).
- TVA: decontul trimestrial/semestrial/anual exportă întreaga perioadă; rd.28 populat; fereastra memoriului TVA la încasare ancorată pe factura sursă.
- Salarizare: impozitul pe indemnizația de concediu medical se declară corect și cu diurnă în exces; suma netaxabilă condiționată de salariul de bază egal cu minimul; plafonul deducerii include venitul asimilat.
- Contabilitate: perimetrul de imutabilitate complet — stornare bon fiscal, creare/ștergere plăți, ștergeri (accruals/provizioane/bunuri de capital/dividende/avansuri/instrumente de plată/note manuale) refuză lunile blocate; plățile pe facturi respinse nu se mai postează.
- Importuri: extrasele MT940 în RON se importă corect; totalurile WinMentor pe facturile primite; deduplicare pe furnizor; moneda contului bancar respectată; potrivirea referințelor pe număr întreg.
- Dividende: scutirea art. 43 alin. (4) pentru PJ rezidente (participație ≥10%, ≥1 an); rând informativ cod 631 pentru nerezidenți.
- Securitate: restaurarea din backup limitată la roluri cu drept de ștergere; escapare HTML în printuri.
- Interfață: vizualizatorul PDF lizibil pe tema întunecată; 5 pagini traduse complet în engleză; gestionarea focusului în dialoguri; facturile recurente respectă blocarea cotelor TVA expirate; fereastra se restaurează din Dock (macOS); iconiță de tray vizibilă pe Windows.

## [0.7.4] - 2026-07-02

Audit final de publicare (11 dimensiuni): remedierea celor două blocante P0 (e-Factura pentru neplătitori de TVA + OAuth pe Windows) plus corecții fiscale D300/SAF-T și completarea blocajelor de perioadă.

### Fixed
- **e-Factura neplătitori de TVA (P0)**: facturile cu categoria O nu mai sunt respinse fatal de ANAF (BR-CO-09/BR-O); preflight BR-IC-02 blochează liniile K emise de vânzători neplătitori; sectoarele Bucureștiului și informațiile de livrare pentru K corectate.
- **Windows (P0)**: URL-ul OAuth nu mai este spart de `cmd` la deschiderea browserului; token-urile ANAF nu mai sunt trunchiate de plafonul de 2560 bytes al Credential Manager (chunking); salvare CSV prin dialog nativ.
- **Fiscal**: remapare rânduri de vânzări D300 pentru categoriile Z/K/E/G + plafonul „suma netaxabilă"; SAF-T Payments cu FX corect; termenul dividendelor în D100; semnele închiderii de TVA.
- **Bancă**: sugestiile de potrivire pentru tranzacțiile de ieșire (plăți către furnizori) nu funcționaseră niciodată (referință la o coloană inexistentă) — acum funcționale; potrivire conștientă de monedă.
- **Blocaj de perioadă**: garduri pe ștergeri/dez-potriviri + blocarea închiderii anuale + exportul D406 pe perioade blocate + RBAC pe exportatoare.
- **UI**: temă întunecată în vizualizatorul XML, înălțimea modalelor, contrast + a11y pe toggle-uri, plurale românești, selector SAGA robust.

### Changed
- Curățenie de cod (Wave 6): reconstruit harness-ul de teste DUK, cod mort eliminat, PeriodLocksPanel conectat, clippy strict pe tot crate-ul; limitări cunoscute documentate (D100/D101 UI-wiring + 8 constatări amânate verificate).

## [0.7.3] - 2026-07-01

Audit pre-publicare profund — a prins un blocant real de publicare pe e-Factura + corecții la importuri și SAF-T.

### Fixed
- **e-Factura unitCode (blocant publicare)**: unitățile de măsură se emit acum ca UN/ECE Rec 20 (ex. „buc" → `H87`) — înainte, orice factură cu unitatea implicită era respinsă de ANAF (BR-CL-23).
- **Import**: WinMentor cu virgulă zecimală nu mai produce silențios 0; diacriticele din DBF-urile SAGA decodate corect (code pages CP852/CP1250).
- **SAF-T D406**: furnizorii străini primesc un SupplierID valid, consecvent cu GL (fără referințe agățate; validatorul DUK respinge bucket-ul generic).
- **Salarizare**: part-time + depășire diurnă — D112 declară CAS/CASS pe baza ridicată (GL ≡ D112).
- `is_period_locked` eșuează închis (fail-closed) pe toate cele 17 situri; i18n + cod mort; asocierea fișierelor `.xml`.

## [0.7.2] - 2026-07-01

Responsive pe toate rezoluțiile + completarea blocajului fiscal de perioadă pe toate căile de postare GL + solduri SAF-T reale + întărirea licențierii.

### Added
- **SAF-T D406**: solduri reale de deschidere/închidere (bazate pe semn, nu fixe pe tip de cont) în locul valorilor 0.00; gate-ul de integrare XSD + DUK restaurat în verificarea locală.
- **D300**: avertisment de reconciliere înainte de depunere când facturile primite au defalcarea de TVA incompletă.

### Fixed
- **Layout responsiv** pentru fiecare ecran (1024px → 4K) + scrollbar-uri Windows + suprimarea flash-ului de consolă la procesele copil pe Windows.
- **Blocaj de perioadă complet**: garduri pe toate căile directe de postare GL (post_manual_journal, post_payroll, post_depreciation, post_register_lines + restul de 8 postere); intrare de audit la deblocarea unei perioade depuse; avertisment la validarea facturilor în perioade închise.
- **Salarizare**: deducerea personală corectată la regula 2026 ancorată în salariul minim (nu pragul fix pre-2023 de 3600 lei).
- **Robustețe**: ingestie SPV atomică (rollback la eșecul inserării liniilor de TVA), garduri multi-tenant, cifra de control CUI la import, indexuri pe căile fierbinți gl_entry, sanitizarea caracterelor de control interzise XML 1.0 în UBL + declarații.
- **UI**: cardul Declarații nu mai taie butoanele din coloana dreaptă; notă TVA 9% locuințe.

### Security
- **Licențiere întărită**: anti-rollback cu semănare în keychain la start_trial/activare, machine-id nativ (machine-uid), eliminarea checksum-ului legacy.

### Changed
- Branding installer RoFactura → Clarito (imagini NSIS/DMG + sursa iconului).

## [0.7.1] - 2026-06-25

Foaia de parcurs ERP completă (P1+P2+P3, 20 de valuri cu QA) + declarații noi + vizualizator XML/PDF în aplicație + RBAC multi-utilizator. Cel mai mare release de până acum (~230 de commit-uri).

### Added
- **Terți & bancă**: IBAN + termene de plată pe contacte, aging AR/AP (balanță cu vechime sold), import extrase bancare (MT940/CAMT.053/CSV) cu potrivire pe facturi, reevaluare valutară lunară (OMFP 1802), casă în valută (5314), registru de casă (14-4-7A), fișă de cont pe partener.
- **Stocuri & producție**: multi-gestiune, NIR (14-3-1A) + mod retail, transfer inter-gestiune (14-3-3A), producție/BOM cu cost complet (materiale + manoperă + regie, OMFP 1802/IAS 2), aviz de însoțire (14-3-6A), dezmembrări, LIFO, registru-inventar + inventariere.
- **Documente comerciale**: comenzi/oferte/devize, contracte cu notificare la expirare, facturi de avans 419/409 (TVA la avans + stornare la decontare, art. 282), cecuri & bilete la ordin, deconturi + avansuri de trezorerie (542) cu motor de diurnă (plafon + split multi-lună).
- **Salarizare**: sporuri + rețineri/popriri, pontaje (condică de prezență), simulator brut↔net, export REGES-Online (HG 295/2025), diurnă impozabilă → D112 automat (toate cele 4 contribuții), config GL per firmă.
- **Declarații noi**: D205 (dividende, validat DUK verify-first), D207 (dividende nerezidenți, XSD), D301/D700/D710 (trec DUKIntegrator real), D112 migrat la `:v7` + concedii medicale B-path, istoric depuneri („Declarații depuse"), toggle rectificativă (D112/D205/D207/D390), registrul bunurilor de capital + ajustare TVA art. 305 (D300 R31_2), rânduri memo „TVA neexigibilă" în D300, angajamente 471/472, provizioane 15x.
- **Import date**: asistent de migrare din SAGA (XML + DBF), WinMentor (TXT) și SmartBill (REST) — staging, dedup, păstrarea numerelor de factură.
- **UI**: import integral „Claude Design" (sidebar rotunjit + toate paginile rescrise), vizualizator/editor XML în aplicație care redă declarațiile ca documente etichetate uman (+ XLSX, Print/PDF, re-validare DUK), vizualizator PDF încorporat (PDFium WASM), pagini Avize + Dezmembrări, popup de notificări, animații.
- **VIES**: validare cod TVA intra-UE + preflight parteneri pe D390.

### Fixed
- **Salarizare**: eliminată contribuția-fantomă CCI 0,85% (abrogată din 2018, inclusă în CAM 2,25%); floor-ul indemnizației de concediu pe durata certificatului (OUG 91/2025); un singur motor de calcul alimentează și GL și D112 (nu mai pot diverge).
- **Audituri**: valuri multiple cu QA — scurgeri de export între companii, orfani GL la ștergerea plăților, blocaj fiscal de perioadă (prima versiune), rasa de conversie ofertă→comandă (status în afara CHECK-ului), tray pe macOS, RBAC viewer cu adevărat read-only + gardă last-admin, parolă minim 8 caractere.
- **D390**: etichetele codurilor de operațiune P/S/R corectate conform OPANAF 705/2020; D300 R17_1 include R1_1 + R13_1 (P0); R12 taxare inversă doar din achiziții.
- Fixture-urile de test cu `CREATE TABLE` scris de mână convertite integral la `sqlx::migrate!` + gard anti-regresie.

### Security
- **RBAC multi-utilizator**: gate de comenzi fără bypass + autentificare Argon2id + roluri (deny-by-default pe scrieri pentru viewer).
- Timeout de sesiune la inactivitate (15 min), cert-pinning ANAF (safe, default-off), scoping multi-tenant pe statusurile facturilor + jurnalul de activitate.

## [0.7.0] - 2026-06-11

Programul fiscal 2026 complet: TVA la încasare pe ambele sensuri, salarizare + D112, bilanțuri oficiale, D100/D101, e-Transport, e-TVA, D390 — plus validare DUK integrată în aplicație (JRE minimal + DUKIntegrator incluse în installer) și internaționalizare EN.

### Added
- **TVA la încasare (cash-VAT)** cap-coadă: decizie de exigibilitate + matrice de excludere, motor de eliberare proporțională, rutare D300 pe data încasării/plății (ambele sensuri, 4428), mențiunea obligatorie pe factură (UBL + PDF), monitor plafon 5.000.000 lei + reminder 097/700.
- **Salarizare & D112**: motor de calcul 2026 (CAS/CASS/CAM/impozit, carve-out 300 lei OUG 89/2025), angajați + rulări lunare + postări GL, concedii medicale (OUG 158/2005), sedii secundare (split F1/F2), taxonomia completă art. 146 part-time, export XML D112 validat pe DecUnica.xsd.
- **Bilanț & profit**: cont de profit și pierdere + închidere 6/7→121, bilanț OMFP 1802/2014 cu export XML oficial pe toate cele 3 formulare (S1005 micro / S1003 mic / S1002 mare), regula de mărime „2 ani consecutivi", D100 (trimestrial) + D101 (impozit pe profit, OPANAF 206/2025, cap 70% report pierdere).
- **e-Transport**: declarație UIT (schema v2) + submisie OAuth + fereastra de 3 zile; **SPV**: inbox general (recipise, notificări, somații); **e-TVA**: preluare P300 precompletat + reconciliere pre-depunere; **D390**: recapitulativă VIES intra-UE.
- **Validare DUK în aplicație**: installerele includ un JRE minimal (jlink) + jar-urile DUKIntegrator — exportul oficial validează local (blocare + override + notă grațioasă); banner de versiune formular ANAF la lansare.
- **Stocuri & imobilizări**: evaluare stoc FIFO/CMP cu ledger + postări GL, amortizare mijloace fixe (registru + 6811/281x lunar + cedare), monitor Intrastat (1.000.000 lei/flux).
- **GL**: închidere TVA de perioadă (4423/4424), registru-jurnal + cartea mare + balanța de verificare (patru egalități), câștig/pierdere FX (665/765), split venituri 701/704/707.
- **i18n**: interfață tradusă integral în engleză (5 valuri) + infrastructură i18next; **UI**: port „verbatim" al designului nou pe toate paginile, icon macOS conform HIG, template PDF de factură personalizabil; facturi B2C (2026).
- **CI**: workflow de release (Windows .exe/.msi + macOS .dmg per-arch).

### Fixed
- **Cote TVA 2026** (Legea 141/2025): catalog aliniat (21%/11%), facturile/produsele noi implicit 21%, avertisment non-blocant pe cote vechi, respingerea cotelor abrogate pe facturi post-reformă.
- **Storno + cash-VAT**: storno nu mai inversează semnele GL (consistent cu D300); originalul STORNAT nu amână TVA la 4428.
- Trei runde de audit (r3): scoping multi-tenant pe stocuri, rotunjire comercială peste modulele vechi, atomicitate batch declarații, garduri de date calendaristice, plafonul micro EOY + avertisment D112 iulie-2026, monitoare de retenție SPV/UIT/plafon TVA.
- Lookup ANAF pe CUI reparat (endpoint v9) cu auto-completare la contacte.

## [0.6.0] - 2026-06-03

Declarații ANAF conforme oficial: **D300, D394 și SAF-T D406 trec validatorul oficial ANAF (DUKIntegrator) cu 0 erori** — nu doar XSD, ci și regulile de business. Regulile au fost extrase prin decompilarea validatoarelor ANAF; fiecare etapă construită cu sub-agenți + QA adversarial (re-rulare a validatorului real) + gate verde.

### Added
- **Generatoare XML oficiale** pentru D300 (decont TVA, v12), D394 (declarație informativă, v5) și SAF-T D406 (v2.4.9) — XML conform schemei ANAF, validat cu `xmllint` pe XSD-ul oficial **și** cu DUKIntegrator pe regulile de business. Strat de versionare per-perioadă (alege automat schema/namespace corect pentru perioada raportată).
- **Motor de contabilitate în partidă dublă (GL)**: note contabile auto-generate din facturi/încasări/plăți pe planul de conturi RO (OMFP 1802/2014) — D 4111 / C 707 + 4427 (vânzări), D 6xx + 4426 / C 401 (achiziții), taxare inversă (4426=4427), stornare; reconciliere care leagă Σ4427 ↔ TVA colectată D300 și Σ4426 ↔ TVA deductibilă. Ecran „Jurnal contabil & reconciliere".
- **SAF-T MasterFiles + SourceDocuments + GeneralLedgerEntries** complete (conturi, clienți, furnizori, taxe, UoM, produse, facturi vânzare/cumpărare, plăți, note contabile).
- **Inventar + imobilizări**: tabele stocuri (`stock_movements`) + mijloace fixe (`fixed_assets`/`asset_transactions`) cu calcul de amortizare liniară; secțiunile SAF-T MovementOfGoods + Assets; varianta **anuală D406A** (HeaderComment=A, perioadă pe an, AssetTransactions) — trece DUKIntegrator cu 0 erori.
- **UI**: butoane „Export oficial ANAF" lângă extrasele de lucru pe D300/D394/SAF-T, formulare de depunere (declarant/CAEN/bancă/reprezentant), cod art. 331 pe produse, ecran GL/reconciliere.
- Validare cifră de control CUI (mod-11) și generare automată a NDP (număr de evidență a plății, 23 caractere) pentru D300.

### Fixed
- Nomenclatoare DUK pentru SAF-T: TaxCode pe 6 cifre (310309/310344/…), UoM UN/ECE (H87/HUR), PaymentMethod, SelfBillingIndicator, regim fiscal — în loc de literali respinși de ANAF.
- D300 R26 (sumă de control `totalPlata_A`) și D394 (matrice tip↔cotă↔partener, op11 cu cod art. 331 real, reconciliere rezumat1/rezumat2) — corectate la formulele exacte ale validatorului.

### Notes
- Validatoarele DUKIntegrator + schemele ANAF sunt vendate local (gitignored, via `scripts/fetch-validators.sh`); ANAF re-versionează nomenclatoarele de ~2 ori/an.
- Depunerea efectivă necesită semnarea PDF cu certificat (smart-card) — în afara automatizării. Cross-platform macOS/Windows validat în CI.

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
