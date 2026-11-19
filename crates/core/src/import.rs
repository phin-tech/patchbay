//! Bringing in what clients already have: every server a client keeps of its own, for the person and in each Claude
//! Code project, gathered by name so the person can choose, one by one, what Patchbay takes over and where from.
//! Nothing here changes a file except [`take_out`], and only for what it's handed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::clients::{Client, ClientError};

/// Where a client keeps a server.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kept {
    /// For the person, wherever they work: what Take Over has always moved.
    User,
    /// For one project only, private to this Mac: Claude Code's local servers, kept in `~/.claude.json` by the
    /// project's folder.
    InProject(PathBuf),
    /// In a project's own `.mcp.json`, a file in its repository that everyone who clones it shares.
    ProjectFile(PathBuf),
}

/// A client, and where it keeps a server.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Origin {
    pub client: Client,
    pub kept: Kept,
}

/// One way a server is set up, and every place it's set up that way.
#[derive(Debug, Clone, PartialEq)]
pub struct Version {
    /// As the client has it, which Add Server reads as pasted. It may hold keys: show it with [`Summary`] only.
    pub server: Value,
    pub origins: Vec<Origin>,
}

/// A server some client has, by name, in each way it's set up.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub name: String,
    /// One for each different setup; only identical ones are put together.
    pub versions: Vec<Version>,
    /// The server Patchbay already has that this is: one of the same name, or one at the same URL or command.
    pub patchbay_has: Option<String>,
}

/// What [`gather`] found, and the files it couldn't read, so a broken one is said rather than looking empty.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Gathered {
    pub found: Vec<Found>,
    pub unreadable: Vec<String>,
}

/// A server of Patchbay's own, as [`gather`] compares what it finds with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Known {
    pub name: String,
    /// Its URL, or its command and arguments.
    pub target: String,
}

/// Every server the clients under `home` keep of their own, by name, with `known`, Patchbay's own, marked where one
/// is the same.
pub fn gather(home: &Path, known: &[Known]) -> Gathered {
    let mut gathered = Gathered::default();
    let mut copies: Vec<(String, Value, Origin)> = Vec::new();
    for client in Client::ALL {
        for (name, server) in client.own_servers(home) {
            copies.push((
                name,
                server,
                Origin {
                    client,
                    kept: Kept::User,
                },
            ));
        }
    }
    let claude = Client::ClaudeCode.config_path(home);
    if let Ok(text) = std::fs::read_to_string(&claude) {
        match serde_json::from_str::<Value>(&text) {
            Ok(config) => {
                let projects = config
                    .get("projects")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                for (folder, project) in projects {
                    let folder = PathBuf::from(folder);
                    for (name, server) in servers_in(&project, "mcpServers") {
                        copies.push((
                            name,
                            server,
                            Origin {
                                client: Client::ClaudeCode,
                                kept: Kept::InProject(folder.clone()),
                            },
                        ));
                    }
                    let shared = folder.join(".mcp.json");
                    let Ok(text) = std::fs::read_to_string(&shared) else {
                        continue;
                    };
                    match serde_json::from_str::<Value>(&text) {
                        Ok(file) => {
                            for (name, server) in servers_in(&file, "mcpServers") {
                                copies.push((
                                    name,
                                    server,
                                    Origin {
                                        client: Client::ClaudeCode,
                                        kept: Kept::ProjectFile(folder.clone()),
                                    },
                                ));
                            }
                        }
                        Err(error) => gathered
                            .unreadable
                            .push(format!("{}: {error}", shared.display())),
                    }
                }
            }
            Err(error) => gathered
                .unreadable
                .push(format!("{}: {error}", claude.display())),
        }
    }
    let mut by_name: BTreeMap<String, Vec<Version>> = BTreeMap::new();
    for (name, server, origin) in copies {
        if crate::clients::is_ours(&name) {
            continue;
        }
        let versions = by_name.entry(name).or_default();
        let same = normal(&server);
        match versions
            .iter_mut()
            .find(|version| normal(&version.server) == same)
        {
            Some(version) => version.origins.push(origin),
            None => versions.push(Version {
                server,
                origins: vec![origin],
            }),
        }
    }
    gathered.found = by_name
        .into_iter()
        .map(|(name, versions)| {
            let patchbay_has = known
                .iter()
                .find(|known| {
                    known.name == name
                        || versions
                            .iter()
                            .any(|version| Summary::of(&version.server).target == known.target)
                })
                .map(|known| known.name.clone());
            Found {
                name,
                versions,
                patchbay_has,
            }
        })
        .collect();
    gathered
}

/// The servers under `key` in a JSON object, by name.
fn servers_in(value: &Value, key: &str) -> Vec<(String, Value)> {
    value
        .get(key)
        .and_then(Value::as_object)
        .map(|servers| {
            servers
                .iter()
                .map(|(name, server)| (name.clone(), server.clone()))
                .collect()
        })
        .unwrap_or_default()
}

