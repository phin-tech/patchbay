//! The Skills place: every skill on this Mac, from the store Patchbay shares with the `skills` CLI or a client's own
//! folder, and the one chosen, with the clients that have it. Adding one takes what `npx skills add` takes, pasted
//! whole or as a source, fetches it off the window's thread, and installs the skills ticked.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use gpui_kit::component::checkbox::Checkbox;
use patchbay_core::skills::{
    self, Change, Checking, Folders, Keeper, Preview, Reach, Request, Skill, SkillsError, Source,
    Update, clients_with_skills,
};

use super::*;

/// A source being added, in place of the chosen skill.
pub struct Adding {
    pub(super) field: Entity<InputState>,
    pub state: Step,
    _changed: Subscription,
    _task: Option<Task<()>>,
}

/// Which skills a Command is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    Chosen,
    Every,
}

/// Where adding has got to.
pub enum Step {
    /// Waiting for a source.
    Typing,
    Fetching(String),
    /// The source's skills, and the ones ticked to install.
    Found {
        preview: Box<Preview>,
        request: Request,
        chosen: BTreeSet<String>,
    },
    Failed(String),
}

/// The skills found, and what checking their sources found.
#[derive(Default)]
pub struct SkillsState {
    pub list: Vec<Skill>,
    /// The chosen skill, by folder.
    pub chosen: Option<String>,
    pub adding: Option<Adding>,
    /// Skills whose sources have changed, by lock name.
    pub updates: BTreeMap<String, Update>,
    pub checking: bool,
    _task: Option<Task<()>>,
}

/// Where skills go: the person's own folders when the app runs for real, the stand-in home otherwise.
pub(super) fn skill_folders(cx: &App) -> Folders {
    let home = client_home(cx);
    let real = std::env::var_os("HOME").is_some_and(|own| Path::new(&own) == home);
    if real {
        Folders::from_env(home)
    } else {
        Folders::new(home)
    }
}

impl MainView {
    /// Reads every skill folder again; what's chosen stays chosen while it's there.
    pub(super) fn sync_skills(&mut self, cx: &mut Context<Self>) {
        let state = &mut self.skills;
        state.list = skills::list(&skill_folders(cx));
        if state
            .chosen
            .as_ref()
            .is_none_or(|folder| !state.list.iter().any(|skill| &skill.folder == folder))
        {
            state.chosen = state.list.first().map(|skill| skill.folder.clone());
        }
        cx.notify();
    }

    fn choose_skill(&mut self, folder: String, cx: &mut Context<Self>) {
        self.skills.chosen = Some(folder);
        self.skills.adding = None;
        cx.notify();
    }

    /// Shows a field for a source in place of the chosen skill.
    pub(super) fn start_adding_skills(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let field = cx.new(|cx| {
            InputState::new(window, cx).placeholder(
                t!("owner/repo, a git URL, or a whole npx skills add line").into_owned(),
            )
        });
        let changed = cx.subscribe_in(&field, window, |this, _, event: &InputEvent, window, cx| {
            match event {
                InputEvent::PressEnter { .. } => this.find_skills(window, cx),
                InputEvent::Change => {
                    // A new source starts again from what's typed.
                    if let Some(adding) = &mut this.skills.adding
                        && !matches!(adding.state, Step::Fetching(_))
                    {
                        adding.state = Step::Typing;
                        cx.notify();
                    }
                }
                _ => {}
            }
        });
        field.read(cx).focus_handle(cx).focus(window, cx);
        self.place = Place::Skills;
        self.skills.adding = Some(Adding {
            field,
            state: Step::Typing,
            _changed: changed,
            _task: None,
        });
        cx.notify();
    }

