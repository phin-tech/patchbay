//! Signing in to remote servers that use MCP's OAuth, with Patchbay as the client: tokens are kept in the Keychain
//! and refreshed by rmcp, and no AI client ever sees them.

use std::sync::Arc;
use std::time::Duration;

use patchbay_core::manifest::{Auth, Launch, Server};
use patchbay_core::secrets::Secrets;
use rmcp::transport::auth::{
    AuthError, AuthorizationManager, AuthorizationRequest, AuthorizationSession, CredentialStore,
    StoredCredentials,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// How long the browser has to come back from signing in.
const SIGN_IN_TIME: Duration = Duration::from_secs(300);

/// rmcp's stored credentials as JSON in one Keychain item (`oauth/<server>`).
pub struct KeychainCredentials {
    pub secrets: Arc<dyn Secrets>,
    pub account: String,
}

#[async_trait::async_trait]
impl CredentialStore for KeychainCredentials {
    async fn load(&self) -> Result<Option<StoredCredentials>, AuthError> {
        let Some(json) = self.secrets.get(&self.account) else {
            return Ok(None);
        };
        serde_json::from_str(&json)
            .map(Some)
            .map_err(|error| AuthError::CredentialStoreError(error.to_string()))
    }

    async fn save(&self, credentials: StoredCredentials) -> Result<(), AuthError> {
        let json = serde_json::to_string(&credentials)
            .map_err(|error| AuthError::CredentialStoreError(error.to_string()))?;
        self.secrets
            .set(&self.account, &json)
            .map_err(|error| AuthError::CredentialStoreError(error.to_string()))
    }

    async fn clear(&self) -> Result<(), AuthError> {
        self.secrets.remove(&self.account);
        Ok(())
    }
}

/// Why signing in didn't finish.
#[derive(Debug, thiserror::Error)]
pub enum SignInError {
    #[error("{0} runs on this Mac, so there's nothing to sign in to")]
    NotOAuth(String),
    #[error("couldn't listen for the browser coming back: {0}")]
    Listen(std::io::Error),
    #[error("the server's sign-in didn't work: {0}")]
    Auth(#[from] AuthError),
    #[error("the browser didn't come back within five minutes")]
    TimedOut,
    #[error("the browser came back without a sign-in: {0}")]
    BadCallback(String),
}

/// The manager for `server`'s sign-in, reading and keeping its tokens in the Keychain.
pub async fn manager(
    server: &Server,
    secrets: &Arc<dyn Secrets>,
) -> Result<AuthorizationManager, AuthError> {
    let Launch::Http { url, .. } = &server.launch else {
        return Err(AuthError::InternalError("not a remote server".to_string()));
    };
    // A company's servers often resolve to private addresses over its VPN, and rmcp's default refuses those
    // even for the server's own host. The user chose this server, and its connection reaches the same addresses
    // anyway; loopback and link-local addresses, such as a cloud's instance metadata, stay refused.
    let mut manager = AuthorizationManager::new_allowing_private_addresses(url.as_str()).await?;
    manager.set_credential_store(KeychainCredentials {
        secrets: Arc::clone(secrets),
        account: server.oauth_account(),
    });
    Ok(manager)
}

/// Signs in to `server`: opens its authorization page with `open`, waits on a loopback port for the browser to come
/// back, and keeps the tokens.
pub async fn sign_in(
    server: &Server,
    secrets: &Arc<dyn Secrets>,
    open: impl FnOnce(&str),
) -> Result<(), SignInError> {
    // A server with no `auth` may still ask for a sign-in when reached; it signs in the same way, with the
    // client registering itself.
    let (client_id, scopes) = match &server.launch {
        Launch::Http {
            auth: Auth::OAuth { client_id, scopes },
            ..
        } => (client_id.clone(), scopes.clone()),
        Launch::Http { .. } => (None, Vec::new()),
        Launch::Stdio { .. } => return Err(SignInError::NotOAuth(server.name.clone())),
    };
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(SignInError::Listen)?;
    let port = listener.local_addr().map_err(SignInError::Listen)?.port();
    let redirect = format!("http://127.0.0.1:{port}/callback");

    let mut manager = manager(server, secrets).await?;
    let metadata = manager.resolve_metadata().await?.metadata;
    manager.set_metadata(metadata);
    let mut request = AuthorizationRequest::new(redirect.clone()).with_client_name("Patchbay");
    if !scopes.is_empty() {
        request = request.with_scopes(scopes.iter().map(String::as_str));
    }
    if let Some(client_id) = &client_id {
        request = request.with_preregistered_client(client_id.as_str());
    }
    let session = AuthorizationSession::new(manager, request)
        .await
        .map_err(|(_, error)| error)?;
    open(session.get_authorization_url());

    let path = tokio::time::timeout(SIGN_IN_TIME, callback(&listener))
        .await
        .map_err(|_| SignInError::TimedOut)??;
    session
        .handle_callback_url(&format!("http://127.0.0.1:{port}{path}"))
        .await?;
    Ok(())
}

/// Waits for the browser's request to `/callback`, answers it, and gives the path with its query.
async fn callback(listener: &TcpListener) -> Result<String, SignInError> {
    loop {
        let (mut stream, _) = listener.accept().await.map_err(SignInError::Listen)?;
        let mut buffer = vec![0; 8192];
        // A browser may open a connection it never sends on (a preconnect); don't wait on it.
        let Ok(Ok(read)) =
            tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buffer)).await
        else {
            continue;
        };
        let request = String::from_utf8_lossy(&buffer[..read]);
        let Some(path) = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .map(str::to_string)
        else {
            continue;
        };
        if !path.starts_with("/callback") {
            let _ = stream
                .write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n")
                .await;
            continue;
        }
        let ok = path.contains("code=");
        let body = if ok {
            "<!doctype html><title>Patchbay</title><body style=\"font:16px -apple-system;margin:4em\">\
             <h1>Signed in</h1><p>Patchbay has it from here. You can close this tab.</p>"
        } else {
            "<!doctype html><title>Patchbay</title><body style=\"font:16px -apple-system;margin:4em\">\
             <h1>Not signed in</h1><p>The server didn't sign you in. Try again from Patchbay.</p>"
        };
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        return if ok {
            Ok(path)
        } else {
            Err(SignInError::BadCallback(path))
        };
    }
}
