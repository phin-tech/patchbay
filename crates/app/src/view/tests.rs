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
    let root = std::env::temp_dir().join(format!(
        "patchbay-view-test-{}-{}",
        std::process::id(),
        manifest.len()
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
fn the_sidebar_switches_between_servers_and_clients(cx: &mut TestAppContext) {
    let (view, cx) = open(cx, "");
    cx.simulate_keystrokes("cmd-2");
    view.read_with(cx, |view, _| assert_eq!(view.place, super::Place::Clients));
    cx.simulate_keystrokes("cmd-1");
    view.read_with(cx, |view, _| assert_eq!(view.place, super::Place::Servers));
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
