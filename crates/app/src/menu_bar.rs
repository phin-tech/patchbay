//! An icon in the menu bar, with a menu, and a shortcut that brings Patchbay forward from any app (`hotkey` in
//! the settings file, `ctrl+alt+super+Space` unless it says otherwise), which Settings changes while the app runs.
//! Both run Commands, as the menus do.

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
    hotkeys: Option<GlobalHotKeyManager>,
    /// The shortcut registered now, to take away when it's changed.
    registered: Option<HotKey>,
}

impl Global for Extra {}

/// Why a shortcut wasn't changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    NoModifier,
    NotAKey,
    Taken,
}

impl Refused {
    /// What Settings says of it.
    pub fn said(self) -> String {
        match self {
            Refused::NoModifier => {
                t!("A shortcut that works from any app needs ⌃, ⌥ or ⌘ with its key.")
            }
            Refused::NotAKey => t!("That key can't be a shortcut."),
            Refused::Taken => t!("Another app has that shortcut."),
        }
        .into_owned()
    }
}

/// A keystroke as the settings file writes a shortcut, `ctrl+alt+super+KeyC`, if it's a key `global-hotkey` knows
/// held with a modifier besides ⇧.
pub fn written(keystroke: &Keystroke) -> Result<String, Refused> {
    let held = &keystroke.modifiers;
    if !(held.control || held.alt || held.platform) {
        return Err(Refused::NoModifier);
    }
    let key = keystroke.key.as_str();
    let named = match key {
        "space" => "Space",
        "enter" => "Enter",
        "tab" => "Tab",
        "up" => "ArrowUp",
        "down" => "ArrowDown",
        "left" => "ArrowLeft",
        "right" => "ArrowRight",
        "," => "Comma",
        "." => "Period",
        "/" => "Slash",
        ";" => "Semicolon",
        "'" => "Quote",
        "[" => "BracketLeft",
        "]" => "BracketRight",
        "\\" => "Backslash",
        "`" => "Backquote",
        "-" => "Minus",
        "=" => "Equal",
        _ => "",
    };
    let code = if !named.is_empty() {
        named.to_string()
    } else if key.len() == 1 && key.chars().all(|c| c.is_ascii_alphabetic()) {
        format!("Key{}", key.to_ascii_uppercase())
    } else if key.len() == 1 && key.chars().all(|c| c.is_ascii_digit()) {
        format!("Digit{key}")
    } else if key
        .strip_prefix('f')
        .and_then(|number| number.parse::<u8>().ok())
        .is_some_and(|number| (1..=20).contains(&number))
    {
        key.to_uppercase()
    } else {
        return Err(Refused::NotAKey);
    };
    let held: Vec<&str> = [
        (held.control, "ctrl"),
        (held.alt, "alt"),
        (held.shift, "shift"),
        (held.platform, "super"),
    ]
    .into_iter()
    .filter_map(|(on, name)| on.then_some(name))
    .collect();
    let written = format!("{}+{code}", held.join("+"));
    HotKey::from_str(&written).map_err(|_| Refused::NotAKey)?;
    Ok(written)
}

/// A shortcut as the settings file writes it, `ctrl+alt+super+KeyC`, as macOS shows one: `⌃⌥⌘C`.
pub fn shown(written: &str) -> String {
    written
        .split('+')
        .map(|part| {
            let symbol = match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => "⌃",
                "alt" | "option" => "⌥",
                "shift" => "⇧",
                "super" | "cmd" | "command" | "meta" => "⌘",
                "arrowup" => "↑",
                "arrowdown" => "↓",
                "arrowleft" => "←",
                "arrowright" => "→",
                "enter" => "↩",
                "tab" => "⇥",
                "comma" => ",",
                "period" => ".",
                "slash" => "/",
                "semicolon" => ";",
                "quote" => "'",
                "bracketleft" => "[",
                "bracketright" => "]",
                "backslash" => "\\",
                "backquote" => "`",
                "minus" => "-",
                "equal" => "=",
                _ => "",
            };
            if symbol.is_empty() {
                part.strip_prefix("Key")
                    .or_else(|| part.strip_prefix("Digit"))
                    .unwrap_or(part)
                    .to_string()
            } else {
                symbol.to_string()
            }
        })
        .collect()
}

/// Changes the shortcut to `written` and keeps it in the settings, taking the old one away from macOS and giving it
/// the new one at once. One another app holds is refused, and the old one stays.
pub fn change(written: &str, cx: &mut App) -> Result<(), Refused> {
    let new = HotKey::from_str(written).map_err(|_| Refused::NotAKey)?;
    let old = HotKey::from_str(&settings::current_hotkey(cx)).ok();
    if cx.has_global::<Extra>() {
        let extra = cx.global_mut::<Extra>();
        if let Some(manager) = &extra.hotkeys {
            if let Some(registered) = extra.registered.take() {
                let _ = manager.unregister(registered);
            }
            if manager.register(new).is_err() {
                // Back as it was, so a refusal changes nothing.
                if let Some(old) = old
                    && manager.register(old).is_ok()
                {
                    extra.registered = Some(old);
                }
                return Err(Refused::Taken);
            }
            extra.registered = Some(new);
        }
    }
    settings::set_hotkey(written.to_string(), cx);
    Ok(())
}

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
    let mut registered = None;
    match (&hotkeys, HotKey::from_str(&shortcut)) {
        (Some(manager), Ok(hotkey)) => match manager.register(hotkey) {
            Ok(()) => registered = Some(hotkey),
            Err(error) => tracing::warn!("the shortcut {shortcut} isn't free: {error}"),
        },
        (_, Err(error)) => tracing::warn!("the shortcut {shortcut} isn't one: {error}"),
        (None, _) => {}
    }
    cx.set_global(Extra {
        _icon: icon,
        hotkeys,
        registered,
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
    use gpui_kit::Keystroke;
    use patchbay_core::Settings;

    use super::{Refused, shown, written};

    fn pressed(keys: &'static str) -> Keystroke {
        Keystroke::parse(keys).unwrap()
    }

    #[test]
    fn a_shortcut_pressed_is_written_as_the_settings_file_keeps_it_and_shown_as_a_mac_shows_it() {
        let kept = written(&pressed("ctrl-alt-cmd-k")).unwrap();
        assert_eq!(kept, "ctrl+alt+super+KeyK");
        assert_eq!(shown(&kept), "⌃⌥⌘K");
        assert_eq!(shown("ctrl+alt+super+Space"), "⌃⌥⌘Space");
        assert_eq!(
            written(&pressed("alt-shift-1")).unwrap(),
            "alt+shift+Digit1"
        );
    }

    #[test]
    fn a_shortcut_with_no_modifier_but_shift_is_refused() {
        assert_eq!(written(&pressed("k")), Err(Refused::NoModifier));
        assert_eq!(written(&pressed("shift-k")), Err(Refused::NoModifier));
    }

    #[test]
    fn the_shortcut_in_new_settings_is_one() {
        let shortcut = Settings::default().hotkey;
        assert!(HotKey::from_str(&shortcut).is_ok(), "{shortcut}");
    }
}
