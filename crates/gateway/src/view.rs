//! What one endpoint of the gateway exposes, and the path it's at under `/mcp`.

use std::fmt;

use patchbay_core::manifest::is_server_name;

/// The servers an endpoint gathers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Exposes {
    /// A profile's servers, each tool under its server's namespace (`linear__list_issues`).
    Profile(String),
    /// One server, its tools under their own names, so a client lists it as a server of its own.
    Server(String),
}

/// How an endpoint shows its tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Layout {
    /// Every tool, as a tool.
    Flat,
    /// Three tools, `search_tools`, `describe_tool` and `call_tool`, standing for all of them, so a client with many
    /// servers doesn't carry every schema in its context.
    Compact,
}

/// One endpoint: what it gathers and how it shows it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct View {
    pub exposes: Exposes,
    pub layout: Layout,
}

/// The word that starts a single server's path, `/mcp/server/linear`, so it can't name a profile.
pub const SERVER: &str = "server";
/// The word that ends a compact endpoint's path, `/mcp/compact`, so it can't name a profile either.
pub const COMPACT: &str = "compact";

impl View {
    pub fn profile(profile: impl Into<String>, layout: Layout) -> View {
        View {
            exposes: Exposes::Profile(profile.into()),
            layout,
        }
    }

    pub fn server(server: impl Into<String>, layout: Layout) -> View {
        View {
            exposes: Exposes::Server(server.into()),
            layout,
        }
    }

    /// The view at `route`, the path after `/mcp/` (empty for `/mcp` itself): `default`, `small/compact`,
    /// `server/linear`.
    pub fn parse(route: &str) -> Option<View> {
        let parts: Vec<&str> = route.split('/').filter(|part| !part.is_empty()).collect();
        let (parts, layout) = match parts.split_last() {
            Some((&COMPACT, rest)) => (rest, Layout::Compact),
            _ => (&parts[..], Layout::Flat),
        };
        match parts {
            [] => Some(View::profile("default", layout)),
            [SERVER, name] if is_server_name(name) => Some(View::server(*name, layout)),
            [profile] if *profile != SERVER && is_server_name(profile) => {
                Some(View::profile(*profile, layout))
            }
            _ => None,
        }
    }

    /// The path after `/mcp/` that [`View::parse`] reads back; empty for the default profile, flat.
    pub fn route(&self) -> String {
        let base = match &self.exposes {
            Exposes::Profile(profile) if profile == "default" => String::new(),
            Exposes::Profile(profile) => profile.clone(),
            Exposes::Server(server) => format!("{SERVER}/{server}"),
        };
        match (self.layout, base.is_empty()) {
            (Layout::Flat, _) => base,
            (Layout::Compact, true) => COMPACT.to_string(),
            (Layout::Compact, false) => format!("{base}/{COMPACT}"),
        }
    }
}

impl fmt::Display for View {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let route = self.route();
        if route.is_empty() {
            f.write_str("/mcp")
        } else {
            write!(f, "/mcp/{route}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Layout, View};

    #[test]
    fn every_view_is_at_a_path_that_reads_back_as_it() {
        let views = [
            View::profile("default", Layout::Flat),
            View::profile("default", Layout::Compact),
            View::profile("small", Layout::Flat),
            View::profile("small", Layout::Compact),
            View::server("linear", Layout::Flat),
            View::server("linear", Layout::Compact),
        ];
        let paths: Vec<String> = views.iter().map(ToString::to_string).collect();
        assert_eq!(
            paths,
            [
                "/mcp",
                "/mcp/compact",
                "/mcp/small",
                "/mcp/small/compact",
                "/mcp/server/linear",
                "/mcp/server/linear/compact"
            ]
        );
        for view in views {
            assert_eq!(View::parse(&view.route()), Some(view));
        }
        assert_eq!(
            View::parse("default"),
            Some(View::profile("default", Layout::Flat))
        );
    }

    #[test]
    fn a_path_that_names_nothing_is_no_view() {
        assert_eq!(View::parse("server"), None);
        assert_eq!(View::parse("server/Linear"), None);
        assert_eq!(View::parse("a/b"), None);
        assert_eq!(View::parse("server/x/y"), None);
    }
}
