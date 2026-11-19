//! Teams kept from a git repository: subscribing by its URL, checking each one in the background, and showing what a
//! team changed for the person to accept, in place of the chosen server. Nothing fetched reaches the gateway until it's
//! accepted, and accepting takes the commit that was shown.

use std::time::Duration;

use patchbay_core::Home;
use patchbay_core::share::TeamError;
use patchbay_core::skills::Stamp;
use patchbay_core::subscription::{
    self, Accepted, Asked, Change, How, Part, Pending, Subscriptions, TeamSkills,
};

use super::*;

/// How often every team is checked while the app runs.
const CHECK_EVERY: Duration = Duration::from_secs(60 * 60);

/// Teams being subscribed to, and the changes teams have waiting.
#[derive(Default)]
pub struct TeamsState {
    pub subscribing: Option<Subscribing>,
    /// Each team's changes, fetched and waiting to be looked at, by the folder it's kept under.
    pub updates: BTreeMap<String, Pending>,
    /// The team whose changes are shown, by folder.
    pub reviewing: Option<String>,
    /// Whether teams are being checked or a team's changes taken in, which each wait for the other.
    pub busy: bool,
    _work: Option<Task<()>>,
}

/// A team's repository being subscribed to, in place of the chosen server.
pub struct Subscribing {
    url: Entity<InputState>,
    branch: Entity<InputState>,
    name: Entity<InputState>,
    pub state: Fetch,
    _changed: Vec<Subscription>,
    _task: Option<Task<()>>,
}

/// Where subscribing has got to.
pub enum Fetch {
    Typing,
    Fetching,
    Found(Box<Pending>),
    Failed(String),
}

