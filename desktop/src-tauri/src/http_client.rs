//! Every HTTP client in the app is built here.
//!
//! reqwest comes without a crypto provider of its own, the way
//! tauri-plugin-updater builds it, so the binary carries one reqwest and one
//! rustls backend (ring). The provider is installed process-wide before the
//! first client exists; certificates are checked against the system store.

/// A client builder with the crypto provider in place.
pub fn builder() -> reqwest::ClientBuilder {
    static PROVIDER: std::sync::Once = std::sync::Once::new();
    PROVIDER.call_once(|| {
        // Err means another component installed one first, which serves too.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
    reqwest::Client::builder()
}

/// A client with default settings.
pub fn client() -> reqwest::Client {
    builder().build().expect("default HTTP client")
}
