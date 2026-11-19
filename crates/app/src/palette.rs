//! The command palette (⌘K), Typecase's: every Command in the menus, found by typing a few of its letters. It reads
//! the menus rather than a list of its own, so a new menu item is in it without anything else to change, and a test
//! holds every Command to being in a menu. The main view shows it over itself and runs what's chosen.

use gpui_kit::*;
use typecase::palette::Command;

use crate::menus;
use crate::shortcuts::{self, RunAction};

/// The palette's own Command, which it doesn't list.
pub const NAME: &str = "app.palette";

/// A Command in a menu.
pub struct Entry {
    /// The top-level menu it's in: File, View…
    pub menu: SharedString,
    /// Its label, without the ellipsis.
    pub label: String,
    pub name: &'static str,
}

/// Each Command in the menus, in menu order: the palette lists them, and Keys shows their keys.
pub fn entries() -> Vec<Entry> {
    fn walk(menu: &SharedString, items: Vec<MenuItem>, into: &mut Vec<Entry>) {
        for item in items {
            match item {
                MenuItem::Submenu(submenu) => walk(menu, submenu.items, into),
                MenuItem::Action { name, action, .. } => {
                    if let Some(run) = action.as_any().downcast_ref::<RunAction>() {
                        into.push(Entry {
                            menu: menu.clone(),
                            label: name.trim_end_matches('…').to_string(),
                            name: run.name,
                        });
                    }
                }
                _ => {}
            }
        }
    }
    let mut entries = vec![];
    for menu in menus::menus() {
        walk(&menu.name, menu.items, &mut entries);
    }
    entries
}

/// What the palette lists: each Command in the menus but its own, with its menu and its key. Choosing one says its
/// name.
pub fn commands() -> Vec<Command> {
    entries()
        .into_iter()
        .filter(|entry| entry.name != NAME)
        .map(|entry| {
            let command = Command::new(entry.label, entry.name).group(entry.menu.to_string());
            match shortcuts::key_for(entry.name) {
                Some(key) => command.shortcut(key),
                None => command,
            }
        })
        .collect()
}
