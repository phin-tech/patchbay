//! What macOS asks the app to open: `patchbay:` links.
//! They can come before the app has finished launching, when there's nothing to open them in yet, so they wait in a
//! channel until it has.

use crate::{settings, view};
use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, unbounded};
use gpui_kit::*;
use patchbay_core::link_target;

pub const SCHEME: &str = "patchbay";

/// Collects what macOS asks to open, from before launch on.
pub fn listen(app: &Application) -> UnboundedReceiver<Vec<String>> {
    let (sender, receiver) = unbounded();
    app.on_open_urls(move |urls| {
        let _ = sender.unbounded_send(urls);
    });
    receiver
}

/// Opens what was asked for during launch. True if that opened a window, so the app needn't open an empty one.
pub fn open_pending(receiver: &mut UnboundedReceiver<Vec<String>>, cx: &mut App) -> bool {
    let before = cx.windows().len();
    while let Ok(urls) = receiver.try_recv() {
        for url in urls {
            open(&url, cx);
        }
    }
    cx.windows().len() > before
}

/// Opens whatever's asked for from now on.
pub fn keep_opening(mut receiver: UnboundedReceiver<Vec<String>>, cx: &mut App) {
    cx.spawn(async move |cx| {
        while let Some(urls) = receiver.next().await {
            cx.update(|cx| {
                for url in urls {
                    open(&url, cx);
                }
            });
        }
    })
    .detach();
}

pub fn open(url: &str, cx: &mut App) {
    if let Some(target) = link_target(url, SCHEME) {
        // A web page can send these, so each is something harmless to be asked for.
        match target.as_str() {
            "new" => {
                view::open(false, cx);
            }
            "settings" => settings::open(cx),
            _ => tracing::warn!("no such link: {url}"),
        }
        cx.activate(true);
        return;
    }
    tracing::warn!("nothing to open {url} with");
}
