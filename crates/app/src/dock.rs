//! Patchbay in the Dock only while one of its windows is open, if the person wants that: with none open it lives in
//! the menu bar, where the gateway it keeps running is one click away.

use gpui_kit::*;

use crate::settings;

/// Leaves the Dock when the last window closes, if the settings say so.
pub fn init(cx: &mut App) {
    if cfg!(feature = "screenshot") {
        return;
    }
    cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() && settings::current(cx).hide_dock {
            cx.set_activation_policy(ActivationPolicy::Accessory);
        }
    })
    .detach();
}

/// Back in the Dock and the foreground, for a window about to open.
pub fn show(cx: &mut App) {
    if cfg!(feature = "screenshot") {
        return;
    }
    cx.set_activation_policy(ActivationPolicy::Regular);
    cx.activate(true);
}

/// Out of the Dock from the start, for a launch with no window: the bridge starting the gateway.
pub fn start_hidden(cx: &mut App) {
    if settings::current(cx).hide_dock {
        cx.set_activation_policy(ActivationPolicy::Accessory);
    }
}
