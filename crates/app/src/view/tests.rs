//! The main window's tests: what it shows, what its Commands do, and adding a server by pasting it.

// Not a glob: the kit's `test` macro would shadow the `#[test]` it expands to.
use super::{Editing, MainView};
use crate::commands::COMMANDS;
use crate::shortcuts::SHORTCUTS;
use crate::{gateway, palette, shortcuts, theme};
use gpui_kit::base::Root;
use gpui_kit::{AppContext as _, Context, Entity, TestAppContext, VisualTestContext};
use patchbay_core::clients::{Choice, Client, Layout};
use patchbay_gateway::{SecretNeeded, ServerStatus, Status};

use std::collections::BTreeSet;

use super::servers::wanted_secrets;

/// Opens a window under the Root the app runs in, on a gateway over `manifest` with no secrets kept.
fn open<'a>(
    cx: &'a mut TestAppContext,
    manifest: &str,
) -> (Entity<MainView>, &'a mut VisualTestContext) {
    // A folder for each window opened, since tests run at once and two may have the same manifest.
    static OPENED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "patchbay-view-test-{}-{}",
        std::process::id(),
        OPENED.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("patchbay.toml"), manifest).unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        theme::init(cx);
        cx.bind_keys(shortcuts::bindings());
        gateway::init_with(
            patchbay_core::Home::new(&root),
            gateway::Secrets::Memory,
            cx,
        );
    });
    let view = std::rc::Rc::new(std::cell::RefCell::new(None));
    let (_, cx) = cx.add_window_view(|window, cx: &mut Context<Root>| {
        let main = cx.new(|cx| MainView::new(window, cx));
        *view.borrow_mut() = Some(main.clone());
        Root::new(main, window, cx)
    });
    let view = view.borrow_mut().take().unwrap();
    (view, cx)
}

#[gpui_kit::test]
fn the_window_lists_each_server_and_every_client(cx: &mut TestAppContext) {
    let (view, cx) = open(
        cx,
        "[servers.linear]\nurl = \"https://mcp.linear.app/mcp\"\nauth = \"oauth\"\n",
    );
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.servers.len(), 1);
        assert_eq!(view.servers[0].name, "linear");
        assert_eq!(
            view.clients.len(),
            patchbay_core::clients::Client::ALL.len()
        );
    });
}

