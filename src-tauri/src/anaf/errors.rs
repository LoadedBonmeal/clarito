//! Mapare coduri erori ANAF → mesaje în română pentru utilizator.
//!
//! Plan Task 4.3 — `lookup_anaf_error(code)` returnează mesajul prietenos.
//! Mesajele includ acțiunea sugerată acolo unde e posibil.

use serde::Serialize;

/// Tipuri de erori ANAF structurate.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum AnafError {
    /// 401 — token expirat sau lipsă
    Unauthorized { message: String },
    /// Erori de validare în XML (lista de detalii)
    Validation { details: Vec<String> },
    /// 5xx — server ANAF indisponibil
    ServerError { status: u16, message: String },
    /// Timeout sau eroare de rețea
    NetworkError { message: String },
    /// 429 — prea multe cereri
    RateLimited { retry_after_secs: u64 },
    /// Alt cod de eroare ANAF cunoscut
    AnafCode { code: String, message: String },
}

impl AnafError {
    /// Mesaj prietenos pentru afișare în UI.
    pub fn user_message(&self) -> String {
        match self {
            Self::Unauthorized { .. } => {
                "Sesiunea ANAF a expirat. Reautentificați-vă din Setări → Certificate.".into()
            }
            Self::Validation { details } => {
                format!("Factura conține {} erori de validare:\n{}", details.len(), details.join("\n"))
            }
            Self::ServerError { status, .. } => {
                format!("Serverul ANAF a returnat eroare {status}. Reîncercați în câteva minute.")
            }
            Self::NetworkError { message } => {
                format!("Nu s-a putut contacta ANAF: {message}. Verificați conexiunea la internet.")
            }
            Self::RateLimited { retry_after_secs } => {
                format!("Prea multe cereri ANAF. Reîncercați după {retry_after_secs} secunde.")
            }
            Self::AnafCode { code, message } => {
                format!("[{code}] {message}")
            }
        }
    }
}

