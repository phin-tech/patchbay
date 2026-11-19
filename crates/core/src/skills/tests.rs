use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{
    Change, Checking, Folders, Keeper, Kind, Lock, Preview, Reach, SkillsError, Source, check,
    folder_name, install, list, remove,
};
use crate::clients::Client;

fn folder(name: &str) -> PathBuf {
    let folder =
        std::env::temp_dir().join(format!("patchbay-skills-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).unwrap();
    folder
}

fn write_skill(repo: &Path, name: &str, body: &str) {
    let at = repo.join("skills").join(name);
    fs::create_dir_all(&at).unwrap();
    fs::write(
        at.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Helps with {name}\n---\n{body}\n"),
    )
    .unwrap();
}

/// A git repository of skills, committed, as a `file://` source.
fn repo(name: &str, skills: &[&str]) -> (PathBuf, Source) {
    let repo = folder(name);
    for skill in skills {
        write_skill(&repo, skill, "Do it well.");
    }
    commit(&repo);
    let source = Source {
        kind: Kind::Git,
        url: format!("file://{}", repo.display()),
        reference: None,
        subpath: None,
        skill: None,
    };
    (repo, source)
}

fn commit(repo: &Path) {
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .current_dir(repo)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "{}",
            String::from_utf8_lossy(&status.stderr)
        );
    };
    if !repo.join(".git").exists() {
        git(&["init", "-q"]);
    }
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "skills"]);
}

#[test]
fn installing_copies_into_the_store_links_each_client_and_records_it_in_the_lock() {
    let home = folder("install-home");
    let folders = Folders::new(&home);
    let (_repo, source) = repo("install-repo", &["review", "plan"]);
    let preview = Preview::fetch(source).unwrap();
    let names: Vec<&str> = preview
        .found
        .iter()
        .map(|found| found.name.as_str())
        .collect();
    assert_eq!(names, ["plan", "review"]);
    let chosen = preview.choose(&["review".into()]).unwrap();
    let installed = install(
        &folders,
        &preview,
        &chosen,
        &[Client::ClaudeCode, Client::Codex, Client::Cursor],
        "now",
    )
    .unwrap();
    assert_eq!(
        installed[0].linked,
        [Client::ClaudeCode, Client::Codex, Client::Cursor]
    );
    assert!(home.join(".agents/skills/review/SKILL.md").is_file());
    let link = fs::read_link(home.join(".claude/skills/review")).unwrap();
    assert_eq!(link, Path::new("../../.agents/skills/review"));
    let entry = Lock::read(folders.lock()).get("review").unwrap();
    assert_eq!(entry.source_type, "git");
    assert_eq!(entry.skill_path.as_deref(), Some("skills/review/SKILL.md"));
    assert_eq!(
        entry.folder_hash,
        super::folder_hash(&home.join(".agents/skills/review")).unwrap()
    );
}

#[test]
fn a_skill_of_someones_own_is_never_replaced() {
    let home = folder("own-home");
    let folders = Folders::new(&home);
    let (_repo, source) = repo("own-repo", &["review"]);
    // A folder of the person's own in Claude Code's skills, and one the CLI didn't record in the store.
    fs::create_dir_all(home.join(".claude/skills/review")).unwrap();
    fs::write(home.join(".claude/skills/review/SKILL.md"), "mine").unwrap();
    let preview = Preview::fetch(source).unwrap();
    let chosen = preview.choose(&[]).unwrap_or_default();
    assert!(chosen.is_empty(), "nothing is chosen unless asked for");
    let chosen = preview.choose(&["*".into()]).unwrap();
    let installed = install(&folders, &preview, &chosen, &[Client::ClaudeCode], "now").unwrap();
    assert_eq!(installed[0].blocked, [Client::ClaudeCode]);
    assert_eq!(
        fs::read_to_string(home.join(".claude/skills/review/SKILL.md")).unwrap(),
        "mine"
    );

    let unrecorded = folder("own-home-2");
    fs::create_dir_all(unrecorded.join(".agents/skills/review")).unwrap();
    assert!(matches!(
        install(&Folders::new(&unrecorded), &preview, &chosen, &[], "now"),
        Err(SkillsError::NotOurs(_))
    ));
    assert!(matches!(
        remove(&folders, "nothing"),
        Err(SkillsError::NotInstalled(_))
    ));
}

