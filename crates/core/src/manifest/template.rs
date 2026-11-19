//! A string from a manifest that may name secrets, so a manifest can be shared without holding any.

use std::fmt;

/// A manifest string with secret references in it: `secret:github-token` on its own, or
/// `Bearer ${secret:linear-key}` inside a longer one. [`Template::render`] fills them in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    parts: Vec<Part>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Text(String),
    Secret(String),
}

/// Why a manifest string can't be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TemplateError {
    #[error("`{0}` opens a `${{secret:…}}` and never closes it")]
    Unclosed(String),
    #[error(
        "`{0}` isn't a secret's name: use lowercase letters, digits, hyphens, underscores and dots"
    )]
    BadName(String),
}

/// A secret a [`Template`] needs and couldn't be given.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("no value for the secret `{0}`")]
pub struct MissingSecret(pub String);

const WHOLE: &str = "secret:";
const OPEN: &str = "${secret:";

/// The string as written, with each secret as its reference rather than its value, so it can be shown.
impl fmt::Display for Template {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for part in &self.parts {
            match part {
                Part::Text(text) => f.write_str(text)?,
                Part::Secret(name) => write!(f, "{OPEN}{name}}}")?,
            }
        }
        Ok(())
    }
}

impl Template {
    pub fn parse(text: &str) -> Result<Template, TemplateError> {
        if let Some(name) = text.strip_prefix(WHOLE) {
            check_name(name)?;
            return Ok(Template {
                parts: vec![Part::Secret(name.to_string())],
            });
        }
        let mut parts = Vec::new();
        let mut rest = text;
        while let Some(start) = rest.find(OPEN) {
            if start > 0 {
                parts.push(Part::Text(rest[..start].to_string()));
            }
            let after = &rest[start + OPEN.len()..];
            let end = after
                .find('}')
                .ok_or_else(|| TemplateError::Unclosed(text.to_string()))?;
            let name = &after[..end];
            check_name(name)?;
            parts.push(Part::Secret(name.to_string()));
            rest = &after[end + 1..];
        }
        if !rest.is_empty() {
            parts.push(Part::Text(rest.to_string()));
        }
        Ok(Template { parts })
    }

    /// The names of the secrets it needs, in the order they appear.
    pub fn secrets(&self) -> impl Iterator<Item = &str> {
        self.parts.iter().filter_map(|part| match part {
            Part::Secret(name) => Some(name.as_str()),
            Part::Text(_) => None,
        })
    }

    /// The string with each secret filled in by `lookup`, or the first one it couldn't fill.
    pub fn render(
        &self,
        mut lookup: impl FnMut(&str) -> Option<String>,
    ) -> Result<String, MissingSecret> {
        let mut out = String::new();
        for part in &self.parts {
            match part {
                Part::Text(text) => out.push_str(text),
                Part::Secret(name) => {
                    out.push_str(&lookup(name).ok_or_else(|| MissingSecret(name.clone()))?);
                }
            }
        }
        Ok(out)
    }
}

/// Whether `name` can name a secret: lowercase letters, digits, `-`, `_` and `.`.
pub(crate) fn is_secret_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'))
}

fn check_name(name: &str) -> Result<(), TemplateError> {
    if is_secret_name(name) {
        Ok(())
    } else {
        Err(TemplateError::BadName(name.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::{MissingSecret, Template, TemplateError};

    fn lookup(name: &str) -> Option<String> {
        (name == "token").then(|| "abc".to_string())
    }

    #[test]
    fn plain_text_renders_as_itself() {
        let template = Template::parse("--verbose").unwrap();
        assert_eq!(template.secrets().count(), 0);
        assert_eq!(template.render(lookup).unwrap(), "--verbose");
    }

    #[test]
    fn a_whole_reference_is_replaced_by_the_secret() {
        let template = Template::parse("secret:token").unwrap();
        assert_eq!(template.secrets().collect::<Vec<_>>(), ["token"]);
        assert_eq!(template.render(lookup).unwrap(), "abc");
    }

    #[test]
    fn references_inside_a_string_are_replaced_in_place() {
        let template = Template::parse("Bearer ${secret:token} (${secret:token})").unwrap();
        assert_eq!(template.render(lookup).unwrap(), "Bearer abc (abc)");
    }

    #[test]
    fn a_secret_with_no_value_is_named() {
        let template = Template::parse("x=${secret:other}").unwrap();
        assert_eq!(
            template.render(lookup),
            Err(MissingSecret("other".to_string()))
        );
    }

    #[test]
    fn a_broken_reference_says_what_is_wrong() {
        assert_eq!(
            Template::parse("${secret:token"),
            Err(TemplateError::Unclosed("${secret:token".to_string()))
        );
        assert_eq!(
            Template::parse("secret:Token"),
            Err(TemplateError::BadName("Token".to_string()))
        );
        assert_eq!(
            Template::parse("${secret:}"),
            Err(TemplateError::BadName(String::new()))
        );
    }
}
