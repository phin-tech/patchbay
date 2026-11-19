//! The Servers place: every server in a list, and the one chosen, with what it needs, its tools and what can be
//! done with it.

use patchbay_core::manifest::edit;

use super::*;

/// What can be done to the chosen server from the menus, as its panel's buttons do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ServerAction {
    Edit,
    Restart,
    Remove,
    TurnOn,
    TurnOff,
    SignIn,
    SignOut,
    ChangeKey,
}

impl MainView {
    /// A secret field for each secret a server is waiting for, or is having replaced; none for the rest.
    pub(super) fn sync_secret_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let wanted = wanted_secrets(&self.servers, &self.replacing);
        self.secrets
            .retain(|account, _| wanted.iter().any(|secret| &secret.account == account));
        for secret in wanted {
            if self.secrets.contains_key(&secret.account) {
                continue;
            }
            let field = cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(true)
                    .placeholder(t!("Paste it here").into_owned())
            });
            let account = secret.account.clone();
            self._fields.push(cx.subscribe_in(
                &field,
                window,
                move |this, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.save_secret(&account, window, cx);
                    }
                },
            ));
            self.secrets.insert(secret.account, field);
        }
    }

    pub(super) fn choose_server(&mut self, name: &str, cx: &mut Context<Self>) {
        self.server = Some(name.to_string());
        self.editor = None;
        self.exporting = None;
        self.importing = None;
        self.teams.subscribing = None;
        self.teams.reviewing = None;
        cx.notify();
    }

    /// Takes a server out: the person's own from their manifest, a team's turned off in it. Undo puts the manifest
    /// back as it was.
    fn remove_server(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(running) = cx.try_global::<Running>() else {
            return;
        };
        let team = self
            .servers
            .iter()
            .find(|server| server.name == name)
            .and_then(|server| server.team.clone());
        let path = running.gateway.home().manifest();
        let old = std::fs::read_to_string(&path).unwrap_or_default();
        let changed = match &team {
            None => edit::remove(&old, name),
            Some(_) => edit::set_enabled(&old, name, false, true),
        };
        let written = changed.map_err(|error| error.to_string()).and_then(|text| {
            patchbay_core::write_atomically(&path, text.as_bytes())
                .map_err(|error| error.to_string())
        });
        match written {
            Ok(()) => {
                reload_gateway(cx);
                let said = match team {
                    None => t!("Removed %{server}", server = name),
                    Some(_) => t!("Turned %{server} off", server = name),
                };
                Toast::new(said)
                    .key("remove")
                    .undo(move |_, cx| {
                        if patchbay_core::write_atomically(&path, old.as_bytes()).is_ok() {
                            reload_gateway(cx);
                        }
                    })
                    .show(window, cx);
            }
            Err(error) => Toast::error(t!("Couldn't change your servers: %{error}", error = error))
                .show(window, cx),
        }
    }

    /// Does to the chosen server what a button in its panel does, from a menu or the palette. Says why not when it
    /// can't, as the panel would have no button for it.
    pub(super) fn act_on_chosen_server(
        &mut self,
        action: ServerAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(server) = self
            .server
            .as_ref()
            .and_then(|name| self.servers.iter().find(|server| &server.name == name))
            .cloned()
        else {
            Toast::new(t!("Choose a server first"))
                .key("server-action")
                .show(window, cx);
            return;
        };
        let name = server.name.clone();
        let refuse = |said: std::borrow::Cow<str>, window: &mut Window, cx: &mut Context<Self>| {
            Toast::new(said).key("server-action").show(window, cx);
        };
        let running = matches!(server.status, Status::Running);
        match action {
            ServerAction::Edit if server.team.is_some() => refuse(
                t!(
                    "%{server} is the team's, so it's changed in the team's repository",
                    server = name
                ),
                window,
                cx,
            ),
            ServerAction::Edit => self.start_editing(Editing::Server(name), window, cx),
            ServerAction::Restart
                if !matches!(server.status, Status::Running | Status::Failed { .. }) =>
            {
                refuse(t!("%{server} isn't running", server = name), window, cx);
            }
            ServerAction::Restart => {
                self.restart(&name, cx);
                Toast::new(t!("Restarting %{server}", server = name))
                    .key("server-action")
                    .show(window, cx);
            }
            ServerAction::Remove => self.remove_server(&name, window, cx),
            ServerAction::TurnOn | ServerAction::TurnOff => {
                let on = action == ServerAction::TurnOn;
                if on == !matches!(server.status, Status::Off) {
                    let said = if on {
                        t!("%{server} is on already", server = name)
                    } else {
                        t!("%{server} is off already", server = name)
                    };
                    refuse(said, window, cx);
                } else {
                    self.set_enabled(&name, on, window, cx);
                }
            }
            ServerAction::SignIn if matches!(server.status, Status::NeedsSignIn) => {
                if !self.signing_in.contains(&name) {
                    self.sign_in(&name, window, cx);
                }
            }
            ServerAction::SignIn if server.oauth && running => {
                refuse(
                    t!("Signed in to %{server} already", server = name),
                    window,
                    cx,
                );
            }
            ServerAction::SignOut if server.oauth && running => self.sign_out(&name, cx),
            ServerAction::SignIn | ServerAction::SignOut => {
                refuse(t!("%{server} has no sign-in", server = name), window, cx);
            }
            ServerAction::ChangeKey
                if server.secrets.is_empty()
                    || matches!(server.status, Status::NeedsSecret { .. }) =>
            {
                let said = if server.secrets.is_empty() {
                    t!("%{server} has no keys", server = name)
                } else {
                    t!(
                        "%{server} is waiting for its key: give it in its panel",
                        server = name
                    )
                };
                self.place = Place::Servers;
                refuse(said, window, cx);
            }
            ServerAction::ChangeKey => {
                self.place = Place::Servers;
                self.toggle_replacing(&name, window, cx);
            }
        }
    }

    pub(super) fn server_list(&self, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let adding = matches!(
            self.editor.as_ref().map(|editor| &editor.mode),
            Some(Editing::New)
        );
        let query = Query::servers(&self.filter_text(Place::Servers, cx));
        let shown: Vec<&ServerStatus> = self
            .servers
            .iter()
            .filter(|server| query.keeps_server(server))
            .collect();
        let nothing_matches = shown.is_empty() && !self.servers.is_empty();
        v_flex()
            .size_full()
            .child(
                self.panel_header(t!("Servers"), cx).child(
                    Button::new("add-server")
                        .ghost()
                        .xsmall()
                        .label(t!("Add…").into_owned())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.run("servers.add", window, cx);
                        })),
                ),
            )
            .children(self.team_updates_banner(cx))
            .when(!self.problems.is_empty(), |this| {
                this.child(
                    v_flex()
                        .gap_1()
                        .px_3()
                        .py_2()
                        .border_b_1()
                        .border_color(theme.border)
                        .children(self.problems.iter().map(|problem| {
                            div()
                                .text_sm()
                                .text_color(theme.warning)
                                .child(problem.clone())
                        })),
                )
            })
            .child(self.filter_field(Place::Servers, cx))
            .child(
                div()
                    .id("server-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .when(nothing_matches, |this| {
                        this.child(div().p_3().child(hint(t!("No servers match."), cx)))
                    })
                    .child(v_flex().children(shown.into_iter().map(|server| {
                        let name = server.name.clone();
                        let chosen =
                            !adding && self.server.as_deref() == Some(server.name.as_str());
                        list_row(chosen, cx)
                            .id(SharedString::from(format!("server-row-{name}")))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .child(div().font_medium().truncate().child(name.clone()))
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(theme.muted_foreground)
                                            .truncate()
                                            .child(server.target.clone()),
                                    ),
                            )
                            .child(status_badge(&server.status, true))
                            .child(self.enabled_switch(server, cx))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.choose_server(&name, cx)),
                            )
                    }))),
            )
            .into_any_element()
    }

    pub(super) fn server_detail(&self, cx: &Context<Self>) -> AnyElement {
        if let Some(editor) = &self.editor {
            return self.editor_panel(editor, cx);
        }
        if let Some(exporting) = &self.exporting {
            return self.export_panel(exporting, cx);
        }
        if let Some(importing) = &self.importing {
            return self.import_panel(importing, cx);
        }
        if let Some(panel) = self.teams_panel(cx) {
            return panel;
        }
        let Some(server) = self
            .server
            .as_ref()
            .and_then(|name| self.servers.iter().find(|server| &server.name == name))
        else {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_3()
                .p_6()
                .child(
                    div()
                        .text_lg()
                        .font_semibold()
                        .child(t!("No servers yet").into_owned()),
                )
                .child(hint(
                    t!("Add one by pasting its config, its command or its URL."),
                    cx,
                ))
                .child(
                    Button::new("add-first")
                        .primary()
                        .label(t!("Add Server…").into_owned())
                        .on_click(
                            cx.listener(|this, _, window, cx| this.run("servers.add", window, cx)),
                        ),
                )
                .into_any_element();
        };
        let theme = cx.theme();
        let name = server.name.clone();
        let mine = server.team.is_none();
        let button =
            |id: &'static str, label: String| Button::new(id).ghost().xsmall().label(label);
        let actions = h_flex()
            .gap_1()
            .when(mine, |row| {
                row.child(
                    button("edit-server", t!("Edit…").into_owned()).on_click(cx.listener({
                        let name = name.clone();
                        move |this, _, window, cx| {
                            this.start_editing(Editing::Server(name.clone()), window, cx)
                        }
                    })),
                )
            })
            .when(
                matches!(server.status, Status::Running | Status::Failed { .. }),
                |row| {
                    row.child(
                        button("restart-server", t!("Restart").into_owned()).on_click(cx.listener(
                            {
                                let name = name.clone();
                                move |this, _, _, cx| this.restart(&name, cx)
                            },
                        )),
                    )
                },
            )
            // A team's server is turned off with the switch in its row; only your own can be taken out, or the whole
            // team.
            .when(!mine, |row| {
                row.child(
                    button("remove-team", t!("Remove Team").into_owned()).on_click(
                        cx.listener(|this, _, window, cx| this.remove_chosen_team(window, cx)),
                    ),
                )
            })
            .when(mine, |row| {
                row.child(
                    button("remove-server", t!("Remove").into_owned()).on_click(cx.listener({
                        let name = name.clone();
                        move |this, _, window, cx| this.remove_server(&name, window, cx)
                    })),
                )
            });
        let signing_in = self.signing_in.contains(&name);
        let secrets = shown_secrets(server, &self.replacing);
        let replacing = self.replacing.contains(&name);
        let exposed = server.tools.iter().filter(|tool| tool.exposed).count();
        // `is:write`, `is:off` and the like in the filter narrow the tools shown too.
        let query = Query::servers(&self.filter_text(Place::Servers, cx));
        let tools: Vec<&ToolStatus> = server
            .tools
            .iter()
            .filter(|tool| !query.narrows_tools() || query.keeps_tool(tool))
            .collect();
        let narrowed = tools.len() < server.tools.len();
        let failed = match &server.status {
            Status::Failed { reason } => Some(reason.clone()),
            _ => None,
        };
        let has_keys = !server.secrets.is_empty();
        let body = v_flex()
            .gap_4()
            .p_4()
            .child(field(
                if server.target.starts_with("http") {
                    t!("Reached at")
                } else {
                    t!("Runs")
                },
                v_flex()
                    .gap_1()
                    .child(h_flex().child(shell::code_chip(server.target.clone(), cx)))
                    .when_some(server.team.clone(), |this, team| {
                        this.child(hint(t!("From %{team}'s manifest", team = team), cx))
                    }),
                cx,
            ))
            .when_some(failed, |this, reason| {
                this.child(field(
                    t!("Problem"),
                    div().text_base().text_color(theme.danger).child(reason),
                    cx,
                ))
            })
            .when(
                matches!(server.status, Status::NeedsSignIn) || server.oauth,
                |this| {
                    let signed_in = matches!(server.status, Status::Running);
                    let control = if matches!(server.status, Status::NeedsSignIn) {
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("sign-in")
                                    .primary()
                                    .xsmall()
                                    .disabled(signing_in)
                                    .label(if signing_in {
                                        t!("Waiting for the browser…").into_owned()
                                    } else {
                                        t!("Sign In…").into_owned()
                                    })
                                    .on_click(cx.listener({
                                        let name = name.clone();
                                        move |this, _, window, cx| this.sign_in(&name, window, cx)
                                    })),
                            )
                            .child(hint(t!("Every client uses this one sign-in."), cx))
                    } else {
                        h_flex()
                            .gap_2()
                            .child(div().text_base().child(if signed_in {
                                t!("Signed in").into_owned()
                            } else {
                                t!("Not signed in").into_owned()
                            }))
                            .when(signed_in, |row| {
                                row.child(button("sign-out", t!("Sign Out").into_owned()).on_click(
                                    cx.listener({
                                        let name = name.clone();
                                        move |this, _, _, cx| this.sign_out(&name, cx)
                                    }),
                                ))
                            })
                    };
                    this.child(field(t!("Sign-in"), control, cx))
                },
            )
            .when(has_keys || !secrets.is_empty(), |this| {
                let content = v_flex()
                    .gap_2()
                    .children(secrets.iter().map(|secret| self.secret_field(secret, cx)))
                    .when(secrets.is_empty(), |list| {
                        list.child(
                            h_flex().gap_2().children(
                                server
                                    .secrets
                                    .iter()
                                    .map(|secret| div().text_base().child(secret.prompt.clone())),
                            ),
                        )
                    })
                    .when(
                        !matches!(server.status, Status::NeedsSecret { .. }),
                        |list| {
                            list.child(
                                h_flex().child(
                                    button(
                                        "change-key",
                                        if replacing {
                                            t!("Keep the Key").into_owned()
                                        } else {
                                            t!("Change Key…").into_owned()
                                        },
                                    )
                                    .on_click(cx.listener({
                                        let name = name.clone();
                                        move |this, _, window, cx| {
                                            this.toggle_replacing(&name, window, cx)
                                        }
                                    })),
                                ),
                            )
                        },
                    );
                this.child(field(t!("Keys"), content, cx))
            })
            .when(!server.tools.is_empty(), |this| {
                this.child(field(
                    t!("Tools"),
                    v_flex()
                        .gap_1p5()
                        .child(hint(
                            if narrowed {
                                t!(
                                    "%{exposed} of %{count} given to clients; %{shown} match the filter",
                                    exposed = exposed,
                                    count = server.tools.len(),
                                    shown = tools.len()
                                )
                            } else {
                                t!(
                                    "%{exposed} of %{count} given to clients",
                                    exposed = exposed,
                                    count = server.tools.len()
                                )
                            },
                            cx,
                        ))
                        .child(
                            h_flex()
                                .gap_1()
                                .flex_wrap()
                                .children(tools.iter().map(|tool| {
                                    let chip = shell::code_chip(tool.name.clone(), cx);
                                    if tool.exposed {
                                        chip
                                    } else {
                                        chip.line_through().text_color(theme.muted_foreground)
                                    }
                                })),
                        ),
                    cx,
                ))
            });
        v_flex()
            .size_full()
            .child(
                self.panel_header_beside(name.clone(), status_badge(&server.status, true), cx)
                    .child(actions),
            )
            .child(
                div()
                    .id("server-detail")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(body),
            )
            .into_any_element()
    }

    pub(super) fn save_secret(
        &mut self,
        account: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(field) = self.secrets.get(account) else {
            return;
        };
        let secret = field.read(cx).value().trim().to_string();
        if secret.is_empty() {
            return;
        }
        let Some(running) = cx.try_global::<Running>() else {
            return;
        };
        let gateway = running.gateway.clone();
        let account = account.to_string();
        let using: Vec<String> = self
            .servers
            .iter()
            .filter(|server| {
                server
                    .secrets
                    .iter()
                    .any(|secret| secret.account == account)
            })
            .map(|server| server.name.clone())
            .collect();
        for name in using {
            self.replacing.remove(&name);
        }
        let saving = running.spawn(async move { gateway.set_secret(&account, &secret).await });
        cx.spawn_in(window, async move |this, cx| {
            let saved = saving.await;
            let _ = this.update_in(cx, |_, window, cx| match saved {
                Ok(Ok(())) => Toast::new(t!("Kept it in the Keychain")).show(window, cx),
                Ok(Err(error)) => Toast::error(t!(
                    "Couldn't keep it: %{error}",
                    error = format!("{error:#}")
                ))
                .show(window, cx),
                Err(_) => {}
            });
        })
        .detach();
    }

    pub(super) fn sign_in(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(running) = cx.try_global::<Running>() else {
            return;
        };
        let gateway = running.gateway.clone();
        let server = name.to_string();
        let signing = running.spawn({
            let server = server.clone();
            async move {
                gateway
                    .sign_in(&server, |url| {
                        let _ = std::process::Command::new("open").arg(url).status();
                    })
                    .await
            }
        });
        self.signing_in.insert(server.clone());
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let signed = signing.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.signing_in.remove(&server);
                cx.notify();
                match signed {
                    Ok(Ok(_)) => {
                        Toast::new(t!("Signed in to %{server}", server = server)).show(window, cx)
                    }
                    Ok(Err(error)) => Toast::error(t!(
                        "Couldn't sign in to %{server}: %{error}",
                        server = server,
                        error = format!("{error:#}")
                    ))
                    .show(window, cx),
                    Err(_) => {}
                }
            });
        })
        .detach();
    }

    pub(super) fn toggle_replacing(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.replacing.remove(name) {
            self.replacing.insert(name.to_string());
        }
        self.sync(window, cx);
    }

    pub(super) fn sign_out(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(running) = cx.try_global::<Running>() else {
            return;
        };
        let gateway = running.gateway.clone();
        let name = name.to_string();
        running.spawn(async move { gateway.sign_out(&name).await });
    }

    pub(super) fn restart(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(running) = cx.try_global::<Running>() else {
            return;
        };
        let gateway = running.gateway.clone();
        let name = name.to_string();
        running.spawn(async move { gateway.restart(&name).await });
    }

    pub(super) fn secret_field(&self, secret: &SecretNeeded, cx: &Context<Self>) -> AnyElement {
        let Some(field) = self.secrets.get(&secret.account) else {
            return div().into_any_element();
        };
        let account = secret.account.clone();
        v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .child(div().text_base().child(secret.prompt.clone()))
                    .when_some(secret.help.clone(), |row, help| {
                        row.child(
                            Button::new(SharedString::from(format!("help-{account}")))
                                .link()
                                .xsmall()
                                .label(t!("Get one").into_owned())
                                .on_click(move |_, _, cx| cx.open_url(&help)),
                        )
                    }),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .max_w(rems(360. / 13.))
                            .child(small_field(field).mask_toggle()),
                    )
                    .child(
                        Button::new(SharedString::from(format!("save-{account}")))
                            .primary()
                            .xsmall()
                            .label(t!("Keep in Keychain").into_owned())
                            .on_click(cx.listener({
                                let account = account.clone();
                                move |this, _, window, cx| this.save_secret(&account, window, cx)
                            })),
                    ),
            )
            .into_any_element()
    }
}

