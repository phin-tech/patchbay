//! The main window, in Typecase's shell: a sidebar of places (Servers, Clients, Skills) on the window's colour, then a panel
//! listing what's in the place chosen, and a panel with the one chosen. The cards over the window (the palette and
//! Keys) are Typecase's.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{
    ActiveTheme, Disableable as _, Sizable as _, StyledExt as _,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use patchbay_core::clients::{Choice, Choices, Client, Entry, Layout};
use patchbay_core::paste::Pasted;
use patchbay_gateway::{SecretNeeded, ServerStatus, Status};
use rust_i18n::t;
use typecase::Toast;
use typecase::help::{HelpEvent, KeyHelp};
use typecase::overlay;
use typecase::palette::{CommandPalette, PaletteEvent};
use typecase::shell::{self, Footer, Lead};

use crate::commands::{self, COMMANDS};
use crate::gateway::Running;
use crate::shortcuts::{CONTEXT, RunAction};
use crate::{about, palette, restore, theme};

mod clients;
#[cfg(test)]
use clients::Given;
mod editor;
mod form;
mod servers;
mod share;
mod skills;

pub use editor::Editing;
use editor::Editor;

/// A Typecase card over the window, which holds the keyboard until it's done.
enum Card {
    Palette(Entity<CommandPalette>),
    Keys(Entity<KeyHelp>),
}

/// The places in the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Servers,
    Clients,
    Skills,
}

/// An AI client as the window shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientRow {
    pub client: Client,
    pub installed: bool,
    /// Patchbay's entries in its config, by name.
    pub entries: Vec<String>,
    pub choice: Choice,
}

impl ClientRow {
    pub fn connected(&self) -> bool {
        !self.entries.is_empty()
    }
}

/// The sidebar's width, and the list panel's, in rems, from points.
const SIDEBAR_WIDTH: f32 = 180. / 13.;
const LIST_WIDTH: f32 = 250. / 13.;

/// Opens a window on the app; with `remembered`, where the last one was when the app quit.
pub fn open(remembered: bool, cx: &mut App) -> Option<AnyWindowHandle> {
    open_with(remembered, cx, MainView::new)
}

/// Opens a window on the view `build` makes.
pub fn open_with(
    remembered: bool,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut Context<MainView>) -> MainView,
) -> Option<AnyWindowHandle> {
    // `PATCHBAY_WIDTH` narrows a capture, for checking a layout at a width of its own.
    let width = std::env::var("PATCHBAY_WIDTH")
        .ok()
        .filter(|_| cfg!(feature = "screenshot"))
        .and_then(|width| width.parse().ok())
        .unwrap_or(1000.);
    let centered = WindowBounds::Windowed(Bounds::centered(None, size(px(width), px(660.)), cx));
    let restored = remembered && !cfg!(feature = "screenshot");
    let bounds = restored.then(|| restore::bounds(cx)).flatten();
    let options = WindowOptions {
        window_bounds: Some(bounds.unwrap_or(centered)),
        window_min_size: Some(size(px(760.), px(420.))),
        titlebar: Some(TitlebarOptions {
            title: Some("Patchbay".into()),
            ..gpui_kit::component::TitleBar::title_bar_options()
        }),
        // Captures run hidden, so they never take over the screen.
        show: !cfg!(feature = "screenshot"),
        focus: !cfg!(feature = "screenshot"),
        ..gpui_kit::component::TitleBar::window_options()
    };
    // Back in the Dock, if it left with the last window.
    crate::dock::show(cx);
    match gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| build(window, cx))) {
        Ok((handle, _view)) => Some(handle),
        Err(error) => {
            tracing::error!("no window: {error}");
            None
        }
    }
}

