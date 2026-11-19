//! The gateway on `127.0.0.1`: MCP at `/mcp` and below it, one endpoint for each [`View`] (`/mcp/small/compact`,
//! `/mcp/server/linear`), and Patchbay's own `/patchbay/status` and `/patchbay/reload` for the command line. Every
//! request needs the gateway's token.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex as StdMutex;

use axum::Json;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use patchbay_core::write_atomically;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use super::*;

/// A gateway that's listening, until `shutdown` is cancelled.
pub struct Listening {
    pub addr: SocketAddr,
    pub shutdown: CancellationToken,
}

/// What `gateway.toml` says, for the command line to find a running gateway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayFile {
    pub port: u16,
    pub pid: u32,
}

impl GatewayFile {
    pub fn read(home: &Home) -> Option<GatewayFile> {
        toml::from_str(&std::fs::read_to_string(home.gateway_file()).ok()?).ok()
    }

    /// The URL of `view` on the running gateway.
    pub fn url(&self, view: &View) -> String {
        format!("http://127.0.0.1:{}{view}", self.port)
    }
}

type Service = StreamableHttpService<Handler, LocalSessionManager>;

/// One MCP service for each view a client has asked for, made the first time, so its sessions last.
#[derive(Clone)]
struct Endpoints {
    gateway: Gateway,
    shutdown: CancellationToken,
    services: Arc<StdMutex<HashMap<View, Service>>>,
}

impl Endpoints {
    fn service(&self, view: View) -> Service {
        let mut services = self
            .services
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        services
            .entry(view.clone())
            .or_insert_with(|| {
                let config = StreamableHttpServerConfig::default()
                    .with_cancellation_token(self.shutdown.child_token());
                let handler = Handler::new(self.gateway.clone(), view);
                StreamableHttpService::new(move || Ok(handler.clone()), Default::default(), config)
            })
            .clone()
    }
}

async fn mcp(State(endpoints): State<Endpoints>, request: Request) -> Response {
    let route = request
        .uri()
        .path()
        .strip_prefix("/mcp")
        .unwrap_or_default()
        .trim_start_matches('/')
        .to_string();
    let Some(view) = View::parse(&route) else {
        return (StatusCode::NOT_FOUND, "Patchbay has nothing at that path\n").into_response();
    };
    if !endpoints.gateway.has(&view.exposes) {
        return (
            StatusCode::NOT_FOUND,
            format!("Patchbay has no {}\n", describe(&view.exposes)),
        )
            .into_response();
    }
    let mut service = endpoints.service(view);
    match tower_service::Service::call(&mut service, request).await {
        Ok(response) => response.into_response(),
        Err(never) => match never {},
    }
}

fn describe(exposes: &Exposes) -> String {
    match exposes {
        Exposes::Profile(profile) => format!("profile {profile}"),
        Exposes::Server(server) => format!("server {server}"),
    }
}

impl Gateway {
    /// Whether the manifests name what `exposes` gathers.
    pub fn has(&self, exposes: &Exposes) -> bool {
        match exposes {
            Exposes::Profile(profile) => {
                profile == "default" || read(&self.0.resolved).profiles.contains_key(profile)
            }
            Exposes::Server(server) => self.slot(server).is_some(),
        }
    }
}

/// Listens on `127.0.0.1:port` (0 for any free port) and serves the gateway, needing `token` on every request.
pub async fn serve(gateway: Gateway, port: u16, token: String) -> anyhow::Result<Listening> {
    let shutdown = CancellationToken::new();
    let endpoints = Endpoints {
        gateway: gateway.clone(),
        shutdown: shutdown.clone(),
        services: Arc::default(),
    };
    let router = axum::Router::new()
        .route("/mcp", any(mcp))
        .route("/mcp/{*route}", any(mcp))
        .with_state(endpoints)
        .merge(
            axum::Router::new()
                .route("/patchbay/status", get(status))
                .route("/patchbay/reload", post(reload))
                .with_state(gateway.clone()),
        )
        .layer(middleware::from_fn(move |request: Request, next: Next| {
            let token = token.clone();
            async move { authorize(&token, request, next).await }
        }));

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    let addr = listener.local_addr()?;
    let file = GatewayFile {
        port: addr.port(),
        pid: std::process::id(),
    };
    if let Some(parent) = gateway.home().gateway_file().parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_atomically(
        &gateway.home().gateway_file(),
        toml::to_string(&file)?.as_bytes(),
    )?;
    tracing::info!(%addr, "gateway listening");
    let stop = shutdown.clone();
    tokio::spawn(async move {
        let served = axum::serve(listener, router)
            .with_graceful_shutdown(async move { stop.cancelled_owned().await })
            .await;
        if let Err(error) = served {
            tracing::error!(%error, "gateway stopped");
        }
    });
    Ok(Listening { addr, shutdown })
}

async fn authorize(token: &str, request: Request, next: Next) -> Response {
    let given = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    if given.is_some_and(|given| constant_time_eq(given.as_bytes(), token.as_bytes())) {
        next.run(request).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            "Patchbay's gateway needs its token\n",
        )
            .into_response()
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[derive(Serialize)]
struct StatusReport {
    servers: Vec<ServerStatus>,
    profiles: BTreeMap<String, Vec<String>>,
    problems: Vec<String>,
}

async fn status(State(gateway): State<Gateway>) -> Json<StatusReport> {
    Json(StatusReport {
        servers: gateway.statuses(),
        profiles: gateway.profiles(),
        problems: gateway.problems(),
    })
}

async fn reload(State(gateway): State<Gateway>) -> StatusCode {
    gateway.reload().await;
    // What was just added (a secret, a sign-in) is tried at once, as the app's own Reload does.
    tokio::spawn(async move { gateway.start_all().await });
    StatusCode::NO_CONTENT
}
