//! The AI clients Patchbay points at its gateway, and the one entry, `patchbay`, it keeps in each one's config.
//! Nothing else in a client's file is touched: other servers, unknown keys and key order stay as they were.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// The name of Patchbay's entry in a client's config, when it has one; one for each server is `patchbay-<server>`.
pub const ENTRY: &str = "patchbay";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Client {
    ClaudeCode,
    ClaudeDesktop,
    Codex,
    Cursor,
    VsCode,
    GeminiCli,
    OpenCode,
    Pi,
    Omp,
    /// PiG, pi in Go (pi-in-go.dev).
    Pig,
}

/// How a client reaches the gateway.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// The gateway's URL, with its token in a header.
    Http,
    /// `patchbay bridge`, run by the client.
    Bridge,
}

/// Where a client's config is and how Patchbay's entry goes in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    /// JSON, with the entry under `key`; `typed` clients want `"type": "http"` beside the URL.
    Json { key: &'static str, typed: bool },
    /// Codex's `config.toml`, `[mcp_servers.patchbay]`.
    CodexToml,
    /// opencode's `opencode.json`: under `mcp`, a `local` server whose `command` is one list.
    OpenCode,
}

/// One entry Patchbay keeps in a client's config: `patchbay`, or `patchbay-<server>` for a client given one entry
/// for each server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    /// The gateway's URL for what the entry exposes.
    pub url: String,
    /// The same, as the bridge takes it: the path after `/mcp/` (`default`, `server/linear`, `small/compact`).
    pub route: String,
    pub token: String,
    /// The `patchbay` binary, for clients that run the bridge.
    pub binary: PathBuf,
    /// What the bridge needs in its environment to find the same gateway: `PATCHBAY_HOME` and `PATCHBAY_SECRETS`
    /// when the app runs with them. See [`bridge_env`].
    pub env: Vec<(String, String)>,
}

/// Whether an entry in a client's config is Patchbay's own, which it may replace or remove.
pub fn is_ours(name: &str) -> bool {
    name == ENTRY || name.starts_with("patchbay-")
}

/// How a client is given the gateway.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Layout {
    /// One entry, `patchbay`, with every tool under its server's namespace.
    #[default]
    OneEntry,
    /// An entry for each server, `patchbay-linear`, so the client lists and toggles each on its own.
    PerServer,
    /// One entry with three tools, `search_tools`, `describe_tool` and `call_tool`, for every tool behind them.
    Compact,
}

impl Layout {
    pub const ALL: [Layout; 3] = [Layout::OneEntry, Layout::PerServer, Layout::Compact];
}

/// What a client is given: a profile's servers, laid out one way.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Choice {
    #[serde(default = "default_profile")]
    pub profile: String,
    #[serde(default)]
    pub layout: Layout,
    /// Whether taking over the client's own servers has been offered, which happens once, when it's first connected.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub offered_take_over: bool,
}

fn default_profile() -> String {
    "default".to_string()
}

impl Default for Choice {
    fn default() -> Choice {
        Choice {
            profile: default_profile(),
            layout: Layout::default(),
            offered_take_over: false,
        }
    }
}

/// Each client's [`Choice`], kept in `clients.toml` in Patchbay's folder, by the client's slug.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Choices(std::collections::BTreeMap<String, Choice>);

impl Choices {
    pub fn load(home: &crate::Home) -> Choices {
        std::fs::read_to_string(home.root().join("clients.toml"))
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, home: &crate::Home) -> std::io::Result<()> {
        let text = toml::to_string(self).map_err(std::io::Error::other)?;
        crate::write_atomically(&home.root().join("clients.toml"), text.as_bytes())
    }

    pub fn get(&self, client: Client) -> Choice {
        self.0.get(client.slug()).cloned().unwrap_or_default()
    }

    pub fn set(&mut self, client: Client, choice: Choice) {
        self.0.insert(client.slug().to_string(), choice);
    }
}

/// The variables a bridge started by a client needs to reach the gateway this process runs: the ones that move
/// Patchbay's folder or its secrets, if set. Without them the bridge would look in the Keychain and the usual
/// folder, find a different token, and be turned away.
pub fn bridge_env() -> Vec<(String, String)> {
    ["PATCHBAY_HOME", "PATCHBAY_SECRETS"]
        .into_iter()
        .filter_map(|key| Some((key.to_string(), std::env::var(key).ok()?)))
        .collect()
}

