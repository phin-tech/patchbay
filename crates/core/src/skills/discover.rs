//! Finding the skills in a repository or folder, by the `skills` CLI's rules, so a repository that works with
//! `npx skills add` works here: a `SKILL.md` at the top, then `skills/` (and its `.curated`, `.experimental` and
//! `.system`), the clients' own skills folders, the paths a `skills.sh.json` lists and a Claude plugin's, and, if
//! none of those has any, every `SKILL.md` five folders deep.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// A skill found in a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Its frontmatter's `name`.
    pub name: String,
    pub description: String,
    /// The folder holding its `SKILL.md`.
    pub path: PathBuf,
    /// The Claude plugin it's part of, if a plugin manifest groups it.
    pub plugin: Option<String>,
}

/// Folders never looked in.
const SKIP: &[&str] = &["node_modules", ".git", "dist", "build", "__pycache__"];

/// The clients' project skills folders, which a repository may keep its skills in.
const CLIENT_FOLDERS: &[&str] = &[
    ".agents/skills",
    ".claude/skills",
    ".cline/skills",
    ".codex/skills",
    ".cursor/skills",
    ".factory/skills",
    ".gemini/skills",
    ".github/skills",
    ".kilo/skills",
    ".kilocode/skills",
    ".opencode/skills",
    ".pi/skills",
];

/// The skills in `root`, or in `subpath` under it. Only what's inside `root`: a source is someone else's repository,
/// and a link in it to a folder elsewhere on this Mac would otherwise copy that folder into the store.
pub fn discover(root: &Path, subpath: Option<&str>) -> Vec<Found> {
    let base = match subpath {
        Some(subpath) => root.join(subpath),
        None => root.to_path_buf(),
    };
    let Ok(inside) = fs::canonicalize(root) else {
        return Vec::new();
    };
    if !within(&base, &inside) {
        return Vec::new();
    }
    let mut finder = Finder {
        found: Vec::new(),
        names: BTreeSet::new(),
        seen: BTreeSet::new(),
        plugins: plugin_groups(&base),
        inside,
    };
    // A skill at the top is the whole source.
    if finder.add(&base) {
        return finder.found;
    }
    let mut containers: Vec<(PathBuf, usize)> = vec![(base.clone(), 1)];
    for folder in [
        "skills",
        "skills/.curated",
        "skills/.experimental",
        "skills/.system",
    ]
    .into_iter()
    .chain(CLIENT_FOLDERS.iter().copied())
    {
        containers.push((base.join(folder), 3));
    }
    containers.extend(skills_sh_paths(&base).into_iter().map(|path| (path, 3)));
    containers.extend(plugin_paths(&base).into_iter().map(|path| (path, 1)));
    for (folder, depth) in containers {
        finder.walk(&folder, depth, 1);
    }
    if finder.found.is_empty() {
        for folder in skill_folders(&base, &finder.inside, 0) {
            finder.add(&folder);
        }
    }
    finder.found
}

struct Finder {
    found: Vec<Found>,
    names: BTreeSet<String>,
    seen: BTreeSet<PathBuf>,
    plugins: Vec<(PathBuf, String)>,
    /// The source's folder, as its real path, which nothing found may be outside.
    inside: PathBuf,
}

impl Finder {
    /// Whether `folder` holds a `SKILL.md`, adding its skill if it's one not yet seen by name.
    fn add(&mut self, folder: &Path) -> bool {
        let file = folder.join("SKILL.md");
        if !file.is_file() || !within(&file, &self.inside) {
            return false;
        }
        if !self.seen.insert(file.clone()) {
            return true;
        }
        let Some((name, description)) = fs::read_to_string(&file)
            .ok()
            .and_then(|text| frontmatter(&text))
        else {
            // The CLI skips one without a name and description too.
            return true;
        };
        if !self.names.insert(name.clone()) {
            return true;
        }
        let real = fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
        let plugin = self
            .plugins
            .iter()
            .find(|(path, _)| *path == real)
            .map(|(_, plugin)| plugin.clone());
        self.found.push(Found {
            name,
            description,
            path: folder.to_path_buf(),
            plugin,
        });
        true
    }