impl MainView {
    /// Shows fields for a team's repository in place of the chosen server.
    pub(super) fn start_subscribing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let field = |placeholder: String, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
        };
        let url = field(
            t!("A git URL, or owner/repo on GitHub").into_owned(),
            window,
            cx,
        );
        let branch = field(t!("The default branch").into_owned(), window, cx);
        let name = field(t!("The name in its [team]").into_owned(), window, cx);
        let changed = [&url, &branch, &name]
            .into_iter()
            .map(|field| {
                cx.subscribe_in(field, window, |this, _, event: &InputEvent, window, cx| {
                    match event {
                        InputEvent::PressEnter { .. } => this.fetch_team(window, cx),
                        // What's typed now is another repository, or another name for it.
                        InputEvent::Change => {
                            if let Some(subscribing) = &mut this.teams.subscribing
                                && !matches!(subscribing.state, Fetch::Fetching)
                            {
                                subscribing.state = Fetch::Typing;
                                cx.notify();
                            }
                        }
                        _ => {}
                    }
                })
            })
            .collect();
        url.read(cx).focus_handle(cx).focus(window, cx);
        self.editor = None;
        self.exporting = None;
        self.importing = None;
        self.teams.reviewing = None;
        self.place = Place::Servers;
        self.teams.subscribing = Some(Subscribing {
            url,
            branch,
            name,
            state: Fetch::Typing,
            _changed: changed,
            _task: None,
        });
        cx.notify();
    }

    /// Fetches the repository typed, off the window's thread, to show what subscribing would add.
    pub(super) fn fetch_team(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(home) = cx
            .try_global::<Running>()
            .map(|running| running.gateway.home().clone())
        else {
            return;
        };
        let Some(subscribing) = &mut self.teams.subscribing else {
            return;
        };
        let value = |field: &Entity<InputState>| field.read(cx).value().trim().to_string();
        let (url, branch, name) = (
            value(&subscribing.url),
            value(&subscribing.branch),
            value(&subscribing.name),
        );
        if url.is_empty() || matches!(subscribing.state, Fetch::Fetching) {
            return;
        }
        subscribing.state = Fetch::Fetching;
        let fetching = cx.background_spawn(async move {
            let asked = Asked {
                branch: Some(branch.as_str()).filter(|branch| !branch.is_empty()),
                name: Some(name.as_str()).filter(|name| !name.is_empty()),
                at: None,
            };
            subscription::subscribe(&home, &url, asked)
        });
        subscribing._task = Some(cx.spawn_in(window, async move |this, cx| {
            let fetched = fetching.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(subscribing) = &mut this.teams.subscribing {
                    subscribing.state = match fetched {
                        Ok(pending) => Fetch::Found(Box::new(pending)),
                        Err(error) => Fetch::Failed(error.to_string()),
                    };
                    cx.notify();
                }
            });
        }));
        cx.notify();
    }

    pub(super) fn cancel_subscribing(&mut self, cx: &mut Context<Self>) {
        self.teams.subscribing = None;
        cx.notify();
    }

    /// Checks every team now, and then every hour while the window's open. A check that finds changes shows them in
    /// the list; one that fails is quiet, since the person didn't ask for it.
    pub(super) fn watch_teams(window: &mut Window, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn_in(window, async move |this, cx| {
            loop {
                if this
                    .update_in(cx, |this, window, cx| this.check_teams(false, window, cx))
                    .is_err()
                {
                    break;
                }
                cx.background_executor().timer(CHECK_EVERY).await;
            }
        })
    }

    /// Asks each team's repository whether it has moved on, off the window's thread, and keeps what changed to look
    /// at. `said` says how it went, for a check the person asked for.
    pub(super) fn check_teams(&mut self, said: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(home) = cx
            .try_global::<Running>()
            .map(|running| running.gateway.home().clone())
        else {
            return;
        };
        if self.teams.busy {
            if said {
                Toast::new(t!("Wait for the teams to finish")).show(window, cx);
            }
            return;
        }
        self.teams.busy = true;
        let checking = cx.background_spawn(async move { check_every_team(&home) });
        self.teams._work = Some(cx.spawn_in(window, async move |this, cx| {
            let checked = checking.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.teams.busy = false;
                let Checked {
                    teams,
                    failed,
                    moved,
                } = checked;
                // A commit that changed only comments was taken in by the check itself.
                if moved {
                    reload_gateway(cx);
                }
                let none = teams.is_empty() && failed.is_empty();
                for (folder, found) in teams {
                    match found {
                        Some(pending) => {
                            this.teams.updates.insert(folder, pending);
                        }
                        None => {
                            this.teams.updates.remove(&folder);
                        }
                    }
                }
                if said {
                    let waiting = this.teams.updates.len();
                    let message = match (none, waiting) {
                        (true, _) => t!("You don't subscribe to any team yet").into_owned(),
                        (false, 0) => t!("Every team is up to date").into_owned(),
                        (false, 1) => t!("A team has changes to look at").into_owned(),
                        (false, count) => {
                            t!("%{count} teams have changes to look at", count = count).into_owned()
                        }
                    };
                    Toast::new(message).key("teams-check").show(window, cx);
                    if let Some((team, error)) = failed.first() {
                        Toast::error(t!(
                            "Couldn't check %{team}: %{error}",
                            team = team,
                            error = error
                        ))
                        .show(window, cx);
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Shows the changes of the first team that has some, or checks when none has.
    pub(super) fn review_teams(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(folder) = self.teams.updates.keys().next().cloned() else {
            self.check_teams(true, window, cx);
            return;
        };
        self.review_team(folder, cx);
    }

    fn review_team(&mut self, folder: String, cx: &mut Context<Self>) {
        self.editor = None;
        self.exporting = None;
        self.importing = None;
        self.teams.subscribing = None;
        self.teams.reviewing = Some(folder);
        self.place = Place::Servers;
        cx.notify();
    }

    fn review_later(&mut self, cx: &mut Context<Self>) {
        self.teams.reviewing = None;
        cx.notify();
    }

    /// Takes in what was shown: a new team's manifest, or a team's changes, and then the skills it adds, off the
    /// window's thread since those are fetched.
    pub(super) fn accept_team(
        &mut self,
        pending: Pending,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(home) = cx
            .try_global::<Running>()
            .map(|running| running.gateway.home().clone())
        else {
            return;
        };
        if self.teams.busy {
            Toast::new(t!("Wait for the teams to finish")).show(window, cx);
            return;
        }
        self.teams.busy = true;
        let folders = skill_folders(cx);
        let accepting = cx.background_spawn(async move {
            let accepted = subscription::accept(&home, &pending)?;
            let skills = subscription::install_skills(&folders, &pending, &Stamp::now());
            Ok::<_, TeamError>((pending, accepted, skills))
        });
        self.teams._work = Some(cx.spawn_in(window, async move |this, cx| {
            let done = accepting.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.teams.busy = false;
                match done {
                    Ok((pending, accepted, skills)) => {
                        this.accepted(&pending, &accepted, &skills, window, cx);
                    }
                    Err(error) => Toast::error(t!(
                        "Couldn't take in the team: %{error}",
                        error = error.to_string()
                    ))
                    .show(window, cx),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Says what accepting did, and shows the team's servers.
    fn accepted(
        &mut self,
        pending: &Pending,
        accepted: &Accepted,
        skills: &TeamSkills,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.teams.updates.remove(&accepted.folder);
        self.teams.subscribing = None;
        self.teams.reviewing = None;
        reload_gateway(cx);
        let said = match pending.since {
            None => t!("Subscribed to %{team}", team = pending.name),
            Some(_) => t!("Took in %{team}'s changes", team = pending.name),
        };
        Toast::new(said).key("team-accepted").show(window, cx);
        if !skills.installed.is_empty() {
            Toast::new(t!(
                "Installed the team's skills: %{skills}",
                skills = skills.installed.join(", ")
            ))
            .show(window, cx);
        }
        if !skills.own.is_empty() {
            Toast::new(t!(
                "Skills of your own were left as they are: %{skills}",
                skills = skills.own.join(", ")
            ))
            .show(window, cx);
        }
        if let Some((skill, why)) = skills.failed.first() {
            Toast::error(t!(
                "Couldn't install %{skill}: %{error}",
                skill = skill,
                error = why
            ))
            .show(window, cx);
        }
        // The team's first server, where its secrets are asked for.
        self.server = subscription_server(pending).or(self.server.take());
        self.sync_skills(cx);
    }

    /// What's shown in place of the chosen server while a team is being subscribed to or its changes looked at.
    pub(super) fn teams_panel(&self, cx: &Context<Self>) -> Option<AnyElement> {
        if let Some(subscribing) = &self.teams.subscribing {
            return Some(self.subscribe_panel(subscribing, cx));
        }
        let pending = self
            .teams
            .reviewing
            .as_ref()
            .and_then(|folder| self.teams.updates.get(folder))?;
        Some(self.review_panel(pending, cx))
    }

    fn subscribe_panel(&self, subscribing: &Subscribing, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let found = match &subscribing.state {
            Fetch::Found(pending) => Some((**pending).clone()),
            _ => None,
        };
        let header = self.panel_header(t!("Subscribe to a Team"), cx).child(
            h_flex()
                .gap_1()
                .child(
                    Button::new("cancel-subscribe")
                        .ghost()
                        .xsmall()
                        .label(t!("Cancel").into_owned())
                        .on_click(cx.listener(|this, _, _, cx| this.cancel_subscribing(cx))),
                )
                .child(
                    Button::new("subscribe")
                        .primary()
                        .xsmall()
                        .disabled(found.is_none() || self.teams.busy)
                        .label(t!("Subscribe").into_owned())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(pending) = found.clone() {
                                this.accept_team(pending, window, cx);
                            }
                        })),
                ),
        );
        let state = match &subscribing.state {
            Fetch::Typing => hint(
                t!(
                    "Press Return to fetch it. The repository has a patchbay.toml at its top, naming the team's servers, the secrets each needs and its skills."
                ),
                cx,
            ),
            Fetch::Fetching => hint(t!("Fetching the team…"), cx),
            Fetch::Failed(error) => div()
                .text_base()
                .text_color(theme.danger)
                .child(error.clone()),
            Fetch::Found(pending) => self.changes_list(pending, cx),
        };
        let body = v_flex()
            .gap_5()
            .p_4()
            .child(
                section(t!("Repository"), cx).child(
                    h_flex()
                        .gap_2()
                        .child(div().flex_1().child(small_field(&subscribing.url)))
                        .child(
                            Button::new("fetch-team")
                                .xsmall()
                                .disabled(matches!(subscribing.state, Fetch::Fetching))
                                .label(t!("Fetch").into_owned())
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.fetch_team(window, cx)),
                                ),
                        ),
                ),
            )
            .child(
                h_flex()
                    .gap_4()
                    .child(
                        div()
                            .flex_1()
                            .child(section(t!("Branch"), cx).child(small_field(&subscribing.branch))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .child(section(t!("Kept as"), cx).child(small_field(&subscribing.name))),
                    ),
            )
            .child(section(t!("What subscribing adds"), cx).child(state))
            .child(hint(
                t!(
                    "A team's servers run on your Mac, so look at what each one runs. Its secrets are yours: Patchbay asks for each when its server starts and keeps it in your Keychain."
                ),
                cx,
            ));
        v_flex()
            .size_full()
            .child(header)
            .child(
                div()
                    .id("subscribe")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(body),
            )
            .into_any_element()
    }

    fn review_panel(&self, pending: &Pending, cx: &Context<Self>) -> AnyElement {
        let title = t!("%{team} has changed", team = pending.name).into_owned();
        let accepting = pending.clone();
        let header = self
            .panel_header_beside(
                title,
                shell::code_chip(pending.short_commit().to_string(), cx),
                cx,
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new("review-later")
                            .ghost()
                            .xsmall()
                            .label(t!("Later").into_owned())
                            .on_click(cx.listener(|this, _, _, cx| this.review_later(cx))),
                    )
                    .child(
                        Button::new("accept-team")
                            .primary()
                            .xsmall()
                            .disabled(self.teams.busy)
                            .label(t!("Accept").into_owned())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.accept_team(accepting.clone(), window, cx);
                            })),
                    ),
            );
        let body = v_flex()
            .gap_5()
            .p_4()
            .child(
                section(t!("From"), cx)
                    .child(h_flex().child(shell::code_chip(pending.url.clone(), cx))),
            )
            .child(section(t!("What changes"), cx).child(self.changes_list(pending, cx)))
            .child(hint(
                t!(
                    "Nothing changes until you accept. A team's servers run on your Mac, so look at what each one runs; what it drops goes, with your switches for it."
                ),
                cx,
            ));
        v_flex()
            .size_full()
            .child(header)
            .child(
                div()
                    .id("review-team")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(body),
            )
            .into_any_element()
    }

    /// Each change a team makes, with the fields it touches as the manifest writes them.
    fn changes_list(&self, pending: &Pending, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        if pending.changes.is_empty() {
            return hint(t!("Nothing in its manifest changes."), cx);
        }
        v_flex()
            .gap_3()
            .children(pending.changes.iter().map(|change| {
                v_flex()
                    .gap_1()
                    .p_3()
                    .rounded(theme.radius)
                    .bg(theme.group_box)
                    .child(
                        h_flex()
                            .gap_2()
                            .child(how_badge(change.how).xsmall())
                            .child(
                                div()
                                    .text_color(theme.muted_foreground)
                                    .child(part_name(change.part)),
                            )
                            .child(div().font_semibold().child(change.name.clone()))
                            .when(change.offered, |row| {
                                row.child(hint(t!("Offered: off until you turn it on"), cx))
                            }),
                    )
                    .children(change.fields.iter().map(|field| {
                        let line = match (&field.before, &field.after) {
                            (None, Some(after)) => format!("{} = {after}", field.key),
                            (Some(before), None) => format!("{} = {before}  ✕", field.key),
                            (Some(before), Some(after)) => {
                                format!("{}: {before} → {after}", field.key)
                            }
                            (None, None) => String::new(),
                        };
                        div()
                            .font_family(theme.mono_font_family.clone())
                            .text_sm()
                            .child(line)
                    }))
            }))
    }

    /// A line under the servers' heading for each team with changes to look at.
    pub(super) fn team_updates_banner(&self, cx: &Context<Self>) -> Option<Div> {
        if self.teams.updates.is_empty() {
            return None;
        }
        let theme = cx.theme();
        Some(
            v_flex()
                .flex_none()
                .gap_1()
                .px_3()
                .py_2()
                .border_b_1()
                .border_color(theme.border)
                .children(self.teams.updates.iter().map(|(folder, pending)| {
                    let folder = folder.clone();
                    h_flex()
                        .justify_between()
                        .gap_2()
                        .child(div().text_sm().text_color(theme.warning).child(
                            t!("%{team} has changes to look at", team = pending.name).into_owned(),
                        ))
                        .child(
                            Button::new(SharedString::from(format!("review-{folder}")))
                                .ghost()
                                .xsmall()
                                .label(t!("Review").into_owned())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.review_team(folder.clone(), cx);
                                })),
                        )
                })),
        )
    }
}

