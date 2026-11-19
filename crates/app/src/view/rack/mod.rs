//! The patch bay: a rack, as audio hardware has, with a module for each client on the left and each server on the
//! right, and a Cable from a client's jack to a server's for each server the client is given. Clicking a jack plugs a
//! Cable in or pulls it out, which gives or takes that server, as ticking it under the client's Servers does. Each
//! client's module has its power (connected or not) and its layout, and each label opens it in its own place.
//!
//! Skills hang under the servers. A Cable to a skill is a link to it in the client's skills folder, as ticking the
//! client under the skill does in Skills. Where that can't change (a client that reads the store itself has every
//! skill, and a client with its own copy keeps it), the Cable is hard-wired: grey, and fixed in place. A skill of
//! someone's own, which Patchbay doesn't look after, is labelled local and has no jacks.

mod cables;
mod drag;
mod layout;

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::{ActiveTheme as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use patchbay_core::clients::{Choice, Client, Layout as Shape};
use patchbay_core::skills::{Folders, Reach, Skill};
use patchbay_core::strand::Point as Spot;
use patchbay_gateway::{ServerStatus, Status};
use rust_i18n::t;

use cables::{Cables, Lead, Motion};
use drag::{Drag, Outcome, Pair, Port, Side};
use layout::{Column, Module, Rect};

/// The patch bay's own state: its Cables, the layout they were last hung on, and any Cable being dragged.
#[derive(Default)]
pub struct Rack {
    cables: Cables,
    layout: layout::Layout,
    clients: Vec<Client>,
    /// The skills' folders, in the rack's order.
    skills: Vec<String>,
    /// Where the rack's top left was last drawn in the window, which a pointer's place is read against.
    origin: Rc<Cell<Point<Pixels>>>,
    drag: Option<Drag>,
}

impl Rack {
    /// Drops any Cable being dragged where it was, as when the window shows another place.
    pub fn stop_dragging(&mut self) {
        self.drag = None;
    }

    fn spot(&self, at: Point<Pixels>) -> Spot {
        let origin = self.origin.get();
        Spot::new(f32::from(at.x - origin.x), f32::from(at.y - origin.y))
    }

    /// The jack on `side` for `pair`, on the rack as last laid out.
    fn jack(&self, side: Side, pair: &Pair, servers: &[ServerStatus]) -> Option<Spot> {
        let client = self.clients.iter().position(|client| *client == pair.0)?;
        match (&pair.1, side) {
            (Port::Server(name), side) => {
                let server = servers.iter().position(|server| &server.name == name)?;
                match side {
                    Side::Client => self.layout.clients.get(client)?.jacks.get(server).copied(),
                    Side::Server => self.layout.servers.get(server)?.jacks.get(client).copied(),
                }
            }
            (Port::Skill(_), Side::Client) => self.layout.skills_jack(client),
            (Port::Skill(folder), Side::Server) => {
                let skill = self.skills.iter().position(|each| each == folder)?;
                self.layout.skills.get(skill)?.jacks.get(client).copied()
            }
        }
    }

    /// The module the loose end of the Cable being dragged is over, as the Cable it would make there.
    fn over(&self, drag: &Drag, servers: &[ServerStatus]) -> Option<(Pair, Rect)> {
        let (client, port, rect) = if drag.target() == Side::Client {
            let at = self.layout.module_at(Column::Clients, drag.at)?;
            (
                Some(*self.clients.get(at)?),
                None,
                self.layout.clients[at].rect,
            )
        } else if let Some(at) = self.layout.module_at(Column::Servers, drag.at) {
            let name = servers.get(at)?.name.clone();
            (None, Some(Port::Server(name)), self.layout.servers[at].rect)
        } else {
            let at = self.layout.module_at(Column::Skills, drag.at)?;
            let folder = self.skills.get(at)?.clone();
            (None, Some(Port::Skill(folder)), self.layout.skills[at].rect)
        };
        Some((drag.over(client, port)?, rect))
    }
}

use super::clients::Given;
use super::{ClientRow, MainView, Place, SIDEBAR_WIDTH};
use crate::gateway::Running;

/// The hardware's colours, which stay the rack's whatever the theme, in a light and a dark finish: brushed aluminium,
/// or anodised black. The lights take the theme's own colours for good, waiting and failed.
#[derive(Clone, Copy)]
struct Look {
    cavity: Hsla,
    rail: Hsla,
    rail_edge: Hsla,
    screw: Hsla,
    slot: Hsla,
    panel_top: Hsla,
    panel_bottom: Hsla,
    panel_edge: Hsla,
    print: Hsla,
    print_muted: Hsla,
    tape: Hsla,
    tape_text: Hsla,
    nut: Hsla,
    nut_edge: Hsla,
    hole: Hsla,
    button: Hsla,
    shadow: Hsla,
    led_off: Hsla,
    good: Hsla,
    waiting: Hsla,
    failed: Hsla,
}

fn color(hex: u32) -> Hsla {
    rgb(hex).into()
}

impl Look {
    fn of(cx: &App) -> Look {
        let theme = cx.theme();
        let (good, waiting, failed) = (theme.success, theme.warning, theme.danger);
        if theme.mode.is_dark() {
            Look {
                cavity: color(0x0e0f11),
                rail: color(0x1d1e21),
                rail_edge: color(0x34363b),
                screw: color(0x80848a),
                slot: color(0x2a2c30),
                panel_top: color(0x3b3e44),
                panel_bottom: color(0x2a2c31),
                panel_edge: color(0x111214),
                print: color(0xd9dbe0),
                print_muted: color(0x8e939b),
                tape: color(0xe2d7b0),
                tape_text: color(0x2b2616),
                nut: color(0x9a9ea4),
                nut_edge: color(0x3a3d42),
                hole: color(0x050506),
                button: color(0x50545b),
                shadow: rgba(0x0000_0080).into(),
                led_off: color(0x24262a),
                good,
                waiting,
                failed,
            }
        } else {
            Look {
                cavity: color(0x1c1d20),
                rail: color(0x2c2e32),
                rail_edge: color(0x484b51),
                screw: color(0xb0b4ba),
                slot: color(0x4b4f55),
                panel_top: color(0xdfe1e4),
                panel_bottom: color(0xbcc0c5),
                panel_edge: color(0x7d8187),
                print: color(0x26282c),
                print_muted: color(0x5c6067),
                tape: color(0xf3e9c6),
                tape_text: color(0x2b2616),
                nut: color(0xc9ccd1),
                nut_edge: color(0x6c7077),
                hole: color(0x0d0e10),
                button: color(0xeceef0),
                shadow: rgba(0x0000_0059).into(),
                led_off: color(0x6f7378),
                good,
                waiting,
                failed,
            }
        }
    }
}

/// A Cable's colour, one for each client, as a studio colours its leads by where they go.
fn lead_color(client: Client) -> Hsla {
    const LEADS: [u32; 10] = [
        0xd2_3c2c, 0xe0_7a1f, 0x2f_9e44, 0xe8_c02a, 0x2f_6fd6, 0x8a_4fd1, 0x1a_a3a3, 0xd6_487e,
        0xe6_e1d6, 0x45_494f,
    ];
    let at = Client::ALL
        .iter()
        .position(|each| *each == client)
        .unwrap_or(0);
    color(LEADS[at % LEADS.len()])
}

/// A hard-wired Cable's colour: plain grey, so it reads as part of the rack rather than a choice.
fn wired_color() -> Hsla {
    hsla(0., 0., 0.55, 1.)
}

/// Whether `skill` is one Patchbay looks after, so it can be linked to a client, rather than one of someone's own.
fn linkable(skill: &Skill) -> bool {
    skill.managed() && skill.stored
}

/// Why `client`'s link to `skill` can't change, if it can't.
fn wired(skill: &Skill, client: Client, folders: &Folders) -> Option<String> {
    let reach = skill
        .clients
        .iter()
        .find(|(each, _)| *each == client)
        .map(|(_, reach)| *reach);
    if folders.reads_store(client) {
        Some(
            t!(
                "%{client} reads ~/.agents/skills itself, so it has every skill there.",
                client = client.name()
            )
            .into_owned(),
        )
    } else if reach == Some(Reach::Own) {
        Some(
            t!(
                "%{client} has its own copy of %{skill}, so it's left as it is.",
                client = client.name(),
                skill = skill.name.clone()
            )
            .into_owned(),
        )
    } else {
        None
    }
}

/// A Cable's key among the rack's: whose it is and where it goes.
fn key((client, port): &Pair) -> String {
    match port {
        Port::Server(name) => format!("{}/{name}", client.slug()),
        Port::Skill(folder) => format!("{}/skill:{folder}", client.slug()),
    }
}

/// A client's letter on the rack, `A` for the first shown, which its jack on each server's module carries too.
fn letter(at: usize) -> String {
    char::from(b'A' + (at % 26) as u8).to_string()
}

/// A point on the rack, from `module`'s top left, where what's inside the module is placed.
fn local_spot(module: &Module, spot: Spot) -> Spot {
    Spot::new(spot.x - module.rect.x, spot.y - module.rect.y)
}

fn place(rect: Rect) -> Div {
    div()
        .absolute()
        .left(px(rect.x))
        .top(px(rect.y))
        .w(px(rect.width))
        .h(px(rect.height))
}

impl MainView {
    /// The clients the rack has a module for: those on this Mac, and any still connected though it's gone.
    pub(super) fn rack_clients(&self) -> Vec<&ClientRow> {
        self.clients
            .iter()
            .filter(|row| row.installed || row.connected())
            .collect()
    }

    /// The Cables plugged in now, as client and server, by their places on the rack.
    fn patched(&self, clients: &[&ClientRow]) -> Vec<(usize, usize)> {
        clients
            .iter()
            .enumerate()
            .flat_map(|(at, row)| {
                let given = self.given(row);
                self.servers
                    .iter()
                    .enumerate()
                    .filter(move |(_, server)| given.contains(&server.name))
                    .map(move |(server, _)| (at, server))
            })
            .collect()
    }

    /// The skills on the rack, in its order: those Patchbay looks after first, then any of someone's own.
    fn rack_skills(&self) -> Vec<Skill> {
        let mut skills = self.skills.list.clone();
        skills.sort_by_key(|skill| (!linkable(skill), skill.name.clone()));
        skills
    }

    fn plugged(&self, (client, port): &Pair) -> bool {
        match port {
            Port::Server(server) => self
                .clients
                .iter()
                .find(|row| row.client == *client)
                .is_some_and(|row| self.given(row).contains(server)),
            Port::Skill(folder) => self.skills.list.iter().any(|skill| {
                &skill.folder == folder && skill.clients.iter().any(|(each, _)| each == client)
            }),
        }
    }

    /// Plugs in (true) or pulls out each Cable in `changes`, all the first one's client's. A server's are one change,
    /// so a Cable moved between two of its servers is one: the client is given only the servers left plugged in,
    /// kept as its own profile. A skill's links it into the client's skills folder, or takes the link out.
    fn set_plugged(
        &mut self,
        changes: &[(&Pair, bool)],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(client) = changes.first().map(|(pair, _)| pair.0) else {
            return;
        };
        for ((_, port), on) in changes {
            if let Port::Skill(folder) = port {
                self.give_skill(folder.clone(), client, *on, window, cx);
            }
        }
        if !changes
            .iter()
            .any(|((_, port), _)| matches!(port, Port::Server(_)))
        {
            return;
        }
        let Some(row) = self.clients.iter().find(|row| row.client == client) else {
            return;
        };
        let mut servers = self.given(row);
        for ((_, port), on) in changes {
            if let Port::Server(server) = port {
                servers.retain(|given| given != server);
                if *on {
                    servers.push(server.clone());
                }
            }
        }
        self.give(client, Given::Only(servers), window, cx);
    }

    /// Plugs `pair`'s Cable in, or pulls it out if it's there.
    pub(super) fn patch(&mut self, pair: &Pair, window: &mut Window, cx: &mut Context<Self>) {
        let on = !self.plugged(pair);
        self.set_plugged(&[(pair, on)], window, cx);
    }

    /// A press on a jack: the start of a drag, or of a click. A hard-wired one says why it can't move instead.
    fn press(
        &mut self,
        pair: Pair,
        side: Side,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(why) = self.wired_to(&pair, cx) {
            typecase::Toast::new(why).show(window, cx);
            return;
        }
        // A client's skills all hang from its one skills jack, so a press there starts a new Cable rather than
        // picking one of them up.
        let bundle = side == Side::Client && matches!(pair.1, Port::Skill(_));
        let plugged = !bundle && self.plugged(&pair);
        let at = self.rack.spot(at);
        self.rack.drag = Some(Drag::new(pair, side, plugged, at));
        cx.notify();
    }

    /// Why `pair`'s Cable is hard-wired, if it is one to a skill whose link can't change.
    fn wired_to(&self, pair: &Pair, cx: &App) -> Option<String> {
        let Port::Skill(folder) = &pair.1 else {
            return None;
        };
        let skill = self
            .skills
            .list
            .iter()
            .find(|skill| &skill.folder == folder)?;
        wired(skill, pair.0, &super::skills::skill_folders(cx))
    }

    fn drag_to(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let at = self.rack.spot(at);
        if let Some(drag) = &mut self.rack.drag {
            drag.move_to(at);
            cx.notify();
        }
    }

    /// Lets go of the Cable being dragged, which does what it was dragged to do.
    fn let_go(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let at = self.rack.spot(at);
        let Some(mut drag) = self.rack.drag.take() else {
            return;
        };
        drag.move_to(at);
        let over = self.rack.over(&drag, &self.servers).map(|(pair, _)| pair);
        let outcome = drag.outcome(over, |pair| self.plugged(pair));
        let landing = match &outcome {
            Outcome::Plug(pair) | Outcome::Move { to: pair, .. } => self.wired_to(pair, cx),
            _ => None,
        };
        if let Some(why) = landing {
            typecase::Toast::new(why).show(window, cx);
            cx.notify();
            return;
        }
        match outcome {
            Outcome::Nothing => {}
            // A click on a client's skills jack names no skill, so it does nothing.
            Outcome::Toggle((_, Port::Skill(folder))) if folder.is_empty() => {}
            Outcome::Toggle(pair) => self.patch(&pair, window, cx),
            Outcome::Plug(pair) => self.set_plugged(&[(&pair, true)], window, cx),
            Outcome::Unplug(pair) => self.set_plugged(&[(&pair, false)], window, cx),
            Outcome::Move { from, to } if from.0 == to.0 => {
                self.set_plugged(&[(&from, false), (&to, true)], window, cx);
            }
            Outcome::Move { from, to } => {
                self.set_plugged(&[(&from, false)], window, cx);
                self.set_plugged(&[(&to, true)], window, cx);
            }
        }
        cx.notify();
    }

    /// The patch bay, filling the space beside the sidebar.
    pub(super) fn patch_bay(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let rem = f32::from(window.rem_size());
        // The panel's width, from the window's, since a layout can't wait for the panel to be measured.
        let width = f32::from(window.viewport_size().width) - (SIDEBAR_WIDTH + 1.5) * rem;
        let rows: Vec<ClientRow> = self.rack_clients().into_iter().cloned().collect();
        let shown: Vec<&ClientRow> = rows.iter().collect();
        // As tall as the panel, from the window's height less its title bar, so the rails run to its foot.
        let height = f32::from(window.viewport_size().height) - 3. * rem;
        let skills = self.rack_skills();
        let rack = layout::layout(
            shown.len(),
            self.servers.len(),
            skills.len(),
            width,
            height,
            rem,
        );
        self.rack.layout = rack.clone();
        self.rack.clients = shown.iter().map(|row| row.client).collect();
        self.rack.skills = skills.iter().map(|skill| skill.folder.clone()).collect();
        let folders = super::skills::skill_folders(cx);
        let patched = self.patched(&shown);
        let drag = self.rack.drag.clone();
        let held = drag.as_ref().and_then(|drag| drag.held.clone());
        let mut leads: Vec<Lead> = patched
            .iter()
            .filter_map(|&(client, server)| {
                let pair = (
                    shown[client].client,
                    Port::Server(self.servers[server].name.clone()),
                );
                // A Cable picked up hangs from the pointer instead, below.
                if held.as_ref() == Some(&pair) {
                    return None;
                }
                let (from, to) = rack.cable(client, server)?;
                Some(Lead {
                    key: key(&pair),
                    color: lead_color(pair.0),
                    from,
                    to,
                })
            })
            .collect();
        for (client_at, row) in shown.iter().enumerate() {
            if folders.client(row.client).is_none() {
                continue;
            }
            for (skill_at, skill) in skills.iter().enumerate() {
                let has = skill.clients.iter().any(|(each, _)| *each == row.client);
                let pair = (row.client, Port::Skill(skill.folder.clone()));
                if !linkable(skill) || !has || held.as_ref() == Some(&pair) {
                    continue;
                }
                let Some((from, to)) = rack.skill_cable(client_at, skill_at) else {
                    continue;
                };
                let color = if wired(skill, row.client, &folders).is_some() {
                    wired_color()
                } else {
                    lead_color(row.client)
                };
                leads.push(Lead {
                    key: key(&pair),
                    color,
                    from,
                    to,
                });
            }
        }
        let over = drag
            .as_ref()
            .filter(|drag| drag.moved)
            .and_then(|drag| self.rack.over(drag, &self.servers));
        if let Some(drag) = drag
            .as_ref()
            .filter(|drag| drag.moved || drag.held.is_some())
            && let Some(anchor) = self.rack.jack(drag.fixed(), &drag.pressed, &self.servers)
        {
            // Keyed as the Cable picked up, so it swings on from where it hung rather than starting again.
            let (from, to) = match drag.fixed() {
                Side::Client => (anchor, drag.at),
                Side::Server => (drag.at, anchor),
            };
            leads.push(Lead {
                key: held.as_ref().map_or_else(|| "dragged".to_string(), key),
                color: lead_color(drag.pressed.0),
                from,
                to,
            });
        }
        // Captures and Reduce motion get the Cables where they'd come to rest; otherwise they fall and swing there.
        let motion = if cx.reduce_motion() || cfg!(feature = "screenshot") {
            Motion::Still
        } else {
            Motion::Swing
        };
        if self.rack.cables.update(&leads, motion) {
            window.request_animation_frame();
        }
        let shapes = self.rack.cables.shapes();
        let look = Look::of(cx);
        let ready = cx
            .try_global::<Running>()
            .is_some_and(|running| running.url().is_some() && running.token.is_some());
        let lead_width = rem * 0.3;
        let origin = Rc::clone(&self.rack.origin);
        let dragging = drag.is_some();
        let mut inner = div()
            .relative()
            .when(dragging, |this| this.cursor(CursorStyle::ClosedHand))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                this.drag_to(event.position, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, window, cx| {
                    this.let_go(event.position, window, cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, window, cx| {
                    this.let_go(event.position, window, cx);
                }),
            )
            .w(px(rack.width))
            .h(px(rack.height))
            .bg(look.cavity)
            .children(rack.ears.iter().map(|ear| ear_strip(*ear, look)))
            .children(
                rack.blanks
                    .iter()
                    .map(|blank| blank_plate(*blank, rem, look)),
            );
        for (at, row) in shown.iter().enumerate() {
            let module = &rack.clients[at];
            inner = inner.child(self.client_module(row, at, module, rem, ready, look, cx));
            inner = inner.children(screws(rack.ears[0], module.rect, rem, look));
        }
        for (at, server) in self.servers.iter().enumerate() {
            let module = &rack.servers[at];
            inner = inner.child(self.server_module(server, at, module, &shown, rem, look, cx));
            inner = inner.children(screws(rack.ears[1], module.rect, rem, look));
        }
        if let Some(divider) = rack.divider {
            inner = inner.child(divider_plate(divider, look));
        }
        for (at, skill) in skills.iter().enumerate() {
            let module = &rack.skills[at];
            inner = inner.child(self.skill_module(skill, module, &shown, &folders, rem, look, cx));
            inner = inner.children(screws(rack.ears[1], module.rect, rem, look));
        }
        // The module a dragged Cable would go to lights up.
        if let Some((_, rect)) = over {
            inner = inner.child(
                place(rect)
                    .rounded(px(3.))
                    .border_2()
                    .border_color(look.waiting)
                    .bg(Hsla {
                        a: 0.12,
                        ..look.waiting
                    }),
            );
        }
        if shown.is_empty() && self.servers.is_empty() {
            inner = inner.child(
                div()
                    .absolute()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(look.print_muted)
                    .child(
                        t!("Nothing to patch yet: add a server, or install a client.").into_owned(),
                    ),
            );
        }
        let inner = inner.child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    origin.set(bounds.origin);
                    cables::paint(bounds, &shapes, look.shadow, lead_width, window);
                },
            )
            .absolute()
            .size_full(),
        );
        div()
            .id("rack")
            .size_full()
            .overflow_scroll()
            .bg(look.cavity)
            .child(inner)
            .into_any_element()
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "each is one thing a module is drawn from"
    )]
    fn client_module(
        &self,
        row: &ClientRow,
        at: usize,
        module: &Module,
        rem: f32,
        ready: bool,
        look: Look,
        cx: &Context<Self>,
    ) -> Div {
        let client = row.client;
        let slug = client.slug();
        let connected = row.connected();
        let can_power = ready && (row.installed || connected);
        let tape = tape(letter(at), client.name().to_string(), look, cx)
            .id(SharedString::from(format!("rack-client-{slug}")))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.client = Some(client);
                this.show(Place::Clients, cx);
            }));
        let power = div()
            .id(SharedString::from(format!("rack-power-{slug}")))
            .child(rocker(connected, look))
            .when(can_power, |this| this.cursor_pointer())
            .when(!can_power, |this| this.opacity(0.5))
            .on_click(cx.listener(move |this, _, window, cx| {
                if !can_power {
                    return;
                }
                if connected {
                    this.disconnect(client, window, cx);
                } else {
                    this.connect(client, window, cx);
                }
            }));
        let shapes = Shape::ALL.into_iter().map(|shape| {
            let label = match shape {
                Shape::OneEntry => t!("All as one"),
                Shape::PerServer => t!("Separate"),
                Shape::Compact => t!("Compact"),
            };
            let choice = Choice {
                layout: shape,
                ..row.choice.clone()
            };
            push_button(
                SharedString::from(format!("rack-layout-{slug}-{shape:?}")),
                label.to_uppercase(),
                row.choice.layout == shape,
                look,
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.choose(client, choice.clone(), window, cx);
            }))
        });
        let face = module.face;
        let power = h_flex()
            .gap(rems(0.45))
            .items_center()
            .child(power)
            .child(led(connected.then_some(look.good), look))
            .child(legend(t!("Connected").to_uppercase(), look));
        let selector = v_flex()
            .flex_none()
            .justify_center()
            .gap(rems(0.25))
            .pl(rems(0.6))
            .border_l_1()
            .border_color(Hsla {
                a: 0.5,
                ..look.panel_edge
            })
            .children(shapes);
        let mut jacks: Vec<Stateful<Div>> = self
            .servers
            .iter()
            .enumerate()
            .map(|(server, status)| {
                let name = status.name.clone();
                let spot = local_spot(module, module.jacks[server]);
                jack(
                    SharedString::from(format!("rack-jack-{slug}-{name}")),
                    spot,
                    (server + 1).to_string(),
                    rem,
                    look,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        let pair = (client, Port::Server(name.clone()));
                        this.press(pair, Side::Client, event.position, window, cx);
                    }),
                )
            })
            .collect();
        // One jack for all the client's skills, if it has a skills folder for them to go in.
        if let Some(spot) = self.rack.layout.skills_jack(at)
            && super::skills::skill_folders(cx).client(client).is_some()
        {
            jacks.push(
                jack(
                    SharedString::from(format!("rack-jack-{slug}-skills")),
                    local_spot(module, spot),
                    // As the skills' own modules are marked.
                    "S".to_string(),
                    rem,
                    look,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        let pair = (client, Port::Skill(String::new()));
                        this.press(pair, Side::Client, event.position, window, cx);
                    }),
                ),
            );
        }
        let local = |rect: Rect| Rect {
            x: rect.x - module.rect.x,
            y: rect.y - module.rect.y,
            ..rect
        };
        panel(module.rect, look)
            .child(
                place(local(face)).child(
                    h_flex()
                        .size_full()
                        .gap(rems(0.6))
                        .p(rems(0.55))
                        .pl(rems(0.7))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .justify_between()
                                .child(tape)
                                .child(power),
                        )
                        .child(selector),
                ),
            )
            .children(jacks)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "each is one thing a module is drawn from"
    )]
    fn server_module(
        &self,
        server: &ServerStatus,
        at: usize,
        module: &Module,
        clients: &[&ClientRow],
        rem: f32,
        look: Look,
        cx: &Context<Self>,
    ) -> Div {
        let name = server.name.clone();
        let tape = {
            let name = name.clone();
            tape((at + 1).to_string(), server.name.clone(), look, cx)
                .id(SharedString::from(format!("rack-server-{name}")))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.server = Some(name.clone());
                    this.show(Place::Servers, cx);
                }))
        };
        let (light, words) = match &server.status {
            Status::Running => (Some(look.good), t!("Running")),
            Status::Starting => (Some(look.waiting), t!("Starting…")),
            Status::Idle => (None, t!("Idle")),
            Status::Off => (None, t!("Off")),
            Status::NeedsSecret { .. } => (Some(look.waiting), t!("Needs a secret")),
            Status::NeedsSignIn => (Some(look.waiting), t!("Needs you to sign in")),
            Status::Failed { .. } => (Some(look.failed), t!("Failed")),
        };
        let tools = server.tools.len();
        let status = h_flex()
            .gap(rems(0.45))
            .items_center()
            .child(led(light, look))
            .child(legend(words.to_uppercase(), look))
            .when(tools > 0, |this| {
                this.child(legend(
                    t!("%{count} tools", count = tools).to_uppercase(),
                    look,
                ))
            });
        let jacks: Vec<AnyElement> = clients
            .iter()
            .enumerate()
            .map(|(client_at, row)| {
                let client = row.client;
                let spot = local_spot(module, module.jacks[client_at]);
                let name = name.clone();
                jack(
                    SharedString::from(format!("rack-jack-{name}-{}", client.slug())),
                    spot,
                    letter(client_at),
                    rem,
                    look,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        let pair = (client, Port::Server(name.clone()));
                        this.press(pair, Side::Server, event.position, window, cx);
                    }),
                )
                .into_any_element()
            })
            .collect();
        let local = Rect {
            x: module.face.x - module.rect.x,
            y: 0.,
            ..module.face
        };
        panel(module.rect, look)
            .child(
                place(local).child(
                    v_flex()
                        .size_full()
                        .justify_between()
                        .p(rems(0.55))
                        .pr(rems(0.7))
                        .child(tape)
                        .child(status),
                ),
            )
            .children(jacks)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "each is one thing a module is drawn from"
    )]
    fn skill_module(
        &self,
        skill: &Skill,
        module: &Module,
        clients: &[&ClientRow],
        folders: &Folders,
        rem: f32,
        look: Look,
        cx: &Context<Self>,
    ) -> Div {
        let folder = skill.folder.clone();
        let ours = linkable(skill);
        let tape = {
            let folder = folder.clone();
            tape(
                if ours {
                    "S".to_string()
                } else {
                    "L".to_string()
                },
                skill.name.clone(),
                look,
                cx,
            )
            .id(SharedString::from(format!("rack-skill-{folder}")))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.skills.chosen = Some(folder.clone());
                this.show(Place::Skills, cx);
            }))
        };
        let says = if ours {
            skill.description.clone().unwrap_or_default().to_uppercase()
        } else {
            t!("Local: one of your own, left as it is").to_uppercase()
        };
        let jacks: Vec<AnyElement> = if ours {
            clients
                .iter()
                .enumerate()
                .filter(|(_, row)| folders.client(row.client).is_some())
                .map(|(client_at, row)| {
                    let client = row.client;
                    let spot = local_spot(module, module.jacks[client_at]);
                    let folder = folder.clone();
                    jack(
                        SharedString::from(format!("rack-jack-skill-{folder}-{}", client.slug())),
                        spot,
                        letter(client_at),
                        rem,
                        look,
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            let pair = (client, Port::Skill(folder.clone()));
                            this.press(pair, Side::Server, event.position, window, cx);
                        }),
                    )
                    .into_any_element()
                })
                .collect()
        } else {
            Vec::new()
        };
        let local = Rect {
            x: module.face.x - module.rect.x,
            y: 0.,
            ..module.face
        };
        panel(module.rect, look)
            .child(
                place(local).child(
                    v_flex()
                        .size_full()
                        .justify_center()
                        .gap(rems(0.3))
                        .p(rems(0.45))
                        .pr(rems(0.7))
                        .child(tape)
                        .child(legend(says, look).truncate()),
                ),
            )
            .children(jacks)
    }
}

