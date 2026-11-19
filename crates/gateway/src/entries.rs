//! The entries a client's [`Choice`] puts in its config.

use std::path::Path;

use patchbay_core::clients::{Choice, Entry, Layout as ClientLayout};

use super::*;

/// The entries `choice` gives a client of the gateway in `file`: one, or one for each of `servers` (the choice's
/// profile's servers).
pub fn client_entries(
    choice: &Choice,
    servers: &[String],
    file: &GatewayFile,
    token: &str,
    binary: &Path,
    env: &[(String, String)],
) -> Vec<Entry> {
    let entry = |name: String, view: View| {
        let route = view.route();
        Entry {
            name,
            url: file.url(&view),
            route: if route.is_empty() {
                "default".to_string()
            } else {
                route
            },
            token: token.to_string(),
            binary: binary.to_path_buf(),
            env: env.to_vec(),
        }
    };
    match choice.layout {
        ClientLayout::OneEntry => vec![entry(
            "patchbay".to_string(),
            View::profile(&choice.profile, Layout::Flat),
        )],
        ClientLayout::Compact => vec![entry(
            "patchbay".to_string(),
            View::profile(&choice.profile, Layout::Compact),
        )],
        ClientLayout::PerServer => servers
            .iter()
            .map(|server| {
                entry(
                    format!("patchbay-{server}"),
                    View::server(server, Layout::Flat),
                )
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use patchbay_core::clients::{Choice, Layout};

    use super::{GatewayFile, client_entries};

    #[test]
    fn each_layout_gives_its_entries() {
        let file = GatewayFile { port: 7357, pid: 0 };
        let servers = ["linear".to_string(), "github".to_string()];
        let entries = |layout, profile: &str| {
            client_entries(
                &Choice {
                    profile: profile.to_string(),
                    layout,
                    ..Choice::default()
                },
                &servers,
                &file,
                "t",
                Path::new("/p"),
                &[],
            )
            .into_iter()
            .map(|entry| format!("{} {} {}", entry.name, entry.url, entry.route))
            .collect::<Vec<_>>()
        };
        assert_eq!(
            entries(Layout::OneEntry, "default"),
            ["patchbay http://127.0.0.1:7357/mcp default"]
        );
        assert_eq!(
            entries(Layout::Compact, "small"),
            ["patchbay http://127.0.0.1:7357/mcp/small/compact small/compact"]
        );
        assert_eq!(
            entries(Layout::PerServer, "default"),
            [
                "patchbay-linear http://127.0.0.1:7357/mcp/server/linear server/linear",
                "patchbay-github http://127.0.0.1:7357/mcp/server/github server/github"
            ]
        );
    }
}
