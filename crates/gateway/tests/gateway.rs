//! The gateway end to end: a remote server behind an API key, reached through Patchbay's endpoint by a client.

use std::sync::Arc;

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use patchbay_core::Home;
use patchbay_core::secrets::{MemorySecrets, Secrets};
use patchbay_gateway::{Gateway, Status, serve};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{
    StreamableHttpClientTransport, StreamableHttpServerConfig, StreamableHttpService,
};
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};

const KEY: &str = "s3cret-key";

/// A server with one tool, `echo`, that says back its `text`.
#[derive(Clone)]
struct Echo;

impl ServerHandler for Echo {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let schema =
            serde_json::json!({"type": "object", "properties": {"text": {"type": "string"}}});
        let schema = schema.as_object().cloned().unwrap_or_default();
        Ok(ListToolsResult::with_all_items(vec![
            Tool::new("echo", "Says it back", Arc::new(schema.clone())),
            Tool::new("delete_everything", "Mustn't be exposed", Arc::new(schema)),
        ]))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let text = request
            .arguments
            .and_then(|arguments| arguments.get("text").cloned())
            .and_then(|text| text.as_str().map(str::to_string))
            .unwrap_or_default();
        Ok(CallToolResult::success(vec![ContentBlock::text(format!("echo: {text}"))]).into())
    }
}

/// Serves [`Echo`] on a free port, turning away requests without the API key, and gives its URL.
async fn echo_server() -> String {
    let service: StreamableHttpService<Echo, LocalSessionManager> = StreamableHttpService::new(
        || Ok(Echo),
        Default::default(),
        StreamableHttpServerConfig::default(),
    );
    let router = axum::Router::new()
        .route_service("/mcp", service)
        .layer(middleware::from_fn(
            |request: Request, next: Next| async move {
                let authorized = request
                    .headers()
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    == Some(&format!("Bearer {KEY}"));
                if authorized {
                    next.run(request).await
                } else {
                    Response::from(StatusCode::UNAUTHORIZED.into_response())
                }
            },
        ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await });
    format!("http://{addr}/mcp")
}

