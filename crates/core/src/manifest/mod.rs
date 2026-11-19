//! `patchbay.toml`: the servers, secrets, profiles and skills a team shares or a person adds, as written. Layering
//! them into what the gateway runs is [`resolve`]'s.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub mod edit;
mod filter;
mod layer;
mod template;

pub use filter::ToolFilter;
pub use layer::{
    Auth, Launch, Layer, Problem, Resolved, Scope, Scoped, SecretPrompt, Server, resolve,
};
pub use template::{MissingSecret, Template, TemplateError};

/// One `patchbay.toml`, as written. Every field of a server is optional, since a person's layer may change only one
/// field of a team's server.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub team: Option<Team>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub secrets: BTreeMap<String, SecretSpec>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub servers: BTreeMap<String, ServerSpec>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub profiles: BTreeMap<String, ProfileSpec>,
    #[serde(default, skip_serializing_if = "SkillsSpec::is_empty")]
    pub skills: SkillsSpec,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Team {
    pub name: String,
}

/// How to ask someone for a secret the manifest names.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretSpec {
    /// What to ask, as a sentence: "A GitHub token with repo and read:org".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// Where to get one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// A command to run, which talks MCP on its stdin and stdout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// A remote server's endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Only needed for a remote server on the old SSE transport.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<Transport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<OAuthSpec>,
    /// The prefix of its tools' names; the server's own name if not given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsSpec>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    Stdio,
    Http,
    Sse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthKind {
    None,
    Oauth,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OAuthSpec {
    /// A client registered ahead of time, for an authorization server that doesn't register clients itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
}

/// Which of a server's tools are exposed, by name, with `*` matching any run of characters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolsSpec {
    /// Only these, if given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow: Option<Vec<String>>,
    /// Never these, whatever `allow` says.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny: Vec<String>,
}

/// A set of servers exposed together at `/mcp/<profile>`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileSpec {
    pub servers: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillsSpec {
    /// Git repositories with skills in `skills/<name>/SKILL.md`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
    /// The skills, by name, to install from them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub install: Vec<String>,
}

impl SkillsSpec {
    fn is_empty(&self) -> bool {
        self.sources.is_empty() && self.install.is_empty()
    }
}

/// Why a `patchbay.toml` can't be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ManifestError(String);

impl Manifest {
    pub fn from_toml(text: &str) -> Result<Manifest, ManifestError> {
        toml::from_str(text).map_err(|error| ManifestError(error.to_string()))
    }

    pub fn to_toml(&self) -> String {
        toml::to_string(self).expect("a manifest is always TOML")
    }
}

/// Whether `name` can name a server, a namespace or a profile: lowercase letters, digits, `-` and `_`, starting with
/// a letter or digit, and never `__`, which separates a namespace from a tool's name.
pub fn is_server_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_'))
        && !name.contains("__")
}

#[cfg(test)]
mod tests {
    use super::{AuthKind, Manifest, is_server_name};

    const TEAM: &str = r#"
[team]
name = "Phin Tech"

[secrets.github-token]
prompt = "A GitHub token with repo and read:org"
help = "https://github.com/settings/personal-access-tokens/new"

[servers.github]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github@2025.4.8"]
env = { GITHUB_PERSONAL_ACCESS_TOKEN = "secret:github-token" }

[servers.sentry]
url = "https://mcp.sentry.dev/mcp"
auth = "oauth"

[profiles.default]
servers = ["github", "sentry"]

[skills]
sources = ["https://github.com/phin-tech/skills"]
install = ["pi-cold-review"]
"#;

    #[test]
    fn a_team_manifest_reads_and_writes_back_the_same() {
        let manifest = Manifest::from_toml(TEAM).unwrap();
        assert_eq!(manifest.team.as_ref().unwrap().name, "Phin Tech");
        assert_eq!(manifest.servers["sentry"].auth, Some(AuthKind::Oauth));
        assert_eq!(manifest.skills.install, ["pi-cold-review"]);
        assert_eq!(Manifest::from_toml(&manifest.to_toml()).unwrap(), manifest);
    }

    #[test]
    fn a_misspelt_field_is_an_error_rather_than_ignored() {
        let error = Manifest::from_toml("[servers.github]\nkommand = \"npx\"\n").unwrap_err();
        assert!(error.to_string().contains("kommand"), "{error}");
    }

    #[test]
    fn server_names_keep_the_namespace_separator_free() {
        assert!(is_server_name("github-work"));
        assert!(is_server_name("s3_logs"));
        assert!(!is_server_name("GitHub"));
        assert!(!is_server_name("git__hub"));
        assert!(!is_server_name("-x"));
        assert!(!is_server_name(""));
    }
}