/// Why a client's config couldn't be changed.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("{path} isn't JSON Patchbay can read: {error}")]
    NotJson { path: String, error: String },
    #[error("{path} isn't TOML Patchbay can read: {error}")]
    NotToml { path: String, error: String },
    #[error("{path}: `{key}` isn't a table")]
    NotTable { path: String, key: String },
    #[error("couldn't write {path}: {error}")]
    Write { path: String, error: std::io::Error },
}

/// A client's own servers, ready to take over: their names, them as a block to paste into Add Server, and those left
/// in the client since Patchbay already has a server by that name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnServers {
    pub names: Vec<String>,
    pub pasted: String,
    pub left: Vec<String>,
}

impl Client {
    pub const ALL: [Client; 10] = [
        Client::ClaudeCode,
        Client::ClaudeDesktop,
        Client::Codex,
        Client::Cursor,
        Client::VsCode,
        Client::GeminiCli,
        Client::OpenCode,
        Client::Pi,
        Client::Omp,
        Client::Pig,
    ];

    /// The short name the command line takes: `claude-code`.
    pub fn slug(self) -> &'static str {
        match self {
            Client::ClaudeCode => "claude-code",
            Client::ClaudeDesktop => "claude-desktop",
            Client::Codex => "codex",
            Client::Cursor => "cursor",
            Client::VsCode => "vscode",
            Client::GeminiCli => "gemini",
            Client::OpenCode => "opencode",
            Client::Pi => "pi",
            Client::Omp => "omp",
            Client::Pig => "pig",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Client::ClaudeCode => "Claude Code",
            Client::ClaudeDesktop => "Claude Desktop",
            Client::Codex => "Codex",
            Client::Cursor => "Cursor",
            Client::VsCode => "VS Code",
            Client::GeminiCli => "Gemini CLI",
            Client::OpenCode => "opencode",
            Client::Pi => "pi",
            Client::Omp => "omp",
            Client::Pig => "PiG",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Client> {
        Client::ALL.into_iter().find(|client| client.slug() == slug)
    }

    pub fn reach(self) -> Reach {
        match self {
            // HTTP for each client whose own tool has written or connected to the entry Patchbay writes: Claude
            // Code, pi, omp and PiG with their `mcp add`, VS Code with `code --add-mcp`, Codex read back by
            // `codex mcp get`, and Cursor and opencode connected to a gateway. The bridge for Claude Desktop,
            // whose config only runs commands, and Gemini CLI, which hasn't been checked.
            Client::ClaudeCode
            | Client::Pi
            | Client::Omp
            | Client::Pig
            | Client::Cursor
            | Client::VsCode
            | Client::Codex
            | Client::OpenCode => Reach::Http,
            Client::ClaudeDesktop | Client::GeminiCli => Reach::Bridge,
        }
    }

    fn format(self) -> Format {
        match self {
            Client::ClaudeCode => Format::Json {
                key: "mcpServers",
                typed: true,
            },
            Client::ClaudeDesktop
            | Client::Cursor
            | Client::GeminiCli
            | Client::Pi
            | Client::Omp
            | Client::Pig => Format::Json {
                key: "mcpServers",
                typed: false,
            },
            Client::VsCode => Format::Json {
                key: "servers",
                typed: true,
            },
            Client::Codex => Format::CodexToml,
            Client::OpenCode => Format::OpenCode,
        }
    }

    /// The client's user-level config under the home folder `home`.
    pub fn config_path(self, home: &Path) -> PathBuf {
        let support = home.join("Library/Application Support");
        match self {
            Client::ClaudeCode => home.join(".claude.json"),
            Client::ClaudeDesktop => support.join("Claude/claude_desktop_config.json"),
            Client::Codex => home.join(".codex/config.toml"),
            Client::Cursor => home.join(".cursor/mcp.json"),
            Client::VsCode => support.join("Code/User/mcp.json"),
            Client::GeminiCli => home.join(".gemini/settings.json"),
            Client::OpenCode => home.join(".config/opencode/opencode.json"),
            Client::Pi => home.join(".pi/agent/mcp.json"),
            Client::Omp => home.join(".omp/agent/mcp.json"),
            // PiG keeps its files in `PIG_HOME` when that's set.
            Client::Pig => std::env::var_os("PIG_HOME")
                .map_or_else(|| home.join(".pig"), PathBuf::from)
                .join("agent/mcp.json"),
        }
    }

    /// Whether the client looks installed: its config, or the folder it would be in, exists.
    pub fn detected(self, home: &Path) -> bool {
        let path = self.config_path(home);
        path.exists() || path.parent().is_some_and(Path::exists) && path.parent() != Some(home)
    }

    /// The config `text` with `entries` in it, in place of any Patchbay kept there before.
    pub fn with_entries(
        self,
        text: &str,
        entries: &[Entry],
        path: &str,
    ) -> Result<String, ClientError> {
        match self.format() {
            Format::Json { key, typed } => edit_json(text, path, key, |servers| {
                servers.retain(|name, _| !is_ours(name));
                for entry in entries {
                    servers.insert(entry.name.clone(), self.json_entry(entry, typed));
                }
            }),
            Format::CodexToml => edit_toml(text, path, |servers| {
                let ours: Vec<String> = servers
                    .iter()
                    .map(|(name, _)| name.to_string())
                    .filter(|name| is_ours(name))
                    .collect();
                for name in ours {
                    servers.remove(&name);
                }
                for entry in entries {
                    let mut table = toml_edit::Table::new();
                    match self.reach() {
                        Reach::Http => {
                            table["url"] = toml_edit::value(entry.url.as_str());
                            let mut headers = toml_edit::InlineTable::new();
                            headers
                                .insert("Authorization", format!("Bearer {}", entry.token).into());
                            table["http_headers"] = toml_edit::value(headers);
                        }
                        Reach::Bridge => {
                            table["command"] = toml_edit::value(entry.binary.display().to_string());
                            let mut args = toml_edit::Array::new();
                            args.push("bridge");
                            args.push(entry.route.as_str());
                            table["args"] = toml_edit::value(args);
                            if !entry.env.is_empty() {
                                let mut env = toml_edit::InlineTable::new();
                                for (key, value) in &entry.env {
                                    env.insert(key, value.as_str().into());
                                }
                                table["env"] = toml_edit::value(env);
                            }
                        }
                    }
                    servers.insert(&entry.name, toml_edit::Item::Table(table));
                }
            }),
            Format::OpenCode => edit_json(text, path, "mcp", |servers| {
                servers.retain(|name, _| !is_ours(name));
                for entry in entries {
                    let server = match self.reach() {
                        Reach::Http => serde_json::json!({
                            "type": "remote",
                            "url": entry.url,
                            "headers": { "Authorization": format!("Bearer {}", entry.token) },
                            "enabled": true,
                        }),
                        Reach::Bridge => {
                            let mut local = serde_json::json!({
                                "type": "local",
                                "command": [entry.binary.display().to_string(), "bridge", entry.route],
                                "enabled": true,
                            });
                            if !entry.env.is_empty() {
                                local["environment"] = env_object(&entry.env);
                            }
                            local
                        }
                    };
                    servers.insert(entry.name.clone(), server);
                }
            }),
        }
    }

    /// The config `text` without any of Patchbay's entries.
    pub fn without_entries(self, text: &str, path: &str) -> Result<String, ClientError> {
        self.with_entries(text, &[], path)
    }

    /// The names of Patchbay's entries in the config `text`.
    pub fn entry_names(self, text: &str) -> Vec<String> {
        let names: Vec<String> = match self.format() {
            Format::Json { key, .. } => json_names(text, key),
            Format::OpenCode => json_names(text, "mcp"),
            Format::CodexToml => text
                .parse::<toml_edit::DocumentMut>()
                .ok()
                .and_then(|doc| {
                    Some(
                        doc.get("mcp_servers")?
                            .as_table_like()?
                            .iter()
                            .map(|(name, _)| name.to_string())
                            .collect(),
                    )
                })
                .unwrap_or_default(),
        };
        names.into_iter().filter(|name| is_ours(name)).collect()
    }

    fn json_entry(self, entry: &Entry, typed: bool) -> Value {
        let mut object = Map::new();
        match self.reach() {
            Reach::Http => {
                if typed {
                    object.insert("type".into(), "http".into());
                }
                object.insert("url".into(), entry.url.clone().into());
                object.insert(
                    "headers".into(),
                    serde_json::json!({ "Authorization": format!("Bearer {}", entry.token) }),
                );
            }
            Reach::Bridge => {
                if typed {
                    object.insert("type".into(), "stdio".into());
                }
                object.insert("command".into(), entry.binary.display().to_string().into());
                object.insert("args".into(), serde_json::json!(["bridge", entry.route]));
                if !entry.env.is_empty() {
                    object.insert("env".into(), env_object(&entry.env));
                }
            }
        }
        Value::Object(object)
    }

    /// Puts `entries` in the client's config under `home` in place of any Patchbay kept there, keeping a copy of the
    /// file as it was beside it (`<file>.patchbay-backup`).
    pub fn connect(self, home: &Path, entries: &[Entry]) -> Result<PathBuf, ClientError> {
        let path = self.config_path(home);
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        // Claude Code rewrites its own file while sessions run, so it's asked to change its entries itself; the
        // file is edited directly only if its command line can't be run.
        if self == Client::ClaudeCode {
            let wanted: Vec<(String, Value)> = entries
                .iter()
                .map(|entry| (entry.name.clone(), self.json_entry(entry, true)))
                .collect();
            if claude_code(home, &self.entry_names(&text), &wanted) {
                return Ok(path);
            }
        }
        let changed = self.with_entries(&text, entries, &path.display().to_string())?;
        save(&path, &text, &changed)?;
        Ok(path)
    }

    /// Takes all of Patchbay's entries out of the client's config under `home`.
    pub fn disconnect(self, home: &Path) -> Result<PathBuf, ClientError> {
        self.connect(home, &[])
    }

    /// The client's own servers but those named in `kept`, which Patchbay already has, as one `mcpServers` block
    /// to paste into Add Server; none if that leaves none. Adding a server replaces one of the same name, so one
    /// Patchbay has is left in the client rather than taking the place of the person's own.
    pub fn own_servers_pasted(self, home: &Path, kept: &[String]) -> Option<OwnServers> {
        let mut own = self.own_servers(home);
        let (left, taken): (Vec<_>, Vec<_>) =
            own.drain(..).partition(|(name, _)| kept.contains(name));
        if taken.is_empty() {
            return None;
        }
        let names = taken.iter().map(|(name, _)| name.clone()).collect();
        let block =
            serde_json::json!({ "mcpServers": taken.into_iter().collect::<Map<String, Value>>() });
        Some(OwnServers {
            names,
            pasted: serde_json::to_string_pretty(&block).expect("JSON values always print"),
            left: left.into_iter().map(|(name, _)| name).collect(),
        })
    }

    /// The servers the client has of its own, not Patchbay's, by their names in its config, each as the JSON a
    /// README gives (`command`, `args`, `env`, `url`, `headers`), which [`crate::paste::read`] reads. Only its
    /// user-level ones: a Claude Code project's servers stay with the project.
    pub fn own_servers(self, home: &Path) -> Vec<(String, Value)> {
        let Ok(text) = std::fs::read_to_string(self.config_path(home)) else {
            return Vec::new();
        };
        let mut servers: Vec<(String, Value)> = match self.format() {
            Format::Json { key, .. } => json_servers(&text, key),
            Format::OpenCode => json_servers(&text, "mcp"),
            Format::CodexToml => codex_servers(&text),
        };
        servers.retain(|(name, _)| !is_ours(name));
        servers
    }

    /// Takes the servers `names` of the client's own out of its config under `home`, keeping a copy of the file as
    /// it was beside it, `<file>.patchbay-take-over-backup`. It's a file of its own, since connecting replaces
    /// `<file>.patchbay-backup` each time, and the servers taken out would be in neither after the next. Claude Code
    /// is asked to do it itself, as [`Client::connect`] does, but is backed up first all the same.
    pub fn remove_own(self, home: &Path, names: &[String]) -> Result<PathBuf, ClientError> {
        let path = self.config_path(home);
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        back_up(&path, &text, ".patchbay-take-over-backup")?;
        let names: Vec<&String> = names.iter().filter(|name| !is_ours(name)).collect();
        if self == Client::ClaudeCode && claude_code_remove(home, &names) {
            return Ok(path);
        }
        let shown = path.display().to_string();
        let changed = match self.format() {
            Format::Json { key, .. } => edit_json(&text, &shown, key, |servers| {
                servers.retain(|name, _| !names.contains(&name));
            }),
            Format::OpenCode => edit_json(&text, &shown, "mcp", |servers| {
                servers.retain(|name, _| !names.contains(&name));
            }),
            Format::CodexToml => edit_toml(&text, &shown, |servers| {
                for name in &names {
                    servers.remove(name);
                }
            }),
        }?;
        save(&path, &text, &changed)?;
        Ok(path)
    }

    /// The names of Patchbay's entries in the client's config under `home`.
    pub fn entries_in(self, home: &Path) -> Vec<String> {
        std::fs::read_to_string(self.config_path(home))
            .map(|text| self.entry_names(&text))
            .unwrap_or_default()
    }

    /// Whether the client's config under `home` has any of Patchbay's entries.
    pub fn connected(self, home: &Path) -> bool {
        !self.entries_in(home).is_empty()
    }
}

fn env_object(env: &[(String, String)]) -> Value {
    Value::Object(
        env.iter()
            .map(|(key, value)| (key.clone(), Value::String(value.clone())))
            .collect(),
    )
}

/// Replaces Patchbay's entries in Claude Code's user scope, `present`, with `wanted`, using its own `claude mcp`.
/// Each entry's JSON goes on stdin, so the gateway's token is never in a process list. False if `claude` couldn't
/// do it, for the caller to edit the file instead. Only for the real home: a test's home is edited directly.
fn claude_code(home: &Path, present: &[String], wanted: &[(String, Value)]) -> bool {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    if std::env::var_os("HOME").is_none_or(|real| Path::new(&real) != home) {
        return false;
    }
    let run = |args: &[&str], input: Option<String>| {
        let mut command = Command::new("claude");
        command
            .args(args)
            .env("PATH", crate::path::login_path())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let Ok(mut child) = command.spawn() else {
            return false;
        };
        if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
            let _ = stdin.write_all(input.as_bytes());
        }
        child.wait().is_ok_and(|status| status.success())
    };
    for name in present {
        if !run(&["mcp", "remove", name, "-s", "user"], None) {
            return false;
        }
    }
    wanted.iter().all(|(name, entry)| {
        run(
            &["mcp", "add-json", "-s", "user", name, "-"],
            Some(entry.to_string()),
        )
    })
}

