//! Welcome to Patchbay: a few steps shown once, at the first launch with no client connected, and again from the Help
//! menu or Settings. What Patchbay is, bringing in your servers, connecting your tools, the keys worth knowing first
//! (with the tour), and the menu bar and Open at login.
//!
//! The window around the steps is Typecase's `welcome`, as Red Pen's and Corkboard's are; what each step says is here.
//! Its buttons run their Commands in the main window, which does what they ask.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use patchbay_core::clients::Client;
use rust_i18n::t;
use typecase::welcome::{self, KeyLine, KeyList, Step, WelcomeFrame};

use crate::gateway::Running;
use crate::shortcuts::{RunAction, written_key_for};
use crate::{commands, login_item, menu_bar, restore, settings, view};

const APP: &str = "Patchbay";

/// The keys worth knowing first: what they do, and the Commands whose keys they are.
pub const KEYS: [(&str, &[&str]); 7] = [
    (
        "The Patch Bay, Servers, Clients and Skills",
        &[
            "view.patch-bay",
            "view.servers",
            "view.clients",
            "view.skills",
        ],
    ),
    ("Add a server", &["servers.add"]),
    ("Filter the list", &["view.filter"]),
    ("Read your servers again", &["servers.reload"]),
    ("Copy the gateway's URL", &["gateway.copy-url"]),
    ("Find any command by name", &["app.palette"]),
    ("Every key", &["app.keys"]),
];

/// The Welcome window, if it's open.
struct Open(AnyWindowHandle);

impl Global for Open {}

/// Opens Welcome at a first launch: once, and only with no client connected yet, so someone who already has
/// Patchbay set up isn't shown it on updating.
pub fn open_if_new(cx: &mut App) {
    if cfg!(feature = "screenshot") || restore::welcomed(cx) {
        return;
    }
    restore::set_welcomed(cx);
    let home = cx
        .try_global::<Running>()
        .map(|running| running.client_home.clone());
    let connected =
        home.is_some_and(|home| Client::ALL.iter().any(|client| client.connected(&home)));
    if !connected {
        open(cx);
    }
}

/// Opens Welcome, or brings it forward if it's open already.
pub fn open(cx: &mut App) {
    if let Some(Open(handle)) = cx.try_global::<Open>()
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    let options = WindowOptions {
        show: !cfg!(feature = "screenshot"),
        focus: !cfg!(feature = "screenshot"),
        ..welcome::window_options(t!("Welcome to %{app}", app = APP).into_owned(), cx)
    };
    // In the Dock while it's open, as the main window is.
    crate::dock::show(cx);
    match gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| WelcomeView::new(window, cx))
    }) {
        Ok((handle, view)) => {
            cx.set_global(Open(handle));
            // A frame's keys go to the main window, so `PATCHBAY_WELCOME_STEP` opens a shot on another step.
            let step = std::env::var("PATCHBAY_WELCOME_STEP")
                .ok()
                .filter(|_| cfg!(feature = "screenshot"))
                .and_then(|step| step.parse().ok());
            if let Some(step) = step {
                let frame = view.read(cx).frame.clone();
                frame.update(cx, |frame, cx| frame.show(step, cx));
            }
        }
        Err(error) => tracing::error!("the Welcome window didn't open: {error}"),
    }
}

/// Typecase's Welcome, in a view that runs Patchbay's Commands, so ⌘Q and the rest work in its window too.
pub struct WelcomeView {
    frame: Entity<WelcomeFrame>,
    /// Hears the gateway, so servers added or a client connected show here as soon as they are.
    _watch: Task<()>,
}

impl WelcomeView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let steps = vec![
            Step::new(t!("One Gateway").into_owned(), |_, cx| about(cx)),
            Step::new(t!("Your Servers").into_owned(), |_, cx| servers(cx)),
            Step::new(t!("Your Tools").into_owned(), |_, cx| clients(cx)),
            Step::new(t!("Keys").into_owned(), |_, _| keys()),
            Step::new(t!("Always at Hand").into_owned(), |_, cx| at_hand(cx)),
        ];
        let watch = match cx.try_global::<Running>() {
            Some(running) => {
                let mut changes = running.gateway.subscribe();
                cx.spawn(async move |this, cx| {
                    while changes.changed().await.is_ok() {
                        if this.update(cx, |_, cx| cx.notify()).is_err() {
                            break;
                        }
                    }
                })
            }
            None => Task::ready(()),
        };
        WelcomeView {
            frame: cx.new(|cx| {
                WelcomeFrame::new(
                    t!("Welcome to %{app}", app = APP).into_owned(),
                    steps,
                    window,
                    cx,
                )
            }),
            _watch: watch,
        }
    }
}

