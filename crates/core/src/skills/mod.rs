//! Agent skills, installed the way the `skills` CLI (skills.sh) installs them for every project: each skill copied
//! into `~/.agents/skills/<name>`, linked into the skills folder of each client that keeps its own, and recorded in
//! the lock the CLI keeps. Patchbay and `npx skills` manage the same skills, and neither loses track of the other's.
//!
//! What's in a client's folder that no lock records, a folder or a link of someone's own, is never replaced or
//! removed.

use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::clients::Client;

pub mod discover;
mod fetch;
mod hash;
mod lock;
pub mod request;
pub mod source;

pub use discover::Found;
pub use hash::folder_hash;
pub use lock::{Entry, Lock};
pub use request::Request;
pub use source::{Kind, Source};

/// Why skills couldn't be fetched, installed or removed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SkillsError {
    #[error(transparent)]
    Source(#[from] source::SourceError),
    #[error("Couldn't fetch {0}")]
    Fetch(String),
    #[error("There are no skills in {0}")]
    NoSkills(String),
    #[error("There's no skill called {name} in {from}")]
    NoSuchSkill { name: String, from: String },
    #[error("{0} wasn't installed by Patchbay or the skills CLI, so it's left as it is")]
    NotOurs(String),
    #[error("There's no skill called {0} installed")]
    NotInstalled(String),
    #[error("{0} is already kept by Patchbay or the skills CLI")]
    AlreadyKept(String),
    #[error("{0} reads every skill in ~/.agents/skills, so it can't be left out of one")]
    ReadsStore(String),
    #[error("{0} can't be read, so it's left as it is: move it aside and try again")]
    UnreadableLock(String),
    /// A file or folder couldn't be read or written: what went wrong, by its kind for a caller to match on, and as it
    /// was said.
    #[error("{message}")]
    Files {
        kind: std::io::ErrorKind,
        message: String,
    },
}

impl From<std::io::Error> for SkillsError {
    fn from(error: std::io::Error) -> SkillsError {
        SkillsError::Files {
            kind: error.kind(),
            message: error.to_string(),
        }
    }
}

/// Where skills go, under a home folder: the person's own, or a stand-in for tests and captures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folders {
    home: PathBuf,
    /// `~/.claude`, or `CLAUDE_CONFIG_DIR`.
    claude: PathBuf,
    /// `~/.config`, or `XDG_CONFIG_HOME`.
    config: PathBuf,
    lock: PathBuf,
    /// Folders chosen in Settings in place of a client's own.
    chosen: Vec<(Client, PathBuf)>,
}

impl Folders {
    /// Every folder under `home`, ignoring the environment.
    pub fn new(home: impl Into<PathBuf>) -> Folders {
        let home = home.into();
        Folders {
            claude: home.join(".claude"),
            config: home.join(".config"),
            lock: home.join(".agents/.skill-lock.json"),
            chosen: Vec::new(),
            home,
        }
    }

    /// These folders, with the skills folders `settings` chooses for clients in place of their own. A path starting
    /// `~/` is under the home folder; a slug no client has is passed over.
    pub fn chosen_in(mut self, settings: &crate::Settings) -> Folders {
        self.chosen = settings
            .skills_folders
            .iter()
            .filter_map(|(slug, path)| {
                let client = Client::from_slug(slug)?;
                let path = match path.strip_prefix("~/") {
                    Some(rest) => self.home.join(rest),
                    None => PathBuf::from(path),
                };
                Some((client, path))
            })
            .collect();
        self
    }

