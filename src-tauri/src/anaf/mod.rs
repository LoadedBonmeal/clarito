//! Integrare ANAF — OAuth2 PKCE + e-Factura REST API.
//!
//! Submodule:
//! - `keychain` — stocare token-uri în OS keychain
//! - `oauth`    — flux OAuth2 PKCE (browser redirect + local TCP listener)
//! - `client`   — client HTTP pentru endpoint-urile ANAF e-Factura
//! - `errors`   — mapare coduri erori ANAF → mesaje în română

pub mod client;
pub mod errors;
pub mod keychain;
pub mod oauth;
