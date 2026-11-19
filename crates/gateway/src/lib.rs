//! Patchbay's MCP gateway: one endpoint in front of every server the manifests name. It runs or reaches each
//! server with its secrets from the Keychain, signs in to OAuth servers, and exposes their tools to AI clients
//! under each server's namespace (`github__create_issue`).

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use patchbay_core::Home;
use patchbay_core::manifest::{Auth, Launch, Resolved, SecretPrompt, Server, resolve};
use patchbay_core::secrets::Secrets;
use rmcp::model::Tool;
use rmcp::{Peer, RoleClient};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, watch};

mod bridge;
mod downstream;
mod endpoint;
mod entries;
mod handler;
pub mod oauth;
mod view;

pub use bridge::bridge;
pub use downstream::ConnectError;
pub use endpoint::{GatewayFile, Listening, serve};
pub use entries::client_entries;
pub use handler::Handler;
pub use view::{Exposes, Layout, View};

/// Separates a server's namespace from its tool's own name.
pub const SEPARATOR: &str = "__";

/// Where a server is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum Status {
    /// Turned off in a manifest.
    Off,
    /// Not started yet; it starts when a client first asks for tools.
    Idle,
    Starting,
    Running,
    NeedsSecret {
        secrets: Vec<SecretNeeded>,
    },
    NeedsSignIn,
    Failed {
        reason: String,
    },
}

/// A secret a server is waiting for, and how to ask for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretNeeded {
    pub account: String,
    pub prompt: String,
    pub help: Option<String>,
}

impl From<SecretPrompt> for SecretNeeded {
    fn from(prompt: SecretPrompt) -> SecretNeeded {
        SecretNeeded {
            account: prompt.account,
            prompt: prompt.prompt,
            help: prompt.help,
        }
    }
}

/// One server as the app and `patchbay status` show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServerStatus {
    pub name: String,
    pub namespace: String,
    /// `npx -y …` or the URL.
    pub target: String,
    pub oauth: bool,
    pub status: Status,
    /// Its tools' own names, the exposed ones first.
    pub tools: Vec<ToolStatus>,
    /// Every secret it uses, kept or not, so one can be replaced.
    pub secrets: Vec<SecretNeeded>,
    /// The team whose manifest it comes from, or none for the person's own.
    pub team: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolStatus {
    pub name: String,
    pub exposed: bool,
    /// Whether the server says the tool only reads (MCP's `readOnlyHint`). One that says nothing may write, as MCP
    /// takes it.
    pub read_only: bool,
}

/// The gateway, shared by the HTTP endpoint, the app and its sheets.
#[derive(Clone)]
pub struct Gateway(Arc<Inner>);

struct Inner {
    home: Home,
    secrets: Arc<dyn Secrets>,
    resolved: RwLock<Resolved>,
    slots: RwLock<BTreeMap<String, Arc<Slot>>>,
    problems: RwLock<Vec<String>>,
    changed: watch::Sender<u64>,
}

/// A server and its connection.
struct Slot {
    server: Server,
    status: RwLock<Status>,
    tools: RwLock<Vec<Tool>>,
    peer: RwLock<Option<Peer<RoleClient>>>,
    /// Held while starting, so two clients asking at once start it once; holds the connection, which stops the
    /// server when it's dropped.
    client: Mutex<Option<downstream::Client>>,
}

impl Gateway {
    /// A gateway over `home`'s manifests, with no server started yet; [`Gateway::reload`] reads them.
    pub fn new(home: Home, secrets: Arc<dyn Secrets>) -> Gateway {
        let (changed, _) = watch::channel(0);
        Gateway(Arc::new(Inner {
            home,
            secrets,
            resolved: RwLock::default(),
            slots: RwLock::default(),
            problems: RwLock::default(),
            changed,
        }))
    }

    pub fn home(&self) -> &Home {
        &self.0.home
    }

    pub fn secrets(&self) -> &Arc<dyn Secrets> {
        &self.0.secrets
    }

