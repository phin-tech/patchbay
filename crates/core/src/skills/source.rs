//! Where skills come from, written the way the `skills` CLI (skills.sh) takes them: `owner/repo`, `owner/repo@skill`,
//! `owner/repo/path`, a GitHub or GitLab URL (with `/tree/<ref>/<path>`), `github:` and `gitlab:`, a `#ref@skill`
//! fragment, any other git URL, or a folder on this Mac. Read the same way, a skill installed by either tool is
//! recorded the same way in the lock they share.

use std::path::{Path, PathBuf};

/// Why a source can't be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourceError {
    #[error("Name a source: owner/repo, a git URL or a folder")]
    Empty,
    #[error("“{0}” goes outside the repository")]
    Outside(String),
}

/// What sort of source it is, as the lock's `sourceType` says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Github,
    Gitlab,
    Git,
    Local,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Github => "github",
            Kind::Gitlab => "gitlab",
            Kind::Git => "git",
            Kind::Local => "local",
        }
    }
}

/// A source, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub kind: Kind,
    /// What git clones, or the folder.
    pub url: String,
    /// A branch, tag or commit; the default branch if not given.
    pub reference: Option<String>,
    /// A folder inside the repository to look in.
    pub subpath: Option<String>,
    /// One skill, by name, as `owner/repo@skill` asks for.
    pub skill: Option<String>,
}

impl Source {
    /// Reads `input` as the `skills` CLI does. A relative folder is taken from `cwd`.
    pub fn parse(input: &str, cwd: &Path) -> Result<Source, SourceError> {
        let input = input.trim();
        if input.is_empty() {
            return Err(SourceError::Empty);
        }
        if is_local(input) {
            // `~/` is the home folder, as a shell has it; the CLI would read it as `owner/repo` on GitHub.
            let home = std::env::var_os("HOME").map(PathBuf::from);
            let path = match (input.strip_prefix("~/"), home) {
                (Some(rest), Some(home)) => home.join(rest),
                _ if Path::new(input).is_absolute() => PathBuf::from(input),
                _ => cwd.join(input),
            };
            return Ok(Source {
                kind: Kind::Local,
                url: path.display().to_string(),
                reference: None,
                subpath: None,
                skill: None,
            });
        }
        let (input, fragment_ref, fragment_skill) = split_fragment(input);
        if let Some(rest) = input.strip_prefix("github:") {
            return Source::parse(&with_fragment(rest, fragment_ref, fragment_skill), cwd);
        }
        if let Some(rest) = input.strip_prefix("gitlab:") {
            let url = format!("https://gitlab.com/{rest}");
            return Source::parse(&with_fragment(&url, fragment_ref, fragment_skill), cwd);
        }
        let mut source = parse_remote(input, fragment_ref.clone())?;
        if source.reference.is_none() {
            source.reference = fragment_ref;
        }
        if fragment_skill.is_some() {
            source.skill = fragment_skill;
        }
        Ok(source)
    }

    /// The source as the lock records it: `owner/repo` for GitHub, the URL for anything else.
    pub fn lock_source(&self) -> String {
        match self.kind {
            Kind::Local => self.url.clone(),
            Kind::Github if self.url.starts_with("https://github.com/") => {
                owner_repo(&self.url).unwrap_or_else(|| self.url.clone())
            }
            _ => self.url.clone(),
        }
    }
}

/// `owner/repo` from a clone URL.
fn owner_repo(url: &str) -> Option<String> {
    let path = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let path = path.split_once('/')?.1;
    let path = path.trim_end_matches('/').trim_end_matches(".git");
    path.contains('/').then(|| path.to_string())
}

fn is_local(input: &str) -> bool {
    input.starts_with('/')
        || input.starts_with("./")
        || input.starts_with("../")
        || input == "."
        || input == ".."
        || input.starts_with("~/")
}

