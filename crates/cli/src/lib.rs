//! The `patchbay` command. It's the app's own binary: given a command it knows, `main` runs it here and exits
//! before any window opens. Anything else opens the app, since macOS starts apps with arguments of its own.
//!
//! Being the app's binary matters for the Keychain: the secrets the command line keeps are the app's, with no
//! second program to allow.

use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;

use patchbay_core::clients::{Choices, Client, Layout};
use patchbay_core::manifest::resolve;
use patchbay_core::secrets::Secrets;
use patchbay_core::{Home, version};
use patchbay_gateway::{Gateway, GatewayFile, View, client_entries, gateway_token, oauth};

/// What a command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Version,
    Help,
    /// Runs the gateway without the app, until interrupted.
    Serve,
    /// The gateway over stdio, for clients that only run commands, at a route under `/mcp/` (`default`,
    /// `server/linear`, `small/compact`).
    Bridge {
        route: String,
    },
    Status,
    /// Keeps a secret, read from stdin, under a Keychain account (`me/github-token`).
    Secret {
        account: String,
    },
    SignIn {
        server: String,
    },
    SignOut {
        server: String,
    },
    /// Points a client at the gateway; a profile or a layout not given keeps the one chosen before.
    Connect {
        client: Client,
        profile: Option<String>,
        layout: Option<Layout>,
        /// Only these servers, in a profile of the client's own; `all` for every one.
        servers: Option<Vec<String>>,
    },
    Disconnect {
        client: Client,
    },
    Clients,
    /// A command it knows, said wrongly.
    Wrong(String),
}

const USAGE: &str = "usage: patchbay <command>

  status                    the gateway's servers and what each needs
  secret <account>          keep a secret (read from stdin) in the Keychain, as `me/<name>`
  sign-in <server>          sign in to an OAuth server in the browser
  sign-out <server>         forget a server's sign-in
  clients                   the AI clients Patchbay knows and whether each is connected
  connect <client> [--servers a,b|all] [--profile <name>] [--layout one|per-server|compact]
                            point a client at the gateway: one entry (every server's tools under its
                            name), an entry for each server, or three tools that find and call the rest
  disconnect <client>       take Patchbay's entries out of a client
  serve                     run the gateway without the app
  bridge [route]            the gateway over stdio, for a client to run: default, server/<name>,
                            <profile>, <profile>/compact
  --version, --help

With no arguments, opens Patchbay. Your servers are in
~/Library/Application Support/tech.phin.patchbay/patchbay.toml (PATCHBAY_HOME moves it).";

/// The command `args` (without the program's name) ask for, or none, to open the app.
pub fn parse<S: AsRef<str>>(args: &[S]) -> Option<Command> {
    let args: Vec<&str> = args.iter().map(AsRef::as_ref).collect();
    let route = |at: usize| args.get(at).map_or("default", |p| *p).to_string();
    let client = |at: usize| {
        args.get(at)
            .and_then(|slug| Client::from_slug(slug))
            .ok_or_else(|| {
                let known: Vec<&str> = Client::ALL.iter().map(|client| client.slug()).collect();
                format!("name a client: {}", known.join(", "))
            })
    };
    let named = |at: usize, what: &str| {
        args.get(at)
            .map(|name| (*name).to_string())
            .ok_or_else(|| format!("name the {what}"))
    };
    let command = match *args.first()? {
        "--version" | "-v" | "version" => Ok(Command::Version),
        "--help" | "-h" | "help" => Ok(Command::Help),
        "serve" => Ok(Command::Serve),
        "bridge" => Ok(Command::Bridge { route: route(1) }),
        "status" => Ok(Command::Status),
        "clients" => Ok(Command::Clients),
        "secret" => named(1, "secret's account, like me/github-token")
            .map(|account| Command::Secret { account }),
        "sign-in" => named(1, "server").map(|server| Command::SignIn { server }),
        "sign-out" => named(1, "server").map(|server| Command::SignOut { server }),
        "connect" => client(1).and_then(|client| connect_options(client, &args[2..])),
        "disconnect" => client(1).map(|client| Command::Disconnect { client }),
        _ => return None,
    };
    Some(command.unwrap_or_else(Command::Wrong))
}