pub struct MainView {
    focus: FocusHandle,
    card: Option<Card>,
    pub place: Place,
    pub servers: Vec<ServerStatus>,
    pub clients: Vec<ClientRow>,
    /// The server or client chosen in the list.
    pub server: Option<String>,
    pub client: Option<Client>,
    /// A server being added or changed, shown in place of the chosen one.
    pub editor: Option<Editor>,
    /// Servers being chosen to export, shown in place of the chosen one.
    pub exporting: Option<share::Exporting>,
    /// The skills on this Mac, the one chosen, and any being added.
    pub skills: skills::SkillsState,
    /// The manifests' profiles, `default` first.
    profiles: Vec<String>,
    /// Each profile's servers.
    pub profile_servers: BTreeMap<String, Vec<String>>,
    problems: Vec<String>,
    /// A secret field for each Keychain account a server is waiting for.
    secrets: BTreeMap<String, Entity<InputState>>,
    /// Servers whose sign-in is open in the browser.
    signing_in: BTreeSet<String>,
    /// Servers whose secrets are shown to be replaced, as a wrong or expired key is.
    replacing: BTreeSet<String>,
    /// A field at the top of each place's list, narrowing it to what matches.
    filters: Filters,
    _fields: Vec<Subscription>,
    _filters: Vec<Subscription>,
    _watch: Task<()>,
    _appearance: Subscription,
    _bounds: Subscription,
}

