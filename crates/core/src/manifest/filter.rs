//! Which of a server's tools a client sees.

use super::*;

/// The `allow` and `deny` patterns of a server's `tools`, where `*` matches any run of characters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolFilter {
    allow: Option<Vec<String>>,
    deny: Vec<String>,
}

impl ToolFilter {
    pub fn new(spec: &ToolsSpec) -> ToolFilter {
        ToolFilter {
            allow: spec.allow.clone(),
            deny: spec.deny.clone(),
        }
    }

    /// Whether the tool named `tool` (its own name, without the namespace) is exposed.
    pub fn allows(&self, tool: &str) -> bool {
        let allowed = self
            .allow
            .as_ref()
            .is_none_or(|allow| allow.iter().any(|pattern| matches(pattern, tool)));
        allowed && !self.deny.iter().any(|pattern| matches(pattern, tool))
    }
}

/// Whether `text` matches `pattern`, `*` standing for any run of characters, none included.
fn matches(pattern: &str, text: &str) -> bool {
    let mut pieces = pattern.split('*');
    let first = pieces.next().unwrap_or_default();
    let Some(mut rest) = text.strip_prefix(first) else {
        return false;
    };
    let pieces: Vec<&str> = pieces.collect();
    let Some((last, middle)) = pieces.split_last() else {
        return rest.is_empty();
    };
    for piece in middle {
        match rest.find(piece) {
            Some(at) => rest = &rest[at + piece.len()..],
            None => return false,
        }
    }
    rest.len() >= last.len() && rest.ends_with(last)
}

#[cfg(test)]
mod tests {
    use super::{ToolFilter, ToolsSpec, matches};

    #[test]
    fn a_star_matches_any_run_of_characters() {
        assert!(matches("delete_*", "delete_issue"));
        assert!(matches("*_issue", "create_issue"));
        assert!(matches("*", "anything"));
        assert!(matches("a*b*c", "aXbYc"));
        assert!(matches("list", "list"));
        assert!(!matches("list", "lists"));
        assert!(!matches("a*a", "a"));
        assert!(!matches("delete_*", "create_issue"));
    }

    #[test]
    fn deny_wins_over_allow() {
        let filter = ToolFilter::new(&ToolsSpec {
            allow: Some(vec!["*_issue".to_string()]),
            deny: vec!["delete_*".to_string()],
        });
        assert!(filter.allows("create_issue"));
        assert!(!filter.allows("delete_issue"));
        assert!(!filter.allows("list_repos"));
    }

    #[test]
    fn with_no_patterns_every_tool_is_exposed() {
        assert!(ToolFilter::default().allows("anything"));
    }
}