    /// Reads the field as `npx skills add` would and fetches the source on the background executor.
    pub(super) fn find_skills(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(adding) = &mut self.skills.adding else {
            return;
        };
        let text = adding.field.read(cx).value().to_string();
        let home = client_home(cx);
        let read = Request::parse_line(&text)
            .map_err(|error| error.to_string())
            .and_then(|request| {
                let source =
                    Source::parse(&request.source, &home).map_err(|error| error.to_string())?;
                Ok((request, source))
            });
        let (request, source) = match read {
            Ok(read) => read,
            Err(error) => {
                adding.state = Step::Failed(error);
                cx.notify();
                return;
            }
        };
        adding.state = Step::Fetching(source.lock_source());
        let fetching = cx.background_spawn(async move { Preview::fetch(source) });
        adding._task = Some(cx.spawn_in(window, async move |this, cx| {
            let fetched = fetching.await;
            let _ = this.update(cx, |this, cx| {
                let Some(adding) = &mut this.skills.adding else {
                    return;
                };
                adding.state = match fetched {
                    Ok(preview) => {
                        // What the line names is ticked, or the only skill there is.
                        let chosen: BTreeSet<String> = match preview.choose(&request.skills) {
                            Ok(chosen) if !chosen.is_empty() => {
                                chosen.iter().map(|found| found.name.clone()).collect()
                            }
                            _ if preview.found.len() == 1 => preview
                                .found
                                .iter()
                                .map(|found| found.name.clone())
                                .collect(),
                            _ => BTreeSet::new(),
                        };
                        Step::Found {
                            preview: Box::new(preview),
                            request,
                            chosen,
                        }
                    }
                    Err(error) => Step::Failed(error.to_string()),
                };
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn tick_skill(&mut self, name: String, ticked: bool, cx: &mut Context<Self>) {
        if let Some(Adding {
            state: Step::Found { chosen, .. },
            ..
        }) = &mut self.skills.adding
        {
            if ticked {
                chosen.insert(name);
            } else {
                chosen.remove(&name);
            }
            cx.notify();
        }
    }

    /// Whether skills are being checked or updated, said so if they are. The lock is written as each update goes
    /// in, so installing, removing or taking over a skill meanwhile waits for it rather than racing it.
    fn still_checking(&self, window: &mut Window, cx: &mut App) -> bool {
        if self.skills.checking {
            Toast::new(t!("Wait for the skills to finish updating")).show(window, cx);
        }
        self.skills.checking
    }

    /// Installs the skills ticked, for the clients the line names or every one installed.
    pub(super) fn install_skills(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.still_checking(window, cx) {
            return;
        }
        let folders = skill_folders(cx);
        let Some(Adding {
            state:
                Step::Found {
                    preview,
                    request,
                    chosen,
                },
            ..
        }) = &self.skills.adding
        else {
            return;
        };
        let found: Vec<_> = preview
            .found
            .iter()
            .filter(|found| chosen.contains(&found.name))
            .collect();
        let clients = request.clients_for(&folders);
        match skills::install(&folders, preview, &found, &clients, &skills::now()) {
            Ok(installed) => {
                let blocked = installed.iter().any(|skill| !skill.blocked.is_empty());
                let first = installed
                    .first()
                    .map(|skill| skills::folder_name(&skill.name));
                let said = match installed.as_slice() {
                    [one] => t!("Installed %{skill}", skill = one.name).into_owned(),
                    many => t!("Installed %{count} skills", count = many.len()).into_owned(),
                };
                Toast::new(said).show(window, cx);
                if blocked {
                    Toast::new(t!(
                        "Some clients already had a skill of that name of their own, left as it was"
                    ))
                    .show(window, cx);
                }
                self.skills.adding = None;
                self.skills.chosen = first;
                self.sync_skills(cx);
            }
            Err(error) => Toast::error(t!("Couldn't install: %{error}", error = error.to_string()))
                .show(window, cx),
        }
    }

    fn cancel_adding(&mut self, cx: &mut Context<Self>) {
        self.skills.adding = None;
        cx.notify();
    }

    /// Removes the chosen skill, if the lock records it.
    pub(super) fn remove_chosen_skill(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.still_checking(window, cx) {
            return;
        }
        let Some(skill) = self.chosen_skill().filter(|skill| skill.managed()) else {
            Toast::new(t!("Choose a skill Patchbay or the skills CLI installed")).show(window, cx);
            return;
        };
        let folder = skill.folder.clone();
        match skills::remove(&skill_folders(cx), &folder) {
            Ok(name) => {
                self.skills.updates.remove(&name);
                Toast::new(t!("Removed %{skill}", skill = name)).show(window, cx);
            }
            Err(error) => Toast::error(t!(
                "Couldn't remove it: %{error}",
                error = error.to_string()
            ))
            .show(window, cx),
        }
        self.sync_skills(cx);
    }

    /// Puts the chosen skill of the person's own in Patchbay's care, or every one with `Which::Every`.
    pub(super) fn take_over_skills(
        &mut self,
        which: Which,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.still_checking(window, cx) {
            return;
        }
        let folders = skill_folders(cx);
        let folders_to_take: Vec<String> = match which {
            Which::Chosen => self
                .chosen_skill()
                .filter(|skill| !skill.managed())
                .map(|skill| skill.folder.clone())
                .into_iter()
                .collect(),
            Which::Every => self
                .skills
                .list
                .iter()
                .filter(|skill| !skill.managed())
                .map(|skill| skill.folder.clone())
                .collect(),
        };
        if folders_to_take.is_empty() {
            Toast::new(t!(
                "Every skill is already kept by Patchbay or the skills CLI"
            ))
            .show(window, cx);
            return;
        }
        let (mut taken, mut kept, mut failed) = (Vec::new(), 0, Vec::new());
        for folder in &folders_to_take {
            match skills::take_over(&folders, folder, &skills::now()) {
                Ok(done) => {
                    kept += done.kept.len();
                    taken.push(done);
                }
                Err(error) => failed.push(format!("{folder}: {error}")),
            }
        }
        let said = match taken.as_slice() {
            [] => None,
            [one] => Some(t!("Took over %{skill}", skill = one.name).into_owned()),
            many => Some(t!("Took over %{count} skills", count = many.len()).into_owned()),
        };
        if let Some(said) = said {
            Toast::new(said).key("skills-take-over").show(window, cx);
        }
        if kept > 0 {
            // A copy that differs is someone's edit; it's never thrown away to make a link.
            Toast::new(t!(
                "%{count} copies differ from the one in ~/.agents/skills, or hold what it leaves out, so they're left as they are",
                count = kept
            ))
            .show(window, cx);
        }
        if let Some(first) = failed.first() {
            Toast::error(t!("Couldn't take over %{what}", what = first)).show(window, cx);
        }
        self.sync_skills(cx);
    }

    /// Gives a client the chosen skill, or takes it away.
    pub(super) fn give_skill(
        &mut self,
        folder: String,
        client: Client,
        given: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let folders = skill_folders(cx);
        let done = if given {
            skills::link_client(&folders, &folder, client)
        } else {
            skills::unlink_client(&folders, &folder, client)
        };
        if let Err(error) = done {
            Toast::error(error.to_string()).show(window, cx);
        }
        self.sync_skills(cx);
    }

    /// Puts in place the updates listed for every skill, or for the one named, as the last check found them. With
    /// none listed it checks instead, so what changes is always shown before it's applied.
    pub(super) fn update_skills(
        &mut self,
        name: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let listed: Vec<Update> = self
            .skills
            .updates
            .values()
            .filter(|update| name.as_ref().is_none_or(|name| *name == update.name))
            .cloned()
            .collect();
        let names: Vec<String> = name.into_iter().collect();
        if listed.is_empty() {
            self.check_skills(names, Checking::Report, window, cx);
        } else {
            self.check_skills(names, Checking::Apply(listed), window, cx);
        }
    }

    /// Puts in place the update listed for the chosen skill.
    pub(super) fn update_chosen_skill(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self
            .chosen_skill()
            .filter(|skill| skill.managed())
            .map(|skill| skill.name.clone())
        else {
            Toast::new(t!("Choose a skill Patchbay or the skills CLI installed")).show(window, cx);
            return;
        };
        self.update_skills(Some(name), window, cx);
    }

    /// Fetches the sources of every skill, or only `names`, off the window's thread, and says which have changed
    /// or applies the changes.
    pub(super) fn check_skills(
        &mut self,
        names: Vec<String>,
        checking: Checking,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.skills.checking {
            return;
        }
        self.skills.checking = true;
        let folders = skill_folders(cx);
        let applying = matches!(checking, Checking::Apply(_));
        let work = cx.background_spawn(async move {
            skills::check(&folders, &names, checking, &skills::now())
        });
        self.skills._task = Some(cx.spawn_in(window, async move |this, cx| {
            let checked = work.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.skills.checking = false;
                this.finish_check(checked, applying, window, cx);
            });
        }));
        cx.notify();
    }

    fn finish_check(
        &mut self,
        checked: Result<skills::Checked, SkillsError>,
        applying: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match checked {
            Ok(checked) => {
                let count = checked.updates.len();
                let said = match (applying, count) {
                    (_, 0) if checked.unchecked.is_empty() => {
                        t!("Every skill is up to date").into_owned()
                    }
                    (_, 0) => t!("No updates found").into_owned(),
                    (true, 1) => t!("Updated 1 skill").into_owned(),
                    (true, count) => t!("Updated %{count} skills", count = count).into_owned(),
                    (false, 1) => t!("1 skill has an update").into_owned(),
                    (false, count) => {
                        t!("%{count} skills have updates", count = count).into_owned()
                    }
                };
                Toast::new(said).key("skills-check").show(window, cx);
                // Offline, or a private repository, isn't the same as up to date.
                if let Some((name, why)) = checked.unchecked.first() {
                    let said = match checked.unchecked.len() {
                        1 => t!("Couldn't check %{skill}: %{why}", skill = name, why = why)
                            .into_owned(),
                        count => t!(
                            "Couldn't check %{count} skills, %{skill} among them: %{why}",
                            count = count,
                            skill = name,
                            why = why
                        )
                        .into_owned(),
                    };
                    Toast::error(said).key("skills-unchecked").show(window, cx);
                }
                if applying {
                    for update in &checked.updates {
                        self.skills.updates.remove(&update.name);
                    }
                    // One that moved on again since it was listed is no longer what the list says.
                    for (name, _) in &checked.unchecked {
                        self.skills.updates.remove(name);
                    }
                } else {
                    self.skills.updates = checked
                        .updates
                        .into_iter()
                        .map(|update| (update.name.clone(), update))
                        .collect();
                }
            }
            Err(error) => Toast::error(t!(
                "Couldn't check the skills: %{error}",
                error = error.to_string()
            ))
            .show(window, cx),
        }
        self.sync_skills(cx);
    }

    fn chosen_skill(&self) -> Option<&Skill> {
        let folder = self.skills.chosen.as_ref()?;
        self.skills
            .list
            .iter()
            .find(|skill| &skill.folder == folder)
    }

    pub(super) fn skill_list(&self, cx: &Context<Self>) -> AnyElement {
        let header = self.panel_header(t!("Skills"), cx).child(
            Button::new("add-skills")
                .ghost()
                .xsmall()
                .label(t!("Add…").into_owned())
                .on_click(cx.listener(|this, _, window, cx| this.run("skills.add", window, cx))),
        );
        let folders = skill_folders(cx);
        let query = self.filter_text(Place::Skills, cx);
        let from = |skill: &Skill| match &skill.keeper {
            Keeper::Locked(entry) if entry.taken_over(&folders) => t!("This Mac").into_owned(),
            Keeper::Locked(entry) => entry.source.clone(),
            Keeper::Unmanaged => t!("Your own").into_owned(),
        };
        let shown: Vec<(&Skill, String)> = self
            .skills
            .list
            .iter()
            .map(|skill| (skill, from(skill)))
            .filter(|(skill, from)| {
                matches_filter(
                    &query,
                    &[
                        &skill.name,
                        skill.description.as_deref().unwrap_or(""),
                        from,
                    ],
                )
            })
            .collect();
        // Skills whose name matches come before those only their description or source does.
        let mut shown = shown;
        shown.sort_by_key(|(skill, _)| !matches_filter(&query, &[&skill.name]));
        let nothing_matches = shown.is_empty() && !self.skills.list.is_empty();
        let rows = shown.into_iter().map(|(skill, from)| {
            let folder = skill.folder.clone();
            let chosen =
                self.skills.adding.is_none() && self.skills.chosen.as_ref() == Some(&folder);
            list_row(chosen, cx)
                .id(SharedString::from(format!("skill-row-{folder}")))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().truncate().child(skill.name.clone()))
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .child(from),
                        ),
                )
                .when(self.skills.updates.contains_key(&skill.name), |row| {
                    row.child(
                        shell::badge(Tag::warning())
                            .xsmall()
                            .child(t!("Update").into_owned()),
                    )
                })
                .on_click(cx.listener(move |this, _, _, cx| this.choose_skill(folder.clone(), cx)))
        });
        v_flex()
            .size_full()
            .child(header)
            .child(self.filter_field(Place::Skills, cx))
            .child(
                div()
                    .id("skill-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(v_flex().children(rows))
                    .when(nothing_matches, |this| {
                        this.child(div().p_3().child(hint(t!("No skills match."), cx)))
                    })
                    .when(self.skills.list.is_empty(), |this| {
                        this.child(div().p_3().child(hint(
                            t!(
                                "No skills yet. Add some from a repository, as npx skills add does."
                            ),
                            cx,
                        )))
                    }),
            )
            .into_any_element()
    }

    pub(super) fn skill_detail(&self, cx: &Context<Self>) -> AnyElement {
        if let Some(adding) = &self.skills.adding {
            return self.adding_panel(adding, cx);
        }
        let Some(skill) = self.chosen_skill() else {
            return div().into_any_element();
        };
        let update = self.skills.updates.get(&skill.name);
        let badge = match &skill.keeper {
            Keeper::Locked(_) if update.is_some() => {
                shell::badge(Tag::warning()).child(t!("Update available").into_owned())
            }
            Keeper::Locked(_) => shell::badge(Tag::success()).child(t!("Installed").into_owned()),
            Keeper::Unmanaged => shell::badge(Tag::secondary()).child(t!("Your own").into_owned()),
        };
        let header = self
            .panel_header_beside(skill.name.clone(), badge.xsmall(), cx)
            .child(
                h_flex()
                    .gap_1()
                    .when(update.is_some(), |row| {
                        row.child(
                            Button::new("update-skill")
                                .primary()
                                .xsmall()
                                .disabled(self.skills.checking)
                                .label(t!("Update").into_owned())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.run("skills.update-chosen", window, cx);
                                })),
                        )
                    })
                    .when(skill.managed(), |row| {
                        row.child(
                            Button::new("remove-skill")
                                .ghost()
                                .xsmall()
                                .label(t!("Remove").into_owned())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.run("skills.remove", window, cx);
                                })),
                        )
                    }),
            );
        let source = match &skill.keeper {
            Keeper::Locked(entry) if entry.taken_over(&skill_folders(cx)) => h_flex().child(hint(
                t!("This Mac: taken over as it was, so there are no updates to fetch."),
                cx,
            )),
            Keeper::Locked(entry) => {
                let mut said = entry.source.clone();
                if let Some(reference) = &entry.reference {
                    said = format!("{said}#{reference}");
                }
                h_flex().child(shell::code_chip(said, cx))
            }
            // Beside what it says, where the eye already is, rather than across the panel in the heading.
            Keeper::Unmanaged => h_flex()
                .gap_2()
                .child(
                    Button::new("take-over-skill")
                        .primary()
                        .xsmall()
                        .label(t!("Take Over").into_owned())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.run("skills.take-over", window, cx);
                        })),
                )
                .child(hint(
                    t!("Your own: not installed by Patchbay or the skills CLI."),
                    cx,
                )),
        };
        // Under the home folder it's written from `~`, as the CLI and a terminal show it.
        let home = client_home(cx);
        let folder = skill
            .path
            .as_ref()
            .and_then(|path| path.parent())
            .map(|path| match path.strip_prefix(&home) {
                Ok(inside) => format!("~/{}", inside.display()),
                Err(_) => path.display().to_string(),
            })
            .unwrap_or_default();
        let body = v_flex()
            .gap_4()
            .p_4()
            .when_some(skill.description.clone(), |this, description| {
                this.child(div().text_base().child(description))
            })
            .child(field(t!("Source"), source, cx))
            .child(field(
                t!("Folder"),
                h_flex().child(shell::code_chip(folder, cx)),
                cx,
            ))
            .child(field(t!("Clients"), self.skill_clients(skill, cx), cx))
            .when_some(update, |this, update| {
                this.child(field(t!("Changes"), changes(&update.changes, cx), cx))
            });
        v_flex()
            .size_full()
            .child(header)
            .child(
                div()
                    .id("skill-detail")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(body),
            )
            .into_any_element()
    }

    /// A tick for each client with a skills folder: whether it has the skill, and how.
    fn skill_clients(&self, skill: &Skill, cx: &Context<Self>) -> Div {
        let folders = skill_folders(cx);
        let rows = clients_with_skills().into_iter().map(|client| {
            let reach = skill
                .clients
                .iter()
                .find(|(has, _)| *has == client)
                .map(|(_, reach)| *reach);
            let reads_store = folders.reads_store(client);
            let fixed = !skill.managed() || reads_store || reach == Some(Reach::Own);
            let folder = skill.folder.clone();
            let says = match reach {
                Some(Reach::Own) => Some(t!("has its own copy")),
                _ if reads_store => Some(t!("reads ~/.agents/skills")),
                _ => None,
            };
            h_flex()
                .gap_2()
                .child(
                    Checkbox::new(SharedString::from(format!(
                        "skill-{}-{}",
                        skill.folder,
                        client.slug()
                    )))
                    .checked(reach.is_some())
                    .disabled(fixed)
                    .label(client.name())
                    .on_click(cx.listener(
                        move |this, checked: &bool, window, cx| {
                            this.give_skill(folder.clone(), client, *checked, window, cx);
                        },
                    )),
                )
                .when_some(says, |row, says| row.child(hint(says, cx)))
        });
        v_flex().gap_1().children(rows)
    }

    fn adding_panel(&self, adding: &Adding, cx: &Context<Self>) -> AnyElement {
        let ticked = match &adding.state {
            Step::Found { chosen, .. } => chosen.len(),
            _ => 0,
        };
        let install_label = match ticked {
            0 | 1 => t!("Install").into_owned(),
            count => t!("Install %{count}", count = count).into_owned(),
        };
        let header = self.panel_header(t!("Add Skills"), cx).child(
            h_flex()
                .gap_1()
                .child(
                    Button::new("cancel-skills")
                        .ghost()
                        .xsmall()
                        .label(t!("Cancel").into_owned())
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_adding(cx))),
                )
                .child(
                    Button::new("install-skills")
                        .primary()
                        .xsmall()
                        .disabled(ticked == 0)
                        .label(install_label)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.install_skills(window, cx)),
                        ),
                ),
        );
        let found = match &adding.state {
            Step::Typing => hint(
                t!(
                    "Press Return to find the skills in it. Repositories laid out for npx skills add work here too."
                ),
                cx,
            ),
            Step::Fetching(source) => hint(t!("Fetching %{source}…", source = source), cx),
            Step::Failed(error) => div()
                .text_base()
                .text_color(cx.theme().danger)
                .child(error.clone()),
            Step::Found {
                preview, chosen, ..
            } => v_flex().gap_2().children(preview.found.iter().map(|found| {
                let name = found.name.clone();
                v_flex()
                    .gap_0p5()
                    .child(
                        Checkbox::new(SharedString::from(format!("found-{name}")))
                            .checked(chosen.contains(&name))
                            .label(name.clone())
                            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                this.tick_skill(name.clone(), *checked, cx);
                            })),
                    )
                    .child(div().pl_6().child(hint(found.description.clone(), cx)))
            })),
        };
        let body = v_flex()
            .gap_5()
            .p_4()
            .child(
                section(t!("Source"), cx).child(
                    h_flex()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .child(Input::new(&adding.field).small().h(rems(24. / 13.))),
                        )
                        .child(
                            Button::new("find-skills")
                                .xsmall()
                                .disabled(matches!(adding.state, Step::Fetching(_)))
                                .label(t!("Find Skills").into_owned())
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.find_skills(window, cx)),
                                ),
                        ),
                ),
            )
            .child(section(t!("Skills"), cx).child(found));
        v_flex()
            .size_full()
            .child(header)
            .child(
                div()
                    .id("adding-skills")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(body),
            )
            .into_any_element()
    }
}

/// A skill's changed files, one a line.
fn changes(changes: &[Change], cx: &App) -> Div {
    if changes.is_empty() {
        return hint(t!("Its files are the same; only its record changed."), cx);
    }
    v_flex().gap_1().children(changes.iter().map(|change| {
        let (path, said) = match change {
            Change::Added(path) => (path, t!("added")),
            Change::Removed(path) => (path, t!("removed")),
            Change::Changed(path) => (path, t!("changed")),
        };
        h_flex()
            .gap_2()
            .child(shell::code_chip(path.clone(), cx))
            .child(hint(said, cx))
    }))
}
