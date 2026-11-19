//! Crash reports. A panic writes one beside the logs in `~/Library/Logs/tech.phin.patchbay/`, before the usual message on
//! stderr, and the next launch says where it is.

use std::fs;
use std::path::PathBuf;

use gpui_kit::*;
use jiff::Zoned;
use patchbay_core::{crash, write_atomically};
use rust_i18n::t;

use crate::logging::folder;

/// Writes a report for any panic from here on. Early in `main`, so a panic at launch is caught too.
pub fn install() {
    let Some(folder) = folder() else {
        return;
    };
    let usual = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let location = info.location().map(ToString::to_string);
        let now = Zoned::now();
        let text = crash::report(
            &now,
            crash::message(info.payload()),
            location.as_deref(),
            &backtrace,
        );
        let path = folder.join(crash::file_name(now.timestamp()));
        if write_atomically(&path, text.as_bytes()).is_ok() {
            tracing::error!("crash report in {}", path.display());
        }
        usual(info);
    }));
}

/// The newest report the person hasn't been told about, marking it told. Names sort by time.
fn unseen() -> Option<PathBuf> {
    let folder = folder()?;
    let marker = folder.join(".seen");
    let seen = fs::read_to_string(&marker).unwrap_or_default();
    let newest = fs::read_dir(&folder)
        .ok()?
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| crash::is_report(name))
        .max()?;
    if newest.as_str() <= seen.trim() {
        return None;
    }
    let _ = fs::write(&marker, &newest);
    Some(folder.join(newest))
}

/// After a crash, says so once, on `window`, and offers to show the report.
pub fn tell_if_crashed(window: &mut Window, cx: &mut App) {
    let Some(report) = unseen() else {
        return;
    };
    let answer = window.prompt(
        PromptLevel::Warning,
        &t!("%{app} quit unexpectedly last time", app = "Patchbay"),
        Some(&t!("A report of what happened is in your Logs folder.")),
        &[&*t!("Show Report"), &*t!("OK")],
        cx,
    );
    cx.spawn(async move |cx| {
        if answer.await == Ok(0) {
            cx.update(|cx| cx.reveal_path(&report));
        }
    })
    .detach();
}
