//! Servers for someone else, and from them: exporting some of the person's own as a manifest (copied or saved to a
//! file), opening a file into the paste box, and turning any server off and on, a team's included.

use std::collections::BTreeSet;
use std::path::PathBuf;

use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::switch::Switch;
use patchbay_core::manifest::edit;
use patchbay_core::share;

use super::*;

/// Servers being chosen to export.
pub struct Exporting {
    /// The person's own servers that go in it, by name.
    pub chosen: BTreeSet<String>,
    /// The team it says it is, if any: then it can be added as that team's.
    team: Entity<InputState>,
}

/// A single-line field's height, fixed since the kit's fields fill what they're given.
const FIELD_HEIGHT: f32 = 24. / 13.;

impl MainView {
    /// The person's own servers, which are the ones they can export.
    fn own_servers(&self) -> impl Iterator<Item = &ServerStatus> {
        self.servers.iter().filter(|server| server.team.is_none())
    }

    /// Shows the export in place of the chosen server, with every one of the person's own servers in it.
    pub(super) fn start_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let team = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("Leave it empty to give them as servers").into_owned())
        });
        self.editor = None;
        self.place = Place::Servers;
        self.exporting = Some(Exporting {
            chosen: self
                .own_servers()
                .map(|server| server.name.clone())
                .collect(),
            team,
        });
        cx.notify();
    }

    pub(super) fn cancel_export(&mut self, cx: &mut Context<Self>) {
        self.exporting = None;
        cx.notify();
    }

    pub(super) fn toggle_exported(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some(exporting) = &mut self.exporting
            && !exporting.chosen.remove(name)
        {
            exporting.chosen.insert(name.to_string());
        }
        cx.notify();
    }

    /// The manifest the export makes now, and the team it names.
    pub(super) fn exported(&self, cx: &App) -> Option<(String, Option<String>)> {
        let exporting = self.exporting.as_ref()?;
        let running = cx.try_global::<Running>()?;
        let text = std::fs::read_to_string(running.gateway.home().manifest()).ok()?;
        let team = Some(exporting.team.read(cx).value().trim().to_string())
            .filter(|team| !team.is_empty());
        // In the list's order, which is the order they're shown in.
        let names: Vec<String> = self
            .own_servers()
            .filter(|server| exporting.chosen.contains(&server.name))
            .map(|server| server.name.clone())
            .collect();
        let exported = share::export(&text, &names, team.as_deref()).ok()?;
        Some((exported, team))
    }

    pub(super) fn copy_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((text, _)) = self.exported(cx) else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.exporting = None;
        Toast::new(t!("Copied. They can paste it into Add Server or Add Team."))
            .key("export")
            .show(window, cx);
        cx.notify();
    }

    /// Asks where to save the export, then writes it there.
    pub(super) fn save_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((text, team)) = self.exported(cx) else {
            return;
        };
        let suggested = match team.as_deref().map(share::folder) {
            Some(Ok(folder)) => format!("{folder}.patchbay.toml"),
            _ => "patchbay.toml".to_string(),
        };
        let folder = std::env::var_os("HOME").map_or_else(
            || PathBuf::from("/"),
            |home| PathBuf::from(home).join("Desktop"),
        );
        let chosen = cx.prompt_for_new_path(&folder, Some(&suggested));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = chosen.await else {
                return;
            };
            let written = write_export(&path, &text);
            let _ = this.update_in(cx, |this, window, cx| {
                match written {
                    Ok(()) => {
                        this.exporting = None;
                        Toast::new(t!("Saved %{file}", file = path.display().to_string()))
                            .key("export")
                            .show(window, cx);
                    }
                    Err(error) => Toast::error(t!(
                        "Couldn't save it: %{error}",
                        error = format!("{error:#}")
                    ))
                    .show(window, cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Asks for a manifest or a server's config, and puts what's in it in the paste box.
    pub(super) fn open_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(t!("Open").into_owned().into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let read = std::fs::read_to_string(&path);
            let _ = this.update_in(cx, |this, window, cx| match read {
                Ok(text) => this.fill_paste(&text, window, cx),
                Err(error) => {
                    Toast::error(t!("Couldn't open it: %{error}", error = error.to_string()))
                        .show(window, cx)
                }
            });
        })
        .detach();
    }

    /// Turns a server on or off in the person's own manifest, whoever's it is. A team's is left as the team wrote it.
    pub(super) fn set_enabled(
        &mut self,
        name: &str,
        on: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(running) = cx.try_global::<Running>() else {
            return;
        };
        let path = running.gateway.home().manifest();
        let old = std::fs::read_to_string(&path).unwrap_or_default();
        let written = edit::set_enabled(&old, name, on)
            .map_err(|error| error.to_string())
            .and_then(|text| {
                patchbay_core::write_atomically(&path, text.as_bytes())
                    .map_err(|error| error.to_string())
            });
        match written {
            Ok(()) => reload_gateway(cx),
            Err(error) => Toast::error(t!("Couldn't change your servers: %{error}", error = error))
                .show(window, cx),
        }
    }

    /// Takes out the team the chosen server is from, every server of it with it. Undo puts it back.
    pub(super) fn remove_chosen_team(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let team = self
            .server
            .as_ref()
            .and_then(|name| self.servers.iter().find(|server| &server.name == name))
            .and_then(|server| server.team.clone());
        let Some(team) = team else {
            Toast::new(t!("Choose one of a team's servers first"))
                .key("remove-team")
                .show(window, cx);
            return;
        };
        let Some(running) = cx.try_global::<Running>() else {
            return;
        };
        let home = running.gateway.home().clone();
        match share::remove(&home, &team) {
            Ok(removed) => {
                reload_gateway(cx);
                // As its `[team]` names it, which reads better than the folder it was kept in.
                let named = patchbay_core::manifest::Manifest::from_toml(&removed.team)
                    .ok()
                    .and_then(|manifest| manifest.team)
                    .map_or(team, |named| named.name);
                Toast::new(t!("Removed the %{team} team", team = named))
                    .key("remove-team")
                    .undo(move |_, cx| {
                        if share::restore(&home, &removed).is_ok() {
                            reload_gateway(cx);
                        }
                    })
                    .show(window, cx);
            }
            Err(error) => Toast::error(t!(
                "Couldn't remove the team: %{error}",
                error = error.to_string()
            ))
            .show(window, cx),
        }
    }

    /// The switch that turns a server on and off, as its list row shows it.
    pub(super) fn enabled_switch(&self, server: &ServerStatus, cx: &Context<Self>) -> Switch {
        let name = server.name.clone();
        let on = !matches!(server.status, Status::Off);
        Switch::new(SharedString::from(format!("enabled-{name}")))
            .checked(on)
            .tooltip(if on {
                t!("Turn Off").into_owned()
            } else {
                t!("Turn On").into_owned()
            })
            .on_click(cx.listener(move |this, on: &bool, window, cx| {
                this.set_enabled(&name, *on, window, cx);
            }))
    }

    pub(super) fn export_panel(&self, exporting: &Exporting, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let nothing = exporting.chosen.is_empty();
        let header = self.panel_header(t!("Export Servers"), cx).child(
            h_flex()
                .gap_1()
                .child(
                    Button::new("cancel-export")
                        .ghost()
                        .xsmall()
                        .label(t!("Cancel").into_owned())
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_export(cx))),
                )
                .child(
                    Button::new("copy-export")
                        .ghost()
                        .xsmall()
                        .disabled(nothing)
                        .label(t!("Copy").into_owned())
                        .on_click(cx.listener(|this, _, window, cx| this.copy_export(window, cx))),
                )
                .child(
                    Button::new("save-export")
                        .primary()
                        .xsmall()
                        .disabled(nothing)
                        .label(t!("Save…").into_owned())
                        .on_click(cx.listener(|this, _, window, cx| this.save_export(window, cx))),
                ),
        );
        let servers = v_flex().gap_2().children(self.own_servers().map(|server| {
            let name = server.name.clone();
            Checkbox::new(SharedString::from(format!("export-{name}")))
                .label(name.clone())
                .checked(exporting.chosen.contains(&name))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_exported(&name, cx)))
        }));
        let body = v_flex()
            .gap_4()
            .p_4()
            .child(section(t!("Servers"), cx).child(servers))
            .child(
                section(t!("Team"), cx)
                    .child(Input::new(&exporting.team).small().h(rems(FIELD_HEIGHT)))
                    .child(hint(
                        t!("Named, it's added as that team's: theirs to turn off, and kept apart from their own servers."),
                        cx,
                    )),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(t!("Keys aren't in it, only what each one is and where to get it. They give their own.").into_owned()),
            );
        v_flex()
            .size_full()
            .child(header)
            .child(div().id("export").flex_1().overflow_y_scroll().child(body))
            .into_any_element()
    }
}

/// Writes an export the person chose a place for. Only a new file or one they chose to replace, so it's written as is.
fn write_export(path: &std::path::Path, text: &str) -> anyhow::Result<()> {
    use anyhow::Context as _;
    patchbay_core::write_atomically(path, text.as_bytes())
        .with_context(|| format!("writing {}", path.display()))
}
