# Developing Patchbay

You need rustup (`rust-toolchain.toml` names the toolchain), `just`, `uv`, and Xcode's command line tools.

| Recipe | Use |
| --- | --- |
| `just run` | Build a debug `Patchbay.app` and run it |
| `just run-as dark` | The same in a forced appearance |
| `just test` | Every test, with cargo-nextest if it's installed |
| `just inspect` | The app with the element inspector (⌥⌘I) |
| `just lint` | Licences and advisories of every dependency (cargo-deny), and spelling (typos) |
| `just review` | Accept or reject changed snapshots (cargo-insta) |
| `just check` | Formatting, clippy and every test |
| `just shot` | Save a frame without showing a window |
| `just ci` | The CI in `.ci.star`, on this machine |
| `just install-local` | A release build in `/Applications`, signed only for this Mac, swapped in only once it's copied |
| `just profile` | Record a Time Profiler trace of a release build with symbols, and open it in Instruments |
| `just icon` | Rebuild `packaging/AppIcon.icns` from `packaging/logo.png`, which is also the About box's logo |

## Settings

The Settings window (⌘,) keeps its choices in `~/Library/Application Support/tech.phin.patchbay/settings.toml`, through
`Settings` in `crates/core`, which is where a new setting goes: a field with a default, so an old file still reads.
A file this version doesn't understand gives the defaults, and saving keeps keys it doesn't know. Captures use none.
The main window's place is kept beside them, in `state.toml`, and put back unless its display has gone.

The window is [Typecase](https://git.phin.tech/sam/typecase)'s frame, a sidebar of sections, listed in `sections()` in
`crates/app/src/settings.rs`; a new setting is drawn in one of them, or a section of its own. Appearance is Typecase's
theme picker, and what it chooses reaches the file through `typecase::theme::on_change`.

## Typecase

The palette, Keys, the Settings window and the themes are Typecase's, from a tag, with its patched gpui-base from the
same tag (`Cargo.toml`). Move both tags together, and with gpui-kit when Typecase does. `.cargo/config.toml` fetches it
with the git command line, so it goes the way your git config sends it.

## Languages

Text the app shows is written in English, through `t!("…")`, and that English is the key in
`crates/app/locales/app.toml`. To add a language, give each entry a translation (`fr = "Réglages"`); tests fail if an
entry is missing one the others have, or translates text the app no longer shows. The app picks the closest
language it has to the system's, and the kit's own buttons and dialogs follow it.

## The database

`crates/core/src/database.rs` keeps the app's data in SQLite, in `data.sqlite` beside the settings, and in memory for
captures. A change to the tables is a new step at the end of `MIGRATIONS`, never an edit to a released one: each
step runs once, recorded in SQLite's `user_version`, and an older app refuses a file a newer one has upgraded.

## Login and the menu bar

Open at login, in Settings, registers the app with SMAppService, which only works for a signed copy in Applications:
`just install-local` or a release, not `just run`. macOS may ask for approval in System Settings > Login Items.

The menu bar extra (`menu_bar.rs`) shows `packaging/menubar.png`, black on clear at 36 px, which macOS tints. Its
shortcut, `ctrl+alt+super+Space` unless `hotkey` in the settings file says otherwise, brings the app forward from
any app.

## Logs and crashes

The app logs with `tracing` to stderr and to `~/Library/Logs/tech.phin.patchbay/`, a file a day for a week; Help > Show
Logs in Finder opens the folder. `PATCHBAY_LOG` says how much (`debug`, `patchbay=trace`), as
`RUST_LOG` does. A panic writes `crash-<time>.log` there with the backtrace, and the next launch offers to show it.
`PATCHBAY_LOGS` puts both somewhere else.

`packaging/app.entitlements` holds what the app may do under the hardened runtime, for `just run` and a release alike;
it starts empty. Add a key there, and its usage string to `packaging/Info.plist`, when the app needs one.

## The command line

`Patchbay.app/Contents/MacOS/Patchbay --version` answers without opening a window: `main` hands a command
line `crates/cli` knows to it first, and anything else opens the app, since macOS starts apps with arguments of its
own. Add a command to `Command` and `parse`; a release ships whatever is
in it, since it's the same binary.
## Links

`patchbay://new` opens a window and `patchbay://settings` the Settings, from a browser or
`open patchbay://new`. `opened.rs` decides what each does. A web page can send these, so a link should only
ask for something harmless.

## CI

CI is [starci](https://git.phin.tech/sam/starci): the steps are in `.ci.star`, and `.forgejo/workflows/ci.yml` only
starts `starci run .` on the Mac registered with the `macos` label, which needs rustup and starci on its `PATH`. Steps
run bare, because GPUI needs macOS and Metal. Cargo's caches are kept on that Mac between runs. CI doesn't run
`just release-check`, since app-release is private and bare steps have no credentials.

## Releasing

[app-release](https://git.phin.tech/sam/app-release) does it, from what's in `release.toml`:

```sh
just doctor              # everything a release needs outside the repo
just ship patch --dry-run
just ship patch          # bump, commit, push, notarize, publish
```

The version is the first `version` in `Cargo.toml`, with the build number under `[workspace.metadata.release]`. Both
change only through `just bump` or `just ship`, and `crates/core` reads both (`version()`, for About). `cargo-app` has
no TestFlight or development builds, and builds only for the Mac it runs on. Releases build with `--locked`, so
`Cargo.lock` is committed.

### Updates

The release puts Sparkle.framework in the app and points `SUFeedURL` at the appcast beside the zips
(`https://apps.phin.tech/<key>/appcast.xml`);
`crates/app/src/update.rs` loads it at launch, so `just run` and `just install-local` have no updates. Each entry is
signed with `sparkle_private_key` from the 1Password item, and the app trusts only `SUPublicEDKey` in
`packaging/Info.plist`. A beta (`just ship beta`) goes in the appcast's `beta` channel, which only copies with Get
beta versions on in Settings take; a beta starts with it on. Sparkle can't sign in, so the app can't have an `allow` list on the download
page.

app-release is pinned to a tag in `scripts/release/pyproject.toml`. Keep it on the newest one, as the other projects
are (`uv lock --upgrade-package app-release --project scripts/release` after changing it): a release
redraws the whole download page with the app-release it runs, so an old one puts an old page back for every app.

## The download page

A release puts the zip on [apps.phin.tech](https://apps.phin.tech), with `latest.zip` beside it, and redraws the page.
`[downloads]` in `release.toml` sets what it says: `description`, `category` (a shelf other than Mac apps), `allow`
(only these people, after signing in) and `pin` (Staff Pick, first on its shelf). `just release-tool page` redraws the
page without a release.

The page is drawn as a 1990s software shop, and Patchbay is also on the shelves of the
[Phin Tech Store](https://github.com/phin-tech/phin-tech-store) app, which reads the same catalog. Both show it as a
boxed package with a magazine ad; until it has an ad of its own, the box is plain and the ad is its description. To give
it one, in the phin-tech-store repo, using its key in the page's
[apps.json](https://apps.phin.tech/apps.json) (the app's name without spaces, the folder its zips are in):

1. Add its copy to `content/ads.json` under that key: a layout (`classic`, `photo`, `columns`, `product`), a box
   colour, a headline, sections, a tagline and fine print, and a `title` to show a name other than the key.
2. Add `<key>-box` (3:4) and `<key>-hero` prompts to `art/prompts.json` and run `tools/genart.py <key>-box <key>-hero`,
   or put your own JPEGs in `content/art/`.
3. `just publish-content`. The Phin Tech Store shows it the next time it opens; the page at the next release of any
   app, or now with `just release-tool page`.
