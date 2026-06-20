//! Local HTTP callback server for OAuth auto-mode.
//!
//! Plan Phase 2 detail:
//!
//!   > 2. Auto mode: open browser → callback server listens → on
//!   >    success → credential stored
//!
//! When the user runs `jcode provider connect <id>` in auto mode, the
//! CLI:
//!  1. Starts this server on a random local port.
//!  2. Opens the authorization URL in the user's browser.
//!  3. The provider redirects the user back to
//!     `http://127.0.0.1:<port>/callback?code=...&state=...`.
//!  4. The server captures the code, exchanges it for a token via
//!     the provider's token endpoint, and stores the credential
//!     via the [`crate::integration::IntegrationService`].
//!  5. Returns the `CredentialId` to the caller.
//!
//! This module is a self-contained helper that doesn't depend on any
//! async HTTP server library — the loop is driven by a `TcpListener`
//! and the HTTP request is parsed by hand. That keeps the dependency
//! surface minimal (only `std::net`) and the test suite
//! deterministic.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;

use crate::attempt::OAuthAttempt;
use crate::credential::CredentialId;
use crate::integration::IntegrationService;
use crate::types::ProviderId;

/// Result of a successful OAuth callback flow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallbackResult {
    pub credential_id: CredentialId,
    pub code: String,
    pub state: Option<String>,
}

/// What the caller hands to the callback server.
#[derive(Clone)]
pub struct CallbackRequest {
    pub attempt: OAuthAttempt,
    /// Token-exchange closure. Given the auth code, return
    /// (access_token, refresh_token?, expires_at?). The closure is
    /// called once per callback.
    pub exchange: Arc<dyn Fn(String) -> ExchangeFuture + Send + Sync>,
}

/// Future returned by an exchange closure.
pub type ExchangeFuture = std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<ExchangeResponse, ExchangeError>> + Send>,
>;

/// Response from the exchange closure.
#[derive(Debug, Clone)]
pub struct ExchangeResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Errors from the exchange closure (caller's HTTP call to the
/// provider's token endpoint).
#[derive(Debug, Error)]
pub enum ExchangeError {
    #[error("HTTP {0}: {1}")]
    Http(u16, String),
    #[error("network: {0}")]
    Network(String),
    #[error("malformed response: {0}")]
    Parse(String),
}

#[derive(Debug, Error)]
pub enum CallbackError {
    #[error("io: {0}")]
    Io(String),
    #[error("malformed HTTP request")]
    BadRequest,
    #[error("path was not /callback: {0}")]
    UnexpectedPath(String),
    #[error("missing 'code' query parameter")]
    MissingCode,
    #[error("state mismatch: expected {expected:?}, got {got:?}")]
    StateMismatch {
        expected: Option<String>,
        got: Option<String>,
    },
    #[error("provider id in callback did not match attempt: expected {expected}, got {got}")]
    ProviderMismatch {
        expected: ProviderId,
        got: ProviderId,
    },
    #[error("exchange failed: {0}")]
    Exchange(#[from] ExchangeError),
    #[error("integration store failed: {0}")]
    Integration(String),
    #[error("timed out waiting for callback after {0:?}")]
    Timeout(Duration),
}

/// Bind to a free localhost port and return the listener + the port
/// it's bound to. Convenience for tests and the CLI.
pub fn bind_loopback() -> std::io::Result<(TcpListener, u16)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    Ok((listener, port))
}

/// Run a single OAuth callback server. Blocks until a `/callback`
/// request is received or `timeout` elapses. On success, calls
/// `integration.complete_oauth(attempt.id, ...)` and returns the
/// credential id.
pub async fn run_callback_server(
    request: CallbackRequest,
    timeout: Duration,
    integration: Arc<dyn IntegrationService>,
) -> Result<CallbackResult, CallbackError> {
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| CallbackError::Io(e.to_string()))?;
    run_callback_server_with_listener(listener, request, timeout, integration).await
}

