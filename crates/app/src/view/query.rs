//! What a place's filter field says, read as Typecase's filter bar reads it: words to search for, `-word` to leave
//! out and `"a phrase"` kept together, and over the servers, `server:`, `team:` and `is:` too, as
//! `server:github is:write is:off`.

use patchbay_gateway::{ServerStatus, Status, ToolStatus};
use typecase::{Filters, Kind, Search};

/// What `is:` asks of a server, or of its tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Is {
    /// A tool clients are given.
    On,
    /// A tool turned off, or a server turned off in a manifest.
    Off,
    /// A tool that may change something, which is any the server doesn't say only reads.
    Write,
    /// A tool the server says only reads.
    Read,
    Running,
    Idle,
    /// Waiting for a key or a sign-in.
    Waiting,
    Failing,
}

impl Is {
    fn read(value: &str) -> Option<Is> {
        Some(match value.to_lowercase().as_str() {
            "on" => Is::On,
            "off" => Is::Off,
            "write" | "writes" => Is::Write,
            "read" | "reads" | "read-only" => Is::Read,
            "running" => Is::Running,
            "idle" => Is::Idle,
            "waiting" => Is::Waiting,
            "failing" | "failed" => Is::Failing,
            _ => return None,
        })
    }

    /// Whether it's asked of each tool rather than of the server.
    fn of_tools(self) -> bool {
        matches!(self, Is::On | Is::Off | Is::Write | Is::Read)
    }

    fn holds_for_tool(self, tool: &ToolStatus) -> bool {
        match self {
            Is::On => tool.exposed,
            Is::Off => !tool.exposed,
            Is::Write => !tool.read_only,
            Is::Read => tool.read_only,
            Is::Running | Is::Idle | Is::Waiting | Is::Failing => true,
        }
    }

    fn holds_for_server(self, status: &Status) -> bool {
        match self {
            Is::Running => matches!(status, Status::Running),
            Is::Idle => matches!(status, Status::Idle | Status::Starting),
            Is::Waiting => matches!(status, Status::NeedsSecret { .. } | Status::NeedsSignIn),
            Is::Failing => matches!(status, Status::Failed { .. }),
            Is::Off => matches!(status, Status::Off),
            Is::On | Is::Write | Is::Read => true,
        }
    }
}

/// A filter field's text, read.
pub(super) struct Query {
    search: Search,
    servers: Vec<String>,
    no_servers: Vec<String>,
    teams: Vec<String>,
    no_teams: Vec<String>,
    is: Vec<Is>,
    is_not: Vec<Is>,
    /// An `is:` this doesn't know, which nothing matches, so a typo shows nothing rather than everything.
    unknown: bool,
}

impl Query {
    /// The text of a list without qualifiers: the clients or the skills.
    pub(super) fn plain(text: &str) -> Query {
        Query::read(Filters::from_query(text, &[]))
    }

    /// The servers list's text, with `server:`, `team:` and `is:`.
    pub(super) fn servers(text: &str) -> Query {
        let kinds = [
            Kind::new("server", "Server").open(),
            Kind::new("team", "Team").open(),
            Kind::new("is", "Is").open(),
        ];
        Query::read(Filters::from_query(text, &kinds))
    }

    fn read(filters: Filters) -> Query {
        let lower = |values: Vec<&str>| values.into_iter().map(str::to_lowercase).collect();
        let is: Vec<Option<Is>> = filters.values("is").map(Is::read).collect();
        let is_not: Vec<Option<Is>> = filters.excluded("is").map(Is::read).collect();
        Query {
            search: filters.search(),
            servers: lower(filters.values("server").collect()),
            no_servers: lower(filters.excluded("server").collect()),
            teams: lower(filters.values("team").collect()),
            no_teams: lower(filters.excluded("team").collect()),
            unknown: is.iter().chain(&is_not).any(Option::is_none),
            is: is.into_iter().flatten().collect(),
            is_not: is_not.into_iter().flatten().collect(),
        }
    }

    /// Whether every word searched for is in one of `fields`, and none left out is, ignoring case: `aws cdk` matches
    /// `aws-cdk`.
    pub(super) fn matches(&self, fields: &[&str]) -> bool {
        self.search.matches(&fields.join("\n"))
    }

    /// Whether the qualifiers ask anything of a server's tools, so its tools are narrowed to those that match.
    pub(super) fn narrows_tools(&self) -> bool {
        self.is.iter().chain(&self.is_not).any(|is| is.of_tools())
    }

    /// Whether `tool` is one the `is:` qualifiers ask for.
    pub(super) fn keeps_tool(&self, tool: &ToolStatus) -> bool {
        self.is.iter().all(|is| is.holds_for_tool(tool))
            && !self
                .is_not
                .iter()
                .any(|is| is.of_tools() && is.holds_for_tool(tool))
    }