/// The plate over the skills, printed with what's below it.
fn divider_plate(rect: Rect, look: Look) -> Div {
    place(rect)
        .rounded(px(2.))
        .bg(look.rail)
        .border_1()
        .border_color(look.rail_edge)
        .flex()
        .items_center()
        .justify_center()
        .gap(rems(0.6))
        .child(div().h(px(1.)).w(rems(3.)).bg(look.screw))
        .child(
            div()
                .text_xs()
                .font_bold()
                .text_color(look.screw)
                .child(t!("Skills").to_uppercase()),
        )
        .child(div().h(px(1.)).w(rems(3.)).bg(look.screw))
}

/// A module's front panel: brushed metal, lit from above, with a hairline catching the light along its top.
fn panel(rect: Rect, look: Look) -> Div {
    place(rect)
        .rounded(px(3.))
        .border_1()
        .border_color(look.panel_edge)
        .bg(linear_gradient(
            180.,
            linear_color_stop(look.panel_top, 0.),
            linear_color_stop(look.panel_bottom, 1.),
        ))
        .shadow(vec![BoxShadow {
            color: look.shadow,
            offset: point(px(0.), px(2.)),
            blur_radius: px(4.),
            spread_radius: px(0.),
            inset: false,
        }])
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(px(1.))
                .bg(hsla(0., 0., 1., 0.25)),
        )
}