#[gpui_kit::test]
fn a_client_given_an_entry_for_each_server_gets_one_and_keeps_up(cx: &mut TestAppContext) {
    let (view, cx) = open(
        cx,
        "[servers.alpha]\nurl = \"http://127.0.0.1:9/a\"\n[servers.beta]\nurl = \"http://127.0.0.1:9/b\"\n",
    );
    let home = cx.update(|_, cx| cx.global::<gateway::Running>().client_home.clone());
    let config = Client::Pi.config_path(&home);
    view.update_in(cx, |view, window, cx| {
        let choice = Choice {
            profile: "default".to_string(),
            layout: Layout::PerServer,
            ..Choice::default()
        };
        view.choose(Client::Pi, choice, window, cx);
        view.connect(Client::Pi, window, cx);
    });
    let mut names = Client::Pi.entries_in(&home);
    names.sort();
    assert_eq!(names, ["patchbay-alpha", "patchbay-beta"]);
    // An entry left from before is put right the next time the window reads the gateway.
    std::fs::write(&config, r#"{"mcpServers": {"patchbay-gone": {}}}"#).unwrap();
    view.update_in(cx, |view, window, cx| view.sync(window, cx));
    let mut names = Client::Pi.entries_in(&home);
    names.sort();
    assert_eq!(names, ["patchbay-alpha", "patchbay-beta"]);
}

#[test]
fn each_secret_two_servers_wait_for_gets_one_field() {
    let secret = SecretNeeded {
        account: "me/k".to_string(),
        prompt: "A key".to_string(),
        help: None,
    };
    let server = |name: &str, status: Status| ServerStatus {
        name: name.to_string(),
        namespace: name.to_string(),
        target: String::new(),
        oauth: false,
        status,
        tools: Vec::new(),
        secrets: vec![secret.clone()],
        team: None,
    };
    let servers = [
        server(
            "a",
            Status::NeedsSecret {
                secrets: vec![secret.clone()],
            },
        ),
        server(
            "b",
            Status::NeedsSecret {
                secrets: vec![secret.clone()],
            },
        ),
        server("c", Status::Running),
    ];
    assert_eq!(
        wanted_secrets(&servers, &BTreeSet::new()),
        std::slice::from_ref(&secret)
    );
    // A running server's key shows only while it's being replaced.
    let running = [server("c", Status::Running)];
    assert!(wanted_secrets(&running, &BTreeSet::new()).is_empty());
    let replacing = BTreeSet::from(["c".to_string()]);
    assert_eq!(wanted_secrets(&running, &replacing), [secret]);
}

#[gpui_kit::test]
fn every_menu_item_and_shortcut_names_a_known_command(_: &mut TestAppContext) {
    for (_, name, _) in SHORTCUTS {
        assert!(COMMANDS.contains(name), "{name}");
    }
    for entry in palette::entries() {
        assert!(
            COMMANDS.contains(&entry.name),
            "{}: {}",
            entry.label,
            entry.name
        );
    }
}

#[gpui_kit::test]
fn every_command_runs_and_can_be_reached(_: &mut TestAppContext) {
    // A debug_assert in `run` only catches a missing case when it's tried; this catches it in `cargo test`.
    let source = [
        include_str!("mod.rs"),
        include_str!("servers.rs"),
        include_str!("clients.rs"),
        include_str!("editor.rs"),
        include_str!("share.rs"),
        include_str!("../commands.rs"),
    ]
    .concat();
    let in_menus: Vec<_> = palette::entries()
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    for name in COMMANDS {
        assert!(
            source.contains(&format!("\"{name}\" =>")),
            "{name} has no case in run"
        );
        let keyed = SHORTCUTS.iter().any(|(_, bound, _)| bound == name);
        assert!(
            in_menus.contains(name) || keyed,
            "{name} is in no menu and has no key"
        );
    }
}

#[gpui_kit::test]
fn keys_opens_over_the_window_and_escape_closes_it(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, "");
    cx.simulate_keystrokes("cmd-/");
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert!(view.card.is_some()));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    view.read_with(cx, |view, _| assert!(view.card.is_none()));
}

#[gpui_kit::test]
fn a_pasted_server_is_added_to_your_manifest_with_its_key_in_the_keychain(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, "# Mine.\n");
    view.update_in(cx, |view, window, cx| {
        view.start_editing(Editing::New, window, cx);
        let super::editor::Body::Paste { field, .. } = &view.editor.as_ref().unwrap().body else {
            panic!("a new server is pasted");
        };
        let field = field.clone();
        field.update(cx, |field, cx| {
            field.set_value(
                r#"{"mcpServers": {"brave": {"command": "npx", "args": ["-y", "brave-mcp"], "env": {"BRAVE_API_KEY": "abc123"}}}}"#,
                window,
                cx,
            );
        });
        view.reread(cx);
        assert_eq!(view.editor.as_ref().unwrap().read.as_ref().map(Vec::len), Ok(1));
        view.save_editor(window, cx);
        assert!(view.editor.is_none());
        assert_eq!(view.server.as_deref(), Some("brave"));
    });
    let (manifest, kept) = cx.update(|_, cx| {
        let running = cx.global::<gateway::Running>();
        (
            std::fs::read_to_string(running.gateway.home().manifest()).unwrap(),
            running.gateway.secrets().get("me/brave-api-key"),
        )
    });
    assert!(manifest.starts_with("# Mine."), "{manifest}");
    assert!(
        manifest.contains("BRAVE_API_KEY = \"secret:brave-api-key\""),
        "{manifest}"
    );
    assert!(
        !manifest.contains("abc123"),
        "the key itself never reaches the manifest"
    );
    assert_eq!(kept.as_deref(), Some("abc123"));
}

