//! An icon in the menu bar, with a menu, and a shortcut that brings Patchbay forward from any app (`hotkey` in
//! the settings file, `ctrl+alt+super+Space` unless it says otherwise). Both run Commands, as the menus do.

use std::str::FromStr as _;

use futures::StreamExt as _;
use futures::channel::mpsc::unbounded;
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use gpui_kit::*;
use rust_i18n::t;
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::{commands, settings, view};

/// The icon, the menu and the shortcut, kept for as long as the app runs: dropping them takes them away.
struct Extra {
    _icon: TrayIcon,
    _hotkeys: Option<GlobalHotKeyManager>,
}

impl Global for Extra {}

/// The icon: `packaging/menubar.png`, black on clear, which macOS shows white in a dark menu bar.
fn icon() -> anyhow::Result<Icon> {
    let image =
        image::load_from_memory(include_bytes!("../../../packaging/menubar.png"))?.into_rgba8();
    let (width, height) = image.dimensions();
    Ok(Icon::from_rgba(image.into_raw(), width, height)?)
}

/// Runs a Command in the front window, or with none open, opens one first.
fn run(name: &str, cx: &mut App) {
    cx.activate(true);
    let front = cx.active_window().or_else(|| cx.windows().first().copied());
    let front = front.or_else(|| view::open(false, cx));
    if let Some(front) = front {
        let _ = front.update(cx, |_, window, cx| commands::run(name, window, cx));
    }
}

pub fn start(cx: &mut App) {
    if let Err(error) = try_start(cx) {
        tracing::warn!("no menu bar extra: {error:#}");
    }
}

fn try_start(cx: &mut App) -> anyhow::Result<()> {
    let items: Vec<(MenuItem, &'static str)> = vec![
        (
            MenuItem::new(t!("Open %{app}", app = "Patchbay"), true, None),
            "window.front",
        ),
        (MenuItem::new(t!("New Window"), true, None), "file.new"),
        (MenuItem::new(t!("Settings…"), true, None), "app.settings"),
        (
            MenuItem::new(t!("Quit %{app}", app = "Patchbay"), true, None),
            "app.quit",
        ),
    ];
    let separator = PredefinedMenuItem::separator();
    let menu = Menu::with_items(&[
        &items[0].0,
        &items[1].0,
        &separator,
        &items[2].0,
        &items[3].0,
    ])?;
    let commands: Vec<(MenuId, &'static str)> = items
        .iter()
        .map(|(item, name)| (item.id().clone(), *name))
        .collect();
    let icon = TrayIconBuilder::new()
        .with_icon_templated(icon()?)
        .with_tooltip("Patchbay")
        .with_menu(Box::new(menu))
        .build()?;

    let hotkeys = GlobalHotKeyManager::new().ok();
    let shortcut = settings::current(cx).hotkey;
    match (&hotkeys, HotKey::from_str(&shortcut)) {
        (Some(manager), Ok(hotkey)) => {
            if let Err(error) = manager.register(hotkey) {
                tracing::warn!("the shortcut {shortcut} isn't free: {error}");
            }
        }
        (_, Err(error)) => tracing::warn!("the shortcut {shortcut} isn't one: {error}"),
        (None, _) => {}
    }
    cx.set_global(Extra {
        _icon: icon,
        _hotkeys: hotkeys,
    });

    // Both arrive off GPUI's thread; a channel brings them to it.
    let (sender, mut asked) = unbounded::<&'static str>();
    let menu_sender = sender.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if let Some((_, name)) = commands.iter().find(|(id, _)| *id == event.id) {
            let _ = menu_sender.unbounded_send(*name);
        }
    }));
    GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
        if event.state == HotKeyState::Pressed {
            let _ = sender.unbounded_send("window.front");
        }
    }));
    cx.spawn(async move |cx| {
        while let Some(name) = asked.next().await {
            cx.update(|cx| run(name, cx));
        }
    })
    .detach();
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr as _;

    use global_hotkey::hotkey::HotKey;
    use patchbay_core::Settings;

    #[test]
    fn the_shortcut_in_new_settings_is_one() {
        let shortcut = Settings::default().hotkey;
        assert!(HotKey::from_str(&shortcut).is_ok(), "{shortcut}");
    }
}
