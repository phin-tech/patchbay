//! Layering manifests, lowest first (each team's, then the person's own), into the servers the gateway runs.

use super::*;

/// Whose a layer is, which decides where its secrets are kept: a team's `github-token` and yours are different
/// Keychain items, so two teams naming a secret alike don't share it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scope {
    /// A team's manifest, by the short name its subscription is kept under.
    Team(String),
    /// The person's own manifest.
    Me,
}

impl Scope {
    /// The Keychain account a secret of this scope is kept under: `phin-tech/github-token`, `me/github-token`.
    pub fn account(&self, secret: &str) -> String {
        match self {
            Scope::Team(team) => format!("{team}/{secret}"),
            Scope::Me => format!("me/{secret}"),
        }
    }
}

/// One manifest and whose it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    pub scope: Scope,
    pub manifest: Manifest,
}

/// A value and the scope of the layer that set it, so its secrets are looked for in the right place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scoped<T> {
    pub scope: Scope,
    pub value: T,
}

impl Scoped<Template> {
    /// The Keychain accounts its secrets are kept under.
    pub fn accounts(&self) -> impl Iterator<Item = String> + '_ {
        self.value
            .secrets()
            .map(|secret| self.scope.account(secret))
    }

    /// The value with its secrets filled in from `lookup`, which is given Keychain accounts.
    pub fn render(
        &self,
        mut lookup: impl FnMut(&str) -> Option<String>,
    ) -> Result<String, MissingSecret> {
        self.value
            .render(|secret| lookup(&self.scope.account(secret)))
            .map_err(|MissingSecret(secret)| MissingSecret(self.scope.account(&secret)))
    }
}

/// A server, layered and checked, ready to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Server {
    pub name: String,
    pub enabled: bool,
    pub launch: Launch,
    pub namespace: String,
    pub tools: ToolFilter,
    /// The lowest layer that names it, which is where it came from.
    pub from: Scope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launch {
    Stdio {
        command: String,
        args: Vec<Scoped<Template>>,
        env: BTreeMap<String, Scoped<Template>>,
        cwd: Option<String>,
    },
    Http {
        url: String,
        sse: bool,
        headers: BTreeMap<String, Scoped<Template>>,
        auth: Auth,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Auth {
    None,
    OAuth {
        client_id: Option<String>,
        scopes: Vec<String>,
    },
}

impl Server {
    /// Every Keychain account its values need, each once.
    pub fn accounts(&self) -> Vec<String> {
        let values: Vec<&Scoped<Template>> = match &self.launch {
            Launch::Stdio { args, env, .. } => args.iter().chain(env.values()).collect(),
            Launch::Http { headers, .. } => headers.values().collect(),
        };
        let mut accounts: Vec<String> = values.into_iter().flat_map(Scoped::accounts).collect();
        accounts.sort();
        accounts.dedup();
        accounts
    }

    /// The Keychain account its OAuth tokens are kept under.
    pub fn oauth_account(&self) -> String {
        format!("oauth/{}", self.name)
    }
}

/// How to ask for a secret, for the Keychain account it's kept under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretPrompt {
    pub account: String,
    pub name: String,
    pub scope: Scope,
    pub prompt: String,
    pub help: Option<String>,
}

/// Something wrong in the manifests that stops one server, profile or secret, but not the rest.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Problem {
    #[error("“{0}” isn't a server's name: use lowercase letters, digits, hyphens and underscores")]
    BadName(String),
    #[error("{0} has both a command and a url; give it one")]
    CommandAndUrl(String),
    #[error("{0} has neither a command nor a url")]
    NoLaunch(String),
    #[error("{server}: {error}")]
    Template {
        server: String,
        error: TemplateError,
    },
    #[error("{server} uses the secret {secret}, which the manifest doesn't describe")]
    UndescribedSecret { server: String, secret: String },
    #[error("the profile {profile} names {server}, which isn't a server")]
    UnknownServer { profile: String, server: String },
    #[error("{server} and {other} both use the namespace {namespace}")]
    SharedNamespace {
        server: String,
        other: String,
        namespace: String,
    },
}

/// Everything the layers add up to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resolved {
    pub servers: BTreeMap<String, Server>,
    /// Each profile's servers. `default` is every enabled server unless a layer says otherwise.
    pub profiles: BTreeMap<String, Vec<String>>,
    /// How to ask for each secret, by Keychain account.
    pub secrets: BTreeMap<String, SecretPrompt>,
    pub skills: SkillsSpec,
    pub problems: Vec<Problem>,
}

