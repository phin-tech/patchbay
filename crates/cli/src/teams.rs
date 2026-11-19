//! `patchbay teams`: subscribing to a team's git repository, checking it for changes and accepting them. Nothing a
//! team's repository says is kept until it's been shown, and accepting takes the commit that was shown, never a newer
//! one. A LiteLLM proxy's MCP servers are kept the same way, by the version of what it lists.

use std::io::{BufRead, Write};

use patchbay_core::Home;
use patchbay_core::secrets::Secrets;
use patchbay_core::skills::{Folders, Stamp};
use patchbay_core::subscription::{
    self, Asked, Origin, Pending, Subscriptions, TeamSkills, accept, at_commit, check, litellm,
    report, subscribe, subscribe_litellm,
};

/// Where `teams litellm` reads the key from before it asks, so a script can give it.
const KEY_VARIABLE: &str = "PATCHBAY_LITELLM_KEY";

/// What `patchbay teams` was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Teams {
    List,
    Add {
        url: String,
        branch: Option<String>,
        name: Option<String>,
        accept: Option<String>,
    },
    LiteLlm {
        url: String,
        name: Option<String>,
        accept: Option<String>,
    },
    Check(Vec<String>),
    Update {
        team: String,
        accept: Option<String>,
    },
}

pub const USAGE: &str = "  teams                     the teams you subscribe to, and the commit each was last accepted at
  teams add <git-url> [--branch <name>] [--name <team>] [--accept <commit>]
                            subscribe to a team's repository (a patchbay.toml at its top): shows what
                            it adds, and keeps it with --accept and the commit shown
  teams litellm <url> [--name <team>] [--accept <version>]
                            keep a LiteLLM proxy's MCP servers as a team, with your key (asked for, or
                            in PATCHBAY_LITELLM_KEY): shows them, and keeps them with --accept
  teams check [team…]       which teams' repositories have changed since you accepted them
  teams update <team> [--accept <commit>]
                            show what a team changed; --accept takes in that commit and its skills";

/// Reads what follows `teams`.
pub fn parse(args: &[&str]) -> Result<Teams, String> {
    match args.first().copied() {
        None | Some("list" | "ls") => Ok(Teams::List),
        Some("add" | "subscribe") => {
            let mut rest = args[1..].iter();
            let (mut url, mut branch, mut name, mut accept) = (None, None, None, None);
            while let Some(arg) = rest.next() {
                let mut value = |what: &str| {
                    rest.next()
                        .map(|value| (*value).to_string())
                        .ok_or_else(|| format!("{arg} needs {what}"))
                };
                match *arg {
                    "--branch" | "-b" => branch = Some(value("a branch")?),
                    "--name" | "-n" => name = Some(value("a name")?),
                    "--accept" => accept = Some(whole_commit(&value("the commit shown")?)?),
                    flag if flag.starts_with('-') => {
                        return Err(format!("teams add doesn't take {flag}"));
                    }
                    given if url.is_none() => url = Some(given.to_string()),
                    extra => return Err(format!("one repository at a time, not {extra} too")),
                }
            }
            let url = url.ok_or("give the team's git repository")?;
            Ok(Teams::Add {
                url,
                branch,
                name,
                accept,
            })
        }
        Some("litellm") => {
            let mut rest = args[1..].iter();
            let (mut url, mut name, mut accept) = (None, None, None);
            while let Some(arg) = rest.next() {
                let mut value = |what: &str| {
                    rest.next()
                        .map(|value| (*value).to_string())
                        .ok_or_else(|| format!("{arg} needs {what}"))
                };
                match *arg {
                    "--name" | "-n" => name = Some(value("a name")?),
                    "--accept" => accept = Some(whole_commit(&value("the version shown")?)?),
                    flag if flag.starts_with('-') => {
                        return Err(format!("teams litellm doesn't take {flag}"));
                    }
                    given if url.is_none() => url = Some(given.to_string()),
                    extra => return Err(format!("one LiteLLM at a time, not {extra} too")),
                }
            }
            let url = url.ok_or("give the LiteLLM proxy's URL")?;
            Ok(Teams::LiteLlm { url, name, accept })
        }
        Some("check") => Ok(Teams::Check(
            args[1..].iter().map(|team| (*team).to_string()).collect(),
        )),
        Some("update") => {
            let team = args.get(1).ok_or("name the team to update")?.to_string();
            let accept = match args.get(2..) {
                Some(["--accept", commit]) => Some(whole_commit(commit)?),
                Some([]) | None => None,
                Some(_) => {
                    return Err("teams update takes a team and --accept <commit>".to_string());
                }
            };
            Ok(Teams::Update { team, accept })
        }
        Some(other) => Err(format!("teams doesn't do {other}")),
    }
}

