//! The app's look: Typecase's themes, chosen in Settings, light, dark or the system's, at the text size set there.
//! Views take colours and fonts from `cx.theme()`.

use std::sync::atomic::{AtomicU32, Ordering};

use gpui_kit::component::{ActiveTheme as _, Theme};
use gpui_kit::{App, Subscription, Window, px};
use patchbay_core::{Appearance as Chosen, Settings};
use typecase::theme::{self, Appearance, ThemeSettings};

/// The text size in Settings, as a multiple of the theme's own, for the refinement every theme passes through.
static TEXT_SCALE: AtomicU32 = AtomicU32::new(0x3f93_3333); // 1.15, DEFAULT_TEXT_SCALE

fn text_scale() -> f32 {
    f32::from_bits(TEXT_SCALE.load(Ordering::Relaxed))
}

/// Installs Typecase's themes, each drawn at the text size in Settings.
pub fn init(cx: &mut App) {
    theme::init_with(cx, |config| {
        let scale = text_scale();
        config.font_size = Some(config.font_size.unwrap_or(16.) * scale);
        config.mono_font_size = Some(config.mono_font_size.unwrap_or(13.) * scale);
    });
}

/// What Typecase shows for `settings`. `PATCHBAY_APPEARANCE` forces `light` or `dark`, for checking both
/// offscreen.
pub fn from_settings(settings: &Settings) -> ThemeSettings {
    let forced = std::env::var("PATCHBAY_APPEARANCE")
        .ok()
        .and_then(|name| Appearance::from_name(&name));
    let appearance = forced.unwrap_or(match settings.appearance {
        Chosen::System => Appearance::System,
        Chosen::Light => Appearance::Light,
        Chosen::Dark => Appearance::Dark,
    });
    ThemeSettings::new(
        appearance,
        settings.light_theme.clone(),
        settings.dark_theme.clone(),
    )
}

/// Puts what Typecase shows, as the theme picker or a Command changed it, into `settings`, to be saved.
pub fn into_settings(shown: &ThemeSettings, settings: &mut Settings) {
    settings.appearance = match shown.appearance {
        Appearance::System => Chosen::System,
        Appearance::Light => Chosen::Light,
        Appearance::Dark => Chosen::Dark,
    };
    settings.light_theme = shown.light.to_string();
    settings.dark_theme = shown.dark.to_string();
}

/// Draws text at `scale` times the theme's own size, in every window.
pub fn set_text_scale(scale: f32, cx: &mut App) {
    TEXT_SCALE.store(scale.to_bits(), Ordering::Relaxed);
    // The next theme shown is refined with it; the one showing now is resized here.
    let name = theme::settings(cx).theme(cx.theme().mode).clone();
    if let Some(config) = theme::config(&name, cx) {
        Theme::update(cx, |theme| {
            theme.font_size = px(config.font_size.unwrap_or(16.));
            theme.mono_font_size = px(config.mono_font_size.unwrap_or(13.));
        });
    }
}

/// Keeps `window` in step with the system's appearance while Settings say to follow it. Keep it as long as the window.
pub fn follow_system(window: &Window, cx: &mut App) -> Subscription {
    theme::follow_system(window, cx)
}