/// A blank plate over a rack space with nothing in it, with a row of vents.
fn blank_plate(rect: Rect, rem: f32, look: Look) -> Div {
    let vents = ((rect.width - 2. * rem) / (rem * 0.9)).floor().max(0.) as usize;
    panel(rect, look).opacity(0.85).child(
        h_flex()
            .size_full()
            .justify_center()
            .items_center()
            .gap(px(rem * 0.45))
            .children((0..vents).map(|_| {
                div()
                    .w(px(rem * 0.45))
                    .h(px(rect.height * 0.45))
                    .rounded(px(rem * 0.22))
                    .bg(look.hole)
                    .opacity(0.6)
            })),
    )
}

/// An ear of the rack, the rail its modules are screwed to.
fn ear_strip(rect: Rect, look: Look) -> Div {
    place(rect)
        .bg(linear_gradient(
            90.,
            linear_color_stop(look.rail, 0.),
            linear_color_stop(look.rail_edge, 1.),
        ))
        .border_x_1()
        .border_color(look.rail_edge)
}

/// The two screws holding a module to `ear`, level with its top and bottom.
fn screws(ear: Rect, module: Rect, rem: f32, look: Look) -> Vec<Div> {
    let across = rem * 0.8;
    let x = ear.x + (ear.width - across) / 2.;
    [
        module.y + rem * 0.6,
        module.y + module.height - rem * 0.6 - across,
    ]
    .into_iter()
    .map(|y| {
        place(Rect {
            x,
            y,
            width: across,
            height: across,
        })
        .rounded_full()
        .bg(look.screw)
        .border_1()
        .border_color(look.slot)
        .flex()
        .items_center()
        .justify_center()
        .child(div().w(px(across * 0.6)).h(px(1.5)).bg(look.slot))
    })
    .collect()
}