impl Resolved {
    /// The Keychain accounts `server` needs that `has` says are missing, with how to ask for each.
    pub fn missing_secrets(
        &self,
        server: &Server,
        mut has: impl FnMut(&str) -> bool,
    ) -> Vec<SecretPrompt> {
        server
            .accounts()
            .into_iter()
            .filter(|account| !has(account))
            .filter_map(|account| self.secrets.get(&account).cloned())
            .collect()
    }
}

/// A server's fields as the layers so far have set them.
#[derive(Default)]
struct Draft {
    from: Option<Scope>,
    enabled: Option<bool>,
    command: Option<String>,
    args: Option<Scoped<Vec<String>>>,
    env: BTreeMap<String, Scoped<String>>,
    cwd: Option<String>,
    url: Option<String>,
    transport: Option<Transport>,
    headers: BTreeMap<String, Scoped<String>>,
    auth: Option<AuthKind>,
    oauth: Option<OAuthSpec>,
    namespace: Option<String>,
    tools: Option<ToolsSpec>,
}

impl Draft {
    fn apply(&mut self, scope: &Scope, spec: &ServerSpec) {
        self.from.get_or_insert_with(|| scope.clone());
        let scoped = |value: &String| Scoped {
            scope: scope.clone(),
            value: value.clone(),
        };
        if spec.enabled.is_some() {
            self.enabled = spec.enabled;
        }
        if spec.command.is_some() {
            self.command.clone_from(&spec.command);
        }
        if let Some(args) = &spec.args {
            self.args = Some(Scoped {
                scope: scope.clone(),
                value: args.clone(),
            });
        }
        for (key, value) in spec.env.iter().flatten() {
            self.env.insert(key.clone(), scoped(value));
        }
        if spec.cwd.is_some() {
            self.cwd.clone_from(&spec.cwd);
        }
        if spec.url.is_some() {
            self.url.clone_from(&spec.url);
        }
        if spec.transport.is_some() {
            self.transport = spec.transport;
        }
        for (key, value) in spec.headers.iter().flatten() {
            self.headers.insert(key.clone(), scoped(value));
        }
        if spec.auth.is_some() {
            self.auth = spec.auth;
        }
        if spec.oauth.is_some() {
            self.oauth.clone_from(&spec.oauth);
        }
        if spec.namespace.is_some() {
            self.namespace.clone_from(&spec.namespace);
        }
        if spec.tools.is_some() {
            self.tools.clone_from(&spec.tools);
        }
    }