const GITHUB: &str = r#"# Mine.

[secrets.github-token]
prompt = "A GitHub personal access token"

# GitHub's own remote server.
[servers.github]
url = "https://api.githubcopilot.com/mcp/"
headers = { Authorization = "Bearer ${secret:github-token}" }

[servers.fetch]
command = "uvx"
args = ["mcp-server-fetch"]
"#;

fn manifest(cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| {
        std::fs::read_to_string(cx.global::<gateway::Running>().gateway.home().manifest()).unwrap()
    })
}

#[gpui_kit::test]
fn a_server_saved_from_its_form_unchanged_leaves_the_manifest_as_it_was(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, GITHUB);
    view.update_in(cx, |view, window, cx| {
        view.start_editing(Editing::Server("github".to_string()), window, cx);
        assert!(matches!(
            view.editor.as_ref().unwrap().body,
            super::editor::Body::Form(_)
        ));
        assert!(view.editor.as_ref().unwrap().read.is_ok());
        view.save_editor(window, cx);
        assert!(view.editor.is_none());
    });
    assert_eq!(manifest(cx), GITHUB);
}

#[gpui_kit::test]
fn a_key_typed_into_a_servers_form_is_kept_in_the_keychain_not_the_manifest(
    cx: &mut TestAppContext,
) {
    let (view, cx) = open(cx, &format!("{GITHUB}\n"));
    view.update_in(cx, |view, window, cx| {
        view.start_editing(Editing::Server("fetch".to_string()), window, cx);
        view.add_row(window, cx);
        let super::editor::Body::Form(form) = &view.editor.as_ref().unwrap().body else {
            panic!("a server that's there is changed in a form");
        };
        let (key, value) = form.last_row().unwrap();
        key.update(cx, |key, cx| key.set_value("FETCH_API_KEY", window, cx));
        value.update(cx, |value, cx| value.set_value("xyz789", window, cx));
        view.reread(cx);
        view.save_editor(window, cx);
    });
    let text = manifest(cx);
    assert!(
        text.contains("env = { FETCH_API_KEY = \"secret:fetch-api-key\" }"),
        "{text}"
    );
    assert!(!text.contains("xyz789"));
    let kept = cx.update(|_, cx| {
        cx.global::<gateway::Running>()
            .gateway
            .secrets()
            .get("me/fetch-api-key")
    });
    assert_eq!(kept.as_deref(), Some("xyz789"));
}

#[gpui_kit::test]
fn the_sidebar_switches_between_the_patch_bay_servers_clients_and_skills(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, "");
    view.read_with(cx, |view, _| assert_eq!(view.place, super::Place::PatchBay));
    cx.simulate_keystrokes("cmd-3");
    view.read_with(cx, |view, _| assert_eq!(view.place, super::Place::Clients));
    cx.simulate_keystrokes("cmd-2");
    view.read_with(cx, |view, _| assert_eq!(view.place, super::Place::Servers));
    cx.simulate_keystrokes("cmd-1");
    view.read_with(cx, |view, _| assert_eq!(view.place, super::Place::PatchBay));
    cx.simulate_keystrokes("cmd-4");
    view.read_with(cx, |view, _| assert_eq!(view.place, super::Place::Skills));
}

