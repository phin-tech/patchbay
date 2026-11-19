//! Import: every server the person's clients already keep, for them and in each Claude Code project, and every
//! skill of their own, listed for them to go through one by one. Nothing is ticked to begin with: a server comes in
//! only once it's ticked, and leaves a client only where "take it out" is ticked too. The servers go through Add
//! Server, as a paste, so the keys it would keep are shown before anything is added.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::radio::Radio;
use patchbay_core::import::{Found, Gathered, Kept, Known, Origin, Summary, gather, pasted};
use patchbay_core::skills::Skill;

use super::clients::TakingOver;
use super::skills::Which;
use super::*;

/// What's being chosen to import.
pub struct Importing {
    gathered: Gathered,
    /// The servers to bring in, by name, with which of its versions.
    chosen: BTreeMap<String, usize>,
    /// The copies to take out once Patchbay has them, by where they are and the server's name.
    out: BTreeSet<(Origin, String)>,
    /// The person's own skills to take over, by folder.
    skills: BTreeSet<String>,
}

impl Importing {
    /// Whether nothing at all is ticked, as Import begins.
    #[cfg(test)]
    pub(super) fn nothing_ticked(&self) -> bool {
        self.chosen.is_empty() && self.out.is_empty() && self.skills.is_empty()
    }

    /// What's ticked, as Import would do it: the servers with their versions, and where each comes out of.
    fn servers(&self) -> Vec<(&Found, usize)> {
        self.gathered
            .found
            .iter()
            .filter_map(|found| Some((found, *self.chosen.get(&found.name)?)))
            .collect()
    }
}

impl MainView {
    /// Shows Import in place of the chosen server, with everything the clients have and nothing ticked.
    pub(super) fn start_import(&mut self, cx: &mut Context<Self>) {
        let known: Vec<Known> = self
            .servers
            .iter()
            .map(|server| Known {
                name: server.name.clone(),
                target: server.target.clone(),
            })
            .collect();
        self.editor = None;
        self.exporting = None;
        self.teams.subscribing = None;
        self.teams.reviewing = None;
        self.place = Place::Servers;
        self.importing = Some(Importing {
            gathered: gather(&client_home(cx), &known),
            chosen: BTreeMap::new(),
            out: BTreeSet::new(),
            skills: BTreeSet::new(),
        });
        cx.notify();
    }

    pub(super) fn cancel_import(&mut self, cx: &mut Context<Self>) {
        self.importing = None;
        cx.notify();
    }

    /// Ticks a server to bring in, with its first version, or unticks it and every copy that was to come out.
    pub(super) fn toggle_import(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some(importing) = &mut self.importing {
            if importing.chosen.remove(name).is_some() {
                importing.out.retain(|(_, out)| out != name);
            } else {
                importing.chosen.insert(name.to_string(), 0);
            }
        }
        cx.notify();
    }

    /// Brings in `name` as its `version`, ticking it if it wasn't.
    fn choose_version(&mut self, name: &str, version: usize, cx: &mut Context<Self>) {
        if let Some(importing) = &mut self.importing {
            importing.chosen.insert(name.to_string(), version);
        }
        cx.notify();
    }

    pub(super) fn toggle_out(&mut self, origin: &Origin, name: &str, cx: &mut Context<Self>) {
        if let Some(importing) = &mut self.importing {
            let key = (origin.clone(), name.to_string());
            if !importing.out.remove(&key) {
                importing.out.insert(key);
            }
        }
        cx.notify();
    }

    fn toggle_import_skill(&mut self, folder: &str, cx: &mut Context<Self>) {
        if let Some(importing) = &mut self.importing
            && !importing.skills.remove(folder)
        {
            importing.skills.insert(folder.to_string());
        }
        cx.notify();
    }