impl Teams {
    /// Runs it over `home`, installing a team's skills into `folders`. Gives whether any team's layer changed, for
    /// the caller to have the gateway read them again.
    pub fn run(
        self,
        home: &Home,
        folders: &Folders,
        secrets: &dyn Secrets,
        out: &mut impl Write,
    ) -> anyhow::Result<bool> {
        match self {
            Teams::List => {
                let kept = Subscriptions::read(home)?;
                if kept.teams.is_empty() {
                    writeln!(
                        out,
                        "You don't subscribe to any team. `patchbay teams add <git-url>` does."
                    )?;
                }
                for (folder, team) in &kept.teams {
                    let branch = team
                        .branch
                        .as_deref()
                        .map(|branch| format!(" ({branch})"))
                        .unwrap_or_default();
                    let from = match team.kind {
                        Origin::Git => "",
                        Origin::LiteLlm => "LiteLLM ",
                    };
                    writeln!(
                        out,
                        "{folder:<20} {from}{}{branch} at {}, accepted {}",
                        team.url,
                        team.commit.get(..7).unwrap_or(&team.commit),
                        team.accepted
                    )?;
                }
                Ok(false)
            }
            Teams::Add {
                url,
                branch,
                name,
                accept,
            } => {
                writeln!(out, "Fetching {url}…")?;
                out.flush()?;
                let asked = Asked {
                    branch: branch.as_deref(),
                    name: name.as_deref(),
                    at: accept.as_deref(),
                };
                let pending = subscribe(home, &url, asked)?;
                write!(out, "{}", report(&pending))?;
                if accept.is_none() {
                    let mut again = format!("patchbay teams add {url}");
                    if let Some(branch) = &branch {
                        again.push_str(&format!(" --branch {branch}"));
                    }
                    if let Some(name) = &name {
                        again.push_str(&format!(" --name '{name}'"));
                    }
                    writeln!(
                        out,
                        "A team's servers run on your Mac. To keep these, run:\n  {again} --accept {}",
                        pending.commit
                    )?;
                    return Ok(false);
                }
                take(home, folders, &pending, out)?;
                Ok(true)
            }
            Teams::LiteLlm { url, name, accept } => {
                let key = match std::env::var(KEY_VARIABLE) {
                    Ok(key) if !key.trim().is_empty() => key,
                    _ => {
                        writeln!(out, "Paste your LiteLLM key, then press Return:")?;
                        out.flush()?;
                        let mut line = String::new();
                        std::io::stdin().lock().read_line(&mut line)?;
                        line
                    }
                };
                let key = key.trim().to_string();
                let url = litellm::base_url(&url)?;
                writeln!(out, "Asking {url} for its MCP servers…")?;
                out.flush()?;
                let asked = Asked {
                    name: name.as_deref(),
                    at: accept.as_deref(),
                    ..Asked::default()
                };
                let pending = subscribe_litellm(home, &url, &key, asked)?;
                write!(out, "{}", report(&pending))?;
                if accept.is_none() {
                    let mut again = format!("patchbay teams litellm {}", pending.url);
                    if let Some(name) = &name {
                        again.push_str(&format!(" --name '{name}'"));
                    }
                    writeln!(
                        out,
                        "These run where LiteLLM runs. To keep them, run:\n  {again} --accept {}",
                        pending.commit
                    )?;
                    return Ok(false);
                }
                // Kept before the team, whose servers look for it as soon as the gateway reads them.
                secrets.set(&litellm::account(&pending.folder), &key)?;
                take(home, folders, &pending, out)?;
                Ok(true)
            }
            Teams::Check(teams) => {
                let teams = if teams.is_empty() {
                    Subscriptions::read(home)?.teams.into_keys().collect()
                } else {
                    teams
                };
                let mut changed = false;
                for team in teams {
                    match check(home, secrets, &team) {
                        Ok(None) => writeln!(out, "{team}: up to date")?,
                        Ok(Some(pending)) => {
                            changed = true;
                            writeln!(
                                out,
                                "{team}: {} to look at, `patchbay teams update {team}` shows them",
                                count(pending.changes.len())
                            )?;
                        }
                        Err(error) => writeln!(out, "{team}: not checked, {error}")?,
                    }
                }
                // A check may take in a commit that only changed comments, which the gateway reads too.
                Ok(changed)
            }
            Teams::Update { team, accept } => {
                let pending = match &accept {
                    Some(commit) => at_commit(home, secrets, &team, commit)?,
                    None => match check(home, secrets, &team)? {
                        Some(pending) => pending,
                        None => {
                            writeln!(out, "{team} is up to date.")?;
                            return Ok(false);
                        }
                    },
                };
                write!(out, "{}", report(&pending))?;
                if pending.changes.is_empty() {
                    writeln!(out, "Nothing in its manifest changed.")?;
                }
                if accept.is_none() {
                    writeln!(
                        out,
                        "To take these in, run:\n  patchbay teams update {team} --accept {}",
                        pending.commit
                    )?;
                    return Ok(false);
                }
                take(home, folders, &pending, out)?;
                Ok(true)
            }
        }
    }
}

