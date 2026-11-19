//! Connecting to one server: running its command, or reaching its URL, with its secrets filled in.

use std::collections::HashMap;
use std::sync::Arc;

use patchbay_core::manifest::{Auth, Launch, MissingSecret, Server};
use patchbay_core::secrets::Secrets;
use rmcp::service::RunningService;
use rmcp::transport::auth::AuthClient;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
use rmcp::{RoleClient, ServiceExt};

use patchbay_core::path;

use crate::oauth;

/// A running connection to a server.
pub type Client = RunningService<RoleClient, ()>;

/// Why a server isn't connected.
#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error("it needs the secret {}", .0.0)]
    Missing(MissingSecret),
    #[error("it needs you to sign in")]
    SignIn,
    #[error("it uses the old SSE transport, which Patchbay doesn't reach yet")]
    Sse,
    #[error("couldn't start it: {0}")]
    Spawn(std::io::Error),
    #[error("{0} isn't a header name or value")]
    BadHeader(String),
    #[error("the server turned its key away; change the key")]
    KeyRefused,
    #[error("it didn't answer: {0}")]
    Handshake(String),
}

impl From<MissingSecret> for ConnectError {
    fn from(missing: MissingSecret) -> ConnectError {
        ConnectError::Missing(missing)
    }
}

/// Connects to `server`, reading its secrets and tokens from `secrets`.
pub async fn connect(server: &Server, secrets: &Arc<dyn Secrets>) -> Result<Client, ConnectError> {
    let lookup = |account: &str| secrets.get(account);
    match &server.launch {
        Launch::Stdio {
            command,
            args,
            env,
            cwd,
        } => {
            let mut process = tokio::process::Command::new(command);
            for arg in args {
                process.arg(arg.render(lookup)?);
            }
            process.env("PATH", path::login_path());
            for (key, value) in env {
                process.env(key, value.render(lookup)?);
            }
            if let Some(cwd) = cwd {
                process.current_dir(cwd);
            }
            // Servers log to stderr; keeping it would fill a pipe nobody reads.
            let (transport, _) = TokioChildProcess::builder(process)
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(ConnectError::Spawn)?;
            ().serve(transport)
                .await
                .map_err(|error| ConnectError::Handshake(plain(&error.to_string())))
        }
        Launch::Http {
            url,
            sse,
            headers,
            auth,
        } => {
            if *sse {
                return Err(ConnectError::Sse);
            }
            let mut config = StreamableHttpClientTransportConfig::with_uri(url.as_str());
            let mut custom = HashMap::new();
            for (key, value) in headers {
                let value = value.render(lookup)?;
                if key.eq_ignore_ascii_case("authorization")
                    && let Some(token) = value.strip_prefix("Bearer ")
                {
                    config = config.auth_header(token.to_string());
                    continue;
                }
                let name = http::HeaderName::try_from(key.as_str())
                    .map_err(|_| ConnectError::BadHeader(key.clone()))?;
                let value = http::HeaderValue::try_from(value)
                    .map_err(|_| ConnectError::BadHeader(key.clone()))?;
                custom.insert(name, value);
            }
            config = config.custom_headers(custom);
            // A server with no `auth` that was signed in to after it asked is reached with that sign-in.
            let signed_in = secrets.get(&server.oauth_account()).is_some();
            match auth {
                Auth::None if !signed_in => {
                    ().serve(StreamableHttpClientTransport::from_config(config))
                        .await
                        .map_err(|error| {
                            let text = error.to_string();
                            match (looks_like_auth(&text), headers.is_empty()) {
                                // No key was sent, so the server wants a sign-in.
                                (true, true) => ConnectError::SignIn,
                                (true, false) => ConnectError::KeyRefused,
                                (false, _) => ConnectError::Handshake(plain(&text)),
                            }
                        })
                }
                Auth::None | Auth::OAuth { .. } => {
                    let mut manager = oauth::manager(server, secrets)
                        .await
                        .map_err(|error| ConnectError::Handshake(error.to_string()))?;
                    if !manager
                        .initialize_from_store()
                        .await
                        .map_err(|_| ConnectError::SignIn)?
                    {
                        return Err(ConnectError::SignIn);
                    }
                    let client = AuthClient::new(reqwest::Client::new(), manager);
                    ().serve(StreamableHttpClientTransport::with_client(client, config))
                        .await
                        .map_err(|error| {
                            let text = error.to_string();
                            if looks_like_auth(&text) {
                                ConnectError::SignIn
                            } else {
                                ConnectError::Handshake(plain(&text))
                            }
                        })
                }
            }
        }
    }
}

/// The part of an rmcp transport error a person can act on: the server's own answer, without the transport's
/// type names around it.
fn plain(error: &str) -> String {
    let answer = error
        .rsplit_once("unexpected server response: ")
        .map_or(error, |(_, answer)| answer);
    let answer = answer.split(", when send").next().unwrap_or(answer);
    let answer = answer
        .rsplit_once("] error: ")
        .map_or(answer, |(_, answer)| answer);
    answer.trim().to_string()
}

/// Whether a failed handshake was the server turning the token away, which signing in again fixes.
fn looks_like_auth(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    [
        "401",
        "unauthorized",
        "auth required",
        "invalid_token",
        "refresh",
    ]
    .iter()
    .any(|word| error.contains(word))
}

#[cfg(test)]
mod tests {
    use super::{looks_like_auth, plain};

    #[test]
    fn a_transport_error_comes_down_to_the_servers_answer() {
        let error = "Send message error Transport [rmcp::transport::worker::WorkerTransport<X<Y>>] error: \
                     unexpected server response: HTTP 400 Bad Request: header is badly formatted\n, when send \
                     initialize request";
        assert_eq!(
            plain(error),
            "HTTP 400 Bad Request: header is badly formatted"
        );
        let refused = "Send message error Transport [W<X>] error: Auth required, when send initialize request";
        assert_eq!(plain(refused), "Auth required");
        assert!(looks_like_auth(refused));
    }
}