    fn finish(self, name: &str) -> Result<Server, Problem> {
        let template = |value: &Scoped<String>| {
            Template::parse(&value.value)
                .map(|template| Scoped {
                    scope: value.scope.clone(),
                    value: template,
                })
                .map_err(|error| Problem::Template {
                    server: name.to_string(),
                    error,
                })
        };
        let templates = |values: &BTreeMap<String, Scoped<String>>| {
            values
                .iter()
                .map(|(key, value)| Ok((key.clone(), template(value)?)))
                .collect::<Result<BTreeMap<_, _>, Problem>>()
        };
        let launch = match (self.command, self.url) {
            (Some(_), Some(_)) => return Err(Problem::CommandAndUrl(name.to_string())),
            (None, None) => return Err(Problem::NoLaunch(name.to_string())),
            (Some(command), None) => {
                let args = match &self.args {
                    Some(args) => args
                        .value
                        .iter()
                        .map(|arg| {
                            template(&Scoped {
                                scope: args.scope.clone(),
                                value: arg.clone(),
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    None => Vec::new(),
                };
                Launch::Stdio {
                    command,
                    args,
                    env: templates(&self.env)?,
                    cwd: self.cwd,
                }
            }
            (None, Some(url)) => Launch::Http {
                url,
                sse: self.transport == Some(Transport::Sse),
                headers: templates(&self.headers)?,
                auth: match self.auth {
                    Some(AuthKind::Oauth) => {
                        let oauth = self.oauth.unwrap_or_default();
                        Auth::OAuth {
                            client_id: oauth.client_id,
                            scopes: oauth.scopes.unwrap_or_default(),
                        }
                    }
                    Some(AuthKind::None) | None => Auth::None,
                },
            },
        };
        let namespace = self.namespace.unwrap_or_else(|| name.to_string());
        if !is_server_name(&namespace) {
            return Err(Problem::BadName(namespace));
        }
        Ok(Server {
            name: name.to_string(),
            enabled: self.enabled.unwrap_or(true),
            launch,
            namespace,
            tools: self.tools.as_ref().map(ToolFilter::new).unwrap_or_default(),
            from: self.from.unwrap_or(Scope::Me),
        })
    }
}

/// Whether the teams' layers among `layers` leave the server `name` on, before the person's own has its say: a team
/// can offer a server without turning it on.
pub fn enabled_beneath(layers: &[Layer], name: &str) -> bool {
    layers
        .iter()
        .rev()
        .filter(|layer| layer.scope != Scope::Me)
        .find_map(|layer| layer.manifest.servers.get(name)?.enabled)
        .unwrap_or(true)
}

/// The servers, profiles and secrets `layers` add up to, lowest first. A later layer changes only the fields it
/// sets; `env` and `headers` change key by key.
pub fn resolve(layers: &[Layer]) -> Resolved {
    let mut resolved = Resolved::default();
    let mut drafts: BTreeMap<String, Draft> = BTreeMap::new();
    let mut profiles: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for layer in layers {
        for (name, spec) in &layer.manifest.secrets {
            let account = layer.scope.account(name);
            resolved.secrets.insert(
                account.clone(),
                SecretPrompt {
                    account,
                    name: name.clone(),
                    scope: layer.scope.clone(),
                    prompt: spec.prompt.clone().unwrap_or_else(|| name.clone()),
                    help: spec.help.clone(),
                },
            );
        }
        for (name, spec) in &layer.manifest.servers {
            drafts
                .entry(name.clone())
                .or_default()
                .apply(&layer.scope, spec);
        }
        for (name, profile) in &layer.manifest.profiles {
            profiles.insert(name.clone(), profile.servers.clone());
        }
        resolved
            .skills
            .sources
            .extend(layer.manifest.skills.sources.iter().cloned());
        resolved
            .skills
            .install
            .extend(layer.manifest.skills.install.iter().cloned());
    }
    resolved.skills.sources.dedup();
    resolved.skills.install.dedup();

    let mut namespaces: BTreeMap<String, String> = BTreeMap::new();
    for (name, draft) in drafts {
        if !is_server_name(&name) {
            resolved.problems.push(Problem::BadName(name));
            continue;
        }
        let server = match draft.finish(&name) {
            Ok(server) => server,
            Err(problem) => {
                resolved.problems.push(problem);
                continue;
            }
        };
        if let Some(other) = namespaces.get(&server.namespace) {
            resolved.problems.push(Problem::SharedNamespace {
                server: name,
                other: other.clone(),
                namespace: server.namespace,
            });
            continue;
        }
        namespaces.insert(server.namespace.clone(), name.clone());
        for account in server.accounts() {
            if resolved.secrets.contains_key(&account) {
                continue;
            }
            let (scope, secret) = scope_of(&server, &account);
            if matches!(scope, Scope::Team(_)) {
                resolved.problems.push(Problem::UndescribedSecret {
                    server: name.clone(),
                    secret: secret.clone(),
                });
            }
            resolved.secrets.insert(
                account.clone(),
                SecretPrompt {
                    account,
                    prompt: secret.clone(),
                    name: secret,
                    scope,
                    help: None,
                },
            );
        }
        resolved.servers.insert(name, server);
    }

    for (profile, servers) in &profiles {
        for server in servers {
            if !resolved.servers.contains_key(server) {
                resolved.problems.push(Problem::UnknownServer {
                    profile: profile.clone(),
                    server: server.clone(),
                });
            }
        }
    }
    profiles.entry("default".to_string()).or_insert_with(|| {
        resolved
            .servers
            .values()
            .filter(|server| server.enabled)
            .map(|server| server.name.clone())
            .collect()
    });
    for servers in profiles.values_mut() {
        servers.retain(|server| {
            resolved
                .servers
                .get(server)
                .is_some_and(|server| server.enabled)
        });
    }
    resolved.profiles = profiles;
    resolved
}

/// The scope and secret name of one of `server`'s accounts.
fn scope_of(server: &Server, account: &str) -> (Scope, String) {
    let values: Vec<&Scoped<Template>> = match &server.launch {
        Launch::Stdio { args, env, .. } => args.iter().chain(env.values()).collect(),
        Launch::Http { headers, .. } => headers.values().collect(),
    };
    values
        .into_iter()
        .find_map(|value| {
            value
                .value
                .secrets()
                .find(|secret| value.scope.account(secret) == account)
                .map(|secret| (value.scope.clone(), secret.to_string()))
        })
        .unwrap_or((Scope::Me, account.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{Auth, Launch, Layer, Manifest, Problem, Scope, resolve};

    fn team(text: &str) -> Layer {
        Layer {
            scope: Scope::Team("phin-tech".to_string()),
            manifest: Manifest::from_toml(text).unwrap(),
        }
    }

    fn me(text: &str) -> Layer {
        Layer {
            scope: Scope::Me,
            manifest: Manifest::from_toml(text).unwrap(),
        }
    }

    const TEAM: &str = r#"
[secrets.github-token]
prompt = "A GitHub token"

[servers.github]
command = "npx"
args = ["-y", "server-github"]
env = { GITHUB_TOKEN = "secret:github-token", LOG = "info" }

[servers.sentry]
url = "https://mcp.sentry.dev/mcp"
auth = "oauth"
"#;

    #[test]
    fn a_team_secret_is_kept_under_the_team() {
        let resolved = resolve(&[team(TEAM)]);
        assert_eq!(resolved.problems, []);
        let github = &resolved.servers["github"];
        assert_eq!(github.accounts(), ["phin-tech/github-token"]);
        assert_eq!(
            resolved.secrets["phin-tech/github-token"].prompt,
            "A GitHub token"
        );
        assert!(matches!(
            resolved.servers["sentry"].launch,
            Launch::Http {
                auth: Auth::OAuth { .. },
                ..
            }
        ));
    }

    #[test]
    fn your_layer_changes_one_env_value_and_keeps_the_rest() {
        let resolved = resolve(&[
            team(TEAM),
            me("[servers.github]\nenv = { GITHUB_TOKEN = \"secret:my-github\" }\n"),
        ]);
        let Launch::Stdio { env, args, .. } = &resolved.servers["github"].launch else {
            panic!("github runs a command");
        };
        assert_eq!(env["GITHUB_TOKEN"].scope, Scope::Me);
        assert_eq!(env["LOG"].scope, Scope::Team("phin-tech".to_string()));
        assert_eq!(args.len(), 2);
        assert_eq!(resolved.servers["github"].accounts(), ["me/my-github"]);
        assert_eq!(resolved.secrets["me/my-github"].prompt, "my-github");
    }

    #[test]
    fn turning_a_team_server_off_leaves_it_out_of_the_default_profile() {
        let resolved = resolve(&[team(TEAM), me("[servers.sentry]\nenabled = false\n")]);
        assert!(!resolved.servers["sentry"].enabled);
        assert_eq!(resolved.profiles["default"], ["github"]);
    }

    #[test]
    fn a_secret_filled_in_renders_and_a_missing_one_is_named() {
        let resolved = resolve(&[team(TEAM)]);
        let Launch::Stdio { env, .. } = &resolved.servers["github"].launch else {
            panic!("github runs a command");
        };
        let token = &env["GITHUB_TOKEN"];
        assert_eq!(
            token.render(|account| (account == "phin-tech/github-token").then(|| "t0k".into())),
            Ok("t0k".to_string())
        );
        assert_eq!(
            token.render(|_| None).unwrap_err().0,
            "phin-tech/github-token"
        );
        let missing = resolved.missing_secrets(&resolved.servers["github"], |_| false);
        assert_eq!(missing[0].prompt, "A GitHub token");
    }

    #[test]
    fn broken_servers_are_reported_and_the_rest_still_run() {
        let resolved = resolve(&[team(
            r#"
[servers.both]
command = "x"
url = "https://x"

[servers.neither]

[servers.loose]
command = "x"
env = { T = "secret:undescribed" }

[servers.ok]
command = "x"

[profiles.small]
servers = ["ok", "gone"]
"#,
        )]);
        insta::assert_snapshot!(
            resolved.problems.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"),
            @r"
        both has both a command and a url; give it one
        loose uses the secret undescribed, which the manifest doesn't describe
        neither has neither a command nor a url
        the profile small names gone, which isn't a server
        "
        );
        assert_eq!(resolved.profiles["small"], ["ok"]);
        assert!(resolved.servers.contains_key("loose"));
        assert!(matches!(resolved.problems[0], Problem::CommandAndUrl(_)));
    }

    #[test]
    fn two_servers_cant_share_a_namespace() {
        let resolved = resolve(&[me(
            "[servers.a]\ncommand = \"x\"\nnamespace = \"gh\"\n[servers.b]\ncommand = \"y\"\nnamespace = \"gh\"\n",
        )]);
        assert_eq!(resolved.servers.len(), 1);
        assert!(matches!(
            resolved.problems[..],
            [Problem::SharedNamespace { .. }]
        ));
    }
}
