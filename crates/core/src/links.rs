//! The URLs macOS hands the app: files opened from Finder or the Dock (`file:///…`), and links to the app's own scheme.

use std::path::PathBuf;

/// The path a `file://` URL names, with its percent-escapes undone.
pub fn path_from_file_url(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    // `file://localhost/…` is the same as `file:///…`.
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    Some(PathBuf::from(percent_decode(rest)?))
}

/// What a link to `scheme` asks for: `myapp://new` and `myapp:new` are both `new`.
pub fn link_target(url: &str, scheme: &str) -> Option<String> {
    let rest = url.strip_prefix(scheme)?.strip_prefix(':')?;
    let rest = rest.trim_start_matches('/').trim_end_matches('/');
    (!rest.is_empty()).then(|| percent_decode(rest)).flatten()
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::{link_target, path_from_file_url};
    use std::path::PathBuf;

    #[test]
    fn a_file_url_becomes_its_path() {
        assert_eq!(
            path_from_file_url("file:///Users/sam/My%20Notes/caf%C3%A9.txt"),
            Some(PathBuf::from("/Users/sam/My Notes/café.txt"))
        );
        assert_eq!(
            path_from_file_url("file://localhost/tmp/a"),
            Some(PathBuf::from("/tmp/a"))
        );
        assert_eq!(path_from_file_url("https://example.com/a"), None);
        assert_eq!(path_from_file_url("file:///bad%zz"), None);
    }

    #[test]
    fn a_link_says_what_it_asks_for() {
        assert_eq!(link_target("myapp://new", "myapp").as_deref(), Some("new"));
        assert_eq!(
            link_target("myapp:settings/", "myapp").as_deref(),
            Some("settings")
        );
        assert_eq!(link_target("myapp://", "myapp"), None);
        assert_eq!(link_target("other://new", "myapp"), None);
        assert_eq!(link_target("myappx://new", "myapp"), None);
    }
}
