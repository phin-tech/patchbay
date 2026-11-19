//! The gateway, run inside the app on a tokio runtime of its own, since GPUI's executor isn't tokio and the
//! gateway's servers and HTTP need one. Views reach it through the [`Running`] global and hear its changes.

use std::sync::Arc;

use gpui_kit::*;
use patchbay_core::Home;
use patchbay_gateway::{Gateway, gateway_token};

/// The running gateway, or why it isn't listening.
pub struct Running {
    pub gateway: Gateway,
    /// The home folder whose AI clients the window reads and connects: the person's own when the app runs for real,
    /// a stand-in in Patchbay's folder for tests and captures, so they never touch a real client's config.
    pub client_home: std::path::PathBuf,
    /// Whether work is started on the gateway's own threads: not in tests, which GPUI runs on one thread and holds
    /// to doing everything there.
    pub background: bool,
    runtime: Arc<tokio::runtime::Runtime>,
    pub token: Option<String>,
    pub listening: Result<std::net::SocketAddr, String>,
}

impl Global for Running {}

/// A manifest to start from, written the first time the app opens: two servers to try, one with an API key and
/// one that signs in.
const STARTER: &str = r#"# Your MCP servers. Patchbay runs them and puts every one behind its gateway.
# Secrets are never written here: `secret:<name>` asks for one and keeps it in the Keychain.
# Change this file, then choose Servers > Reload (⌘R).

[secrets.github-token]
prompt = "A GitHub personal access token"
help = "https://github.com/settings/personal-access-tokens/new"

# GitHub's own remote server, reached with the token above.
[servers.github]
url = "https://api.githubcopilot.com/mcp/"
headers = { Authorization = "Bearer ${secret:github-token}" }

# Linear's remote server, which signs in in the browser.
[servers.linear]
url = "https://mcp.linear.app/mcp"
auth = "oauth"

# A profile gathers some of the servers, for a client that should see only those; choose a client's profile beside
# it in Patchbay. Without any, every client sees every server.
# [profiles.issues]
# servers = ["linear"]
"#;

/// Where the gateway's secrets come from.
pub enum Secrets {
    /// The Keychain, or `PATCHBAY_SECRETS=file` in development.
    FromEnv,
    /// Nothing kept, for tests and captures.
    Memory,
}

/// Starts the gateway, before any window opens. Captures get one with nothing in it, so a frame never touches
/// anyone's servers or secrets.
pub fn init(cx: &mut App) {
    if cfg!(feature = "screenshot") {
        let home =
            Home::new(std::env::temp_dir().join(format!("patchbay-shot-{}", std::process::id())));
        let _ = std::fs::create_dir_all(home.root());
        // With one profile beside the default, so a capture shows a client's profile too.
        let manifest = format!("{STARTER}\n[profiles.issues]\nservers = [\"linear\"]\n");
        let _ = std::fs::write(home.manifest(), manifest);
        init_with(home, Secrets::Memory, cx);
        return;
    }
    let home = Home::from_env();
    if !home.manifest().exists() {
        let _ = std::fs::create_dir_all(home.root());
        if let Err(error) = patchbay_core::write_atomically(&home.manifest(), STARTER.as_bytes()) {
            tracing::warn!(%error, "no starter manifest");
        }
    }
    init_with(home, Secrets::FromEnv, cx);
}

/// Starts a gateway over `home`'s manifests. Only one with the real secrets listens on the usual port and starts
/// its servers at once; the others take any free port and start servers when asked.
pub fn init_with(home: Home, secrets: Secrets, cx: &mut App) {
    let real = matches!(secrets, Secrets::FromEnv);
    let client_home = if real {
        std::env::var_os("HOME").map_or_else(|| std::path::PathBuf::from("."), Into::into)
    } else {
        // A few clients look installed there, so a capture shows what a row can do.
        let stand_in = home.root().join("client-home");
        for folder in [".pi/agent", ".codex", ".cursor"] {
            let _ = std::fs::create_dir_all(stand_in.join(folder));
        }
        stand_in
    };
    let secrets: Arc<dyn patchbay_core::secrets::Secrets> = match secrets {
        Secrets::FromEnv => patchbay_core::secrets::from_env(&home),
        Secrets::Memory => Arc::new(patchbay_core::secrets::MemorySecrets::default()),
    };
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("gateway")
            .build()
            .expect("a tokio runtime starts"),
    );
    let gateway = Gateway::new(home, Arc::clone(&secrets));
    runtime.block_on(gateway.reload());
    let token = gateway_token(secrets.as_ref())
        .inspect_err(|error| tracing::error!(%error, "no gateway token"))
        .ok();
    let port = if real { patchbay_core::port() } else { 0 };
    let listening = match &token {
        Some(token) => runtime
            .block_on(patchbay_gateway::serve(
                gateway.clone(),
                port,
                token.clone(),
            ))
            .map(|listening| listening.addr)
            .map_err(|error| {
                tracing::error!(%error, "gateway not listening");
                format!("{error:#}")
            }),
        None => Err("there's no gateway token in the Keychain".to_string()),
    };
    // A capture starts its servers too, to show what each needs; with no secrets, none reaches the network.
    if listening.is_ok() && (real || cfg!(feature = "screenshot")) {
        let starting = gateway.clone();
        runtime.spawn(async move { starting.start_all().await });
    }
    cx.set_global(Running {
        gateway,
        client_home,
        background: real || cfg!(feature = "screenshot"),
        runtime,
        token,
        listening,
    });
}

impl Running {
    /// Reads the manifests again before returning, for a test, where nothing runs on the gateway's threads.
    #[cfg(test)]
    pub fn reload_now(&self) {
        self.runtime.block_on(self.gateway.reload());
    }

    /// The default profile's URL, as clients reach it.
    pub fn url(&self) -> Option<String> {
        self.listening
            .as_ref()
            .ok()
            .map(|addr| format!("http://{addr}/mcp"))
    }

    /// Runs `work` on the gateway's runtime; awaiting what it gives back works from GPUI's executor too.
    pub fn spawn<F>(&self, work: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.runtime.spawn(work)
    }
}
