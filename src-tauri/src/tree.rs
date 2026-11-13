// Totals over folders, for the properties dialog. Links to folders are not
// followed (they can loop), as in transfers.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;

use crate::error::{OmniError, Result};
use crate::protocols::Protocol;

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub struct TreeSize {
    pub bytes: u64,
    pub files: u64,
    pub folders: u64,
    /// Folders that could not be listed: the totals are then a lower bound.
    pub unreadable: u64,
}

fn check(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) { Err(OmniError::Cancelled) } else { Ok(()) }
}

pub fn local_tree_size(paths: &[String], cancel: &AtomicBool) -> Result<TreeSize> {
    fn walk(path: &Path, total: &mut TreeSize, cancel: &AtomicBool) -> Result<()> {
        check(cancel)?;
        let Ok(meta) = std::fs::symlink_metadata(path) else {
            total.unreadable += 1;
            return Ok(());
        };
        if meta.is_symlink() {
            // A link to a file counts as that file and a broken link as itself, as the
            // panes list them; a link to a folder is not followed.
            match std::fs::metadata(path) {
                Ok(target) if target.is_dir() => {}
                Ok(target) => {
                    total.files += 1;
                    total.bytes += target.len();
                }
                Err(_) => {
                    total.files += 1;
                    total.bytes += meta.len();
                }
            }
        } else if meta.is_dir() {
            total.folders += 1;
            match std::fs::read_dir(path) {
                Ok(entries) => {
                    for entry in entries {
                        match entry {
                            Ok(entry) => walk(&entry.path(), total, cancel)?,
                            Err(_) => total.unreadable += 1,
                        }
                    }
                }
                Err(_) => total.unreadable += 1,
            }
        } else {
            total.files += 1;
            total.bytes += meta.len();
        }
        Ok(())
    }
    let mut total = TreeSize::default();
    for path in paths {
        walk(Path::new(path), &mut total, cancel)?;
    }
    Ok(total)
}

/// Same as `local_tree_size`; fails only if cancelled or if the session died.
pub fn remote_tree_size(conn: &dyn Protocol, paths: &[String], cancel: &AtomicBool) -> Result<TreeSize> {
    fn walk(conn: &dyn Protocol, path: &str, total: &mut TreeSize, cancel: &AtomicBool) -> Result<()> {
        check(cancel)?;
        total.folders += 1;
        let entries = match conn.list_dir(path) {
            Ok(entries) => entries,
            Err(e) if !conn.is_alive() => return Err(e),
            Err(_) => {
                total.unreadable += 1;
                return Ok(());
            }
        };
        for entry in entries {
            if entry.is_dir && !entry.is_symlink {
                walk(conn, &entry.path.to_string_lossy(), total, cancel)?;
            } else if !entry.is_dir {
                total.files += 1;
                total.bytes += entry.size;
            }
        }
        Ok(())
    }
    let mut total = TreeSize::default();
    for path in paths {
        let entry = match conn.stat(path) {
            Ok(entry) => entry,
            Err(e) if !conn.is_alive() => return Err(e),
            Err(_) => {
                total.unreadable += 1;
                continue;
            }
        };
        if entry.is_dir && !entry.is_symlink {
            walk(conn, path, &mut total, cancel)?;
        } else if !entry.is_dir {
            total.files += 1;
            total.bytes += entry.size;
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_totals_skip_folder_links_and_count_unreadable_folders() {
        let root = std::env::temp_dir().join(format!("omniscp-tree-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::write(root.join("a/one"), b"12345").unwrap();
        std::fs::write(root.join("a/b/two"), b"123").unwrap();
        std::fs::write(root.join("three"), b"1").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("a"), root.join("loop")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("gone", root.join("broken")).unwrap();

        let cancel = AtomicBool::new(false);
        let paths = [root.join("a").to_string_lossy().into_owned(), root.join("three").to_string_lossy().into_owned()];
        let total = local_tree_size(&paths, &cancel).unwrap();
        assert_eq!(total, TreeSize { bytes: 9, files: 3, folders: 2, unreadable: 0 });

        let whole = local_tree_size(&[root.to_string_lossy().into_owned()], &cancel).unwrap();
        // The folder link is not followed; the broken link counts as its own 4 bytes.
        assert_eq!((whole.bytes, whole.files, whole.folders), (13, 4, 3));

        let missing = local_tree_size(&[root.join("nope").to_string_lossy().into_owned()], &cancel).unwrap();
        assert_eq!(missing.unreadable, 1);

        cancel.store(true, Ordering::Relaxed);
        assert!(matches!(local_tree_size(&paths, &cancel), Err(OmniError::Cancelled)));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