#[gpui_kit::test]
fn a_client_given_only_some_servers_gets_a_profile_of_its_own(cx: &mut TestAppContext) {
    let (view, cx) = open(
        cx,
        "[servers.alpha]\nurl = \"http://127.0.0.1:9/a\"\n\n[servers.beta]\nurl = \"http://127.0.0.1:9/b\"\n",
    );
    let read = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let running = cx.global::<gateway::Running>();
            let home = running.gateway.home();
            (
                std::fs::read_to_string(home.manifest()).unwrap(),
                patchbay_core::clients::Choices::load(home)
                    .get(Client::Pi)
                    .profile,
            )
        })
    };
    view.update_in(cx, |view, window, cx| {
        view.give(
            Client::Pi,
            super::Given::Only(vec!["alpha".to_string()]),
            window,
            cx,
        );
    });
    let (manifest, profile) = read(cx);
    assert!(
        manifest.contains("[profiles.pi]\nservers = [\"alpha\"]"),
        "{manifest}"
    );
    assert_eq!(profile, "pi");
    view.update_in(cx, |view, window, cx| {
        view.give(Client::Pi, super::Given::Every, window, cx)
    });
    let (manifest, profile) = read(cx);
    assert!(!manifest.contains("profiles"), "{manifest}");
    assert_eq!(profile, "default");
}

#[gpui_kit::test]
fn an_export_holds_only_the_servers_left_ticked_and_none_of_their_keys(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, &format!("{GITHUB}\n\n"));
    let text = view.update_in(cx, |view, window, cx| {
        view.start_export(window, cx);
        view.toggle_exported("fetch", cx);
        view.exported(cx).unwrap().0
    });
    assert!(text.contains("[servers.github]"), "{text}");
    assert!(text.contains("[secrets.github-token]"), "{text}");
    assert!(!text.contains("fetch"), "{text}");
}

#[gpui_kit::test]
fn a_pasted_team_is_kept_as_its_own_layer_and_not_in_your_manifest(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, "# Only mine, a little longer.\n");
    let team = "# Ours.\n[team]\nname = \"Phin Tech\"\n\n[servers.sentry]\nurl = \"https://mcp.sentry.dev/mcp\"\nauth = \"oauth\"\n";
    view.update_in(cx, |view, window, cx| {
        view.start_editing(Editing::Team, window, cx);
        view.fill_paste(team, window, cx);
        assert_eq!(
            view.editor.as_ref().unwrap().read.as_ref().map(Vec::len),
            Ok(1)
        );
        view.save_editor(window, cx);
        assert!(view.editor.is_none());
    });
    let (mine, theirs) = cx.update(|_, cx| {
        let home = cx.global::<gateway::Running>().gateway.home().clone();
        (
            std::fs::read_to_string(home.manifest()).unwrap(),
            std::fs::read_to_string(home.teams().join("phin-tech/patchbay.toml")).unwrap(),
        )
    });
    assert!(!mine.contains("sentry"), "{mine}");
    assert!(theirs.starts_with("# Ours."), "{theirs}");
}

#[gpui_kit::test]
fn a_team_cant_take_a_name_your_own_keys_are_kept_under(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, "# Only mine, and longer again.\n");
    view.update_in(cx, |view, window, cx| {
        view.start_editing(Editing::Team, window, cx);
        view.fill_paste(
            "[team]\nname = \"me\"\n\n[servers.x]\nurl = \"https://x.example/mcp\"\n",
            window,
            cx,
        );
        assert!(view.editor.as_ref().unwrap().read.is_err());
    });
}

#[gpui_kit::test]
fn a_server_is_turned_off_and_on_in_your_own_manifest(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, &format!("{GITHUB}\n\n\n"));
    view.update_in(cx, |view, window, cx| {
        view.set_enabled("fetch", false, window, cx)
    });
    assert!(manifest(cx).contains("enabled = false"), "{}", manifest(cx));
    view.update_in(cx, |view, window, cx| {
        view.set_enabled("fetch", true, window, cx)
    });
    assert_eq!(manifest(cx), format!("{GITHUB}\n\n\n"));
}

#[gpui_kit::test]
fn removing_a_team_takes_its_folder_out_and_leaves_your_servers(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, "# Mine, before a team is removed.\n");
    let team =
        "[team]\nname = \"Phin Tech\"\n\n[servers.sentry]\nurl = \"https://mcp.sentry.dev/mcp\"\n";
    view.update_in(cx, |view, window, cx| {
        view.start_editing(Editing::Team, window, cx);
        view.fill_paste(team, window, cx);
        view.save_editor(window, cx);
        cx.global::<gateway::Running>().reload_now();
        view.sync(window, cx);
        view.choose_server("sentry", cx);
        view.remove_chosen_team(window, cx);
    });
    let (folder, mine) = cx.update(|_, cx| {
        let home = cx.global::<gateway::Running>().gateway.home().clone();
        (
            home.teams().join("phin-tech").exists(),
            std::fs::read_to_string(home.manifest()).unwrap(),
        )
    });
    assert!(!folder);
    assert_eq!(mine, "# Mine, before a team is removed.\n");
}

