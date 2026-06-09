# ANAF integration — how to test, validate & fix the live round-trips

Deep-research-backed guide (2026) for exercising the integration "residuals": the SPV inbox fetch,
the e-Transport submit, and the response-format / validation questions. Each claim below is
high-confidence (primary ANAF docs + the `printesoi/e-factura-go` reference SDK) unless marked
**OPEN**.

## 1. There is no certificate-free ANAF sandbox

All ANAF API auth funnels through a single OAuth2 IdP:

- authorize: `https://logincert.anaf.ro/anaf-oauth2/v1/authorize`
- token / refresh: `https://logincert.anaf.ro/anaf-oauth2/v1/token` (Basic Auth `client_id:client_secret`, `application/x-www-form-urlencoded`, `grant_type=authorization_code` then `refresh_token`)

Getting a token **always requires a real qualified digital certificate** enrolled in SPV with a
PJ role. The `/test/` endpoints are *not* an auth bypass — they consume the same certificate-derived
Bearer token and just write to a non-production data sink. Token lifetimes: **access 90 days**,
**refresh 365 days**; the token must be fetched within ~60 s of the authorize step.

So "test against ANAF" means: a real cert + an OAuth app + the `/test/` paths. Without a cert, you
test locally (section 5).

### Registering the OAuth app (developer, one-time)
1. Enrol the certificate in SPV (PJ role) at `https://www.anaf.ro/InregPersFizicePublic/#tabs-2`
   (also requires the certificate-confirmation document from the CA, e.g. certSIGN).
2. Register the OAuth client at `https://www.anaf.ro/InregOauth` — pick **E-Factura** and/or
   **E-Transport** from the nomenclator, set a callback URL (Postman's
   `https://oauth.pstmn.io/v1/callback` works for manual testing), click **Generare Client ID** →
   `client_id` + `client_secret`.
3. In the app: connect the company (the existing OAuth flow), then the app's commands hit the
   `/test/` host when `test_mode = true` (`AnafClient::new(true)` → `https://api.anaf.ro/test`).

## 2. Endpoint matrix (test ⇄ prod = swap the path segment)

| Service | OAuth2 host | Cert-at-call host |
|---|---|---|
| e-Factura (FCTEL) | `api.anaf.ro/{test\|prod}/FCTEL/rest/...` | `webserviceapl.anaf.ro/{test\|prod}/FCTEL/rest/...` |
| e-Transport | `api.anaf.ro/{test\|prod}/ETRANSPORT/ws/v1/...` | `webserviceapl.anaf.ro/...` |
| SPV inbox (general) | `webserviced.anaf.ro/SPVWS2/rest/listaMesaje` (read-only) | — |

The app uses the OAuth2 hosts. `AnafClient.base_url` already switches `test`/`prod`.

## 3. e-Transport UploadV2 response — CONFIRMED JSON (was "assumed")

`POST api.anaf.ro/{test|prod}/ETRANSPORT/ws/v1/upload/ETRANSP/{cif}/2` **accepts an XML body and
replies in JSON** (`application/json`). Verified against the MF Swagger spec and
`printesoi/e-factura-go` (`pkg/etransport/rest.go`: `UploadV2Response.IsOk()` ⇔ `ExecutionStatus == 0`).

Documented fields:

```jsonc
{
  "dateResponse": "…",
  "ExecutionStatus": 0,          // int32; 0 = accepted-for-processing
  "index_incarcare": 5012345678, // int64
  "UIT": "3R0…",                 // issued on accept
  "trace_id": "…",
  "ref_declarant": "…",
  "atentie": "…",                // optional, non-fatal warning
  "errors": [{ "errorMessage": "…" }]  // present on rejection
}
```

Our `parse_etransport_upload` (src-tauri/src/anaf/client.rs) handles exactly this: int64-or-string
index, `ExecutionStatus != 0` / `errors[]` → a human message, `UIT` optional, non-JSON → graceful
error. Locked by `etransport_upload_parse_matches_documented_uploadv2_contract`.

## 4. e-TVA precompletat (P300ETVA) — fetch endpoint RESOLVED

The precompletat is fetched from a **dedicated** ANAF service (NOT the general SPV `/cerere`):