/// A server as it's compared: without the `type` that says only what its other fields already do (`stdio` for a
/// command, `http` for a URL), which one client writes and another leaves out. `sse` stays, since it changes how
/// it's reached.
fn normal(server: &Value) -> Value {
    let mut server = server.clone();
    if let Some(object) = server.as_object_mut()
        && object
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind != "sse")
    {
        object.remove("type");
    }
    server
}

/// What a server is, safe to show: where it's reached, and the names (never the values) of the keys it's given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// Its URL, or its command and arguments, with any argument that looks like a key hidden.
    pub target: String,
    /// The names of its environment variables and headers.
    pub keys: Vec<String>,
}

impl Summary {
    pub fn of(server: &Value) -> Summary {
        let field = |key: &str| server.get(key).and_then(Value::as_str);
        let target = match field("url") {
            Some(url) => url.to_string(),
            None => {
                let args: Vec<&str> = server
                    .get("args")
                    .and_then(Value::as_array)
                    .map(|args| args.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default();
                let mut shown = vec![field("command").unwrap_or_default().to_string()];
                let mut after_flag = false;
                for arg in args {
                    shown.push(if after_flag || looks_secret(arg) {
                        "•••".to_string()
                    } else {
                        arg.to_string()
                    });
                    after_flag = arg.starts_with('-') && names_a_secret(arg) && !arg.contains('=');
                }
                shown.join(" ")
            }
        };
        let mut keys: Vec<String> = ["env", "headers"]
            .into_iter()
            .filter_map(|key| server.get(key).and_then(Value::as_object))
            .flat_map(Map::keys)
            .cloned()
            .collect();
        keys.sort();
        Summary { target, keys }
    }
}

/// Whether a name says it's for a key: `--api-key`, `GITHUB_TOKEN`.
fn names_a_secret(name: &str) -> bool {
    let name = name.to_lowercase();
    ["token", "key", "secret", "password", "auth"]
        .iter()
        .any(|word| name.contains(word))
}

/// Whether an argument is, or holds, a key: `--token=abc`, or a long run of letters and digits that names nothing.
fn looks_secret(arg: &str) -> bool {
    if let Some((name, _)) = arg.split_once('=') {
        return names_a_secret(name);
    }
    arg.len() >= 24
        && !arg.contains(['/', '.', '@', ':'])
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && arg.chars().any(|c| c.is_ascii_digit())
}

/// Takes the servers `taken`, by name and where each is kept, out of the clients under `home`. Each file is backed
/// up once, as it was before any of them, beside it as `<file>.patchbay-take-over-backup`; a project's `.mcp.json`
/// is backed up into `backups` instead, so nothing is left in its repository. Gives what couldn't be taken out, and
/// why, leaving the rest done.
pub fn take_out(
    home: &Path,
    backups: &Path,
    taken: &[(Origin, String)],
) -> Vec<(Origin, ClientError)> {
    let mut by_origin: BTreeMap<Origin, Vec<String>> = BTreeMap::new();
    for (origin, name) in taken {
        by_origin
            .entry(origin.clone())
            .or_default()
            .push(name.clone());
    }
    let mut failed = Vec::new();
    // Every file is backed up before any change to it, so a second change never backs up the first.
    let claude = Client::ClaudeCode.config_path(home);
    let touches_claude = by_origin.keys().any(|origin| {
        origin.client == Client::ClaudeCode && !matches!(origin.kept, Kept::ProjectFile(_))
    });
    if touches_claude && let Err(error) = crate::clients::back_up_once(&claude, None) {
        return by_origin
            .into_keys()
            .map(|origin| (origin, clone_error(&error)))
            .collect();
    }
    for (origin, names) in by_origin {
        let done = match &origin.kept {
            // Claude Code's file was backed up above, since its projects' servers are in it too; any other client's is
            // backed up as its servers are taken out.
            Kept::User if origin.client == Client::ClaudeCode => {
                origin.client.remove_own_backed_up(home, &names)
            }
            Kept::User => origin.client.remove_own(home, &names).map(|_| ()),
            Kept::InProject(folder) => remove_in_project(home, folder, &names),
            Kept::ProjectFile(folder) => remove_from_project_file(folder, backups, &names),
        };
        if let Err(error) = done {
            failed.push((origin, error));
        }
    }
    failed
}

fn clone_error(error: &ClientError) -> ClientError {
    ClientError::Write {
        path: String::new(),
        error: std::io::Error::other(error.to_string()),
    }
}

/// Takes `names` out of a Claude Code project's local servers. Claude Code is asked to, from inside the project, when
/// it's the person's own home and the folder is still there; otherwise `~/.claude.json` is edited directly.
fn remove_in_project(home: &Path, folder: &Path, names: &[String]) -> Result<(), ClientError> {
    use std::process::{Command, Stdio};

    let real = std::env::var_os("HOME").is_some_and(|own| Path::new(&own) == home);
    if real && folder.is_dir() {
        let all = names.iter().all(|name| {
            Command::new("claude")
                .args(["mcp", "remove", name.as_str(), "-s", "local"])
                .current_dir(folder)
                .env("PATH", crate::path::login_path())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        });
        if all {
            return Ok(());
        }
    }
    let path = Client::ClaudeCode.config_path(home);
    let shown = path.display().to_string();
    let text = std::fs::read_to_string(&path).map_err(|error| ClientError::Write {
        path: shown.clone(),
        error,
    })?;
    let mut config: Value = serde_json::from_str(&text).map_err(|error| ClientError::NotJson {
        path: shown.clone(),
        error: error.to_string(),
    })?;
    let key = folder.display().to_string();
    if let Some(servers) = config
        .get_mut("projects")
        .and_then(|projects| projects.get_mut(&key))
        .and_then(|project| project.get_mut("mcpServers"))
        .and_then(Value::as_object_mut)
    {
        servers.retain(|name, _| !names.contains(name));
    }
    let changed = serde_json::to_string_pretty(&config).expect("JSON values always print");
    crate::clients::write_keeping_permissions(&path, &changed)
}

/// Takes `names` out of a project's `.mcp.json`, keeping a copy of it as it was in `backups`.
fn remove_from_project_file(
    folder: &Path,
    backups: &Path,
    names: &[String],
) -> Result<(), ClientError> {
    let path = folder.join(".mcp.json");
    let shown = path.display().to_string();
    let write = |error| ClientError::Write {
        path: shown.clone(),
        error,
    };
    let text = std::fs::read_to_string(&path).map_err(write)?;
    let mut file: Value = serde_json::from_str(&text).map_err(|error| ClientError::NotJson {
        path: shown.clone(),
        error: error.to_string(),
    })?;
    let backup = backups.join(backup_name(folder));
    crate::clients::back_up_once(&path, Some(&backup))?;
    if let Some(servers) = file.get_mut("mcpServers").and_then(Value::as_object_mut) {
        servers.retain(|name, _| !names.contains(name));
    }
    let changed = serde_json::to_string_pretty(&file).expect("JSON values always print") + "\n";
    crate::clients::write_keeping_permissions(&path, &changed)
}

/// A project file's backup's name, from its folder's path: `Users-sam-src-app.mcp.json`.
fn backup_name(folder: &Path) -> String {
    let path: String = folder
        .display()
        .to_string()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!("{}.mcp.json", path.trim_matches('-'))
}

#[cfg(test)]
mod tests {
    use super::{Kept, Known, Origin, Summary, gather, take_out};
    use crate::clients::Client;
    use std::path::{Path, PathBuf};

