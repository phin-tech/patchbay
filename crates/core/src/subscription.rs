//! A team's manifest kept from a git repository: subscribed to by its URL, checked for new commits, and taken in only
//! once what changed has been shown and accepted.
//!
//! A manifest runs commands on the person's Mac, so a fetched one is never put where the gateway reads layers
//! (`teams/<folder>/patchbay.toml`) until it's accepted. Until then it's a [`Pending`], held in memory with the
//! commit it came from, and accepting writes exactly that text. `patchbay.lock` records the commit each team was
//! last accepted at.
//!
//! A team decides what's on and what's only offered: a server it writes `enabled = false` for is in the catalog but
//! off until the person turns it on in their own layer, and of the skills its sources hold, only those it names in
//! `install` are installed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Home;
use crate::clients::Client;
use crate::manifest::{Manifest, edit};
use crate::share::{self, TeamError};
use crate::skills::{self, Folders, Kind, Preview, SkillsError, Source, Stamp, fetch};

/// The file at the top of a team's repository that is its manifest.
pub const MANIFEST: &str = "patchbay.toml";

/// A team's repository, and the commit of it last accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subscription {
    /// What git clones.
    pub url: String,
    /// The branch or tag followed; the default branch if none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub commit: String,
    /// When that commit was accepted.
    pub accepted: String,
}

/// `patchbay.lock`: each subscribed team, by the folder it's kept under.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subscriptions {
    #[serde(default)]
    pub teams: BTreeMap<String, Subscription>,
}

impl Subscriptions {
    /// The lock in `home`, empty if there's none. One that can't be read is an error, never started again, so a team's
    /// record isn't lost to a hand edit gone wrong.
    pub fn read(home: &Home) -> Result<Subscriptions, TeamError> {
        let path = home.subscriptions();
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text)
                .map_err(|error| TeamError::Files(format!("{}: {error}", path.display()))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(Subscriptions::default())
            }
            Err(error) => Err(TeamError::Files(format!("{}: {error}", path.display()))),
        }
    }

    pub fn write(&self, home: &Home) -> Result<(), TeamError> {
        let text = toml::to_string(self).map_err(|error| TeamError::Files(error.to_string()))?;
        let path = home.subscriptions();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| TeamError::Files(error.to_string()))?;
        }
        crate::write_atomically(&path, text.as_bytes())
            .map_err(|error| TeamError::Files(error.to_string()))
    }
}

/// A team's manifest as fetched, waiting to be looked at and accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    /// The folder it's kept under, which begins its secrets' Keychain accounts. It's chosen when subscribing and kept,
    /// whatever the team renames itself to later, so the keys the person gave stay theirs.
    pub folder: String,
    /// The team's name, as its manifest gives it, or its folder.
    pub name: String,
    pub url: String,
    pub branch: Option<String>,
    pub commit: String,
    /// The commit accepted before this one; none for a new subscription.
    pub since: Option<String>,
    /// The manifest, exactly as it is at `commit`.
    pub text: String,
    pub changes: Vec<Change>,
}

impl Pending {
    /// The commit, short, as git shows it.
    pub fn short_commit(&self) -> &str {
        short(&self.commit)
    }
}

/// What part of a manifest a change is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Part {
    Team,
    Secret,
    Server,
    Profile,
    SkillSource,
    Skill,
}

impl Part {
    /// The word for it, as the manifest has it.
    pub fn as_str(self) -> &'static str {
        match self {
            Part::Team => "team",
            Part::Secret => "secret",
            Part::Server => "server",
            Part::Profile => "profile",
            Part::SkillSource => "skills from",
            Part::Skill => "skill",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    Added,
    Removed,
    Changed,
}

/// One thing a manifest adds, removes or changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub part: Part,
    pub name: String,
    pub how: How,
    /// For a server, whether the team leaves it off until the person turns it on.
    pub offered: bool,
    /// Each of its fields that changed, by its key in the manifest (`args`, `env.GITHUB_TOKEN`), as TOML.
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub key: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

/// How to subscribe to a team's repository, beyond its URL.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Asked<'a> {
    /// A branch or tag to follow other than the default branch.
    pub branch: Option<&'a str>,
    /// A name of the person's choosing to keep the team under, rather than the one its manifest gives.
    pub name: Option<&'a str>,
    /// Exactly this commit, already looked at, rather than wherever the branch is now.
    pub at: Option<&'a str>,
}

