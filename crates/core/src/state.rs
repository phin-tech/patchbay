//! What the app remembers between runs that isn't a choice in Settings: where the main window was, and whether
//! Welcome has been shown. Kept apart from
//! the settings, in `state.toml` beside them, since it changes all the time and nobody edits it.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::files::write_atomically;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub window: Option<Frame>,
    /// Whether Welcome has been shown, or wasn't needed, so it opens by itself only once.
    pub welcomed: bool,
}

/// A window's place on the screen, in points from the top left of the main display.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    #[serde(default)]
    pub maximized: bool,
}

/// A display's area, in the same points as a [`Frame`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Screen {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Frame {
    /// Whether enough of the title bar is on one of `screens` to drag the window by, so a window saved on a display
    /// that's gone doesn't come back where it can't be reached.
    pub fn reachable_on(&self, screens: &[Screen]) -> bool {
        const GRIP: f32 = 40.0;
        screens.iter().any(|screen| {
            let left = self.x.max(screen.x);
            let right = (self.x + self.width).min(screen.x + screen.width);
            let top_inside = self.y >= screen.y && self.y + GRIP <= screen.y + screen.height;
            right - left >= GRIP * 2.0 && top_inside
        })
    }
}

impl State {
    /// The state in `path`, or none remembered if there's no file there or it doesn't parse.
    pub fn load(path: &Path) -> State {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let text = toml::to_string(self).map_err(io::Error::other)?;
        write_atomically(path, text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::{Frame, Screen, State};

    const LAPTOP: Screen = Screen {
        x: 0.0,
        y: 0.0,
        width: 1512.0,
        height: 982.0,
    };

    fn frame(x: f32, y: f32) -> Frame {
        Frame {
            x,
            y,
            width: 900.0,
            height: 640.0,
            maximized: false,
        }
    }

    #[test]
    fn a_window_comes_back_where_it_was() {
        let path = std::env::temp_dir().join(format!("state-{}/state.toml", std::process::id()));
        let state = State {
            window: Some(frame(120.0, 80.0)),
            welcomed: true,
        };
        state.save(&path).unwrap();
        assert_eq!(State::load(&path), state);
    }

    #[test]
    fn a_window_left_on_a_missing_display_is_not_restored_there() {
        assert!(frame(120.0, 80.0).reachable_on(&[LAPTOP]));
        // On a second display, to the right, that's been unplugged.
        assert!(!frame(1800.0, 80.0).reachable_on(&[LAPTOP]));
        // Title bar above the top of the screen.
        assert!(!frame(120.0, -300.0).reachable_on(&[LAPTOP]));
        // Mostly off the left edge, but the title bar's end is still there to grab.
        assert!(frame(-800.0, 80.0).reachable_on(&[LAPTOP]));
    }
}