```
GET https://api.anaf.ro/{test|prod}/decont/ws/v1/info?cui={cui}&an={an}&luna={luna}   # OAuth2
GET https://webserviceapl.anaf.ro/{test|prod}/decont/ws/v1/info?...                    # certificate
```

`cui`/`an`/`luna` are numeric + mandatory. The reply is a **zip with two JSON files** (the decont +
its `detalii`). The app now implements this: `AnafClient::fetch_etva_decont` + the
`etva_fetch_precompletat` command + `extract_etva_jsons` (unit-tested), surfaced by the **"Solicită
din SPV"** button in the e-TVA report.

**Still OPEN — the JSON key names.** ANAF publishes only the human-readable PDF row model
(OPANAF 2351/2025: rows rd.1–rd.36, collected rd.1–19 with rd.19 = "TOTAL TAXA COLECTATA",
deductible rd.20–36 with rd.36 = "TOTAL TAXA DEDUSA"; each row has `Valoare`/`TVA` + a `Surse de
date` field). No XSD/JSON schema is published. So: fetch one real zip with the button, read the
displayed JSON, and pin the key→row mapping. `reconcile_etva` deliberately takes user-entered
totals, so nothing is blocked meanwhile.

## 5. DUKIntegrator CLI — CONFIRMED (the app already uses it)

DUKIntegrator has a documented non-interactive CLI (ANAF `Instructiuni.txt`, cross-platform):

```
java -jar DUKIntegrator.jar [-c caleConfig] -v|-p|-s <tipDeclaratie> <fisierXML> [<fisierRezultat>] [optiuneValidare]
#   -v = validate only   -p = validate + PDF   -s = validate + signed PDF
#   skipped optional params must be passed as a literal "$"
```

The app's `run_java_validator` already invokes exactly this (`-v <TYPE> <xml> <result>`), and the
official-export commands (`export_d300_official`, `export_d394_official`, `export_saft_official`)
run the generated XML through it via `BundledProvider`.

**Status: integrated, bundled, and verified working.** `src-tauri/resources/duk/` ships
`DUKIntegrator.jar` + its validator jars (`D300Validator.jar`, `D394Validator.jar`,
`D406[T]Validator.jar`, the `*Pdf.jar`s, iText, bouncycastle), and `src-tauri/resources/jre-min/`
ships a jlink'd **OpenJDK 17** JRE; both are bundled as Tauri resources (`tauri.conf.json`), built
by `scripts/fetch-validators.sh` + `scripts/jlink-jre.sh`.

Empirically confirmed: `jre-min/bin/java -jar duk/DUKIntegrator.jar -c duk -v D300 <xml> <result>`
runs headlessly on the bundled JRE 17 and returns ANAF's real validation output. The jar is
Java-1.7-built but carries its OWN dependencies in `lib/`, so the removed-module (JAXB) concern
about modern JREs does **not** apply — JRE 17 runs it fine; **Temurin 8 is not required**. When the
bundle is absent (a bare dev checkout / `cargo test` with no `EFACTURA_DUK_JAR`), `run_duk` returns
`None` and the export degrades gracefully to the app's own XSD/rule validation.

## 6. Testing strategy without a certificate (what we actually do)

1. **Golden-fixture contract tests** (preferred): pin the documented response shapes and assert our
   parsers. Done for e-Transport UploadV2; extend the same pattern to FCTEL `stareMesaj` and SPVWS2
   `listaMesaje` (note `id_solicitare` is null for unsolicited messages — already handled).
2. **Local mock/simulator**: run an ANAF digital-twin (e.g. `aperta-sync/anaf-api-simulator`) and
   point `AnafClient.base_url` at it to exercise the OAuth + REST flow end-to-end without a cert.
3. **Cross-check against reference SDKs**: `printesoi/e-factura-go` (Go), `TecsiAron/ANAF-API-Client-PHP`,
   `florin-szilagyi/efactura-anaf-ts-sdk` — use these as the source of truth for field shapes.
4. **Live smoke test** (when a cert is available): connect a real company, set `test_mode`, submit a
   sample e-Transport UIT + fetch the SPV inbox, and capture the real JSON into new golden fixtures.
