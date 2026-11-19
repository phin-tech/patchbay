//! An install asked for the way `npx skills add` is: a source, then `--skill`, `--agent`, `--list`, `-y`, `-g` and
//! `--all`. A line copied from skills.sh or a README (`npx skills add vercel-labs/agent-skills --skill x`) reads the
//! same as the arguments to `patchbay skills add`.

use crate::clients::Client;

/// Why an install can't be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RequestError {
    #[error("Name a source: owner/repo, a git URL or a folder")]
    NoSource,
    #[error("Give {0} a value")]
    NoValue(String),
    #[error("Patchbay doesn't know a client called {0}")]
    NoClient(String),
    #[error("{0} isn't something skills add takes")]
    Unknown(String),
    #[error("Patchbay installs for every project, so it can't install into one ({0})")]
    Project(String),
    #[error("Patchbay links skills rather than copying them ({0})")]
    Copy(String),
    #[error("That line has an unclosed quote")]
    Quote,
}

/// What to install, and for whom.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Request {
    pub source: String,
    /// Skills by name; `*` for every one.
    pub skills: Vec<String>,
    /// The clients to link them into; every client with a skills folder that looks installed if none.
    pub clients: Vec<Client>,
    /// `--agent '*'`: every client with a skills folder, installed or not.
    pub every_client: bool,
    /// `--list`: say what's there and install nothing.
    pub list: bool,
}

impl Request {
    /// The clients to link into: those named, every one with a skills folder for `*`, or else those with a skills
    /// folder that look installed under `folders`' home.
    pub fn clients_for(&self, folders: &super::Folders) -> Vec<Client> {
        if self.every_client {
            return super::clients_with_skills();
        }
        if !self.clients.is_empty() {
            return self.clients.clone();
        }
        super::clients_with_skills()
            .into_iter()
            .filter(|client| client.detected(folders.home()))
            .collect()
    }

    /// Reads a pasted line, with or without `npx skills add` (or `bunx`, `pnpm dlx`, `skills@latest`) before it.
    pub fn parse_line(line: &str) -> Result<Request, RequestError> {
        let words = split(line)?;
        let mut words: &[String] = &words;
        fn first(words: &[String]) -> Option<&str> {
            words.first().map(String::as_str)
        }
        if matches!(first(words), Some("npx" | "bunx" | "pnpx")) {
            words = &words[1..];
        } else if matches!(first(words), Some("pnpm" | "yarn" | "bun"))
            && matches!(words.get(1).map(String::as_str), Some("dlx" | "x"))
        {
            words = &words[2..];
        }
        while matches!(first(words), Some("-y" | "--yes")) {
            words = &words[1..];
        }
        if first(words).is_some_and(|word| word == "skills" || word.starts_with("skills@")) {
            words = &words[1..];
        }
        if matches!(first(words), Some("add" | "a" | "install" | "i")) {
            words = &words[1..];
        }
        Request::parse_args(words)
    }