impl MainView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let appearance = theme::follow_system(window, cx);
        let bounds =
            cx.observe_window_bounds(window, |_, window, cx| restore::remember(window, cx));
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        // The gateway says when anything changes; the window reads it again then.
        let watch = match cx.try_global::<Running>() {
            Some(running) => {
                let mut changes = running.gateway.subscribe();
                cx.spawn_in(window, async move |this, cx| {
                    while changes.changed().await.is_ok() {
                        if this
                            .update_in(cx, |this, window, cx| this.sync(window, cx))
                            .is_err()
                        {
                            break;
                        }
                    }
                })
            }
            None => Task::ready(()),
        };
        let filter = |window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("Filter").into_owned()))
        };
        let filters = Filters {
            servers: filter(window, cx),
            clients: filter(window, cx),
            skills: filter(window, cx),
        };
        // The list follows what's typed; Return gives the keyboard back to the window and its keys.
        let follow = |field: &Entity<InputState>, window: &mut Window, cx: &mut Context<Self>| {
            cx.subscribe_in(
                field,
                window,
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => cx.notify(),
                    InputEvent::PressEnter { .. } => this.focus.focus(window, cx),
                    _ => {}
                },
            )
        };
        let filter_subscriptions = vec![
            follow(&filters.servers, window, cx),
            follow(&filters.clients, window, cx),
            follow(&filters.skills, window, cx),
        ];
        let mut view = Self {
            focus,
            card: None,
            place: Place::Servers,
            servers: Vec::new(),
            clients: Vec::new(),
            server: None,
            client: None,
            editor: None,
            exporting: None,
            skills: skills::SkillsState::default(),
            profiles: Vec::new(),
            profile_servers: BTreeMap::new(),
            problems: Vec::new(),
            secrets: BTreeMap::new(),
            signing_in: BTreeSet::new(),
            replacing: BTreeSet::new(),
            filters,
            _fields: Vec::new(),
            _filters: filter_subscriptions,
            _watch: watch,
            _appearance: appearance,
            _bounds: bounds,
        };
        view.sync(window, cx);
        view
    }

    /// Reads the gateway and the clients' configs again: what's chosen stays chosen while it's there, a client
    /// given an entry for each server follows the servers, and each secret newly wanted gets a field.
    pub fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(running) = cx.try_global::<Running>() else {
            return;
        };
        self.servers = running.gateway.statuses();
        self.problems = running.gateway.problems();
        self.profile_servers = running.gateway.profiles();
        let mut profiles: Vec<String> = self.profile_servers.keys().cloned().collect();
        profiles.sort_by_key(|profile| (profile != "default", profile.clone()));
        self.profiles = profiles;
        if self
            .server
            .as_ref()
            .is_none_or(|name| !self.servers.iter().any(|server| &server.name == name))
        {
            self.server = self.servers.first().map(|server| server.name.clone());
        }
        self.sync_clients(cx);
        if self.client.is_none() {
            self.client = self
                .clients
                .iter()
                .find(|row| row.installed)
                .map(|row| row.client);
        }
        self.sync_secret_fields(window, cx);
        self.sync_skills(cx);
        cx.notify();
    }

    /// Does what a menu item or a shortcut named: the Commands that are this view's own, then the rest.
    fn run(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        match name {
            "servers.add" => self.start_editing(Editing::New, window, cx),
            "servers.add-team" => self.start_editing(Editing::Team, window, cx),
            "servers.export" => self.start_export(window, cx),
            "servers.remove-team" => self.remove_chosen_team(window, cx),
            "servers.reload" => self.reload(window, cx),
            "servers.edit" => edit_manifest(cx),
            "gateway.copy-url" => self.copy_url(window, cx),
            "view.servers" => self.show(Place::Servers, cx),
            "view.clients" => self.show(Place::Clients, cx),
            "view.skills" => self.show(Place::Skills, cx),
            "view.filter" => self.focus_filter(window, cx),
            "skills.take-over" => self.take_over_skills(skills::Which::Chosen, window, cx),
            "skills.take-over-all" => self.take_over_skills(skills::Which::Every, window, cx),
            "skills.add" => self.start_adding_skills(window, cx),
            "skills.check" => self.check_skills(
                Vec::new(),
                patchbay_core::skills::Checking::Report,
                window,
                cx,
            ),
            "skills.update" => self.check_skills(
                Vec::new(),
                patchbay_core::skills::Checking::Apply,
                window,
                cx,
            ),
            "skills.remove" => self.remove_chosen_skill(window, cx),
            "app.palette" => self.open_palette(window, cx),
            "app.keys" => self.open_keys(window, cx),
            _ => {
                let ran = commands::run(name, window, cx);
                debug_assert!(ran || !COMMANDS.contains(&name), "{name} has no case here");
            }
        }
    }

    pub fn show(&mut self, place: Place, cx: &mut Context<Self>) {
        self.place = place;
        cx.notify();
    }

    fn reload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        reload_gateway(cx);
        Toast::new(t!("Read your servers again"))
            .key("reload")
            .show(window, cx);
    }

    fn copy_url(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(url) = cx.try_global::<Running>().and_then(Running::url) else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(url));
        Toast::new(t!("Copied the gateway's URL"))
            .key("copy")
            .show(window, cx);
    }

    /// Shows the palette over the window. What's chosen runs here, as if from its menu, once the palette has gone
    /// and given focus back.
    fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let palette = cx.new(|cx| CommandPalette::new(&t!("run"), palette::commands(), window, cx));
        cx.subscribe_in(
            &palette,
            window,
            |this, _, event: &PaletteEvent, window, cx| match event {
                PaletteEvent::Run(name) => {
                    this.close_card(cx);
                    this.run(&name.clone(), window, cx);
                }
                PaletteEvent::Dismissed => this.close_card(cx),
                _ => {}
            },
        )
        .detach();
        self.card = Some(Card::Palette(palette));
        cx.notify();
    }

    /// Shows Keys over the window: every key, by its menu, narrowed by typing.
    fn open_keys(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let keys = cx.new(|cx| KeyHelp::new(&t!("Keys"), about::key_sections(), window, cx));
        cx.subscribe_in(&keys, window, |this, _, _: &HelpEvent, _, cx| {
            this.close_card(cx)
        })
        .detach();
        self.card = Some(Card::Keys(keys));
        cx.notify();
    }

    fn close_card(&mut self, cx: &mut Context<Self>) {
        self.card = None;
        cx.notify();
    }

    fn sidebar(&self, cx: &Context<Self>) -> Div {
        let running = cx.try_global::<Running>();
        let connected = self.clients.iter().filter(|row| row.connected()).count();
        let waiting = self
            .servers
            .iter()
            .filter(|server| {
                matches!(
                    server.status,
                    Status::NeedsSecret { .. } | Status::NeedsSignIn | Status::Failed { .. }
                )
            })
            .count();
        let place = |place: Place, label: String, count: usize, cx: &Context<Self>| {
            shell::sidebar_row(self.place == place, cx)
                .id(SharedString::from(format!("place-{place:?}")))
                .child(label)
                .child(shell::count(count, cx))
                .on_click(cx.listener(move |this, _, _, cx| this.show(place, cx)))
        };
        let mut footer = Footer::new();
        match running.map(|running| (&running.listening, running.url())) {
            Some((Ok(_), Some(url))) => footer = footer.line(url),
            Some((Err(error), _)) => {
                footer = footer.error(t!("Not listening: %{error}", error = error).into_owned());
            }
            _ => footer = footer.line(t!("Starting…").into_owned()),
        }
        if waiting > 0 {
            footer = footer.line(t!("%{count} need you", count = waiting).into_owned());
        }
        v_flex()
            .flex_none()
            .w(rems(SIDEBAR_WIDTH))
            .h_full()
            .child(
                v_flex()
                    .flex_1()
                    .gap_0p5()
                    .px_2()
                    .pt_1()
                    .child(place(
                        Place::Servers,
                        t!("Servers").into_owned(),
                        self.servers.len(),
                        cx,
                    ))
                    .child(place(
                        Place::Clients,
                        t!("Clients").into_owned(),
                        connected,
                        cx,
                    ))
                    .child(place(
                        Place::Skills,
                        t!("Skills").into_owned(),
                        self.skills.list.len(),
                        cx,
                    )),
            )
            .child(
                h_flex().px_2().pb_1().child(
                    Button::new("copy-url")
                        .ghost()
                        .xsmall()
                        .label(t!("Copy Gateway URL").into_owned())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.run("gateway.copy-url", window, cx);
                        })),
                ),
            )
            .child(footer)
    }

    /// A panel's heading row: its title, and what it offers on the right.
    fn panel_header(&self, title: impl Into<SharedString>, cx: &Context<Self>) -> Div {
        h_flex()
            .flex_none()
            .justify_between()
            .gap_2()
            .px_3()
            .h(rems(36. / 13.))
            .border_b_1()
            .border_color(cx.theme().border)
            .child(div().font_semibold().child(title.into()))
    }

    /// A panel's heading with something beside its title, such as the state of what it shows.
    fn panel_header_beside(
        &self,
        title: impl Into<SharedString>,
        beside: impl IntoElement,
        cx: &Context<Self>,
    ) -> Div {
        h_flex()
            .flex_none()
            .justify_between()
            .gap_2()
            .px_3()
            .h(rems(36. / 13.))
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .gap_2()
                    .min_w_0()
                    .child(div().font_semibold().truncate().child(title.into()))
                    .child(beside),
            )
    }
}