/// `connect`'s `--profile` and `--layout`.
fn connect_options(client: Client, args: &[&str]) -> Result<Command, String> {
    let (mut profile, mut layout, mut servers) = (None, None, None);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match *arg {
            "--profile" => profile = Some(args.next().ok_or("name the profile")?.to_string()),
            "--servers" => {
                let names = args.next().ok_or("name the servers, or all")?;
                servers = Some(names.split(',').map(str::to_string).collect());
            }
            "--layout" => {
                layout = Some(match *args.next().ok_or("name the layout")? {
                    "one" | "one-entry" => Layout::OneEntry,
                    "per-server" => Layout::PerServer,
                    "compact" => Layout::Compact,
                    other => {
                        return Err(format!(
                            "{other} isn't a layout: one, per-server or compact"
                        ));
                    }
                });
            }
            other => return Err(format!("connect doesn't take {other}")),
        }
    }
    Ok(Command::Connect {
        client,
        profile,
        layout,
        servers,
    })
}

impl Command {
    /// Runs the command, writing to `out`, and gives the exit status.
    pub fn run(self, out: &mut impl Write) -> io::Result<i32> {
        match self {
            Command::Version => writeln!(out, "patchbay {}", version())?,
            Command::Help => writeln!(out, "{USAGE}")?,
            Command::Wrong(reason) => {
                writeln!(out, "patchbay: {reason}\n\n{USAGE}")?;
                return Ok(2);
            }
            command => {
                let runtime = tokio::runtime::Runtime::new()?;
                if let Err(error) = runtime.block_on(command.run_async(out)) {
                    writeln!(out, "patchbay: {error:#}")?;
                    return Ok(1);
                }
            }
        }
        Ok(0)
    }

    async fn run_async(self, out: &mut impl Write) -> anyhow::Result<()> {
        let home = Home::from_env();
        let secrets: Arc<dyn Secrets> = patchbay_core::secrets::from_env(&home);
        match self {
            Command::Serve => {
                let gateway = Gateway::new(home, Arc::clone(&secrets));
                gateway.reload().await;
                for problem in gateway.problems() {
                    writeln!(out, "problem: {problem}")?;
                }
                let token = gateway_token(secrets.as_ref())?;
                let listening =
                    patchbay_gateway::serve(gateway.clone(), patchbay_core::port(), token).await?;
                writeln!(
                    out,
                    "Patchbay's gateway is at http://{}/mcp",
                    listening.addr
                )?;
                out.flush()?;
                gateway.start_all().await;
                for server in gateway.statuses() {
                    writeln!(out, "  {}: {}", server.name, describe(&server.status))?;
                }
                out.flush()?;
                tokio::signal::ctrl_c().await?;
                listening.shutdown.cancel();
            }
            Command::Bridge { route } => {
                let view = View::parse(&route)
                    .ok_or_else(|| anyhow::anyhow!("{route} isn't a route of the gateway's"))?;
                let file = running_or_launch(&home).await?;
                let token = gateway_token(secrets.as_ref())?;
                patchbay_gateway::bridge(&file.url(&view), &token).await?;
            }
            Command::Status => {
                let file = running(&home)?;
                let token = gateway_token(secrets.as_ref())?;
                let report: serde_json::Value = serde_json::from_str(
                    &reqwest::Client::new()
                        .get(format!("http://127.0.0.1:{}/patchbay/status", file.port))
                        .bearer_auth(token)
                        .send()
                        .await?
                        .error_for_status()?
                        .text()
                        .await?,
                )?;
                writeln!(
                    out,
                    "Gateway: {}",
                    file.url(&View::parse("").expect("the default view"))
                )?;
                for server in report["servers"].as_array().into_iter().flatten() {
                    let status: patchbay_gateway::Status =
                        serde_json::from_value(server["status"].clone())
                            .unwrap_or(patchbay_gateway::Status::Idle);
                    let tools = server["tools"].as_array().map_or(0, Vec::len);
                    writeln!(
                        out,
                        "  {} ({}): {}{}",
                        server["name"].as_str().unwrap_or_default(),
                        server["target"].as_str().unwrap_or_default(),
                        describe(&status),
                        if tools > 0 {
                            format!(", {tools} tools")
                        } else {
                            String::new()
                        }
                    )?;
                }
                for problem in report["problems"].as_array().into_iter().flatten() {
                    writeln!(out, "  problem: {}", problem.as_str().unwrap_or_default())?;
                }
            }
            Command::Secret { account } => {
                writeln!(out, "Paste the secret for {account}, then press Return:")?;
                out.flush()?;
                let mut line = String::new();
                io::stdin().lock().read_line(&mut line)?;
                anyhow::ensure!(!line.trim().is_empty(), "nothing was pasted");
                secrets.set(&account, &line)?;
                writeln!(out, "Kept {account}.")?;
                reload(&home, secrets.as_ref(), out).await;
            }
            Command::SignIn { server } => {
                let (layers, _) = home.layers();
                let resolved = resolve(&layers);
                let found = resolved
                    .servers
                    .get(&server)
                    .ok_or_else(|| anyhow::anyhow!("there's no server called {server}"))?;
                writeln!(out, "Opening {server}'s sign-in in your browser…")?;
                out.flush()?;
                oauth::sign_in(found, &secrets, |url| {
                    let _ = std::process::Command::new("open").arg(url).status();
                })
                .await?;
                writeln!(out, "Signed in to {server}.")?;
                reload(&home, secrets.as_ref(), out).await;
            }
            Command::SignOut { server } => {
                secrets.remove(&format!("oauth/{server}"));
                writeln!(out, "Forgot the sign-in to {server}.")?;
                reload(&home, secrets.as_ref(), out).await;
            }
            Command::Clients => {
                let user_home = user_home();
                for client in Client::ALL {
                    let state = if client.connected(&user_home) {
                        "connected"
                    } else if client.detected(&user_home) {
                        "not connected"
                    } else {
                        "not installed"
                    };
                    writeln!(out, "  {:<15} {:<15} {state}", client.slug(), client.name())?;
                }
            }
            Command::Connect {
                client,
                profile,
                layout,
                servers,
            } => {
                let mut choices = Choices::load(&home);
                let mut choice = choices.get(client);
                if let Some(profile) = profile {
                    choice.profile = profile;
                }
                if let Some(servers) = servers {
                    let path = home.manifest();
                    let text = std::fs::read_to_string(&path).unwrap_or_default();
                    let text = if servers == ["all"] {
                        choice.profile = "default".to_string();
                        patchbay_core::manifest::edit::remove_profile(&text, client.slug())?
                    } else {
                        choice.profile = client.slug().to_string();
                        patchbay_core::manifest::edit::set_profile(&text, client.slug(), &servers)?
                    };
                    patchbay_core::write_atomically(&path, text.as_bytes())?;
                    reload(&home, secrets.as_ref(), out).await;
                }
                if let Some(layout) = layout {
                    choice.layout = layout;
                }
                let (layers, _) = home.layers();
                let resolved = resolve(&layers);
                let servers = resolved
                    .profiles
                    .get(&choice.profile)
                    .cloned()
                    .ok_or_else(|| {
                        anyhow::anyhow!("there's no profile called {}", choice.profile)
                    })?;
                choices.set(client, choice.clone());
                choices.save(&home)?;
                let token = gateway_token(secrets.as_ref())?;
                let port =
                    GatewayFile::read(&home).map_or_else(patchbay_core::port, |file| file.port);
                let entries = client_entries(
                    &choice,
                    &servers,
                    &GatewayFile { port, pid: 0 },
                    &token,
                    &std::env::current_exe()?,
                    &patchbay_core::clients::bridge_env(),
                );
                let path = client.connect(&user_home(), &entries)?;
                let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
                writeln!(
                    out,
                    "Connected {} to Patchbay ({}) in {}. Restart it to pick the change up.",
                    client.name(),
                    names.join(", "),
                    path.display()
                )?;
            }
            Command::Disconnect { client } => {
                let path = client.disconnect(&user_home())?;
                writeln!(
                    out,
                    "Took Patchbay out of {} ({}).",
                    client.name(),
                    path.display()
                )?;
            }
            Command::Version | Command::Help | Command::Wrong(_) => {}
        }
        Ok(())
    }
}

