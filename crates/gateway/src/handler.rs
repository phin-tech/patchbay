//! What an AI client talks to: one [`View`]'s servers as one MCP server. A profile's tools are under their
//! servers' namespaces (`linear__list_issues`), one server's under their own names, and a compact view stands for
//! them all with `search_tools`, `describe_tool` and `call_tool`.

use std::borrow::Cow;
use std::sync::Arc;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    JsonObject, ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler};
use serde_json::{Value, json};

use super::*;

/// The name of the stand-in tool a server that isn't ready exposes, so an agent can say what's wrong.
const SETUP_TOOL: &str = "needs_setup";

const SEARCH: &str = "search_tools";
const DESCRIBE: &str = "describe_tool";
const CALL: &str = "call_tool";

/// How many tools a search gives unless it asks for more.
const SEARCH_LIMIT: usize = 25;

/// One view's servers, for one client connection.
#[derive(Clone)]
pub struct Handler {
    gateway: Gateway,
    view: View,
}

impl Handler {
    pub fn new(gateway: Gateway, view: View) -> Handler {
        Handler { gateway, view }
    }
}

impl ServerHandler for Handler {
    fn get_info(&self) -> ServerConfig {
        let name = match &self.view.exposes {
            Exposes::Profile(_) => "patchbay".to_string(),
            Exposes::Server(server) => format!("patchbay-{server}"),
        };
        let instructions = match (&self.view.exposes, self.view.layout) {
            (Exposes::Profile(_), Layout::Flat) => {
                "Patchbay gathers several MCP servers into one. Each tool is named after its server: \
                 `github__create_issue` is GitHub's `create_issue`. A tool ending in `__needs_setup` is a server \
                 that isn't ready; calling it says what the person has to do in Patchbay."
            }
            (_, Layout::Compact) => {
                "Patchbay stands for many tools with three. Find one with `search_tools` (words from what you \
                 want to do), read its arguments with `describe_tool`, then run it with `call_tool`, giving its \
                 name and its arguments."
            }
            (Exposes::Server(_), Layout::Flat) => {
                "This server is reached through Patchbay, which keeps its secrets and sign-in. A tool called \
                 `needs_setup` means it isn't ready; calling it says what the person has to do in Patchbay."
            }
        };
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(name, patchbay_core::VERSION))
            .with_instructions(instructions)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(
            self.gateway.tools(&self.view).await,
        ))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        self.gateway.call(&self.view, request).await
    }
}

impl Gateway {
    /// The tools `view` lists, starting idle servers first.
    pub async fn tools(&self, view: &View) -> Vec<Tool> {
        match view.layout {
            Layout::Flat => self.named_tools(&view.exposes).await,
            Layout::Compact => compact_tools(),
        }
    }

    /// Calls a tool `view` lists.
    pub async fn call(
        &self,
        view: &View,
        request: CallToolRequestParams,
    ) -> Result<CallToolResponse, ErrorData> {
        match view.layout {
            Layout::Flat => self.call_named(&view.exposes, request).await,
            Layout::Compact => self.call_compact(&view.exposes, request).await,
        }
    }

    /// The servers `exposes` gathers, started if they're idle.
    async fn slots_for(&self, exposes: &Exposes) -> Vec<Arc<Slot>> {
        let names = match exposes {
            Exposes::Profile(profile) => read(&self.0.resolved)
                .profiles
                .get(profile)
                .cloned()
                .unwrap_or_default(),
            Exposes::Server(server) => vec![server.clone()],
        };
        let slots: Vec<Arc<Slot>> = names
            .iter()
            .filter_map(|name| self.slot(name))
            .filter(|slot| slot.server.enabled)
            .collect();
        let idle: Vec<_> = slots
            .iter()
            .filter(|slot| *read(&slot.status) == Status::Idle)
            .map(|slot| {
                let gateway = self.clone();
                let name = slot.server.name.clone();
                tokio::spawn(async move { gateway.start(&name).await })
            })
            .collect();
        for start in idle {
            let _ = start.await;
        }
        slots
    }

    /// Every tool `exposes` gathers, named as a client calls it: under its server's namespace for a profile,
    /// under its own name for one server.
    async fn named_tools(&self, exposes: &Exposes) -> Vec<Tool> {
        let prefixed = matches!(exposes, Exposes::Profile(_));
        let name = |server: &Server, tool: &str| {
            if prefixed {
                format!("{}{SEPARATOR}{tool}", server.namespace)
            } else {
                tool.to_string()
            }
        };
        let mut exposed = Vec::new();
        for slot in self.slots_for(exposes).await {
            let server = &slot.server;
            let status = read(&slot.status).clone();
            if status != Status::Running {
                if let Some(reason) = setup_reason(&server.name, &status) {
                    exposed.push(Tool::new(
                        name(server, SETUP_TOOL),
                        reason,
                        Arc::new(object_schema(json!({}), &[])),
                    ));
                }
                continue;
            }
            for tool in read(&slot.tools).iter() {
                if !server.tools.allows(&tool.name) {
                    continue;
                }
                let mut tool = tool.clone();
                tool.name = Cow::Owned(name(server, &tool.name));
                exposed.push(tool);
            }
        }
        exposed
    }