/// What subscribing to the repository at `url` would add, fetched but not yet kept.
pub fn subscribe(home: &Home, url: &str, asked: Asked) -> Result<Pending, TeamError> {
    let Asked { branch, name, at } = asked;
    let mut source = repository(url, branch)?;
    let follow = source.reference.clone();
    let subscriptions = Subscriptions::read(home)?;
    if let Some((folder, _)) = subscriptions
        .teams
        .iter()
        .find(|(_, subscription)| subscription.url == source.url)
    {
        return Err(TeamError::Subscribed(folder.clone()));
    }
    if let Some(at) = at {
        source.reference = Some(at.to_string());
    }
    let (commit, text, manifest) = fetch_manifest(&source)?;
    let given = name.map(str::trim).filter(|name| !name.is_empty());
    let team_name = manifest.team.as_ref().map(|team| team.name.clone());
    let folder = share::folder(
        given
            .map(str::to_string)
            .or_else(|| team_name.clone())
            .unwrap_or_else(|| repository_name(&source.url))
            .as_str(),
    )?;
    if home.teams().join(&folder).exists() || subscriptions.teams.contains_key(&folder) {
        return Err(TeamError::Taken(folder));
    }
    Ok(Pending {
        name: given
            .map(str::to_string)
            .or(team_name)
            .unwrap_or_else(|| folder.clone()),
        folder,
        url: source.url,
        branch: follow,
        commit,
        since: None,
        changes: changes(&Manifest::default(), &manifest),
        text,
    })
}

/// What the team kept under `folder` has changed since it was last accepted, or none if nothing has. Asks the remote
/// for its commit first, so a team that hasn't moved costs no clone. A new commit that changes nothing in the
/// manifest but its comments is taken in at once: there's nothing in it to look at.
pub fn check(home: &Home, folder: &str) -> Result<Option<Pending>, TeamError> {
    let mut subscriptions = Subscriptions::read(home)?;
    let subscription = subscriptions
        .teams
        .get(folder)
        .cloned()
        .ok_or_else(|| TeamError::NoTeam(folder.to_string()))?;
    let head = fetch::remote_head(&subscription.url, subscription.branch.as_deref())
        .map_err(|error| fetch_error(&subscription.url, &error))?;
    if head == subscription.commit {
        return Ok(None);
    }
    let source = Source {
        kind: Kind::Git,
        url: subscription.url.clone(),
        reference: subscription.branch.clone(),
        subpath: None,
        skill: None,
    };
    let pending = pending_from(home, folder, &subscription, &source)?;
    if pending.commit == subscription.commit {
        return Ok(None);
    }
    if !pending.changes.is_empty() {
        return Ok(Some(pending));
    }
    write_manifest(home, folder, &pending.text)?;
    if let Some(kept) = subscriptions.teams.get_mut(folder) {
        kept.commit = pending.commit;
        kept.accepted = Stamp::now().as_str().to_string();
    }
    subscriptions.write(home)?;
    Ok(None)
}