    fn home(name: &str) -> PathBuf {
        let home =
            std::env::temp_dir().join(format!("patchbay-import-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        home
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// Claude Code with a server of the person's own and two projects with servers, one of them with a `.mcp.json`;
    /// Cursor with the same server set up differently.
    fn seeded(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let home = home(name);
        let (one, two) = (home.join("src/one"), home.join("src/two"));
        std::fs::create_dir_all(&one).unwrap();
        std::fs::create_dir_all(&two).unwrap();
        let claude = serde_json::json!({
            "mcpServers": {"linear": {"type": "http", "url": "https://mcp.linear.app/mcp"}},
            "projects": {
                one.display().to_string(): {"mcpServers": {"drawsimple": {"type": "stdio", "command": "drawsimple", "args": [], "env": {"DRAW_TOKEN": "s3cr3t"}}}},
                two.display().to_string(): {"mcpServers": {"drawsimple": {"command": "drawsimple", "args": [], "env": {"DRAW_TOKEN": "s3cr3t"}}}},
                home.join("src/gone").display().to_string(): {"mcpServers": {}},
            }
        });
        write(&home.join(".claude.json"), &claude.to_string());
        write(
            &two.join(".mcp.json"),
            r#"{"mcpServers": {"retro": {"command": "npx", "args": ["-y", "retro-mcp", "--api-key", "abc123"]}}}"#,
        );
        write(
            &home.join(".cursor/mcp.json"),
            r#"{"mcpServers": {"linear": {"url": "https://mcp.linear.app/sse", "type": "sse"}}}"#,
        );
        (home, one, two)
    }

    #[test]
    fn a_server_set_up_the_same_in_two_projects_is_one_version_kept_in_both() {
        let (home, one, two) = seeded("same");
        let gathered = gather(&home, &[]);
        let draw = gathered
            .found
            .iter()
            .find(|found| found.name == "drawsimple")
            .unwrap();
        assert_eq!(draw.versions.len(), 1);
        assert_eq!(
            draw.versions[0].origins,
            [
                Origin {
                    client: Client::ClaudeCode,
                    kept: Kept::InProject(one)
                },
                Origin {
                    client: Client::ClaudeCode,
                    kept: Kept::InProject(two)
                },
            ]
        );
    }

    #[test]
    fn a_server_set_up_differently_by_two_clients_is_two_versions() {
        let (home, _, _) = seeded("differs");
        let gathered = gather(&home, &[]);
        let linear = gathered
            .found
            .iter()
            .find(|found| found.name == "linear")
            .unwrap();
        assert_eq!(linear.versions.len(), 2);
        let clients: Vec<Client> = linear
            .versions
            .iter()
            .map(|version| version.origins[0].client)
            .collect();
        assert_eq!(clients, [Client::ClaudeCode, Client::Cursor]);
    }

    #[test]
    fn a_projects_mcp_json_is_found_as_shared_with_the_project() {
        let (home, _, two) = seeded("shared");
        let gathered = gather(&home, &[]);
        let retro = gathered
            .found
            .iter()
            .find(|found| found.name == "retro")
            .unwrap();
        assert_eq!(
            retro.versions[0].origins,
            [Origin {
                client: Client::ClaudeCode,
                kept: Kept::ProjectFile(two)
            }]
        );
    }

    #[test]
    fn a_server_patchbay_already_has_by_name_or_by_url_is_marked() {
        let (home, _, _) = seeded("known");
        let known = [Known {
            name: "linear-app".into(),
            target: "https://mcp.linear.app/mcp".into(),
        }];
        let gathered = gather(&home, &known);
        let linear = gathered
            .found
            .iter()
            .find(|found| found.name == "linear")
            .unwrap();
        assert_eq!(linear.patchbay_has.as_deref(), Some("linear-app"));
        let retro = gathered
            .found
            .iter()
            .find(|found| found.name == "retro")
            .unwrap();
        assert_eq!(retro.patchbay_has, None);
    }

    #[test]
    fn a_file_that_cant_be_read_is_said_and_the_rest_still_listed() {
        let (home, _, two) = seeded("broken");
        write(&two.join(".mcp.json"), "{ not json");
        let gathered = gather(&home, &[]);
        assert_eq!(gathered.unreadable.len(), 1, "{:?}", gathered.unreadable);
        assert!(
            gathered
                .found
                .iter()
                .any(|found| found.name == "drawsimple")
        );
    }

    #[test]
    fn a_summary_shows_where_a_server_is_and_its_keys_names_but_never_their_values() {
        let server = serde_json::json!({
            "command": "npx",
            "args": ["-y", "retro-mcp", "--api-key", "abc123", "--token=xyz", "ghp_abcdefghijklmnopqrstuvwxyz0123"],
            "env": {"RETRO_TOKEN": "hidden"},
        });
        let summary = Summary::of(&server);
        assert_eq!(summary.target, "npx -y retro-mcp --api-key ••• ••• •••");
        assert_eq!(summary.keys, ["RETRO_TOKEN"]);
    }

    #[test]
    fn taking_servers_out_backs_each_file_up_once_as_it_was_first() {
        let (home, one, two) = seeded("take-out");
        let backups = home.join("backups");
        let before = std::fs::read_to_string(home.join(".claude.json")).unwrap();
        let failed = take_out(
            &home,
            &backups,
            &[
                (
                    Origin {
                        client: Client::ClaudeCode,
                        kept: Kept::User,
                    },
                    "linear".into(),
                ),
                (
                    Origin {
                        client: Client::ClaudeCode,
                        kept: Kept::InProject(one.clone()),
                    },
                    "drawsimple".into(),
                ),
                (
                    Origin {
                        client: Client::ClaudeCode,
                        kept: Kept::ProjectFile(two.clone()),
                    },
                    "retro".into(),
                ),
            ],
        );
        assert!(failed.is_empty(), "{failed:?}");
        let gathered = gather(&home, &[]);
        let names: Vec<(&str, usize)> = gathered
            .found
            .iter()
            .map(|found| {
                (
                    found.name.as_str(),
                    found.versions.iter().map(|v| v.origins.len()).sum(),
                )
            })
            .collect();
        // drawsimple stays in the second project; linear in Cursor; retro is gone.
        assert_eq!(names, [("drawsimple", 1), ("linear", 1)]);
        assert_eq!(
            std::fs::read_to_string(home.join(".claude.json.patchbay-take-over-backup")).unwrap(),
            before
        );
        let saved = std::fs::read_dir(&backups).unwrap().count();
        assert_eq!(saved, 1);
        assert!(!two.join(".mcp.json.patchbay-take-over-backup").exists());
    }
}