#[test]
fn listing_says_who_keeps_each_skill_and_which_clients_have_it() {
    let home = folder("list-home");
    let folders = Folders::new(&home);
    let (_repo, source) = repo("list-repo", &["review"]);
    let preview = Preview::fetch(source).unwrap();
    let chosen = preview.choose(&["review".into()]).unwrap();
    install(
        &folders,
        &preview,
        &chosen,
        &[Client::ClaudeCode, Client::Pi],
        "now",
    )
    .unwrap();
    fs::create_dir_all(home.join(".cursor/skills/mine")).unwrap();
    fs::write(
        home.join(".cursor/skills/mine/SKILL.md"),
        "---\nname: mine\ndescription: Of my own\n---\n",
    )
    .unwrap();
    let skills = list(&folders);
    let summary: Vec<String> = skills
        .iter()
        .map(|skill| {
            let clients: Vec<String> = skill
                .clients
                .iter()
                .map(|(client, reach)| {
                    format!(
                        "{}{}",
                        client.slug(),
                        if *reach == Reach::Own { " (own)" } else { "" }
                    )
                })
                .collect();
            format!(
                "{} [{}] {}: {}",
                skill.name,
                if matches!(skill.keeper, Keeper::Locked(_)) {
                    "locked"
                } else {
                    "unmanaged"
                },
                skill.description.clone().unwrap_or_default(),
                clients.join(", ")
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            "mine [unmanaged] Of my own: cursor (own)",
            "review [locked] Helps with review: claude-code, codex, pi",
        ]
    );
}

#[test]
fn removing_takes_out_the_links_the_folder_and_the_lock_entry() {
    let home = folder("remove-home");
    let folders = Folders::new(&home);
    let (_repo, source) = repo("remove-repo", &["review"]);
    let preview = Preview::fetch(source).unwrap();
    let chosen = preview.choose(&["review".into()]).unwrap();
    install(
        &folders,
        &preview,
        &chosen,
        &[Client::ClaudeCode, Client::GeminiCli],
        "now",
    )
    .unwrap();
    assert_eq!(remove(&folders, "Review").unwrap(), "review");
    assert!(fs::symlink_metadata(home.join(".claude/skills/review")).is_err());
    assert!(fs::symlink_metadata(home.join(".gemini/skills/review")).is_err());
    assert!(!home.join(".agents/skills/review").exists());
    assert!(Lock::read(folders.lock()).names().is_empty());
}

#[test]
fn checking_finds_what_changed_and_updating_applies_it() {
    let home = folder("update-home");
    let folders = Folders::new(&home);
    let (repo, source) = repo("update-repo", &["review", "plan"]);
    let preview = Preview::fetch(source).unwrap();
    let chosen = preview.choose(&["*".into()]).unwrap();
    install(&folders, &preview, &chosen, &[Client::ClaudeCode], "then").unwrap();
    drop(preview);
    assert_eq!(
        check(&folders, &[], Checking::Report, "now")
            .unwrap()
            .updates,
        []
    );

    write_skill(&repo, "review", "Do it better.");
    fs::write(repo.join("skills/review/checklist.md"), "- one\n").unwrap();
    commit(&repo);
    let checked = check(&folders, &[], Checking::Report, "now").unwrap();
    assert_eq!(checked.updates.len(), 1);
    assert_eq!(checked.updates[0].name, "review");
    assert_eq!(
        checked.updates[0].changes,
        [
            Change::Changed("SKILL.md".into()),
            Change::Added("checklist.md".into())
        ]
    );
    check(
        &folders,
        &["review".into()],
        Checking::Apply(checked.updates.clone()),
        "now",
    )
    .unwrap();
    assert!(home.join(".claude/skills/review/checklist.md").is_file());
    let entry = Lock::read(folders.lock()).get("review").unwrap();
    assert_eq!(entry.updated_at.as_deref(), Some("now"));
    assert_eq!(entry.installed_at.as_deref(), Some("then"));
    assert_eq!(
        check(&folders, &[], Checking::Report, "later")
            .unwrap()
            .updates,
        []
    );
}

#[test]
fn a_lock_that_cant_be_read_is_left_alone() {
    let home = folder("broken-home");
    let folders = Folders::new(&home);
    fs::create_dir_all(home.join(".agents")).unwrap();
    fs::write(folders.lock(), "{ not json").unwrap();
    assert!(matches!(
        remove(&folders, "x"),
        Err(SkillsError::UnreadableLock(_))
    ));
    assert_eq!(fs::read_to_string(folders.lock()).unwrap(), "{ not json");
}

#[test]
fn a_skill_can_be_given_to_one_more_client_or_taken_from_one() {
    let home = folder("link-home");
    let folders = Folders::new(&home);
    let (_repo, source) = repo("link-repo", &["review"]);
    let preview = Preview::fetch(source).unwrap();
    let chosen = preview.choose(&["review".into()]).unwrap();
    install(&folders, &preview, &chosen, &[Client::ClaudeCode], "now").unwrap();
    super::link_client(&folders, "review", Client::Cursor).unwrap();
    assert!(home.join(".cursor/skills/review/SKILL.md").is_file());
    super::unlink_client(&folders, "review", Client::ClaudeCode).unwrap();
    assert!(fs::symlink_metadata(home.join(".claude/skills/review")).is_err());
    assert!(matches!(
        super::unlink_client(&folders, "review", Client::Codex),
        Err(SkillsError::ReadsStore(_))
    ));
}

#[test]
fn taking_over_a_skill_links_the_copies_that_match_and_keeps_the_rest() {
    let home = folder("take-home");
    let folders = Folders::new(&home);
    let skill = "---\nname: bedrock\ndescription: Bedrock\n---\nBody\n";
    for at in [
        ".claude/skills/bedrock",
        ".cursor/skills/bedrock",
        ".gemini/skills/bedrock",
    ] {
        fs::create_dir_all(home.join(at)).unwrap();
        fs::write(home.join(at).join("SKILL.md"), skill).unwrap();
    }
    fs::write(
        home.join(".gemini/skills/bedrock/SKILL.md"),
        "changed by hand",
    )
    .unwrap();
    let taken = super::take_over(&folders, "bedrock", "now").unwrap();
    assert_eq!(taken.name, "bedrock");
    assert_eq!(taken.linked, [Client::ClaudeCode, Client::Cursor]);
    assert_eq!(taken.kept, [Client::GeminiCli]);
    assert_eq!(
        fs::read_link(home.join(".claude/skills/bedrock")).unwrap(),
        Path::new("../../.agents/skills/bedrock")
    );
    assert_eq!(
        fs::read_to_string(home.join(".gemini/skills/bedrock/SKILL.md")).unwrap(),
        "changed by hand"
    );
    let entry = Lock::read(folders.lock()).get("bedrock").unwrap();
    assert_eq!(entry.source_type, "local");
    assert!(list(&folders)[0].managed());
    // Taken over, it has nothing to fetch, and can't be taken over twice.
    let checked = check(&folders, &[], Checking::Report, "later").unwrap();
    assert_eq!((checked.updates.len(), checked.unchecked.len()), (0, 0));
    assert!(matches!(
        super::take_over(&folders, "bedrock", "now"),
        Err(SkillsError::AlreadyKept(_))
    ));
}

#[test]
fn folder_names_are_made_as_the_cli_makes_them() {
    assert_eq!(folder_name("Web Design"), "web-design");
    assert_eq!(folder_name("..sneaky/../x"), "sneaky-..-x");
    assert_eq!(folder_name("--"), "unnamed-skill");
    assert_eq!(folder_name("a_b.c"), "a_b.c");
}

#[test]
fn taking_over_a_skill_never_removes_a_copy_holding_what_the_store_leaves_out() {
    let home = folder("take-git");
    let folders = Folders::new(&home);
    let skill = "---\nname: bedrock\ndescription: Bedrock\n---\nBody\n";
    for at in [".claude/skills/bedrock", ".cursor/skills/bedrock"] {
        fs::create_dir_all(home.join(at)).unwrap();
        fs::write(home.join(at).join("SKILL.md"), skill).unwrap();
    }
    // Claude Code's copy is a git checkout, and Cursor's has a link out of the folder: neither reaches the store.
    fs::create_dir_all(home.join(".claude/skills/bedrock/.git")).unwrap();
    fs::write(home.join(".claude/skills/bedrock/.git/HEAD"), "ref").unwrap();
    std::os::unix::fs::symlink(&home, home.join(".cursor/skills/bedrock/home")).unwrap();
    let taken = super::take_over(&folders, "bedrock", "now").unwrap();
    assert_eq!(taken.linked, []);
    assert_eq!(taken.kept, [Client::ClaudeCode, Client::Cursor]);
    assert!(home.join(".claude/skills/bedrock/.git/HEAD").is_file());
    assert!(home.join(".cursor/skills/bedrock/home").is_symlink());
    assert!(folders.store().join("bedrock/SKILL.md").is_file());
}

#[test]
fn taking_over_a_skill_keeps_a_copy_whose_line_endings_differ() {
    let home = folder("take-crlf");
    let folders = Folders::new(&home);
    for (at, body) in [
        (".claude/skills/bedrock", "---\nname: bedrock\n---\nBody\n"),
        (
            ".cursor/skills/bedrock",
            "---\r\nname: bedrock\r\n---\r\nBody\r\n",
        ),
    ] {
        fs::create_dir_all(home.join(at)).unwrap();
        fs::write(home.join(at).join("SKILL.md"), body).unwrap();
    }
    let taken = super::take_over(&folders, "bedrock", "now").unwrap();
    assert_eq!(taken.linked, [Client::ClaudeCode]);
    assert_eq!(taken.kept, [Client::Cursor]);
}

#[test]
fn an_update_applies_only_what_was_listed() {
    let home = folder("update-listed-home");
    let folders = Folders::new(&home);
    let (repo, source) = repo("update-listed-repo", &["review", "plan"]);
    let preview = Preview::fetch(source).unwrap();
    let chosen = preview.choose(&["*".into()]).unwrap();
    install(&folders, &preview, &chosen, &[Client::ClaudeCode], "then").unwrap();
    drop(preview);
    write_skill(&repo, "review", "Do it better.");
    commit(&repo);
    let listed = check(&folders, &[], Checking::Report, "now")
        .unwrap()
        .updates;
    // The source moves on again between the check and the update, and a skill is installed meanwhile.
    write_skill(&repo, "review", "Do it differently.");
    commit(&repo);
    let mut lock = Lock::read(folders.lock());
    let mut other = lock.get("plan").unwrap();
    other.source = "elsewhere".into();
    lock.set("meanwhile", &other, "now");
    lock.write(folders.lock()).unwrap();
    let applied = check(&folders, &[], Checking::Apply(listed), "now").unwrap();
    assert_eq!(applied.updates, []);
    assert_eq!(applied.unchecked[0].0, "review");
    assert!(
        fs::read_to_string(folders.store().join("review/SKILL.md"))
            .unwrap()
            .contains("Do it well."),
        "nothing newer than was listed is put in place"
    );
    // Checked again, it's listed as it is now and applies, and the skill installed meanwhile keeps its entry.
    let listed = check(&folders, &[], Checking::Report, "now")
        .unwrap()
        .updates;
    check(&folders, &[], Checking::Apply(listed), "now").unwrap();
    assert!(
        fs::read_to_string(folders.store().join("review/SKILL.md"))
            .unwrap()
            .contains("Do it differently.")
    );
    assert!(Lock::read(folders.lock()).get("meanwhile").is_some());
    // Applying with nothing listed fetches nothing and changes nothing.
    assert_eq!(
        check(&folders, &[], Checking::Apply(Vec::new()), "now").unwrap(),
        Default::default()
    );
}
