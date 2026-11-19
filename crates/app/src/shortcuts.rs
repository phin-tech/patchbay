//! Keyboard shortcuts. Each one names a Command from [`crate::view`], so anything the menus can reach can be bound here
//! too. Write `primary` for this platform's main modifier, so each shortcut is written once.

use gpui_kit::*;

/// The main shortcut modifier on this platform.
const PRIMARY: &str = if cfg!(target_os = "macos") {
    "cmd"
} else {
    "ctrl"
};

/// Runs a named Command.
#[derive(Action, Clone, PartialEq, Eq)]
#[action(namespace = patchbay, no_json)]
pub struct RunAction {
    pub name: &'static str,
}

/// The key context of the main view, while the view itself has focus. GPUI matches a binding against every context
/// around what has focus, so a shortcut here would reach a text field inside the view and take its letter; the view
/// gives up this context while a field has focus (see its `render`).
pub const CONTEXT: &str = "MainView";

/// A menu shows the first key bound to its Command, so list the one to show first: `primary-=` before `primary-+`.
pub const SHORTCUTS: &[(&str, &str, Option<&str>)] = &[
    ("primary-q", "app.quit", None),
    ("primary-,", "app.settings", None),
    ("primary-h", "app.hide", None),
    ("primary-alt-h", "app.hide-others", None),
    ("primary-n", "servers.add", None),
    ("primary-shift-n", "file.new", None),
    ("primary-1", "view.servers", None),
    ("primary-2", "view.clients", None),
    ("primary-w", "file.close", None),
    ("primary-m", "window.minimize", None),
    ("ctrl-primary-f", "window.fullscreen", None),
    ("primary-shift-d", "view.dark", None),
    ("primary-k", "app.palette", None),
    ("primary-/", "app.keys", None),
    ("primary-shift-p", "app.palette", None),
    ("primary-=", "view.bigger-text", None),
    ("primary-+", "view.bigger-text", None),
    ("primary--", "view.smaller-text", None),
    ("primary-0", "view.actual-text", None),
    ("primary-r", "servers.reload", None),
    ("primary-e", "servers.edit", None),
    ("primary-shift-c", "gateway.copy-url", None),
];

/// A key sequence with this platform's modifier in place of `primary`. Bindings want a static string, and they're only
/// made once, at startup.
fn keys(keys: &str) -> &'static str {
    keys.replace("primary", PRIMARY).leak()
}

pub fn bindings() -> Vec<KeyBinding> {
    SHORTCUTS
        .iter()
        .map(|&(shortcut, name, context)| {
            KeyBinding::new(keys(shortcut), RunAction { name }, context)
        })
        .collect()
}

/// A shortcut as a Mac shows it: `primary-shift-d` is ⇧⌘D, modifiers in the Mac's order.
pub fn symbols(shortcut: &str) -> String {
    let mut rest = shortcut;
    let mut held = [false; 4];
    // Modifiers can be written in any order, so each one found starts the search again.
    while let Some((modifier, after)) = ["ctrl-", "alt-", "shift-", "primary-", "cmd-"]
        .iter()
        .find_map(|modifier| Some((*modifier, rest.strip_prefix(modifier)?)))
        .filter(|(_, after)| !after.is_empty())
    {
        let slot = match modifier {
            "ctrl-" => 0,
            "alt-" => 1,
            "shift-" => 2,
            _ => 3,
        };
        held[slot] = true;
        rest = after;
    }
    let mut out: String = held
        .iter()
        .zip([
            "⌃",
            "⌥",
            "⇧",
            if cfg!(target_os = "macos") {
                "⌘"
            } else {
                "Ctrl+"
            },
        ])
        .filter(|(on, _)| **on)
        .map(|(_, symbol)| symbol)
        .collect();
    out.push_str(&match rest {
        "space" => "Space".to_string(),
        "backspace" => "⌫".to_string(),
        "delete" => "⌦".to_string(),
        "enter" => "↩".to_string(),
        "escape" => "⎋".to_string(),
        "tab" => "⇥".to_string(),
        "up" => "↑".to_string(),
        "down" => "↓".to_string(),
        "left" => "←".to_string(),
        "right" => "→".to_string(),
        key => key.to_uppercase(),
    });
    out
}

/// The first key bound to a Command, as a Mac shows it.
pub fn key_for(name: &str) -> Option<String> {
    SHORTCUTS
        .iter()
        .find(|(_, bound, _)| *bound == name)
        .map(|(shortcut, _, _)| symbols(shortcut))
}

#[cfg(test)]
mod tests {
    use super::symbols;

    #[test]
    fn shortcuts_read_as_a_mac_writes_them() {
        assert_eq!(symbols("primary-shift-d"), "⇧⌘D");
        assert_eq!(symbols("ctrl-primary-f"), "⌃⌘F");
        assert_eq!(symbols("primary--"), "⌘-");
        assert_eq!(symbols("primary-,"), "⌘,");
        assert_eq!(symbols("space"), "Space");
        assert_eq!(symbols("primary-backspace"), "⌘⌫");
    }
}