    /// The folders under `home` the CLI would use, moved where the environment moves them.
    pub fn from_env(home: impl Into<PathBuf>) -> Folders {
        let mut folders = Folders::new(home);
        let set = |name: &str| {
            std::env::var_os(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        if let Some(claude) = set("CLAUDE_CONFIG_DIR") {
            folders.claude = claude;
        }
        if let Some(config) = set("XDG_CONFIG_HOME") {
            folders.config = config;
        }
        if let Some(state) = set("XDG_STATE_HOME") {
            folders.lock = state.join("skills/.skill-lock.json");
        }
        folders
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    /// Where every skill is kept: `~/.agents/skills`.
    pub fn store(&self) -> PathBuf {
        self.home.join(".agents/skills")
    }

    pub fn lock(&self) -> &Path {
        &self.lock
    }

    /// The skills folder `client` reads, as the `skills` CLI has it; none for a client that has no skills folder
    /// or isn't known to read one.
    pub fn client(&self, client: Client) -> Option<PathBuf> {
        if let Some((_, chosen)) = self.chosen.iter().find(|(chosen, _)| *chosen == client) {
            return Some(chosen.clone());
        }
        self.usual(client)
    }

    /// The skills folder `client` reads unless Settings chooses another, as the `skills` CLI has it.
    pub fn usual(&self, client: Client) -> Option<PathBuf> {
        match client {
            Client::ClaudeCode => Some(self.claude.join("skills")),
            Client::Codex | Client::Pi => Some(self.store()),
            Client::Cursor => Some(self.home.join(".cursor/skills")),
            Client::VsCode => Some(self.home.join(".copilot/skills")),
            Client::GeminiCli => Some(self.home.join(".gemini/skills")),
            Client::OpenCode => Some(self.config.join("opencode/skills")),
            Client::ClaudeDesktop | Client::Omp | Client::Pig => None,
        }
    }

    /// Whether `client` reads the store itself, so it needs no link.
    pub fn reads_store(&self, client: Client) -> bool {
        self.client(client)
            .is_some_and(|folder| folder == self.store())
    }
}

/// The clients that have a skills folder, in Patchbay's order.
pub fn clients_with_skills() -> Vec<Client> {
    let folders = Folders::new("/");
    Client::ALL
        .into_iter()
        .filter(|client| folders.usual(*client).is_some())
        .collect()
}

/// A client by the name the command line or the `skills` CLI gives it (`claude-code`, `gemini-cli`,
/// `github-copilot`).
pub fn client_named(name: &str) -> Option<Client> {
    match name {
        "gemini-cli" => Some(Client::GeminiCli),
        "github-copilot" => Some(Client::VsCode),
        name => Client::from_slug(name),
    }
}

/// The folder a skill is installed in, made from its name as the CLI makes it: `Web Design` is `web-design`.
pub fn folder_name(name: &str) -> String {
    let mut folder = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_' {
            folder.push(c);
        } else if !folder.ends_with('-') {
            folder.push('-');
        }
    }
    let folder = folder.trim_matches(|c| c == '.' || c == '-');
    let folder: String = folder.chars().take(255).collect();
    if folder.is_empty() {
        "unnamed-skill".to_string()
    } else {
        folder
    }
}

/// A time as the lock records it, in `installedAt` and `updatedAt`: UTC to the millisecond, as the `skills` CLI writes
/// it, `2026-11-19T10:00:00.000Z`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp(String);

impl Stamp {
    pub fn now() -> Stamp {
        Stamp(
            jiff::Timestamp::now()
                .strftime("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string(),
        )
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One as written, as tests give them.
impl From<&str> for Stamp {
    fn from(text: &str) -> Stamp {
        Stamp(text.to_string())
    }
}

/// A source fetched and looked through, its files kept until it's dropped.
pub struct Preview {
    pub source: Source,
    pub found: Vec<Found>,
    fetched: fetch::Fetched,
}

impl Preview {
    /// Fetches `source` and finds its skills.
    pub fn fetch(source: Source) -> Result<Preview, SkillsError> {
        let fetched = fetch::fetch(&source)?;
        let found = discover::discover(&fetched.root, source.subpath.as_deref());
        if found.is_empty() {
            return Err(SkillsError::NoSkills(source.lock_source()));
        }
        Ok(Preview {
            source,
            found,
            fetched,
        })
    }

    /// The skills `names` asks for, by name or folder: every one for `*`, the one the source names if `names` is
    /// empty, or none.
    pub fn choose(&self, names: &[String]) -> Result<Vec<&Found>, SkillsError> {
        let wanted: Vec<&str> = match (&self.source.skill, names.is_empty()) {
            (Some(skill), true) => vec![skill.as_str()],
            _ => names.iter().map(String::as_str).collect(),
        };
        if wanted.contains(&"*") {
            return Ok(self.found.iter().collect());
        }
        wanted
            .into_iter()
            .map(|name| {
                self.found
                    .iter()
                    .find(|found| {
                        folder_name(&found.name) == folder_name(name)
                            || found.path.file_name().is_some_and(|folder| folder == name)
                    })
                    .ok_or_else(|| SkillsError::NoSuchSkill {
                        name: name.to_string(),
                        from: self.source.lock_source(),
                    })
            })
            .collect()
    }

    /// What the lock records for `found`.
    fn entry(&self, found: &Found) -> Entry {
        let folder = found
            .path
            .strip_prefix(&self.fetched.root)
            .ok()
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let skill_path = if folder.is_empty() {
            "SKILL.md".to_string()
        } else {
            format!("{folder}/SKILL.md")
        };
        let folder_hash = match self.source.kind {
            Kind::Github => self.fetched.tree_hash(&folder),
            _ => None,
        }
        .or_else(|| folder_hash(&found.path).ok())
        .unwrap_or_default();
        Entry {
            source: self.source.lock_source(),
            source_type: self.source.kind.as_str().to_string(),
            source_url: self.source.url.clone(),
            reference: self.source.reference.clone(),
            skill_path: Some(skill_path),
            folder_hash,
            plugin: found.plugin.clone(),
            installed_at: None,
            updated_at: None,
        }
    }
}

/// What installing one skill did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub name: String,
    pub folder: PathBuf,
    /// The clients that have it, by a link or by reading the store.
    pub linked: Vec<Client>,
    /// The clients whose folder already holds something else of that name, left as it is.
    pub blocked: Vec<Client>,
}

/// Installs each of `chosen` from `preview` and links it into each of `clients`' folders.
pub fn install(
    folders: &Folders,
    preview: &Preview,
    chosen: &[&Found],
    clients: &[Client],
    now: &Stamp,
) -> Result<Vec<Installed>, SkillsError> {
    let mut lock = read_lock(folders)?;
    // Every skill is checked before any is copied, so a refusal leaves nothing half done.
    for found in chosen {
        let folder = folder_name(&found.name);
        let target = folders.store().join(&folder);
        if exists(&target) && lock.name_for_folder(&folder).is_none() {
            return Err(SkillsError::NotOurs(target.display().to_string()));
        }
    }
    let mut installed = Vec::new();
    for found in chosen {
        let folder = folder_name(&found.name);
        let target = folders.store().join(&folder);
        copy_into_place(&found.path, &target)?;
        let (linked, blocked) = link(folders, &folder, clients)?;
        // A skill renamed in its frontmatter but kept in the same folder replaces the old name.
        if let Some(old) = lock
            .name_for_folder(&folder)
            .filter(|old| *old != found.name)
        {
            lock.remove(&old);
        }
        lock.set(&found.name, &preview.entry(found), now);
        // Recorded as each is put in place, so a failure later never leaves one in the store unrecorded.
        lock.write(folders.lock())?;
        installed.push(Installed {
            name: found.name.clone(),
            folder: target,
            linked,
            blocked,
        });
    }
    Ok(installed)
}

fn read_lock(folders: &Folders) -> Result<Lock, SkillsError> {
    let lock = Lock::read(folders.lock());
    if lock.unreadable() {
        return Err(SkillsError::UnreadableLock(
            folders.lock().display().to_string(),
        ));
    }
    Ok(lock)
}

/// Whether anything, even a broken link, is at `path`.
fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// Whether the skill at `at` can give way to a link to `store` with nothing lost. The hash the CLI uses isn't enough:
/// it leaves out `.git` and `metadata.json` and reads CRLF as LF, so a git checkout or a copy with other line endings
/// would hash the same and be removed. Every byte has to match, and nothing in `at` may be one the copy leaves out.
fn same_as_store(at: &Path, store: &Path) -> bool {
    let files = |folder: &Path| -> std::io::Result<Vec<(String, Vec<u8>)>> {
        let mut files = Vec::new();
        hash::collect_files(folder, &fs::canonicalize(folder)?, &mut files)?;
        files.sort();
        Ok(files)
    };
    hash::copies_whole(at).unwrap_or(false)
        && matches!((files(at), files(store)), (Ok(ours), Ok(kept)) if ours == kept)
}

/// Copies the skill at `from` to `to` beside it first, then puts it in place, so a failure leaves the old one.
fn copy_into_place(from: &Path, to: &Path) -> Result<(), SkillsError> {
    let parent = to.parent().ok_or_else(|| SkillsError::Files {
        kind: std::io::ErrorKind::InvalidInput,
        message: format!("{} has no folder to go in", to.display()),
    })?;
    fs::create_dir_all(parent)?;
    let name = to
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let staging = parent.join(format!(".{name}.patchbay-{}", std::process::id()));
    let _ = fs::remove_dir_all(&staging);
    let root = fs::canonicalize(from)?;
    copy_folder(from, &staging, &root, &mut Vec::new()).inspect_err(|_| {
        let _ = fs::remove_dir_all(&staging);
    })?;
    remove_any(to)?;
    fs::rename(&staging, to)?;
    Ok(())
}

/// Removes a folder, a file or a link at `path`, if anything is there.
fn remove_any(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(_) => Ok(()),
    }
}

/// Copies what the CLI copies: every file but the ones it leaves out, following links that stay in the skill.
fn copy_folder(
    from: &Path,
    to: &Path,
    root: &Path,
    ancestors: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    let real = fs::canonicalize(from)?;
    if ancestors.contains(&real) {
        return Ok(());
    }
    ancestors.push(real);
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        let path = entry.path();
        let kind = entry.file_type()?;
        let is_folder = if kind.is_symlink() {
            match fs::canonicalize(&path) {
                Ok(target) if target.starts_with(root) => target.is_dir(),
                _ => continue,
            }
        } else {
            kind.is_dir()
        };
        if hash::excluded(&name.to_string_lossy(), is_folder) {
            continue;
        }
        if is_folder {
            copy_folder(&path, &to.join(&name), root, ancestors)?;
        } else if kind.is_file() || kind.is_symlink() {
            fs::copy(&path, to.join(&name))?;
        }
    }
    ancestors.pop();
    Ok(())
}

/// Links the skill in the store's `folder` into each of `clients`' skills folders: the clients it reaches, and
/// those whose folder already holds something else by that name.
fn link(
    folders: &Folders,
    folder: &str,
    clients: &[Client],
) -> Result<(Vec<Client>, Vec<Client>), SkillsError> {
    let target = folders.store().join(folder);
    let (mut linked, mut blocked) = (Vec::new(), Vec::new());
    for &client in clients {
        let Some(skills) = folders.client(client) else {
            continue;
        };
        if skills == folders.store() {
            linked.push(client);
            continue;
        }
        let at = skills.join(folder);
        if exists(&at) {
            if points_at(&at, &target) {
                linked.push(client);
            } else {
                blocked.push(client);
            }
            continue;
        }
        fs::create_dir_all(&skills)?;
        let real = fs::canonicalize(&skills).unwrap_or(skills);
        let real_target = fs::canonicalize(&target).unwrap_or_else(|_| target.clone());
        std::os::unix::fs::symlink(relative_path(&real, &real_target), &at)?;
        linked.push(client);
    }
    Ok((linked, blocked))
}

/// Gives `client` the skill in the store's `folder` too, by a link in its skills folder.
pub fn link_client(folders: &Folders, folder: &str, client: Client) -> Result<(), SkillsError> {
    if !folders.store().join(folder).join("SKILL.md").is_file() {
        return Err(SkillsError::NotInstalled(folder.to_string()));
    }
    let (_, blocked) = link(folders, folder, &[client])?;
    if blocked.is_empty() {
        Ok(())
    } else {
        let at = folders.client(client).unwrap_or_default().join(folder);
        Err(SkillsError::NotOurs(at.display().to_string()))
    }
}

impl Entry {
    /// Whether the skill was taken over as it was on this Mac, so the store is its source.
    pub fn taken_over(&self, folders: &Folders) -> bool {
        self.source_type == Kind::Local.as_str()
            && Path::new(&self.source_url).starts_with(folders.store())
    }
}

/// What taking over a skill did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakenOver {
    pub name: String,
    /// The clients whose copy is now a link to the store.
    pub linked: Vec<Client>,
    /// The clients whose copy differs from the store's, or is a link elsewhere, left as it is.
    pub kept: Vec<Client>,
}