/// Takes Claude Code's own user-level servers `names` out with its own `claude mcp remove`. False if it couldn't, for
/// the caller to edit the file instead; only for the real home, as [`claude_code`] is.
fn claude_code_remove(home: &Path, names: &[&String]) -> bool {
    use std::process::{Command, Stdio};

    if std::env::var_os("HOME").is_none_or(|real| Path::new(&real) != home) {
        return false;
    }
    names.iter().all(|name| {
        Command::new("claude")
            .args(["mcp", "remove", name.as_str(), "-s", "user"])
            .env("PATH", crate::path::login_path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    })
}

/// The servers under `key` in a JSON config, by name.
fn json_servers(text: &str, key: &str) -> Vec<(String, Value)> {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|value| {
            let servers = value.get(key)?.as_object()?;
            Some(
                servers
                    .iter()
                    .map(|(name, server)| (name.clone(), server.clone()))
                    .collect(),
            )
        })
        .unwrap_or_default()
}

/// Codex's `[mcp_servers.<name>]` tables, as the JSON a README gives: its `http_headers` are `headers`.
fn codex_servers(text: &str) -> Vec<(String, Value)> {
    let Ok(doc) = text.parse::<toml::Table>() else {
        return Vec::new();
    };
    let Some(servers) = doc.get("mcp_servers").and_then(toml::Value::as_table) else {
        return Vec::new();
    };
    servers
        .iter()
        .filter_map(|(name, server)| {
            let mut json = serde_json::to_value(server.as_table()?).ok()?;
            let object = json.as_object_mut()?;
            if let Some(headers) = object.remove("http_headers") {
                object.insert("headers".into(), headers);
            }
            Some((name.clone(), json))
        })
        .collect()
}