    /// Whether `server` is one the text asks for: its name and command or URL searched, then each qualifier. Asked
    /// of its tools, the qualifiers keep a server with one tool that matches, or one turned off for `is:off`.
    pub(super) fn keeps_server(&self, server: &ServerStatus) -> bool {
        if self.unknown || !self.matches(&[&server.name, &server.target]) {
            return false;
        }
        let name = server.name.to_lowercase();
        if !self.servers.is_empty() && !self.servers.iter().any(|wanted| name.contains(wanted)) {
            return false;
        }
        if self
            .no_servers
            .iter()
            .any(|unwanted| name.contains(unwanted))
        {
            return false;
        }
        let team = server
            .team
            .as_deref()
            .map_or_else(|| "me".to_string(), str::to_lowercase);
        if !self.teams.is_empty() && !self.teams.contains(&team) {
            return false;
        }
        if self.no_teams.contains(&team) {
            return false;
        }
        let of_server = |is: &&Is| !is.of_tools() || **is == Is::Off;
        let turned_off = matches!(server.status, Status::Off);
        if !self
            .is
            .iter()
            .filter(|is| !is.of_tools())
            .all(|is| is.holds_for_server(&server.status))
            || self
                .is_not
                .iter()
                .filter(of_server)
                .any(|is| is.holds_for_server(&server.status))
        {
            return false;
        }
        if !self.narrows_tools() {
            return true;
        }
        let only_off = self.is_not.is_empty()
            && self
                .is
                .iter()
                .filter(|is| is.of_tools())
                .all(|is| *is == Is::Off);
        (turned_off && only_off) || server.tools.iter().any(|tool| self.keeps_tool(tool))
    }
}

#[cfg(test)]
mod tests {
    use super::Query;
    use patchbay_gateway::{ServerStatus, Status, ToolStatus};

    fn tool(name: &str, exposed: bool, read_only: bool) -> ToolStatus {
        ToolStatus {
            name: name.to_string(),
            exposed,
            read_only,
        }
    }

    fn server(
        name: &str,
        status: Status,
        tools: Vec<ToolStatus>,
        team: Option<&str>,
    ) -> ServerStatus {
        ServerStatus {
            name: name.to_string(),
            namespace: name.to_string(),
            target: format!("https://{name}.example/mcp"),
            oauth: false,
            status,
            tools,
            secrets: Vec::new(),
            team: team.map(str::to_string),
        }
    }

    fn kept(text: &str, servers: &[ServerStatus]) -> Vec<String> {
        let query = Query::servers(text);
        servers
            .iter()
            .filter(|server| query.keeps_server(server))
            .map(|server| server.name.clone())
            .collect()
    }

    fn servers() -> Vec<ServerStatus> {
        vec![
            server(
                "github",
                Status::Running,
                vec![
                    tool("create_issue", false, false),
                    tool("get_issue", true, true),
                ],
                None,
            ),
            server(
                "linear",
                Status::NeedsSignIn,
                vec![tool("list_issues", true, true)],
                Some("phin-tech"),
            ),
            server("notion", Status::Off, Vec::new(), None),
        ]
    }

    #[test]
    fn words_search_the_name_and_target_and_a_dash_leaves_one_out() {
        assert_eq!(kept("git", &servers()), ["github"]);
        assert_eq!(kept("example -linear", &servers()), ["github", "notion"]);
    }

    #[test]
    fn is_write_and_is_off_keep_servers_with_a_tool_that_writes_and_is_turned_off() {
        assert_eq!(kept("is:write is:off", &servers()), ["github"]);
        let query = Query::servers("server:github is:write is:off");
        let github = &servers()[0];
        let tools: Vec<&str> = github
            .tools
            .iter()
            .filter(|tool| query.keeps_tool(tool))
            .map(|tool| tool.name.as_str())
            .collect();
        assert_eq!(tools, ["create_issue"]);
    }

    #[test]
    fn is_off_alone_keeps_a_server_turned_off_too() {
        assert_eq!(kept("is:off", &servers()), ["github", "notion"]);
        assert_eq!(kept("-is:off", &servers()), ["github", "linear"]);
    }

    #[test]
    fn a_servers_state_and_team_narrow_it() {
        assert_eq!(kept("is:waiting", &servers()), ["linear"]);
        assert_eq!(kept("team:phin-tech", &servers()), ["linear"]);
        assert_eq!(kept("team:me -is:running", &servers()), ["notion"]);
        assert_eq!(kept("is:read", &servers()), ["github", "linear"]);
    }

    #[test]
    fn an_is_it_doesnt_know_matches_nothing() {
        assert_eq!(kept("is:sparkly", &servers()), Vec::<String>::new());
    }

    #[test]
    fn a_plain_list_searches_phrases_and_leaves_words_out() {
        let query = Query::plain("\"claude code\" -desktop");
        assert!(query.matches(&["Claude Code", "claude-code"]));
        assert!(!query.matches(&["Claude Desktop", "claude"]));
    }
}
