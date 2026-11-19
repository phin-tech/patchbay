//! A LiteLLM proxy's MCP servers, kept as a team's layer: the servers its `GET /v1/mcp/server` lists for the
//! person's key, each written as a remote server at LiteLLM's own path for it.
//!
//! What's written is only ever URLs and the key's header, never a command, whatever LiteLLM lists: the servers run
//! where LiteLLM runs, and LiteLLM holds their credentials. A LiteLLM has no commits, so the version a person
//! accepts is a hash of the manifest written, which only changes when what LiteLLM lists does.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::process::{Command, Stdio};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::manifest::{Manifest, SecretSpec, ServerSpec, Team, is_server_name};
use crate::share::TeamError;

/// The secret holding the person's LiteLLM key, under the team's folder: it both lists the servers and reaches them.
pub const KEY: &str = "litellm-key";

/// The header LiteLLM's MCP endpoints read the key from.
const HEADER: &str = "x-litellm-api-key";

/// The separator LiteLLM puts between a server's name and each of its tools' names, unless its
/// `MCP_TOOL_PREFIX_SEPARATOR` says otherwise. If it does, the names just keep their prefix.
const SEPARATOR: &str = "-";

/// The Keychain account the key for the LiteLLM kept under `folder` is in.
pub fn account(folder: &str) -> String {
    format!("{folder}/{KEY}")
}

/// The proxy's URL as Patchbay keeps it: `http` or `https`, without a trailing `/`, or the `/ui` its admin page is at,
/// which is what people tend to copy.
pub fn base_url(url: &str) -> Result<String, TeamError> {
    let url = url.trim().trim_end_matches('/');
    let url = url.strip_suffix("/ui").unwrap_or(url).trim_end_matches('/');
    let host = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"));
    match host {
        Some(host) if !host.is_empty() && !host.contains(char::is_whitespace) => {
            Ok(url.to_string())
        }
        _ => Err(TeamError::NotLiteLlm(url.to_string())),
    }
}

/// One server as `GET /v1/mcp/server` lists it. A key that isn't an admin's gets little more than these.
#[derive(Debug, Deserialize)]
struct Listed {
    server_name: Option<String>,
    alias: Option<String>,
    /// `active`, or a server still waiting for an admin to approve it.
    approval_status: Option<String>,
}

impl Listed {
    /// What LiteLLM calls it in paths and before its tools' names: its alias if it has one.
    fn shown(&self) -> Option<&str> {
        self.alias
            .as_deref()
            .or(self.server_name.as_deref())
            .map(str::trim)
            .filter(|name| !name.is_empty())
    }
}

/// The manifest for the servers LiteLLM at `base` listed (`listed`, the body it answered with), kept as the team
/// `name`. Written the same way every time for the same servers, so its hash only moves when they do.
pub fn manifest(base: &str, name: &str, listed: &str) -> Result<String, TeamError> {
    let servers: Vec<Listed> = serde_json::from_str(listed).map_err(|_| {
        TeamError::LiteLlm(format!(
            "{base} didn't answer as a LiteLLM proxy lists its MCP servers"
        ))
    })?;
    let mut shown: Vec<&str> = servers
        .iter()
        .filter(|server| {
            server
                .approval_status
                .as_deref()
                .is_none_or(|status| status == "active")
        })
        .filter_map(Listed::shown)
        .collect();
    shown.sort_unstable();
    shown.dedup();
    let mut manifest = Manifest {
        team: Some(Team {
            name: name.to_string(),
        }),
        ..Manifest::default()
    };
    manifest.secrets.insert(
        KEY.to_string(),
        SecretSpec {
            prompt: Some("Your LiteLLM key (sk-…)".to_string()),
            help: Some(format!("{base}/ui")),
        },
    );
    let mut taken = BTreeSet::new();
    for shown in shown {
        let spec = ServerSpec {
            url: Some(format!("{base}/{}/mcp", path_segment(shown))),
            headers: Some(BTreeMap::from([(
                HEADER.to_string(),
                format!("Bearer ${{secret:{KEY}}}"),
            )])),
            tool_prefix: Some(format!("{shown}{SEPARATOR}")),
            ..ServerSpec::default()
        };
        manifest
            .servers
            .insert(server_name(shown, &mut taken), spec);
    }
    Ok(format!(
        "# Written by Patchbay from the MCP servers LiteLLM at {base} lists for your key. Patchbay writes it again\n\
         # when they change, so change them in LiteLLM.\n\n{}",
        manifest.to_toml()
    ))
}

