//! A LiteLLM proxy's MCP servers kept as a team, against a real LiteLLM: subscribed to with an ordinary key, reached
//! through Patchbay's gateway with LiteLLM's prefix taken off their tools, and checked for a server added later.
//!
//! LiteLLM runs in Docker, so these are left out of `cargo test`; `just litellm-test` starts it and runs them.

use std::process::Command;
use std::sync::Arc;

use patchbay_core::Home;
use patchbay_core::secrets::{MemorySecrets, Secrets};
use patchbay_core::subscription::{self, Asked, How, Part, litellm};
use patchbay_gateway::{Gateway, serve};
use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;

/// Where `tests/litellm/compose.yaml` serves LiteLLM, unless `PATCHBAY_LITELLM_URL` says.
fn litellm_url() -> String {
    std::env::var("PATCHBAY_LITELLM_URL").unwrap_or_else(|_| "http://localhost:4555".to_string())
}

/// The master key in `tests/litellm/config.yaml`.
const MASTER: &str = "sk-patchbay-test-master";

/// Asks LiteLLM's admin API, as its admin, with curl: the gateway's reqwest has no JSON of its own.
fn admin(method: &str, path: &str, body: &serde_json::Value) -> serde_json::Value {
    let output = Command::new("/usr/bin/curl")
        .args(["--silent", "--show-error", "--fail-with-body", "-X", method])
        .args(["-H", &format!("Authorization: Bearer {MASTER}")])
        .args([
            "-H",
            "content-type: application/json",
            "-d",
            &body.to_string(),
        ])
        .arg(format!("{}{path}", litellm_url()))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{method} {path}: {text}");
    serde_json::from_str(&text).unwrap_or(serde_json::Value::Null)
}

/// A virtual key of LiteLLM's own making, which isn't an admin's.
fn ordinary_key() -> String {
    admin("POST", "/key/generate", &serde_json::json!({}))["key"]
        .as_str()
        .unwrap()
        .to_string()
}

fn scratch_home(name: &str) -> Home {
    let root = std::env::temp_dir().join(format!("patchbay-litellm-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    Home::new(&root)
}

#[tokio::test]
#[ignore = "needs LiteLLM in Docker: just litellm-test"]
async fn litellms_servers_come_through_the_gateway_under_their_own_names() {
    let home = scratch_home("gateway");
    let key = ordinary_key();
    let url = format!("{}/ui/", litellm_url());
    let (subscribing, given) = (home.clone(), key.clone());
    let pending = tokio::task::spawn_blocking(move || {
        subscription::subscribe_litellm(&subscribing, &url, &given, Asked::default())
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(pending.url, litellm_url(), "kept without its admin page");
    let added: Vec<&str> = pending
        .changes
        .iter()
        .filter(|change| change.part == Part::Server && change.how == How::Added)
        .map(|change| change.name.as_str())
        .collect();
    assert!(added.contains(&"clock_mcp"), "{added:?}");

    let secrets: Arc<dyn Secrets> = Arc::new(MemorySecrets::default());
    secrets
        .set(&litellm::account(&pending.folder), &key)
        .unwrap();
    subscription::accept(&home, &pending).unwrap();

    let gateway = Gateway::new(home.clone(), Arc::clone(&secrets));
    gateway.reload().await;
    assert_eq!(gateway.problems(), Vec::<String>::new());
    let listening = serve(gateway.clone(), 0, "gateway-token".to_string())
        .await
        .unwrap();
    let endpoint = format!("http://{}/mcp/server/clock_mcp", listening.addr);
    let config = StreamableHttpClientTransportConfig::with_uri(endpoint.as_str())
        .auth_header("gateway-token".to_string());
    let client = ().serve(StreamableHttpClientTransport::from_config(config)).await.unwrap();

    let mut names: Vec<String> = client
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect();
    names.sort();
    assert_eq!(names, ["echo", "now"], "LiteLLM's Clock_MCP- is taken off");

    let mut call = CallToolRequestParams::new("echo");
    call.arguments = serde_json::json!({"text": "through LiteLLM"})
        .as_object()
        .cloned();
    let result = client.call_tool(call).await.unwrap();
    let text = serde_json::to_string(&result.content).unwrap();
    assert!(text.contains("through LiteLLM"), "{text}");
    let _ = client.cancel().await;
}

#[tokio::test]
#[ignore = "needs LiteLLM in Docker: just litellm-test"]
async fn a_server_added_in_litellm_waits_to_be_looked_at() {
    let home = scratch_home("check");
    let key = ordinary_key();
    let secrets = MemorySecrets::default();
    let (url, folder) = (litellm_url(), "litellm-check");
    let pending = tokio::task::spawn_blocking({
        let (home, key) = (home.clone(), key.clone());
        move || {
            let asked = Asked {
                name: Some("LiteLLM check"),
                ..Asked::default()
            };
            subscription::subscribe_litellm(&home, &url, &key, asked)
        }
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(pending.folder, folder);
    secrets.set(&litellm::account(folder), &key).unwrap();
    subscription::accept(&home, &pending).unwrap();

    // Waiting on curl holds up this test's own runtime, which has nothing else to do.
    let check =
        |home: &Home, secrets: &MemorySecrets| subscription::check(home, secrets, folder).unwrap();
    assert_eq!(check(&home, &secrets), None, "nothing has changed yet");

    let name = format!("weather_{}", std::process::id());
    let server = admin(
        "POST",
        "/v1/mcp/server",
        &serde_json::json!({
            "server_name": name,
            "transport": "stdio",
            "command": "python",
            "args": ["/srv/clock.py"],
            "allow_all_keys": true,
        }),
    );
    let found = check(&home, &secrets);
    let id = server["server_id"].as_str().unwrap();
    admin(
        "DELETE",
        &format!("/v1/mcp/server/{id}"),
        &serde_json::json!({}),
    );

    let found = found.expect("the new server is waiting");
    let change = found
        .changes
        .iter()
        .find(|change| change.part == Part::Server)
        .unwrap();
    assert_eq!(
        (change.name.as_str(), change.how),
        (name.as_str(), How::Added)
    );
    assert!(
        change
            .fields
            .iter()
            .all(|field| ["url", "tool_prefix"].contains(&field.key.as_str())
                || field.key.starts_with("headers.")),
        "only ever a URL and LiteLLM's key: {:?}",
        change.fields
    );
    assert_eq!(
        check(&home, &secrets),
        None,
        "taken out again before it was accepted, there's nothing to look at"
    );
}
