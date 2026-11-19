//! Every Command, by name, and the ones any window runs. A menu item or a shortcut names a Command; the focused
//! window's view runs what's its own (the palette and Keys are the main view's, drawn over it) and hands the rest to
//! [`run`].

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use typecase::theme::Appearance;

use crate::{about, logging, settings, view};
use rust_i18n::t;

/// Every Command, by name. Menus and shortcuts name these, and a test holds them to this list.
pub const COMMANDS: &[&str] = &[
    "app.about",
    "app.check-updates",
    "app.hide",
    "app.hide-others",
    "app.keys",
    "app.logs",
    "app.palette",
    "app.quit",
    "app.settings",
    "app.show-all",
    "file.close",
    "file.new",
    "gateway.copy-url",
    "servers.add",
    "servers.edit",
    "servers.reload",
    "view.actual-text",
    "view.bigger-text",
    "view.clients",
    "view.dark",
    "view.servers",
    "view.smaller-text",
    "window.fullscreen",
    "window.front",
    "window.minimize",
    "window.zoom",
];

/// Runs a Command that isn't one window's own, in `window`. False if it's not one of these.
pub fn run(name: &str, window: &mut Window, cx: &mut App) -> bool {
    match name {
        "app.about" => about::about(window, cx),
        "app.check-updates" => {
            if !crate::update::check() {
                tell(
                    &t!("Updates come with the released app"),
                    &t!(
                        "%{app} updates itself once it's installed from a release, from apps.phin.tech.",
                        app = "Patchbay"
                    ),
                    window,
                    cx,
                );
            }
        }
        "app.logs" => logging::reveal(cx),
        "app.settings" => settings::open(cx),
        "app.hide" => cx.hide(),
        "app.hide-others" => cx.hide_other_apps(),
        "app.show-all" => cx.unhide_other_apps(),
        "app.quit" => cx.quit(),
        "file.new" => {
            view::open(false, cx);
        }
        "file.close" => window.remove_window(),
        // A lasting choice, the same one as in Settings, which saves it.
        "view.dark" => {
            let appearance = if cx.theme().mode.is_dark() {
                Appearance::Light
            } else {
                Appearance::Dark
            };
            typecase::theme::set_appearance(appearance, cx);
            cx.refresh_windows();
        }
        "view.bigger-text" => settings::step_text(1, cx),
        "view.smaller-text" => settings::step_text(-1, cx),
        "view.actual-text" => settings::step_text(0, cx),
        "window.minimize" => window.minimize_window(),
        "window.zoom" => window.zoom_window(),
        "window.fullscreen" => window.toggle_fullscreen(),
        "window.front" => cx.activate(true),
        _ => return false,
    }
    true
}

/// Shows a message on `window`, with an OK button.
pub fn tell(title: &str, detail: &str, window: &mut Window, cx: &mut App) {
    let answer = window.prompt(PromptLevel::Info, title, Some(detail), &[&*t!("OK")], cx);
    cx.background_spawn(async move {
        let _ = answer.await;
    })
    .detach();
}
