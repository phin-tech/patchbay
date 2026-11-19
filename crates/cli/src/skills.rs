//! `patchbay skills`, which takes what `npx skills` takes for skills installed for every project, and manages the
//! same skills in the same folders: `add`, `list`, `remove`, `check` and `update`.

use std::io::{self, Write};
use std::path::Path;

use patchbay_core::skills::{
    self, Change, Checking, Folders, Keeper, Preview, Reach, Request, Source,
};

/// What `patchbay skills` was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skills {
    Add(Request),
    List,
    Remove(Vec<String>),
    Check,
    Update(Vec<String>),
}

pub const USAGE: &str = "  skills add <source> [--skill <name>…|'*'] [--agent <client>…|'*'] [--list]
                            install skills as `npx skills add -g` does: owner/repo, owner/repo@skill,
                            a git URL or a folder, into ~/.agents/skills and each client's folder
  skills list               every skill, who keeps it and which clients have it
  skills remove <name>…     take a skill out of every client and the store
  skills check              which skills their sources have changed
  skills update [name…]     list those changes, then apply them";

/// Reads what follows `skills`.
pub fn parse(args: &[&str]) -> Result<Skills, String> {
    let rest = || {
        args[1..]
            .iter()
            .map(|name| (*name).to_string())
            .collect::<Vec<_>>()
    };
    match args.first().copied() {
        Some("add" | "a" | "install" | "i") => Request::parse_args(&args[1..])
            .map(Skills::Add)
            .map_err(|error| error.to_string()),
        Some("list" | "ls") => Ok(Skills::List),
        Some("remove" | "rm" | "r") if args.len() > 1 => Ok(Skills::Remove(rest())),
        Some("remove" | "rm" | "r") => Err("name the skills to remove".to_string()),
        Some("check") => Ok(Skills::Check),
        Some("update" | "upgrade") => Ok(Skills::Update(rest())),
        Some(other) => Err(format!("skills doesn't do {other}")),
        None => Err("say what to do with skills: add, list, remove, check or update".to_string()),
    }
}

impl Skills {
    /// Runs it over `folders`, reading a relative folder source from `cwd`.
    pub fn run(self, folders: &Folders, cwd: &Path, out: &mut impl Write) -> anyhow::Result<()> {
        match self {
            Skills::Add(request) => add(request, folders, cwd, out)?,
            Skills::List => list(folders, out)?,
            Skills::Remove(names) => {
                for name in names {
                    let removed = skills::remove(folders, &name)?;
                    writeln!(out, "Removed {removed}.")?;
                }
            }
            Skills::Check => {
                let checked = skills::check(folders, &[], Checking::Report, &skills::Stamp::now())?;
                for update in &checked.updates {
                    writeln!(
                        out,
                        "{} ({}): {}",
                        update.name,
                        update.source,
                        describe(&update.changes)
                    )?;
                }
                for (name, why) in &checked.unchecked {
                    writeln!(out, "{name}: not checked, {why}")?;
                }
                if checked.updates.is_empty() {
                    writeln!(out, "Every skill is up to date.")?;
                } else {
                    writeln!(out, "`patchbay skills update` applies them.")?;
                }
            }
            Skills::Update(names) => {
                // What changes is listed before it's put in place, and only what was listed is.
                let listed =
                    skills::check(folders, &names, Checking::Report, &skills::Stamp::now())?;
                for update in &listed.updates {
                    writeln!(
                        out,
                        "{} ({}): {}",
                        update.name,
                        update.source,
                        describe(&update.changes)
                    )?;
                }
                let mut unchecked = listed.unchecked;
                if listed.updates.is_empty() {
                    writeln!(out, "Every skill is up to date.")?;
                } else {
                    let applied = skills::check(
                        folders,
                        &names,
                        Checking::Apply(listed.updates),
                        &skills::Stamp::now(),
                    )?;
                    for update in &applied.updates {
                        writeln!(out, "Updated {}.", update.name)?;
                    }
                    unchecked.extend(applied.unchecked);
                }
                for (name, why) in &unchecked {
                    writeln!(out, "{name}: not checked, {why}")?;
                }
            }
        }
        Ok(())
    }
}