/// A strip of label tape with a name written on it, and the letter or number it goes by beside it.
fn tape(mark: String, name: String, look: Look, cx: &App) -> Div {
    let mono = cx.theme().mono_font_family.clone();
    h_flex()
        .gap(rems(0.4))
        .items_center()
        .min_w_0()
        .child(
            div()
                .flex_none()
                .min_w(rems(1.3))
                .h(rems(1.3))
                .px(rems(0.2))
                .rounded(px(2.))
                .border_1()
                .border_color(look.print_muted)
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .font_bold()
                .text_color(look.print)
                .child(mark),
        )
        .child(
            div()
                .relative()
                .min_w_0()
                .px(rems(0.6))
                .py(rems(0.1))
                .child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| torn(bounds, look.tape, window),
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                )
                .font_family(mono)
                .font_semibold()
                .text_color(look.tape_text)
                .truncate()
                .child(name.to_uppercase()),
        )
}

/// How deep each tooth of a torn end is, as a share of the most, uneven so it reads as torn by hand.
const TEETH: [f32; 7] = [1., 0.45, 0.8, 0.3, 0.95, 0.55, 0.75];

/// A strip of tape filling `bounds`, its two ends torn into uneven teeth, over a shadow a point below it.
fn torn(bounds: Bounds<Pixels>, color: Hsla, window: &mut Window) {
    let (x, y) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
    let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let deep = (height * 0.18).min(width / 4.);
    let rows = TEETH.len();
    let shape = |dy: f32| {
        let at = |px_: f32, py: f32| point(px(px_), px(py + dy));
        let mut path = PathBuilder::fill();
        path.move_to(at(x + deep * TEETH[0], y));
        path.line_to(at(x + width - deep * TEETH[rows - 1], y));
        // Down the right end, then back up the left, in and out at each tooth.
        for (row, depth) in TEETH.iter().rev().enumerate() {
            let along = y + height * (row as f32 + 0.5) / rows as f32;
            let edge = if row % 2 == 0 { 0. } else { deep * depth };
            path.line_to(at(x + width - edge, along));
        }
        path.line_to(at(x + width - deep * TEETH[0], y + height));
        path.line_to(at(x + deep * TEETH[rows - 1], y + height));
        for (row, depth) in TEETH.iter().enumerate().rev() {
            let along = y + height * (row as f32 + 0.5) / rows as f32;
            let edge = if row % 2 == 0 { 0. } else { deep * depth };
            path.line_to(at(x + edge, along));
        }
        path.close();
        path.build().ok()
    };
    if let Some(path) = shape(1.) {
        window.paint_path(path, hsla(0., 0., 0., 0.3));
    }
    if let Some(path) = shape(0.) {
        window.paint_path(path, color);
    }
}

