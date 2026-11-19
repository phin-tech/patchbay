//! Where the main window was, put back at the next launch, unless the display it was on has gone, and whether
//! Welcome has been shown. Kept in `state.toml` beside the settings, and written when the app quits.

use std::path::PathBuf;

use gpui_kit::*;

use patchbay_core::{Frame, Screen, State};

use crate::settings;

struct Remembered {
    state: State,
    path: Option<PathBuf>,
}

impl Global for Remembered {}

/// Reads where the window was, and saves where it is on quitting.
pub fn init(cx: &mut App) {
    let path = settings::file("state.toml");
    let state = path.as_deref().map(State::load).unwrap_or_default();
    cx.set_global(Remembered { state, path });
    cx.on_app_quit(|cx| {
        let remembered = cx.global::<Remembered>();
        if let Some(path) = &remembered.path
            && let Err(error) = remembered.state.save(path)
        {
            tracing::warn!("state not saved to {}: {error}", path.display());
        }
        async {}
    })
    .detach();
}

/// Whether Welcome has been shown, or wasn't needed.
pub fn welcomed(cx: &App) -> bool {
    cx.try_global::<Remembered>()
        .is_some_and(|remembered| remembered.state.welcomed)
}

/// Notes that Welcome has been shown, so it isn't again: saved at once, so a launch that doesn't quit cleanly still
/// remembers it.
pub fn set_welcomed(cx: &mut App) {
    if !cx.has_global::<Remembered>() {
        return;
    }
    let remembered = cx.global_mut::<Remembered>();
    remembered.state.welcomed = true;
    if let Some(path) = &remembered.path
        && let Err(error) = remembered.state.save(path)
    {
        tracing::warn!("state not saved to {}: {error}", path.display());
    }
}

/// For tests: nothing remembered, kept nowhere.
#[cfg(test)]
pub fn in_memory(cx: &mut App) {
    cx.set_global(Remembered {
        state: State::default(),
        path: None,
    });
}

/// Where to put the first window: where it was, if that can still be reached.
pub fn bounds(cx: &App) -> Option<WindowBounds> {
    let frame = cx.try_global::<Remembered>()?.state.window?;
    let screens: Vec<Screen> = cx
        .displays()
        .iter()
        .map(|display| {
            let bounds = display.bounds();
            Screen {
                x: bounds.origin.x.into(),
                y: bounds.origin.y.into(),
                width: bounds.size.width.into(),
                height: bounds.size.height.into(),
            }
        })
        .collect();
    if !frame.reachable_on(&screens) {
        return None;
    }
    let bounds = Bounds::new(
        point(px(frame.x), px(frame.y)),
        size(px(frame.width), px(frame.height)),
    );
    Some(if frame.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    })
}

/// Notes where `window` is now, for the next launch. Full screen keeps the place it had before.
pub fn remember(window: &Window, cx: &mut App) {
    let (bounds, maximized) = match window.window_bounds() {
        WindowBounds::Windowed(bounds) => (bounds, false),
        WindowBounds::Maximized(bounds) => (bounds, true),
        WindowBounds::Fullscreen(_) => return,
    };
    if cx.has_global::<Remembered>() {
        cx.global_mut::<Remembered>().state.window = Some(Frame {
            x: bounds.origin.x.into(),
            y: bounds.origin.y.into(),
            width: bounds.size.width.into(),
            height: bounds.size.height.into(),
            maximized,
        });
    }
}