    /// Ticks whenever a server's status, its tools or the manifests change.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.0.changed.subscribe()
    }

    fn changed(&self) {
        self.0
            .changed
            .send_modify(|tick| *tick = tick.wrapping_add(1));
    }

    /// Reads the manifests again. Servers whose definition changed are stopped and start again when next asked
    /// for; the rest keep running.
    pub async fn reload(&self) {
        let (layers, mut problems) = self.0.home.layers();
        let resolved = resolve(&layers);
        problems.extend(resolved.problems.iter().map(ToString::to_string));
        let mut stopped = Vec::new();
        {
            let mut slots = write(&self.0.slots);
            let old = std::mem::take(&mut *slots);
            for (name, server) in &resolved.servers {
                let kept = old.get(name).filter(|slot| slot.server == *server).cloned();
                // A secret or a sign-in may have been added from the command line since; try again.
                if let Some(slot) = &kept
                    && matches!(
                        *read(&slot.status),
                        Status::NeedsSecret { .. } | Status::NeedsSignIn | Status::Failed { .. }
                    )
                {
                    slot.set_status(Status::Idle);
                }
                let slot = kept.unwrap_or_else(|| {
                    Arc::new(Slot {
                        server: server.clone(),
                        status: RwLock::new(if server.enabled {
                            Status::Idle
                        } else {
                            Status::Off
                        }),
                        tools: RwLock::default(),
                        peer: RwLock::default(),
                        client: Mutex::default(),
                    })
                });
                slots.insert(name.clone(), slot);
            }
            for (name, slot) in old {
                if slots
                    .get(&name)
                    .is_none_or(|kept| !Arc::ptr_eq(kept, &slot))
                {
                    stopped.push(slot);
                }
            }
        }
        *write(&self.0.resolved) = resolved;
        *write(&self.0.problems) = problems;
        for slot in stopped {
            slot.stop().await;
        }
        self.changed();
    }

    /// What's wrong in the manifests, as sentences.
    pub fn problems(&self) -> Vec<String> {
        read(&self.0.problems).clone()
    }

    /// The profiles and the servers in each.
    pub fn profiles(&self) -> BTreeMap<String, Vec<String>> {
        read(&self.0.resolved).profiles.clone()
    }

    fn slot(&self, name: &str) -> Option<Arc<Slot>> {
        read(&self.0.slots).get(name).cloned()
    }

    fn slots(&self) -> Vec<Arc<Slot>> {
        read(&self.0.slots).values().cloned().collect()
    }

    /// Every server and where it is.
    pub fn statuses(&self) -> Vec<ServerStatus> {
        self.slots()
            .iter()
            .map(|slot| {
                let server = &slot.server;
                let mut tools: Vec<ToolStatus> = read(&slot.tools)
                    .iter()
                    .map(|tool| ToolStatus {
                        name: tool.name.to_string(),
                        exposed: server.tools.allows(&tool.name),
                        read_only: tool
                            .annotations
                            .as_ref()
                            .and_then(|annotations| annotations.read_only_hint)
                            .unwrap_or(false),
                    })
                    .collect();
                tools.sort_by_key(|tool| !tool.exposed);
                let (target, oauth) = match &server.launch {
                    Launch::Stdio { command, args, .. } => (
                        std::iter::once(command.clone())
                            .chain(args.iter().map(|arg| arg.value.to_string()))
                            .collect::<Vec<_>>()
                            .join(" "),
                        false,
                    ),
                    Launch::Http { url, auth, .. } => {
                        (url.clone(), matches!(auth, Auth::OAuth { .. }))
                    }
                };
                let prompts = &read(&self.0.resolved).secrets;
                let secrets = server
                    .accounts()
                    .into_iter()
                    .filter_map(|account| prompts.get(&account).cloned())
                    .map(SecretNeeded::from)
                    .collect();
                ServerStatus {
                    secrets,
                    team: match &server.from {
                        patchbay_core::manifest::Scope::Team(team) => Some(team.clone()),
                        patchbay_core::manifest::Scope::Me => None,
                    },
                    name: server.name.clone(),
                    namespace: server.namespace.clone(),
                    target,
                    oauth,
                    status: read(&slot.status).clone(),
                    tools,
                }
            })
            .collect()
    }

    /// Starts every enabled server that isn't running, at once, as the app does at launch.
    pub async fn start_all(&self) {
        let starts = self
            .slots()
            .into_iter()
            .filter(|slot| slot.server.enabled)
            .map(|slot| {
                let gateway = self.clone();
                tokio::spawn(async move { gateway.start(&slot.server.name).await })
            })
            .collect::<Vec<_>>();
        for start in starts {
            let _ = start.await;
        }
    }

    /// Starts `name` if it isn't running, and says where it ended up.
    pub async fn start(&self, name: &str) -> Option<Status> {
        let slot = self.slot(name)?;
        if !slot.server.enabled {
            return Some(Status::Off);
        }
        let mut client = slot.client.lock().await;
        if client.is_some() && *read(&slot.status) == Status::Running {
            return Some(Status::Running);
        }
        let missing = read(&self.0.resolved).missing_secrets(&slot.server, |account| {
            self.0.secrets.get(account).is_some()
        });
        if !missing.is_empty() {
            let status = Status::NeedsSecret {
                secrets: missing.into_iter().map(SecretNeeded::from).collect(),
            };
            slot.set_status(status.clone());
            self.changed();
            return Some(status);
        }
        slot.set_status(Status::Starting);
        self.changed();
        let status = match downstream::connect(&slot.server, &self.0.secrets).await {
            Ok(connected) => {
                let peer = connected.peer().clone();
                match peer.list_all_tools().await {
                    Ok(tools) => {
                        tracing::info!(server = name, tools = tools.len(), "connected");
                        *write(&slot.tools) = tools;
                        *write(&slot.peer) = Some(peer);
                        *client = Some(connected);
                        Status::Running
                    }
                    Err(error) => Status::Failed {
                        reason: format!("it didn't list its tools: {error}"),
                    },
                }
            }
            Err(ConnectError::SignIn) => Status::NeedsSignIn,
            Err(ConnectError::Missing(missing)) => {
                let prompt = read(&self.0.resolved).secrets.get(&missing.0).cloned();
                Status::NeedsSecret {
                    secrets: prompt.into_iter().map(SecretNeeded::from).collect(),
                }
            }
            Err(error) => {
                tracing::warn!(server = name, %error, "couldn't connect");
                Status::Failed {
                    reason: error.to_string(),
                }
            }
        };
        slot.set_status(status.clone());
        self.changed();
        Some(status)
    }

    /// Stops `name` and starts it again.
    pub async fn restart(&self, name: &str) -> Option<Status> {
        self.slot(name)?.stop().await;
        self.start(name).await
    }

    /// Keeps a secret in the Keychain and starts again every server that was waiting for it.
    pub async fn set_secret(&self, account: &str, secret: &str) -> anyhow::Result<()> {
        self.0.secrets.set(account, secret)?;
        let waiting: Vec<String> = self
            .slots()
            .iter()
            .filter(|slot| {
                slot.server
                    .accounts()
                    .iter()
                    .any(|needed| needed == account)
            })
            .map(|slot| slot.server.name.clone())
            .collect();
        for name in waiting {
            self.restart(&name).await;
        }
        Ok(())
    }

    /// Signs in to `name` in the browser (`open` shows the page), then connects to it.
    pub async fn sign_in(&self, name: &str, open: impl FnOnce(&str)) -> anyhow::Result<Status> {
        let slot = self
            .slot(name)
            .ok_or_else(|| anyhow::anyhow!("there's no server called {name}"))?;
        oauth::sign_in(&slot.server, &self.0.secrets, open).await?;
        Ok(self.restart(name).await.unwrap_or(Status::Idle))
    }

    /// Forgets `name`'s sign-in.
    pub async fn sign_out(&self, name: &str) {
        if let Some(slot) = self.slot(name) {
            self.0.secrets.remove(&slot.server.oauth_account());
            slot.stop().await;
            slot.set_status(Status::NeedsSignIn);
            self.changed();
        }
    }
}

impl Slot {
    fn set_status(&self, status: Status) {
        *write(&self.status) = status;
    }

    async fn stop(&self) {
        let mut client = self.client.lock().await;
        *write(&self.peer) = None;
        if let Some(client) = client.take() {
            let _ = client.cancel().await;
        }
        if self.server.enabled {
            self.set_status(Status::Idle);
        }
    }
}

/// The gateway's own token, which clients send to reach it, made the first time it's asked for.
pub fn gateway_token(secrets: &dyn Secrets) -> anyhow::Result<String> {
    const ACCOUNT: &str = "gateway/token";
    if let Some(token) = secrets.get(ACCOUNT) {
        return Ok(token);
    }
    let mut bytes = [0u8; 24];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut random| std::io::Read::read_exact(&mut random, &mut bytes))?;
    let token: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    secrets.set(ACCOUNT, &token)?;
    Ok(token)
}

fn read<T>(lock: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn write<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