/// Accepts `pending`, installs the skills it adds, and says what happened.
fn take(
    home: &Home,
    folders: &Folders,
    pending: &Pending,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    let accepted = accept(home, pending)?;
    writeln!(
        out,
        "Kept {} at {}, as “{}”.",
        pending.name,
        pending.short_commit(),
        accepted.folder
    )?;
    if !accepted.dropped.is_empty() {
        writeln!(
            out,
            "Your switches for {} went with them.",
            accepted.dropped.join(", ")
        )?;
    }
    let TeamSkills {
        installed,
        already,
        own,
        failed,
    } = subscription::install_skills(folders, pending, &Stamp::now());
    for name in installed {
        writeln!(out, "Installed the skill {name}.")?;
    }
    for name in already {
        writeln!(
            out,
            "{name} is installed already; `patchbay skills check` finds its updates."
        )?;
    }
    for name in own {
        writeln!(out, "{name} is a skill of your own, so it's left as it is.")?;
    }
    for (name, why) in failed {
        writeln!(out, "Couldn't install {name}: {why}")?;
    }
    // A LiteLLM's one secret is its key, kept already.
    let wanted = patchbay_core::manifest::Manifest::from_toml(&pending.text)
        .map(|manifest| manifest.secrets.into_keys().collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .filter(|_| pending.origin == Origin::Git)
        .collect::<Vec<_>>();
    if !wanted.is_empty() {
        writeln!(
            out,
            "It names secrets you keep yourself; Patchbay asks for each when its server starts, or\n  patchbay secret {}/<name>  keeps one now ({}).",
            accepted.folder,
            wanted.join(", ")
        )?;
    }
    Ok(())
}

/// `commit`, if it's the whole commit a report gave, or the whole version of a LiteLLM's servers: anything shorter,
/// or a branch, could be another commit.
fn whole_commit(commit: &str) -> Result<String, String> {
    let hex = commit.chars().all(|c| c.is_ascii_hexdigit());
    if hex && (commit.len() == 40 || litellm::is_version(commit)) {
        Ok(commit.to_string())
    } else {
        Err(format!(
            "--accept takes the whole commit or version shown, not {commit}"
        ))
    }
}

fn count(changes: usize) -> String {
    if changes == 1 {
        "1 change".to_string()
    } else {
        format!("{changes} changes")
    }
}

#[cfg(test)]
mod tests {
    use super::{Teams, parse};

    #[test]
    fn teams_reads_a_repository_and_how_to_keep_it() {
        assert_eq!(parse(&[]), Ok(Teams::List));
        assert_eq!(
            parse(&[
                "add",
                "git@git.phin.tech:phin-tech/team.git",
                "--branch",
                "main",
                "--accept",
                "0123456789abcdef0123456789abcdef01234567"
            ]),
            Ok(Teams::Add {
                url: "git@git.phin.tech:phin-tech/team.git".to_string(),
                branch: Some("main".to_string()),
                name: None,
                accept: Some("0123456789abcdef0123456789abcdef01234567".to_string()),
            })
        );
        assert_eq!(
            parse(&[
                "update",
                "phin-tech",
                "--accept",
                "0123456789abcdef0123456789abcdef01234567"
            ]),
            Ok(Teams::Update {
                team: "phin-tech".to_string(),
                accept: Some("0123456789abcdef0123456789abcdef01234567".to_string()),
            })
        );
        assert!(
            parse(&["update", "phin-tech", "--accept", "main"]).is_err(),
            "a branch could be anywhere by the time it's fetched"
        );
        assert!(parse(&["add", "x/y", "--accept", "abc1234"]).is_err());
        assert_eq!(parse(&["check"]), Ok(Teams::Check(Vec::new())));
        let version = "ab".repeat(32);
        assert_eq!(
            parse(&[
                "litellm",
                "https://ai.example.com",
                "--name",
                "AI",
                "--accept",
                &version
            ]),
            Ok(Teams::LiteLlm {
                url: "https://ai.example.com".to_string(),
                name: Some("AI".to_string()),
                accept: Some(version.clone()),
            })
        );
        assert!(parse(&["litellm"]).is_err());
        assert!(parse(&["litellm", "https://a", "--accept", "abc"]).is_err());
        assert!(parse(&["litellm", "https://a", "--branch", "main"]).is_err());
        assert!(parse(&["add"]).is_err());
        assert!(parse(&["add", "a", "b"]).is_err());
        assert!(parse(&["update"]).is_err());
        assert!(parse(&["sideways"]).is_err());
    }
}