#[gpui_kit::test]
fn skills_found_in_a_pasted_source_are_installed_for_each_client_and_listed(
    cx: &mut TestAppContext,
) {
    let (view, cx) = open(cx, "# Skills, from a folder on this Mac.\n");
    let home = cx.update(|_, cx| cx.global::<gateway::Running>().client_home.clone());
    let source = home.join("my-skills");
    for name in ["review", "plan"] {
        std::fs::create_dir_all(source.join("skills").join(name)).unwrap();
        std::fs::write(
            source.join("skills").join(name).join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Helps with {name}\n---\n"),
        )
        .unwrap();
    }
    // A skill of the person's own, which is listed and left alone.
    std::fs::create_dir_all(home.join(".cursor/skills/mine")).unwrap();
    std::fs::write(
        home.join(".cursor/skills/mine/SKILL.md"),
        "---\nname: mine\ndescription: Of my own\n---\n",
    )
    .unwrap();
    let line = format!(
        "npx skills add {} --skill review -a claude-code",
        source.display()
    );
    view.update_in(cx, |view, window, cx| {
        view.run("skills.add", window, cx);
        let field = view.skills.adding.as_ref().unwrap().field.clone();
        field.update(cx, |field, cx| field.set_value(line.clone(), window, cx));
        view.find_skills(window, cx);
    });
    cx.run_until_parked();
    view.update_in(cx, |view, window, cx| view.install_skills(window, cx));
    assert!(home.join(".claude/skills/review/SKILL.md").is_file());
    assert!(!home.join(".agents/skills/plan").exists());
    view.read_with(cx, |view, _| {
        assert!(view.skills.adding.is_none());
        let names: Vec<&str> = view
            .skills
            .list
            .iter()
            .map(|skill| skill.name.as_str())
            .collect();
        assert_eq!(names, ["mine", "review"]);
        assert_eq!(view.skills.chosen.as_deref(), Some("review"));
    });
    view.update_in(cx, |view, window, cx| {
        view.skills.chosen = Some("mine".to_string());
        view.run("skills.remove", window, cx);
    });
    assert!(home.join(".cursor/skills/mine/SKILL.md").is_file());
}

#[gpui_kit::test]
fn a_clients_own_servers_are_taken_over_into_your_manifest_and_out_of_its_config(
    cx: &mut TestAppContext,
) {
    let (view, cx) = open(cx, "# Mine, before Cursor's servers are taken over.\n");
    let home = cx.update(|_, cx| cx.global::<gateway::Running>().client_home.clone());
    let config = Client::Cursor.config_path(&home);
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(
        &config,
        r#"{"mcpServers": {"github": {"command": "npx", "args": ["-y", "server-github"], "env": {"GITHUB_TOKEN": "ghp_abc123def456ghi789"}}}}"#,
    )
    .unwrap();
    view.update_in(cx, |view, window, cx| {
        view.sync(window, cx);
        view.client = Some(Client::Cursor);
        view.run("clients.take-over", window, cx);
        assert!(view.editor.as_ref().unwrap().read.is_ok());
        view.save_editor(window, cx);
    });
    let mine = manifest(cx);
    assert!(mine.contains("[servers.github]"), "{mine}");
    assert!(
        !mine.contains("ghp_abc123"),
        "the key went to the Keychain: {mine}"
    );
    assert!(Client::Cursor.own_servers(&home).is_empty());
    assert_eq!(Client::Cursor.entries_in(&home), ["patchbay"]);
}