/// The `#ref` or `#ref@skill` after a git source, and the source without it.
fn split_fragment(input: &str) -> (&str, Option<String>, Option<String>) {
    let Some((before, fragment)) = input.split_once('#') else {
        return (input, None, None);
    };
    if fragment.is_empty() {
        return (input, None, None);
    }
    let non_empty = |s: &str| (!s.is_empty()).then(|| s.to_string());
    match fragment.split_once('@') {
        Some((reference, skill)) => (before, non_empty(reference), non_empty(skill)),
        None => (before, Some(fragment.to_string()), None),
    }
}

fn with_fragment(input: &str, reference: Option<String>, skill: Option<String>) -> String {
    if reference.is_none() && skill.is_none() {
        return input.to_string();
    }
    format!(
        "{input}#{}{}",
        reference.unwrap_or_default(),
        skill.map(|skill| format!("@{skill}")).unwrap_or_default()
    )
}

/// A folder inside a repository, refused if it climbs out of it.
fn subpath(path: &str) -> Result<Option<String>, SourceError> {
    let path = path.trim_matches('/');
    if path.is_empty() {
        return Ok(None);
    }
    if path.split(['/', '\\']).any(|segment| segment == "..") {
        return Err(SourceError::Outside(path.to_string()));
    }
    Ok(Some(path.to_string()))
}

/// A version or a commit, which `owner/repo@…` pins rather than naming a skill.
fn is_version_pin(value: &str) -> bool {
    let commit = value.len() == 40 && value.chars().all(|c| c.is_ascii_hexdigit());
    let version = value.strip_prefix('v').unwrap_or(value);
    let mut parts = version.split(['-', '+']).next().unwrap_or("").split('.');
    let numbered = parts.clone().count() >= 2
        && parts.all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()));
    commit || numbered
}

