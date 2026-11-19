//! Changing one of the person's own servers as a form: how it's reached, then its environment or headers a row each.
//! What the form doesn't show (tools, namespace, an OAuth client) is kept as it was; the manifest itself is a key
//! away for those.

use gpui_kit::component::switch::Switch;
use patchbay_core::form::{Reach, ServerFields};
use patchbay_core::manifest::ServerSpec;

use super::*;

/// A server's fields, being changed.
pub struct Form {
    pub(super) name: String,
    /// The server as it was, which what the form shows is laid over.
    original: ServerSpec,
    reach: Reaching,
    rows: Vec<Row>,
    /// The id the next row gets: rows are taken out as well as added, so a row's place doesn't name it.
    next_row: usize,
}

enum Reaching {
    Command {
        command: Entity<InputState>,
        args: Entity<TextareaState>,
        cwd: Entity<InputState>,
        _changed: [Subscription; 3],
    },
    Url {
        url: Entity<InputState>,
        sign_in: bool,
        _changed: Subscription,
    },
}

/// One variable or header: its name and its value.
struct Row {
    id: usize,
    key: Entity<InputState>,
    value: Entity<InputState>,
    _changed: [Subscription; 2],
}

/// A single-line field's height, fixed since the kit's fields fill what they're given.
const FIELD_HEIGHT: f32 = 24. / 13.;
/// The share of a row its name takes: names are short (`Authorization`, `GITHUB_TOKEN`), and values longer.
const KEY_SHARE: f32 = 0.35;

impl Form {
    /// The form showing the server `name`, as `spec` has it.
    pub(super) fn new(
        name: String,
        spec: ServerSpec,
        window: &mut Window,
        cx: &mut Context<MainView>,
    ) -> Form {
        let fields = ServerFields::of(&spec);
        let reach = match fields.reach {
            Reach::Command { command, args, cwd } => {
                let command = line(&command, "npx", window, cx);
                let args = cx.new(|cx| {
                    TextareaState::new(window, cx)
                        .auto_grow(2, 8)
                        .placeholder(t!("One a line").into_owned())
                        .default_value(args)
                });
                let cwd = line(&cwd, &t!("Where it runs, if it matters"), window, cx);
                let changed = [
                    watch_line(&command, window, cx),
                    watch_text(&args, cx),
                    watch_line(&cwd, window, cx),
                ];
                Reaching::Command {
                    command,
                    args,
                    cwd,
                    _changed: changed,
                }
            }
            Reach::Url { url, sign_in } => {
                let url = line(&url, "https://", window, cx);
                let changed = watch_line(&url, window, cx);
                Reaching::Url {
                    url,
                    sign_in,
                    _changed: changed,
                }
            }
        };
        let mut form = Form {
            name,
            original: spec,
            reach,
            rows: Vec::new(),
            next_row: 0,
        };
        for (key, value) in fields.pairs {
            form.push_row(&key, &value, window, cx);
        }
        form
    }

    fn push_row(
        &mut self,
        key: &str,
        value: &str,
        window: &mut Window,
        cx: &mut Context<MainView>,
    ) {
        let key = line(key, &t!("Name"), window, cx);
        let value = line(value, &t!("Value"), window, cx);
        let changed = [watch_line(&key, window, cx), watch_line(&value, window, cx)];
        self.rows.push(Row {
            id: self.next_row,
            key,
            value,
            _changed: changed,
        });
        self.next_row += 1;
    }

    /// What the fields say now, as the server it'd save.
    pub(super) fn read(&self, cx: &App) -> Result<Vec<Pasted>, String> {
        let text = |field: &Entity<InputState>| field.read(cx).value().to_string();
        let reach = match &self.reach {
            Reaching::Command {
                command, args, cwd, ..
            } => Reach::Command {
                command: text(command),
                args: args.read(cx).value().to_string(),
                cwd: text(cwd),
            },
            Reaching::Url { url, sign_in, .. } => Reach::Url {
                url: text(url),
                sign_in: *sign_in,
            },
        };
        let pairs = self
            .rows
            .iter()
            .map(|row| (text(&row.key), text(&row.value)))
            .collect();
        ServerFields { reach, pairs }
            .server(&self.name, &self.original)
            .map(|server| vec![server])
            .map_err(|error| error.to_string())
    }

    /// The last row's name and value fields.
    #[cfg(test)]
    pub(super) fn last_row(&self) -> Option<(Entity<InputState>, Entity<InputState>)> {
        self.rows
            .last()
            .map(|row| (row.key.clone(), row.value.clone()))
    }

    /// The field that takes the keyboard when the form opens.
    pub(super) fn focus(&self, window: &mut Window, cx: &mut App) {
        let first = match &self.reach {
            Reaching::Command { command, .. } => command,
            Reaching::Url { url, .. } => url,
        };
        first.read(cx).focus_handle(cx).focus(window, cx);
    }
}

/// A one-line field holding `value`.
fn line(
    value: &str,
    placeholder: &str,
    window: &mut Window,
    cx: &mut Context<MainView>,
) -> Entity<InputState> {
    cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(placeholder.to_string())
            .default_value(value.to_string())
    })
}