/// Small print on the panel, as a legend under a control.
fn legend(text: String, look: Look) -> Div {
    div()
        .text_size(rems(9. / 13.))
        .font_semibold()
        .text_color(look.print_muted)
        .whitespace_nowrap()
        .child(text)
}

/// A panel light: lit in `on`'s colour with a glow around it, or dark.
fn led(on: Option<Hsla>, look: Look) -> Div {
    let across = rems(0.6);
    let dot = div().size(across).rounded_full().border_1();
    match on {
        Some(color) => dot
            .bg(color)
            .border_color(Hsla {
                l: color.l * 0.6,
                ..color
            })
            .shadow(vec![BoxShadow {
                color: Hsla { a: 0.7, ..color },
                offset: point(px(0.), px(0.)),
                blur_radius: px(6.),
                spread_radius: px(1.),
                inset: false,
            }]),
        None => dot.bg(look.led_off).border_color(look.panel_edge),
    }
}

/// A rocker switch, pressed in at the top when on.
fn rocker(on: bool, look: Look) -> Div {
    let half = |pressed: bool| {
        div()
            .h(rems(0.8))
            .w_full()
            .rounded(px(2.))
            .when(pressed, |this| this.bg(look.rail))
            .when(!pressed, |this| {
                this.bg(linear_gradient(
                    180.,
                    linear_color_stop(look.button, 0.),
                    linear_color_stop(look.panel_bottom, 1.),
                ))
            })
            .when(!pressed, |this| {
                this.shadow(vec![BoxShadow {
                    color: hsla(0., 0., 0., 0.4),
                    offset: point(px(0.), px(1.)),
                    blur_radius: px(1.),
                    spread_radius: px(0.),
                    inset: false,
                }])
            })
    };
    v_flex()
        .w(rems(1.2))
        .h(rems(1.9))
        .p(px(2.))
        .gap(px(2.))
        .rounded(px(3.))
        .bg(look.hole)
        .child(half(on))
        .child(half(!on))
}