#[gpui_kit::test]
fn connecting_a_client_with_servers_of_its_own_offers_once_to_take_them_over(
    cx: &mut TestAppContext,
) {
    let (view, cx) = open(cx, "");
    let home = cx.update(|_, cx| cx.global::<gateway::Running>().client_home.clone());
    let config = Client::Cursor.config_path(&home);
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(
        &config,
        r#"{"mcpServers": {"linear": {"url": "https://mcp.linear.app/mcp"}}}"#,
    )
    .unwrap();
    let offered = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let home = cx.global::<gateway::Running>().gateway.home().clone();
            patchbay_core::clients::Choices::load(&home)
                .get(Client::Cursor)
                .offered_take_over
        })
    };
    assert!(!offered(cx));
    view.update_in(cx, |view, window, cx| {
        view.connect(Client::Cursor, window, cx)
    });
    assert!(offered(cx), "offered the first time");
    // Only an offer: the client keeps its own server until it's taken over.
    assert_eq!(Client::Cursor.own_servers(&home)[0].0, "linear");
}

#[gpui_kit::test]
fn a_server_taken_out_of_the_paste_stays_in_the_client_it_came_from(cx: &mut TestAppContext) {
    let (view, cx) = open(
        cx,
        "[servers.github]\nurl = \"https://api.githubcopilot.com/mcp/\"\n",
    );
    let home = cx.update(|_, cx| cx.global::<gateway::Running>().client_home.clone());
    let config = Client::Cursor.config_path(&home);
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    // GitHub is one Patchbay has already, so it's left; linear is taken out of the paste before saving.
    std::fs::write(
        &config,
        r#"{"mcpServers": {"github": {"command": "gh-mcp"}, "linear": {"url": "https://mcp.linear.app/mcp"}, "notion": {"url": "https://mcp.notion.com/mcp"}}}"#,
    )
    .unwrap();
    view.update_in(cx, |view, window, cx| {
        view.sync(window, cx);
        view.client = Some(Client::Cursor);
        view.run("clients.take-over", window, cx);
        view.fill_paste(
            r#"{"mcpServers": {"notion": {"url": "https://mcp.notion.com/mcp"}}}"#,
            window,
            cx,
        );
        view.save_editor(window, cx);
    });
    let mine = manifest(cx);
    assert!(mine.contains("[servers.notion]"), "{mine}");
    assert!(!mine.contains("gh-mcp"), "Patchbay's github stays: {mine}");
    let mut left: Vec<String> = Client::Cursor
        .own_servers(&home)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    left.sort();
    assert_eq!(left, ["github", "linear"]);
}

/// The servers `client` is given now, as the patch bay shows them plugged in.
fn plugged(view: &Entity<MainView>, client: Client, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, _| {
        let row = view
            .clients
            .iter()
            .find(|row| row.client == client)
            .unwrap();
        let mut servers = view.given(row);
        servers.sort();
        servers
    })
}

/// Presses at `from`, moves to `to` on the way, and lets go there.
fn drag(
    from: gpui_kit::Point<gpui_kit::Pixels>,
    to: gpui_kit::Point<gpui_kit::Pixels>,
    cx: &mut VisualTestContext,
) {
    use gpui_kit::{Modifiers, MouseButton};
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
    let middle = gpui_kit::point((from.x + to.x) / 2., (from.y + to.y) / 2.);
    cx.simulate_mouse_move(middle, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::default());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
}

const TWO_SERVERS: &str = "[servers.alpha]\nurl = \"http://127.0.0.1:9/a\"\n\n[servers.beta]\nurl = \"http://127.0.0.1:9/b\"\n";

