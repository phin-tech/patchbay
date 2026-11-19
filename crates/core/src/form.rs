//! A server as the edit form shows it, field by field, and the server it saves. The form shows what most servers
//! need; everything else a server has (its tools, namespace, transport, OAuth client) is kept as it was.

use std::collections::BTreeMap;

use crate::manifest::{AuthKind, ServerSpec, Template};
use crate::paste::{self, Pasted};

/// How the server is reached, with the fields for it as typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    Command {
        command: String,
        /// One argument a line, so an argument with spaces in it is written as it is.
        args: String,
        cwd: String,
    },
    Url {
        url: String,
        sign_in: bool,
    },
}

/// A server's fields as the form holds them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerFields {
    pub reach: Reach,
    /// A command's environment, or a URL's headers, a name and a value a row.
    pub pairs: Vec<(String, String)>,
}

/// Why the form doesn't make a server yet, said so the person can fix it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FieldsError {
    #[error("Give the command to run")]
    NoCommand,
    #[error("Give a URL starting with https:// or http://")]
    NotUrl,
    #[error("Give a name for the value “{0}”")]
    NoName(String),
    #[error("“{0}” is given twice")]
    Twice(String),
    #[error("{0}")]
    Template(String),
}

impl ServerFields {
    /// The fields showing `spec`.
    pub fn of(spec: &ServerSpec) -> ServerFields {
        let (reach, pairs) = match &spec.url {
            Some(url) => (
                Reach::Url {
                    url: url.clone(),
                    sign_in: spec.auth == Some(AuthKind::Oauth),
                },
                spec.headers.as_ref(),
            ),
            None => (
                Reach::Command {
                    command: spec.command.clone().unwrap_or_default(),
                    args: spec
                        .args
                        .iter()
                        .flatten()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("\n"),
                    cwd: spec.cwd.clone().unwrap_or_default(),
                },
                spec.env.as_ref(),
            ),
        };
        let pairs = pairs
            .into_iter()
            .flatten()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        ServerFields { reach, pairs }
    }

    /// The server `name` these fields make, from `original` with what the form shows replaced. A key typed into a
    /// value is taken out to keep in the Keychain, as a pasted one is, and the server names it instead.
    pub fn server(&self, name: &str, original: &ServerSpec) -> Result<Pasted, FieldsError> {
        let mut spec = original.clone();
        let pairs = self.pairs()?;
        let pairs = (!pairs.is_empty()).then_some(pairs);
        match &self.reach {
            Reach::Command { command, args, cwd } => {
                let command = command.trim();
                if command.is_empty() {
                    return Err(FieldsError::NoCommand);
                }
                let args: Vec<String> = args
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .map(str::to_string)
                    .collect();
                spec.command = Some(command.to_string());
                spec.args = (!args.is_empty()).then_some(args);
                spec.cwd = Some(cwd.trim().to_string()).filter(|cwd| !cwd.is_empty());
                spec.env = pairs;
            }
            Reach::Url { url, sign_in } => {
                let url = url.trim();
                if !(url.starts_with("https://") || url.starts_with("http://"))
                    || url.contains(char::is_whitespace)
                {
                    return Err(FieldsError::NotUrl);
                }
                spec.url = Some(url.to_string());
                spec.headers = pairs;
                spec.auth = match (sign_in, original.auth) {
                    (true, _) => Some(AuthKind::Oauth),
                    // Left unset, a server without sign-in reads the same, and the manifest says less.
                    (false, Some(AuthKind::Oauth)) => None,
                    (false, kept) => kept,
                };
            }
        }
        Ok(paste::with_keys_taken_out(name, spec))
    }

    /// The rows given, without the empty ones, each checked.
    fn pairs(&self) -> Result<BTreeMap<String, String>, FieldsError> {
        let mut pairs = BTreeMap::new();
        for (key, value) in &self.pairs {
            let key = key.trim();
            if key.is_empty() && value.trim().is_empty() {
                continue;
            }
            if key.is_empty() {
                return Err(FieldsError::NoName(value.clone()));
            }
            Template::parse(value).map_err(|error| FieldsError::Template(error.to_string()))?;
            if pairs.insert(key.to_string(), value.clone()).is_some() {
                return Err(FieldsError::Twice(key.to_string()));
            }
        }
        Ok(pairs)
    }
}