/// A small push button with a light in it, lit while it's the one chosen, and its legend beside it.
fn push_button(id: SharedString, label: String, chosen: bool, look: Look) -> Stateful<Div> {
    h_flex()
        .id(id)
        .items_center()
        .gap(rems(0.4))
        .cursor_pointer()
        .child(
            div()
                .flex_none()
                .w(rems(1.5))
                .h(rems(0.95))
                .rounded(px(2.))
                .border_1()
                .border_color(look.panel_edge)
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(look.button, 0.),
                    linear_color_stop(look.panel_bottom, 1.),
                ))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .w(rems(0.7))
                        .h(rems(0.22))
                        .rounded(px(1.))
                        .when(chosen, |this| this.bg(look.waiting))
                        .when(!chosen, |this| this.bg(look.led_off)),
                ),
        )
        .child(legend(label, look).when(chosen, |this| this.text_color(look.print)))
}

/// A jack on the panel, centred on `centre` (in the module's own points), with its number or letter printed over it.
fn jack(id: SharedString, centre: Spot, mark: String, rem: f32, look: Look) -> Stateful<Div> {
    let across = rem * 1.2;
    let hole = rem * 0.62;
    let hover = look.print;
    v_flex()
        .id(id)
        .absolute()
        .left(px(centre.x - rem))
        .top(px(centre.y - rem * 1.75))
        .w(px(rem * 2.))
        .items_center()
        .cursor(CursorStyle::OpenHand)
        .child(
            div()
                .h(px(rem * 1.15))
                .text_size(rems(9. / 13.))
                .font_bold()
                .text_color(look.print_muted)
                .child(mark),
        )
        .child(
            div()
                .size(px(across))
                .rounded_full()
                .border_1()
                .border_color(look.nut_edge)
                .bg(look.nut)
                .flex()
                .items_center()
                .justify_center()
                .hover(move |this| this.border_color(hover))
                .child(div().size(px(hole)).rounded_full().bg(look.hole)),
        )
}

