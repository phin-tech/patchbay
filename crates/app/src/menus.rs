//! The Mac menu bar. Each item names a Command from [`crate::commands`], and the menus show whatever key is bound to
//! it. Labels go through `t!`, so they're in the language the app chose; built after it has.

use std::borrow::Cow;

use gpui_kit::component::input::{Copy, Cut, Paste, Redo, SelectAll, Undo};
use gpui_kit::*;
use rust_i18n::t;

use crate::shortcuts::RunAction;

const APP: &str = "Patchbay";

fn run(label: Cow<str>, name: &'static str) -> MenuItem {
    MenuItem::action(label.into_owned(), RunAction { name })
}

fn edit(label: Cow<str>, action: impl Action, os: OsAction) -> MenuItem {
    MenuItem::os_action(label.into_owned(), action, os)
}

fn menu(name: Cow<str>, items: Vec<MenuItem>) -> Menu {
    Menu {
        name: name.into_owned().into(),
        items,
        disabled: false,
    }
}

pub fn menus() -> Vec<Menu> {
    vec![
        menu(
            APP.into(),
            vec![
                run(t!("About %{app}", app = APP), "app.about"),
                run(t!("Check for Updates…"), "app.check-updates"),
                MenuItem::separator(),
                run(t!("Settings…"), "app.settings"),
                MenuItem::separator(),
                MenuItem::os_submenu(t!("Services").into_owned(), SystemMenuType::Services),
                MenuItem::separator(),
                run(t!("Hide %{app}", app = APP), "app.hide"),
                run(t!("Hide Others"), "app.hide-others"),
                run(t!("Show All"), "app.show-all"),
                MenuItem::separator(),
                run(t!("Quit %{app}", app = APP), "app.quit"),
            ],
        ),
        menu(
            t!("File"),
            vec![
                run(t!("New Window"), "file.new"),
                MenuItem::separator(),
                run(t!("Close Window"), "file.close"),
            ],
        ),
        // The kit's own actions, which every Input handles, and which the system needs to recognise.
        menu(
            t!("Edit"),
            vec![
                edit(t!("Undo"), Undo, OsAction::Undo),
                edit(t!("Redo"), Redo, OsAction::Redo),
                MenuItem::separator(),
                edit(t!("Cut"), Cut, OsAction::Cut),
                edit(t!("Copy"), Copy, OsAction::Copy),
                edit(t!("Paste"), Paste, OsAction::Paste),
                edit(t!("Select All"), SelectAll, OsAction::SelectAll),
            ],
        ),
        menu(
            t!("View"),
            vec![
                run(t!("Servers"), "view.servers"),
                run(t!("Clients"), "view.clients"),
                MenuItem::separator(),
                run(t!("Command Palette…"), "app.palette"),
                MenuItem::separator(),
                run(t!("Bigger Text"), "view.bigger-text"),
                run(t!("Smaller Text"), "view.smaller-text"),
                run(t!("Actual Text Size"), "view.actual-text"),
                MenuItem::separator(),
                run(t!("Dark Mode"), "view.dark"),
                // macOS adds Enter Full Screen here itself; ⌃⌘F stays bound to window.fullscreen for it.
            ],
        ),
        menu(
            t!("Servers"),
            vec![
                run(t!("Add Server…"), "servers.add"),
                run(t!("Reload Servers"), "servers.reload"),
                run(t!("Edit patchbay.toml…"), "servers.edit"),
                MenuItem::separator(),
                run(t!("Copy Gateway URL"), "gateway.copy-url"),
            ],
        ),
        // GPUI finds the Window menu by this name to make it the system's, so it stays as it is.
        menu(
            "Window".into(),
            vec![
                run(t!("Minimize"), "window.minimize"),
                run(t!("Zoom"), "window.zoom"),
                MenuItem::separator(),
                run(t!("Bring All to Front"), "window.front"),
            ],
        ),
        menu(
            t!("Help"),
            vec![
                run(t!("Keys…"), "app.keys"),
                run(t!("Show Logs in Finder"), "app.logs"),
            ],
        ),
    ]
}
