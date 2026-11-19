//! The lock the `skills` CLI keeps for skills installed for every project (`~/.agents/.skill-lock.json`, version 3),
//! which Patchbay shares: what it installs is recorded as the CLI records it, so `npx skills list`, `check` and
//! `update` see it too, and the other way round. Keys Patchbay doesn't use (`dismissed`, `lastSelectedAgents`, and
//! any added later) are kept as they were.

use std::path::Path;

use serde_json::{Map, Value};

use super::SkillsError;

/// The lock's version, which the CLI reads as empty if it's older.
const VERSION: u64 = 3;

/// One skill in the lock.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    /// `owner/repo` for GitHub, otherwise the URL or folder.
    pub source: String,
    /// `github`, `gitlab`, `git`, `local`, or one of the CLI's others.
    pub source_type: String,
    pub source_url: String,
    pub reference: Option<String>,
    /// The `SKILL.md` within the source: `skills/<name>/SKILL.md`.
    pub skill_path: Option<String>,
    /// The folder's git tree hash for a GitHub source, the CLI's content hash for any other.
    pub folder_hash: String,
    pub plugin: Option<String>,
    pub installed_at: Option<String>,
    pub updated_at: Option<String>,
}

/// The lock, as read, with whatever else is in it.
#[derive(Debug, Clone, PartialEq)]
pub struct Lock {
    root: Map<String, Value>,
    /// Whether a lock was there but couldn't be read, so writing would lose what's in it.
    unreadable: bool,
}

impl Lock {
    /// Reads the lock at `path`; one that's missing, broken or older than version 3 reads as empty, as the CLI
    /// reads it.
    pub fn read(path: &Path) -> Lock {
        let text = std::fs::read_to_string(path).ok();
        let present = text.is_some();
        let root = text
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|value| match value {
                Value::Object(root) => Some(root),
                _ => None,
            })
            .filter(|root| {
                root.get("version")
                    .and_then(Value::as_u64)
                    .is_some_and(|version| version >= VERSION)
                    && root.get("skills").is_some_and(Value::is_object)
            });
        Lock {
            unreadable: present && root.is_none(),
            root: root.unwrap_or_else(|| {
                let mut root = Map::new();
                root.insert("version".into(), VERSION.into());
                root.insert("skills".into(), Value::Object(Map::new()));
                root.insert("dismissed".into(), Value::Object(Map::new()));
                root
            }),
        }
    }

    /// Whether there's a lock that can't be read: broken, or from an older CLI. The CLI would start it again
    /// empty; Patchbay leaves it for the person to see to.
    pub fn unreadable(&self) -> bool {
        self.unreadable
    }

    fn skills(&self) -> &Map<String, Value> {
        self.root["skills"]
            .as_object()
            .expect("skills is an object")
    }

    fn skills_mut(&mut self) -> &mut Map<String, Value> {
        self.root
            .get_mut("skills")
            .and_then(Value::as_object_mut)
            .expect("skills is an object")
    }

    /// Every skill's name, as the lock keys it (its frontmatter `name`).
    pub fn names(&self) -> Vec<String> {
        self.skills().keys().cloned().collect()
    }

    pub fn get(&self, name: &str) -> Option<Entry> {
        let entry = self.skills().get(name)?;
        let text = |key: &str| entry.get(key).and_then(Value::as_str).map(str::to_string);
        Some(Entry {
            source: text("source").unwrap_or_default(),
            source_type: text("sourceType").unwrap_or_default(),
            source_url: text("sourceUrl").unwrap_or_default(),
            reference: text("ref"),
            skill_path: text("skillPath"),
            folder_hash: text("skillFolderHash").unwrap_or_default(),
            plugin: text("pluginName"),
            installed_at: text("installedAt"),
            updated_at: text("updatedAt"),
        })
    }

    /// The name the lock keys a skill by, for a skill installed in the folder `folder`: the CLI names the folder
    /// after the skill, so they match once both are made folder names.
    pub fn name_for_folder(&self, folder: &str) -> Option<String> {
        self.skills()
            .keys()
            .find(|name| super::folder_name(name) == folder)
            .cloned()
    }

    /// Records `entry` for `name`, keeping when it was first installed and any fields the CLI added.
    pub fn set(&mut self, name: &str, entry: &Entry, now: &super::Stamp) {
        let skills = self.skills_mut();
        let mut fields = match skills.get(name) {
            Some(Value::Object(old)) => old.clone(),
            _ => Map::new(),
        };
        let installed = fields
            .get("installedAt")
            .and_then(Value::as_str)
            .map_or_else(|| now.as_str().to_string(), str::to_string);
        let mut put = |key: &str, value: Option<&str>| match value {
            Some(value) => {
                fields.insert(key.into(), Value::String(value.to_string()));
            }
            None => {
                fields.remove(key);
            }
        };
        put("source", Some(&entry.source));
        put("sourceType", Some(&entry.source_type));
        put("sourceUrl", Some(&entry.source_url));
        put("ref", entry.reference.as_deref());
        put("skillPath", entry.skill_path.as_deref());
        put("skillFolderHash", Some(&entry.folder_hash));
        put("pluginName", entry.plugin.as_deref());
        put("installedAt", Some(&installed));
        put("updatedAt", Some(now.as_str()));
        skills.insert(name.to_string(), Value::Object(fields));
    }

    pub fn remove(&mut self, name: &str) -> bool {
        self.skills_mut().remove(name).is_some()
    }

    /// Writes the lock to `path`, all at once.
    pub fn write(&self, path: &Path) -> Result<(), SkillsError> {
        let text = serde_json::to_string_pretty(&Value::Object(self.root.clone()))
            .expect("a lock is always JSON");
        crate::write_atomically(path, text.as_bytes()).map_err(|error| SkillsError::Files {
            kind: error.kind(),
            message: format!("{}: {error}", path.display()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Entry, Lock};

    #[test]
    fn writing_an_entry_keeps_the_rest_of_the_lock() {
        let folder = std::env::temp_dir().join(format!("patchbay-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let path = folder.join(".skill-lock.json");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            &path,
            r#"{"version":3,"skills":{"old":{"source":"o/r","sourceType":"github","sourceUrl":"https://github.com/o/r.git","skillPath":"skills/old/SKILL.md","skillFolderHash":"abc","installedAt":"then","updatedAt":"then","future":1}},"dismissed":{"findSkillsPrompt":true},"lastSelectedAgents":["codex"]}"#,
        )
        .unwrap();
        let mut lock = Lock::read(&path);
        let mut old = lock.get("old").unwrap();
        old.folder_hash = "def".into();
        lock.set("old", &old, &"now".into());
        lock.set(
            "new",
            &Entry {
                source: "o/r".into(),
                source_type: "github".into(),
                source_url: "https://github.com/o/r.git".into(),
                skill_path: Some("skills/new/SKILL.md".into()),
                folder_hash: "123".into(),
                ..Entry::default()
            },
            &"now".into(),
        );
        lock.write(&path).unwrap();
        insta::assert_snapshot!(std::fs::read_to_string(&path).unwrap());
    }

    #[test]
    fn an_older_lock_reads_as_empty_as_the_cli_reads_it() {
        let folder = std::env::temp_dir().join(format!("patchbay-lock-old-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join(".skill-lock.json");
        std::fs::write(&path, r#"{"version":2,"skills":{"x":{}}}"#).unwrap();
        assert!(Lock::read(&path).names().is_empty());
    }
}
