//! Getting a source's files: a shallow `git clone` into a folder of its own, removed when done, or the folder itself
//! for a source on this Mac. git asks for nothing, so a private repository fails rather than waiting at a prompt no
//! one can see, and a clone that hangs is stopped.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use super::SkillsError;
use super::source::{Kind, Source};

/// How long a clone may take.
const CLONE_TIMEOUT: Duration = Duration::from_secs(300);

/// A source's files, where they can be read.
pub struct Fetched {
    pub root: PathBuf,
    /// Whether `root` is a clone, removed when this is dropped.
    temporary: bool,
}

impl Drop for Fetched {
    fn drop(&mut self) {
        if self.temporary {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

impl Fetched {
    /// Where its links may lead: only inside a clone of someone's repository, anywhere in a folder of the person's
    /// own.
    pub fn links(&self) -> super::discover::Links {
        if self.temporary {
            super::discover::Links::StayInside
        } else {
            super::discover::Links::Followed
        }
    }

    /// The git tree hash of `folder` in the clone, which is what the lock holds for a GitHub source: GitHub's API
    /// gives the same one.
    pub fn tree_hash(&self, folder: &str) -> Option<String> {
        if !self.temporary {
            return None;
        }
        let spec = if folder.is_empty() {
            "HEAD^{tree}".to_string()
        } else {
            format!("HEAD:{folder}")
        };
        let output = git(&self.root).args(["rev-parse", &spec]).output().ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

/// Fetches `source`.
pub fn fetch(source: &Source) -> Result<Fetched, SkillsError> {
    if source.kind == Kind::Local {
        let root = PathBuf::from(&source.url);
        if !root.is_dir() {
            return Err(SkillsError::Fetch(format!(
                "there's no folder at {}",
                root.display()
            )));
        }
        return Ok(Fetched {
            root,
            temporary: false,
        });
    }
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "patchbay-skill-{}-{}",
        std::process::id(),
        COUNT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&root);
    let fetched = Fetched {
        root: root.clone(),
        temporary: true,
    };
    let commit = source.reference.as_deref().filter(|reference| {
        reference.len() == 40 && reference.chars().all(|c| c.is_ascii_hexdigit())
    });
    if let Some(commit) = commit {
        // A commit can't be cloned by name, only fetched.
        std::fs::create_dir_all(&root).map_err(|error| SkillsError::Fetch(error.to_string()))?;
        run(git(&root).args(["init", "-q"]), &source.url)?;
        run(
            git(&root).args(["remote", "add", "origin", &source.url]),
            &source.url,
        )?;
        run(
            git(&root).args(["fetch", "-q", "--depth", "1", "origin", commit]),
            &source.url,
        )?;
        run(
            git(&root).args(["checkout", "-q", "FETCH_HEAD"]),
            &source.url,
        )?;
    } else {
        let mut clone = git(Path::new("."));
        clone.args(["clone", "-q", "--depth", "1"]);
        if let Some(reference) = &source.reference {
            clone.args(["--branch", reference]);
        }
        clone.arg("--").arg(&source.url).arg(&root);
        run(&mut clone, &source.url)?;
    }
    Ok(fetched)
}

/// git in `folder`, asking for nothing.
fn git(folder: &Path) -> Command {
    let mut git = Command::new("git");
    git.current_dir(folder)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "")
        .env("SSH_ASKPASS", "")
        .env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes")
        // Only the protocols the CLI clones over.
        .env("GIT_ALLOW_PROTOCOL", "https:http:ssh:git:file")
        .stdin(Stdio::null());
    git
}

/// Runs a git command to its end, or stops it after [`CLONE_TIMEOUT`].
fn run(command: &mut Command, url: &str) -> Result<(), SkillsError> {
    let mut child = command
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| SkillsError::Fetch(format!("couldn't run git: {error}")))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => {
                let mut said = String::new();
                if let Some(mut stderr) = child.stderr.take() {
                    let _ = std::io::Read::read_to_string(&mut stderr, &mut said);
                }
                let said = said
                    .trim()
                    .lines()
                    .last()
                    .unwrap_or("git failed")
                    .to_string();
                return Err(SkillsError::Fetch(format!("{url}: {said}")));
            }
            Ok(None) if started.elapsed() > CLONE_TIMEOUT => {
                let _ = child.kill();
                return Err(SkillsError::Fetch(format!(
                    "{url} took longer than {} seconds",
                    CLONE_TIMEOUT.as_secs()
                )));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => return Err(SkillsError::Fetch(error.to_string())),
        }
    }
}