    /// Reads what comes after `add`.
    pub fn parse_args<S: AsRef<str>>(args: &[S]) -> Result<Request, RequestError> {
        let mut request = Request::default();
        let mut args = args.iter().map(AsRef::as_ref).peekable();
        let mut sources = Vec::new();
        while let Some(arg) = args.next() {
            let (flag, inline) = match arg.split_once('=') {
                Some((flag, value)) if flag.starts_with("--") => (flag, Some(value)),
                _ => (arg, None),
            };
            // A flag taking a list takes every word after it up to the next flag, as the CLI's does.
            let mut values = |name: &str| -> Result<Vec<String>, RequestError> {
                let mut values: Vec<String> = inline.map(str::to_string).into_iter().collect();
                if inline.is_none() {
                    while let Some(value) = args.next_if(|value| !value.starts_with('-')) {
                        values.push(value.to_string());
                    }
                }
                if values.is_empty() {
                    return Err(RequestError::NoValue(name.to_string()));
                }
                Ok(values)
            };
            match flag {
                "-s" | "--skill" => request.skills.extend(values("--skill")?),
                "-a" | "--agent" => {
                    for name in values("--agent")? {
                        if name == "*" {
                            request.every_client = true;
                        } else {
                            let client =
                                super::client_named(&name).ok_or(RequestError::NoClient(name))?;
                            if !request.clients.contains(&client) {
                                request.clients.push(client);
                            }
                        }
                    }
                }
                "-l" | "--list" => request.list = true,
                "--all" => {
                    request.skills.push("*".to_string());
                    request.every_client = true;
                }
                // Patchbay always installs for every project and never asks, so these change nothing.
                "-g" | "--global" | "-y" | "--yes" | "--full-depth" => {}
                "--copy" => return Err(RequestError::Copy(arg.to_string())),
                "--subagent" | "--project" => return Err(RequestError::Project(arg.to_string())),
                flag if flag.starts_with('-') && flag.len() > 1 => {
                    return Err(RequestError::Unknown(flag.to_string()));
                }
                source => sources.push(source.to_string()),
            }
        }
        request.source = sources.into_iter().next().ok_or(RequestError::NoSource)?;
        Ok(request)
    }
}

/// Words as a shell splits them, for a pasted line: spaces between, and quotes and backslashes keeping them
/// together.
fn split(line: &str) -> Result<Vec<String>, RequestError> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut chars = line.trim().chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => return Err(RequestError::Quote),
                    }
                }
            }
            '"' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => word.extend(chars.next()),
                        Some(c) => word.push(c),
                        None => return Err(RequestError::Quote),
                    }
                }
            }
            '\\' => {
                started = true;
                word.extend(chars.next());
            }
            c if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            // A prompt's `$ ` before a copied command.
            '$' if !started && words.is_empty() => {}
            c => {
                started = true;
                word.push(c);
            }
        }
    }
    if started {
        words.push(word);
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::{Request, RequestError};
    use crate::clients::Client;

    #[test]
    fn a_line_from_skills_sh_reads_as_its_arguments() {
        let request = Request::parse_line(
            "$ npx skills add vercel-labs/agent-skills --skill web-design-guidelines -a claude-code",
        )
        .unwrap();
        assert_eq!(request.source, "vercel-labs/agent-skills");
        assert_eq!(request.skills, ["web-design-guidelines"]);
        assert_eq!(request.clients, [Client::ClaudeCode]);
        assert_eq!(
            Request::parse_line("pnpm dlx skills@latest add o/r -g -y")
                .unwrap()
                .source,
            "o/r"
        );
        assert_eq!(Request::parse_line("o/r@one").unwrap().source, "o/r@one");
    }

    #[test]
    fn a_list_flag_takes_every_word_up_to_the_next_flag() {
        let request =
            Request::parse_line("skills add o/r -s one two --agent cursor gemini-cli -l").unwrap();
        assert_eq!(request.skills, ["one", "two"]);
        assert_eq!(request.clients, [Client::Cursor, Client::GeminiCli]);
        assert!(request.list);
        let quoted = Request::parse_line("npx skills add o/r --skill '*' --agent='*'").unwrap();
        assert_eq!(quoted.skills, ["*"]);
        assert!(quoted.every_client);
    }

    #[test]
    fn what_patchbay_cant_do_is_said() {
        assert_eq!(
            Request::parse_line("npx skills add o/r --copy"),
            Err(RequestError::Copy("--copy".into()))
        );
        assert_eq!(
            Request::parse_line("npx skills add"),
            Err(RequestError::NoSource)
        );
        assert_eq!(
            Request::parse_line("npx skills add o/r -a emacs"),
            Err(RequestError::NoClient("emacs".into()))
        );
        assert_eq!(
            Request::parse_line("skills add 'o/r"),
            Err(RequestError::Quote)
        );
    }
}
