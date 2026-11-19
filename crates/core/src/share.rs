//! Handing servers to someone else: some of the person's own, written out as a manifest of their own with every
//! secret only named, and a manifest someone sent, added as a team's layer under the person's own.

use std::path::PathBuf;

use toml_edit::DocumentMut;

use crate::Home;
use crate::manifest::{Manifest, ManifestError, SecretSpec, edit, is_server_name};
use crate::paste::{self, Pasted, PastedSecret};

/// Why a pasted team's manifest can't be added.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TeamError {
    #[error("Paste a team's patchbay.toml, or open it")]
    Empty,
    #[error("That isn't a patchbay.toml Patchbay can read: {0}")]
    Broken(String),
    #[error("That manifest has no servers in it")]
    NoServers,
    #[error("Give the team a name")]
    NoName,
    #[error("“{0}” is a name Patchbay keeps for itself; give the team another")]
    Reserved(String),
    #[error("There's no team “{0}”")]
    NoTeam(String),
    #[error("{0}")]
    Files(String),
    #[error("Give the URL of the team's git repository")]
    NoUrl,
    #[error("That isn't a team's repository: {0}")]
    NotARepository(String),
    #[error("{0} has no patchbay.toml at its top")]
    NoManifest(String),
    #[error("That manifest has no servers or skills in it")]
    NothingShared,
    #[error("You're subscribed to that repository already, as “{0}”")]
    Subscribed(String),
    #[error("There's a team kept as “{0}” already; give this one another name")]
    Taken(String),
    #[error("{0} was updated while you were looking; check it again")]
    Stale(String),
    #[error(
        "{0} needs you signed in. Patchbay's git never asks, so clone it once in a terminal to save your sign-in, or use an SSH URL with a key ssh-agent holds"
    )]
    Denied(String),
    /// What git said, which already says what it couldn't fetch.
    #[error("{0}")]
    Fetch(String),
}

/// A team taken out, with what's needed to put it back as it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removed {
    pub folder: String,
    /// The team's manifest.
    pub team: String,
    /// The person's own manifest from before the team's servers' switches were taken out of it.
    pub mine: String,
    /// The repository it was kept from, if it was subscribed to rather than pasted.
    pub subscription: Option<crate::subscription::Subscription>,
}

/// A team's manifest, read from what was pasted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Team {
    /// The name its `[team]` gives, if it gives one.
    pub name: Option<String>,
    pub servers: Vec<Pasted>,
}

/// The servers `names` from the person's manifest `text`, as a manifest to give someone: each secret they name
/// described, a key written into a value taken out and named instead (and never written), and none turned off. With
/// `team`, it says whose it is, so it can be added as that team's.
pub fn export(text: &str, names: &[String], team: Option<&str>) -> Result<String, ManifestError> {
    let manifest = Manifest::from_toml(text)?;
    let mut servers = Vec::new();
    for name in names {
        let Some(spec) = manifest.servers.get(name) else {
            continue;
        };
        let mut spec = spec.clone();
        spec.enabled = None;
        let mut server = paste::with_keys_taken_out(name, spec);
        for secret in &mut server.secrets {
            secret.value = None;
        }
        for secret in paste::references(&server.spec) {
            if server.secrets.iter().any(|named| named.name == secret) {
                continue;
            }
            server.secrets.push(PastedSecret {
                spec: manifest
                    .secrets
                    .get(&secret)
                    .cloned()
                    .unwrap_or_else(|| SecretSpec {
                        prompt: Some(format!("{name}'s {secret}")),
                        help: None,
                    }),
                name: secret,
                value: None,
            });
        }
        servers.push(server);
    }
    let head = match team.map(str::trim).filter(|team| !team.is_empty()) {
        Some(team) => {
            let mut doc = DocumentMut::new();
            set_team(&mut doc, team);
            doc.to_string()
        }
        None => String::new(),
    };
    edit::add(&head, &servers)
}

/// The team manifest in `text`.
pub fn read(text: &str) -> Result<Team, TeamError> {
    if text.trim().is_empty() {
        return Err(TeamError::Empty);
    }
    let manifest =
        Manifest::from_toml(text).map_err(|error| TeamError::Broken(error.to_string()))?;
    if manifest.servers.is_empty() {
        return Err(TeamError::NoServers);
    }
    let servers = paste::from_toml(text).map_err(|error| TeamError::Broken(error.to_string()))?;
    Ok(Team {
        name: manifest.team.map(|team| team.name),
        servers,
    })
}