/// What the team kept under `folder` would change at exactly `commit`, which a person has already looked at: the
/// command line shows a check's changes and takes the commit back to accept, so nothing newer slips in between.
pub fn at_commit(home: &Home, folder: &str, commit: &str) -> Result<Pending, TeamError> {
    let subscriptions = Subscriptions::read(home)?;
    let subscription = subscriptions
        .teams
        .get(folder)
        .ok_or_else(|| TeamError::NoTeam(folder.to_string()))?;
    let source = Source {
        kind: Kind::Git,
        url: subscription.url.clone(),
        reference: Some(commit.to_string()),
        subpath: None,
        skill: None,
    };
    let mut pending = pending_from(home, folder, subscription, &source)?;
    pending.branch.clone_from(&subscription.branch);
    Ok(pending)
}

/// What accepting a team's changes did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accepted {
    pub folder: String,
    /// The servers the team no longer has, whose switches in the person's own manifest went with them.
    pub dropped: Vec<String>,
}

/// Takes `pending` in: its manifest becomes the team's layer, the lock moves to its commit, and the switches the person
/// kept for servers the team dropped are taken out. Refused if the team was accepted at another commit since
/// `pending` was fetched, so two looks at it can't cross.
pub fn accept(home: &Home, pending: &Pending) -> Result<Accepted, TeamError> {
    let mut subscriptions = Subscriptions::read(home)?;
    let current = subscriptions
        .teams
        .get(&pending.folder)
        .map(|kept| kept.commit.clone());
    match (&pending.since, &current) {
        (None, Some(_)) => return Err(TeamError::Taken(pending.folder.clone())),
        (None, None) if home.teams().join(&pending.folder).exists() => {
            return Err(TeamError::Taken(pending.folder.clone()));
        }
        (Some(since), current) if current.as_ref() != Some(since) => {
            return Err(TeamError::Stale(pending.name.clone()));
        }
        _ => {}
    }
    // Read before anything is written, so a manifest that isn't one never replaces the team's.
    let after =
        Manifest::from_toml(&pending.text).map_err(|error| TeamError::Broken(error.to_string()))?;
    let before = std::fs::read_to_string(share::manifest_path(home, &pending.folder))
        .ok()
        .and_then(|text| Manifest::from_toml(&text).ok())
        .unwrap_or_default();
    let dropped: Vec<String> = before
        .servers
        .keys()
        .filter(|name| !after.servers.contains_key(*name))
        .cloned()
        .collect();
    write_manifest(home, &pending.folder, &pending.text)?;
    if !dropped.is_empty() {
        let mine = std::fs::read_to_string(home.manifest()).unwrap_or_default();
        let forgotten = edit::forget_switches(&mine, &dropped)
            .map_err(|error| TeamError::Files(error.to_string()))?;
        if forgotten != mine {
            crate::write_atomically(&home.manifest(), forgotten.as_bytes())
                .map_err(|error| TeamError::Files(error.to_string()))?;
        }
    }
    subscriptions.teams.insert(
        pending.folder.clone(),
        Subscription {
            url: pending.url.clone(),
            branch: pending.branch.clone(),
            commit: pending.commit.clone(),
            accepted: Stamp::now().as_str().to_string(),
        },
    );
    subscriptions.write(home)?;
    Ok(Accepted {
        folder: pending.folder.clone(),
        dropped,
    })
}

/// What installing a team's skills did, skill by skill.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TeamSkills {
    pub installed: Vec<String>,
    /// Installed already, by Patchbay or the `skills` CLI; its updates come through checking skills.
    pub already: Vec<String>,
    /// A skill of the person's own has the name, so it's left as it is.
    pub own: Vec<String>,
    /// Each skill, or source, that couldn't be installed, and why.
    pub failed: Vec<(String, String)>,
}

