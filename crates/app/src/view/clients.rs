//! The Clients place: every AI client Patchbay knows, and the one chosen, with how it's given the gateway.

use gpui_kit::component::checkbox::Checkbox;
use patchbay_core::manifest::edit;

use super::*;

/// A client whose own servers, by name, are in the editor, to come out of it once Patchbay has them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakingOver {
    pub client: Client,
    pub names: Vec<String>,
}

impl MainView {
    /// Reads each client's config, and puts right the entries of any connected client given an entry for each server
    /// whose servers have changed.
    pub(super) fn sync_clients(&mut self, cx: &mut Context<Self>) {
        let Some(running) = cx.try_global::<Running>() else {
            return;
        };
        let choices = Choices::load(running.gateway.home());
        let home = client_home(cx);
        self.clients = Client::ALL
            .into_iter()
            .map(|client| ClientRow {
                client,
                installed: client.detected(&home),
                entries: client.entries_in(&home),
                own: client
                    .own_servers(&home)
                    .into_iter()
                    .map(|(name, _)| name)
                    .collect(),
                choice: choices.get(client),
            })
            .collect();
        // A client given an entry for each server follows the servers: one added or removed changes its entries.
        let stale: Vec<Client> = self
            .clients
            .iter()
            .filter(|row| row.connected())
            .filter(|row| {
                let mut wanted: Vec<String> = entries_for(row.client, cx)
                    .map(|entries| entries.into_iter().map(|entry| entry.name).collect())
                    .unwrap_or_default();
                let mut present = row.entries.clone();
                wanted.sort();
                present.sort();
                !wanted.is_empty() && wanted != present
            })
            .map(|row| row.client)
            .collect();
        for client in stale {
            if let Ok(entries) = entries_for(client, cx)
                && let Err(error) = client.connect(&home, &entries)
            {
                tracing::warn!(client = client.slug(), %error, "couldn't follow the servers");
            }
            if let Some(row) = self.clients.iter_mut().find(|row| row.client == client) {
                row.entries = client.entries_in(&home);
            }
        }
    }

    fn choose_client(&mut self, client: Client, cx: &mut Context<Self>) {
        self.client = Some(client);
        cx.notify();
    }

