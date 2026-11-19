# Patchbay

Patchbay is a macOS app in Rust. `crates/core` (`patchbay-core`) holds everything that isn't UI and is tested
headless; `crates/app` is the GPUI front end. Read `CONTEXT.md` before naming anything.

## Where things are

- `crates/app/src/commands.rs`: every Command, in `COMMANDS`, and the ones any window runs. `view.rs` runs the main
  view's own and hands it the rest. `shortcuts.rs`: keys. `menus.rs`: the menu bar, which the command palette
  (`palette.rs`, ⌘K, Typecase's, shown over the main view) lists too. Tests hold every menu item and key to `COMMANDS`, and every Command to having a case in
  a `run` and a place in a menu or a key.
- `settings.rs`: the Settings window (⌘,), Typecase's frame with a section for each part, over
  `patchbay_core::Settings`, a TOML file in Application Support.
  `restore.rs` puts the main window back where it was (`state.toml`, beside the settings).
- `language.rs` and `locales/app.toml`: the language, and translations keyed by the English text.
- `crates/core/src/database.rs`: SQLite, with migrations; `crates/app/src/database.rs` opens it.
- `crates/core/src/skills/`: skills, installed into the store and lock the `skills` CLI (skills.sh) uses, so both
  manage the same ones; `docs/design.md` says how. `view/skills.rs` is the Skills place, `crates/cli/src/skills.rs`
  `patchbay skills`. Tests give it a stand-in home (`Folders::new`), never the real `~/.agents`.
- `login_item.rs`: Open at login, in Settings.
- `menu_bar.rs`: the menu bar extra and its shortcut (`hotkey` in the settings file).
- `about.rs`: About, and what Keys (⌘/), Typecase's sheet over the main view, lists: every key by its menu. `logging.rs`: `tracing`, to stderr and
  `~/Library/Logs/tech.phin.patchbay/`; `crash.rs` writes a report there on a panic and says so at the next launch.
- `opened.rs`: `patchbay:` links. Each target is something a web page may ask for, since one can.
  `theme.rs`: the look, Typecase's themes, which follow the appearance, theme and text size set there.
- `update.rs`: Sparkle, which only the released app carries; Check for Updates… and Settings ask it.
- `crates/cli`: the command line, which the app's own binary answers before any window opens.
- `DEVELOPMENT.md`: building, checking, CI, releasing.

## Done

The change is done when these pass, and your report names any you did not run:

- `just check`, always: formatting, clippy and every test.
- `just lint` for a new dependency: its licence has to be in `deny.toml`.
- `just shot`, looked at in light and dark, for anything visible.
- `just release-check` for changes to `release.toml` or `scripts/release`.

Write test names as sentences saying what the app does. Comments say why, in plain sentences.

## Seeing the GUI

The maintainer works alongside you, so GUI runs stay hidden and leave focus alone. Ask before any run that needs the
foreground.

- `just shot <name> <appearance>` saves a frame to `target/shots/<name>.png`. `PATCHBAY_KEYS` drives it first:
  keys, `click:X,Y` (or `cmd-click:X,Y`) and `wait`, separated by spaces. The frame is the newest window, so
  `PATCHBAY_KEYS="cmd-, wait"` shows Settings and `cmd-k` the palette.
- The frame is at 2x: divide its pixel coordinates by two for `click:`.
- Shots read and write no settings, so yours are safe. `PATCHBAY_SETTINGS=<file>` gives them some, and
  `PATCHBAY_WIDTH` sets the main window's width.
- Hidden frames don't show a selection highlight.
- Check layout at the window's real size. A larger frame hides clipping.

## Gotchas

- A key bound in a view's context reaches every text field inside the view too, and takes the letter before the field
  sees it: `o` bound to a Command makes `o` impossible to type in a search box there. The main view keeps its context
  only while it has focus itself; a view of your own with single-letter keys and a text field needs the same.
- `use gpui_kit::*` in a test module shadows `#[test]` and fails with a recursion limit error. Import names from `super`
  instead.
- The kit's `Input` fills whatever height it's offered, so give it a fixed height. It also binds `tab` and `escape`, so
  overriding them takes a binding on `Parent > Input`.
- `cargo test` leaves `target/debug/patchbay` stale. Run `cargo build` before trying that binary.
- Text the app shows goes through `t!("The English text")` (`rust_i18n::t`), with `%{name}` for what goes in it:
  `t!("Save the changes to “%{name}”?", name = title)`. The English is the key, and what shows until someone
  translates it. GPUI finds the Window menu by its English name, so that one stays as it is.
- Errors: `thiserror` enums in the core, where a caller may match on them; `anyhow` in the app, with `.context(…)`
  saying what was being done, shown with `{error:#}`.
- Text output, file formats and reports are tested with `insta::assert_snapshot!`; `just review` accepts a change.
- `just inspect` runs the app with the element inspector (⌥⌘I): click anything to see its layout, style and source.
- Log with `tracing::info!` and friends, not `eprintln!`, so it reaches the log file. `PATCHBAY_LOG=debug`
  shows more.
- Unsafe code is denied in the workspace (`Cargo.toml`'s lints); a module that needs the Objective-C runtime says so
  with `#![expect(unsafe_code, reason = …)]`, as `update.rs` does, and each block with a `// SAFETY:` comment.
- Colours come from Typecase's themes, chosen in Settings ▸ Appearance. Views take them from `cx.theme()`, and text
  sizes from the theme or rems, so the text size in Settings reaches them; a fixed width that holds text should grow with it too.
- A window of a new kind hands the Commands it doesn't run to `commands::run`, as Settings does, or the menus do
  nothing while it has focus.
- Anything reached with the mouse should be a Command in a menu, which puts it in the palette as well.
- macOS adds Enter Full Screen to the View menu itself, so there's no menu item for `window.fullscreen`, only ⌃⌘F.
- `gpui-kit` is pinned exactly, and its GPUI snapshot's API moves. Bump it on its own, with `rust-toolchain.toml` if it
  needs that. If the kit itself needs changing, copy the crate to `vendor/`, add it under `[patch.crates-io]`, exclude
  `vendor` from the workspace and mark each change, so it can be reapplied at the next bump.
- `release.toml`'s `[cargo]` copies one binary into the bundle, named after the package. Anything else the app needs at
  run time has to be compiled in (`include_str!`, `include_bytes!`).
- Files the app keeps (settings, state, documents) are written with `write_atomically` from the core, so a crash
  midway never leaves half a file.
- `.ci.star`'s steps get a clean environment. A step that needs a variable sets it in its `env`.
- The release tool is app-release, a separate repository pinned in `scripts/release/pyproject.toml`; change it there.

## Releases

The maintainer releases with `just ship patch|minor|major|beta`; leave releases, version bumps and pushes to them unless
asked. Every release recipe takes `--dry-run`.
