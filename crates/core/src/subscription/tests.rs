use std::path::{Path, PathBuf};
use std::process::Command;

use super::{
    Asked, How, Part, Subscriptions, TeamSkills, accept, at_commit, check, install_skills, report,
    subscribe,
};
use crate::Home;
use crate::manifest::{Scope, resolve};
use crate::share::{self, TeamError};
use crate::skills::{Folders, Lock, Stamp};

const TEAM: &str = r#"[team]
name = "Phin Tech"

[secrets.github-token]
prompt = "A GitHub token with repo and read:org"

[servers.github]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github@2025.4.8"]
env = { GITHUB_PERSONAL_ACCESS_TOKEN = "secret:github-token" }

[servers.sentry]
url = "https://mcp.sentry.dev/mcp"
auth = "oauth"
"#;

/// A scratch folder for one test, empty.
fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "patchbay-subscription-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(repo)
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args([
            "-c",
            "init.defaultBranch=main",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// A team's repository at `root/team`, its manifest `text`, and the URL to subscribe with.
fn team_repo(root: &Path, text: &str) -> (PathBuf, String) {
    let repo = root.join("team");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    commit(&repo, text, "The team's servers");
    let url = format!("file://{}", repo.display());
    (repo, url)
}

/// Commits `text` as the repository's manifest, giving the commit.
fn commit(repo: &Path, text: &str, message: &str) -> String {
    std::fs::write(repo.join("patchbay.toml"), text).unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-q", "-m", message]);
    git(repo, &["rev-parse", "HEAD"])
}

/// `report`, with what changes from run to run put as words.
fn steady(text: &str, url: &str, commits: &[&str]) -> String {
    let mut text = text.replace(url, "<url>");
    for (at, commit) in commits.iter().enumerate() {
        text = text.replace(&commit[..7], &format!("<commit {at}>"));
    }
    text
}

#[test]
fn subscribing_shows_everything_the_team_adds_and_keeps_nothing_until_accepted() {
    let root = scratch("subscribe");
    let (repo, url) = team_repo(&root, TEAM);
    let home = Home::new(root.join("home"));
    let pending = subscribe(&home, &url, Asked::default()).unwrap();
    assert_eq!(pending.folder, "phin-tech");
    assert_eq!(pending.commit, git(&repo, &["rev-parse", "HEAD"]));
    assert!(
        !home.teams().join("phin-tech").exists(),
        "nothing is kept until it's accepted"
    );
    assert!(!home.subscriptions().exists());
    insta::assert_snapshot!(steady(&report(&pending), &url, &[&pending.commit]), @r#"
    Phin Tech (<url>) at <commit 0>, kept as “phin-tech”:
    + secret github-token
        prompt = "A GitHub token with repo and read:org"
    + server github
        args = ["-y", "@modelcontextprotocol/server-github@2025.4.8"]
        command = "npx"
        env.GITHUB_PERSONAL_ACCESS_TOKEN = "secret:github-token"
    + server sentry
        auth = "oauth"
        url = "https://mcp.sentry.dev/mcp"
    "#);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_accepted_team_is_a_layer_and_its_commit_is_in_the_lock() {
    let root = scratch("accept");
    let (_, url) = team_repo(&root, TEAM);
    let home = Home::new(root.join("home"));
    let pending = subscribe(&home, &url, Asked::default()).unwrap();
    let accepted = accept(&home, &pending).unwrap();
    assert!(accepted.dropped.is_empty());
    let (layers, errors) = home.layers();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(layers[0].scope, Scope::Team("phin-tech".to_string()));
    let lock = Subscriptions::read(&home).unwrap();
    assert_eq!(lock.teams["phin-tech"].url, url);
    assert_eq!(lock.teams["phin-tech"].commit, pending.commit);
    assert!(
        matches!(
            subscribe(&home, &url, Asked { name: Some("Again"), ..Asked::default() }),
            Err(TeamError::Subscribed(folder)) if folder == "phin-tech"
        ),
        "one repository is subscribed to once"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_team_that_hasnt_moved_has_nothing_to_look_at() {
    let root = scratch("unmoved");
    let (_, url) = team_repo(&root, TEAM);
    let home = Home::new(root.join("home"));
    accept(&home, &subscribe(&home, &url, Asked::default()).unwrap()).unwrap();
    assert_eq!(check(&home, "phin-tech").unwrap(), None);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_teams_changes_wait_to_be_accepted_and_take_your_switches_for_what_it_dropped() {
    let root = scratch("update");
    let (repo, url) = team_repo(&root, TEAM);
    let home = Home::new(root.join("home"));
    let first = subscribe(&home, &url, Asked::default()).unwrap();
    accept(&home, &first).unwrap();
    std::fs::write(home.manifest(), "[servers.sentry]\nenabled = false\n").unwrap();

    let changed = TEAM
        .replace("server-github@2025.4.8", "server-github@2025.6.1")
        .replace(
            "[servers.sentry]\nurl = \"https://mcp.sentry.dev/mcp\"\nauth = \"oauth\"\n",
            "[servers.linear]\nurl = \"https://mcp.linear.app/mcp\"\nauth = \"oauth\"\nenabled = false\n",
        );
    let second = commit(&repo, &changed, "Linear in, Sentry out");
    let before = std::fs::read_to_string(share::manifest_path(&home, "phin-tech")).unwrap();

    let pending = check(&home, "phin-tech").unwrap().unwrap();
    assert_eq!(pending.commit, second);
    assert_eq!(
        std::fs::read_to_string(share::manifest_path(&home, "phin-tech")).unwrap(),
        before,
        "the layer the gateway reads is the one accepted until this one is"
    );
    insta::assert_snapshot!(steady(&report(&pending), &url, &[&first.commit, &second]), @r#"
    Phin Tech has changed, <commit 0> → <commit 1>:
    ~ server github
        args: ["-y", "@modelcontextprotocol/server-github@2025.4.8"] → ["-y", "@modelcontextprotocol/server-github@2025.6.1"]
    + server linear (offered: off until you turn it on)
        auth = "oauth"
        enabled = false
        url = "https://mcp.linear.app/mcp"
    - server sentry
    "#);

    let accepted = accept(&home, &pending).unwrap();
    assert_eq!(accepted.dropped, ["sentry"]);
    assert_eq!(
        std::fs::read_to_string(home.manifest()).unwrap().trim(),
        "",
        "a switch for a server the team dropped would be a server with nothing to run"
    );
    assert_eq!(check(&home, "phin-tech").unwrap(), None);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_server_the_team_only_offers_is_off_until_you_turn_it_on() {
    let root = scratch("offered");
    let text = format!(
        "{TEAM}\n[servers.linear]\nurl = \"https://mcp.linear.app/mcp\"\nenabled = false\n"
    );
    let (_, url) = team_repo(&root, &text);
    let home = Home::new(root.join("home"));
    let pending = subscribe(&home, &url, Asked::default()).unwrap();
    let linear = pending
        .changes
        .iter()
        .find(|change| change.name == "linear")
        .unwrap();
    assert!(linear.offered);
    accept(&home, &pending).unwrap();

    let (layers, _) = home.layers();
    assert!(!resolve(&layers).servers["linear"].enabled);
    std::fs::write(home.manifest(), "[servers.linear]\nenabled = true\n").unwrap();
    let (layers, _) = home.layers();
    let resolved = resolve(&layers);
    assert!(resolved.servers["linear"].enabled);
    assert!(resolved.servers["github"].enabled);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_new_commit_that_only_changes_comments_is_taken_in_without_asking() {
    let root = scratch("comments");
    let (repo, url) = team_repo(&root, TEAM);
    let home = Home::new(root.join("home"));
    accept(&home, &subscribe(&home, &url, Asked::default()).unwrap()).unwrap();
    let text = format!("# Ask in #platform before changing these.\n{TEAM}");
    let moved = commit(&repo, &text, "A comment");
    assert_eq!(check(&home, "phin-tech").unwrap(), None);
    assert_eq!(
        Subscriptions::read(&home).unwrap().teams["phin-tech"].commit,
        moved
    );
    assert_eq!(
        std::fs::read_to_string(share::manifest_path(&home, "phin-tech")).unwrap(),
        text
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_look_at_a_team_that_was_updated_since_is_refused() {
    let root = scratch("stale");
    let (repo, url) = team_repo(&root, TEAM);
    let home = Home::new(root.join("home"));
    accept(&home, &subscribe(&home, &url, Asked::default()).unwrap()).unwrap();
    commit(&repo, &TEAM.replace("2025.4.8", "2025.5.0"), "One");
    let first_look = check(&home, "phin-tech").unwrap().unwrap();
    let second_look = check(&home, "phin-tech").unwrap().unwrap();
    accept(&home, &first_look).unwrap();
    assert!(matches!(
        accept(&home, &second_look),
        Err(TeamError::Stale(_))
    ));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn accepting_on_the_command_line_takes_the_commit_looked_at_not_a_newer_one() {
    let root = scratch("at-commit");
    let (repo, url) = team_repo(&root, TEAM);
    let home = Home::new(root.join("home"));
    accept(&home, &subscribe(&home, &url, Asked::default()).unwrap()).unwrap();
    let looked_at = commit(&repo, &TEAM.replace("2025.4.8", "2025.5.0"), "One");
    commit(&repo, &TEAM.replace("2025.4.8", "2026.1.0"), "Two");
    let pending = at_commit(&home, "phin-tech", &looked_at).unwrap();
    accept(&home, &pending).unwrap();
    let kept = std::fs::read_to_string(share::manifest_path(&home, "phin-tech")).unwrap();
    assert!(kept.contains("2025.5.0"), "{kept}");
    assert!(
        check(&home, "phin-tech").unwrap().is_some(),
        "the newer commit is still there to look at"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn subscribing_at_a_commit_looked_at_still_follows_the_branch() {
    let root = scratch("subscribe-at");
    let (repo, url) = team_repo(&root, TEAM);
    let looked_at = git(&repo, &["rev-parse", "HEAD"]);
    commit(&repo, &TEAM.replace("2025.4.8", "2025.5.0"), "Newer");
    let home = Home::new(root.join("home"));
    let pending = subscribe(
        &home,
        &url,
        Asked {
            at: Some(&looked_at),
            ..Asked::default()
        },
    )
    .unwrap();
    assert_eq!(pending.commit, looked_at);
    assert_eq!(pending.branch, None);
    accept(&home, &pending).unwrap();
    assert!(check(&home, "phin-tech").unwrap().is_some());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_team_that_renames_itself_keeps_its_folder_so_your_keys_stay_found() {
    let root = scratch("renamed");
    let (repo, url) = team_repo(&root, TEAM);
    let home = Home::new(root.join("home"));
    accept(&home, &subscribe(&home, &url, Asked::default()).unwrap()).unwrap();
    commit(
        &repo,
        &TEAM.replace("Phin Tech", "Phin Technologies"),
        "Rename",
    );
    let pending = check(&home, "phin-tech").unwrap().unwrap();
    assert_eq!(pending.folder, "phin-tech");
    assert_eq!(pending.changes[0].part, Part::Team);
    accept(&home, &pending).unwrap();
    let (layers, _) = home.layers();
    let resolved = resolve(&layers);
    assert_eq!(
        resolved.secrets["phin-tech/github-token"].scope,
        Scope::Team("phin-tech".to_string())
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_team_can_be_kept_under_a_name_of_your_own_on_another_branch() {
    let root = scratch("branch");
    let (repo, url) = team_repo(&root, TEAM);
    git(&repo, &["checkout", "-q", "-b", "staging"]);
    let staging = commit(&repo, &TEAM.replace("2025.4.8", "2025.9.9"), "Staging");
    git(&repo, &["checkout", "-q", "main"]);
    let home = Home::new(root.join("home"));
    let pending = subscribe(
        &home,
        &url,
        Asked {
            branch: Some("staging"),
            name: Some("Platform Staging"),
            at: None,
        },
    )
    .unwrap();
    assert_eq!(pending.folder, "platform-staging");
    assert_eq!(pending.commit, staging);
    accept(&home, &pending).unwrap();
    assert_eq!(
        Subscriptions::read(&home).unwrap().teams["platform-staging"]
            .branch
            .as_deref(),
        Some("staging")
    );
    assert_eq!(check(&home, "platform-staging").unwrap(), None);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn removing_a_subscribed_team_forgets_its_repository_and_undo_remembers_it() {
    let root = scratch("remove");
    let (_, url) = team_repo(&root, TEAM);
    let home = Home::new(root.join("home"));
    accept(&home, &subscribe(&home, &url, Asked::default()).unwrap()).unwrap();
    let removed = share::remove(&home, "phin-tech").unwrap();
    assert!(Subscriptions::read(&home).unwrap().teams.is_empty());
    share::restore(&home, &removed).unwrap();
    assert_eq!(
        Subscriptions::read(&home).unwrap().teams["phin-tech"].url,
        url
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn only_a_git_repository_with_a_manifest_can_be_subscribed_to() {
    let root = scratch("refused");
    let home = Home::new(root.join("home"));
    assert_eq!(
        subscribe(&home, "  ", Asked::default()),
        Err(TeamError::NoUrl)
    );
    assert!(matches!(
        subscribe(&home, root.to_str().unwrap(), Asked::default()),
        Err(TeamError::NotARepository(_))
    ));
    assert!(matches!(
        subscribe(&home, "phin-tech/team@one-skill", Asked::default()),
        Err(TeamError::NotARepository(_))
    ));
    let repo = root.join("empty");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    std::fs::write(repo.join("README.md"), "Nothing here").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "Readme"]);
    assert!(matches!(
        subscribe(
            &home,
            &format!("file://{}", repo.display()),
            Asked::default()
        ),
        Err(TeamError::NoManifest(_))
    ));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_repository_that_wants_a_sign_in_says_how_to_give_one() {
    let error = super::fetch_error(
        "https://git.example.com/team.git",
        &crate::skills::SkillsError::Fetch(
            "https://git.example.com/team.git: fatal: could not read Username for 'https://git.example.com': terminal prompts disabled".to_string(),
        ),
    );
    assert_eq!(
        error,
        TeamError::Denied("https://git.example.com/team.git".to_string())
    );
    assert!(error.to_string().contains("clone it once in a terminal"));
}

#[test]
fn the_skills_a_team_installs_come_from_its_own_repository_and_yours_are_left_alone() {
    let root = scratch("skills");
    let text = format!(
        "{TEAM}\n[skills]\nsources = [\".\"]\ninstall = [\"team-review\", \"mine\", \"missing\"]\n"
    );
    let (repo, url) = team_repo(&root, &text);
    for name in ["team-review", "mine", "offered-only"] {
        let folder = repo.join("skills").join(name);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: The team's {name}.\n---\nDo it well.\n"),
        )
        .unwrap();
    }
    commit(&repo, &text, "Skills");
    let client_home = root.join("client-home");
    let folders = Folders::new(&client_home);
    let own = folders.store().join("mine");
    std::fs::create_dir_all(&own).unwrap();
    std::fs::write(
        own.join("SKILL.md"),
        "---\nname: mine\ndescription: Mine.\n---\n",
    )
    .unwrap();

    let home = Home::new(root.join("home"));
    let pending = subscribe(&home, &url, Asked::default()).unwrap();
    let skills: Vec<_> = pending
        .changes
        .iter()
        .filter(|change| change.part == Part::Skill)
        .map(|change| (change.name.as_str(), change.how))
        .collect();
    assert_eq!(
        skills,
        [
            ("mine", How::Added),
            ("missing", How::Added),
            ("team-review", How::Added)
        ]
    );
    accept(&home, &pending).unwrap();
    let done = install_skills(&folders, &pending, &Stamp::from("2026-11-19T00:00:00.000Z"));
    assert_eq!(
        done,
        TeamSkills {
            installed: vec!["team-review".to_string()],
            already: Vec::new(),
            own: vec!["mine".to_string()],
            failed: vec![(
                "missing".to_string(),
                "none of the team's skill sources has it".to_string()
            )],
        }
    );
    assert!(folders.store().join("team-review/SKILL.md").is_file());
    assert!(
        !folders.store().join("offered-only").exists(),
        "a skill the team doesn't name in install is only offered"
    );
    assert_eq!(
        std::fs::read_to_string(own.join("SKILL.md")).unwrap(),
        "---\nname: mine\ndescription: Mine.\n---\n"
    );
    let entry = Lock::read(folders.lock()).get("team-review").unwrap();
    assert_eq!(entry.source_url, url);
    let _ = std::fs::remove_dir_all(&root);
}
