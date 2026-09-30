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

/// Read a provider response without trusting Content-Length or the transfer encoding.
/// The limit applies to decoded body bytes as they arrive, including chunked replies.
#[derive(Debug)]
pub enum ResponseBodyError {
    TooLarge,
    Read(reqwest::Error),
}

pub async fn read_bounded_body(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, ResponseBodyError> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(ResponseBodyError::TooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(ResponseBodyError::Read)? {
        if chunk.len() > limit.saturating_sub(body.len()) {
            return Err(ResponseBodyError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn serve(response: Vec<u8>) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(&response);
        });
        format!("http://{addr}/response")
    }

    #[tokio::test]
    async fn bounded_reader_rejects_declared_and_chunked_oversize_then_accepts_next_request() {
        let client = client();
        let declared = serve(
            b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nabcdef".to_vec(),
        );
        let response = client.get(declared).send().await.unwrap();
        assert!(matches!(
            read_bounded_body(response, 3).await,
            Err(ResponseBodyError::TooLarge)
        ));

        let chunked = serve(b"HTTP/1.1 500 Error\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n6\r\nabcdef\r\n0\r\n\r\n".to_vec());
        let response = client.get(chunked).send().await.unwrap();
        assert!(matches!(
            read_bounded_body(response, 3).await,
            Err(ResponseBodyError::TooLarge)
        ));

        let valid =
            serve(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc".to_vec());
        let response = client.get(valid).send().await.unwrap();
        assert_eq!(read_bounded_body(response, 3).await.unwrap(), b"abc");
    }
}
