//! The About box, as a dialog over the window that asked, which closes with Escape or a click outside it, and what
//! Keys (⌘/), Typecase's sheet of shortcuts, lists.

use std::sync::{Arc, LazyLock};

use gpui_kit::component::{ActiveTheme as _, StyledExt as _, WindowExt as _, v_flex};
use gpui_kit::*;
use patchbay_core::version;
use rust_i18n::t;
use typecase::help::Section;

use crate::palette;
use crate::shortcuts::{self, SHORTCUTS};

/// The logo, from `packaging/logo.png`, compiled in since a release carries only the binary.
static LOGO: LazyLock<Arc<Image>> = LazyLock::new(|| {
    Arc::new(Image::from_bytes(
        ImageFormat::Png,
        include_bytes!("../../../packaging/logo.png").to_vec(),
    ))
});

pub fn about(window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, |dialog, _, cx| {
        let muted = cx.theme().muted_foreground;
        dialog.w(px(340.)).child(
            v_flex()
                .items_center()
                .gap_2()
                .py_4()
                .child(img(LOGO.clone()).size(px(96.)))
                .child(div().pt_2().text_xl().font_semibold().child("Patchbay"))
                .child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child(t!("Version %{version}", version = version()).into_owned()),
                )
                .child(
                    div().text_sm().text_color(muted).child(
                        t!(
                            "One place for your MCP servers and agent skills, shared with your team"
                        )
                        .into_owned(),
                    ),
                ),
        )
    });
}

/// The Commands that have keys, grouped by the menu they're in, with any that are in no menu at the end.
pub fn key_groups() -> Vec<(SharedString, Vec<(String, String)>)> {
    let mut groups: Vec<(SharedString, Vec<(String, String)>)> = vec![];
    let entries = palette::entries();
    for entry in &entries {
        let Some(key) = shortcuts::key_for(entry.name) else {
            continue;
        };
        match groups.last_mut() {
            Some((menu, rows)) if *menu == entry.menu => rows.push((entry.label.clone(), key)),
            _ => groups.push((entry.menu.clone(), vec![(entry.label.clone(), key)])),
        }
    }
    let mut others = vec![];
    for (shortcut, name, _) in SHORTCUTS {
        let listed = entries.iter().any(|entry| entry.name == *name);
        let first = SHORTCUTS
            .iter()
            .find(|(_, bound, _)| bound == name)
            .map(|(keys, _, _)| keys);
        if !listed && first == Some(shortcut) {
            others.push((name.to_string(), shortcuts::symbols(shortcut)));
        }
    }
    if !others.is_empty() {
        groups.push((t!("Other").into_owned().into(), others));
    }
    groups
}

/// What Keys lists: a section for each menu, with its Commands' keys, and any in no menu at the end.
pub fn key_sections() -> Vec<Section> {
    key_groups()
        .into_iter()
        .map(|(menu, rows)| {
            Section::new(
                menu.to_string(),
                rows.into_iter().map(|(label, key)| (key, label)),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::key_groups;

    #[test]
    fn keys_are_grouped_by_their_menu() {
        let groups = key_groups();
        let file = groups
            .iter()
            .find(|(menu, _)| menu == "File")
            .expect("a File group");
        let new = "New Window";
        assert!(
            file.1.contains(&(new.to_string(), "⇧⌘N".to_string())),
            "{:?}",
            file.1
        );
        let other = groups
            .iter()
            .find(|(menu, _)| menu == "Other")
            .expect("an Other group");
        assert!(
            other.1.iter().any(|(name, _)| name == "window.fullscreen"),
            "{:?}",
            other.1
        );
    }
}
