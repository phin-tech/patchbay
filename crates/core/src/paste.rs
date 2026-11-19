//! Reading a server from whatever was pasted: a README's JSON block (Claude Desktop's, Cursor's, VS Code's,
//! opencode's), an `mcp add` command, a bare `npx …`, a URL, or Patchbay's own TOML. Keys pasted in it are taken
//! out, to be kept in the Keychain, and the server names them as secrets instead.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::manifest::{
    AuthKind, Manifest, SecretSpec, ServerSpec, Template, Transport, is_server_name,
};

/// A server read from a paste, ready to add to the person's manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pasted {
    pub name: String,
    pub spec: ServerSpec,
    pub secrets: Vec<PastedSecret>,
}

/// A secret a pasted server names: its value, if the paste held a real one, to keep in the Keychain; if it held a
/// placeholder (`<YOUR_TOKEN>`), Patchbay asks for it instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PastedSecret {
    pub name: String,
    pub spec: SecretSpec,
    pub value: Option<String>,
}

/// Why a paste couldn't be read as a server.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PasteError {
    #[error("Paste a server's config, its command or its URL")]
    Empty,
    #[error(
        "Patchbay couldn't find a server in that. Paste its JSON, its command (npx …) or its URL"
    )]
    Unknown,
    #[error("{0}")]
    Broken(String),
}

/// The servers in `text`.
pub fn read(text: &str) -> Result<Vec<Pasted>, PasteError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(PasteError::Empty);
    }
    if text.starts_with('{') || text.starts_with('"') {
        return from_json(text);
    }
    if text.starts_with('[') {
        return from_toml(text);
    }
    if (text.starts_with("https://") || text.starts_with("http://"))
        && !text.contains(char::is_whitespace)
    {
        return Ok(vec![finish_with_headers(
            name_from_url(text),
            ServerSpec {
                url: Some(text.to_string()),
                ..ServerSpec::default()
            },
            BTreeMap::new(),
        )]);
    }
    from_command(text)
}

fn from_toml(text: &str) -> Result<Vec<Pasted>, PasteError> {
    let manifest =
        Manifest::from_toml(text).map_err(|error| PasteError::Broken(error.to_string()))?;
    if manifest.servers.is_empty() {
        return Err(PasteError::Unknown);
    }
    Ok(manifest
        .servers
        .into_iter()
        .map(|(name, spec)| {
            let used: Vec<String> = references(&spec);
            let secrets = manifest
                .secrets
                .iter()
                .filter(|(secret, _)| used.contains(secret))
                .map(|(secret, spec)| PastedSecret {
                    name: secret.clone(),
                    spec: spec.clone(),
                    value: None,
                })
                .collect();
            Pasted {
                name,
                spec,
                secrets,
            }
        })
        .collect())
}

/// The secrets a server's values name.
fn references(spec: &ServerSpec) -> Vec<String> {
    let values = spec
        .args
        .iter()
        .flatten()
        .chain(spec.env.iter().flat_map(BTreeMap::values))
        .chain(spec.headers.iter().flat_map(BTreeMap::values));
    values
        .filter_map(|value| Template::parse(value).ok())
        .flat_map(|template| template.secrets().map(str::to_string).collect::<Vec<_>>())
        .collect()
}

fn from_json(text: &str) -> Result<Vec<Pasted>, PasteError> {
    // A README often shows only the server's own entry, `"github": { … }`, without the braces round it.
    let value: Value = serde_json::from_str(text)
        .or_else(|_| serde_json::from_str(&format!("{{{text}}}")))
        .or_else(|_| serde_json::from_str(&format!("{{{}}}", text.trim_end_matches(','))))
        .map_err(|error| {
            PasteError::Broken(format!("That isn't JSON Patchbay can read: {error}"))
        })?;
    let object = value.as_object().ok_or(PasteError::Unknown)?;
    let servers = ["mcpServers", "servers", "mcp", "context_servers"]
        .iter()
        .find_map(|key| object.get(*key)?.as_object())
        .unwrap_or(object);
    if is_server(servers) {
        let name = guess_name(servers);
        return server_from_json(&name, servers).map(|pasted| vec![pasted]);
    }
    let found: Vec<Pasted> = servers
        .iter()
        .filter_map(|(name, server)| {
            let server = server.as_object()?;
            is_server(server).then(|| server_from_json(&clean_name(name), server))
        })
        .collect::<Result<_, _>>()?;
    if found.is_empty() {
        Err(PasteError::Unknown)
    } else {
        Ok(found)
    }
}