/// The running gateway, or why there isn't one.
fn running(home: &Home) -> anyhow::Result<GatewayFile> {
    GatewayFile::read(home).ok_or_else(|| {
        anyhow::anyhow!("Patchbay's gateway isn't running: open Patchbay, or run `patchbay serve`")
    })
}

/// How long the bridge waits for Patchbay it launched to have its gateway up.
const LAUNCH_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// The running gateway, launching Patchbay in the background (no window, and out of the Dock if the settings say so)
/// when it isn't, as a client starting the bridge expects to find it.
async fn running_or_launch(home: &Home) -> anyhow::Result<GatewayFile> {
    if let Some(file) = answering(home).await {
        return Ok(file);
    }
    // Patchbay launched from Finder wouldn't see a folder moved for a trial, so it isn't launched for one.
    if std::env::var_os("PATCHBAY_HOME").is_some() {
        return running(home);
    }
    launch()?;
    let deadline = tokio::time::Instant::now() + LAUNCH_WAIT;
    while tokio::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        if let Some(file) = answering(home).await {
            return Ok(file);
        }
    }
    anyhow::bail!(
        "Patchbay didn't start its gateway within {} seconds",
        LAUNCH_WAIT.as_secs()
    )
}

/// The gateway in `gateway.toml`, if something is listening on its port: a gateway that quit leaves the file behind.
async fn answering(home: &Home) -> Option<GatewayFile> {
    let file = GatewayFile::read(home)?;
    tokio::net::TcpStream::connect(("127.0.0.1", file.port))
        .await
        .ok()
        .map(|_| file)
}

