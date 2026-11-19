//! The rack's modules: one for each client, each server and each skill, with its jacks and controls.

use super::*;

impl MainView {
    #[expect(
        clippy::too_many_arguments,
        reason = "each is one thing a module is drawn from"
    )]
    pub(super) fn client_module(
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
                // The switch is Clients ▸ Connect or Disconnect, for the client it's on.
                this.client = Some(client);
                this.run(
                    if connected {
                        "clients.disconnect"
                    } else {
                        "clients.connect"
                    },
                    window,
                    cx,
                );
            }));
        let shapes = Shape::ALL.into_iter().map(|shape| {
            let label = match shape {
                Shape::OneEntry => t!("All as one"),
                Shape::PerServer => t!("Separate"),
                Shape::Compact => t!("Compact"),
            };
            let command = match shape {
                Shape::OneEntry => "clients.layout-one",
                Shape::PerServer => "clients.layout-separate",
                Shape::Compact => "clients.layout-compact",
            };
            push_button(
                SharedString::from(format!("rack-layout-{slug}-{shape:?}")),
                label.to_uppercase(),
                row.choice.layout == shape,
                look,
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.client = Some(client);
                this.run(command, window, cx);
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
            .rack_servers(cx)
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
            && crate::view::skill_folders(cx).client(client).is_some()
        {
            jacks.push(
                jack(
                    SharedString::from(format!("rack-jack-{slug}-skills")),
                    local_spot(module, spot),
                    // As the skills' own modules are marked: S for a skill in the store.
                    t!("S").into_owned(),
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
    pub(super) fn server_module(
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
    pub(super) fn skill_module(
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
                // S for a skill Patchbay looks after, which can be patched; L for one of the person's own, hard-wired.
                if ours {
                    t!("S").into_owned()
                } else {
                    t!("L").into_owned()
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
