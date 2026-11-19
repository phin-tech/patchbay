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

/// The commit the branch or tag `reference` (or the default branch) of the repository at `url` is at, asked of the
/// remote without fetching anything.
pub fn remote_head(url: &str, reference: Option<&str>) -> Result<String, SkillsError> {
    let wanted = reference.unwrap_or("HEAD");
    let said = output(
        git(Path::new(".")).args(["ls-remote", "--", url, wanted]),
        url,
    )?;
    // A tag peeled to its commit (`refs/tags/v1^{}`) is the one to compare, so it wins over the tag itself.
    let lines: Vec<(&str, &str)> = said
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .collect();
    lines
        .iter()
        .find(|(_, name)| name.ends_with("^{}"))
        .or_else(|| lines.first())
        .map(|(commit, _)| (*commit).to_string())
        .ok_or_else(|| SkillsError::Fetch(format!("{url} has no {wanted}")))
}

impl Fetched {
    /// The commit a clone is at.
    pub fn commit(&self) -> Option<String> {
        if !self.temporary {
            return None;
        }
        let output = git(&self.root).args(["rev-parse", "HEAD"]).output().ok()?;
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

/// git in `folder`, asking for nothing. With the GitHub CLI on this Mac, git asks it for a sign-in after any helper of
/// its own, so a private repository on GitHub is reached with the account `gh auth login` signed in to.
fn git(folder: &Path) -> Command {
    let mut git = Command::new("git");
    if let Some(gh) = gh() {
        // `-c` adds to the helpers git's config names rather than replacing them, so a saved sign-in still comes first.
        git.arg("-c").arg(credential_helper(gh));
    }
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

/// The GitHub CLI, found once: on the `PATH`, or where Homebrew puts it, since an app opened from the Finder has only
/// the system's `PATH`.
fn gh() -> Option<&'static Path> {
    static GH: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    GH.get_or_init(|| {
        let path = std::env::var_os("PATH").unwrap_or_default();
        std::env::split_paths(&path)
            .chain(["/opt/homebrew/bin".into(), "/usr/local/bin".into()])
            .map(|folder| folder.join("gh"))
            .find(|gh| gh.is_file())
    })
    .as_deref()
}

/// The setting that has git ask `gh` for a sign-in, as `gh auth setup-git` writes it.
fn credential_helper(gh: &Path) -> String {
    let quoted = gh.display().to_string().replace('\'', "'\\''");
    format!("credential.helper=!'{quoted}' auth git-credential")
}

/// What a git command writes, once it's run to its end.
fn output(command: &mut Command, url: &str) -> Result<String, SkillsError> {
    let mut written = std::env::temp_dir().join(format!(
        "patchbay-git-{}-{}",
        std::process::id(),
        OUTPUTS.fetch_add(1, Ordering::Relaxed)
    ));
    written.set_extension("out");
    // Into a file rather than a pipe, so a long answer can't fill the pipe while `run` waits for git to end.
    let file = std::fs::File::create(&written)
        .map_err(|error| SkillsError::Fetch(format!("couldn't run git: {error}")))?;
    let ran = run_with(command.stdout(file), url);
    let said = std::fs::read_to_string(&written).unwrap_or_default();
    let _ = std::fs::remove_file(&written);
    ran.map(|()| said)
}

static OUTPUTS: AtomicUsize = AtomicUsize::new(0);

/// What git says when a repository wants a sign-in it wasn't given, or doesn't let this one in, in any case.
pub(crate) const DENIED: [&str; 6] = [
    "authentication",
    "could not read username",
    "permission denied",
    "terminal prompts disabled",
    "repository not found",
    "access denied",
];

/// The line of git's complaint worth showing: the one saying it was refused, if it says so, since an SSH refusal
/// ends with advice ("and the repository exists."); otherwise its last `fatal:`, or its last line.
pub(crate) fn telling_line(said: &str) -> &str {
    let lines: Vec<&str> = said
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    lines
        .iter()
        .find(|line| {
            let lower = line.to_lowercase();
            DENIED.iter().any(|word| lower.contains(word))
        })
        .or_else(|| lines.iter().rev().find(|line| line.starts_with("fatal:")))
        .or_else(|| lines.last())
        .copied()
        .unwrap_or("git failed")
}

/// Runs a git command to its end, or stops it after [`CLONE_TIMEOUT`].
fn run(command: &mut Command, url: &str) -> Result<(), SkillsError> {
    run_with(command.stdout(Stdio::null()), url)
}

/// Runs a git command whose output is already set, to its end, or stops it after [`CLONE_TIMEOUT`].
fn run_with(command: &mut Command, url: &str) -> Result<(), SkillsError> {
    let mut child = command
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
                return Err(SkillsError::Fetch(format!(
                    "{url}: {}",
                    telling_line(&said)
                )));
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

#[cfg(test)]
mod tests {
    use super::{credential_helper, telling_line};

    #[test]
    fn git_asks_the_github_cli_for_a_sign_in_wherever_it_is() {
        assert_eq!(
            credential_helper(std::path::Path::new("/opt/homebrew/bin/gh")),
            "credential.helper=!'/opt/homebrew/bin/gh' auth git-credential"
        );
        assert_eq!(
            credential_helper(std::path::Path::new("/Users/o'neill/bin/gh")),
            "credential.helper=!'/Users/o'\\''neill/bin/gh' auth git-credential"
        );
    }

    #[test]
    fn a_refused_ssh_clone_says_it_was_refused_not_the_advice_after() {
        let said = "git@github.com: Permission denied (publickey).\r\nfatal: Could not read from remote repository.\n\nPlease make sure you have the correct access rights\nand the repository exists.\n";
        assert_eq!(
            telling_line(said),
            "git@github.com: Permission denied (publickey)."
        );
        assert_eq!(
            telling_line("Cloning into 'x'...\nfatal: repository 'x' does not exist\n"),
            "fatal: repository 'x' does not exist"
        );
        assert_eq!(telling_line(""), "git failed");
    }
}
