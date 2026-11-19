//! Import: every server the person's clients already keep, for them and in each Claude Code project, and every
//! skill of their own, listed for them to go through one by one. Nothing is ticked to begin with: a server comes in
//! only once it's ticked, and leaves a client only where "take it out" is ticked too. The servers go through Add
//! Server, as a paste, so the keys it would keep are shown before anything is added.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use gpui_kit::component::checkbox::Checkbox;
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
        let rows = importing
            .gathered
            .found
            .iter()
            .map(|found| self.import_row(found, importing, &home, cx));
        let mut leaving: Vec<&str> = importing
            .out
            .iter()
            .map(|(origin, _)| origin.client.name())
            .collect();
        leaving.dedup();
        let own_skills: Vec<&Skill> = self
            .skills
            .list
            .iter()
            .filter(|skill| !skill.managed())
            .collect();
        let skills = v_flex().gap_2().children(own_skills.iter().map(|skill| {
            let folder = skill.folder.clone();
            let clients: Vec<&str> = skill
                .clients
                .iter()
                .map(|(client, _)| client.name())
                .collect();
            v_flex()
                .gap_0p5()
                .child(
                    Checkbox::new(SharedString::from(format!("import-skill-{folder}")))
                        .label(skill.name.clone())
                        .checked(importing.skills.contains(&folder))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.toggle_import_skill(&folder, cx);
                        })),
                )
                .child(
                    div()
                        .pl_6()
                        .child(hint(t!("In %{clients}", clients = clients.join(", ")), cx)),
                )
        }));
        let body = v_flex()
            .gap_4()
            .p_4()
            .child(hint(
                t!("Tick what to bring into Patchbay. Nothing leaves a client unless you tick that too, and a client a server leaves is connected to Patchbay, so it still has it."),
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
                    .when(importing.gathered.found.is_empty(), |this| {
                        this.child(hint(t!("Your clients have no servers of their own."), cx))
                    })
                    .children(rows),
            )
            .when(!own_skills.is_empty(), |this| {
                this.child(
                    section(t!("Your own skills"), cx)
                        .child(hint(
                            t!("Taken over, a skill is kept once and each client's copy becomes a link to it, where it's the same."),
                            cx,
                        ))
                        .child(skills),
                )
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
            .child(header)
            .child(div().id("import").flex_1().overflow_y_scroll().child(body))
            .into_any_element()
    }

    /// One server: its tick, each version to choose between if they differ, and each place it can be taken out of.
    fn import_row(
        &self,
        found: &Found,
        importing: &Importing,
        home: &Path,
        cx: &Context<Self>,
    ) -> Div {
        let theme = cx.theme();
        let name = found.name.clone();
        let chosen = importing.chosen.get(&found.name).copied();
        let mut row = v_flex().gap_1();
        let tick = Checkbox::new(SharedString::from(format!("import-{name}")))
            .label(name.clone())
            .checked(chosen.is_some())
            .disabled(found.patchbay_has.is_some())
            .on_click(cx.listener({
                let name = name.clone();
                move |this, _, _, cx| this.toggle_import(&name, cx)
            }));
        row = row.child(tick);
        if let Some(has) = &found.patchbay_has {
            return row.child(div().pl_6().child(hint(
                t!(
                    "Patchbay already has it, as %{server}",
                    server = has.clone()
                ),
                cx,
            )));
        }
        let several = found.versions.len() > 1;
        for (at, version) in found.versions.iter().enumerate() {
            let summary = Summary::of(&version.server);
            let says = if summary.keys.is_empty() {
                summary.target.clone()
            } else {
                t!(
                    "%{target} · keys %{keys}",
                    target = summary.target.clone(),
                    keys = summary.keys.join(", ")
                )
                .into_owned()
            };
            let mut block = v_flex().pl_6().gap_1();
            block = if several {
                let name = name.clone();
                block.child(
                    Checkbox::new(SharedString::from(format!("import-{name}-version-{at}")))
                        .label(t!("This one: %{says}", says = says).into_owned())
                        .checked(chosen == Some(at))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.choose_version(&name, at, cx);
                        })),
                )
            } else {
                block.child(hint(says, cx))
            };
            for origin in &version.origins {
                let key = (origin.clone(), found.name.clone());
                let out = importing.out.contains(&key);
                // Taking out a copy set up differently from the one brought in loses how it was set up.
                let differs = chosen.is_some_and(|chosen| chosen != at);
                let origin_for_click = origin.clone();
                let name = name.clone();
                block = block.child(
                    v_flex()
                        .pl_6()
                        .child(
                            Checkbox::new(SharedString::from(format!(
                                "import-out-{name}-{}",
                                origin_id(origin)
                            )))
                            .label(take_out_label(origin, home))
                            .checked(out)
                            .disabled(chosen.is_none())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.toggle_out(&origin_for_click, &name, cx);
                            })),
                        )
                        .when(out && differs, |this| {
                            this.child(
                                div().pl_6().text_sm().text_color(theme.warning).child(
                                    t!("It's set up differently there, and that setup goes with it.")
                                        .into_owned(),
                                ),
                            )
                        }),
                );
            }
            row = row.child(block);
        }
        row
    }
}

/// "Take it out of Claude Code", and for a project, which one and whether it's shared with the project's repository.
fn take_out_label(origin: &Origin, home: &Path) -> String {
    let client = origin.client.name();
    let shown = |folder: &Path| match folder.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => folder.display().to_string(),
    };
    match &origin.kept {
        Kept::User => t!("Take it out of %{client}", client = client).into_owned(),
        Kept::InProject(folder) => t!(
            "Take it out of %{client} in %{project}, this project only",
            client = client,
            project = shown(folder)
        )
        .into_owned(),
        Kept::ProjectFile(folder) => t!(
            "Take it out of %{file}, which the project's repository shares",
            file = shown(&folder.join(".mcp.json"))
        )
        .into_owned(),
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