/// The width of a detail panel's labels, in rems: 84 points.
const LABEL_WIDTH: f32 = 84. / 13.;

/// A row of a detail panel, as a form has them: a short label on the left, what it's about on the right.
fn field(label: impl Into<SharedString>, content: impl IntoElement, cx: &App) -> Div {
    h_flex()
        .items_start()
        .gap_4()
        .child(
            div()
                .flex_none()
                .w(rems(LABEL_WIDTH))
                .pt(rems(2. / 13.))
                .text_base()
                .text_color(cx.theme().muted_foreground)
                .child(label.into()),
        )
        .child(div().flex_1().min_w_0().child(content))
}

/// The filter field over each place's list.
struct Filters {
    servers: Entity<InputState>,
    clients: Entity<InputState>,
    skills: Entity<InputState>,
}

impl MainView {
    fn filter_for(&self, place: Place) -> &Entity<InputState> {
        match place {
            Place::Servers => &self.filters.servers,
            Place::Clients => &self.filters.clients,
            Place::Skills => &self.filters.skills,
        }
    }

    /// What's typed in `place`'s filter.
    fn filter_text(&self, place: Place, cx: &App) -> String {
        self.filter_for(place).read(cx).value().trim().to_string()
    }

    /// The filter field, under a list's heading.
    fn filter_field(&self, place: Place, cx: &App) -> Div {
        div()
            .flex_none()
            .px_2()
            .py_1p5()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Input::new(self.filter_for(place))
                    .small()
                    .h(rems(24. / 13.)),
            )
    }

    /// Puts the keyboard in the filter of the place shown.
    fn focus_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.filter_for(self.place)
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx);
    }
}

