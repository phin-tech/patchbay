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
mod hardware;
mod layout;
mod look;
mod modules;

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::{ActiveTheme as _, StyledExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use patchbay_core::clients::{Client, Layout as Shape};
use patchbay_core::skills::{Folders, Reach, Skill};
use patchbay_core::strand::Point as Spot;
use patchbay_gateway::{ServerStatus, Status};
use rust_i18n::t;

use cables::{Cables, Lead, Motion};
use drag::{Drag, Outcome, Pair, Port, Side};
use hardware::*;
use layout::{Column, Module, Rect};
use look::{Look, lead_color, wired_color};

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
                    key: pair.clone(),
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
                    key: pair.clone(),
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
            // Keyed as the Cable picked up, so it swings on from where it hung rather than starting again; a new one
            // by the jack it's drawn from, which holds no Cable yet.
            let (from, to) = match drag.fixed() {
                Side::Client => (anchor, drag.at),
                Side::Server => (drag.at, anchor),
            };
            leads.push(Lead {
                key: held.clone().unwrap_or_else(|| drag.pressed.clone()),
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
