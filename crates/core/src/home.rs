//! Where Patchbay keeps its own files, which the app and the command line share.

use std::path::{Path, PathBuf};

use crate::manifest::{Layer, Manifest, Scope};

/// Patchbay's folder: `~/Library/Application Support/tech.phin.patchbay`, or `PATCHBAY_HOME` if it's set, so tests
/// and trials never touch the real one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Home {
    root: PathBuf,
}

/// The port the gateway listens on unless the settings say otherwise.
pub const DEFAULT_PORT: u16 = 7357;

/// The port to listen on: `PATCHBAY_PORT`, for a trial beside the app, or [`DEFAULT_PORT`].
pub fn port() -> u16 {
    std::env::var("PATCHBAY_PORT")
        .ok()
        .and_then(|port| port.parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

impl Home {
    pub fn new(root: impl Into<PathBuf>) -> Home {
        Home { root: root.into() }
    }

    /// `PATCHBAY_HOME`, or the folder in Application Support.
    pub fn from_env() -> Home {
        if let Some(root) = std::env::var_os("PATCHBAY_HOME") {
            return Home::new(root);
        }
        let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
        Home::new(home.join("Library/Application Support/tech.phin.patchbay"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The person's own manifest.
    pub fn manifest(&self) -> PathBuf {
        self.root.join("patchbay.toml")
    }

    /// Where each team subscription is checked out, one folder each, named for its scope.
    pub fn teams(&self) -> PathBuf {
        self.root.join("teams")
    }

    /// What the running gateway says about itself, for the command line and clients to find it.
    pub fn gateway_file(&self) -> PathBuf {
        self.root.join("gateway.toml")
    }

    /// Every manifest, lowest first: each team's (`teams/<name>/patchbay.toml`, by name), then the person's own. A
    /// missing file is no layer; one that can't be read is left out and said why.
    pub fn layers(&self) -> (Vec<Layer>, Vec<String>) {
        let mut layers = Vec::new();
        let mut errors = Vec::new();
        let mut teams: Vec<(String, PathBuf)> = std::fs::read_dir(self.teams())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_str()?.to_string();
                let path = entry.path().join("patchbay.toml");
                path.is_file().then_some((name, path))
            })
            .collect();
        teams.sort();
        let mine = (Scope::Me, self.manifest());
        for (scope, path) in teams
            .into_iter()
            .map(|(name, path)| (Scope::Team(name), path))
            .chain([mine])
        {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            match Manifest::from_toml(&text) {
                Ok(manifest) => layers.push(Layer { scope, manifest }),
                Err(error) => errors.push(format!("{}: {error}", path.display())),
            }
        }
        (layers, errors)
    }
}

#[cfg(test)]
mod tests {
    use super::{Home, Scope};

    #[test]
    fn layers_are_each_team_by_name_then_your_own() {
        let root = std::env::temp_dir().join(format!("patchbay-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = Home::new(&root);
        for team in ["zeta", "alpha"] {
            std::fs::create_dir_all(home.teams().join(team)).unwrap();
            std::fs::write(
                home.teams().join(team).join("patchbay.toml"),
                "[servers.x]\ncommand = \"x\"\n",
            )
            .unwrap();
        }
        std::fs::write(home.manifest(), "[servers.y]\nbroken = 1\n").unwrap();
        let (layers, errors) = home.layers();
        let scopes: Vec<Scope> = layers.into_iter().map(|layer| layer.scope).collect();
        assert_eq!(
            scopes,
            [Scope::Team("alpha".into()), Scope::Team("zeta".into())]
        );
        assert_eq!(errors.len(), 1, "{errors:?}");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