/// Whether every word of `query` is in one of `fields`, ignoring case: `aws cdk` matches `aws-cdk`.
fn matches_filter(query: &str, fields: &[&str]) -> bool {
    let fields: Vec<String> = fields.iter().map(|field| field.to_lowercase()).collect();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|word| fields.iter().any(|field| field.contains(word)))
}

/// A line saying what a choice means, small and quiet under it.
fn hint(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

impl Render for MainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (list, detail) = match self.place {
            Place::Servers => (self.server_list(cx), self.server_detail(cx)),
            Place::Clients => (self.client_list(cx), self.client_detail(cx)),
            Place::Skills => (self.skill_list(cx), self.skill_detail(cx)),
        };
        shell::window(cx)
            .relative()
            .track_focus(&self.focus)
            // The view's own keys only while it has focus itself: a binding here would otherwise take its letter
            // before a text field inside the view sees it.
            .key_context(if self.focus.is_focused(window) {
                CONTEXT
            } else {
                "Typing"
            })
            .on_action(cx.listener(|this, action: &RunAction, window, cx| {
                this.run(action.name, window, cx);
            }))
            .child(shell::title_bar(cx).child("Patchbay"))
            .child(
                shell::columns(Lead::Sidebar)
                    .child(self.sidebar(cx))
                    .child(
                        shell::panel(false, cx)
                            .flex_none()
                            .w(rems(LIST_WIDTH))
                            .child(list),
                    )
                    .child(shell::panel(false, cx).flex_1().child(detail)),
            )
            // Last, so it's over everything; a click outside it closes it.
            .when_some(self.card.as_ref(), |this, card| {
                let card = match card {
                    Card::Palette(palette) => palette.clone().into_any_element(),
                    Card::Keys(keys) => keys.clone().into_any_element(),
                };
                this.child(
                    overlay(card).on_dismiss(cx.listener(|this, _, _, cx| this.close_card(cx))),
                )
            })
    }
}

/// A row of a list panel: the chosen one in the list's selection colour.
fn list_row(chosen: bool, cx: &App) -> Div {
    let theme = cx.theme();
    let hover = theme.list_hover;
    h_flex()
        .gap_2()
        .px_3()
        .py_2()
        .cursor_pointer()
        .when(chosen, |row| row.bg(theme.list_active))
        .when(!chosen, |row| row.hover(move |row| row.bg(hover)))
}

/// Something in a detail panel: a small heading and what's under it.
fn section(title: impl Into<SharedString>, cx: &App) -> Div {
    v_flex().gap_2().child(
        div()
            .text_sm()
            .font_semibold()
            .text_color(cx.theme().muted_foreground)
            .child(title.into()),
    )
}

/// Asks the gateway to read the manifests again and start what it can.
fn reload_gateway(cx: &App) {
    let Some(running) = cx
        .try_global::<Running>()
        .filter(|running| running.background)
    else {
        return;
    };
    let gateway = running.gateway.clone();
    running.spawn(async move {
        gateway.reload().await;
        gateway.start_all().await;
    });
}

/// Opens the person's manifest in their text editor.
fn edit_manifest(cx: &App) {
    let path = cx
        .try_global::<Running>()
        .map(|running| running.gateway.home().manifest());
    if let Some(path) = path {
        let _ = std::process::Command::new("open")
            .arg("-t")
            .arg(path)
            .status();
    }
}

/// The home folder whose clients the window shows: see [`Running::client_home`].
fn client_home(cx: &App) -> PathBuf {
    cx.try_global::<Running>()
        .map_or_else(|| PathBuf::from("."), |running| running.client_home.clone())
}

#[cfg(test)]
mod tests;