    /// Calls a tool by the name [`Gateway::named_tools`] gave it.
    async fn call_named(
        &self,
        exposes: &Exposes,
        mut request: CallToolRequestParams,
    ) -> Result<CallToolResponse, ErrorData> {
        let slots = self.slots_for(exposes).await;
        let (slot, tool) = match exposes {
            Exposes::Profile(profile) => {
                let Some((namespace, tool)) = request.name.split_once(SEPARATOR) else {
                    return Err(ErrorData::invalid_params(
                        format!("{} isn't one of Patchbay's tools", request.name),
                        None,
                    ));
                };
                let found = slots
                    .into_iter()
                    .find(|slot| slot.server.namespace == namespace);
                let Some(slot) = found else {
                    return Err(ErrorData::invalid_params(
                        format!("there's no server {namespace} in Patchbay's {profile} profile"),
                        None,
                    ));
                };
                (slot, tool.to_string())
            }
            Exposes::Server(server) => {
                let Some(slot) = slots.into_iter().next() else {
                    return Err(ErrorData::invalid_params(
                        format!("there's no server {server} in Patchbay"),
                        None,
                    ));
                };
                (slot, request.name.to_string())
            }
        };
        let mut status = read(&slot.status).clone();
        if status != Status::Running {
            status = self.start(&slot.server.name).await.unwrap_or(Status::Idle);
        }
        if status != Status::Running {
            let reason = setup_reason(&slot.server.name, &status)
                .unwrap_or_else(|| format!("{} isn't running", slot.server.name));
            return Ok(CallToolResult::error(vec![ContentBlock::text(reason)]).into());
        }
        if tool == SETUP_TOOL || !slot.server.tools.allows(&tool) {
            return Err(ErrorData::invalid_params(
                format!(
                    "{} isn't one of {}'s tools in Patchbay",
                    tool, slot.server.name
                ),
                None,
            ));
        }
        let Some(peer) = read(&slot.peer).clone() else {
            return Ok(text_error(format!("{} isn't running", slot.server.name)));
        };
        request.name = Cow::Owned(slot.downstream_name(&tool));
        match peer.call_tool_once(request).await {
            Ok(response) => Ok(response),
            Err(error) => {
                tracing::warn!(server = slot.server.name, %error, "call failed");
                if matches!(error, rmcp::ServiceError::TransportClosed) {
                    slot.stop().await;
                    self.changed();
                }
                Ok(text_error(format!(
                    "{} couldn't run it: {error}",
                    slot.server.name
                )))
            }
        }
    }

    /// Runs one of the compact view's three tools.
    async fn call_compact(
        &self,
        exposes: &Exposes,
        request: CallToolRequestParams,
    ) -> Result<CallToolResponse, ErrorData> {
        let arguments = request.arguments.clone().unwrap_or_default();
        let text = |key: &str| {
            arguments
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
        };
        match request.name.as_ref() {
            SEARCH => {
                let limit = arguments
                    .get("limit")
                    .and_then(Value::as_u64)
                    .and_then(|limit| usize::try_from(limit).ok())
                    .unwrap_or(SEARCH_LIMIT);
                let tools = self.named_tools(exposes).await;
                Ok(text_result(search(&tools, text("query"), limit)))
            }
            DESCRIBE => {
                let name = text("name");
                let tools = self.named_tools(exposes).await;
                match tools.iter().find(|tool| tool.name == name) {
                    Some(tool) => Ok(text_result(
                        serde_json::to_string_pretty(&json!({
                            "name": tool.name,
                            "description": tool.description,
                            "inputSchema": tool.input_schema,
                            "annotations": tool.annotations,
                        }))
                        .unwrap_or_default(),
                    )),
                    None => Ok(text_error(format!(
                        "There's no tool called {name}. Find one with {SEARCH}."
                    ))),
                }
            }
            CALL => {
                let mut call = CallToolRequestParams::new(text("name").to_string());
                call.arguments = arguments
                    .get("arguments")
                    .and_then(Value::as_object)
                    .cloned();
                call.meta = request.meta;
                self.call_named(exposes, call).await
            }
            other => Err(ErrorData::invalid_params(
                format!(
                    "{other} isn't one of Patchbay's tools: use {SEARCH}, {DESCRIBE} or {CALL}"
                ),
                None,
            )),
        }
    }
}

