//! Hugging Face snapshots can contain either regular files (often on Windows) or links to
//! shared blobs. File-level removal unlinks one enumerated snapshot entry, never its target.
use serde::Serialize;
use std::{fs, path::Path};

#[derive(Serialize, Debug, PartialEq)]
pub struct CachedFile {
    pub revision: String,
    pub path: String,
    pub bytes: u64,
}

pub(super) fn real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| {
        #[cfg(windows)]
        if std::os::windows::fs::MetadataExt::file_attributes(&m) & 0x400 != 0 {
            return false;
        }
        m.is_dir()
    })
}

/// Return bytes stored directly in snapshots, excluding partial files. Report GGUFs with their
/// actual directory entries (not paths supplied by the UI). Never recurse through directory links.
pub(super) fn walk(repo_dir: &Path, mut gguf: impl FnMut(&Path, CachedFile)) -> u64 {
    let snapshots = repo_dir.join("snapshots");
    if !real_dir(repo_dir) || !real_dir(&snapshots) {
        return 0;
    }
    let mut dirs = vec![snapshots.clone()];
    let mut bytes = 0;
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                if real_dir(&path) {
                    dirs.push(path);
                }
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.ends_with(".incomplete") {
                continue;
            }
            let is_gguf = name.to_ascii_lowercase().ends_with(".gguf");
            if !kind.is_file() && !(kind.is_symlink() && is_gguf) {
                continue;
            }
            let Ok(meta) = fs::metadata(&path) else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            if kind.is_file() {
                bytes += meta.len();
            }
            if !is_gguf {
                continue;
            }
            let mut parts = path
                .strip_prefix(&snapshots)
                .expect("walk is under snapshots")
                .iter();
            let Some(revision) = parts.next().and_then(|p| p.to_str()) else {
                continue;
            };
            let Some(parts) = parts.map(|p| p.to_str()).collect::<Option<Vec<_>>>() else {
                continue;
            };
            if parts.is_empty() {
                continue;
            }
            gguf(
                &path,
                CachedFile {
                    revision: revision.into(),
                    path: parts.join("/"),
                    bytes: meta.len(),
                },
            );
        }
    }
    bytes
}

pub(super) fn remove(cache: &Path, repo: &str, revision: &str, file: &str) -> Result<(), String> {
    if !super::valid_repo(repo) {
        return Err("Not a Hugging Face model id".into());
    }
    let repo_dir = cache.join(format!("models--{}", repo.replace('/', "--")));
    let mut selected = None;
    walk(&repo_dir, |path, f| {
        if f.revision == revision && f.path == file {
            selected = Some(path.to_owned());
        }
    });
    let path = selected.ok_or("That GGUF is no longer in this snapshot. Reload the cache.")?;
    fs::remove_file(path).map_err(|e| format!("Could not remove {file}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removes_only_the_exact_snapshot_file_and_never_follows_directory_links() {
        let t = tempfile::tempdir().unwrap();
        let cache = t.path();
        let repo = cache.join("models--org--m");
        let snap = repo.join("snapshots");
        for rev in ["old", "new"] {
            fs::create_dir_all(snap.join(rev).join("quants")).unwrap();
            fs::write(snap.join(rev).join("quants/q.gguf"), b"model").unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let blob = repo.join("blobs/content");
            fs::create_dir_all(blob.parent().unwrap()).unwrap();
            fs::write(&blob, b"shared").unwrap();
            symlink(&blob, snap.join("new/linked.gguf")).unwrap();
            symlink(&blob, snap.join("old/other.gguf")).unwrap();
            symlink(snap.join("old/quants"), snap.join("new/linked-dir")).unwrap();
            assert!(remove(cache, "org/m", "new", "linked-dir/q.gguf").is_err());
            remove(cache, "org/m", "new", "linked.gguf").unwrap();
            assert!(fs::symlink_metadata(snap.join("new/linked.gguf")).is_err());
            assert_eq!(fs::read(snap.join("old/other.gguf")).unwrap(), b"shared");
        }
        for (rev, file) in [
            ("../old", "quants/q.gguf"),
            ("new", "../old/quants/q.gguf"),
            ("new", "/quants/q.gguf"),
        ] {
            assert!(remove(cache, "org/m", rev, file).is_err());
        }
        remove(cache, "org/m", "new", "quants/q.gguf").unwrap();
        assert!(!snap.join("new/quants/q.gguf").exists());
        assert!(snap.join("old/quants/q.gguf").is_file());
    }
}