fn json_names(text: &str, key: &str) -> Vec<String> {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|value| Some(value.get(key)?.as_object()?.keys().cloned().collect()))
        .unwrap_or_default()
}

/// Keeps `old`, the file at `path` as it was, beside it as `<file><suffix>`, unless it was empty. These files hold
/// other servers' tokens, so the backup keeps the original's permissions.
fn back_up(path: &Path, old: &str, suffix: &str) -> Result<(), ClientError> {
    if old.is_empty() {
        return Ok(());
    }
    let write = |error| ClientError::Write {
        path: path.display().to_string(),
        error,
    };
    let mut backup = path.as_os_str().to_owned();
    backup.push(suffix);
    let backup = PathBuf::from(backup);
    crate::write_atomically(&backup, old.as_bytes()).map_err(write)?;
    if let Ok(meta) = std::fs::metadata(path) {
        std::fs::set_permissions(&backup, meta.permissions()).map_err(write)?;
    }
    Ok(())
}

fn save(path: &Path, old: &str, new: &str) -> Result<(), ClientError> {
    let write = |error| ClientError::Write {
        path: path.display().to_string(),
        error,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(write)?;
    }
    let permissions = std::fs::metadata(path).ok().map(|meta| meta.permissions());
    back_up(path, old, ".patchbay-backup")?;
    crate::write_atomically(path, new.as_bytes()).map_err(write)?;
    if let Some(permissions) = permissions {
        std::fs::set_permissions(path, permissions).map_err(write)?;
    }
    Ok(())
}

