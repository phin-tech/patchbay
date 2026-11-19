//! A skill folder's hash, as the `skills` CLI computes it, so either tool can tell whether a skill the other
//! installed has changed. For a GitHub source the lock holds the folder's git tree hash instead; see
//! [`super::fetch::tree_hash`].

use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};

/// Files and folders the CLI never copies, and so never hashes.
pub(super) fn excluded(name: &str, folder: bool) -> bool {
    name == "metadata.json" || folder && matches!(name, ".git" | "__pycache__" | "__pypackages__")
}

/// The SHA-256 of every file in `folder`, each as its path, a NUL, its length, a NUL and its contents, in order of
/// path, with CRLF read as LF in text. Links are followed when they stay inside the folder.
pub fn folder_hash(folder: &Path) -> std::io::Result<String> {
    let root = fs::canonicalize(folder)?;
    let mut files = Vec::new();
    collect(folder, "", &root, &mut Vec::new(), &mut files)?;
    files.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let mut hash = Sha256::new();
    for (path, contents) in files {
        let contents = if !contents.contains(&0) && contents.windows(2).any(|pair| pair == b"\r\n")
        {
            let mut lf = Vec::with_capacity(contents.len());
            let mut bytes = contents.iter().peekable();
            while let Some(&byte) = bytes.next() {
                if byte == b'\r' && bytes.peek() == Some(&&b'\n') {
                    continue;
                }
                lf.push(byte);
            }
            lf
        } else {
            contents
        };
        hash.update(path.as_bytes());
        hash.update([0]);
        hash.update(contents.len().to_string().as_bytes());
        hash.update([0]);
        hash.update(&contents);
    }
    Ok(format!("{:x}", hash.finalize()))
}

/// Every file the hash covers in `folder`, by its path inside it, with its contents.
pub(super) fn collect_files(
    folder: &Path,
    root: &Path,
    files: &mut Vec<(String, Vec<u8>)>,
) -> std::io::Result<()> {
    collect(folder, "", root, &mut Vec::new(), files)
}

fn collect(
    folder: &Path,
    prefix: &str,
    root: &Path,
    ancestors: &mut Vec<std::path::PathBuf>,
    files: &mut Vec<(String, Vec<u8>)>,
) -> std::io::Result<()> {
    let real = fs::canonicalize(folder)?;
    if ancestors.contains(&real) {
        return Ok(());
    }
    ancestors.push(real);
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let kind = entry.file_type()?;
        let path = entry.path();
        let is_folder = if kind.is_symlink() {
            match fs::canonicalize(&path) {
                Ok(target) if target.starts_with(root) => target.is_dir(),
                _ => continue,
            }
        } else if kind.is_dir() {
            true
        } else if kind.is_file() {
            false
        } else {
            continue;
        };
        if excluded(&name, is_folder) {
            continue;
        }
        let relative = format!("{prefix}{name}");
        if is_folder {
            collect(&path, &format!("{relative}/"), root, ancestors, files)?;
        } else {
            files.push((relative, fs::read(&path)?));
        }
    }
    ancestors.pop();
    Ok(())
}

/// Whether copying `folder` keeps everything in it: nothing the CLI leaves out (`.git`, `metadata.json`), no link
/// out of the folder or to nothing, and nothing but files, folders and links.
pub(super) fn copies_whole(folder: &Path) -> std::io::Result<bool> {
    let root = fs::canonicalize(folder)?;
    whole(folder, &root, &mut Vec::new())
}

fn whole(
    folder: &Path,
    root: &Path,
    ancestors: &mut Vec<std::path::PathBuf>,
) -> std::io::Result<bool> {
    let real = fs::canonicalize(folder)?;
    if ancestors.contains(&real) {
        return Ok(true);
    }
    ancestors.push(real);
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let kind = entry.file_type()?;
        let path = entry.path();
        let is_folder = if kind.is_symlink() {
            match fs::canonicalize(&path) {
                Ok(target) if target.starts_with(root) => target.is_dir(),
                _ => return Ok(false),
            }
        } else if kind.is_dir() {
            true
        } else if kind.is_file() {
            false
        } else {
            return Ok(false);
        };
        if excluded(&name, is_folder) {
            return Ok(false);
        }
        if is_folder && !whole(&path, root, ancestors)? {
            return Ok(false);
        }
    }
    ancestors.pop();
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::folder_hash;

    /// The vectors in the `skills` CLI's README.
    #[test]
    fn the_hash_matches_the_skills_cli() {
        let folder = std::env::temp_dir().join(format!("patchbay-hash-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        assert_eq!(
            folder_hash(&folder).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        fs::write(folder.join("SKILL.md"), "# hi\n").unwrap();
        assert_eq!(
            folder_hash(&folder).unwrap(),
            "93c36952d8843b33e68ae970ebabcc03e1687c9bb0dd1ecaf357060318952f55"
        );
        fs::write(folder.join("SKILL.md"), "a\r\n").unwrap();
        fs::create_dir_all(folder.join("docs")).unwrap();
        fs::write(folder.join("docs/b.md"), "b\n").unwrap();
        fs::write(folder.join("metadata.json"), "{}").unwrap();
        assert_eq!(
            folder_hash(&folder).unwrap(),
            "b22c4d22aa319d2924312fa18fcbfedc88fa89c74dbd8f298c683e1caf7b7f24"
        );
    }
}