/// Puts a skill of the person's own in Patchbay's care: copied into the store if it isn't there, recorded in the lock
/// as a skill from this Mac (which the `skills` CLI never fetches or updates), and each client's copy replaced by a
/// link to the store, only where its files are the same. Nothing that differs is replaced, so nothing is lost.
pub fn take_over(folders: &Folders, folder: &str, now: &Stamp) -> Result<TakenOver, SkillsError> {
    let mut lock = read_lock(folders)?;
    if let Some(name) = lock.name_for_folder(folder) {
        return Err(SkillsError::AlreadyKept(name));
    }
    let store = folders.store().join(folder);
    let mut clients: Vec<(Client, PathBuf)> = Vec::new();
    for client in Client::ALL {
        if let Some(skills) = folders
            .client(client)
            .filter(|skills| *skills != folders.store())
        {
            clients.push((client, skills.join(folder)));
        }
    }
    if !store.join("SKILL.md").is_file() {
        // The first client's copy, a folder of its own rather than a link, becomes the store's.
        let from = clients
            .iter()
            .map(|(_, at)| at)
            .find(|at| {
                at.join("SKILL.md").is_file()
                    && fs::symlink_metadata(at).is_ok_and(|meta| meta.is_dir())
            })
            .ok_or_else(|| SkillsError::NotInstalled(folder.to_string()))?;
        copy_into_place(from, &store)?;
    }
    let kept_hash = folder_hash(&store)?;
    let (mut linked, mut kept) = (Vec::new(), Vec::new());
    for (client, at) in clients {
        if !exists(&at) {
            continue;
        }
        if points_at(&at, &store) {
            linked.push(client);
            continue;
        }
        let own_folder = fs::symlink_metadata(&at).is_ok_and(|meta| meta.is_dir());
        if own_folder && same_as_store(&at, &store) {
            fs::remove_dir_all(&at)?;
            let parent = at.parent().unwrap_or(&at);
            let real = fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
            let real_store = fs::canonicalize(&store).unwrap_or_else(|_| store.clone());
            std::os::unix::fs::symlink(relative_path(&real, &real_store), &at)?;
            linked.push(client);
        } else {
            kept.push(client);
        }
    }
    let name = fs::read_to_string(store.join("SKILL.md"))
        .ok()
        .and_then(|text| discover::frontmatter(&text))
        .map_or_else(|| folder.to_string(), |(name, _)| name);
    let path = store.display().to_string();
    let entry = Entry {
        source: path.clone(),
        source_type: Kind::Local.as_str().to_string(),
        source_url: path,
        reference: None,
        skill_path: Some("SKILL.md".to_string()),
        folder_hash: kept_hash,
        plugin: None,
        installed_at: None,
        updated_at: None,
    };
    lock.set(&name, &entry, now);
    lock.write(folders.lock())?;
    Ok(TakenOver { name, linked, kept })
}