    fn walk(&mut self, folder: &Path, most: usize, depth: usize) {
        for child in subfolders(folder, &self.inside) {
            let name = child
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if self.add(&child) || depth >= most || SKIP.contains(&name) {
                continue;
            }
            self.walk(&child, most, depth + 1);
        }
    }
}

/// Whether `path`, links followed, is in the folder `inside` (a real path).
pub(super) fn within(path: &Path, inside: &Path) -> bool {
    fs::canonicalize(path).is_ok_and(|real| real.starts_with(inside))
}

/// The folders in `folder`, by name, following links to folders that stay `inside`.
fn subfolders(folder: &Path, inside: &Path) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && within(path, inside))
        .collect();
    folders.sort();
    folders
}

/// Every folder holding a `SKILL.md`, up to five deep.
fn skill_folders(folder: &Path, inside: &Path, depth: usize) -> Vec<PathBuf> {
    if depth > 5 {
        return Vec::new();
    }
    let mut found = Vec::new();
    if folder.join("SKILL.md").is_file() {
        found.push(folder.to_path_buf());
    }
    for child in subfolders(folder, inside) {
        let name = child
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        if !SKIP.contains(&name) {
            found.extend(skill_folders(&child, inside, depth + 1));
        }
    }
    found
}

/// A relative path a manifest gives, `./` first and never climbing out.
fn relative(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("./")?;
    let climbs = rest.split('/').any(|segment| segment == "..");
    (!climbs && !path.contains('\\')).then_some(rest)
}