/// Reads the form again as a field changes, and saves it on Return.
fn watch_line(
    field: &Entity<InputState>,
    window: &mut Window,
    cx: &mut Context<MainView>,
) -> Subscription {
    cx.subscribe_in(
        field,
        window,
        |this, _, event: &InputEvent, window, cx| match event {
            InputEvent::Change => this.reread(cx),
            InputEvent::PressEnter { .. } => this.save_editor(window, cx),
            _ => {}
        },
    )
}

/// Reads the form again as a field of several lines changes; Return there starts a line.
fn watch_text(field: &Entity<TextareaState>, cx: &mut Context<MainView>) -> Subscription {
    cx.subscribe(field, |this, _, event: &InputEvent, cx| {
        if matches!(event, InputEvent::Change) {
            this.reread(cx);
        }
    })
}

impl MainView {
    fn form_mut(&mut self) -> Option<&mut Form> {
        match &mut self.editor.as_mut()?.body {
            editor::Body::Form(form) => Some(form),
            editor::Body::Paste { .. } => None,
        }
    }

    pub(super) fn add_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = self.form_mut() else {
            return;
        };
        form.push_row("", "", window, cx);
        if let Some(row) = form.rows.last() {
            row.key.read(cx).focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }

    fn remove_row(&mut self, id: usize, cx: &mut Context<Self>) {
        if let Some(form) = self.form_mut() {
            form.rows.retain(|row| row.id != id);
        }
        self.reread(cx);
    }

    pub(super) fn set_sign_in(&mut self, on: bool, cx: &mut Context<Self>) {
        if let Some(Form {
            reach: Reaching::Url { sign_in, .. },
            ..
        }) = self.form_mut()
        {
            *sign_in = on;
        }
        self.reread(cx);
    }

    /// The form's fields, each under its label: the panel can be narrow, and a row of a name and a value wants all
    /// its width.
    pub(super) fn form_fields(&self, form: &Form, cx: &Context<Self>) -> Div {
        let input = |field: &Entity<InputState>| Input::new(field).small().h(rems(FIELD_HEIGHT));
        let reach = match &form.reach {
            Reaching::Command {
                command, args, cwd, ..
            } => v_flex()
                .gap_3()
                .child(labelled(t!("Command"), input(command), cx))
                .child(labelled(
                    t!("Arguments"),
                    div()
                        .rounded(cx.theme().radius)
                        .border_1()
                        .border_color(cx.theme().input)
                        .px_2()
                        .py_1()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_base()
                        .child(Textarea::new(args).appearance(false)),
                    cx,
                ))
                .child(labelled(t!("Folder"), input(cwd), cx)),
            Reaching::Url { url, sign_in, .. } => v_flex()
                .gap_3()
                .child(labelled(t!("URL"), input(url), cx))
                .child(labelled(
                    t!("Sign-in"),
                    v_flex()
                        .gap_1()
                        .child(
                            Switch::new("sign-in")
                                .label(t!("Signs in with the browser").into_owned())
                                .checked(*sign_in)
                                .on_click(cx.listener(|this, on: &bool, _, cx| {
                                    this.set_sign_in(*on, cx);
                                })),
                        )
                        .child(hint(
                            t!("For a server that asks you to sign in rather than for a key."),
                            cx,
                        )),
                    cx,
                )),
        };
        let (label, add) = match form.reach {
            Reaching::Command { .. } => (t!("Environment"), t!("Add Variable")),
            Reaching::Url { .. } => (t!("Headers"), t!("Add Header")),
        };
        let rows = v_flex()
            .gap_2()
            .children(form.rows.iter().map(|row| {
                let id = row.id;
                h_flex()
                    .gap_2()
                    .child(div().flex_none().w(relative(KEY_SHARE)).child(input(&row.key)))
                    .child(div().flex_1().min_w_0().child(input(&row.value)))
                    .child(
                        Button::new(("remove-row", id))
                            .ghost()
                            .xsmall()
                            .label("✕")
                            .tooltip(t!("Remove").into_owned())
                            .on_click(cx.listener(move |this, _, _, cx| this.remove_row(id, cx))),
                    )
            }))
            .child(
                h_flex().child(
                    Button::new("add-row")
                        .ghost()
                        .xsmall()
                        .label(add.into_owned())
                        .on_click(cx.listener(|this, _, window, cx| this.add_row(window, cx))),
                ),
            )
            .child(hint(
                t!("Type a key as it is and Patchbay keeps it in the Keychain. ${secret:name} stands for one it already has."),
                cx,
            ));
        v_flex()
            .gap_4()
            .child(reach)
            .child(labelled(label, rows, cx))
            .child(
                v_flex()
                    .items_start()
                    .gap_1()
                    .child(hint(
                        t!("Its tools, namespace and name are in your manifest."),
                        cx,
                    ))
                    .child(
                        Button::new("open-manifest")
                            .link()
                            .xsmall()
                            .label(t!("Edit patchbay.toml…").into_owned())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.run("servers.edit", window, cx);
                            })),
                    ),
            )
    }
}

/// A field of the form: its label, small, over what it holds.
fn labelled(label: impl Into<SharedString>, content: impl IntoElement, cx: &App) -> Div {
    v_flex()
        .gap_1()
        .child(
            div()
                .text_base()
                .text_color(cx.theme().muted_foreground)
                .child(label.into()),
        )
        .child(content)
}