fn add(
    request: Request,
    folders: &Folders,
    cwd: &Path,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    let source = Source::parse(&request.source, cwd)?;
    writeln!(out, "Fetching {}…", source.lock_source())?;
    out.flush()?;
    let preview = Preview::fetch(source)?;
    let named = !request.skills.is_empty() || preview.source.skill.is_some();
    if request.list || !named && preview.found.len() > 1 {
        writeln!(
            out,
            "{} has {} skills:",
            preview.source.lock_source(),
            preview.found.len()
        )?;
        for found in &preview.found {
            writeln!(out, "  {:<24} {}", found.name, found.description)?;
        }
        if !request.list {
            writeln!(
                out,
                "Choose with --skill <name>, or --skill '*' for every one."
            )?;
        }
        return Ok(());
    }
    let chosen = if named {
        preview.choose(&request.skills)?
    } else {
        preview.found.iter().collect()
    };
    let clients = request.clients_for(folders);
    let installed = skills::install(folders, &preview, &chosen, &clients, &skills::Stamp::now())?;
    for skill in installed {
        let names: Vec<&str> = skill.linked.iter().map(|client| client.name()).collect();
        writeln!(
            out,
            "Installed {} in {}{}.",
            skill.name,
            skill.folder.display(),
            if names.is_empty() {
                String::new()
            } else {
                format!(", for {}", names.join(", "))
            }
        )?;
        for client in skill.blocked {
            writeln!(
                out,
                "  {} already has a {} of its own, left as it is.",
                client.name(),
                skill.name
            )?;
        }
    }
    Ok(())
}

fn list(folders: &Folders, out: &mut impl Write) -> io::Result<()> {
    let skills = skills::list(folders);
    if skills.is_empty() {
        return writeln!(
            out,
            "No skills yet: `patchbay skills add <owner/repo>` adds some."
        );
    }
    for skill in skills {
        let keeper = match &skill.keeper {
            Keeper::Locked(entry) => entry.source.clone(),
            Keeper::Unmanaged => "(your own)".to_string(),
        };
        let clients: Vec<String> = skill
            .clients
            .iter()
            .map(|(client, reach)| match reach {
                Reach::Linked => client.slug().to_string(),
                Reach::Own => format!("{} (own)", client.slug()),
            })
            .collect();
        writeln!(
            out,
            "  {:<28} {:<32} {}",
            skill.name,
            keeper,
            clients.join(", ")
        )?;
    }
    Ok(())
}

/// A skill's changes in a few words: `SKILL.md changed, checklist.md added`.
fn describe(changes: &[Change]) -> String {
    if changes.is_empty() {
        return "its files are the same, only the record changed".to_string();
    }
    changes
        .iter()
        .map(|change| match change {
            Change::Added(path) => format!("{path} added"),
            Change::Removed(path) => format!("{path} removed"),
            Change::Changed(path) => format!("{path} changed"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use patchbay_core::skills::Folders;

    use super::{Skills, parse};

    fn folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("patchbay-cli-skills-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    /// A git repository with two skills, as a `file://` URL.
    fn repo() -> String {
        let repo = folder("repo");
        for name in ["review", "plan"] {
            fs::create_dir_all(repo.join("skills").join(name)).unwrap();
            fs::write(
                repo.join("skills").join(name).join("SKILL.md"),
                format!("---\nname: {name}\ndescription: Helps with {name}\n---\n"),
            )
            .unwrap();
        }
        for args in [
            &["init", "-q"][..],
            &["add", "-A"],
            &["commit", "-q", "-m", "skills"],
        ] {
            let status = Command::new("git")
                .current_dir(&repo)
                .args(["-c", "user.name=t", "-c", "user.email=t@t"])
                .args(args)
                .status()
                .unwrap();
            assert!(status.success());
        }
        format!("file://{}", repo.display())
    }

    fn run(args: &[&str], folders: &Folders, url: &str) -> String {
        let mut out = Vec::new();
        parse(args)
            .unwrap()
            .run(folders, Path::new("/"), &mut out)
            .unwrap();
        String::from_utf8(out)
            .unwrap()
            .replace(url, "<repo>")
            .replace(&folders.home().display().to_string(), "~")
    }

    #[test]
    fn skills_add_lists_a_sources_skills_then_installs_the_ones_named() {
        let home = folder("home");
        fs::create_dir_all(home.join(".cursor")).unwrap();
        let folders = Folders::new(&home);
        let url = repo();
        let mut said = run(&["add", &url], &folders, &url);
        said += &run(
            &[
                "add",
                &url,
                "--skill",
                "review",
                "plan",
                "-a",
                "claude-code",
                "cursor",
            ],
            &folders,
            &url,
        );
        said += &run(&["list"], &folders, &url);
        said += &run(&["check"], &folders, &url);
        said += &run(&["remove", "plan"], &folders, &url);
        insta::assert_snapshot!(said);
    }

    #[test]
    fn skills_commands_read_their_arguments() {
        assert_eq!(parse(&["ls"]), Ok(Skills::List));
        assert_eq!(parse(&["update"]), Ok(Skills::Update(vec![])));
        assert_eq!(
            parse(&["rm", "a", "b"]),
            Ok(Skills::Remove(vec!["a".into(), "b".into()]))
        );
        assert!(parse(&["remove"]).is_err());
        assert!(parse(&["fly"]).is_err());
        let Ok(Skills::Add(request)) = parse(&["add", "o/r@x", "-g", "-y"]) else {
            panic!("add reads its source");
        };
        assert_eq!(request.source, "o/r@x");
    }
}