fn edit_json(
    text: &str,
    path: &str,
    key: &str,
    change: impl FnOnce(&mut Map<String, Value>),
) -> Result<String, ClientError> {
    let mut root: Value = if text.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str(text).map_err(|error| ClientError::NotJson {
            path: path.to_string(),
            error: error.to_string(),
        })?
    };
    let not_table = || ClientError::NotTable {
        path: path.to_string(),
        key: key.to_string(),
    };
    let servers = root
        .as_object_mut()
        .ok_or_else(not_table)?
        .entry(key)
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(not_table)?;
    change(servers);
    let mut out = serde_json::to_string_pretty(&root).expect("JSON values always print");
    out.push('\n');
    Ok(out)
}

fn edit_toml(
    text: &str,
    path: &str,
    change: impl FnOnce(&mut toml_edit::Table),
) -> Result<String, ClientError> {
    let mut doc: toml_edit::DocumentMut =
        text.parse()
            .map_err(|error: toml_edit::TomlError| ClientError::NotToml {
                path: path.to_string(),
                error: error.to_string(),
            })?;
    let servers = doc
        .entry("mcp_servers")
        .or_insert_with(|| {
            let mut table = toml_edit::Table::new();
            table.set_implicit(true);
            toml_edit::Item::Table(table)
        })
        .as_table_mut()
        .ok_or_else(|| ClientError::NotTable {
            path: path.to_string(),
            key: "mcp_servers".to_string(),
        })?;
    change(servers);
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Choice, Choices, Client, Entry, Layout};

    fn entry() -> Entry {
        Entry {
            url: "http://127.0.0.1:7357/mcp".to_string(),
            token: "tok".to_string(),
            binary: PathBuf::from("/Applications/Patchbay.app/Contents/MacOS/patchbay"),
            name: "patchbay".to_string(),
            route: "default".to_string(),
            env: Vec::new(),
        }
    }

    #[test]
    fn claude_code_gets_an_http_entry_beside_its_other_servers() {
        let text = r#"{"numStartups": 3, "mcpServers": {"2do": {"type": "stdio", "command": "x"}}, "zebra": true}"#;
        let changed = Client::ClaudeCode
            .with_entries(text, &[entry()], "f")
            .unwrap();
        insta::assert_snapshot!(changed, @r#"
        {
          "numStartups": 3,
          "mcpServers": {
            "2do": {
              "type": "stdio",
              "command": "x"
            },
            "patchbay": {
              "type": "http",
              "url": "http://127.0.0.1:7357/mcp",
              "headers": {
                "Authorization": "Bearer tok"
              }
            }
          },
          "zebra": true
        }
        "#);
        assert!(!Client::ClaudeCode.entry_names(&changed).is_empty());
        let removed = Client::ClaudeCode.without_entries(&changed, "f").unwrap();
        assert!(Client::ClaudeCode.entry_names(&removed).is_empty());
        assert!(removed.contains("2do"));
    }

    #[test]
    fn claude_desktop_runs_the_bridge() {
        let changed = Client::ClaudeDesktop
            .with_entries("", &[entry()], "f")
            .unwrap();
        insta::assert_snapshot!(changed, @r#"
        {
          "mcpServers": {
            "patchbay": {
              "command": "/Applications/Patchbay.app/Contents/MacOS/patchbay",
              "args": [
                "bridge",
                "default"
              ]
            }
          }
        }
        "#);
    }

    #[test]
    fn codex_gets_a_url_entry_and_keeps_its_comments_and_other_servers() {
        // `codex mcp get --json` reads this back as a streamable HTTP server with the header.
        let text = "# mine\nmodel = \"o3\"\n\n[mcp_servers.other]\ncommand = \"x\" # keep\n";
        let changed = Client::Codex.with_entries(text, &[entry()], "f").unwrap();
        insta::assert_snapshot!(changed, @r#"
        # mine
        model = "o3"

        [mcp_servers.other]
        command = "x" # keep

        [mcp_servers.patchbay]
        url = "http://127.0.0.1:7357/mcp"
        http_headers = { Authorization = "Bearer tok" }
        "#);
        assert!(!Client::Codex.entry_names(&changed).is_empty());
        assert_eq!(Client::Codex.without_entries(&changed, "f").unwrap(), text);
    }

    #[test]
    fn pi_omp_and_pig_get_the_http_entry_their_mcp_add_writes() {
        // `pi mcp add patchbay --url … --header Authorization=…` writes exactly this.
        let pi = Client::Pi.with_entries("", &[entry()], "f").unwrap();
        assert_eq!(Client::Omp.with_entries("", &[entry()], "f").unwrap(), pi);
        // PiG's `pig mcp add --url … --header …` writes the same.
        assert_eq!(Client::Pig.with_entries("", &[entry()], "f").unwrap(), pi);
        {
            insta::assert_snapshot!(pi, @r#"
            {
              "mcpServers": {
                "patchbay": {
                  "url": "http://127.0.0.1:7357/mcp",
                  "headers": {
                    "Authorization": "Bearer tok"
                  }
                }
              }
            }
            "#);
        }
    }

    #[test]
    fn opencode_gets_a_remote_entry_beside_its_own() {
        // `opencode mcp list` connects to this.
        let text = r#"{"$schema": "https://opencode.ai/config.json", "mcp": {"aws-mcp": {"type": "local", "command": ["uvx"]}}}"#;
        let changed = Client::OpenCode
            .with_entries(text, &[entry()], "f")
            .unwrap();
        insta::assert_snapshot!(changed, @r#"
        {
          "$schema": "https://opencode.ai/config.json",
          "mcp": {
            "aws-mcp": {
              "type": "local",
              "command": [
                "uvx"
              ]
            },
            "patchbay": {
              "type": "remote",
              "url": "http://127.0.0.1:7357/mcp",
              "headers": {
                "Authorization": "Bearer tok"
              },
              "enabled": true
            }
          }
        }
        "#);
        assert!(!Client::OpenCode.entry_names(&changed).is_empty());
        assert!(Client::OpenCode.entry_names(text).is_empty());
    }

    #[test]
    fn vs_code_gets_the_http_entry_code_add_mcp_writes() {
        let changed = Client::VsCode.with_entries("", &[entry()], "f").unwrap();
        insta::assert_snapshot!(changed, @r#"
        {
          "servers": {
            "patchbay": {
              "type": "http",
              "url": "http://127.0.0.1:7357/mcp",
              "headers": {
                "Authorization": "Bearer tok"
              }
            }
          }
        }
        "#);
        // Cursor's is the same without the type, which `cursor-agent mcp list-tools` connects to.
        let cursor = Client::Cursor.with_entries("", &[entry()], "f").unwrap();
        assert!(
            cursor.contains("\"url\"") && !cursor.contains("\"type\""),
            "{cursor}"
        );
    }

    #[test]
    fn an_entry_for_each_server_replaces_whatever_patchbay_kept_before() {
        let server = |name: &str| Entry {
            name: format!("patchbay-{name}"),
            url: format!("http://127.0.0.1:7357/mcp/server/{name}"),
            route: format!("server/{name}"),
            ..entry()
        };
        let text = r#"{"mcpServers": {"patchbay": {}, "patchbay-old": {}, "mine": {}}}"#;
        let changed = Client::Pi
            .with_entries(text, &[server("linear"), server("github")], "f")
            .unwrap();
        let mut names = Client::Pi.entry_names(&changed);
        names.sort();
        assert_eq!(names, ["patchbay-github", "patchbay-linear"]);
        assert!(changed.contains("\"mine\""));
        let codex = Client::Codex
            .with_entries("", &[server("linear")], "f")
            .unwrap();
        assert!(
            codex.contains("url = \"http://127.0.0.1:7357/mcp/server/linear\""),
            "{codex}"
        );
        assert_eq!(Client::Codex.entry_names(&codex), ["patchbay-linear"]);
    }

    #[test]
    fn each_clients_choice_comes_back_from_its_file() {
        let root = std::env::temp_dir().join(format!("patchbay-choices-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let home = crate::Home::new(&root);
        assert_eq!(Choices::load(&home).get(Client::Cursor), Choice::default());
        let mut choices = Choices::default();
        let compact = Choice {
            profile: "small".to_string(),
            layout: Layout::Compact,
            ..Choice::default()
        };
        choices.set(Client::Cursor, compact.clone());
        choices.save(&home).unwrap();
        insta::assert_snapshot!(std::fs::read_to_string(root.join("clients.toml")).unwrap(), @r#"
        [cursor]
        profile = "small"
        layout = "compact"
        "#);
        assert_eq!(Choices::load(&home).get(Client::Cursor), compact);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_bridge_carries_where_patchbay_keeps_its_files() {
        let mut entry = entry();
        entry.env = vec![("PATCHBAY_SECRETS".to_string(), "file".to_string())];
        for client in [Client::ClaudeDesktop, Client::GeminiCli] {
            let changed = client
                .with_entries("", std::slice::from_ref(&entry), "f")
                .unwrap();
            assert!(
                changed.contains("\"env\": {\n        \"PATCHBAY_SECRETS\": \"file\""),
                "{changed}"
            );
        }
    }

    #[test]
    fn a_rewritten_config_and_its_backup_stay_private() {
        use std::os::unix::fs::PermissionsExt as _;
        let home = std::env::temp_dir().join(format!("patchbay-perms-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let path = Client::Codex.config_path(&home);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "model = \"o3\"\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        Client::Codex.connect(&home, &[entry()]).unwrap();
        let mode =
            |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(
            mode(&home.join(".codex/config.toml.patchbay-backup")),
            0o600
        );
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn a_config_that_isnt_json_is_left_alone_and_said_why() {
        let error = Client::Cursor
            .with_entries("{nope", &[entry()], "mcp.json")
            .unwrap_err();
        assert!(
            error.to_string().starts_with("mcp.json isn't JSON"),
            "{error}"
        );
    }

    #[test]
    fn connecting_keeps_a_backup_of_the_file_as_it_was() {
        let home = std::env::temp_dir().join(format!("patchbay-clients-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let path = Client::Cursor.config_path(&home);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{\"mcpServers\": {}}").unwrap();
        Client::Cursor.connect(&home, &[entry()]).unwrap();
        assert!(Client::Cursor.connected(&home));
        let backup =
            std::fs::read_to_string(home.join(".cursor/mcp.json.patchbay-backup")).unwrap();
        assert_eq!(backup, "{\"mcpServers\": {}}");
        Client::Cursor.disconnect(&home).unwrap();
        assert!(!Client::Cursor.connected(&home));
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn a_clients_own_servers_read_as_a_paste_and_come_out_leaving_patchbays() {
        let home = std::env::temp_dir().join(format!("patchbay-own-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let path = Client::Cursor.config_path(&home);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text = r#"{"mcpServers": {"github": {"command": "npx", "args": ["-y", "server-github"], "env": {"GITHUB_TOKEN": "ghp_abc123def456ghi789"}}, "patchbay": {"url": "http://127.0.0.1:7357/mcp"}}}"#;
        std::fs::write(&path, text).unwrap();
        let own = Client::Cursor.own_servers(&home);
        let names: Vec<&str> = own.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["github"]);
        let pasted = serde_json::json!({ "mcpServers": own.iter().cloned().collect::<serde_json::Map<_, _>>() });
        let read = crate::paste::read(&pasted.to_string()).unwrap();
        assert_eq!(read[0].name, "github");
        assert!(
            read[0].secrets[0].value.is_some(),
            "the token is taken out, to be kept in the Keychain"
        );
        Client::Cursor
            .remove_own(&home, &["github".to_string()])
            .unwrap();
        assert_eq!(Client::Cursor.entries_in(&home), ["patchbay"]);
        assert!(Client::Cursor.own_servers(&home).is_empty());
        assert!(home.join(".cursor/mcp.json.patchbay-backup").is_file());
        // Connecting again replaces the usual backup, but the one from before the take-over still has github.
        Client::Cursor.connect(&home, &[]).unwrap();
        let before =
            std::fs::read_to_string(home.join(".cursor/mcp.json.patchbay-take-over-backup"))
                .unwrap();
        assert!(before.contains("server-github"), "{before}");
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn a_server_patchbay_already_has_is_left_in_the_client_when_taking_over() {
        let home = std::env::temp_dir().join(format!("patchbay-own-kept-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let path = Client::Cursor.config_path(&home);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text = r#"{"mcpServers": {"github": {"command": "gh-mcp"}, "linear": {"url": "https://mcp.linear.app/mcp"}}}"#;
        std::fs::write(&path, text).unwrap();
        let own = Client::Cursor
            .own_servers_pasted(&home, &["github".to_string()])
            .unwrap();
        assert_eq!(own.names, ["linear"]);
        assert_eq!(own.left, ["github"]);
        assert!(!own.pasted.contains("gh-mcp"), "{}", own.pasted);
        assert!(
            Client::Cursor
                .own_servers_pasted(&home, &["github".into(), "linear".into()])
                .is_none()
        );
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn codexs_own_servers_read_with_their_headers() {
        let home = std::env::temp_dir().join(format!("patchbay-own-codex-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let path = Client::Codex.config_path(&home);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "model = \"o3\"\n\n[mcp_servers.linear]\nurl = \"https://mcp.linear.app/mcp\"\nhttp_headers = { Authorization = \"Bearer lin_abc\" }\n",
        )
        .unwrap();
        let own = Client::Codex.own_servers(&home);
        assert_eq!(own[0].0, "linear");
        assert_eq!(own[0].1["headers"]["Authorization"], "Bearer lin_abc");
        Client::Codex
            .remove_own(&home, &["linear".to_string()])
            .unwrap();
        let left = std::fs::read_to_string(&path).unwrap();
        assert!(
            left.contains("model = \"o3\"") && !left.contains("linear"),
            "{left}"
        );
        std::fs::remove_dir_all(&home).unwrap();
    }
}