impl Render for WelcomeView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .key_context("Welcome")
            .on_action(cx.listener(|_, action: &RunAction, window, cx| {
                commands::run(action.name, window, cx);
            }))
            .child(self.frame.clone())
    }
}

fn paragraph(text: std::borrow::Cow<str>) -> Div {
    div().child(text.into_owned())
}

fn quiet(text: std::borrow::Cow<str>, cx: &App) -> Div {
    div()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(text.into_owned())
}

/// A button that runs `command` in the main window.
fn run_button(id: &'static str, label: std::borrow::Cow<str>, command: &'static str) -> Button {
    Button::new(id)
        .outline()
        .label(label.into_owned())
        .on_click(move |_, _, cx| view::run_in_front(command, cx))
}

fn about(cx: &App) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let term = |name: std::borrow::Cow<str>, means: std::borrow::Cow<str>| {
        v_flex()
            .gap_0p5()
            .child(div().font_semibold().child(name.into_owned()))
            .child(div().text_sm().text_color(muted).child(means.into_owned()))
    };
    v_flex()
        .gap_4()
        .child(paragraph(t!(
            "Patchbay keeps your MCP servers in one place, behind one gateway on this Mac. Each AI tool connects \
             to it once."
        )))
        .child(
            v_flex()
                .gap_3()
                .child(term(
                    t!("Servers"),
                    t!("Kept in patchbay.toml, their keys in your Keychain. Signed in to once, for every tool."),
                ))
                .child(term(
                    t!("Clients"),
                    t!("Claude Code, Codex, Cursor and the rest, each given the servers you choose."),
                ))
                .child(term(
                    t!("Skills"),
                    t!("Kept once, linked into every tool's folder, and kept up to date."),
                ))
                .child(term(
                    t!("Teams"),
                    t!("A team's servers and skills, from a git repository, reviewed before they change."),
                )),
        )
        .into_any_element()
}

fn servers(cx: &App) -> AnyElement {
    let count = cx
        .try_global::<Running>()
        .map_or(0, |running| running.gateway.statuses().len());
    v_flex()
        .gap_4()
        .child(paragraph(t!(
            "Your tools probably have servers of their own already. Import them, and Patchbay takes their keys out \
             for the Keychain. Or add one by pasting its JSON, its command or its URL."
        )))
        .child(quiet(
            match count {
                0 => t!("No servers yet."),
                1 => t!("One server in patchbay.toml now."),
                count => t!("%{count} servers in patchbay.toml now.", count = count),
            },
            cx,
        ))
        .child(
            h_flex()
                .gap_2()
                .flex_wrap()
                .child(
                    run_button("import", t!("Import from Your Clients…"), "servers.import")
                        .primary(),
                )
                .child(run_button("add", t!("Add Server…"), "servers.add"))
                .child(run_button(
                    "subscribe",
                    t!("Subscribe to a Team…"),
                    "teams.subscribe",
                )),
        )
        .into_any_element()
}

fn clients(cx: &App) -> AnyElement {
    let theme = cx.theme();
    let (muted, good) = (theme.muted_foreground, theme.success);
    let home = cx
        .try_global::<Running>()
        .map(|running| running.client_home.clone());
    let found: Vec<(Client, bool)> = home
        .map(|home| {
            Client::ALL
                .into_iter()
                .filter(|client| client.detected(&home))
                .map(|client| (client, client.connected(&home)))
                .collect()
        })
        .unwrap_or_default();
    v_flex()
        .gap_4()
        .child(paragraph(t!(
            "Connecting a tool gives it one entry, for Patchbay's gateway, beside anything it has of its own. \
             Disconnecting takes only that entry out."
        )))
        .child(if found.is_empty() {
            quiet(t!("No AI tools found on this Mac yet."), cx).into_any_element()
        } else {
            v_flex()
                .gap_1()
                .children(found.into_iter().map(|(client, connected)| {
                    h_flex()
                        .gap_2()
                        .child(client.name())
                        .child(
                            div()
                                .text_sm()
                                .text_color(if connected { good } else { muted })
                                .child(if connected {
                                    t!("Connected").into_owned()
                                } else {
                                    t!("Not connected").into_owned()
                                }),
                        )
                }))
                .into_any_element()
        })
        .child(h_flex().child(run_button(
            "clients",
            t!("Connect in Clients…"),
            "view.clients",
        )))
        .into_any_element()
}

