//! Stocare token-uri OAuth2 în OS keychain.
//!
//! Token bundle-ul (access + refresh + expiry) e serializat ca JSON și stocat
//! sub cheia "efactura::{company_id}" în keychain-ul sistemului.

use keyring::Entry;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBundle {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix timestamp când expiră access_token-ul.
    pub expires_at: i64,
}

impl TokenBundle {
    /// Salvează bundle-ul în OS keychain sub "efactura::{company_id}".
    pub fn save(&self, company_id: &str) -> Result<(), keyring::Error> {
        let entry = Entry::new("efactura", company_id)?;
        let json = serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string());
        entry.set_password(&json)
    }

    /// Încarcă bundle-ul din OS keychain. Returnează `None` dacă nu există
    /// sau dacă JSON-ul e corupt.
    pub fn load(company_id: &str) -> Option<TokenBundle> {
        let entry = Entry::new("efactura", company_id).ok()?;
        let json = entry.get_password().ok()?;
        serde_json::from_str(&json).ok()
    }

    /// Șterge token-ul din keychain (logout / revocare).
    pub fn delete(company_id: &str) {
        if let Ok(entry) = Entry::new("efactura", company_id) {
            let _ = entry.delete_credential();
        }
    }

    /// `true` dacă access_token-ul expiră în mai puțin de 60 de secunde.
    pub fn is_expired(&self) -> bool {
        self.expires_at <= chrono::Utc::now().timestamp() + 60
    }
}
