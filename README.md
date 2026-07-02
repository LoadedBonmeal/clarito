# Clarito — e-Factura & contabilitate ANAF (desktop)

**Clarito** este o aplicație desktop nativă pentru contabilitate și raportare fiscală românească: emite și trimite facturi electronice prin SPV, ține contabilitatea în partidă dublă și generează declarațiile ANAF (D300, D390, D394, D112, D205, D406 SAF-T, bilanț), totul **local**, fără backend cloud propriu. Construită cu **Tauri 2 + React 19 + Rust + SQLite**.

> Versiune: vezi [CHANGELOG](CHANGELOG.md) · Bundle id: `com.lucaris.efactura` · Licență: Proprietary © Lucaris

---

## Cuprins

- [Ce face Clarito](#ce-face-clarito)
- [Matricea declarațiilor & e-documentelor](#matricea-declarațiilor--e-documentelor)
- [Funcționalități în detaliu](#funcționalități-în-detaliu)
- [Validare (DUK & XSD)](#validare-duk--xsd)
- [Conformitate fiscală & temei legal](#conformitate-fiscală--temei-legal)
- [Tech stack](#tech-stack)
- [Structură proiect](#structură-proiect)
- [Setup dezvoltare](#setup-dezvoltare)
- [Build-uri de producție](#build-uri-de-producție)
- [Poarta de verificare locală](#poarta-de-verificare-locală)
- [Validatori ANAF (fetch)](#validatori-anaf-fetch)
- [Medii ANAF SPV](#medii-anaf-spv)
- [Confidențialitate](#confidențialitate)
- [Ce NU e (încă) implementat](#ce-nu-e-încă-implementat)
- [Licențiere & cumpărare](#licențiere--cumpărare)
- [Troubleshooting](#troubleshooting)
- [Support](#support)

---

## Ce face Clarito

| Domeniu | Pe scurt |
|---|---|
| **e-Factura** | Generare UBL 2.1 CIUS-RO, trimitere la ANAF SPV (OAuth2 PKCE), urmărire stare, storno/note de credit, PDF cu template |
| **TVA** | Decont D300 (v12), recapitulativ D390 (v3), informativ D394 (v5) — toate cu mapare automată din facturi |
| **Salarii** | Motor de calcul 2026 (CAS/CASS/CAM/CCI/impozit), concedii medicale, D112 (`declaratie_unica:v7`) validat DUK |
| **Profit & dividende** | D100, D101 (report pierdere), D205 (informativă dividende, XML + DUK) |
| **e-Transport** | Notificare UIT v2, submisie OAuth, validitate cod UIT |
| **SAF-T** | D406 periodic (L) + anual (A), validat XSD oficial |
| **Contabilitate** | Registru-jurnal partidă dublă, plan de conturi OMFP 1802/2014, închideri lună/an, balanță, fișă cont, cartea mare |
| **Bilanț** | S1005 (micro), S1003 (mic), S1002 (mediu/mare) — XML conform ANAF |
| **Imobilizări & stocuri** | Amortizare liniară + postare GL, evaluare stoc FIFO / CMP, fișă de magazie |
| **Platformă** | Multi-firmă cu izolare de date, vizualizator XML→document, export PDF/XLSX, monitoare de praguri, licențiere pe niveluri |

Aplicația acoperă un ciclu contabil lunar/anual real pentru 2026: emiți facturi → se postează automat în GL → calculezi salariile → generezi declarațiile → închizi luna/anul → exporți bilanțul. **192 de funcționalități** implementate la nivel de cod.

---

## Matricea declarațiilor & e-documentelor

| Document | Schemă / namespace | XML | Validare | PDF/XLSX | Temei legal |
|---|---|:---:|---|:---:|---|
| **e-Factura** | UBL 2.1 `CIUS-RO:1.0.1` | ✅ | 50+ reguli EN16931/CIUS-RO | PDF + XLSX | Legea 139/2022 |
| **D300** decont TVA | `d300:declaratie:v12` | ✅ | DUK + xmllint XSD | — | OPANAF 174/2026 |
| **D390** recapitulativ | `d390:declaratie:v3` | ✅ | test structural | — | OPANAF 705/2020 |
| **D394** informativ | `d394:declaratie:v5` | ✅ | DUK + xmllint XSD | — | OPANAF 3769/2015 (mod. 2194/2025) |
| **D112** salarii/contribuții | `declaratie_unica:declaratie:v7` | ✅ | DUK (`D112Validator.jar`) | — | Ordin 605/95/928/2.314/2026 |
| **D205** dividende | `declaratie205` v3 | ✅ | DUK (`D205Validator.jar`) | — | OPANAF 179/2022 (mod. 102/2025) |
| **D406 SAF-T** | `d406:declaratie:v1` (v249) | ✅ | xmllint XSD | — | OPANAF 1783/2021 |
| **e-Transport** | `eTransport:declaratie:v2` | ✅ | xmllint XSD (`schema_ETR_v2.xsd`) | — | OUG 41/2022 |
| **Bilanț micro** | `s1005:v14` | ✅ | xmllint XSD | — | OMFP 1802/2014 |
| **Bilanț mic** | `s1003:v15` | ✅ | xmllint XSD | — | OMFP 1802/2014 |
| **Bilanț mediu/mare** | `s1002:v15` | ✅ | xmllint XSD | — | OMFP 1802/2014 |
| **D100** obligații de plată | — (informativ) | PDF inteligent | calcul intern | — | Nomenclator obligații |
| **D101** impozit pe profit | — (informativ) | PDF inteligent | calcul intern | — | OPANAF 206/2025 |

Toate declarațiile cu XML pot fi vizualizate în aplicație ca **document profesional cu etichete în română**, tipărite/salvate ca PDF și exportate XLSX (vezi [Vizualizator XML](#vizualizator-xml--export-pdfxlsx)).

---

## Funcționalități în detaliu

### e-Factura (facturare electronică)

- **Generare UBL 2.1 CIUS-RO** — `CustomizationID` conform `urn:efactura.mfinante.ro:CIUS-RO:1.0.1`, tip 380 (normală) / 381 (storno), matematică pe `Decimal` (fără `f64`), multi-monedă cu `TaxCurrencyCode=RON`.
- **Ciclu de viață factură** — mașină de stări `DRAFT → QUEUED → SUBMITTED → VALIDATED|REJECTED → STORNED`, cu tranziții atomice și blocarea stărilor terminale.
- **Draft & editare** — alocare atomică de numere per firmă+serie (fără goluri), linii cu cotă+categorie TVA (S/AE/E/Z/K/O), cod CPV, natură venit (marfă/serviciu/reducere), cod art. 331.
- **Storno & note de credit (381)** — copiază liniile cu cantități negative, marchează originalul `STORNED` atomic, previne storno-de-storno.
- **Duplicare factură**, **verificare integritate fișiere** (amprentă MD5/SHA la generare, re-verificată la acces).
- **PDF factură** — layout A4, branding firmă, sumar TVA, sumă în litere (RO); template per firmă (preset clasic/modern/minimal, culoare accent, note antet/subsol).
- **Submisie ANAF SPV** — OAuth2 PKCE cu refresh single-flight, `POST /FCTEL/rest/upload`, secret în OS keychain, comutare test/prod.
- **Verificare stare** — `GET /FCTEL/rest/stareMesaj`, polling de fundal la 5 min pentru facturile trimise, auto-update status.
- **Sincronizare SPV inbox** — task zilnic 04:00, descărcare ZIP (factură.xml + .pdf), dedup atomic, categorizare mesaje (recipisă/notificare/somație/decizie/factură).
- **Facturi primite** — listare, status (nou→revizuit→aprobat/respins→arhivat), tip intra-UE (bunuri/servicii) pentru rutarea corectă în D300.
- **Chitanțe/bonuri** — PDF A5, numerotare atomică per serie.
- **Cash-VAT (TVA la încasare, art. 282)**, **taxare inversă (art. 331)**, **intra-UE (categoria K) & multi-monedă**, **blocare cote vechi** (19%/5% interzise pe facturi ≥ 2025-08-01, conform Legea 141/2025).

### TVA — D300 / D390 / D394

- **D300 (decont TVA)** — randuri R1–R42 mapate automat din facturi grupate pe (categorie, cotă, natură); cote 2026 (21%/11%/9%); regularizări cote vechi în R16/R30 (OPANAF 174/2026); servicii intra-UE (R7/R20) separate de bunuri (R5/R18); taxare inversă (R12 colectat / R25 dedus, egale prin reguli DUK); pro-rata deducere (art. 300); generare NDP (Număr de Evidență a Plății, 23 caractere); sumă de control `totalPlata_A`.
- **D390 (recapitulativ VIES)** — agregare pe (țară, cod TVA partener, tip operațiune L/T/A/P/S/R); contor „dropped" pentru parteneri UE fără cod TVA valid (alertă sub-raportare VIES).
- **D394 (informativ)** — clasificare automată tip partener (1–4) din CUI, mapare operațiuni (L/V/LS/A/C/AI/AS), sub-secțiuni op11 cu coduri produs art. 331, cartuș G/I pentru bonuri fiscale AMEF + facturi simplificate (intrare manuală), maparea perioadei (luna = sfârșit de perioadă).

### Salarii & D112

- **Gestiune angajați** — tip asigurat, pensionar, tip contract (N / P1–P7), ore/normă, sediu secundar, excepții CAS minim, beneficiar sumă netaxabilă; validare CNP mod-11.
- **Calcul salariu 2026** — brut → (CAS 25%, CASS 10%, impozit 10%) → net; CAM 2,25% + CCI 0,85% (cost angajator); plafonare deducere personală (art. 77); sumă netaxabilă 300/200 lei (art. III OUG 89/2025) scutită de toate cele 4 contribuții.
- **Salariul minim garantat part-time** — bazele CAS/CASS ridicate la salariul minim cu proratare pe zile active (angajare/încetare la mijloc de lună); excepții art. 146(5⁷).
- **Concedii medicale (OUG 158/2005)** — proratare salariu pe zile lucrate, indemnizație în CAS 25%, CASS selectiv (coduri 01/07/10), NU în CAM; tratament fiscal per cod indemnizație; prima zi neplătită + tranșe graduale 55/65/75% (OUG 91/2025).
- **D112 (`declaratie_unica:v7`)** — căi A (standard) / B (concediu medical) + angajator C1/C2/C4/F1/F2; CAM/CCI agregate (regula A21.46, fără drift de 1 leu); sedii secundare; coduri obligații 602/412/432/480; **test golden GL ≡ D112** (totalurile din contabilitate = valorile din declarație); poartă DUK (`D112Validator.jar`).
- **Postare GL salarii** — 641/421/4315/4316/444/646/436/6458/4373/4382, idempotent per perioadă.

### Profit/venit & dividende — D100 / D101 / D205

- **D100** — obligații trimestriale: micro 1% (poz. 5), profit 16% anticipat T1–T3 (poz. 2); scadență 25 lună următoare; rânduri informative dividende (PF cod 604 / PJ cod 150).
- **D101** — rezultat fiscal, report pierdere plafonat 70%/an (OUG 115/2023), impozit 16%, credit sponsorizare min(0,75% CA, 20% impozit); deadline 25 martie din 2026.
- **D205 (informativă dividende, XML)** — agregare beneficiari rezidenți persoane fizice pe CNP, tip venit 08; rată impozit 16% (din 2026) / 10% (tranzitoriu sau situații interimare 2025); postare 117/457/446; export DUK-validat + previzualizare în vizualizator.

### e-Transport & SAF-T (D406)

- **e-Transport UIT v2** — toate cele 11 tipuri de operațiune; validare CUI declarant, bunuri (denumire/cantitate/UM/greutate brută), partener, vehicul, locații (adresă / PTF / birou vamal), documente transport; fereastră de predeclarare ≤ 3 zile; validitate cod UIT 5/15 zile; submisie OAuth la `ETRANSPORT/ws/v1/upload`.
- **SAF-T D406 periodic (L)** — Header + MasterFiles (plan conturi, clienți/furnizori, TaxTable cu coduri DUK pe 6 cifre, UOM, produse) + GeneralLedgerEntries (jurnale auto-postate, debit=credit) + SourceDocuments (facturi emise/primite, plăți).
- **SAF-T D406 anual (A)** — imobilizări + tranzacții de amortizare; restul secțiunilor wrapper-e goale conform regulilor DUK v249.
- **Calculator deadline D406** (OPANAF 1783/2021), rutare automată L/A, validare XSD oficial (`Ro_SAFT_Schema_v249.xsd`).

### Contabilitate în partidă dublă (registru-jurnal)

- **Postare GL automată** din facturi (emise/primite), plăți, salarii, amortizare, stocuri — cu garda de echilibru (±0,005 RON/notă), grupare TVA pe cotă, idempotență per (firmă, sursă).
- **Plan de conturi OMFP 1802/2014** (60+ conturi seed, clasele 1–7), **TVA la încasare** (4428 → 4427/4426 la plată, split storno art. 282), **taxare inversă** (4426 = 4427), **diferențe de curs** (665/765).
- **Închideri**: decont TVA (4426/4427 → 4423/4424), închidere 6/7 → 121, impozit pe profit (691/698), închidere anuală 121 → 117.
- **Rapoarte**: balanță de verificare (4 egalități, OMFP 2634/2015), registru-jurnal (Legea 82/1991), cartea mare / fișă de cont, P&L, bilanț contabil; **reconciliere GL ↔ D300**.

### Situații financiare — Bilanț

- **S1005 micro** (`s1005:v14`), **S1003 mic** (`s1003:v15`), **S1002 mediu/mare** (`s1002:v15`) — F10/F20 derivate din balanță; clasificare automată pe mărime cu **regula celor doi ani consecutivi** (OMFP 1802/2014 pct. 13(2)); mapare cod județ → cod ANAF; XML canonic (UTF-8 + LF + 2 spații) validat XSD.

### Imobilizări & stocuri

- **Registru imobilizări** — amortizare liniară, postare lunară D 6811 / C 281x, începe luna după PIF, oprește luna înainte de casare; **casare/cesiune** (D 281x + D 6583 / C 21x).
- **Evaluare stoc** — **FIFO** (consumă straturile cele mai vechi) și **CMP** (cost mediu ponderat), per produs; **fișă de magazie** (cantitate/valoare rulante); avertizare **gestiune negativă** (nu blochează — responsabilitatea contabilului); postare GL (capitalizare 371 / COGS 607).

### Vizualizator XML & export PDF/XLSX

- **Document viewer (XML → HTML)** — randează declarațiile și facturile ca **documente cu titluri și etichete în română** (nu cod brut): parser tipat pentru UBL, sistem de descriptori + dicționar de etichete pentru D205/D112/D300/D390/D394/D406/e-Transport.
- **Print & Salvare PDF** din vizualizator, **export XLSX etichetat** (o filă per secțiune, layout identic cu PDF-ul), **export registru facturi XLSX** (filtrabil).

---

## Validare (DUK & XSD)

Clarito validează ieșirile cu instrumentele oficiale ANAF, **înainte** de submisie:

- **DUKIntegrator** (validator Java oficial ANAF) — acoperă **D300, D394, D406, D112, D205**. La export, dacă DUK raportează erori, fișierul **nu** se scrie (decât cu override explicit); avertismentele trec. Degradare grațioasă: dacă runtime-ul Java/jar lipsește, exportul continuă.
- **xmllint + XSD oficial** — validare structurală pentru **D300, D394, D406, e-Transport** și bilanț, prin teste de integrare care rulează `xmllint --schema`. Se sare grațios când XSD-ul sau xmllint lipsesc (build verde peste tot).
- **Teste structurale** — D390 (XSD-ul `:v3` nu e publicat standalone) e acoperit de un test structural care fixează namespace-ul și câmpurile obligatorii.

Toate XML-urile generate au **round-trip real** XSD/DUK în suita de teste (D300/D394/SAF-T/e-Transport prin xmllint; D112/D205 prin DUK).

---

## Conformitate fiscală & temei legal

Implementările respectă legislația ANAF în vigoare pentru 2026, fără presupuneri hardcodate despre cote/scheme vechi (`version.rs` rezolvă dinamic per perioadă raportată):

- **TVA**: OPANAF 174/2026 (D300 v12, cote 21/11/9%, regularizări), OPANAF 705/2020 (D390 v3), OPANAF 3769/2015 mod. 2194/2025 (D394 v5)
- **Salarii**: OUG 89/2025 (sumă netaxabilă), OUG 158/2005 + OUG 91/2025 (concedii medicale), art. 77/146 Cod fiscal, Ordin 605/95/928/2.314/2026 (D112 v7 H2 2026)
- **Profit/dividende**: OPANAF 206/2025 (D101), OPANAF 179/2022 mod. 102/2025 (D205), OUG 115/2023 (report pierdere 70%), Legea 141/2025 (cote dividende 16%/10%)
- **Contabilitate**: OMFP 1802/2014 (plan conturi, bilanț), OMFP 2634/2015 (balanță), Legea 82/1991 (registre)
- **SAF-T / e-Transport**: OPANAF 1783/2021 (D406), OUG 41/2022 (e-Transport)
- **Praguri**: OUG 89/2025 (plafon micro 100.000 EUR), OUG 8/2026 (plafon TVA la încasare 5.000.000 lei), Ord. INS 1604/2025 (Intrastat)

> ⚠️ Clarito asistă pregătirea declarațiilor; responsabilitatea fiscală finală rămâne a contribuabilului/contabilului. Verificați întotdeauna cu DUK oficial înainte de depunere.

---

## Tech stack

**Frontend:** React 19.1 · TypeScript 5.8 (strict) · Vite 7 · Tailwind CSS 4.3 · Zustand 5 · TanStack Router · React Query 5 · Radix UI · i18next (RO/EN) · Vitest 4

**Backend:** Rust 2021 · Tauri 2 · SQLx 0.8 (SQLite + Tokio) · quick-xml 0.36 · printpdf 0.7 · rust_xlsxwriter 0.80 · reqwest 0.12 (**rustls**, fără OpenSSL) · keyring 3 (Keychain macOS / Credential Manager Windows)

**Profil release:** opt-level 3, LTO, `codegen-units=1`, `strip=true`, `panic=abort`.

**Targets:** macOS (arm64 + x86_64 universal), Windows (x64 + arm64).

---

## Structură proiect

```
efactura-desktop/
├── src/                       # Frontend React + TS
│   ├── pages/ components/      # UI (facturi, declarații, rapoarte, setări)
│   ├── lib/doc-render/         # vizualizator XML→document (labels, descriptors, XmlDocView)
│   ├── lib/xml-to-tables.ts    # export XLSX etichetat
│   └── locales/{ro,en}/        # i18n
├── src-tauri/                  # Backend Rust + Tauri
│   ├── src/anaf_decl/          # generatoare declarații (d300, d390, d394, d112, d205, saft, etransport, bilant)
│   ├── src/ubl/                # e-Factura UBL (generator, validator, pdf)
│   ├── src/db/                 # GL, facturi, salarii, dividende, imobilizări, stocuri, firme, licență
│   ├── src/commands/           # comenzi Tauri (IPC)
│   ├── src/anaf/               # OAuth + client SPV
│   ├── src/background/         # polling SPV, sync, certificate
│   ├── migrations/             # SQL migrations
│   ├── tests/                  # round-trip XSD (d300_xsd, d394_xsd, saft_xsd, etransport_xsd)
│   ├── tools/                  # validatori ANAF (git-ignored — vezi fetch-validators.sh)
│   ├── Cargo.toml
│   └── tauri.conf.json
├── scripts/
│   ├── verify-local.sh         # poarta de verificare (gate)
│   └── fetch-validators.sh     # descarcă DUK + XSD-uri ANAF
└── package.json
```

---

## Setup dezvoltare

### Prerechizite

```bash
# Rust (toolchain stable)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Node.js 22+ și pnpm
npm install -g pnpm
```

### Targets Rust

```bash
# macOS (build local universal)
rustup target add aarch64-apple-darwin x86_64-apple-darwin
# Windows MSVC
rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc
```

### Instalare & pornire dev

```bash
cd efactura-desktop
pnpm install
pnpm tauri dev      # Vite pe localhost:1420 + fereastra Tauri cu hot-reload
```

---

## Build-uri de producție

```bash
pnpm build:mac        # macOS universal (arm64 + Intel)
pnpm build:mac-arm    # doar Apple Silicon
pnpm build:mac-intel  # doar Intel
pnpm build:win-x64    # Windows x64 (rulează pe Windows)
pnpm build:win-arm    # Windows ARM64 (rulează pe Windows)
```

Output (ex. macOS): `src-tauri/target/universal-apple-darwin/release/bundle/dmg/Clarito_0.7.0_universal.dmg`

**Cross-platform:** build-urile Windows din macOS/Linux necesită `cargo-xwin` sau o mașină Windows reală. Recomandare: build-urile de release prin CI pentru semnătură + notarizare consistentă.

---

## Poarta de verificare locală

Înainte de orice commit, rulează poarta completă:

```bash
bash scripts/verify-local.sh
```

Rulează, în ordine: `tsc --noEmit` · `pnpm test` (Vitest) · `pnpm build` · `cargo fmt --check` · `cargo check` · `cargo test --lib` · `cargo clippy -- -D warnings`. Banner final la succes: **„Toate verificările au trecut!"**

---

## Validatori ANAF (fetch)

Validatorii oficiali (jar DUK + XSD-uri) **nu** sunt în repo (`src-tauri/tools/` e git-ignored). Pentru a activa poarta DUK/XSD:

```bash
bash scripts/fetch-validators.sh
export EFACTURA_DUK_JAR="$PWD/src-tauri/tools/dukintegrator/DUKIntegrator.jar"
```

Scriptul descarcă: `DUKIntegrator.jar`, `D112Validator.jar`, SAF-T XSD (v249), e-Transport XSD (v2). Idempotent și ne-blocant — testele care au nevoie de validatori se sar grațios dacă lipsesc. ANAF publică versiuni noi de ~2 ori/an; actualizează URL-urile în script când apare o schemă nouă.

---

## Medii ANAF SPV

- **TEST:** `https://api.anaf.ro/test/` — dezvoltare și testare
- **PROD:** `https://api.anaf.ro/prod/` — facturare reală

Comutare: Setări → Avansat → „Folosește mediul test ANAF".

---

## Confidențialitate

- Toate datele sunt stocate **local** (SQLite + arhivă pe disk).
- Tokenii OAuth ANAF sunt în **OS Keychain** (macOS Keychain / Windows Credential Manager), nu în DB.
- **Fără backend cloud propriu** — comunicarea e direct cu ANAF. CSP strictă: `connect-src` permite doar `ipc:`, `webservicesp.anaf.ro`, `api.anaf.ro`.
- **Export GDPR**: Setări → exportă toate datele (ZIP + PDF-uri) sau șterge toate datele (cu confirmare).

---

## Ce NU e (încă) implementat

Pentru transparență (catalog v0.7.0):

- **D207** (dividende nerezidenți) — modelat în date (`beneficiary_resident`), dar emiterea XML nu e implementată.
- **D100 / D101** — fără XML; depunere manuală prin formularul PDF inteligent ANAF + SPV (vizualizările din app sunt informative/estimative).
- **Semnătură digitală XAdES** pe facturi, **import în lot**, **reconciliere cu extrase bancare** — neimplementate.
- **Amortizare accelerată/degresivă** (doar liniară), **reevaluare stoc** lunar la curs BNR, **legare automată stocuri↔facturi**.
- **SAF-T MovementOfGoods** — wrapper gol (regulile DUK v249 interzic copiii `StockMovement`; auto-populare la relaxarea regulii).
- Rândurile **F30 / F20 detaliat** ale bilanțului — completate în importatorul PDF ANAF (prin design).

---

## Licențiere & cumpărare

Niveluri: **Trial** (30 zile, 3 firme) · **Solo** (1 firmă) · **Accountant** (15 firme) · **Firm** (nelimitat). Activare cu cheie legată de mașină + email.

### Pentru utilizatori

1. „Cumpără licență →" în app (Setări → Suport și feedback) sau în ecranul „Licența a expirat".
2. Se deschide Stripe Payment Link în browser.
3. După plată primești cheia pe email.
4. Introdu cheia: Setări → Licență → Activează.

### Pentru dev (issue manual chei)

```bash
cargo run --bin license-gen -- --tier SOLO --email customer@example.com --expires-days 365
```

⚠️ **Important**: cheile sunt legate de build-ul curent (versiunea din `Cargo.toml`). Un version bump schimbă salt-ul XOR din `build.rs` → cheile vechi devin invalide. Re-emite cheile după fiecare release.

---

## Troubleshooting

### Licența invalidată după version bump

**Cauza**: salt-ul XOR din `build.rs` se calculează din `pkg_name + pkg_version`; bump-ul schimbă salt-ul → fingerprint-urile vechi nu mai corespund.

**Fix dev (propria mașină):**

```bash
pkill -f "Clarito|efactura-desktop"
sqlite3 ~/Library/Application\ Support/com.lucaris.efactura/data.db \
  "DELETE FROM license WHERE id=1; DELETE FROM settings WHERE key LIKE 'license_%';"
security delete-generic-password -s "ro.lucaris.efactura.trial.v1" -a "trial_status" 2>/dev/null
cargo run --bin license-gen -- --tier SOLO --email tu@tine.ro
```

### Anti-rollback hard-fail după sleep lung / corecție NTP

Rezolvat în v0.2.0 (tolerează drift până la 30 zile). Sub 0.2.0 → update; peste, fă clean state ca mai sus.

### „Blocking waiting for file lock on build directory"

`pnpm tauri dev` și un release build intră în conflict pe lock-ul cargo. Oprește dev-ul (Ctrl+C) înainte de release, sau folosește `CARGO_TARGET_DIR=/tmp/cargo-release pnpm build:mac`.

---

## Support

- **Email**: support@lucaris.ro
- **Buton „Trimite feedback"** în app: Setări → Suport și feedback → deschide clientul de email cu diagnostic atașat (versiune, OS, machine ID anonimizat, ultimele 50 linii log).

---

## Licență

Proprietary © Lucaris. Toate drepturile rezervate.
