//! `patchbay teams`: subscribing to a team's git repository, checking it for changes and accepting them. Nothing a
//! team's repository says is kept until it's been shown, and accepting takes the commit that was shown, never a newer
//! one.

use std::io::Write;

use patchbay_core::Home;
use patchbay_core::skills::{Folders, Stamp};
use patchbay_core::subscription::{
    self, Asked, Pending, Subscriptions, TeamSkills, accept, at_commit, check, report, subscribe,
};

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
                    "--accept" => accept = Some(value("the commit shown")?),
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
        Some("check") => Ok(Teams::Check(
            args[1..].iter().map(|team| (*team).to_string()).collect(),
        )),
        Some("update") => {
            let team = args.get(1).ok_or("name the team to update")?.to_string();
            let accept = match args.get(2..) {
                Some(["--accept", commit]) => Some((*commit).to_string()),
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
    pub fn run(self, home: &Home, folders: &Folders, out: &mut impl Write) -> anyhow::Result<bool> {
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
                    writeln!(
                        out,
                        "{folder:<20} {}{branch} at {}, accepted {}",
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
            Teams::Check(teams) => {
                let teams = if teams.is_empty() {
                    Subscriptions::read(home)?.teams.into_keys().collect()
                } else {
                    teams
                };
                let mut changed = false;
                for team in teams {
                    match check(home, &team) {
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
                    Some(commit) => at_commit(home, &team, commit)?,
                    None => match check(home, &team)? {
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
    let wanted = patchbay_core::manifest::Manifest::from_toml(&pending.text)
        .map(|manifest| manifest.secrets.into_keys().collect::<Vec<_>>())
        .unwrap_or_default();
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
                "abc1234"
            ]),
            Ok(Teams::Add {
                url: "git@git.phin.tech:phin-tech/team.git".to_string(),
                branch: Some("main".to_string()),
                name: None,
                accept: Some("abc1234".to_string()),
            })
        );
        assert_eq!(
            parse(&["update", "phin-tech", "--accept", "abc1234"]),
            Ok(Teams::Update {
                team: "phin-tech".to_string(),
                accept: Some("abc1234".to_string()),
            })
        );
        assert_eq!(parse(&["check"]), Ok(Teams::Check(Vec::new())));
        assert!(parse(&["add"]).is_err());
        assert!(parse(&["add", "a", "b"]).is_err());
        assert!(parse(&["update"]).is_err());
        assert!(parse(&["sideways"]).is_err());
    }
}
