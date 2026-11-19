//! Writing files so that a crash or a full disk midway never leaves a half-written one behind.

use std::fs;
use std::io;
use std::path::Path;

/// Writes `contents` to a file beside `path` and renames it over `path`, making the folder if need be. A symlink is
/// followed first, so a file kept in a dotfiles repository stays linked (an idea from herdr-app, Apache-2.0).
pub fn write_atomically(path: &Path, contents: &[u8]) -> io::Result<()> {
    let target = fs::canonicalize(path).ok();
    let path = target.as_deref().unwrap_or(path);
    let folder = path
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty());
    if let Some(folder) = folder {
        fs::create_dir_all(folder)?;
    }
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("no file name"))?;
    let mut temporary = path.to_path_buf();
    temporary.set_file_name(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id()
    ));
    fs::write(&temporary, contents)?;
    fs::rename(&temporary, path).inspect_err(|_| {
        let _ = fs::remove_file(&temporary);
    })
}

#[cfg(test)]
mod tests {
    use super::write_atomically;
    use std::fs;

    #[test]
    fn writing_replaces_the_file_and_leaves_nothing_else() {
        let folder = std::env::temp_dir().join(format!("files-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        let path = folder.join("deep/note.txt");
        write_atomically(&path, b"one").unwrap();
        write_atomically(&path, b"two").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        let names: Vec<_> = fs::read_dir(path.parent().unwrap()).unwrap().collect();
        assert_eq!(names.len(), 1);
    }

    #[test]
    fn writing_through_a_symlink_keeps_the_link() {
        let folder = std::env::temp_dir().join(format!("files-link-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        let real = folder.join("real.toml");
        let link = folder.join("link.toml");
        fs::write(&real, "one").unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        write_atomically(&link, b"two").unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&real).unwrap(), "two");
    }
}