/// Installs the skills `pending` adds to the team's `install`, from the team's sources, for every client found on this
/// Mac. A source of `.` (or `./skills`) is the team's own repository. A skill already installed, or one of the
/// person's own by that name, is left as it is; nothing is ever removed because a team dropped it.
pub fn install_skills(folders: &Folders, pending: &Pending, now: &Stamp) -> TeamSkills {
    let mut done = TeamSkills::default();
    let mut wanted: Vec<String> = pending
        .changes
        .iter()
        .filter(|change| change.part == Part::Skill && change.how == How::Added)
        .map(|change| change.name.clone())
        .collect();
    if wanted.is_empty() {
        return done;
    }
    let Ok(manifest) = Manifest::from_toml(&pending.text) else {
        return done;
    };
    let lock = skills::Lock::read(folders.lock());
    wanted.retain(|name| {
        let there = lock.get(name).is_some();
        if there {
            done.already.push(name.clone());
        }
        !there
    });
    let clients: Vec<Client> = skills::clients_with_skills()
        .into_iter()
        .filter(|client| client.detected(folders.home()))
        .collect();
    for written in &manifest.skills.sources {
        if wanted.is_empty() {
            break;
        }
        let source = match skill_source(written, pending) {
            Ok(source) => source,
            Err(why) => {
                done.failed.push((written.clone(), why));
                continue;
            }
        };
        let preview = match Preview::fetch(source) {
            Ok(preview) => preview,
            Err(error) => {
                done.failed.push((written.clone(), error.to_string()));
                continue;
            }
        };
        let mut left = Vec::new();
        for name in wanted {
            let Ok(found) = preview.choose(std::slice::from_ref(&name)) else {
                left.push(name);
                continue;
            };
            match skills::install(folders, &preview, &found, &clients, now) {
                Ok(_) => done.installed.push(name),
                Err(SkillsError::NotOurs(_)) => done.own.push(name),
                Err(error) => done.failed.push((name, error.to_string())),
            }
        }
        wanted = left;
    }
    for name in wanted {
        done.failed
            .push((name, "none of the team's skill sources has it".to_string()));
    }
    done
}

/// Every change, as the command line shows it.
pub fn report(pending: &Pending) -> String {
    let mut text = match &pending.since {
        None => format!(
            "{} ({}) at {}, kept as “{}”:\n",
            pending.name,
            pending.url,
            pending.short_commit(),
            pending.folder
        ),
        Some(since) => format!(
            "{} has changed, {} → {}:\n",
            pending.name,
            short(since),
            pending.short_commit()
        ),
    };
    for change in &pending.changes {
        let mark = match change.how {
            How::Added => '+',
            How::Removed => '-',
            How::Changed => '~',
        };
        let offered = if change.offered {
            " (offered: off until you turn it on)"
        } else {
            ""
        };
        text.push_str(&format!(
            "{mark} {} {}{offered}\n",
            change.part.as_str(),
            change.name
        ));
        for field in &change.fields {
            let line = match (&field.before, &field.after) {
                (None, Some(after)) => format!("    {} = {after}\n", field.key),
                (Some(before), None) => format!("    {} = {before} (taken out)\n", field.key),
                (Some(before), Some(after)) => {
                    format!("    {}: {before} → {after}\n", field.key)
                }
                (None, None) => continue,
            };
            text.push_str(&line);
        }
    }
    text
}

/// What changes from `before` to `after`, part by part, each in name order.
pub fn changes(before: &Manifest, after: &Manifest) -> Vec<Change> {
    let mut changes = Vec::new();
    let names = |manifest: &Manifest| manifest.team.as_ref().map(|team| team.name.clone());
    if before.team.is_some() && names(before) != names(after) {
        changes.push(Change {
            part: Part::Team,
            name: names(after).unwrap_or_default(),
            how: How::Changed,
            offered: false,
            fields: vec![Field {
                key: "name".to_string(),
                before: names(before).map(|name| quoted(&name)),
                after: names(after).map(|name| quoted(&name)),
            }],
        });
    }
    compare(&mut changes, Part::Secret, &before.secrets, &after.secrets);
    compare(&mut changes, Part::Server, &before.servers, &after.servers);
    for change in &mut changes {
        if change.part == Part::Server && change.how != How::Removed {
            change.offered = after.servers[&change.name].enabled == Some(false);
        }
    }
    compare(
        &mut changes,
        Part::Profile,
        &before.profiles,
        &after.profiles,
    );
    listed(
        &mut changes,
        Part::SkillSource,
        &before.skills.sources,
        &after.skills.sources,
    );
    listed(
        &mut changes,
        Part::Skill,
        &before.skills.install,
        &after.skills.install,
    );
    changes
}