fn is_server(object: &serde_json::Map<String, Value>) -> bool {
    ["command", "url", "serverUrl", "httpUrl"]
        .iter()
        .any(|key| object.contains_key(*key))
}

fn guess_name(object: &serde_json::Map<String, Value>) -> String {
    if let Some(url) = ["url", "serverUrl", "httpUrl"]
        .iter()
        .find_map(|key| object.get(*key)?.as_str())
    {
        return name_from_url(url);
    }
    let words: Vec<String> = strings(object.get("args"));
    name_from_words(object.get("command").and_then(Value::as_str), &words)
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn map(value: Option<&Value>) -> BTreeMap<String, String> {
    value
        .and_then(Value::as_object)
        .map(|items| {
            items
                .iter()
                .map(|(key, value)| {
                    let value = value
                        .as_str()
                        .map_or_else(|| value.to_string(), str::to_string);
                    (key.clone(), value)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn server_from_json(
    name: &str,
    object: &serde_json::Map<String, Value>,
) -> Result<Pasted, PasteError> {
    let url = ["url", "serverUrl", "httpUrl"]
        .iter()
        .find_map(|key| object.get(*key)?.as_str());
    let mut spec = ServerSpec::default();
    let env = map(object.get("env").or_else(|| object.get("environment")));
    if let Some(url) = url {
        spec.url = Some(url.to_string());
        if object.get("type").and_then(Value::as_str) == Some("sse") {
            spec.transport = Some(Transport::Sse);
        }
        let headers = map(object.get("headers"));
        return Ok(finish_with_headers(name.to_string(), spec, headers));
    }
    // opencode gives the command and its arguments as one list.
    let (command, mut args) = match object.get("command") {
        Some(Value::Array(_)) => {
            let mut words = strings(object.get("command"));
            if words.is_empty() {
                return Err(PasteError::Unknown);
            }
            let command = words.remove(0);
            (command, words)
        }
        Some(Value::String(command)) => (command.clone(), strings(object.get("args"))),
        _ => return Err(PasteError::Unknown),
    };
    // `npx mcp-remote <url>` is a bridge to a remote server; Patchbay reaches remote servers itself.
    if let Some(remote) = remote_url(&command, &args) {
        let mut headers = BTreeMap::new();
        let mut words = args.iter();
        while let Some(word) = words.next() {
            if word == "--header"
                && let Some((key, value)) = words.next().and_then(|header| header.split_once(':'))
            {
                let value = expand(value.trim(), &env);
                headers.insert(key.trim().to_string(), value);
            }
        }
        let spec = ServerSpec {
            url: Some(remote),
            ..ServerSpec::default()
        };
        return Ok(finish_with_headers(name.to_string(), spec, headers));
    }
    spec.cwd = object
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::to_string);
    spec.command = Some(command);
    if !args.is_empty() {
        spec.args = Some(std::mem::take(&mut args));
    }
    Ok(finish(name.to_string(), spec, env))
}

/// The URL an `mcp-remote` command bridges to, if it is one.
fn remote_url(command: &str, args: &[String]) -> Option<String> {
    let words: Vec<&str> = std::iter::once(command)
        .chain(args.iter().map(String::as_str))
        .collect();
    let bridged = words.iter().any(|word| word.starts_with("mcp-remote"));
    bridged
        .then(|| {
            words
                .iter()
                .find(|word| word.starts_with("https://") || word.starts_with("http://"))
                .map(|url| (*url).to_string())
        })
        .flatten()
}

/// `${NAME}` in an `mcp-remote` header filled in from the entry's `env`.
fn expand(value: &str, env: &BTreeMap<String, String>) -> String {
    let mut value = value.to_string();
    for (key, given) in env {
        value = value.replace(&format!("${{{key}}}"), given);
    }
    value
}

fn from_command(text: &str) -> Result<Vec<Pasted>, PasteError> {
    let words = split(text)
        .ok_or_else(|| PasteError::Broken("A quote in that isn't closed".to_string()))?;
    let words: Vec<String> = words.into_iter().filter(|word| word != "\\").collect();
    // `claude mcp add`, `codex mcp add`, `pi mcp add`: a name, options, then a URL or `-- command`.
    if let Some(at) = words
        .windows(2)
        .position(|pair| pair[0] == "mcp" && pair[1] == "add")
    {
        return from_add(&words[at + 2..]);
    }
    let mut env = BTreeMap::new();
    let mut rest = words.as_slice();
    while let Some((first, tail)) = rest.split_first() {
        match first.split_once('=') {
            Some((key, value))
                if key
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') =>
            {
                env.insert(key.to_string(), value.to_string());
                rest = tail;
            }
            _ => break,
        }
    }
    let Some((command, args)) = rest.split_first() else {
        return Err(PasteError::Unknown);
    };
    if !looks_like_command(command) {
        return Err(PasteError::Unknown);
    }
    let name = name_from_words(Some(command), args);
    command_server(name, command.clone(), args.to_vec(), env)
}

fn command_server(
    name: String,
    command: String,
    args: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<Vec<Pasted>, PasteError> {
    if let Some(url) = remote_url(&command, &args) {
        let spec = ServerSpec {
            url: Some(url.clone()),
            ..ServerSpec::default()
        };
        return Ok(vec![finish_with_headers(
            name_from_url(&url),
            spec,
            BTreeMap::new(),
        )]);
    }
    let spec = ServerSpec {
        command: Some(command),
        args: (!args.is_empty()).then_some(args),
        ..ServerSpec::default()
    };
    Ok(vec![finish(name, spec, env)])
}

fn from_add(words: &[String]) -> Result<Vec<Pasted>, PasteError> {
    let (options, command) = match words.iter().position(|word| word == "--") {
        Some(at) => (&words[..at], Some(&words[at + 1..])),
        None => (words, None),
    };
    let mut name = None;
    let mut url = None;
    let mut sse = false;
    let mut env = BTreeMap::new();
    let mut headers = BTreeMap::new();
    let mut options = options.iter();
    while let Some(word) = options.next() {
        let mut value = || options.next().cloned().unwrap_or_default();
        match word.as_str() {
            "--transport" | "-t" => sse = value() == "sse",
            "--env" | "-e" => {
                if let Some((key, given)) = value().split_once('=') {
                    env.insert(key.to_string(), given.to_string());
                }
            }
            "--header" | "-H" => {
                let header = value();
                if let Some((key, given)) =
                    header.split_once(':').or_else(|| header.split_once('='))
                {
                    headers.insert(key.trim().to_string(), given.trim().to_string());
                }
            }
            "--url" => url = Some(value()),
            "--scope" | "-s" | "--cwd" | "--description" | "--exposure" => {
                value();
            }
            word if word.starts_with('-') => {}
            word if word.starts_with("https://") || word.starts_with("http://") => {
                url = Some(word.to_string());
            }
            word if name.is_none() => name = Some(clean_name(word)),
            _ => {}
        }
    }
    if let Some(url) = url {
        let spec = ServerSpec {
            url: Some(url.clone()),
            transport: sse.then_some(Transport::Sse),
            ..ServerSpec::default()
        };
        let name = name.unwrap_or_else(|| name_from_url(&url));
        return Ok(vec![finish_with_headers(name, spec, headers)]);
    }
    let Some((command, args)) = command.and_then(<[String]>::split_first) else {
        return Err(PasteError::Unknown);
    };
    let name = name.unwrap_or_else(|| name_from_words(Some(command), args));
    command_server(name, command.clone(), args.to_vec(), env)
}

fn looks_like_command(word: &str) -> bool {
    [
        "npx", "uvx", "bunx", "pnpm", "yarn", "node", "python", "python3", "uv", "docker", "deno",
        "bun", "go", "cargo",
    ]
    .contains(&word)
        || word.starts_with('/')
        || word.starts_with("./")
        || word.starts_with("~/")
}

/// Shell words, with quotes and backslash escapes, or none if a quote isn't closed.
fn split(text: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') => word.push(chars.next()?),
            (Some(_), c) => word.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                started = true;
            }
            (None, '\\') => match chars.next()? {
                '\n' => {}
                next => word.push(next),
            },
            (None, c) if c.is_whitespace() => {
                if started || !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            (None, c) => word.push(c),
        }
    }
    if quote.is_some() {
        return None;
    }
    if started || !word.is_empty() {
        words.push(word);
    }
    Some(words)
}

/// A server's name from its package: `@modelcontextprotocol/server-github` is `github`, `mcp-server-fetch` is
/// `fetch`.
fn name_from_words(command: Option<&str>, args: &[String]) -> String {
    let package = args
        .iter()
        .find(|arg| !arg.starts_with('-') && !arg.contains("://"))
        .map_or_else(|| command.unwrap_or("server").to_string(), Clone::clone);
    let package = package.rsplit('/').next().unwrap_or(&package);
    let package = package.split('@').next().unwrap_or(package);
    let mut name = package.to_lowercase();
    for affix in [
        "-mcp-server",
        "mcp-server-",
        "server-",
        "mcp-",
        "-mcp",
        "-server",
        ".py",
        ".js",
    ] {
        name = name.replace(affix, "");
    }
    clean_name(&name)
}

/// A server's name from its URL: `https://mcp.linear.app/mcp` is `linear`.
fn name_from_url(url: &str) -> String {
    let host = url
        .split("://")
        .nth(1)
        .unwrap_or(url)
        .split(['/', ':', '?'])
        .next()
        .unwrap_or_default();
    let parts: Vec<&str> = host
        .split('.')
        .filter(|part| {
            ![
                "mcp", "api", "www", "app", "com", "io", "dev", "ai", "net", "org", "sh",
            ]
            .contains(part)
        })
        .collect();
    clean_name(parts.first().copied().unwrap_or(host))
}

/// `name` made into a server's name: lowercase letters, digits and hyphens.
fn clean_name(name: &str) -> String {
    let cleaned: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let cleaned = cleaned
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if is_server_name(&cleaned) {
        cleaned
    } else {
        "server".to_string()
    }
}

/// A server with its `env` checked for keys: each one that looks secret becomes a secret of the server's.
/// The server `name`, given field by field, with any key in its env or headers taken out to keep in the Keychain, as
/// a pasted one's are. Its `auth` is kept as given: a server given no headers is asked to sign in only if it says so.
pub fn with_keys_taken_out(name: &str, mut spec: ServerSpec) -> Pasted {
    let auth = spec.auth;
    let env = spec.env.take().unwrap_or_default();
    let headers = spec.headers.take().unwrap_or_default();
    let from_env = finish(name.to_string(), spec, env);
    let mut pasted = finish_with_headers(name.to_string(), from_env.spec, headers);
    pasted.secrets.splice(0..0, from_env.secrets);
    pasted.spec.auth = auth;
    pasted
}

fn finish(name: String, mut spec: ServerSpec, env: BTreeMap<String, String>) -> Pasted {
    let mut secrets = Vec::new();
    let mut kept = BTreeMap::new();
    for (key, value) in env {
        let named = value.starts_with("secret:") || value.contains("${secret:");
        if named || !is_secret_key(&key) && !is_placeholder(&value) {
            kept.insert(key, value);
            continue;
        }
        let secret = secret_name(&name, &key);
        secrets.push(new_secret(&secret, &key, &name, &value));
        kept.insert(key, format!("secret:{secret}"));
    }
    if !kept.is_empty() {
        spec.env = Some(kept);
    }
    Pasted {
        name,
        spec,
        secrets,
    }
}

/// A remote server with its headers checked for keys. A remote server with no key is asked to sign in, which a
/// server that doesn't need it never asks for.
fn finish_with_headers(
    name: String,
    mut spec: ServerSpec,
    headers: BTreeMap<String, String>,
) -> Pasted {
    let mut secrets = Vec::new();
    let mut kept = BTreeMap::new();
    for (key, value) in headers {
        let lower = key.to_lowercase();
        let secretish = lower == "authorization" || is_secret_key(&key) || is_placeholder(&value);
        if value.contains("secret:") || !secretish {
            kept.insert(key, value);
            continue;
        }
        let (prefix, token) = match value.split_once(' ') {
            Some((scheme, token))
                if ["bearer", "token", "basic"].contains(&scheme.to_lowercase().as_str()) =>
            {
                (format!("{scheme} "), token.to_string())
            }
            _ => (String::new(), value.clone()),
        };
        let secret = if lower == "authorization" {
            format!("{name}-token")
        } else {
            secret_name(&name, &key)
        };
        secrets.push(new_secret(&secret, &key, &name, &token));
        kept.insert(key, format!("{prefix}${{secret:{secret}}}"));
    }
    if !kept.is_empty() {
        spec.headers = Some(kept);
    } else if spec.auth.is_none() {
        spec.auth = Some(AuthKind::Oauth);
    }
    Pasted {
        name,
        spec,
        secrets,
    }
}

fn new_secret(secret: &str, key: &str, server: &str, value: &str) -> PastedSecret {
    PastedSecret {
        name: secret.to_string(),
        spec: SecretSpec {
            prompt: Some(format!("{server}'s {key}")),
            help: None,
        },
        value: (!is_placeholder(value)).then(|| value.to_string()),
    }
}

/// `brave-api-key` for `BRAVE_API_KEY` on `brave`; the server's name first unless the key already starts with it.
fn secret_name(server: &str, key: &str) -> String {
    let key = clean_name(key);
    if key.starts_with(server) {
        key
    } else {
        format!("{server}-{key}")
    }
}

fn is_secret_key(key: &str) -> bool {
    let key = key.to_uppercase();
    [
        "TOKEN",
        "KEY",
        "SECRET",
        "PASSWORD",
        "PASS",
        "PAT",
        "AUTH",
        "CREDENTIAL",
    ]
    .iter()
    .any(|word| key.contains(word))
}

/// Whether a value is a README's stand-in for the person's own: empty, `<…>`, `YOUR_…`, `xxx`, `${…}`.
fn is_placeholder(value: &str) -> bool {
    let lower = value.to_lowercase();
    value.trim().is_empty()
        || value.contains('<')
        || value.contains("${")
        || lower.contains("your")
        || lower.contains("xxx")
        || lower.contains("here")
        || lower.contains("replace")
        || lower == "..."
}

#[cfg(test)]
mod tests {
    use super::{PasteError, read};
    use crate::manifest::AuthKind;

    #[test]
    fn a_claude_desktop_block_gives_its_servers_and_takes_the_key_out() {
        let pasted = read(
            r#"{
              "mcpServers": {
                "github": {
                  "command": "npx",
                  "args": ["-y", "@modelcontextprotocol/server-github"],
                  "env": { "GITHUB_PERSONAL_ACCESS_TOKEN": "ghp_realtoken123", "LOG_LEVEL": "info" }
                }
              }
            }"#,
        )
        .unwrap();
        assert_eq!(pasted.len(), 1);
        let github = &pasted[0];
        assert_eq!(github.name, "github");
        assert_eq!(github.spec.command.as_deref(), Some("npx"));
        let env = github.spec.env.as_ref().unwrap();
        assert_eq!(
            env["GITHUB_PERSONAL_ACCESS_TOKEN"],
            "secret:github-personal-access-token"
        );
        assert_eq!(env["LOG_LEVEL"], "info");
        assert_eq!(github.secrets[0].value.as_deref(), Some("ghp_realtoken123"));
    }

    #[test]
    fn a_placeholder_key_is_asked_for_rather_than_kept() {
        let pasted = read(r#""brave": {"command": "npx", "args": ["-y", "@brave/brave-search-mcp-server"], "env": {"BRAVE_API_KEY": "YOUR_API_KEY_HERE"}}"#).unwrap();
        assert_eq!(pasted[0].name, "brave");
        assert_eq!(pasted[0].secrets[0].name, "brave-api-key");
        assert_eq!(pasted[0].secrets[0].value, None);
    }

    #[test]
    fn a_url_alone_is_a_remote_server_that_signs_in() {
        let pasted = read("https://mcp.linear.app/mcp").unwrap();
        assert_eq!(pasted[0].name, "linear");
        assert_eq!(
            pasted[0].spec.url.as_deref(),
            Some("https://mcp.linear.app/mcp")
        );
        assert_eq!(pasted[0].spec.auth, Some(AuthKind::Oauth));
    }

    #[test]
    fn claude_mcp_add_reads_its_name_url_and_header() {
        let pasted = read(
            "claude mcp add --transport http sentry https://mcp.sentry.dev/mcp --header \"Authorization: Bearer sntrys_abc\"",
        )
        .unwrap();
        let sentry = &pasted[0];
        assert_eq!(sentry.name, "sentry");
        assert_eq!(
            sentry.spec.headers.as_ref().unwrap()["Authorization"],
            "Bearer ${secret:sentry-token}"
        );
        assert_eq!(sentry.secrets[0].value.as_deref(), Some("sntrys_abc"));
        assert_eq!(sentry.spec.auth, None);
    }

    #[test]
    fn a_command_with_its_key_before_it_is_a_server() {
        let pasted =
            read("BRAVE_API_KEY=abc123 npx -y @modelcontextprotocol/server-brave-search").unwrap();
        assert_eq!(pasted[0].name, "brave-search");
        assert_eq!(
            pasted[0].spec.args.as_deref(),
            Some(
                &[
                    "-y".to_string(),
                    "@modelcontextprotocol/server-brave-search".to_string()
                ][..]
            )
        );
        assert_eq!(pasted[0].secrets[0].value.as_deref(), Some("abc123"));
        let codex = read("codex mcp add fetch -- uvx mcp-server-fetch").unwrap();
        assert_eq!(codex[0].name, "fetch");
        assert_eq!(codex[0].spec.command.as_deref(), Some("uvx"));
    }

    #[test]
    fn mcp_remote_becomes_the_remote_server_it_bridges_to() {
        let pasted = read(
            r#"{"mcpServers": {"notion": {"command": "npx", "args": ["-y", "mcp-remote", "https://mcp.notion.com/mcp"]}}}"#,
        )
        .unwrap();
        assert_eq!(pasted[0].name, "notion");
        assert_eq!(
            pasted[0].spec.url.as_deref(),
            Some("https://mcp.notion.com/mcp")
        );
        assert_eq!(pasted[0].spec.command, None);
    }

    #[test]
    fn vs_code_and_opencode_blocks_are_read_too() {
        let vscode = read(r#"{"servers": {"playwright": {"type": "stdio", "command": "npx", "args": ["@playwright/mcp@latest"]}}}"#).unwrap();
        assert_eq!(vscode[0].name, "playwright");
        let opencode = read(r#"{"mcp": {"fs": {"type": "local", "command": ["npx", "-y", "server-filesystem", "/tmp"]}}}"#).unwrap();
        assert_eq!(opencode[0].spec.command.as_deref(), Some("npx"));
        assert_eq!(opencode[0].spec.args.as_ref().unwrap().len(), 3);
    }

    #[test]
    fn patchbays_own_toml_comes_back_with_its_secrets() {
        let pasted = read(
            "[secrets.k]\nprompt = \"A key\"\n\n[servers.x]\nurl = \"https://x.dev/mcp\"\nheaders = { Authorization = \"Bearer ${secret:k}\" }\n",
        )
        .unwrap();
        assert_eq!(pasted[0].name, "x");
        assert_eq!(pasted[0].secrets[0].name, "k");
    }

    #[test]
    fn a_paste_with_no_server_says_so() {
        assert_eq!(read("  "), Err(PasteError::Empty));
        assert_eq!(read("hello there"), Err(PasteError::Unknown));
        assert!(matches!(read("{nope"), Err(PasteError::Broken(_))));
    }
}