/// The folder a team named `name` is kept under, which also begins its secrets' Keychain accounts: `Phin Tech` is
/// `phin-tech`. Never one the person's own keys or sign-ins are kept under.
pub fn folder(name: &str) -> Result<String, TeamError> {
    let mut folder = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            folder.push(c.to_ascii_lowercase());
        } else if !folder.is_empty() && !folder.ends_with('-') {
            folder.push('-');
        }
    }
    let folder = folder.trim_end_matches('-').to_string();
    if folder.is_empty() {
        return Err(TeamError::NoName);
    }
    if ["me", "oauth"].contains(&folder.as_str()) || !is_server_name(&folder) {
        return Err(TeamError::Reserved(folder));
    }
    Ok(folder)
}

/// Where the team kept under `folder` has its manifest.
pub fn manifest_path(home: &Home, folder: &str) -> PathBuf {
    home.teams().join(folder).join("patchbay.toml")
}

/// Takes the team kept under `folder` out: its folder, and the switches in the person's own manifest that turned its
/// servers off. Its keys stay in the Keychain, so adding it again doesn't ask for them.
pub fn remove(home: &Home, folder: &str) -> Result<Removed, TeamError> {
    // Only a folder name Patchbay itself would give, so nothing outside `teams/` is ever reached.
    if self::folder(folder).as_deref() != Ok(folder) {
        return Err(TeamError::NoTeam(folder.to_string()));
    }
    let path = manifest_path(home, folder);
    let team = std::fs::read_to_string(&path).map_err(|_| TeamError::NoTeam(folder.to_string()))?;
    let names: Vec<String> = Manifest::from_toml(&team)
        .map(|manifest| manifest.servers.into_keys().collect())
        .unwrap_or_default();
    let mine = std::fs::read_to_string(home.manifest()).unwrap_or_default();
    let forgotten = edit::forget_switches(&mine, &names)
        .map_err(|error| TeamError::Files(error.to_string()))?;
    if forgotten != mine {
        crate::write_atomically(&home.manifest(), forgotten.as_bytes())
            .map_err(|error| TeamError::Files(error.to_string()))?;
    }
    let mut subscriptions = crate::subscription::Subscriptions::read(home)?;
    let subscription = subscriptions.teams.remove(folder);
    if subscription.is_some() {
        subscriptions.write(home)?;
    }
    std::fs::remove_dir_all(home.teams().join(folder))
        .map_err(|error| TeamError::Files(error.to_string()))?;
    Ok(Removed {
        folder: folder.to_string(),
        team,
        mine,
        subscription,
    })
}

/// Puts a team taken out back as it was.
pub fn restore(home: &Home, removed: &Removed) -> Result<(), TeamError> {
    let path = manifest_path(home, &removed.folder);
    let files = |error: std::io::Error| TeamError::Files(error.to_string());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(files)?;
    }
    crate::write_atomically(&path, removed.team.as_bytes()).map_err(files)?;
    crate::write_atomically(&home.manifest(), removed.mine.as_bytes()).map_err(files)?;
    if let Some(subscription) = &removed.subscription {
        let mut subscriptions = crate::subscription::Subscriptions::read(home)?;
        subscriptions
            .teams
            .insert(removed.folder.clone(), subscription.clone());
        subscriptions.write(home)?;
    }
    Ok(())
}

/// `text` as the team `name`'s manifest, saying so in its `[team]`, and kept as it was otherwise (comments,
/// profiles and skills too).
pub fn named(text: &str, name: &str) -> Result<String, TeamError> {
    let mut doc = text
        .parse::<DocumentMut>()
        .map_err(|error| TeamError::Broken(error.to_string()))?;
    set_team(&mut doc, name.trim());
    Ok(doc.to_string())
}

/// `doc`'s `[team]` naming `name`, as a table of its own like the rest of a manifest.
fn set_team(doc: &mut DocumentMut, name: &str) {
    let team = doc
        .entry("team")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));
    if !team.is_table() {
        *team = toml_edit::Item::Table(toml_edit::Table::new());
    }
    team["name"] = toml_edit::value(name);
}

#[cfg(test)]
mod tests {
    use super::{TeamError, export, folder, manifest_path, named, read, remove, restore};
    use crate::Home;
    use crate::manifest::{Layer, Manifest, Scope, resolve};