/// The entries of one table of a manifest that are added, removed or changed, with their fields.
fn compare<T: Serialize>(
    changes: &mut Vec<Change>,
    part: Part,
    before: &BTreeMap<String, T>,
    after: &BTreeMap<String, T>,
) {
    let names: BTreeSet<&String> = before.keys().chain(after.keys()).collect();
    for name in names {
        let (was, is) = (before.get(name).map(flatten), after.get(name).map(flatten));
        let how = match (&was, &is) {
            (None, Some(_)) => How::Added,
            (Some(_), None) => How::Removed,
            (Some(was), Some(is)) if was != is => How::Changed,
            _ => continue,
        };
        let (was, is) = (was.unwrap_or_default(), is.unwrap_or_default());
        let keys: BTreeSet<&String> = if how == How::Removed {
            BTreeSet::new()
        } else {
            was.keys().chain(is.keys()).collect()
        };
        let fields = keys
            .into_iter()
            .filter(|key| was.get(*key) != is.get(*key))
            .map(|key| Field {
                key: key.clone(),
                before: was.get(key).cloned(),
                after: is.get(key).cloned(),
            })
            .collect();
        changes.push(Change {
            part,
            name: name.clone(),
            how,
            offered: false,
            fields,
        });
    }
}

/// The names one list of a manifest gains and loses.
fn listed(changes: &mut Vec<Change>, part: Part, before: &[String], after: &[String]) {
    let names: BTreeSet<&String> = before.iter().chain(after).collect();
    for name in names {
        let how = match (before.contains(name), after.contains(name)) {
            (false, true) => How::Added,
            (true, false) => How::Removed,
            _ => continue,
        };
        changes.push(Change {
            part,
            name: name.clone(),
            how,
            offered: false,
            fields: Vec::new(),
        });
    }
}

/// Each value in `entry`, by its dotted key, as TOML writes it: tables are opened up so a change to one key of `env`
/// shows as that key.
fn flatten<T: Serialize>(entry: &T) -> BTreeMap<String, String> {
    fn walk(prefix: &str, value: &toml::Value, into: &mut BTreeMap<String, String>) {
        match value {
            toml::Value::Table(table) => {
                for (key, value) in table {
                    let key = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{prefix}.{key}")
                    };
                    walk(&key, value, into);
                }
            }
            value => {
                into.insert(prefix.to_string(), value.to_string());
            }
        }
    }
    let mut into = BTreeMap::new();
    if let Ok(value) = toml::Value::try_from(entry) {
        walk("", &value, &mut into);
    }
    into
}

fn quoted(text: &str) -> String {
    toml::Value::String(text.to_string()).to_string()
}

fn short(commit: &str) -> &str {
    commit.get(..7).unwrap_or(commit)
}

/// The repository `url` names, read as `npx skills add` reads a source, so `owner/repo` is GitHub's.
fn repository(url: &str, branch: Option<&str>) -> Result<Source, TeamError> {
    let url = url.trim();
    if url.is_empty() {
        return Err(TeamError::NoUrl);
    }
    let mut source = Source::parse(url, Path::new("."))
        .map_err(|error| TeamError::NotARepository(error.to_string()))?;
    if source.kind == Kind::Local {
        return Err(TeamError::NotARepository(format!(
            "{url} is a folder; give a git repository's URL"
        )));
    }
    if source.subpath.is_some() || source.skill.is_some() {
        return Err(TeamError::NotARepository(format!(
            "give the repository's own URL, with {MANIFEST} at its top"
        )));
    }
    if let Some(branch) = branch.map(str::trim).filter(|branch| !branch.is_empty()) {
        source.reference = Some(branch.to_string());
    }
    Ok(source)
}