    pub(super) fn client_list(&self, cx: &Context<Self>) -> AnyElement {
        let query = Query::plain(&self.filter_text(Place::Clients, cx));
        let shown = self
            .clients
            .iter()
            .filter(|row| query.matches(&[row.client.name(), row.client.slug()]));
        v_flex()
            .size_full()
            .child(self.panel_header(t!("Clients"), cx))
            .child(self.filter_field(Place::Clients, cx))
            .child(
                div()
                    .id("client-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(v_flex().children(shown.map(|row| {
                        let client = row.client;
                        let given = self.given(row);
                        let gets = if row.choice.profile == "default" {
                            t!("Every server").into_owned()
                        } else if given.is_empty() {
                            t!("No servers").into_owned()
                        } else {
                            given.join(", ")
                        };
                        list_row(self.client == Some(client), cx)
                            .id(SharedString::from(format!("client-row-{}", client.slug())))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .child(div().truncate().child(client.name()))
                                    .when(row.installed, |this| {
                                        this.child(
                                            div()
                                                .text_sm()
                                                .text_color(cx.theme().muted_foreground)
                                                .truncate()
                                                .child(gets),
                                        )
                                    }),
                            )
                            .child(client_badge(row).xsmall())
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.choose_client(client, cx)),
                            )
                    }))),
            )
            .into_any_element()
    }

    pub(super) fn client_detail(&self, cx: &Context<Self>) -> AnyElement {
        let Some(row) = self
            .client
            .and_then(|client| self.clients.iter().find(|row| row.client == client))
        else {
            return div().into_any_element();
        };
        let client = row.client;
        let ready = cx
            .try_global::<Running>()
            .is_some_and(|running| running.url().is_some() && running.token.is_some());
        let button = if row.connected() {
            Button::new("disconnect")
                .ghost()
                .xsmall()
                .label(t!("Disconnect").into_owned())
                .on_click(
                    cx.listener(move |this, _, window, cx| this.disconnect(client, window, cx)),
                )
        } else {
            Button::new("connect")
                .primary()
                .xsmall()
                .disabled(!ready || !row.installed)
                .label(t!("Connect").into_owned())
                .on_click(cx.listener(move |this, _, window, cx| this.connect(client, window, cx)))
        };
        let layout_says = match row.choice.layout {
            Layout::OneEntry => {
                t!("One entry; each tool is named after its server, as linear__list_issues.")
            }
            Layout::PerServer => {
                t!("An entry for each server, kept in step as your servers change.")
            }
            Layout::Compact => t!(
                "Three tools that search for the rest and call them, to save the client's context."
            ),
        };
        let path = client.config_path(&client_home(cx)).display().to_string();
        let body = v_flex()
            .gap_4()
            .p_4()
            .child(field(
                t!("Config"),
                v_flex()
                    .gap_1()
                    .child(h_flex().child(shell::code_chip(path, cx)))
                    .when(!row.installed, |this| {
                        this.child(hint(
                            t!(
                                "%{client} doesn't look installed on this Mac.",
                                client = client.name()
                            ),
                            cx,
                        ))
                    }),
                cx,
            ))
            .when(!row.own.is_empty(), |this| {
                this.child(field(t!("Its own"), own_servers(row, cx), cx))
            })
            .child(field(t!("Servers"), self.server_choice(row, cx), cx))
            .child(field(
                t!("Layout"),
                v_flex()
                    .gap_1p5()
                    .child(self.choices(row, cx))
                    .child(hint(layout_says, cx)),
                cx,
            ))
            .when(row.connected(), |this| {
                this.child(field(
                    t!("Entries"),
                    v_flex()
                        .gap_1p5()
                        .child(
                            h_flex().gap_1().flex_wrap().children(
                                row.entries
                                    .iter()
                                    .map(|entry| shell::code_chip(entry.clone(), cx)),
                            ),
                        )
                        .child(hint(
                            t!(
                                "Restart %{client} to pick up a change.",
                                client = client.name()
                            ),
                            cx,
                        )),
                    cx,
                ))
            });
        v_flex()
            .size_full()
            .child(
                self.panel_header_beside(client.name(), client_badge(row).xsmall(), cx)
                    .child(button),
            )
            .child(
                div()
                    .id("client-detail")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(body),
            )
            .into_any_element()
    }

    pub(super) fn connect(&mut self, client: Client, window: &mut Window, cx: &mut Context<Self>) {
        let connected = entries_for(client, cx).and_then(|entries| {
            client
                .connect(&client_home(cx), &entries)
                .map_err(|error| error.to_string())
        });
        match connected {
            Ok(_) => Toast::new(t!(
                "Connected %{client}: restart it to pick Patchbay up",
                client = client.name()
            ))
            .key(format!("client-{}", client.slug()))
            .undo(move |_, cx| {
                let _ = client.disconnect(&client_home(cx));
                cx.refresh_windows();
            })
            .show(window, cx),
            Err(error) => Toast::error(t!(
                "Couldn't connect %{client}: %{error}",
                client = client.name(),
                error = error
            ))
            .show(window, cx),
        }
        self.sync(window, cx);
    }

    /// Connects the chosen client, or takes Patchbay out of it.
    pub(super) fn power_chosen_client(
        &mut self,
        on: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(client) = self.client else {
            Toast::new(t!("Choose a client")).show(window, cx);
            return;
        };
        if on {
            self.connect(client, window, cx);
        } else {
            self.disconnect(client, window, cx);
        }
    }

    /// Gives the chosen client its servers in `layout`, keeping the profile it has.
    pub(super) fn lay_out_chosen_client(
        &mut self,
        layout: Layout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self
            .client
            .and_then(|client| self.clients.iter().find(|row| row.client == client))
        else {
            Toast::new(t!("Choose a client")).show(window, cx);
            return;
        };
        let (client, choice) = (
            row.client,
            Choice {
                layout,
                ..row.choice.clone()
            },
        );
        self.choose(client, choice, window, cx);
    }

    /// Keeps what `client` is given, and gives it that now if it's connected.
    pub(super) fn choose(
        &mut self,
        client: Client,
        choice: Choice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(running) = cx.try_global::<Running>() else {
            return;
        };
        let mut choices = Choices::load(running.gateway.home());
        choices.set(client, choice);
        if let Err(error) = choices.save(running.gateway.home()) {
            Toast::error(t!(
                "Couldn't keep the choice: %{error}",
                error = error.to_string()
            ))
            .show(window, cx);
            return;
        }
        if client.connected(&client_home(cx)) {
            self.connect(client, window, cx);
        } else {
            self.sync(window, cx);
        }
    }

    /// Opens the editor on the chosen client's own servers, as if they'd been pasted, so what Patchbay will add (and
    /// the keys it takes out of them) is seen before anything changes.
    pub(super) fn start_taking_over(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(client) = self.client else {
            return;
        };
        let kept = own_server_names(cx);
        let Some(own) = client.own_servers_pasted(&client_home(cx), &kept) else {
            let said = if client.own_servers(&client_home(cx)).is_empty() {
                t!(
                    "%{client} has no servers of its own",
                    client = client.name()
                )
            } else {
                t!(
                    "Patchbay already has every server %{client} has",
                    client = client.name()
                )
            };
            Toast::new(said).show(window, cx);
            return;
        };
        if !own.left.is_empty() {
            Toast::new(t!(
                "Left %{servers} in %{client}, since Patchbay has a server by that name",
                servers = own.left.join(", "),
                client = client.name()
            ))
            .show(window, cx);
        }
        self.start_editing(Editing::New, window, cx);
        self.fill_paste(&own.pasted, window, cx);
        self.taking_over = Some(TakingOver {
            client,
            names: own.names,
        });
    }

    /// Takes the servers just added, `saved`, out of the client being taken over, and connects it if it wasn't, so
    /// it still has them. Only those saved: one taken out of the paste, or read under another name, stays. False if
    /// none of them was saved, so nothing was taken over.
    pub(super) fn finish_taking_over(
        &mut self,
        taking_over: TakingOver,
        saved: &[&str],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let client = taking_over.client;
        let taken: Vec<String> = taking_over
            .names
            .into_iter()
            .filter(|name| saved.contains(&name.as_str()))
            .collect();
        if taken.is_empty() {
            return false;
        }
        let home = client_home(cx);
        if let Err(error) = client.remove_own(&home, &taken) {
            Toast::error(t!(
                "Patchbay has them, but couldn't take them out of %{client}: %{error}",
                client = client.name(),
                error = error.to_string()
            ))
            .show(window, cx);
            return true;
        }
        Toast::new(t!(
            "Took over %{count} servers from %{client}: restart it to reach them through Patchbay",
            count = taken.len(),
            client = client.name()
        ))
        .key(format!("take-over-{}", client.slug()))
        .show(window, cx);
        if client.connected(&home) {
            self.sync(window, cx);
        } else {
            self.connect(client, window, cx);
        }
        true
    }

    pub(super) fn disconnect(
        &mut self,
        client: Client,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match client.disconnect(&client_home(cx)) {
            Ok(_) => Toast::new(t!("Took Patchbay out of %{client}", client = client.name()))
                .show(window, cx),
            Err(error) => Toast::error(t!(
                "Couldn't change %{client}: %{error}",
                client = client.name(),
                error = error.to_string()
            ))
            .show(window, cx),
        }
        self.sync(window, cx);
    }

    /// A client's layout and profile, as chips: what it's given once connected, or is given now.
    pub(super) fn choices(&self, row: &ClientRow, cx: &Context<Self>) -> Div {
        let client = row.client;
        let layouts = Layout::ALL.into_iter().map(|layout| {
            let label = match layout {
                Layout::OneEntry => t!("One entry"),
                Layout::PerServer => t!("An entry for each server"),
                Layout::Compact => t!("Compact"),
            };
            let choice = Choice {
                layout,
                ..row.choice.clone()
            };
            shell::chip(row.choice.layout == layout, cx)
                .id(SharedString::from(format!(
                    "layout-{}-{layout:?}",
                    client.slug()
                )))
                .text_sm()
                .child(label.into_owned())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.choose(client, choice.clone(), window, cx);
                }))
        });
        h_flex().gap_1().flex_wrap().children(layouts)
    }

    /// The servers `row`'s client is given, by name, as the gateway has its profile.
    pub(super) fn given(&self, row: &ClientRow) -> Vec<String> {
        self.profile_servers
            .get(&row.choice.profile)
            .cloned()
            .unwrap_or_default()
    }

    /// Which servers a client gets: every one, a profile's, or only those ticked, and the servers that are.
    fn server_choice(&self, row: &ClientRow, cx: &Context<Self>) -> Div {
        let client = row.client;
        let slug = client.slug();
        let current = row.choice.profile.as_str();
        let own = current == slug;
        let chip = |id: String, label: String, showing: bool, given: Given, cx: &Context<Self>| {
            shell::chip(showing, cx)
                .id(SharedString::from(id))
                .text_sm()
                .child(label)
                .on_click(cx.listener(move |this, _, window, cx| {
                    if !showing {
                        this.give(client, given.clone(), window, cx);
                    }
                }))
        };
        let named: Vec<&String> = self
            .profiles
            .iter()
            .filter(|profile| *profile != "default" && Client::from_slug(profile).is_none())
            .collect();
        let every: Vec<String> = self
            .servers
            .iter()
            .map(|server| server.name.clone())
            .collect();
        let chips = h_flex()
            .gap_1()
            .flex_wrap()
            .child(chip(
                format!("give-every-{slug}"),
                t!("Every server").into_owned(),
                current == "default",
                Given::Every,
                cx,
            ))
            .children(named.into_iter().map(|profile| {
                chip(
                    format!("give-profile-{slug}-{profile}"),
                    profile.clone(),
                    current == profile,
                    Given::Profile(profile.clone()),
                    cx,
                )
            }))
            .child(chip(
                format!("give-only-{slug}"),
                t!("Only these").into_owned(),
                own,
                Given::Only(every),
                cx,
            ));
        let says = match current {
            "default" => t!("Including any you add later.").into_owned(),
            _ if own => t!("A server you add later starts unticked.").into_owned(),
            profile => t!("As the %{profile} profile has them.", profile = profile).into_owned(),
        };
        let ticked = self.profile_servers.get(slug).cloned().unwrap_or_default();
        let given = self.given(row);
        let list = if own {
            v_flex().gap_1().children(self.servers.iter().map(|server| {
                let name = server.name.clone();
                h_flex()
                    .gap_2()
                    .child(
                        Checkbox::new(SharedString::from(format!("give-{slug}-{name}")))
                            .checked(ticked.contains(&name))
                            .label(name.clone())
                            .on_click(cx.listener(move |this, checked: &bool, window, cx| {
                                let mut servers =
                                    this.profile_servers.get(slug).cloned().unwrap_or_default();
                                servers.retain(|given| given != &name);
                                if *checked {
                                    servers.push(name.clone());
                                }
                                this.give(client, Given::Only(servers), window, cx);
                            })),
                    )
                    .child(servers::status_badge(&server.status, true))
            }))
        } else {
            v_flex()
                .gap_1()
                .children(
                    self.servers
                        .iter()
                        .filter(|server| given.contains(&server.name))
                        .map(|server| {
                            h_flex()
                                .gap_2()
                                .text_base()
                                .child(server.name.clone())
                                .child(servers::status_badge(&server.status, true))
                        }),
                )
                .when(given.is_empty(), |this| {
                    this.child(hint(t!("No servers yet."), cx))
                })
        };
        v_flex()
            .gap_1p5()
            .child(chips)
            .child(hint(says, cx))
            .child(list)
    }

    /// Gives `client` the servers `given` says: its own profile in the person's manifest for only some, kept as its
    /// choice. The gateway learns a new profile when it reads the manifest again, and a connected client's entries
    /// follow once it has.
    pub(super) fn give(
        &mut self,
        client: Client,
        given: Given,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(running) = cx.try_global::<Running>() else {
            return;
        };
        let home = running.gateway.home().clone();
        let slug = client.slug();
        let path = home.manifest();
        let old = std::fs::read_to_string(&path).unwrap_or_default();
        let (profile, changed) = match &given {
            Given::Every => ("default".to_string(), edit::remove_profile(&old, slug)),
            Given::Profile(profile) => (profile.clone(), Ok(old.clone())),
            Given::Only(servers) => (slug.to_string(), edit::set_profile(&old, slug, servers)),
        };
        let written = changed.map_err(|error| error.to_string()).and_then(|text| {
            if text == old {
                return Ok(());
            }
            patchbay_core::write_atomically(&path, text.as_bytes())
                .map_err(|error| error.to_string())
        });
        if let Err(error) = written {
            Toast::error(t!("Couldn't change your servers: %{error}", error = error))
                .show(window, cx);
            return;
        }
        if let Given::Only(servers) = &given {
            self.profile_servers
                .insert(slug.to_string(), servers.clone());
        }
        let mut choices = Choices::load(&home);
        let mut choice = choices.get(client);
        let moved = choice.profile != profile;
        choice.profile = profile;
        // The row follows at once, not when the gateway has read the manifest again, so a second change made in
        // the meantime (a quick second Cable on the patch bay) starts from this one.
        if let Some(row) = self.clients.iter_mut().find(|row| row.client == client) {
            row.choice = choice.clone();
        }
        choices.set(client, choice);
        if let Err(error) = choices.save(&home) {
            Toast::error(t!(
                "Couldn't keep the choice: %{error}",
                error = error.to_string()
            ))
            .show(window, cx);
            return;
        }
        let connected = client.connected(&client_home(cx));
        if running.background {
            let gateway = running.gateway.clone();
            let reloading = running.spawn(async move {
                gateway.reload().await;
                // Back to where they were: what reloading set idle starts again, as Reload does.
                let starting = gateway.clone();
                tokio::spawn(async move { starting.start_all().await });
            });
            cx.spawn_in(window, async move |this, cx| {
                let _ = reloading.await;
                let _ = this.update_in(cx, |this, window, cx| {
                    if connected && moved {
                        this.connect(client, window, cx);
                    } else {
                        this.sync(window, cx);
                    }
                });
            })
            .detach();
        } else {
            self.sync_clients(cx);
        }
        cx.notify();
    }
}