/// What checking every team found.
struct Checked {
    /// Each team checked, with its changes if it has any.
    teams: Vec<(String, Option<Pending>)>,
    failed: Vec<(String, String)>,
    /// Whether a check took in a commit by itself, as one that changes only comments is.
    moved: bool,
}

fn check_every_team(home: &Home) -> Checked {
    let before = Subscriptions::read(home).unwrap_or_default();
    let mut checked = Checked {
        teams: Vec::new(),
        failed: Vec::new(),
        moved: false,
    };
    for folder in before.teams.keys() {
        match subscription::check(home, folder) {
            Ok(found) => checked.teams.push((folder.clone(), found)),
            Err(error) => {
                tracing::info!(team = %folder, %error, "team not checked");
                checked.failed.push((folder.clone(), error.to_string()));
            }
        }
    }
    checked.moved = Subscriptions::read(home).unwrap_or_default() != before;
    checked
}

/// The first server a team adds, to show once it's accepted.
fn subscription_server(pending: &Pending) -> Option<String> {
    pending
        .changes
        .iter()
        .find(|change| change.part == Part::Server && change.how == How::Added)
        .map(|change: &Change| change.name.clone())
}

fn how_badge(how: How) -> Tag {
    match how {
        How::Added => shell::badge(Tag::success()).child(t!("Added").into_owned()),
        How::Removed => shell::badge(Tag::danger()).child(t!("Removed").into_owned()),
        How::Changed => shell::badge(Tag::warning()).child(t!("Changed").into_owned()),
    }
}

fn part_name(part: Part) -> String {
    match part {
        Part::Team => t!("Team"),
        Part::Secret => t!("Secret"),
        Part::Server => t!("Server"),
        Part::Profile => t!("Profile"),
        Part::SkillSource => t!("Skills from"),
        Part::Skill => t!("Skill"),
    }
    .into_owned()
}