/// Takes the link to the store's `folder` out of `client`'s skills folder. A client that reads the store itself
/// can't be left out this way.
pub fn unlink_client(folders: &Folders, folder: &str, client: Client) -> Result<(), SkillsError> {
    let Some(skills) = folders
        .client(client)
        .filter(|skills| *skills != folders.store())
    else {
        return Err(SkillsError::ReadsStore(client.name().to_string()));
    };
    let at = skills.join(folder);
    if !points_at(&at, &folders.store().join(folder)) {
        return Err(SkillsError::NotOurs(at.display().to_string()));
    }
    fs::remove_file(&at)?;
    Ok(())
}

/// Whether the link at `at` leads to `target`.
fn points_at(at: &Path, target: &Path) -> bool {
    let is_link = fs::symlink_metadata(at).is_ok_and(|meta| meta.file_type().is_symlink());
    is_link
        && match (fs::canonicalize(at), fs::canonicalize(target)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
}

/// `to` written relative to the folder `from`, as the CLI writes its links: `../../.agents/skills/x`.
fn relative_path(from: &Path, to: &Path) -> PathBuf {
    let from: Vec<Component> = from.components().collect();
    let to_parts: Vec<Component> = to.components().collect();
    let shared = from
        .iter()
        .zip(&to_parts)
        .take_while(|(a, b)| a == b)
        .count();
    let mut path = PathBuf::new();
    for _ in shared..from.len() {
        path.push("..");
    }
    for part in &to_parts[shared..] {
        path.push(part);
    }
    path
}

/// Removes the skill `name` (or the one in that folder): its links in every client's folder, its folder in the
/// store, and its place in the lock. One no lock records is left as it is.
pub fn remove(folders: &Folders, name: &str) -> Result<String, SkillsError> {
    let mut lock = read_lock(folders)?;
    let folder = folder_name(name);
    let Some(locked) = lock.name_for_folder(&folder) else {
        let anywhere = exists(&folders.store().join(&folder))
            || Client::ALL
                .iter()
                .filter_map(|client| folders.client(*client))
                .any(|skills| exists(&skills.join(&folder)));
        return Err(if anywhere {
            SkillsError::NotOurs(name.to_string())
        } else {
            SkillsError::NotInstalled(name.to_string())
        });
    };
    let target = folders.store().join(&folder);
    let mut seen = Vec::new();
    for skills in Client::ALL
        .iter()
        .filter_map(|client| folders.client(*client))
    {
        if skills == folders.store() || seen.contains(&skills) {
            continue;
        }
        let at = skills.join(&folder);
        if points_at(&at, &target) {
            fs::remove_file(&at)?;
        }
        seen.push(skills);
    }
    remove_any(&target)?;
    lock.remove(&locked);
    lock.write(folders.lock())?;
    Ok(locked)
}

/// Who looks after a skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Keeper {
    /// Recorded in the lock Patchbay shares with the `skills` CLI: either installed it.
    Locked(Box<Entry>),
    /// A folder or a link of someone's own.
    Unmanaged,
}