fn parse_remote(input: &str, fragment_ref: Option<String>) -> Result<Source, SourceError> {
    let github = |url: String, reference: Option<String>, subpath: Option<String>| Source {
        kind: Kind::Github,
        url,
        reference,
        subpath,
        skill: None,
    };
    // A GitHub URL: the repository, or a folder in it at a ref.
    if let Some(path) = input
        .strip_prefix("https://github.com/")
        .or_else(|| input.strip_prefix("http://github.com/"))
    {
        let parts: Vec<&str> = path.trim_end_matches('/').split('/').collect();
        if let [owner, repo, rest @ ..] = parts.as_slice() {
            let repo = repo.trim_end_matches(".git");
            let url = format!("https://github.com/{owner}/{repo}.git");
            if let ["tree", reference, folder @ ..] = rest {
                return Ok(github(
                    url,
                    Some((*reference).to_string()),
                    subpath(&folder.join("/"))?,
                ));
            }
            return Ok(github(url, fragment_ref, None));
        }
    }
    // A GitLab URL, on gitlab.com or any host with `/-/tree/`.
    if let Some((repo, tree)) = input.split_once("/-/tree/")
        && (repo.starts_with("https://") || repo.starts_with("http://"))
    {
        let (reference, folder) = tree.split_once('/').unwrap_or((tree, ""));
        return Ok(Source {
            kind: Kind::Gitlab,
            url: format!("{}.git", repo.trim_end_matches(".git")),
            reference: Some(reference.to_string()),
            subpath: subpath(folder)?,
            skill: None,
        });
    }
    if let Some(path) = input.strip_prefix("https://gitlab.com/") {
        let path = path.trim_end_matches('/').trim_end_matches(".git");
        if path.contains('/') {
            return Ok(Source {
                kind: Kind::Gitlab,
                url: format!("https://gitlab.com/{path}.git"),
                reference: fragment_ref,
                subpath: None,
                skill: None,
            });
        }
    }
    // Shorthand: `owner/repo`, `owner/repo/folder` or `owner/repo@skill`, on GitHub.
    if !input.contains(':') && !input.starts_with('.') && !input.starts_with('/') {
        if let Some((repo, after)) = input.split_once('@')
            && let Some((owner, repo)) = repo.split_once('/')
            && !repo.contains('/')
            && !owner.is_empty()
            && !repo.is_empty()
            && !after.is_empty()
        {
            let url = format!("https://github.com/{owner}/{repo}.git");
            if fragment_ref.is_none() && is_version_pin(after) {
                return Ok(github(url, Some(after.to_string()), None));
            }
            return Ok(Source {
                skill: Some(after.to_string()),
                ..github(url, fragment_ref, None)
            });
        }
        let parts: Vec<&str> = input.trim_end_matches('/').splitn(3, '/').collect();
        if let [owner, repo, rest @ ..] = parts.as_slice()
            && !owner.is_empty()
            && !repo.is_empty()
        {
            let url = format!("https://github.com/{owner}/{repo}.git");
            return Ok(github(
                url,
                fragment_ref,
                subpath(rest.first().copied().unwrap_or(""))?,
            ));
        }
    }
    // Anything else is a git URL as given: another forge, SSH, or a `file://` repository.
    Ok(Source {
        kind: Kind::Git,
        url: input.to_string(),
        reference: fragment_ref,
        subpath: None,
        skill: None,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Kind, Source, SourceError};

    fn parse(input: &str) -> Source {
        Source::parse(input, Path::new("/work")).unwrap()
    }

    #[test]
    fn shorthand_is_a_github_repository() {
        let source = parse("vercel-labs/agent-skills");
        assert_eq!(source.kind, Kind::Github);
        assert_eq!(
            source.url,
            "https://github.com/vercel-labs/agent-skills.git"
        );
        assert_eq!(source.lock_source(), "vercel-labs/agent-skills");
        assert_eq!(parse("o/r/skills/x").subpath.as_deref(), Some("skills/x"));
    }

    #[test]
    fn an_at_names_one_skill_unless_it_is_a_version() {
        assert_eq!(parse("o/r@web-design").skill.as_deref(), Some("web-design"));
        let pinned = parse("o/r@v1.2.0");
        assert_eq!(pinned.skill, None);
        assert_eq!(pinned.reference.as_deref(), Some("v1.2.0"));
    }

    #[test]
    fn a_fragment_gives_the_ref_and_the_skill() {
        let source = parse("o/r#main@web-design");
        assert_eq!(source.reference.as_deref(), Some("main"));
        assert_eq!(source.skill.as_deref(), Some("web-design"));
        assert_eq!(parse("github:o/r#dev").reference.as_deref(), Some("dev"));
    }

    #[test]
    fn a_tree_url_gives_the_ref_and_folder() {
        let source = parse("https://github.com/o/r/tree/main/skills/x");
        assert_eq!(source.url, "https://github.com/o/r.git");
        assert_eq!(source.reference.as_deref(), Some("main"));
        assert_eq!(source.subpath.as_deref(), Some("skills/x"));
        let gitlab = parse("https://gitlab.com/g/sub/r/-/tree/dev/skills");
        assert_eq!(gitlab.kind, Kind::Gitlab);
        assert_eq!(gitlab.url, "https://gitlab.com/g/sub/r.git");
        assert_eq!(gitlab.subpath.as_deref(), Some("skills"));
    }

    #[test]
    fn any_other_url_is_cloned_as_given_and_locked_by_it() {
        let source = parse("https://git.phin.tech/phin-tech/skills.git");
        assert_eq!(source.kind, Kind::Git);
        assert_eq!(
            source.lock_source(),
            "https://git.phin.tech/phin-tech/skills.git"
        );
        assert_eq!(parse("git@github.com:o/r.git").kind, Kind::Git);
    }

    #[test]
    fn a_folder_is_read_where_it_is() {
        let source = parse("./mine");
        assert_eq!(source.kind, Kind::Local);
        assert_eq!(source.url, "/work/./mine");
        // CI's steps may run without a HOME.
        if let Ok(home) = std::env::var("HOME") {
            let tilde = parse("~/src/mine");
            assert_eq!(tilde.kind, Kind::Local);
            assert_eq!(tilde.url, format!("{home}/src/mine"));
        }
    }

    #[test]
    fn a_folder_climbing_out_of_the_repository_is_refused() {
        assert_eq!(
            Source::parse("o/r/../../etc", Path::new("/")),
            Err(SourceError::Outside("../../etc".into()))
        );
    }
}