#[gpui_kit::test]
fn dragging_a_cable_from_a_servers_jack_to_a_client_gives_that_client_the_server(
    cx: &mut TestAppContext,
) {
    let (view, cx) = open(cx, TWO_SERVERS);
    view.update_in(cx, |view, window, cx| {
        view.give(Client::Codex, super::Given::Only(Vec::new()), window, cx);
    });
    cx.run_until_parked();
    assert!(plugged(&view, Client::Codex, cx).is_empty());
    // Beta's jack for Pi already holds a Cable; Codex's for beta is empty, so that's where the new one starts.
    let (from, to) = view.read_with(cx, |view, _| {
        (
            view.jack_on_screen(false, Client::Codex, "beta"),
            view.client_on_screen(Client::Codex),
        )
    });
    drag(from, to, cx);
    assert_eq!(plugged(&view, Client::Codex, cx), ["beta"]);
}

#[gpui_kit::test]
fn a_plug_dragged_to_another_client_moves_the_server_to_it(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, TWO_SERVERS);
    view.update_in(cx, |view, window, cx| {
        view.give(Client::Codex, super::Given::Only(Vec::new()), window, cx);
    });
    cx.run_until_parked();
    let (from, to) = view.read_with(cx, |view, _| {
        (
            view.jack_on_screen(true, Client::Pi, "alpha"),
            view.client_on_screen(Client::Codex),
        )
    });
    drag(from, to, cx);
    assert_eq!(plugged(&view, Client::Pi, cx), ["beta"]);
    assert_eq!(plugged(&view, Client::Codex, cx), ["alpha"]);
}

#[gpui_kit::test]
fn a_plug_let_go_over_nothing_is_pulled_out_and_a_click_puts_it_back(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, TWO_SERVERS);
    cx.run_until_parked();
    let (jack, gap) = view.read_with(cx, |view, _| {
        (
            view.jack_on_screen(false, Client::Pi, "alpha"),
            view.gap_on_screen(),
        )
    });
    drag(jack, gap, cx);
    assert_eq!(plugged(&view, Client::Pi, cx), ["beta"]);
    cx.simulate_click(jack, gpui_kit::Modifiers::default());
    cx.run_until_parked();
    assert_eq!(plugged(&view, Client::Pi, cx), ["alpha", "beta"]);
    // Two clicks in a row each count, rather than the second reading what was there before the first.
    cx.simulate_click(jack, gpui_kit::Modifiers::default());
    cx.simulate_click(jack, gpui_kit::Modifiers::default());
    cx.run_until_parked();
    assert_eq!(plugged(&view, Client::Pi, cx), ["alpha", "beta"]);
}

#[gpui_kit::test]
fn a_skill_dragged_to_a_client_is_linked_into_its_folder_and_a_hard_wired_one_stays(
    cx: &mut TestAppContext,
) {
    let (view, cx) = open(cx, TWO_SERVERS);
    let home = cx.update(|_, cx| cx.global::<gateway::Running>().client_home.clone());
    gateway::seed_skills(&home);
    view.update(cx, |view, cx| view.sync_skills(cx));
    cx.run_until_parked();
    let linked = home.join(".cursor/skills/code-review");
    assert!(!linked.exists());
    let (from, to) = view.read_with(cx, |view, _| {
        (
            view.skill_jack_on_screen(Client::Cursor, "code-review"),
            view.client_on_screen(Client::Cursor),
        )
    });
    drag(from, to, cx);
    assert!(linked.join("SKILL.md").is_file());
    // Pulled out again by letting go of its plug over nothing.
    let gap = view.read_with(cx, |view, _| view.gap_on_screen());
    drag(from, gap, cx);
    assert!(!linked.exists());
    // Codex reads the store itself, so its Cable to the skill can't be pulled out.
    let wired = view.read_with(cx, |view, _| {
        view.skill_jack_on_screen(Client::Codex, "code-review")
    });
    drag(wired, gap, cx);
    assert!(home.join(".agents/skills/code-review/SKILL.md").is_file());
    view.read_with(cx, |view, _| {
        let skill = view
            .skills
            .list
            .iter()
            .find(|skill| skill.folder == "code-review")
            .unwrap();
        assert!(
            skill
                .clients
                .iter()
                .any(|(client, _)| *client == Client::Codex)
        );
    });
}