fn keys() -> AnyElement {
    let lines = KEYS.iter().map(|(said, names)| {
        names
            .iter()
            .filter_map(|name| written_key_for(name))
            .fold(KeyLine::new(t!(*said).into_owned()), KeyLine::keys)
    });
    v_flex()
        .gap_4()
        .child(paragraph(t!(
            "Everything is in the menus, and the menus are in the palette. These are the keys worth knowing first. \
             The tour shows where things are, and is in the Help menu after."
        )))
        .child(KeyList::new().lines(lines))
        .child(h_flex().child(
            Button::new("tour")
                .primary()
                .label(t!("Take the Tour").into_owned())
                .on_click(|_, _, cx| view::run_in_front("app.tour", cx)),
        ))
        .into_any_element()
}

fn at_hand(cx: &App) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let current = settings::current(cx);
    v_flex()
        .gap_4()
        .child(paragraph(t!(
            "The gateway keeps running with the window closed, so your tools can always reach it. Patchbay stays in \
             the menu bar, and %{keys} brings it forward from any app.",
            keys = menu_bar::shown(&current.hotkey)
        )))
        .child(
            v_flex()
                .gap_2()
                .child(
                    Switch::new("login")
                        .label(t!("Open %{app} at login", app = APP).into_owned())
                        .checked(login_item::is_on())
                        .on_click(|on, window, _| {
                            if let Err(error) = login_item::set(*on) {
                                tracing::warn!("open at login not changed: {error:#}");
                            }
                            window.refresh();
                        }),
                )
                .when(login_item::needs_approval(), |this| {
                    this.child(div().text_sm().text_color(muted).child(
                        t!("Allow it in System Settings › General › Login Items.").into_owned(),
                    ))
                })
                .child(
                    Switch::new("hide-dock")
                        .label(t!("Leave the Dock while no window is open").into_owned())
                        .checked(current.hide_dock)
                        .on_click(|on, _, cx| settings::set_hide_dock(*on, cx)),
                ),
        )
        .child(quiet(
            t!("Settings can change the shortcut. Welcome is in the Help menu, should you want it again."),
            cx,
        ))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::{KEYS, open_if_new};
    use crate::commands::COMMANDS;
    use crate::shortcuts::{key_for, written_key_for};
    use crate::{gateway, restore, settings, theme};
    use gpui_kit::{Keystroke, TestAppContext};

    fn launch(cx: &mut TestAppContext, root: &std::path::Path) {
        let _ = std::fs::remove_dir_all(root);
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join("patchbay.toml"), "").unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            theme::init(cx);
            settings::init_at(None, cx);
            restore::in_memory(cx);
            gateway::init_with(patchbay_core::Home::new(root), gateway::Secrets::Memory, cx);
        });
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("patchbay-welcome-{name}-{}", std::process::id()))
    }

    #[gpui_kit::test]
    fn welcome_opens_at_the_first_launch_and_not_again(cx: &mut TestAppContext) {
        launch(cx, &scratch("first"));
        cx.update(open_if_new);
        assert_eq!(cx.windows().len(), 1, "Welcome opened");
        assert!(cx.update(|cx| restore::welcomed(cx)));
        cx.update(|cx| cx.windows()[0].update(cx, |_, window, _| window.remove_window()))
            .unwrap();
        cx.run_until_parked();
        cx.update(open_if_new);
        assert_eq!(cx.windows().len(), 0, "not again");
    }

    #[gpui_kit::test]
    fn someone_with_a_client_connected_already_is_not_welcomed_on_updating(
        cx: &mut TestAppContext,
    ) {
        let root = scratch("updating");
        launch(cx, &root);
        let home = cx.update(|cx| cx.global::<gateway::Running>().client_home.clone());
        // Pi, given Patchbay's entry, as connecting it in an earlier version would have left it.
        let config = patchbay_core::clients::Client::Pi.config_path(&home);
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(
            &config,
            r#"{"mcpServers": {"patchbay": {"url": "http://127.0.0.1:1/mcp"}}}"#,
        )
        .unwrap();
        assert!(
            patchbay_core::clients::Client::Pi.connected(&home),
            "the setup is a connection"
        );
        cx.update(open_if_new);
        assert_eq!(cx.windows().len(), 0);
        assert!(
            cx.update(|cx| restore::welcomed(cx)),
            "nor at the launch after, if they disconnect it"
        );
    }

    #[test]
    fn every_key_welcome_shows_is_a_bound_command_gpui_can_read() {
        for (said, names) in KEYS {
            for name in names {
                assert!(
                    COMMANDS.contains(name),
                    "{name}, for {said}, isn't a Command"
                );
                assert!(key_for(name).is_some(), "{name}, for {said}, has no key");
                for stroke in written_key_for(name).unwrap().split_whitespace() {
                    assert!(
                        Keystroke::parse_with_os(stroke, gpui_kit::OperatingSystem::Mac).is_ok(),
                        "{name}'s {stroke} doesn't read as a key"
                    );
                }
            }
        }
    }
}