/// The last part of a repository's URL, without `.git`: `phin-tech/team.git` is `team`.
fn repository_name(url: &str) -> String {
    let last = url
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or(url);
    last.strip_suffix(".git").unwrap_or(last).to_string()
}

/// The commit `source` is at, and its manifest, as written and as read.
fn fetch_manifest(source: &Source) -> Result<(String, String, Manifest), TeamError> {
    let fetched = fetch::fetch(source).map_err(|error| fetch_error(&source.url, &error))?;
    let commit = fetched.commit().ok_or_else(|| {
        TeamError::Fetch(format!("Couldn't fetch {}: it has no commit", source.url))
    })?;
    let text = std::fs::read_to_string(fetched.root.join(MANIFEST))
        .map_err(|_| TeamError::NoManifest(source.url.clone()))?;
    let manifest =
        Manifest::from_toml(&text).map_err(|error| TeamError::Broken(error.to_string()))?;
    if manifest.servers.is_empty() && manifest.skills.is_empty() {
        return Err(TeamError::NothingShared);
    }
    Ok((commit, text, manifest))
}

/// `source` fetched, as a change to the team kept under `folder`.
fn pending_from(
    home: &Home,
    folder: &str,
    subscription: &Subscription,
    source: &Source,
) -> Result<Pending, TeamError> {
    let (commit, text, manifest) = fetch_manifest(source)?;
    let before = std::fs::read_to_string(share::manifest_path(home, folder))
        .ok()
        .and_then(|text| Manifest::from_toml(&text).ok())
        .unwrap_or_default();
    Ok(Pending {
        folder: folder.to_string(),
        name: manifest
            .team
            .as_ref()
            .map_or_else(|| folder.to_string(), |team| team.name.clone()),
        url: subscription.url.clone(),
        branch: source.reference.clone(),
        commit,
        since: Some(subscription.commit.clone()),
        changes: changes(&before, &manifest),
        text,
    })
}

/// The source a team's manifest names in `[skills] sources`.
fn skill_source(written: &str, pending: &Pending) -> Result<Source, String> {
    let written = written.trim();
    if written == "." || written.starts_with("./") {
        // The team's own repository, at the branch it's followed on, so the skills' updates follow it too.
        let mut source = Source::parse(&pending.url, Path::new(".")).map_err(|e| e.to_string())?;
        source.reference.clone_from(&pending.branch);
        source.subpath = written
            .strip_prefix("./")
            .map(|path| path.trim_matches('/').to_string())
            .filter(|path| !path.is_empty());
        return Ok(source);
    }
    let source = Source::parse(written, Path::new(".")).map_err(|error| error.to_string())?;
    if source.kind == Kind::Local {
        return Err(
            "a folder on someone else's Mac isn't somewhere skills can come from".to_string(),
        );
    }
    Ok(source)
}

/// `error` from git, with what to do about the usual reason a team's (private) repository can't be reached.
fn fetch_error(url: &str, error: &SkillsError) -> TeamError {
    let said = error.to_string();
    let lower = said.to_lowercase();
    let denied = [
        "authentication",
        "could not read username",
        "permission denied",
        "terminal prompts disabled",
        "repository not found",
        "access denied",
    ]
    .iter()
    .any(|word| lower.contains(word));
    if denied {
        TeamError::Denied(url.to_string())
    } else {
        TeamError::Fetch(said)
    }
}

/// Puts `text` in as the team's manifest under `folder`.
fn write_manifest(home: &Home, folder: &str, text: &str) -> Result<(), TeamError> {
    let path = share::manifest_path(home, folder);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| TeamError::Files(error.to_string()))?;
    }
    crate::write_atomically(&path, text.as_bytes())
        .map_err(|error| TeamError::Files(error.to_string()))
}

#[cfg(test)]
mod tests;