    const MINE: &str = r#"# Mine.

[secrets.github-token]
prompt = "A GitHub personal access token"
help = "https://github.com/settings/personal-access-tokens/new"

[servers.github]
url = "https://api.githubcopilot.com/mcp/"
headers = { Authorization = "Bearer ${secret:github-token}" }

[servers.brave]
command = "npx"
args = ["-y", "brave-mcp"]
env = { BRAVE_API_KEY = "sk-written-in-by-hand", REGION = "eu", OTHER = "secret:undescribed" }
enabled = false

[servers.linear]
url = "https://mcp.linear.app/mcp"
auth = "oauth"
"#;

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn an_export_has_only_the_servers_chosen_with_their_secrets_described() {
        let text = export(MINE, &names(&["github", "linear"]), Some("Phin Tech")).unwrap();
        insta::assert_snapshot!(text, @r#"
        [team]
        name = "Phin Tech"

        [secrets.github-token]
        prompt = "A GitHub personal access token"
        help = "https://github.com/settings/personal-access-tokens/new"

        [servers.github]
        url = "https://api.githubcopilot.com/mcp/"
        headers = { Authorization = "Bearer ${secret:github-token}" }

        [servers.linear]
        url = "https://mcp.linear.app/mcp"
        auth = "oauth"
        "#);
    }

    #[test]
    fn a_key_written_into_your_manifest_is_never_exported() {
        let text = export(MINE, &names(&["brave"]), None).unwrap();
        assert!(!text.contains("sk-written-in-by-hand"), "{text}");
        assert!(
            text.contains("BRAVE_API_KEY = \"secret:brave-api-key\""),
            "{text}"
        );
        assert!(
            !text.contains("enabled"),
            "what they're given starts on: {text}"
        );
        assert!(text.contains("[secrets.undescribed]"), "{text}");
    }

    #[test]
    fn an_export_reads_back_as_a_team_with_nothing_wrong() {
        let text = export(
            MINE,
            &names(&["github", "brave", "linear"]),
            Some("Phin Tech"),
        )
        .unwrap();
        let team = read(&text).unwrap();
        assert_eq!(team.name.as_deref(), Some("Phin Tech"));
        assert_eq!(team.servers.len(), 3);
        let resolved = resolve(&[Layer {
            scope: Scope::Team("phin-tech".to_string()),
            manifest: Manifest::from_toml(&text).unwrap(),
        }]);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    }

    #[test]
    fn a_teams_folder_is_its_name_made_safe_and_never_one_of_yours() {
        assert_eq!(folder("Phin Tech").unwrap(), "phin-tech");
        assert_eq!(folder("../../etc").unwrap(), "etc");
        assert_eq!(folder("  "), Err(TeamError::NoName));
        assert_eq!(folder("Me"), Err(TeamError::Reserved("me".to_string())));
        assert_eq!(
            folder("OAuth"),
            Err(TeamError::Reserved("oauth".to_string()))
        );
    }

    #[test]
    fn only_a_manifest_with_servers_is_a_team() {
        assert_eq!(read(""), Err(TeamError::Empty));
        assert_eq!(read("[team]\nname = \"x\"\n"), Err(TeamError::NoServers));
        assert!(matches!(read("{\"x\": 1}"), Err(TeamError::Broken(_))));
    }

    #[test]
    fn a_team_is_kept_with_its_comments_and_the_name_it_was_given() {
        let text = named("# Ours.\n[servers.fetch]\ncommand = \"uvx\"\n", "Phin Tech").unwrap();
        assert!(text.starts_with("# Ours."), "{text}");
        assert_eq!(
            Manifest::from_toml(&text).unwrap().team.unwrap().name,
            "Phin Tech"
        );
    }

    #[test]
    fn a_removed_team_takes_its_switches_with_it_and_undo_puts_both_back() {
        let root =
            std::env::temp_dir().join(format!("patchbay-remove-team-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = Home::new(&root);
        let team = "[team]\nname = \"Phin Tech\"\n\n[servers.sentry]\nurl = \"https://mcp.sentry.dev/mcp\"\n\n[servers.fetch]\nurl = \"https://f.example/mcp\"\n";
        let mine = "# Mine.\n[servers.sentry]\nenabled = false\n\n[servers.fetch]\nenabled = false\nnamespace = \"f\"\n";
        std::fs::create_dir_all(home.teams().join("phin-tech")).unwrap();
        std::fs::write(manifest_path(&home, "phin-tech"), team).unwrap();
        std::fs::write(home.manifest(), mine).unwrap();

        let removed = remove(&home, "phin-tech").unwrap();
        assert!(!home.teams().join("phin-tech").exists());
        let left = std::fs::read_to_string(home.manifest()).unwrap();
        assert!(
            !left.contains("sentry"),
            "only a switch, so it goes: {left}"
        );
        assert!(
            left.contains("namespace = \"f\""),
            "more than a switch, so it stays: {left}"
        );

        restore(&home, &removed).unwrap();
        assert_eq!(
            std::fs::read_to_string(manifest_path(&home, "phin-tech")).unwrap(),
            team
        );
        assert_eq!(std::fs::read_to_string(home.manifest()).unwrap(), mine);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn only_a_team_folder_patchbay_named_can_be_removed() {
        let home = Home::new(std::env::temp_dir().join("patchbay-remove-nothing"));
        assert!(matches!(remove(&home, "../.."), Err(TeamError::NoTeam(_))));
        assert!(matches!(remove(&home, "me"), Err(TeamError::NoTeam(_))));
        assert!(matches!(remove(&home, "nobody"), Err(TeamError::NoTeam(_))));
    }
}