/// The compact view's three tools.
fn compact_tools() -> Vec<Tool> {
    vec![
        Tool::new(
            SEARCH,
            "Find the tools that do something, by words from what you want to do (`create issue`, `list \
             files`). Gives each one's name and what it does; an empty query lists them all.",
            Arc::new(object_schema(
                json!({
                    "query": {"type": "string", "description": "Words from what you want to do"},
                    "limit": {"type": "integer", "description": "The most to give back", "default": SEARCH_LIMIT},
                }),
                &["query"],
            )),
        ),
        Tool::new(
            DESCRIBE,
            "A tool's description and the arguments it takes, as a JSON schema, before you call it.",
            Arc::new(object_schema(
                json!({"name": {"type": "string", "description": "The tool's name, as search_tools gave it"}}),
                &["name"],
            )),
        ),
        Tool::new(
            CALL,
            "Runs a tool by its name, with its arguments.",
            Arc::new(object_schema(
                json!({
                    "name": {"type": "string", "description": "The tool's name, as search_tools gave it"},
                    "arguments": {"type": "object", "description": "Its arguments, as describe_tool says"},
                }),
                &["name"],
            )),
        ),
    ]
}

/// The tools whose name and description hold every word of `query`, one a line, at most `limit`.
fn search(tools: &[Tool], query: &str, limit: usize) -> String {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let found: Vec<&Tool> = tools
        .iter()
        .filter(|tool| {
            let hay = format!(
                "{} {}",
                tool.name.replace('_', " "),
                tool.description.as_deref().unwrap_or_default()
            )
            .to_lowercase();
            words.iter().all(|word| hay.contains(word.as_str()))
        })
        .collect();
    if found.is_empty() {
        return format!("No tool matches “{query}”. Try fewer or other words.");
    }
    let mut lines: Vec<String> = found
        .iter()
        .take(limit)
        .map(|tool| {
            let summary = tool
                .description
                .as_deref()
                .unwrap_or_default()
                .lines()
                .next()
                .unwrap_or_default();
            let summary: String = summary.chars().take(160).collect();
            format!("- {}: {summary}", tool.name)
        })
        .collect();
    if found.len() > limit {
        lines.push(format!(
            "…and {} more; narrow the words or raise the limit.",
            found.len() - limit
        ));
    }
    lines.join("\n")
}

fn object_schema(properties: Value, required: &[&str]) -> JsonObject {
    let schema = json!({"type": "object", "properties": properties, "required": required});
    schema.as_object().cloned().unwrap_or_default()
}

fn text_result(text: String) -> CallToolResponse {
    CallToolResult::success(vec![ContentBlock::text(text)]).into()
}

fn text_error(text: String) -> CallToolResponse {
    CallToolResult::error(vec![ContentBlock::text(text)]).into()
}

/// What a server that isn't ready needs, as a sentence an agent can pass on, or nothing if it's simply starting.
fn setup_reason(name: &str, status: &Status) -> Option<String> {
    match status {
        Status::NeedsSecret { secrets } => {
            let wanted: Vec<&str> = secrets
                .iter()
                .map(|secret| secret.prompt.as_str())
                .collect();
            Some(format!(
                "{name} isn't available yet: Patchbay needs {}. Open Patchbay to add it.",
                if wanted.is_empty() {
                    "a secret".to_string()
                } else {
                    wanted.join(" and ")
                }
            ))
        }
        Status::NeedsSignIn => Some(format!(
            "{name} isn't available yet: sign in to it in Patchbay."
        )),
        Status::Failed { reason } => Some(format!("{name} isn't available: {reason}.")),
        Status::Off | Status::Idle | Status::Starting | Status::Running => None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rmcp::model::Tool;

    use super::search;

    fn tool(name: &str, description: &str) -> Tool {
        Tool::new(
            name.to_string(),
            description.to_string(),
            Arc::new(Default::default()),
        )
    }

    #[test]
    fn a_search_finds_tools_with_every_word_in_their_name_or_description() {
        let tools = [
            tool("linear__save_issue", "Create or update an issue"),
            tool("linear__list_issues", "List issues in a team"),
            tool("github__create_issue", "Open an issue on GitHub"),
        ];
        assert_eq!(
            search(&tools, "create issue", 25),
            "- linear__save_issue: Create or update an issue\n- github__create_issue: Open an issue on GitHub"
        );
        assert_eq!(
            search(&tools, "list", 25),
            "- linear__list_issues: List issues in a team"
        );
        assert!(search(&tools, "deploy", 25).starts_with("No tool matches"));
        assert!(
            search(&tools, "", 1).ends_with("…and 2 more; narrow the words or raise the limit.")
        );
    }
}