/// Opens Patchbay in the background: the bundle this binary is in, so a development build opens itself, or the
/// installed app by its identifier.
fn launch() -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let bundle = exe
        .ancestors()
        .find(|path| path.extension().is_some_and(|extension| extension == "app"));
    let mut open = std::process::Command::new("open");
    open.arg("-g");
    match bundle {
        Some(bundle) => open.arg(bundle),
        None => open.args(["-b", "tech.phin.patchbay"]),
    };
    let status = open.args(["--args", "--background"]).status()?;
    anyhow::ensure!(status.success(), "couldn't open Patchbay");
    Ok(())
}

/// Asks a running gateway to read the manifests again, so what was just kept is used.
async fn reload(home: &Home, secrets: &dyn Secrets, out: &mut impl Write) {
    let (Some(file), Ok(token)) = (GatewayFile::read(home), gateway_token(secrets)) else {
        return;
    };
    let reloaded = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{}/patchbay/reload", file.port))
        .bearer_auth(token)
        .send()
        .await
        .is_ok_and(|response| response.status().is_success());
    if reloaded {
        let _ = writeln!(out, "The gateway has it.");
    }
}

fn user_home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from)
}

fn describe(status: &patchbay_gateway::Status) -> String {
    use patchbay_gateway::Status;
    match status {
        Status::Off => "off".to_string(),
        Status::Idle => "idle".to_string(),
        Status::Starting => "starting".to_string(),
        Status::Running => "running".to_string(),
        Status::NeedsSecret { secrets } => format!(
            "needs {} (`patchbay secret {}`)",
            secrets
                .iter()
                .map(|secret| secret.prompt.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            secrets.first().map_or("", |secret| secret.account.as_str())
        ),
        Status::NeedsSignIn => "needs you to sign in (`patchbay sign-in <server>`)".to_string(),
        Status::Failed { reason } => format!("failed: {reason}"),
    }
}

#[cfg(test)]
mod tests {
    use patchbay_core::clients::{Client, Layout};

    use super::{Command, parse};

    fn output(command: Command) -> String {
        let mut out = Vec::new();
        assert_eq!(command.run(&mut out).unwrap(), 0);
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn version_prints_the_version_and_build() {
        assert_eq!(parse(&["--version"]), Some(Command::Version));
        let printed = output(Command::Version);
        assert_eq!(
            printed.trim(),
            format!("patchbay {}", patchbay_core::version())
        );
    }

    #[test]
    fn help_says_how_to_use_it() {
        assert_eq!(parse(&["-h"]), Some(Command::Help));
        assert!(output(Command::Help).starts_with("usage: patchbay"));
    }

    #[test]
    fn commands_read_their_arguments() {
        assert_eq!(
            parse(&["connect", "cursor"]),
            Some(Command::Connect {
                client: Client::Cursor,
                profile: None,
                layout: None,
                servers: None,
            })
        );
        assert_eq!(
            parse(&[
                "connect",
                "pi",
                "--layout",
                "per-server",
                "--profile",
                "small"
            ]),
            Some(Command::Connect {
                client: Client::Pi,
                profile: Some("small".to_string()),
                layout: Some(Layout::PerServer),
                servers: None,
            })
        );
        assert_eq!(
            parse(&["connect", "cursor", "--servers", "linear,github"]),
            Some(Command::Connect {
                client: Client::Cursor,
                profile: None,
                layout: None,
                servers: Some(vec!["linear".to_string(), "github".to_string()]),
            })
        );
        assert!(matches!(
            parse(&["connect", "pi", "--layout", "sideways"]),
            Some(Command::Wrong(_))
        ));
        assert_eq!(
            parse(&["bridge", "server/linear"]),
            Some(Command::Bridge {
                route: "server/linear".to_string()
            })
        );
        assert_eq!(
            parse(&["sign-in", "linear"]),
            Some(Command::SignIn {
                server: "linear".to_string()
            })
        );
        assert!(matches!(
            parse(&["connect", "emacs"]),
            Some(Command::Wrong(_))
        ));
        assert!(matches!(parse(&["secret"]), Some(Command::Wrong(_))));
    }

    #[test]
    fn anything_else_opens_the_app() {
        assert_eq!(parse::<&str>(&[]), None);
        assert_eq!(parse(&["-psn_0_12345"]), None);
        assert_eq!(parse(&["-NSDocumentRevisionsDebugMode", "YES"]), None);
        assert_eq!(parse(&["notes.txt"]), None);
    }
}
