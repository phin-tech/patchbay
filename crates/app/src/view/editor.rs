//! Adding a server by pasting what its README gives (a JSON block, an `mcp add` or `npx` command, a URL), adding a
//! team's manifest someone sent as a layer of its own, or changing a server in a form. What's pasted or typed is read as it changes, and shown as what Patchbay will save before anything
//! is.

use patchbay_core::manifest::{AuthKind, Manifest, Scope, edit};
use patchbay_core::{paste, share};

use super::form::Form;
use super::*;

/// What the editor is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Editing {
    New,
    /// A team's manifest, pasted or opened, kept as a layer under the person's own.
    Team,
    /// The person's own server, by name, shown as a form.
    Server(String),
}

/// A server being added or changed.
pub struct Editor {
    pub mode: Editing,
    pub(super) body: Body,
    /// What the editor reads as now, or why it doesn't read as a server.
    pub read: Result<Vec<Pasted>, String>,
}

/// Where the server is given: pasted, for a new one or a team, or field by field, for one that's there.
pub(super) enum Body {
    Paste {
        field: Entity<TextareaState>,
        /// For a team, its name, when what's pasted doesn't give one or it should be another.
        team: Option<Entity<InputState>>,
        _changed: Vec<Subscription>,
    },
    Form(Box<Form>),
}