/// Which servers a client is given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Given {
    Every,
    Profile(String),
    /// These, by name, in the client's own profile.
    Only(Vec<String>),
}

/// The servers a client has of its own, and the button that brings them into Patchbay.
fn own_servers(row: &ClientRow, cx: &Context<MainView>) -> Div {
    v_flex()
        .gap_1p5()
        .child(
            h_flex().gap_1().flex_wrap().children(
                row.own
                    .iter()
                    .map(|name| shell::code_chip(name.clone(), cx)),
            ),
        )
        .child(hint(
            t!("Take Over adds them to Patchbay, with their keys in the Keychain, and takes them out of here."),
            cx,
        ))
        .child(
            h_flex().child(
                Button::new("take-over-servers")
                    .primary()
                    .xsmall()
                    .label(t!("Take Over").into_owned())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.run("clients.take-over", window, cx);
                    })),
            ),
        )
}

fn client_badge(row: &ClientRow) -> Tag {
    if row.connected() {
        shell::badge(Tag::success()).child(t!("Connected").into_owned())
    } else if row.installed {
        shell::badge(Tag::secondary()).child(t!("Not connected").into_owned())
    } else {
        shell::badge(Tag::secondary()).child(t!("Not found").into_owned())
    }
}

/// The entries `client`'s choice gives it on the running gateway, or why there are none.
fn entries_for(client: Client, cx: &App) -> Result<Vec<Entry>, String> {
    let running = cx
        .try_global::<Running>()
        .ok_or_else(|| "the gateway isn't running".to_string())?;
    let (Ok(addr), Some(token)) = (&running.listening, &running.token) else {
        return Err("the gateway isn't listening".to_string());
    };
    let choice = Choices::load(running.gateway.home()).get(client);
    let servers = running
        .gateway
        .profiles()
        .get(&choice.profile)
        .cloned()
        .ok_or_else(|| format!("there's no profile called {}", choice.profile))?;
    let binary = std::env::current_exe().map_err(|error| error.to_string())?;
    Ok(patchbay_gateway::client_entries(
        &choice,
        &servers,
        &patchbay_gateway::GatewayFile {
            port: addr.port(),
            pid: 0,
        },
        token,
        &binary,
        &patchbay_core::clients::bridge_env(),
    ))
}

/// The names of the servers in the person's own manifest.
fn own_server_names(cx: &App) -> Vec<String> {
    cx.try_global::<Running>()
        .and_then(|running| std::fs::read_to_string(running.gateway.home().manifest()).ok())
        .and_then(|text| patchbay_core::manifest::Manifest::from_toml(&text).ok())
        .map(|manifest| manifest.servers.into_keys().collect())
        .unwrap_or_default()
}