/// The folders `skills.sh.json` lists under `paths`.
fn skills_sh_paths(base: &Path) -> Vec<PathBuf> {
    let Some(config) = fs::read_to_string(base.join("skills.sh.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
    else {
        return Vec::new();
    };
    config["paths"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|path| relative(path.as_str()?))
        .map(|path| path.trim_end_matches('/'))
        .filter(|path| !path.is_empty())
        .map(|path| base.join(path))
        .collect()
}

/// Each plugin a `.claude-plugin/marketplace.json` or `plugin.json` names: its folder, and the skills it lists.
fn plugins(base: &Path) -> Vec<(PathBuf, Option<String>, Vec<String>)> {
    let read = |name: &str| {
        fs::read_to_string(base.join(".claude-plugin").join(name))
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
    };
    let skills = |plugin: &serde_json::Value| -> Vec<String> {
        plugin["skills"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|skill| relative(skill.as_str()?).map(str::to_string))
            .collect()
    };
    let mut found = Vec::new();
    if let Some(marketplace) = read("marketplace.json") {
        let root = match marketplace["metadata"]["pluginRoot"].as_str() {
            Some(root) => relative(root).map(|root| base.join(root)),
            None => Some(base.to_path_buf()),
        };
        for plugin in marketplace["plugins"].as_array().into_iter().flatten() {
            let Some(root) = &root else { break };
            let folder = match &plugin["source"] {
                serde_json::Value::Null => Some(root.clone()),
                serde_json::Value::String(source) => {
                    relative(source).map(|source| root.join(source))
                }
                _ => None,
            };
            if let Some(folder) = folder {
                let name = plugin["name"].as_str().map(str::to_string);
                found.push((folder, name, skills(plugin)));
            }
        }
    }
    if let Some(plugin) = read("plugin.json") {
        let name = plugin["name"].as_str().map(str::to_string);
        found.push((base.to_path_buf(), name, skills(&plugin)));
    }
    found
}

/// Folders a plugin keeps its skills in: the ones it lists, and its `skills/`.
fn plugin_paths(base: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for (folder, _, skills) in plugins(base) {
        for skill in skills {
            if let Some(parent) = folder.join(skill).parent() {
                paths.push(parent.to_path_buf());
            }
        }
        paths.push(folder.join("skills"));
    }
    paths
}

/// Each skill folder a plugin lists, with the plugin's name.
fn plugin_groups(base: &Path) -> Vec<(PathBuf, String)> {
    plugins(base)
        .into_iter()
        .filter_map(|(folder, name, skills)| Some((folder, name?, skills)))
        .flat_map(|(folder, name, skills)| {
            skills.into_iter().map(move |skill| {
                let path = folder.join(skill);
                (fs::canonicalize(&path).unwrap_or(path), name.clone())
            })
        })
        .collect()
}

/// The `name` and `description` in a `SKILL.md`'s frontmatter, each on one line, if it has both.
pub fn frontmatter(text: &str) -> Option<(String, String)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let block: Vec<&str> = lines.take_while(|line| line.trim_end() != "---").collect();
    let clean = |value: String| {
        let value: String = value
            .chars()
            .filter(|c| !c.is_control() || *c == '\n')
            .collect();
        let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
        (!value.is_empty()).then_some(value)
    };
    let name = clean(scalar(&block, "name")?)?;
    let description = clean(scalar(&block, "description")?)?;
    Some((name, description))
}

/// A top-level key's value in YAML: plain, quoted, or a `|` or `>` block, over as many lines as it takes.
fn scalar(block: &[&str], key: &str) -> Option<String> {
    let at = block.iter().position(|line| {
        line.strip_prefix(key)
            .is_some_and(|rest| rest.trim_start().starts_with(':'))
    })?;
    let value = block[at][key.len()..].trim_start()[1..].trim();
    // The lines after it that belong to it: indented, or blank.
    let more: Vec<&str> = block[at + 1..]
        .iter()
        .take_while(|line| line.trim().is_empty() || line.starts_with([' ', '\t']))
        .copied()
        .collect();
    if value.starts_with('|') || value.starts_with('>') {
        let lines: Vec<&str> = more.iter().map(|line| line.trim()).collect();
        let joined = if value.starts_with('|') {
            lines.join("\n")
        } else {
            lines.join(" ")
        };
        return Some(joined);
    }
    if let Some(quote) = value.chars().next().filter(|c| *c == '"' || *c == '\'') {
        let mut text = value[1..].to_string();
        for line in &more {
            if ends_quote(&text, quote) {
                break;
            }
            text.push(' ');
            text.push_str(line.trim());
        }
        let text = text.trim_end();
        let text = text.strip_suffix(quote).unwrap_or(text);
        return Some(if quote == '"' {
            unescape(text)
        } else {
            text.replace("''", "'")
        });
    }
    let mut text = strip_comment(value).to_string();
    for line in more {
        text.push(' ');
        text.push_str(line.trim());
    }
    Some(text)
}

/// Whether a quoted value has reached its closing quote.
fn ends_quote(text: &str, quote: char) -> bool {
    let text = text.trim_end();
    if !text.ends_with(quote) {
        return false;
    }
    // In single quotes a quote is written twice, so an odd run of them at the end closes it.
    if quote == '\'' {
        return !(text.len() - text.trim_end_matches('\'').len()).is_multiple_of(2);
    }
    let backslashes =
        text[..text.len() - 1].len() - text[..text.len() - 1].trim_end_matches('\\').len();
    backslashes.is_multiple_of(2)
}

fn unescape(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// A plain value without a ` #` comment after it.
fn strip_comment(value: &str) -> &str {
    value
        .split_once(" #")
        .map_or(value, |(value, _)| value)
        .trim()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::{discover, frontmatter};

    fn folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("patchbay-discover-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn skill(root: &Path, at: &str, name: &str) {
        fs::create_dir_all(root.join(at)).unwrap();
        fs::write(
            root.join(at).join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Does {name}\n---\n# {name}\n"),
        )
        .unwrap();
    }

    fn names(root: &Path, subpath: Option<&str>) -> Vec<String> {
        discover(root, subpath)
            .into_iter()
            .map(|found| found.name)
            .collect()
    }

    #[test]
    fn a_skill_at_the_top_is_the_only_one() {
        let root = folder("top");
        skill(&root, "", "top");
        skill(&root, "skills/other", "other");
        assert_eq!(names(&root, None), ["top"]);
    }

    #[test]
    fn skills_are_found_in_skills_and_nested_categories() {
        let root = folder("nested");
        skill(&root, "skills/a", "a");
        skill(&root, "skills/engineering/b", "b");
        skill(&root, ".claude/skills/c", "c");
        skill(&root, "node_modules/x/skills/hidden", "hidden");
        assert_eq!(names(&root, None), ["a", "b", "c"]);
    }

    #[test]
    fn a_subpath_narrows_where_to_look() {
        let root = folder("subpath");
        skill(&root, "skills/a", "a");
        skill(&root, "other/b", "b");
        assert_eq!(names(&root, Some("other")), ["b"]);
    }

    #[test]
    fn links_out_of_the_source_find_nothing_outside_it() {
        let outside = folder("outside");
        skill(&outside, "secret", "secret");
        let root = folder("escape");
        skill(&root, "skills/a", "a");
        fs::create_dir_all(root.join("skills")).unwrap();
        std::os::unix::fs::symlink(outside.join("secret"), root.join("skills/secret")).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("out")).unwrap();
        // Linked inside the source is fine: it's the source's own folder by another name.
        std::os::unix::fs::symlink(root.join("skills/a"), root.join("skills/also")).unwrap();
        assert_eq!(names(&root, None), ["a"]);
        assert_eq!(names(&root, Some("out")), Vec::<String>::new());
        let bare = folder("escape-bare");
        std::os::unix::fs::symlink(&outside, bare.join("out")).unwrap();
        assert_eq!(names(&bare, None), Vec::<String>::new());
    }

    #[test]
    fn with_nothing_in_the_usual_places_every_skill_md_counts() {
        let root = folder("deep");
        skill(&root, "one/two/three/d", "d");
        assert_eq!(names(&root, None), ["d"]);
    }

    #[test]
    fn a_plugin_manifest_names_its_skills_and_groups_them() {
        let root = folder("plugin");
        skill(&root, "plugins/p/skills/e", "e");
        fs::create_dir_all(root.join(".claude-plugin")).unwrap();
        fs::write(
            root.join(".claude-plugin/marketplace.json"),
            r#"{"plugins":[{"name":"p","source":"./plugins/p","skills":["./skills/e"]}]}"#,
        )
        .unwrap();
        let found = discover(&root, None);
        assert_eq!(found[0].name, "e");
        assert_eq!(found[0].plugin.as_deref(), Some("p"));
    }

    #[test]
    fn frontmatter_reads_plain_quoted_and_folded_values() {
        let read = |text: &str| frontmatter(text).unwrap();
        assert_eq!(
            read("---\nname: a\ndescription: Does a: well # no\n---\n"),
            ("a".into(), "Does a: well".into())
        );
        assert_eq!(
            read("---\nname: \"b\"\ndescription: 'It''s b'\n---\n"),
            ("b".into(), "It's b".into())
        );
        assert_eq!(
            read("---\nname: c\ndescription: >-\n  Folded over\n  two lines.\nlicense: MIT\n---\n"),
            ("c".into(), "Folded over two lines.".into())
        );
        assert_eq!(
            read("---\nname: d\ndescription: \"Quoted\n  over lines\"\n---\n"),
            ("d".into(), "Quoted over lines".into())
        );
        assert_eq!(frontmatter("# no frontmatter"), None);
        assert_eq!(frontmatter("---\nname: e\n---\n"), None);
    }
}
