//! The GPUI front end. What the app does lives in `patchbay_core`.

mod about;
mod commands;
mod crash;
mod database;
mod dock;
mod gateway;
mod language;
mod logging;
mod login_item;
mod menu_bar;
mod menus;
mod opened;
mod palette;
mod restore;
#[cfg(feature = "screenshot")]
mod screenshot;
mod settings;
mod shortcuts;
mod theme;
mod update;
mod view;

use gpui_kit::assets::AllAssets;

// What the app shows in other languages, keyed by its English text: locales/app.toml.
rust_i18n::i18n!("locales", fallback = "en");

fn main() {
    // A command line it knows runs here, headless, and never opens a window.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(command) = patchbay_cli::parse(&args) {
        let status = command.run(&mut std::io::stdout()).unwrap_or(1);
        std::process::exit(status);
    }
    let _log = logging::init();
    crash::install();
    language::choose();
    // The bridge starts the app to have its gateway, without a window in front of whatever the person is doing.
    let background = args.iter().any(|arg| arg == "--background");
    let app = gpui_kit::application().with_assets(AllAssets);
    let mut asked = opened::listen(&app);
    // The gateway keeps running with no window open; the Dock icon or the menu bar brings one back.
    app.on_reopen(|cx| {
        if cx.windows().is_empty() {
            view::open(true, cx);
        }
    });
    app.run(move |cx| {
        gpui_kit::init(cx);
        theme::init(cx);
        settings::init(cx);
        restore::init(cx);
        database::init(cx);
        // Before any window, which shows it.
        gateway::init(cx);
        dock::init(cx);
        cx.bind_keys(shortcuts::bindings());
        // The element inspector (⌥⌘I), in `just inspect` builds only.
        #[cfg(feature = "devtools")]
        gpui_devtools::init(cx);
        // After the keys are bound, since the menus show them.
        cx.set_menus(menus::menus());
        // Not while capturing frames, which must leave nothing behind.
        if !cfg!(feature = "screenshot") {
            menu_bar::start(cx);
        }
        // Not while capturing frames, which must leave nothing behind and open nothing of their own.
        if !cfg!(feature = "screenshot") {
            update::start();
        }
        // What Finder or a link asked for at launch opens instead of an empty window.
        let first = if opened::open_pending(&mut asked, cx) {
            cx.windows().first().copied()
        } else if background {
            dock::start_hidden(cx);
            None
        } else {
            view::open(true, cx)
        };
        opened::keep_opening(asked, cx);
        #[cfg(not(feature = "screenshot"))]
        if !background {
            cx.activate(true);
        }
        if let Some(first) = first {
            let _ = first.update(cx, |_, window, cx| crash::tell_if_crashed(window, cx));
        }
        #[cfg(feature = "screenshot")]
        screenshot::capture(cx);
    });
}