    /// Takes over the skills ticked, and opens the servers ticked in Add Server, to be added from there.
    pub(super) fn run_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(importing) = self.importing.take() else {
            return;
        };
        if !importing.skills.is_empty() {
            let folders = importing.skills.iter().cloned().collect();
            self.take_over_skills(Which::These(folders), window, cx);
        }
        let servers = importing.servers();
        if servers.is_empty() {
            cx.notify();
            return;
        }
        let pasted = pasted(&servers);
        let taken = importing.out.iter().cloned().collect();
        self.start_editing(Editing::New, window, cx);
        self.fill_paste(&pasted, window, cx);
        self.taking_over = Some(TakingOver { taken });
    }

    pub(super) fn import_panel(&self, importing: &Importing, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let servers = importing.servers();
        let nothing = servers.is_empty() && importing.skills.is_empty();
        let label = match (servers.len(), importing.skills.len()) {
            (0, 0) => t!("Import").into_owned(),
            (1, 0) => t!("Import 1 Server…").into_owned(),
            (servers, 0) => t!("Import %{count} Servers…", count = servers).into_owned(),
            (0, 1) => t!("Import 1 Skill").into_owned(),
            (0, skills) => t!("Import %{count} Skills", count = skills).into_owned(),
            (servers, skills) => t!("Import %{count}…", count = servers + skills).into_owned(),
        };
        let header = self.panel_header(t!("Import from Your Clients"), cx).child(
            h_flex()
                .flex_none()
                .gap_1()
                .child(
                    Button::new("cancel-import")
                        .ghost()
                        .xsmall()
                        .label(t!("Cancel").into_owned())
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_import(cx))),
                )
                .child(
                    Button::new("import")
                        .primary()
                        .xsmall()
                        .disabled(nothing)
                        .label(label)
                        .on_click(cx.listener(|this, _, window, cx| this.run_import(window, cx))),
                ),
        );
        let home = client_home(cx);
        let (new, had): (Vec<&Found>, Vec<&Found>) = importing
            .gathered
            .found
            .iter()
            .partition(|found| found.patchbay_has.is_none());
        let mut leaving: Vec<&str> = importing
            .out
            .iter()
            .map(|(origin, _)| origin.client.name())
            .collect();
        leaving.sort_unstable();
        leaving.dedup();
        let own_skills: Vec<&Skill> = self
            .skills
            .list
            .iter()
            .filter(|skill| !skill.managed())
            .collect();
        let body = v_flex()
            .gap_5()
            .p_4()
            .min_w_0()
            .child(hint(
                t!("Tick what to bring into Patchbay. Nothing leaves a client unless you choose where to take it out of, and a client a server leaves is connected to Patchbay, so it still has it."),
                cx,
            ))
            .children(importing.gathered.unreadable.iter().map(|problem| {
                div()
                    .text_sm()
                    .text_color(theme.danger)
                    .child(t!("Couldn't read %{file}", file = problem.clone()).into_owned())
            }))
            .child(
                section(t!("Servers"), cx)
                    .when(new.is_empty(), |this| {
                        this.child(hint(t!("Your clients have no servers Patchbay doesn't have already."), cx))
                    })
                    .children(new.iter().map(|found| self.import_card(found, importing, &home, cx))),
            )
            .when(!own_skills.is_empty(), |this| {
                this.child(
                    section(t!("Your own skills"), cx)
                        .child(hint(
                            t!("Taken over, a skill is kept once and each client's copy becomes a link to it, where it's the same."),
                            cx,
                        ))
                        .children(own_skills.iter().map(|skill| self.skill_card(skill, importing, cx))),
                )
            })
            .when(!had.is_empty(), |this| {
                let names: Vec<String> = had
                    .iter()
                    .map(|found| match &found.patchbay_has {
                        Some(has) if *has != found.name => format!("{} ({has})", found.name),
                        _ => found.name.clone(),
                    })
                    .collect();
                this.child(section(t!("Patchbay has these already"), cx).child(hint(names.join(", "), cx)))
            })
            .when(!leaving.is_empty(), |this| {
                this.child(hint(
                    t!(
                        "%{clients} will be connected to Patchbay, if they aren't, for what they lose.",
                        clients = leaving.join(", ")
                    ),
                    cx,
                ))
            });
        v_flex()
            .size_full()
            .min_w_0()
            .child(header)
            .child(
                div()
                    .id("import")
                    .flex_1()
                    .min_w_0()
                    .overflow_y_scroll()
                    .child(body),
            )
            .into_any_element()
    }

    /// One server, as a card: its tick and name, the clients that have it, where it's reached, and once it's ticked,
    /// which setup to bring in and where to take it out of.
    fn import_card(
        &self,
        found: &Found,
        importing: &Importing,
        home: &Path,
        cx: &Context<Self>,
    ) -> Div {
        let theme = cx.theme();
        let name = found.name.clone();
        let chosen = importing.chosen.get(&found.name).copied();
        let mut clients: Vec<&str> = found
            .versions
            .iter()
            .flat_map(|version| &version.origins)
            .map(|origin| origin.client.name())
            .collect();
        clients.sort_unstable();
        clients.dedup();
        let several = found.versions.len() > 1;
        // The whole row ticks it, not only the box.
        let head = h_flex()
            .id(SharedString::from(format!("import-row-{name}")))
            .gap_2()
            .items_center()
            .min_w_0()
            .cursor_pointer()
            .on_click(cx.listener({
                let name = name.clone();
                move |this, _, _, cx| this.toggle_import(&name, cx)
            }))
            .child(
                Checkbox::new(SharedString::from(format!("import-{name}")))
                    .checked(chosen.is_some()),
            )
            .child(div().flex_none().font_semibold().child(name.clone()))
            .when(several, |this| {
                this.child(
                    shell::badge(Tag::secondary())
                        .xsmall()
                        .child(t!("%{count} setups", count = found.versions.len()).into_owned()),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_right()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(clients.join(", ")),
            );
        let mut card = v_flex()
            .gap_2()
            .p_3()
            .min_w_0()
            .rounded_lg()
            .border_1()
            .border_color(if chosen.is_some() {
                theme.primary
            } else {
                theme.border
            })
            .child(head);
        let shown = chosen.unwrap_or(0);
        match chosen {
            // Ticked with several setups: choose one, each with the clients that have it.
            Some(at) if several => {
                card = card.child(label_line(t!("Bring in this setup"), cx));
                for (version_at, version) in found.versions.iter().enumerate() {
                    let summary = Summary::of(&version.server);
                    let mut by: Vec<&str> = version
                        .origins
                        .iter()
                        .map(|origin| origin.client.name())
                        .collect();
                    by.dedup();
                    let name = name.clone();
                    card = card.child(
                        h_flex()
                            .id(SharedString::from(format!(
                                "import-{name}-setup-{version_at}"
                            )))
                            .gap_2()
                            .items_center()
                            .min_w_0()
                            .pl_6()
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.choose_version(&name, version_at, cx);
                            }))
                            .child(
                                Radio::new(SharedString::from(format!(
                                    "import-{}-radio-{version_at}",
                                    found.name
                                )))
                                .checked(at == version_at),
                            )
                            .child(shell::code_chip(tilde(&summary.target, home), cx))
                            .child(
                                div()
                                    .flex_none()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child(by.join(", ")),
                            ),
                    );
                }
            }
            _ => {
                let summary = Summary::of(&found.versions[shown].server);
                card = card.child(
                    h_flex()
                        .pl_6()
                        .gap_2()
                        .min_w_0()
                        .child(shell::code_chip(tilde(&summary.target, home), cx)),
                );
            }
        }
        let summary = Summary::of(&found.versions[shown].server);
        if !summary.keys.is_empty() {
            card = card.child(div().pl_6().child(hint(
                t!(
                    "Keys: %{keys}, which go to the Keychain",
                    keys = summary.keys.join(", ")
                ),
                cx,
            )));
        }
        let Some(at) = chosen else {
            return card;
        };
        // Where it can be taken out of, as chips to switch on: every place it's kept, whichever setup.
        let mut differs = Vec::new();
        let chips = found
            .versions
            .iter()
            .enumerate()
            .flat_map(|(version_at, version)| {
                version
                    .origins
                    .iter()
                    .map(move |origin| (version_at, origin))
            });
        let chips: Vec<Button> = chips
            .map(|(version_at, origin)| {
                let out = importing
                    .out
                    .contains(&(origin.clone(), found.name.clone()));
                if out && version_at != at {
                    differs.push(origin.client.name());
                }
                let (label, says) = place_label(origin, home);
                let origin_for_click = origin.clone();
                let name = name.clone();
                Button::new(SharedString::from(format!(
                    "import-out-{name}-{}",
                    origin_id(origin)
                )))
                .xsmall()
                // On, a chip is filled, so what's to come out reads at a glance.
                .when(out, |chip| chip.primary())
                .when(!out, |chip| chip.outline())
                .toggled(out)
                .label(label)
                .tooltip(says)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_out(&origin_for_click, &name, cx);
                }))
            })
            .collect();
        card = card
            .child(label_line(t!("Take it out of"), cx))
            .child(h_flex().pl_6().gap_1().flex_wrap().children(chips));
        if !differs.is_empty() {
            card = card.child(
                div().pl_6().text_sm().text_color(theme.warning).child(
                    t!(
                        "It's set up differently in %{clients}, and that setup goes with it.",
                        clients = differs.join(", ")
                    )
                    .into_owned(),
                ),
            );
        }
        card
    }

    /// One of the person's own skills, as a card: its tick and name, and the clients that have it.
    fn skill_card(
        &self,
        skill: &Skill,
        importing: &Importing,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let theme = cx.theme();
        let folder = skill.folder.clone();
        let ticked = importing.skills.contains(&folder);
        let clients: Vec<&str> = skill
            .clients
            .iter()
            .map(|(client, _)| client.name())
            .collect();
        h_flex()
            .id(SharedString::from(format!("import-skill-row-{folder}")))
            .gap_2()
            .items_center()
            .p_3()
            .min_w_0()
            .rounded_lg()
            .border_1()
            .border_color(if ticked { theme.primary } else { theme.border })
            .cursor_pointer()
            .child(
                Checkbox::new(SharedString::from(format!("import-skill-{folder}"))).checked(ticked),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_import_skill(&folder, cx);
            }))
            .child(div().flex_none().font_semibold().child(skill.name.clone()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_right()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(clients.join(", ")),
            )
    }
}

