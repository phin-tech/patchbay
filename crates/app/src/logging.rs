//! Logging with `tracing`: to stderr, and to a file a day in `~/Library/Logs/tech.phin.patchbay/`, kept for a week, since a
//! Mac app's stderr goes nowhere anyone looks. GPUI's own messages, which use the `log` crate, come too.
//! `PATCHBAY_LOG` says what's kept, as `RUST_LOG` would: `debug`, `patchbay=trace`.

use std::io::IsTerminal as _;
use std::path::PathBuf;

use gpui_kit::App;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

/// Where logs and crash reports go. Captures write none, and `PATCHBAY_LOGS` puts them somewhere else.
pub fn folder() -> Option<PathBuf> {
    if let Some(folder) = std::env::var_os("PATCHBAY_LOGS") {
        return Some(folder.into());
    }
    if cfg!(feature = "screenshot") {
        return None;
    }
    let home = PathBuf::from(std::env::var_os("HOME")?);
    Some(home.join("Library/Logs/tech.phin.patchbay"))
}

/// Starts logging. Keep what it returns for as long as the app runs: the file stops being written when it's dropped.
pub fn init() -> Option<WorkerGuard> {
    let filter = EnvFilter::try_from_env("PATCHBAY_LOG")
        .or_else(|_| EnvFilter::try_from_default_env())
        .unwrap_or_else(|_| EnvFilter::new("warn,patchbay=info,patchbay_core=info"));
    let stderr = fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal());
    let appender = folder().and_then(|folder| {
        Builder::new()
            .rotation(Rotation::DAILY)
            .filename_prefix("patchbay")
            .filename_suffix("log")
            .max_log_files(7)
            .build(folder)
            .ok()
    });
    let (file, guard) = match appender {
        Some(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            (
                Some(fmt::layer().with_writer(writer).with_ansi(false)),
                Some(guard),
            )
        }
        None => (None, None),
    };
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(stderr)
        .with(file)
        .try_init();
    guard
}

/// Shows the logs folder in Finder, as Help > Show Logs in Finder does.
pub fn reveal(cx: &mut App) {
    if let Some(folder) = folder() {
        let _ = std::fs::create_dir_all(&folder);
        cx.reveal_path(&folder);
    }
}