/// How a client has a skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// A link to the store, or the store itself.
    Linked,
    /// A folder or a link of its own, elsewhere.
    Own,
}

/// A skill on this Mac, wherever it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    /// Its folder's name.
    pub folder: String,
    /// The name the lock or its `SKILL.md` gives it.
    pub name: String,
    pub description: Option<String>,
    pub keeper: Keeper,
    /// Whether it's in the store.
    pub stored: bool,
    /// Each client that has it, and how.
    pub clients: Vec<(Client, Reach)>,
    /// Where its `SKILL.md` is.
    pub path: Option<PathBuf>,
}

impl Skill {
    pub fn managed(&self) -> bool {
        matches!(self.keeper, Keeper::Locked(_))
    }
}

/// Every skill in the store or in any client's folder, by folder name.
pub fn list(folders: &Folders) -> Vec<Skill> {
    let lock = Lock::read(folders.lock());
    let store = folders.store();
    let folders_in = |path: &Path| -> Vec<String> {
        fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
            .filter(|name| !name.starts_with('.'))
            .collect()
    };
    let mut names: Vec<String> = folders_in(&store);
    for client in Client::ALL {
        if let Some(skills) = folders.client(client) {
            names.extend(folders_in(&skills));
        }
    }
    names.sort();
    names.dedup();
    names
        .into_iter()
        .map(|folder| {
            let target = store.join(&folder);
            let stored = target.join("SKILL.md").is_file();
            let mut clients = Vec::new();
            for client in Client::ALL {
                let Some(skills) = folders.client(client) else {
                    continue;
                };
                let at = skills.join(&folder);
                if skills == store {
                    if stored {
                        clients.push((client, Reach::Linked));
                    }
                } else if points_at(&at, &target) {
                    clients.push((client, Reach::Linked));
                } else if at.join("SKILL.md").is_file() {
                    clients.push((client, Reach::Own));
                }
            }
            let path = std::iter::once(target.clone())
                .chain(
                    Client::ALL
                        .iter()
                        .filter_map(|client| Some(folders.client(*client)?.join(&folder))),
                )
                .map(|path| path.join("SKILL.md"))
                .find(|path| path.is_file());
            let read = path
                .as_ref()
                .and_then(|path| fs::read_to_string(path).ok())
                .and_then(|text| discover::frontmatter(&text));
            let locked = lock.name_for_folder(&folder).filter(|_| stored);
            let keeper = match locked.as_ref().and_then(|name| lock.get(name)) {
                Some(entry) => Keeper::Locked(Box::new(entry)),
                None => Keeper::Unmanaged,
            };
            let name = locked
                .or_else(|| read.as_ref().map(|(name, _)| name.clone()))
                .unwrap_or_else(|| folder.clone());
            Skill {
                folder,
                name,
                description: read.map(|(_, description)| description),
                keeper,
                stored,
                clients,
                path,
            }
        })
        .collect()
}