fn home_with(manifest: &str) -> Home {
    let root = std::env::temp_dir().join(format!(
        "patchbay-gateway-test-{}-{}",
        std::process::id(),
        manifest.len()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let home = Home::new(&root);
    std::fs::write(home.manifest(), manifest).unwrap();
    home
}

#[tokio::test]
async fn a_server_waits_for_its_secret_then_its_tools_come_through_the_gateway() {
    let url = echo_server().await;
    let home = home_with(&format!(
        r#"
[secrets.echo-key]
prompt = "The echo server's API key"

[servers.echo]
url = "{url}"
headers = {{ Authorization = "Bearer ${{secret:echo-key}}" }}
tools.deny = ["delete_*"]
"#
    ));
    let secrets: Arc<dyn Secrets> = Arc::new(MemorySecrets::default());
    let gateway = Gateway::new(home, Arc::clone(&secrets));
    gateway.reload().await;
    assert_eq!(gateway.problems(), Vec::<String>::new());

    let Some(Status::NeedsSecret { secrets: wanted }) = gateway.start("echo").await else {
        panic!("echo should wait for its key");
    };
    assert_eq!(wanted[0].account, "me/echo-key");
    assert_eq!(wanted[0].prompt, "The echo server's API key");

    gateway.set_secret("me/echo-key", KEY).await.unwrap();
    assert_eq!(gateway.statuses()[0].status, Status::Running);

    let listening = serve(gateway.clone(), 0, "gateway-token".to_string())
        .await
        .unwrap();
    let endpoint = format!("http://{}/mcp", listening.addr);
    let config = StreamableHttpClientTransportConfig::with_uri(endpoint.as_str())
        .auth_header("gateway-token".to_string());
    let client = ().serve(StreamableHttpClientTransport::from_config(config)).await.unwrap();

    let tools = client.list_all_tools().await.unwrap();
    let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
    assert_eq!(names, ["echo__echo"]);

    let mut call = CallToolRequestParams::new("echo__echo");
    call.arguments = serde_json::json!({"text": "hi"}).as_object().cloned();
    let result = client.call_tool(call).await.unwrap();
    let text = serde_json::to_string(&result.content).unwrap();
    assert!(text.contains("echo: hi"), "{text}");

    let denied = client
        .call_tool(CallToolRequestParams::new("echo__delete_everything"))
        .await;
    assert!(denied.is_err(), "a denied tool can't be called");
    let _ = client.cancel().await;

    // The one server on its own, its tools under their own names.
    let alone = connect(&listening, "/mcp/server/echo").await;
    let names: Vec<String> = alone
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect();
    assert_eq!(names, ["echo"]);
    let mut call = CallToolRequestParams::new("echo");
    call.arguments = serde_json::json!({"text": "alone"}).as_object().cloned();
    let text = serde_json::to_string(&alone.call_tool(call).await.unwrap().content).unwrap();
    assert!(text.contains("echo: alone"), "{text}");
    let _ = alone.cancel().await;

    // Compact: three tools standing for every one.
    let compact = connect(&listening, "/mcp/compact").await;
    let names: Vec<String> = compact
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect();
    assert_eq!(names, ["search_tools", "describe_tool", "call_tool"]);
    let mut search = CallToolRequestParams::new("search_tools");
    search.arguments = serde_json::json!({"query": "says back"})
        .as_object()
        .cloned();
    let found = serde_json::to_string(&compact.call_tool(search).await.unwrap().content).unwrap();
    assert!(found.contains("echo__echo"), "{found}");
    let mut run = CallToolRequestParams::new("call_tool");
    run.arguments = serde_json::json!({"name": "echo__echo", "arguments": {"text": "compact"}})
        .as_object()
        .cloned();
    let ran = serde_json::to_string(&compact.call_tool(run).await.unwrap().content).unwrap();
    assert!(ran.contains("echo: compact"), "{ran}");
    let _ = compact.cancel().await;

    // A server the manifests don't name has no endpoint.
    let missing = reqwest::Client::new()
        .post(format!("http://{}/mcp/server/nope", listening.addr))
        .bearer_auth("gateway-token")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), reqwest::StatusCode::NOT_FOUND);

    listening.shutdown.cancel();
}

/// A client of the gateway at `path`, with its token.
async fn connect(
    listening: &patchbay_gateway::Listening,
    path: &str,
) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    let endpoint = format!("http://{}{path}", listening.addr);
    let config = StreamableHttpClientTransportConfig::with_uri(endpoint.as_str())
        .auth_header("gateway-token".to_string());
    ().serve(StreamableHttpClientTransport::from_config(config))
        .await
        .unwrap()
}

#[tokio::test]
async fn a_client_without_the_gateways_token_is_turned_away() {
    let home = home_with("");
    let gateway = Gateway::new(home, Arc::new(MemorySecrets::default()));
    gateway.reload().await;
    let listening = serve(gateway, 0, "right".to_string()).await.unwrap();
    let response = reqwest::Client::new()
        .post(format!("http://{}/mcp", listening.addr))
        .header("authorization", "Bearer wrong")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
    listening.shutdown.cancel();
}

#[tokio::test]
async fn a_server_that_needs_setup_tells_the_agent_what_to_do() {
    let home = home_with(
        "[secrets.k]\nprompt = \"A key for X\"\n[servers.x]\nurl = \"http://127.0.0.1:9/mcp\"\nheaders = { Authorization = \"Bearer ${secret:k}\" }\n",
    );
    let gateway = Gateway::new(home, Arc::new(MemorySecrets::default()));
    gateway.reload().await;
    let tools = gateway
        .tools(&patchbay_gateway::View::profile(
            "default",
            patchbay_gateway::Layout::Flat,
        ))
        .await;
    assert_eq!(tools[0].name, "x__needs_setup");
    assert!(
        tools[0]
            .description
            .as_deref()
            .unwrap_or_default()
            .contains("A key for X"),
        "{tools:?}"
    );
}