/// A server's state as a badge: small in a list, with its words in full beside the server's own name.
pub(super) fn status_badge(status: &Status, small: bool) -> Tag {
    let (tag, words) = match status {
        Status::Running => (Tag::success(), t!("Running")),
        Status::Starting => (Tag::secondary(), t!("Starting…")),
        Status::Idle => (Tag::secondary(), t!("Idle")),
        Status::Off => (Tag::secondary(), t!("Off")),
        Status::NeedsSecret { .. } => (Tag::warning(), t!("Needs a secret")),
        Status::NeedsSignIn => (Tag::warning(), t!("Needs you to sign in")),
        Status::Failed { .. } => (Tag::danger(), t!("Failed")),
    };
    let tag = shell::badge(tag).child(words.into_owned());
    if small { tag.xsmall() } else { tag }
}

/// The secrets a server shows a field for: those it's waiting for, or all of them while they're being replaced.
pub(super) fn shown_secrets(
    server: &ServerStatus,
    replacing: &BTreeSet<String>,
) -> Vec<SecretNeeded> {
    match &server.status {
        Status::NeedsSecret { secrets } => secrets.clone(),
        _ if replacing.contains(&server.name) => server.secrets.clone(),
        _ => Vec::new(),
    }
}

/// Every secret the servers are waiting for or are having replaced, each once.
pub(super) fn wanted_secrets(
    servers: &[ServerStatus],
    replacing: &BTreeSet<String>,
) -> Vec<SecretNeeded> {
    let mut wanted: Vec<SecretNeeded> = servers
        .iter()
        .flat_map(|server| shown_secrets(server, replacing))
        .collect();
    wanted.sort_by(|a, b| a.account.cmp(&b.account));
    wanted.dedup_by(|a, b| a.account == b.account);
    wanted
}