/// A small heading inside a card.
fn label_line(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .pl_6()
        .text_xs()
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

/// `text` with the home folder written as `~`.
fn tilde(text: &str, home: &Path) -> String {
    let home = home.display().to_string();
    text.replace(&format!("{home}/"), "~/")
}

/// A place a copy can be taken out of, short for its chip, and in full for its tooltip: whether it's the client's own,
/// one project's only, or a file the project's repository shares.
fn place_label(origin: &Origin, home: &Path) -> (String, String) {
    let client = origin.client.name();
    let project = |folder: &Path| {
        folder.file_name().map_or_else(
            || folder.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        )
    };
    let shown = |folder: &Path| tilde(&folder.display().to_string(), home);
    match &origin.kept {
        Kept::User => (
            client.to_string(),
            t!("%{client}'s own servers", client = client).into_owned(),
        ),
        Kept::InProject(folder) => (
            format!("{client} · {}", project(folder)),
            t!(
                "%{client} in %{project}, this project only",
                client = client,
                project = shown(folder)
            )
            .into_owned(),
        ),
        Kept::ProjectFile(folder) => (
            format!("{}/.mcp.json", project(folder)),
            t!(
                "%{file}, which the project's repository shares",
                file = shown(&folder.join(".mcp.json"))
            )
            .into_owned(),
        ),
    }
}

/// A short, steady id for where a copy is, for its checkbox's element id.
fn origin_id(origin: &Origin) -> String {
    let place = match &origin.kept {
        Kept::User => "user".to_string(),
        Kept::InProject(folder) => format!("project-{}", folder.display()),
        Kept::ProjectFile(folder) => format!("file-{}", folder.display()),
    };
    format!("{}-{place}", origin.client.slug())
}