/// The version of a manifest [`manifest`] wrote, which a person accepts in place of a commit.
pub fn version(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Whether `version` is a whole version as [`version`] gives one.
pub fn is_version(version: &str) -> bool {
    version.len() == 64 && version.chars().all(|c| c.is_ascii_hexdigit())
}

/// What LiteLLM at `base` lists for `key`: the body of its `GET /v1/mcp/server`.
///
/// Asked with curl, as skills and teams ask git: the core has no HTTP client of its own, and stays synchronous. The
/// key goes to curl on its standard input, never in its arguments, where any process could read it.
pub fn list(base: &str, key: &str) -> Result<String, TeamError> {
    let key = key.trim();
    if key.is_empty() {
        return Err(TeamError::NoKey);
    }
    let mut curl = Command::new("/usr/bin/curl")
        .args([
            "--silent",
            "--show-error",
            "--max-time",
            "30",
            "--proto",
            "=http,https",
            "--config",
            "-",
            "--write-out",
            "\n%{http_code}",
        ])
        .arg(format!("{base}/v1/mcp/server"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| TeamError::LiteLlm(format!("Couldn't run curl: {error}")))?;
    if let Some(mut stdin) = curl.stdin.take() {
        let escaped = key.replace('\\', "\\\\").replace('"', "\\\"");
        writeln!(stdin, "header = \"Authorization: Bearer {escaped}\"")
            .map_err(|error| TeamError::LiteLlm(format!("Couldn't run curl: {error}")))?;
    }
    let output = curl
        .wait_with_output()
        .map_err(|error| TeamError::LiteLlm(format!("Couldn't run curl: {error}")))?;
    if !output.status.success() {
        let said = String::from_utf8_lossy(&output.stderr);
        let said = said.trim().trim_start_matches("curl: ");
        return Err(TeamError::LiteLlm(format!("Couldn't reach {base}: {said}")));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let (body, status) = text.rsplit_once('\n').unwrap_or(("", &text));
    match status.trim() {
        "200" => Ok(body.to_string()),
        "401" | "403" => Err(TeamError::KeyRefused(base.to_string())),
        "404" => Err(TeamError::LiteLlm(format!(
            "{base} has no MCP servers to list: is it a LiteLLM proxy?"
        ))),
        status => Err(TeamError::LiteLlm(format!(
            "{base} answered {status} when asked for its MCP servers"
        ))),
    }
}

/// A name Patchbay can give the server LiteLLM calls `shown`, other than any in `taken`: `Clock_MCP` is `clock_mcp`.
fn server_name(shown: &str, taken: &mut BTreeSet<String>) -> String {
    let mut name = String::new();
    for c in shown.chars() {
        let c = match c {
            'a'..='z' | '0'..='9' | '-' => c,
            'A'..='Z' => c.to_ascii_lowercase(),
            '_' if !name.ends_with('_') => '_',
            '_' => continue,
            _ => '-',
        };
        name.push(c);
    }
    let name = name.trim_matches(['-', '_']);
    let name = if is_server_name(name) { name } else { "server" };
    let mut chosen = name.to_string();
    let mut count = 1;
    while taken.contains(&chosen) {
        count += 1;
        chosen = format!("{name}-{count}");
    }
    taken.insert(chosen.clone());
    chosen
}

/// `name` as one segment of a URL's path. LiteLLM's names are letters, digits, `_`, `-` and `.`, but one that isn't
/// still reaches the right path.
fn path_segment(name: &str) -> String {
    name.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            byte => format!("%{byte:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{base_url, is_version, manifest, server_name, version};
    use crate::manifest::Manifest;
    use crate::share::TeamError;

    /// What LiteLLM 1.108.12 answered an ordinary key allowed two servers, one from its config and one added in its
    /// admin page.
    const LISTED: &str = include_str!("litellm/listed.json");

    #[test]
    fn litellms_servers_become_remote_servers_at_its_own_path_for_each() {
        let text = manifest("http://localhost:4000", "LiteLLM", LISTED).unwrap();
        insta::assert_snapshot!(text);
        let manifest = Manifest::from_toml(&text).unwrap();
        assert!(
            manifest
                .servers
                .values()
                .all(|server| server.command.is_none() && server.env.is_none()),
            "nothing LiteLLM lists runs on the Mac"
        );
    }

    #[test]
    fn the_same_servers_listed_in_another_order_write_the_same_manifest() {
        let servers: Vec<serde_json::Value> = serde_json::from_str(LISTED).unwrap();
        let reversed: Vec<_> = servers.into_iter().rev().collect();
        let reversed = serde_json::to_string(&reversed).unwrap();
        let one = manifest("http://localhost:4000", "LiteLLM", LISTED).unwrap();
        let other = manifest("http://localhost:4000", "LiteLLM", &reversed).unwrap();
        assert_eq!(version(&one), version(&other));
        assert!(is_version(&version(&one)));
    }

    #[test]
    fn a_server_waiting_for_approval_isnt_written() {
        let listed = r#"[
            {"server_name": "ready", "approval_status": "active"},
            {"server_name": "waiting", "approval_status": "pending_review"},
            {"server_name": "old"}
        ]"#;
        let text = manifest("https://ai.example.com", "AI", listed).unwrap();
        let manifest = Manifest::from_toml(&text).unwrap();
        assert_eq!(
            manifest.servers.keys().collect::<Vec<_>>(),
            ["old", "ready"]
        );
    }

    #[test]
    fn an_answer_that_isnt_a_list_of_servers_is_turned_away() {
        assert!(matches!(
            manifest("https://ai.example.com", "AI", "<html>"),
            Err(TeamError::LiteLlm(_))
        ));
    }

    #[test]
    fn litellms_names_become_names_patchbay_can_give_a_server() {
        let mut taken = BTreeSet::new();
        assert_eq!(server_name("Clock_MCP", &mut taken), "clock_mcp");
        assert_eq!(server_name("clock__mcp", &mut taken), "clock_mcp-2");
        assert_eq!(server_name("_Echo DB.v2_", &mut taken), "echo-db-v2");
        assert_eq!(server_name("日本", &mut taken), "server");
    }

    #[test]
    fn the_url_of_the_proxy_is_kept_without_its_admin_page_or_a_trailing_slash() {
        assert_eq!(
            base_url(" https://ai.example.com/ui/ ").unwrap(),
            "https://ai.example.com"
        );
        assert_eq!(
            base_url("http://localhost:4000/").unwrap(),
            "http://localhost:4000"
        );
        assert!(base_url("ai.example.com").is_err());
        assert!(base_url("https://").is_err());
        assert!(base_url("file:///etc").is_err());
    }
}