/// A change to one file of a skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Added(String),
    Removed(String),
    Changed(String),
}

/// A skill whose source has moved on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    pub name: String,
    pub source: String,
    /// What would change in its folder, file by file.
    pub changes: Vec<Change>,
    /// The hash of the skill as it was found, so applying it puts in place what was listed and nothing newer.
    pub hash: String,
}

/// What [`check`] found: the updates, and the skills it couldn't check and why.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Checked {
    pub updates: Vec<Update>,
    pub unchecked: Vec<(String, String)>,
}

/// What [`check`] does with a change it finds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checking {
    /// Says what would change.
    Report,
    /// Puts in place the updates a report listed, and only those: one whose source has moved on again since is left,
    /// and said so, since what it would put in place is no longer what was listed.
    Apply(Vec<Update>),
}

/// The skills in the lock from one source at one ref, so they share one fetch.
type BySource = Vec<((String, Option<String>), Vec<(String, Entry)>)>;

/// Fetches the source of each skill in the lock (or only `names`) and says which have changed. Skills from one
/// source share one fetch.
pub fn check(
    folders: &Folders,
    names: &[String],
    checking: Checking,
    now: &Stamp,
) -> Result<Checked, SkillsError> {
    let listed = match &checking {
        Checking::Report => None,
        Checking::Apply(updates) => Some(updates),
    };
    let lock = read_lock(folders)?;
    let mut wanted: Vec<String> = names.iter().map(|name| folder_name(name)).collect();
    if let Some(listed) = listed {
        let named: Vec<String> = listed
            .iter()
            .map(|update| folder_name(&update.name))
            .filter(|folder| wanted.is_empty() || wanted.contains(folder))
            .collect();
        if named.is_empty() {
            return Ok(Checked::default());
        }
        wanted = named;
    }
    let mut by_source: BySource = Vec::new();
    let mut checked = Checked::default();
    for name in lock.names() {
        if !wanted.is_empty() && !wanted.contains(&folder_name(&name)) {
            continue;
        }
        let Some(entry) = lock.get(&name) else {
            continue;
        };
        // A skill taken over from this Mac is its own source; there's nothing to fetch.
        if entry.taken_over(folders) {
            continue;
        }
        if !matches!(
            entry.source_type.as_str(),
            "github" | "gitlab" | "git" | "local"
        ) || entry.skill_path.is_none()
            || entry.source_url.is_empty()
        {
            checked.unchecked.push((
                name,
                format!(
                    "its source ({}) is one only the skills CLI fetches",
                    entry.source_type
                ),
            ));
            continue;
        }
        let key = (entry.source_url.clone(), entry.reference.clone());
        match by_source.iter_mut().find(|(k, _)| *k == key) {
            Some((_, skills)) => skills.push((name, entry)),
            None => by_source.push((key, vec![(name, entry)])),
        }
    }
    for ((url, reference), skills) in by_source {
        let kind = match skills[0].1.source_type.as_str() {
            "github" => Kind::Github,
            "gitlab" => Kind::Gitlab,
            "local" => Kind::Local,
            _ => Kind::Git,
        };
        let source = Source {
            kind,
            url,
            reference,
            subpath: None,
            skill: None,
        };
        let fetched = match fetch::fetch(&source) {
            Ok(fetched) => fetched,
            Err(error) => {
                for (name, _) in skills {
                    checked.unchecked.push((name, error.to_string()));
                }
                continue;
            }
        };
        for (name, entry) in skills {
            let skill_path = entry.skill_path.clone().unwrap_or_default();
            let folder = skill_path
                .strip_suffix("SKILL.md")
                .unwrap_or(&skill_path)
                .trim_end_matches('/');
            let from = fetched.root.join(folder);
            // An update is held to the source's folder as discovery is: a link there now could point anywhere.
            let inside = fs::canonicalize(&fetched.root).unwrap_or_else(|_| fetched.root.clone());
            if !from.join("SKILL.md").is_file() || !discover::within(&from, &inside) {
                checked.unchecked.push((
                    name,
                    format!("{skill_path} is no longer in {}", entry.source),
                ));
                continue;
            }
            let tree = entry.folder_hash.len() == 40 && entry.source_type == "github";
            let latest = if tree {
                fetched.tree_hash(folder)
            } else {
                folder_hash(&from).ok()
            };
            let target = folders.store().join(folder_name(&name));
            if latest.as_deref() == Some(entry.folder_hash.as_str()) && target.is_dir() {
                continue;
            }
            let changes = changes(&target, &from);
            let hash = latest.unwrap_or_default();
            if let Some(listed) = listed {
                let as_listed = listed
                    .iter()
                    .any(|update| update.name == name && update.hash == hash);
                if !as_listed {
                    checked
                        .unchecked
                        .push((name, "it changed again since it was checked".to_string()));
                    continue;
                }
                copy_into_place(&from, &target)?;
                // Read again and written as each is put in place, as installing does: a fetch can take minutes, and
                // the lock read before it may since have gained or lost a skill, here or through the `skills` CLI.
                let mut lock = read_lock(folders)?;
                let mut entry = lock.get(&name).unwrap_or_else(|| entry.clone());
                entry.folder_hash = hash.clone();
                lock.set(&name, &entry, now);
                lock.write(folders.lock())?;
            }
            checked.updates.push(Update {
                name,
                source: entry.source.clone(),
                changes,
                hash,
            });
        }
    }
    Ok(checked)
}

/// The files that differ between the skill folder `old` and `new`, by path.
fn changes(old: &Path, new: &Path) -> Vec<Change> {
    let files = |folder: &Path| -> Vec<(String, Vec<u8>)> {
        let mut files = Vec::new();
        if let Ok(root) = fs::canonicalize(folder) {
            let _ = hash::collect_files(folder, &root, &mut files);
        }
        files.sort();
        files
    };
    let (old, new) = (files(old), files(new));
    let mut changes = Vec::new();
    for (path, contents) in &new {
        match old.iter().find(|(old, _)| old == path) {
            None => changes.push(Change::Added(path.clone())),
            Some((_, was)) if was != contents => changes.push(Change::Changed(path.clone())),
            Some(_) => {}
        }
    }
    for (path, _) in &old {
        if !new.iter().any(|(new, _)| new == path) {
            changes.push(Change::Removed(path.clone()));
        }
    }
    changes
}

#[cfg(test)]
mod tests;
