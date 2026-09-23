//! Mock HTTP tests for the cloud STT provider.
//!
//! Stands up a local `TcpListener` that mimics an OpenAI-compatible
//! `/audio/transcriptions` endpoint, then drives `cloud_stt::transcribe`
//! against it. Verifies:
//!   - the multipart/form-data body has the right shape
//!   - WAV bytes are produced and embedded in the `file` field
//!   - the `Authorization: Bearer <key>` header is set
//!   - a non-2xx response surfaces as a typed error
//!   - a malformed response surfaces as a typed error
//!   - a successful response parses `text` out of JSON

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use sotto_lib::cloud_stt::{
    audio_to_wav_bytes, transcribe, transcribe_cancellable, CloudSttProvider, CloudSttRequest,
};

struct DelayedServer {
    url: String,
    requests: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for DelayedServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn delayed_server(delay: Duration) -> DelayedServer {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(AtomicUsize::new(0));
    let count = requests.clone();
    // The task owns all response tasks so dropping the fixture cancels them too.
    let task = tokio::spawn(async move {
        let mut responses = tokio::task::JoinSet::new();
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let count = count.clone();
            responses.spawn(async move {
                let mut request = Vec::new();
                loop {
                    let mut chunk = [0; 4096];
                    let size = stream.read(&mut chunk).await.unwrap_or(0);
                    if size == 0 {
                        return;
                    }
                    request.extend_from_slice(&chunk[..size]);
                    if let Some(end) = find_double_crlf(&request).map(|end| end + 4) {
                        let length = String::from_utf8_lossy(&request[..end])
                            .lines()
                            .find_map(|line| {
                                let (key, value) = line.split_once(':')?;
                                key.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if request.len() >= end + length {
                            break;
                        }
                    }
                }
                count.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(delay).await;
                let body = r#"{"text":"delayed result"}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });
    DelayedServer {
        url,
        requests,
        task,
    }
}

#[tokio::test]
async fn engine_cloud_request_survives_multiple_cancel_polls_without_reposting() {
    let server = delayed_server(Duration::from_millis(350)).await;
    let cancel = AtomicBool::new(false);
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        transcribe_cancellable(sample_request(server.url.clone()), &cancel),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result.text, "delayed result");
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn engine_cloud_timeout_is_one_total_request_budget() {
    let server = delayed_server(Duration::from_secs(10)).await;
    let mut request = sample_request(server.url.clone());
    request.timeout_seconds = 1;
    let cancel = AtomicBool::new(false);
    let error = tokio::time::timeout(
        Duration::from_secs(3),
        transcribe_cancellable(request, &cancel),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert!(error.contains("timeout"), "{error}");
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn engine_cloud_cancellation_drops_stalled_request_without_waiting_for_response() {
    let server = delayed_server(Duration::from_secs(10)).await;
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let request = sample_request(server.url.clone());
    let task = tokio::spawn(async move { transcribe_cancellable(request, &flag).await });
    tokio::time::timeout(Duration::from_secs(3), async {
        while server.requests.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    cancel.store(true, Ordering::SeqCst);
    let error = tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(error.contains("cancelled"), "{error}");
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cancelled_cloud_request_does_not_upload() {
    let server = delayed_server(Duration::ZERO).await;
    let cancel = AtomicBool::new(true);
    assert!(
        transcribe_cancellable(sample_request(server.url.clone()), &cancel)
            .await
            .unwrap_err()
            .contains("cancelled")
    );
    assert_eq!(server.requests.load(Ordering::SeqCst), 0);
}

#[derive(Default, Debug)]
struct Captured {
    method: String,
    path: String,
    auth_header: Option<String>,
    content_type: Option<String>,
    body: Vec<u8>,
}

type Shared = Arc<Mutex<Captured>>;

fn spawn_mock(response_status_line: &'static str, response_body: &'static str) -> (String, Shared) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let captured: Shared = Arc::new(Mutex::new(Captured::default()));
    let captured_thread = Arc::clone(&captured);
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buffer = Vec::new();
        let mut temp = [0_u8; 4096];
        // Read until we have headers AND the full body, per Content-Length.
        let mut header_end = None;
        loop {
            let n = match stream.read(&mut temp) {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => break,
            };
            buffer.extend_from_slice(&temp[..n]);
            if header_end.is_none() {
                if let Some(idx) = find_double_crlf(&buffer) {
                    header_end = Some(idx + 4);
                }
            }
            if let Some(end) = header_end {
                let headers = std::str::from_utf8(&buffer[..end]).unwrap_or("");
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let lower = line.to_ascii_lowercase();
                        if lower.starts_with("content-length:") {
                            lower
                                .trim_start_matches("content-length:")
                                .trim()
                                .parse::<usize>()
                                .ok()
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0);
                if buffer.len() >= end + content_length {
                    break;
                }
            }
        }

        let (headers, body) = match header_end {
            Some(end) => buffer.split_at(end),
            None => (&buffer[..0], &buffer[..0]),
        };

        let mut cap = captured_thread.lock().unwrap();
        let header_str = std::str::from_utf8(headers).unwrap_or("");
        cap.method = header_str
            .lines()
            .next()
            .unwrap_or("")
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string();
        cap.path = header_str
            .lines()
            .next()
            .unwrap_or("")
            .split_whitespace()
            .nth(1)
            .unwrap_or("")
            .to_string();
        cap.auth_header = header_str
            .lines()
            .find(|line| line.to_ascii_lowercase().starts_with("authorization:"))
            .map(|line| line.trim().to_string());
        cap.content_type = header_str
            .lines()
            .find(|line| line.to_ascii_lowercase().starts_with("content-type:"))
            .map(|line| line.trim().to_string());
        cap.body = body.to_vec();

        let response = format!(
            "{response_status_line}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{response_body}",
            response_body.len(),
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
    });
    (format!("http://{addr}"), captured)
}

fn find_double_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn sample_request(base_url: String) -> CloudSttRequest {
    CloudSttRequest {
        provider: CloudSttProvider::Compatible,
        base_url,
        api_key: "sk-test-key".into(),
        model: "whisper-large-v3-turbo".into(),
        language: Some("ru".into()),
        audio: Arc::new(vec![0.0_f32; 1600]),
        timeout_seconds: 5,
    }
}

#[tokio::test]
async fn transcribe_sends_multipart_with_wav_file_and_parses_text() {
    let (base_url, captured) = spawn_mock("HTTP/1.1 200 OK", r#"{"text": "hello world"}"#);
    let result = transcribe(sample_request(base_url)).await.unwrap();
    assert_eq!(result.text, "hello world");
    assert_eq!(result.model, "whisper-large-v3-turbo");
    let cap = captured.lock().unwrap();
    assert_eq!(cap.method, "POST");
    assert_eq!(cap.path, "/audio/transcriptions");
    assert_eq!(
        cap.auth_header
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("authorization: bearer sk-test-key")
    );
    let content_type = cap.content_type.as_deref().unwrap_or("");
    assert!(
        content_type
            .to_ascii_lowercase()
            .contains("multipart/form-data; boundary="),
        "expected multipart content-type, got: {content_type:?}"
    );
    let body_str = String::from_utf8_lossy(&cap.body);
    assert!(body_str.contains("name=\"model\""));
    assert!(body_str.contains("whisper-large-v3-turbo"));
    assert!(body_str.contains("name=\"language\""));
    assert!(body_str.contains("\r\nru\r\n"));
    assert!(body_str.contains("name=\"file\"; filename=\"recording.wav\""));
    // The WAV bytes are embedded between the file headers and the
    // closing boundary.
    let wav_start = body_str.find("Content-Type: audio/wav\r\n\r\n").unwrap()
        + "Content-Type: audio/wav\r\n\r\n".len();
    let wav_end = body_str[wav_start..]
        .find("\r\n------")
        .map(|idx| wav_start + idx)
        .expect("closing boundary after WAV");
    let wav_bytes = &cap.body[wav_start..wav_end];
    assert!(wav_bytes.starts_with(b"RIFF"));
    assert!(wav_bytes.windows(4).any(|w| w == b"WAVE"));
}

#[tokio::test]
async fn transcribe_propagates_4xx_as_http_error() {
    let (base_url, _captured) = spawn_mock(
        "HTTP/1.1 401 Unauthorized",
        r#"{"error": "missing api key"}"#,
    );
    let err = transcribe(sample_request(base_url)).await.unwrap_err();
    assert!(err.contains("http 401"), "got: {err}");
    // The error reaches the overlay and history; the provider's body does not.
    assert!(!err.contains("missing api key"), "got: {err}");
}

#[tokio::test]
async fn transcribe_propagates_500_as_http_error() {
    let (base_url, _captured) = spawn_mock("HTTP/1.1 500 Internal Server Error", r#"{}"#);
    let err = transcribe(sample_request(base_url)).await.unwrap_err();
    assert!(err.contains("http 500"), "got: {err}");
}

#[tokio::test]
async fn transcribe_propagates_malformed_json() {
    let (base_url, _captured) = spawn_mock("HTTP/1.1 200 OK", "not json");
    let err = transcribe(sample_request(base_url)).await.unwrap_err();
    assert!(err.starts_with("malformed:"), "got: {err}");
}

#[tokio::test]
async fn transcribe_propagates_missing_text_field() {
    let (base_url, _captured) = spawn_mock("HTTP/1.1 200 OK", r#"{"result": "hi"}"#);
    let err = transcribe(sample_request(base_url)).await.unwrap_err();
    assert!(err.starts_with("malformed:"), "got: {err}");
    assert!(err.contains("missing 'text'"));
    assert!(
        !err.contains("hi\""),
        "the response leaked into the error: {err}"
    );
}

#[tokio::test]
async fn transcribe_timeout_when_server_hangs() {
    // Server that accepts the connection but never replies.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        // Accept but never write a response. Drop the stream after a
        // beat so the client sees the EOF and the timeout (not a
        // connection-reset error) drives the failure.
        let (_stream, _) = listener.accept().unwrap();
        thread::sleep(Duration::from_secs(8));
    });

    let mut req = sample_request(format!("http://{addr}"));
    req.timeout_seconds = 2;
    let err = transcribe(req).await.unwrap_err();
    assert!(err.contains("timeout after 2s"), "got: {err}");
}

#[test]
fn audio_to_wav_bytes_round_trips_silence() {
    let wav = audio_to_wav_bytes(&[0.0_f32; 1600]);
    // 44-byte header + 1600 * 2 bytes of zero PCM
    assert_eq!(wav.len(), 44 + 3200);
}

/// The bearer token is safe with reqwest's defaults; the policy is asserted
/// here so a later change to the shared client cannot quietly relax it.
#[tokio::test]
async fn transcribe_does_not_follow_a_redirect_to_another_host() {
    let (other_host, other_captured) = spawn_mock("HTTP/1.1 200 OK", r#"{"text": "stolen"}"#);
    let other_port = other_host.rsplit(':').next().unwrap();
    let redirect: &'static str = Box::leak(
        format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: http://localhost:{other_port}/audio/transcriptions")
            .into_boxed_str(),
    );
    let (base_url, _) = spawn_mock(redirect, "");

    let err = transcribe(sample_request(base_url)).await.unwrap_err();

    assert!(err.contains("http 307"), "got: {err}");
    assert!(
        other_captured.lock().unwrap().body.is_empty(),
        "the audio reached another host"
    );
}