impl MainView {
    /// Shows the editor in place of the chosen server: an empty paste box for a new one, or a server's form.
    pub(super) fn start_editing(
        &mut self,
        mode: Editing,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let body = match &mode {
            Editing::New | Editing::Team => {
                let placeholder = match mode {
                    Editing::Team => t!("Paste the team's patchbay.toml, or open it"),
                    _ => t!("Paste its JSON, its command or its URL"),
                };
                let field = cx.new(|cx| {
                    TextareaState::new(window, cx)
                        .auto_grow(8, 18)
                        .placeholder(placeholder.into_owned())
                });
                let mut changed = vec![cx.subscribe(&field, |this, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.reread(cx);
                    }
                })];
                let team = (mode == Editing::Team).then(|| {
                    let name = cx.new(|cx| {
                        InputState::new(window, cx).placeholder(
                            t!("The name in its [team], or one you give it").into_owned(),
                        )
                    });
                    changed.push(cx.subscribe(&name, |this, _, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Change) {
                            this.reread(cx);
                        }
                    }));
                    name
                });
                field.read(cx).focus_handle(cx).focus(window, cx);
                Body::Paste {
                    field,
                    team,
                    _changed: changed,
                }
            }
            Editing::Server(name) => {
                let spec = cx
                    .try_global::<Running>()
                    .and_then(|running| {
                        std::fs::read_to_string(running.gateway.home().manifest()).ok()
                    })
                    .and_then(|text| Manifest::from_toml(&text).ok())
                    .and_then(|mut manifest| manifest.servers.remove(name));
                let Some(spec) = spec else {
                    Toast::error(t!(
                        "Couldn't read %{server} from your manifest",
                        server = name
                    ))
                    .show(window, cx);
                    return;
                };
                let form = Form::new(name.clone(), spec, window, cx);
                form.focus(window, cx);
                Body::Form(Box::new(form))
            }
        };
        let read = read(&body, cx);
        self.exporting = None;
        self.place = Place::Servers;
        self.editor = Some(Editor { mode, body, read });
        cx.notify();
    }

    /// Reads what the editor holds again.
    pub(super) fn reread(&mut self, cx: &mut Context<Self>) {
        if let Some(editor) = &mut self.editor {
            editor.read = read(&editor.body, cx);
            cx.notify();
        }
    }

    /// Puts `text` in the paste box, as if it had been pasted there.
    pub(super) fn fill_paste(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(Editor {
            body: Body::Paste { field, .. },
            ..
        }) = &self.editor
        {
            field.update(cx, |field, cx| {
                field.set_value(text.to_string(), window, cx)
            });
        }
        self.reread(cx);
    }

    pub(super) fn cancel_editing(&mut self, cx: &mut Context<Self>) {
        self.editor = None;
        cx.notify();
    }

    /// Puts what the editor reads into the person's manifest (in place of the server being changed), keeps the keys
    /// it held in the Keychain, and has the gateway read the manifests again.
    pub(super) fn save_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Editor {
            mode,
            read: Ok(servers),
            ..
        }) = &self.editor
        else {
            return;
        };
        let (mode, servers) = (mode.clone(), servers.clone());
        if mode == Editing::Team {
            self.save_team(window, cx);
            return;
        }
        let Some(running) = cx.try_global::<Running>() else {
            return;
        };
        let path = running.gateway.home().manifest();
        let old = std::fs::read_to_string(&path).unwrap_or_default();
        // A server changed in the form keeps its name, so adding it replaces it where it is.
        let changed = edit::add(&old, &servers)
            .map_err(|error| error.to_string())
            .and_then(|text| {
                if let Some(folder) = path.parent() {
                    let _ = std::fs::create_dir_all(folder);
                }
                patchbay_core::write_atomically(&path, text.as_bytes())
                    .map_err(|error| error.to_string())
            });
        if let Err(error) = changed {
            Toast::error(t!("Couldn't change your servers: %{error}", error = error))
                .show(window, cx);
            return;
        }
        let secrets = running.gateway.secrets().clone();
        for secret in servers.iter().flat_map(|server| &server.secrets) {
            if let Some(value) = &secret.value
                && let Err(error) = secrets.set(&Scope::Me.account(&secret.name), value)
            {
                Toast::error(t!(
                    "Couldn't keep a key: %{error}",
                    error = error.to_string()
                ))
                .show(window, cx);
            }
        }
        reload_gateway(cx);
        let names: Vec<&str> = servers.iter().map(|server| server.name.as_str()).collect();
        let said = match mode {
            Editing::New | Editing::Team => t!("Added %{servers}", servers = names.join(", ")),
            Editing::Server(_) => t!("Saved %{servers}", servers = names.join(", ")),
        };
        self.server = names.first().map(|name| (*name).to_string());
        self.editor = None;
        Toast::new(said)
            .key("added")
            .undo(move |_, cx| {
                if patchbay_core::write_atomically(&path, old.as_bytes()).is_ok() {
                    reload_gateway(cx);
                }
            })
            .show(window, cx);
        cx.notify();
    }

    /// Keeps the pasted team's manifest as its own layer, named as the person said, and has the gateway read it.
    /// Undo puts back the team that was there before, or takes the new one out.
    fn save_team(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Editor { body, .. }) = &self.editor else {
            return;
        };
        let Some(running) = cx.try_global::<Running>() else {
            return;
        };
        let home = running.gateway.home().clone();
        let team = pasted_team(body, cx).and_then(|(name, folder, text)| {
            let named = share::named(&text, &name).map_err(|error| error.to_string())?;
            Ok((name, folder, named))
        });
        let (name, folder, text) = match team {
            Ok(team) => team,
            Err(error) => {
                Toast::error(error).show(window, cx);
                return;
            }
        };
        let path = share::manifest_path(&home, &folder);
        let old = std::fs::read_to_string(&path).ok();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(error) = patchbay_core::write_atomically(&path, text.as_bytes()) {
            Toast::error(t!(
                "Couldn't add the team: %{error}",
                error = error.to_string()
            ))
            .show(window, cx);
            return;
        }
        reload_gateway(cx);
        self.editor = None;
        Toast::new(t!("Added %{team}'s servers", team = name))
            .key("added")
            .undo(move |_, cx| {
                let undone = match &old {
                    Some(old) => patchbay_core::write_atomically(&path, old.as_bytes()).is_ok(),
                    None => std::fs::remove_file(&path).is_ok(),
                };
                if undone {
                    reload_gateway(cx);
                }
            })
            .show(window, cx);
        cx.notify();
    }

    pub(super) fn editor_panel(&self, editor: &Editor, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let title = match &editor.mode {
            Editing::New => t!("Add a Server").into_owned(),
            Editing::Team => t!("Add a Team").into_owned(),
            Editing::Server(name) => t!("Edit %{server}", server = name).into_owned(),
        };
        let count = editor.read.as_ref().map_or(0, Vec::len);
        let save_label = match (&editor.mode, count) {
            (Editing::Server(_), _) => t!("Save").into_owned(),
            (Editing::Team, _) => t!("Add Team").into_owned(),
            (Editing::New, 0 | 1) => t!("Add Server").into_owned(),
            (Editing::New, count) => t!("Add %{count} Servers", count = count).into_owned(),
        };
        let header = self.panel_header(title, cx).child(
            h_flex()
                .gap_1()
                .when(matches!(editor.body, Body::Paste { .. }), |row| {
                    row.child(
                        Button::new("open-file")
                            .ghost()
                            .xsmall()
                            .label(t!("Open File…").into_owned())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_file(window, cx)),
                            ),
                    )
                })
                .child(
                    Button::new("cancel-edit")
                        .ghost()
                        .xsmall()
                        .label(t!("Cancel").into_owned())
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_editing(cx))),
                )
                .child(
                    Button::new("save-edit")
                        .primary()
                        .xsmall()
                        .disabled(editor.read.is_err())
                        .label(save_label)
                        .on_click(cx.listener(|this, _, window, cx| this.save_editor(window, cx))),
                ),
        );
        let preview = match &editor.read {
            Ok(servers) => v_flex().gap_3().children(
                servers
                    .iter()
                    .map(|server| self.pasted(server, &editor.mode, cx)),
            ),
            Err(reason) => v_flex().child(
                div()
                    .text_base()
                    .text_color(theme.muted_foreground)
                    .child(reason.clone()),
            ),
        };
        let body = match &editor.body {
            Body::Paste { field, team, .. } => v_flex()
                .gap_5()
                .p_4()
                .when_some(team.as_ref(), |body, team| {
                    let replacing = pasted_team(&editor.body, cx)
                        .ok()
                        .filter(|(_, folder, _)| {
                            self.servers.iter().any(|server| server.team.as_deref() == Some(folder.as_str()))
                        })
                        .map(|(name, _, _)| name);
                    body.child(
                        section(t!("Team"), cx)
                            .child(Input::new(team).small().h(rems(24. / 13.)))
                            .when_some(replacing, |section, name| {
                                section.child(
                                    div()
                                        .text_base()
                                        .text_color(theme.warning)
                                        .child(t!("Replaces the %{team} you have", team = name).into_owned()),
                                )
                            }),
                    )
                })
                .child(
                    div()
                        .rounded(theme.radius)
                        .border_1()
                        .border_color(theme.border)
                        .p_2()
                        .font_family(theme.mono_font_family.clone())
                        .text_base()
                        .child(Textarea::new(field).appearance(false)),
                )
                .child(section(t!("What Patchbay will add"), cx).child(preview))
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(match team {
                            Some(_) => t!("A team's servers are kept apart from yours, and its keys too. Turn off any you don't want.")
                                .into_owned(),
                            None => t!("Keys in what you paste go into the Keychain; your manifest only names them.")
                                .into_owned(),
                        }),
                ),
            Body::Form(form) => {
                // Sign-in has its own switch in the form, so only what else saving does is said under it.
                let (notes, color) = match &editor.read {
                    Ok(servers) => (
                        servers
                            .iter()
                            .flat_map(|server| self.notes(server, &editor.mode))
                            .filter(|note| !note.starts_with(&*t!("Signs in")))
                            .collect(),
                        theme.muted_foreground,
                    ),
                    Err(reason) => (vec![reason.clone()], theme.danger),
                };
                v_flex()
                    .gap_4()
                    .p_4()
                    .child(self.form_fields(form, cx))
                    .children(
                        notes
                            .into_iter()
                            .map(|note: String| div().text_base().text_color(color).child(note)),
                    )
            }
        };
        v_flex()
            .size_full()
            .child(header)
            .child(div().id("editor").flex_1().overflow_y_scroll().child(body))
            .into_any_element()
    }

    /// One server the editor reads, as it'll be added.
    fn pasted(&self, server: &Pasted, mode: &Editing, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        let spec = &server.spec;
        let target = match (&spec.url, &spec.command) {
            (Some(url), _) => url.clone(),
            (None, Some(command)) => std::iter::once(command.clone())
                .chain(spec.args.iter().flatten().cloned())
                .collect::<Vec<_>>()
                .join(" "),
            (None, None) => String::new(),
        };
        v_flex()
            .gap_1()
            .p_3()
            .rounded(theme.radius)
            .bg(theme.group_box)
            .child(
                h_flex()
                    .gap_2()
                    .child(div().font_semibold().child(server.name.clone()))
                    .child(shell::code_chip(target, cx)),
            )
            .children(self.notes(server, mode).into_iter().map(|note| {
                div()
                    .text_base()
                    .text_color(if note.starts_with(&*t!("Replaces")) {
                        theme.warning
                    } else {
                        theme.muted_foreground
                    })
                    .child(note)
            }))
    }

    /// What saving `server` does beyond writing it: signs in, keeps keys, replaces one that's there.
    fn notes(&self, server: &Pasted, mode: &Editing) -> Vec<String> {
        let spec = &server.spec;
        let replaces = *mode == Editing::New
            && self
                .servers
                .iter()
                .any(|existing| existing.name == server.name);
        // Layers combine a server of the same name field by field, rather than one taking the other's place.
        let combines = *mode == Editing::Team
            && self
                .servers
                .iter()
                .any(|existing| existing.name == server.name && existing.team.is_none());
        let mut notes: Vec<String> = Vec::new();
        if spec.auth == Some(AuthKind::Oauth) {
            notes.push(t!("Signs in with your browser once it's added").into_owned());
        }
        for secret in &server.secrets {
            let prompt = secret
                .spec
                .prompt
                .clone()
                .unwrap_or_else(|| secret.name.clone());
            notes.push(match &secret.value {
                Some(_) => t!("Keeps %{secret} in the Keychain", secret = prompt).into_owned(),
                None => t!("Asks you for %{secret}", secret = prompt).into_owned(),
            });
        }
        if replaces {
            notes.push(t!("Replaces the %{server} you have", server = server.name).into_owned());
        }
        if combines {
            notes.push(
                t!(
                    "Replaces part of it: your own %{server} keeps what it sets",
                    server = server.name
                )
                .into_owned(),
            );
        }
        notes
    }
}

/// What `body` reads as: the servers pasted, or the one the form makes.
fn read(body: &Body, cx: &App) -> Result<Vec<Pasted>, String> {
    match body {
        Body::Paste {
            field, team: None, ..
        } => paste::read(&field.read(cx).value()).map_err(|error| error.to_string()),
        Body::Paste { team: Some(_), .. } => {
            let (_, _, text) = pasted_team(body, cx)?;
            share::read(&text)
                .map(|team| team.servers)
                .map_err(|error| error.to_string())
        }
        Body::Form(form) => form.read(cx),
    }
}

/// The team pasted: its name (as given, or its own), the folder it's kept in, and its manifest as pasted.
fn pasted_team(body: &Body, cx: &App) -> Result<(String, String, String), String> {
    let Body::Paste {
        field,
        team: Some(name),
        ..
    } = body
    else {
        return Err(String::new());
    };
    let text = field.read(cx).value().to_string();
    let team = share::read(&text).map_err(|error| error.to_string())?;
    let given = name.read(cx).value().trim().to_string();
    let name = if given.is_empty() {
        team.name.unwrap_or_default()
    } else {
        given
    };
    let folder = share::folder(&name).map_err(|error| error.to_string())?;
    Ok((name, folder, text))
}