/// Returnează mesajul în română pentru un cod de eroare ANAF.
/// Acoperă 50+ coduri documentate în ghidul tehnic ANAF.
pub fn lookup_anaf_error(code: &str) -> &'static str {
    match code {
        // ── Erori CIF / identificare ───────────────────────────────────────
        "CIFV" | "CIF_INVALID" =>
            "CIF-ul furnizorului nu este valid. Verificați formatul (RO + cifre, max 10 cifre).",
        "CIFS" | "CIF_NOT_REGISTERED" =>
            "CIF-ul nu este înregistrat în sistemul RO e-Factura. Verificați dacă firma a aderat la sistem.",
        "CIFC" | "CIF_CUSTOMER_INVALID" =>
            "CIF-ul cumpărătorului nu este valid. Verificați formatul.",
        "CIFCS" | "CIF_CUSTOMER_NOT_REGISTERED" =>
            "CIF-ul cumpărătorului nu este înregistrat în RO e-Factura. Cumpărătorul poate să nu fie obligat să utilizeze sistemul.",

        // ── Erori sumă / calcule ───────────────────────────────────────────
        "SUMA_TOTALA" | "TOTAL_MISMATCH" =>
            "Suma totală a facturii nu corespunde cu suma liniilor + TVA. Recalculați totalul.",
        "SUMA_TVA" | "VAT_MISMATCH" =>
            "Suma TVA calculată nu corespunde cu sumele pe linii. Verificați cotele TVA.",
        "SUMA_NETA" | "NET_MISMATCH" =>
            "Suma netă (fără TVA) nu corespunde cu suma liniilor. Recalculați.",
        "SUMA_LINIE" | "LINE_TOTAL_MISMATCH" =>
            "Totalul unei linii (qty × preț) nu corespunde. Verificați calculele pe fiecare linie.",
        "SUMA_NEGATIVA" | "NEGATIVE_AMOUNT" =>
            "Sumele negative nu sunt permise pe facturile standard. Folosiți factura de storno.",

        // ── Erori structură XML / UBL ──────────────────────────────────────
        "XSD_INVALID" | "SCHEMA_XSD_INVALID" =>
            "Factura nu respectă structura XML UBL 2.1 cerută de ANAF. Verificați generatorul.",
        "CIUS_INVALID" | "CUSTOMIZATION_ID" =>
            "CustomizationID incorect. Valoarea cerută: urn:cen.eu:en16931:2017#compliant#urn:efactura.mfinante.ro:CIUS-RO:1.0.1",
        "PROFILE_ID" =>
            "ProfileID incorect. Valoarea cerută: urn:fdc:peppol.eu:2017:poacc:billing:01:1.0",
        "BOM_MISSING" | "ENCODING" =>
            "XML-ul trebuie să fie UTF-8 cu BOM (0xEF 0xBB 0xBF). Regenerați factura.",
        "NAMESPACE_INVALID" =>
            "Namespace-urile XML sunt incorecte. Verificați prefixele cbc: și cac:.",
        "INVOICE_TYPE_CODE" =>
            "InvoiceTypeCode trebuie să fie 380 pentru facturi normale sau 381 pentru note de credit.",
        "CURRENCY_INVALID" =>
            "Codul de monedă este invalid. Folosiți RON pentru tranzacții interne sau codul ISO 4217.",

        // ── Erori date ─────────────────────────────────────────────────────
        "DATA_EMITERE" | "ISSUE_DATE_INVALID" =>
            "Data emiterii este invalidă sau în format greșit. Folosiți formatul ISO 8601 (YYYY-MM-DD).",
        "DATA_SCADENTA" | "DUE_DATE_INVALID" =>
            "Data scadenței este invalidă sau anterioară datei emiterii.",
        "DATA_VIITOARE" | "FUTURE_DATE" =>
            "Data facturii este în viitor. ANAF nu acceptă facturi cu dată viitoare.",
        "DATA_VECHE" | "DATE_TOO_OLD" =>
            "Data facturii este prea veche. În mediul de test, facturile trebuie să fie din ultimele 30 zile.",

        // ── Erori TVA ──────────────────────────────────────────────────────
        "COD_TVA_INVALID" | "VAT_CODE_INVALID" =>
            "Codul de categorie TVA este invalid. Valorile acceptate: S (standard), Z (zero), E (scutit), AE (taxare inversă).",
        "COTA_TVA_INVALIDA" | "VAT_RATE_INVALID" =>
            "Cota TVA este invalidă pentru categoria selectată. Cotele acceptate în RO: 19%, 9%, 5%, 0%.",
        "TVA_INEXISTENT" | "VAT_RATE_MISSING" =>
            "Lipsește cota TVA pe o linie. Completați vat_rate pentru fiecare produs/serviciu.",
        "SCUTIRE_TVA" | "VAT_EXEMPTION_REASON" =>
            "Motivul scutirii de TVA lipsește. Adăugați TaxExemptionReasonCode și TaxExemptionReason.",

        // ── Erori număr factură ────────────────────────────────────────────
        "NUMAR_DUPLICAT" | "DUPLICATE_INVOICE" =>
            "Numărul facturii a mai fost transmis. Verificați seria și numărul — nu se poate trimite aceeași factură de două ori.",
        "SERIE_INVALIDA" | "SERIES_INVALID" =>
            "Seria facturii conține caractere nevalide. Folosiți doar litere și cifre.",
        "NUMAR_INVALID" | "NUMBER_INVALID" =>
            "Numărul facturii este invalid sau lipsește.",

        // ── Erori furnizor / cumpărător ────────────────────────────────────
        "FURNIZOR_LIPSA" | "SUPPLIER_MISSING" =>
            "Informațiile furnizorului (AccountingSupplierParty) lipsesc sau sunt incomplete.",
        "CUMPARATOR_LIPSA" | "CUSTOMER_MISSING" =>
            "Informațiile cumpărătorului (AccountingCustomerParty) lipsesc sau sunt incomplete.",
        "ADRESA_LIPSA" | "ADDRESS_MISSING" =>
            "Adresa furnizorului sau cumpărătorului lipsește. Completați câmpurile obligatorii.",
        "IBAN_INVALID" =>
            "IBAN-ul furnizorului este invalid. Verificați formatul (RO + 22 caractere alfanumerice).",

        // ── Erori linii factură ────────────────────────────────────────────
        "LINII_LIPSA" | "NO_LINES" =>
            "Factura nu conține nicio linie de produse/servicii. Adăugați cel puțin o linie.",
        "UNITATE_MASURA" | "UNIT_INVALID" =>
            "Unitatea de măsură nu este un cod UN/ECE valid. Folosiți: BUC, KGM, HUR, MON, MTR etc.",
        "CANTITATE_NEGATIVA" | "QUANTITY_NEGATIVE" =>
            "Cantitățile negative nu sunt permise pe facturile normale.",
        "PRET_NEGATIV" | "PRICE_NEGATIVE" =>
            "Prețul unitar negativ nu este permis. Folosiți factura de storno pentru reduceri.",
        "DESCRIERE_LIPSA" | "DESCRIPTION_MISSING" =>
            "Descrierea produsului/serviciului lipsește de pe o linie.",

        // ── Erori mijloc de plată ──────────────────────────────────────────
        "MIJLOC_PLATA" | "PAYMENT_MEANS_MISSING" =>
            "Mijlocul de plată (PaymentMeans) lipsește. Adăugați codul: 30 (transfer), 10 (numerar), 48 (card).",
        "COD_PLATA_INVALID" | "PAYMENT_CODE_INVALID" =>
            "Codul mijlocului de plată este invalid. Valorile acceptate: 10 (numerar), 30 (transfer bancar), 48 (card).",

        // ── Erori certificate / autentificare ──────────────────────────────
        "TOKEN_EXPIRAT" | "TOKEN_EXPIRED" =>
            "Token-ul de autentificare a expirat. Reautentificați-vă din Setări → Certificate.",
        "CERTIFICAT_INVALID" | "CERT_INVALID" =>
            "Certificatul digital este invalid sau expirat. Obțineți un certificat nou de pe portalul ANAF.",
        "CERTIFICAT_EXPIRAT" | "CERT_EXPIRED" =>
            "Certificatul digital a expirat. Reînnoire necesară prin portalul ANAF SPV.",
        "FIRMA_NECERTIFICATA" | "COMPANY_NOT_CERTIFIED" =>
            "Firma nu este înregistrată pentru e-Factura cu acest certificat. Verificați portalul ANAF.",

        // ── Erori sistem ANAF ──────────────────────────────────────────────
        "SISTEM_INDISPONIBIL" | "SYSTEM_UNAVAILABLE" =>
            "Sistemul ANAF este temporar indisponibil. Reîncercați în 30 de minute.",
        "DIMENSIUNE_DEPASITA" | "FILE_TOO_LARGE" =>
            "Fișierul XML depășește dimensiunea maximă permisă de ANAF (10 MB). Reduceți numărul de linii.",
        "FORMAT_INVALID" | "INVALID_FORMAT" =>
            "Formatul fișierului este invalid. ANAF acceptă doar XML UBL 2.1.",
        "CHARSET_INVALID" | "ENCODING_INVALID" =>
            "Codificarea caracterelor este incorectă. Fișierul trebuie să fie UTF-8 cu BOM.",

        // ── Erori storno ───────────────────────────────────────────────────
        "FACTURA_STORNO_INVALIDA" | "STORNO_INVALID" =>
            "Factura de storno referă o factură inexistentă sau deja stornată.",
        "REFERINTA_STORNO" | "STORNO_REF_MISSING" =>
            "Factura de storno trebuie să conțină referința la factura originală (BillingReference).",

        // ── Erori diverse ──────────────────────────────────────────────────
        "MONEDA_NECONCORDANTA" | "CURRENCY_MISMATCH" =>
            "Moneda nu este consistentă în toată factura. Folosiți același cod de monedă pretutindeni.",
        "PROCENT_DISCOUNT" | "DISCOUNT_PERCENT" =>
            "Procentul de discount depășește 100% sau este negativ.",
        "REFERINTA_COMANDA" | "ORDER_REF_INVALID" =>
            "Referința la comandă (OrderReference) este invalidă sau lipsește identificatorul.",
        "TARA_INVALIDA" | "COUNTRY_INVALID" =>
            "Codul de țară este invalid. Folosiți codul ISO 3166-1 alpha-2 (RO, DE, FR etc.).",
        "COD_POSTAL" | "POSTAL_CODE_INVALID" =>
            "Codul poștal este în format incorect.",
        "EMAIL_INVALID" =>
            "Adresa de email este invalidă.",

        // ── Fallback ───────────────────────────────────────────────────────
        _ => "Eroare necunoscută ANAF. Consultați documentația ANAF e-Factura pentru detalii.",
    }
}

/// Extrage codul de eroare dintr-un body de răspuns ANAF XML sau text.
pub fn extract_error_code(response_body: &str) -> Option<String> {
    // Try XML format: <Errors errorMessage="XXXXX">
    if let Some(start) = response_body.find("errorMessage=\"") {
        let rest = &response_body[start + 14..];
        if let Some(end) = rest.find('"') {
            return Some(rest[..end].to_uppercase());
        }
    }
    // Try JSON format: {"eroare":"XXXXX"}
    if let Some(start) = response_body.find("\"eroare\":\"") {
        let rest = &response_body[start + 10..];
        if let Some(end) = rest.find('"') {
            return Some(rest[..end].to_uppercase());
        }
    }
    None
}

/// Returnează mesajul prietenos direct din body-ul răspunsului ANAF.
pub fn friendly_message_from_body(response_body: &str) -> String {
    if let Some(code) = extract_error_code(response_body) {
        let msg = lookup_anaf_error(&code);
        if !msg.starts_with("Eroare necunoscută") {
            return msg.to_string();
        }
    }
    // Fallback: trimite body-ul scurt dacă nu recunoaștem codul
    let short: String = response_body.chars().take(200).collect();
    format!("Eroare ANAF: {short}")
}