#[cfg(test)]
mod tests {
    use super::{FieldsError, Reach, ServerFields};
    use crate::manifest::{AuthKind, Manifest, ServerSpec};

    const SERVERS: &str = r#"
[servers.brave]
command = "npx"
args = ["-y", "brave-mcp", "--name", "a name with spaces"]
cwd = "~/src"
env = { BRAVE_API_KEY = "secret:brave-api-key", REGION = "eu" }

[servers.github]
url = "https://api.githubcopilot.com/mcp/"
headers = { Authorization = "Bearer ${secret:github-token}" }

[servers.linear]
url = "https://mcp.linear.app/mcp"
auth = "oauth"
oauth = { client_id = "patchbay", scopes = ["read"] }
namespace = "lin"
tools = { deny = ["delete_*"] }

[servers.bare]
url = "https://example.com/mcp"
"#;

    fn spec(name: &str) -> ServerSpec {
        Manifest::from_toml(SERVERS).unwrap().servers[name].clone()
    }

    #[test]
    fn a_server_saved_from_the_form_unchanged_is_the_same_server() {
        for name in ["brave", "github", "linear", "bare"] {
            let original = spec(name);
            let saved = ServerFields::of(&original).server(name, &original).unwrap();
            assert_eq!(saved.spec, original, "{name}");
            assert!(saved.secrets.is_empty(), "{name}: {:?}", saved.secrets);
        }
    }

    #[test]
    fn a_key_typed_into_a_header_goes_to_the_keychain_and_the_server_names_it() {
        let original = spec("bare");
        let mut fields = ServerFields::of(&original);
        fields
            .pairs
            .push(("Authorization".into(), "Bearer abc123".into()));
        let saved = fields.server("bare", &original).unwrap();
        assert_eq!(
            saved.spec.headers.unwrap()["Authorization"],
            "Bearer ${secret:bare-token}"
        );
        assert_eq!(saved.secrets[0].value.as_deref(), Some("abc123"));
        assert_eq!(
            saved.spec.auth, None,
            "a server with a key isn't made to sign in"
        );
    }

    #[test]
    fn rows_taken_out_of_the_form_are_taken_out_of_the_server() {
        let original = spec("brave");
        let mut fields = ServerFields::of(&original);
        fields.pairs.clear();
        let Reach::Command { args, cwd, .. } = &mut fields.reach else {
            unreachable!()
        };
        args.clear();
        cwd.clear();
        let saved = fields.server("brave", &original).unwrap().spec;
        assert_eq!((saved.env, saved.args, saved.cwd), (None, None, None));
    }

    #[test]
    fn turning_sign_in_off_leaves_the_rest_of_the_server_as_it_was() {
        let original = spec("linear");
        let mut fields = ServerFields::of(&original);
        fields.reach = Reach::Url {
            url: "https://mcp.linear.app/mcp".into(),
            sign_in: false,
        };
        let saved = fields.server("linear", &original).unwrap().spec;
        assert_eq!(saved.auth, None);
        assert_eq!(saved.namespace.as_deref(), Some("lin"));
        assert!(saved.tools.is_some() && saved.oauth.is_some());
        fields.reach = Reach::Url {
            url: "https://mcp.linear.app/mcp".into(),
            sign_in: true,
        };
        assert_eq!(
            fields.server("linear", &original).unwrap().spec.auth,
            Some(AuthKind::Oauth)
        );
    }

    #[test]
    fn the_form_says_what_stops_it_making_a_server() {
        let original = spec("brave");
        let mut fields = ServerFields::of(&original);
        fields.pairs.push(("REGION".into(), "us".into()));
        assert_eq!(
            fields.server("brave", &original).unwrap_err(),
            FieldsError::Twice("REGION".into())
        );
        fields.pairs.pop();
        fields.pairs.push((String::new(), "us".into()));
        assert_eq!(
            fields.server("brave", &original).unwrap_err(),
            FieldsError::NoName("us".into())
        );
        fields.pairs.pop();
        fields.pairs.push((String::new(), String::new()));
        assert!(
            fields.server("brave", &original).is_ok(),
            "an empty row is left out"
        );
        fields.reach = Reach::Url {
            url: "example.com".into(),
            sign_in: false,
        };
        assert_eq!(
            fields.server("brave", &original).unwrap_err(),
            FieldsError::NotUrl
        );
    }
}
