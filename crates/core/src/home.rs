//! Where Patchbay keeps its own files, which the app and the command line share.

use std::io;
use std::path::{Path, PathBuf};

use crate::manifest::{Layer, Manifest, Scope};

/// Patchbay's folders: `~/Library/Application Support/tech.phin.patchbay` for what it keeps for itself, and
/// `~/.config/patchbay` for what a person edits (the settings and their servers), so a dotfiles repository can hold
/// those. `PATCHBAY_HOME` puts both in one folder, so tests and trials never touch the real ones.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Home {
    root: PathBuf,
    config: PathBuf,
}

/// The files that moved from Application Support to the config folder: the name each had, and the one it has now.
const MOVED: [(&str, &str); 2] = [
    ("settings.toml", "config.toml"),
    ("patchbay.toml", "patchbay.toml"),
];

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
    /// Everything in one folder, `root`.
    pub fn new(root: impl Into<PathBuf>) -> Home {
        let root = root.into();
        Home {
            config: root.clone(),
            root,
        }
    }

    /// What Patchbay keeps for itself in `root`, and what a person edits in `config`.
    pub fn with_config(root: impl Into<PathBuf>, config: impl Into<PathBuf>) -> Home {
        Home {
            root: root.into(),
            config: config.into(),
        }
    }

    /// `PATCHBAY_HOME` for both, or the folder in Application Support and `~/.config/patchbay`. Not
    /// `XDG_CONFIG_HOME`: an app opened from the Finder, and a bridge a client starts, never see a shell's variables,
    /// so the app and the command line would each read a different file.
    pub fn from_env() -> Home {
        if let Some(root) = std::env::var_os("PATCHBAY_HOME") {
            return Home::new(root);
        }
        let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
        Home::with_config(
            home.join("Library/Application Support/tech.phin.patchbay"),
            home.join(".config/patchbay"),
        )
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The folder of what a person edits.
    pub fn config(&self) -> &Path {
        &self.config
    }

    /// The person's own manifest.
    pub fn manifest(&self) -> PathBuf {
        self.config_file("patchbay.toml", "patchbay.toml")
    }

    /// What's set in the Settings window.
    pub fn settings(&self) -> PathBuf {
        self.config_file("config.toml", "settings.toml")
    }

    /// `name` in the config folder, or `old` in Application Support while it's still there and `name` isn't: a move
    /// that failed leaves the file where it was, and it's read there until it moves, rather than the app starting
    /// afresh in the config folder and hiding it.
    fn config_file(&self, name: &str, old: &str) -> PathBuf {
        let new = self.config.join(name);
        let old = self.root.join(old);
        if self.root != self.config && new.symlink_metadata().is_err() && old.is_file() {
            return old;
        }
        new
    }

    /// Moves the settings and the manifest from where 0.1 kept them, in Application Support, to the
    /// config folder, once: a file already in the config folder wins, and the old one is left where it is. Gives
    /// each file that couldn't be moved and why, so the caller can say so; it stays where it was, to try again.
    pub fn move_old_files(&self) -> Vec<(PathBuf, io::Error)> {
        if self.root == self.config {
            return Vec::new();
        }
        MOVED
            .into_iter()
            .filter_map(|(old, new)| {
                let (old, new) = (self.root.join(old), self.config.join(new));
                // Checked here rather than through `manifest()`, which gives the old file until it's moved.
                if !old.is_file() || new.symlink_metadata().is_ok() {
                    return None;
                }
                move_file(&old, &new).err().map(|error| (old, error))
            })
            .collect()
    }

    /// Where each team subscription is checked out, one folder each, named for its scope.
    pub fn teams(&self) -> PathBuf {
        self.root.join("teams")
    }

    /// `patchbay.lock`: the repository each subscribed team comes from, and the commit of it last accepted.
    pub fn subscriptions(&self) -> PathBuf {
        self.root.join("patchbay.lock")
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

/// Renames `old` to `new`, or copies it and removes `old` when they're on different volumes, as a `~/.config` linked
/// into a dotfiles repository on another disk can be.
fn move_file(old: &Path, new: &Path) -> io::Result<()> {
    if let Some(folder) = new.parent() {
        std::fs::create_dir_all(folder)?;
    }
    if std::fs::rename(old, new).is_ok() {
        return Ok(());
    }
    crate::write_atomically(new, &std::fs::read(old)?)?;
    std::fs::remove_file(old)
}

#[cfg(test)]
mod tests {
    use super::{Home, Scope};

    fn scratch(name: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("patchbay-home-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("support")).unwrap();
        root
    }

    #[test]
    fn the_settings_and_servers_move_from_application_support_to_the_config_folder() {
        let root = scratch("move");
        let home = Home::with_config(root.join("support"), root.join("config"));
        std::fs::write(home.root().join("settings.toml"), "text_scale = 1.3\n").unwrap();
        std::fs::write(home.root().join("patchbay.toml"), "[servers.x]\n").unwrap();
        std::fs::write(home.root().join("state.toml"), "kept").unwrap();
        assert!(home.move_old_files().is_empty());
        assert_eq!(
            std::fs::read_to_string(home.settings()).unwrap(),
            "text_scale = 1.3\n"
        );
        assert_eq!(
            std::fs::read_to_string(home.manifest()).unwrap(),
            "[servers.x]\n"
        );
        assert!(!home.root().join("settings.toml").exists());
        assert!(!home.root().join("patchbay.toml").exists());
        assert!(home.root().join("state.toml").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_file_already_in_the_config_folder_is_never_replaced_by_an_old_one() {
        let root = scratch("kept");
        let home = Home::with_config(root.join("support"), root.join("config"));
        std::fs::create_dir_all(home.config()).unwrap();
        std::fs::write(home.settings(), "new").unwrap();
        std::fs::write(home.root().join("settings.toml"), "old").unwrap();
        assert!(home.move_old_files().is_empty());
        assert_eq!(std::fs::read_to_string(home.settings()).unwrap(), "new");
        assert!(home.root().join("settings.toml").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_file_that_couldnt_be_moved_is_read_where_it_is() {
        let root = scratch("unmoved");
        let home = Home::with_config(root.join("support"), root.join("config"));
        std::fs::write(home.root().join("patchbay.toml"), "[servers.x]\n").unwrap();
        std::fs::write(home.root().join("settings.toml"), "").unwrap();
        assert_eq!(home.manifest(), home.root().join("patchbay.toml"));
        assert_eq!(home.settings(), home.root().join("settings.toml"));
        assert!(home.move_old_files().is_empty());
        assert_eq!(home.manifest(), home.config().join("patchbay.toml"));
        assert_eq!(home.settings(), home.config().join("config.toml"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn one_folder_for_both_moves_nothing() {
        let root = scratch("one");
        let home = Home::new(root.join("support"));
        std::fs::write(home.root().join("settings.toml"), "old").unwrap();
        assert!(home.move_old_files().is_empty());
        assert!(home.root().join("settings.toml").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }

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
