//! What a person has set in the Settings window, kept as a small TOML file. Every setting has a default, a file that's
//! missing or can't be read gives the defaults, and a setting this version doesn't know is left alone.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::files::write_atomically;

/// The light or dark look: following the system, or fixed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

/// The themes the app starts with, Typecase's, by name.
pub const LIGHT_THEME: &str = "Typecase Light";
pub const DARK_THEME: &str = "Typecase Dark";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub appearance: Appearance,
    /// The theme shown while the app is light, and while it's dark, by name: one of Typecase's, chosen in Settings.
    pub light_theme: String,
    pub dark_theme: String,
    /// How much bigger than the theme's sizes text is drawn: one of [`TEXT_SCALES`].
    pub text_scale: f32,
    /// The shortcut that brings the app forward from any app, as `global-hotkey` reads it: `ctrl+alt+super+Space`.
    pub hotkey: String,
    /// Whether Sparkle offers betas, which come before a release. A beta starts with them on, so it keeps getting
    /// them until they're switched off; a tester moves on to the release that follows, since builds only go up.
    pub beta_updates: bool,
    /// Whether Patchbay leaves the Dock while none of its windows is open, keeping only its menu bar icon.
    pub hide_dock: bool,
    /// Whether the main window has the patch bay, a rack of the clients and servers with a cable between each client
    /// and the servers it's given, and opens on it.
    pub patch_bay: bool,
    /// Whether the patch bay shows the servers, and the skills. Hiding one draws it and its Cables no more; what each
    /// client is given stays as it is.
    pub patch_bay_servers: bool,
    pub patch_bay_skills: bool,
    /// A skills folder chosen for a client in place of the one it usually reads, by the client's slug: a path, which
    /// may start `~/`. Skills are linked there, by the app and `patchbay skills` alike.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub skills_folders: BTreeMap<String, String>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            appearance: Appearance::default(),
            light_theme: LIGHT_THEME.to_string(),
            dark_theme: DARK_THEME.to_string(),
            text_scale: DEFAULT_TEXT_SCALE,
            hotkey: "ctrl+alt+super+Space".to_string(),
            beta_updates: crate::VERSION.contains("-beta"),
            hide_dock: true,
            patch_bay: true,
            patch_bay_servers: true,
            patch_bay_skills: true,
            skills_folders: BTreeMap::new(),
        }
    }
}

/// The text size a new install starts at, and Actual Size goes back to: a step above the theme's own, which reads
/// small at a desk.
pub const DEFAULT_TEXT_SCALE: f32 = 1.15;

/// The text sizes on offer, as multiples of the theme's own.
pub const TEXT_SCALES: [f32; 7] = [0.85, 1.0, 1.15, 1.3, 1.5, 1.75, 2.0];

/// The scale `steps` along from `current`, staying inside the range. A saved scale that isn't one of the steps moves
/// to the nearest.
pub fn step_text_scale(current: f32, steps: isize) -> f32 {
    let nearest = TEXT_SCALES
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (*a - current).abs().total_cmp(&(*b - current).abs()))
        .map_or(1, |(index, _)| index);
    let index = (nearest as isize + steps).clamp(0, TEXT_SCALES.len() as isize - 1);
    TEXT_SCALES[index as usize]
}

impl Settings {
    /// The settings in `path`, or the defaults if there's no file there or it doesn't parse.
    pub fn load(path: &Path) -> Settings {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Writes the settings to `path`, making its folder if need be. Settings the file has that this version doesn't
    /// know are kept, so going back and forth between versions loses nothing.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let mut table: toml::Table = fs::read_to_string(path)
            .ok()
            .and_then(|text| text.parse().ok())
            .unwrap_or_default();
        let ours = toml::Table::try_from(self).map_err(io::Error::other)?;
        // A setting left out while it's empty would otherwise keep what the file had, and come back at the next launch.
        table.remove("skills_folders");
        table.extend(ours);
        let text = toml::to_string(&table).map_err(io::Error::other)?;
        write_atomically(path, text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::{Appearance, Settings, TEXT_SCALES, step_text_scale};
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("settings-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        folder.join("settings.toml")
    }

    #[test]
    fn settings_come_back_as_they_were_saved() {
        let path = scratch("round-trip");
        let settings = Settings {
            appearance: Appearance::Dark,
            light_theme: "Gruvppuccin Light".to_string(),
            dark_theme: "Nightbrew Cove".to_string(),
            text_scale: 1.3,
            hotkey: "ctrl+alt+super+KeyN".to_string(),
            beta_updates: true,
            hide_dock: false,
            patch_bay: false,
            patch_bay_servers: false,
            patch_bay_skills: true,
            skills_folders: [("cursor".to_string(), "~/dotfiles/skills".to_string())].into(),
        };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
    }

    #[test]
    fn a_skills_folder_put_back_to_the_usual_one_stays_put_back() {
        let path = scratch("usual-again");
        let mut settings = Settings::default();
        settings
            .skills_folders
            .insert("cursor".into(), "~/elsewhere".into());
        settings.save(&path).unwrap();
        settings.skills_folders.clear();
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path).skills_folders, BTreeMap::new());
    }

    #[test]
    fn a_settings_file_reads_plainly() {
        let settings = Settings {
            appearance: Appearance::System,
            light_theme: "Typecase Light".to_string(),
            dark_theme: "Typecase Dark".to_string(),
            text_scale: 1.0,
            hotkey: "ctrl+alt+super+Space".to_string(),
            beta_updates: false,
            hide_dock: true,
            patch_bay: true,
            patch_bay_servers: true,
            patch_bay_skills: true,
            skills_folders: BTreeMap::new(),
        };
        let text = toml::to_string(&settings).unwrap();
        insta::assert_snapshot!(text, @r#"
        appearance = "system"
        light_theme = "Typecase Light"
        dark_theme = "Typecase Dark"
        text_scale = 1.0
        hotkey = "ctrl+alt+super+Space"
        beta_updates = false
        hide_dock = true
        patch_bay = true
        patch_bay_servers = true
        patch_bay_skills = true
        "#);
    }

    #[test]
    fn a_missing_or_broken_file_gives_the_defaults() {
        let path = scratch("broken");
        assert_eq!(Settings::load(&path), Settings::default());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "appearance = [").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
    }

    #[test]
    fn saving_keeps_settings_from_another_version() {
        let path = scratch("unknown");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "appearance = \"light\"\nfrom_the_future = 3\n").unwrap();
        let mut settings = Settings::load(&path);
        assert_eq!(settings.appearance, Appearance::Light);
        settings.appearance = Appearance::System;
        settings.save(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("from_the_future = 3"), "{text}");
        assert!(text.contains("appearance = \"system\""), "{text}");
    }

    #[test]
    fn text_size_steps_stop_at_both_ends() {
        let last = TEXT_SCALES[TEXT_SCALES.len() - 1];
        assert_eq!(step_text_scale(1.0, 1), 1.15);
        assert_eq!(step_text_scale(1.0, -5), TEXT_SCALES[0]);
        assert_eq!(step_text_scale(last, 1), last);
    }

    #[test]
    fn an_odd_saved_text_size_steps_from_the_nearest() {
        assert_eq!(step_text_scale(1.2, 0), 1.15);
        assert_eq!(step_text_scale(9.0, -1), 1.75);
    }
}