/// Where things on the rack are in the window, as last drawn, for tests to press and let go.
#[cfg(test)]
impl MainView {
    fn on_screen(&self, spot: Spot) -> Point<Pixels> {
        self.rack.origin.get() + point(px(spot.x), px(spot.y))
    }

    /// The jack for `client` and `server` on the client's module, or with `on_client` false, on the server's.
    pub(super) fn jack_on_screen(
        &self,
        on_client: bool,
        client: Client,
        server: &str,
    ) -> Point<Pixels> {
        let side = if on_client {
            Side::Client
        } else {
            Side::Server
        };
        let spot = self
            .rack
            .jack(
                side,
                &(client, Port::Server(server.to_string())),
                &self.servers,
            )
            .expect("the jack is on the rack");
        self.on_screen(spot)
    }

    /// `client`'s jack on `folder`'s skill module.
    pub(super) fn skill_jack_on_screen(&self, client: Client, folder: &str) -> Point<Pixels> {
        let pair = (client, Port::Skill(folder.to_string()));
        let spot = self
            .rack
            .jack(Side::Server, &pair, &self.servers)
            .expect("the jack is on the rack");
        self.on_screen(spot)
    }

    /// The middle of `client`'s module's face, away from its jacks and switches.
    pub(super) fn client_on_screen(&self, client: Client) -> Point<Pixels> {
        let at = self
            .rack
            .clients
            .iter()
            .position(|each| *each == client)
            .expect("the client is on the rack");
        let face = self.rack.layout.clients[at].face;
        self.on_screen(Spot::new(
            face.x + face.width / 2.,
            face.y + face.height - 4.,
        ))
    }

    /// The gap between the columns, where a Cable is let go over nothing.
    pub(super) fn gap_on_screen(&self) -> Point<Pixels> {
        let client = self.rack.layout.clients[0].rect;
        let server = self.rack.layout.servers[0].rect;
        self.on_screen(Spot::new(
            (client.x + client.width + server.x) / 2.,
            self.rack.layout.height - 4.,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::letter;

    #[test]
    fn clients_on_the_rack_go_by_letters_from_a() {
        assert_eq!(letter(0), "A");
        assert_eq!(letter(2), "C");
    }
}