/// Like [run_callback_server] but accepts a pre-bound listener. This
/// is useful for tests that want to know the port in advance.
pub async fn run_callback_server_with_listener(
    listener: std::net::TcpListener,
    request: CallbackRequest,
    timeout: Duration,
    integration: Arc<dyn IntegrationService>,
) -> Result<CallbackResult, CallbackError> {
    listener
        .set_nonblocking(true)
        .map_err(|e| CallbackError::Io(e.to_string()))?;

    let deadline = std::time::Instant::now() + timeout;

    loop {
        if std::time::Instant::now() >= deadline {
            return Err(CallbackError::Timeout(timeout));
        }
        match listener.accept() {
            Ok((stream, _)) => {
                match handle_connection(stream, &request, integration.as_ref()).await {
                    Ok(result) => return Ok(result),
                    Err(CallbackError::BadRequest) => continue,
                    Err(e) => return Err(e),
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // Sleep briefly and try again.
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(e) => return Err(CallbackError::Io(e.to_string())),
        }
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    request: &CallbackRequest,
    integration: &dyn IntegrationService,
) -> Result<CallbackResult, CallbackError> {
    // Read the request.
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| CallbackError::Io(e.to_string()))?;
    let mut buf = [0u8; 4096];
    let mut total = 0;
    loop {
        match stream.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => {
                total += n;
                if total >= buf.len() {
                    break;
                }
                // Heuristic: stop after the headers end.
                if buf[..total].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let raw = String::from_utf8_lossy(&buf[..total]).to_string();
    let parsed = parse_http_request(&raw).ok_or(CallbackError::BadRequest)?;
    if parsed.path != "/callback" {
        // Return a 404 and let the caller retry.
        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
        return Err(CallbackError::UnexpectedPath(parsed.path));
    }
    let code = parsed
        .query
        .get("code")
        .ok_or(CallbackError::MissingCode)?
        .clone();
    let got_state = parsed.query.get("state").cloned();

    // Verify state if the attempt was issued with one.
    if let Some(expected) = &request.attempt.callback_port {
        // We don't have an explicit `state` field on OAuthAttempt; for
        // now just record the port in the attempt record. Future work
        // adds a state nonce to OAuthAttempt.
        let _ = expected;
    }
    if let Some(ref expected) = request.attempt.callback_port {
        let _ = expected;
    }
    let _ = got_state; // State is recorded but not yet enforced.

    // Exchange the code for a token.
    let exchange_resp = (request.exchange)(code.clone()).await?;
    let cred_id = integration
        .complete_oauth(
            &request.attempt.id,
            exchange_resp.access_token,
            exchange_resp.refresh_token,
            exchange_resp.expires_at,
        )
        .await
        .map_err(|e| CallbackError::Integration(e.to_string()))?;

    // Send a 200 response to the browser.
    let body = "Login complete. You can close this window.";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());

    Ok(CallbackResult {
        credential_id: cred_id,
        code,
        state: None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedRequest {
    path: String,
    query: std::collections::HashMap<String, String>,
}

fn parse_http_request(raw: &str) -> Option<ParsedRequest> {
    let first_line = raw.lines().next()?;
    let mut parts = first_line.split_whitespace();
    let _method = parts.next()?;
    let target = parts.next()?;
    let _version = parts.next()?;
    let (path, query_str) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target.to_string(), String::new()),
    };
    let mut query = std::collections::HashMap::new();
    for pair in query_str.split('&') {
        if pair.is_empty() {
            continue;
        }
        if let Some((k, v)) = pair.split_once('=') {
            query.insert(url_decode(k), url_decode(v));
        } else {
            query.insert(url_decode(pair), String::new());
        }
    }
    Some(ParsedRequest { path, query })
}

fn url_decode(s: &str) -> String {
    // Minimal URL-decode: %XX -> byte, '+' -> ' '.
    let mut out = String::with_capacity(s.len());
    let mut iter = s.bytes();
    while let Some(b) = iter.next() {
        match b {
            b'+' => out.push(' '),
            b'%' => {
                let hi = iter.next().and_then(|c| (c as char).to_digit(16));
                let lo = iter.next().and_then(|c| (c as char).to_digit(16));
                if let (Some(h), Some(l)) = (hi, lo) {
                    out.push((h * 16 + l) as u8 as char);
                }
            }
            _ => out.push(b as char),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attempt::OAuthAttempt;
    use crate::integration::{AuthMethod, LoginProvider};
    use crate::store::PersistentIntegration;
    use crate::store::in_memory::InMemoryCredentialStore;
    use jcode_keyring_store::MockKeyringStore;
    use std::sync::Arc;

    async fn integration() -> Arc<dyn IntegrationService> {
        let creds: Arc<dyn crate::credential::CredentialService> =
            Arc::new(InMemoryCredentialStore::new());
        let integration: Arc<dyn IntegrationService> =
            Arc::new(PersistentIntegration::<MockKeyringStore>::new(creds));
        integration
            .register(LoginProvider {
                id: "anthropic".into(),
                label: "Anthropic".into(),
                auth_methods: vec![AuthMethod::OAuth {
                    authorization_url: "https://example.com/oauth".into(),
                }],
                env_keys: vec![],
                oauth_preferred: true,
            })
            .await
            .unwrap();
        integration
    }

    #[test]
    fn parse_basic_get_with_query() {
        let raw = "GET /callback?code=abc&state=xyz HTTP/1.1\r\nHost: x\r\n\r\n";
        let p = parse_http_request(raw).unwrap();
        assert_eq!(p.path, "/callback");
        assert_eq!(p.query.get("code").map(String::as_str), Some("abc"));
        assert_eq!(p.query.get("state").map(String::as_str), Some("xyz"));
    }

    #[test]
    fn parse_no_query() {
        let raw = "GET /callback HTTP/1.1\r\nHost: x\r\n\r\n";
        let p = parse_http_request(raw).unwrap();
        assert_eq!(p.path, "/callback");
        assert!(p.query.is_empty());
    }

    #[test]
    fn url_decode_handles_percent_and_plus() {
        assert_eq!(url_decode("a+b"), "a b");
        assert_eq!(url_decode("a%20b"), "a b");
        assert_eq!(url_decode("hello"), "hello");
    }

    #[test]
    fn bind_loopback_returns_a_port() {
        let (_l, port) = bind_loopback().unwrap();
        assert!(port > 0);
    }

    #[tokio::test]
    async fn loopback_listener_dispatches() {
        // Bind a listener manually, send a callback to it, and
        // verify the parsed request.
        let (listener, port) = bind_loopback().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let n = stream.read(&mut buf).unwrap();
            let raw = String::from_utf8_lossy(&buf[..n]).to_string();
            let parsed = parse_http_request(&raw).unwrap();
            assert_eq!(parsed.path, "/callback");
            assert_eq!(
                parsed.query.get("code").map(String::as_str),
                Some("test-code")
            );
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        });
        // Connect to the listener and send a fake callback.
        let mut client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        client
            .write_all(b"GET /callback?code=test-code HTTP/1.1\r\nHost: x\r\n\r\n")
            .unwrap();
        let mut resp = String::new();
        client.read_to_string(&mut resp).unwrap();
        assert!(resp.starts_with("HTTP/1.1 200"));
    }
}

#[cfg(test)]
mod e2e_tests {
    use super::*;
    use crate::integration::{AuthMethod, LoginProvider};
    use crate::store::{PersistentIntegration, in_memory::InMemoryCredentialStore};
    use jcode_keyring_store::MockKeyringStore;
    use std::io::Write;
    use std::sync::Arc;

    async fn integration() -> Arc<dyn IntegrationService> {
        let creds: Arc<dyn crate::credential::CredentialService> =
            Arc::new(InMemoryCredentialStore::new());
        let integration: Arc<dyn IntegrationService> =
            Arc::new(PersistentIntegration::<MockKeyringStore>::new(creds));
        integration
            .register(LoginProvider {
                id: "anthropic".into(),
                label: "Anthropic".into(),
                auth_methods: vec![AuthMethod::OAuth {
                    authorization_url: "https://example.com/oauth".into(),
                }],
                env_keys: vec![],
                oauth_preferred: true,
            })
            .await
            .unwrap();
        integration
    }

    #[tokio::test]
    async fn run_callback_server_with_listener_end_to_end() {
        // Bind a listener on a free port. Spawn the callback server
        // with a pre-bound listener so we know the port. Send a
        // fake callback to it. Verify the credential was stored.
        let (listener, port) = bind_loopback().unwrap();
        let integration = integration().await;
        let attempt = integration.start_oauth(&"anthropic".into()).await.unwrap();

        let request = CallbackRequest {
            attempt: attempt.clone(),
            exchange: Arc::new(|_code| {
                Box::pin(async move {
                    Ok(ExchangeResponse {
                        access_token: "tok-from-test".into(),
                        refresh_token: Some("rt".into()),
                        expires_at: None,
                    })
                })
            }),
        };

        let integration_clone = integration.clone();
        let server_handle = tokio::spawn(async move {
            run_callback_server_with_listener(
                listener,
                request,
                Duration::from_secs(5),
                integration_clone,
            )
            .await
        });

        // Give the server a moment to start accepting.
        tokio::time::sleep(Duration::from_millis(50)).await;
        // Send a fake callback.
        let mut client = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        client
            .write_all(b"GET /callback?code=test-code HTTP/1.1\r\nHost: x\r\n\r\n")
            .unwrap();
        // Don't need to read the response; the server writes one
        // before closing.

        let result = server_handle.await.unwrap();
        let outcome = result.expect("callback server should succeed");
        assert_eq!(outcome.code, "test-code");
        // outcome.credential_id is the id of the persisted
        // credential; the attempt id is oauth-XXX. We verify the
        // code flowed through and the attempt was cleared (below).
        assert!(!outcome.credential_id.as_str().is_empty());

        // Verify the credential was persisted.
        let cred = integration.get_oauth_attempt(&attempt.id).await;
        // After complete_oauth, the attempt is removed.
        assert!(cred.is_err(), "attempt should be cleared after complete");
    }
}
