// Directory synchronization: scans both sides, then computes a plan the user
// reviews. The plan is executed by the transfer queue (see transfer/queue.rs).
//
// A path that could not be read is never treated as absent: that would turn a
// permission problem into deletions on the other side.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::error::{OmniError, Result};
use crate::protocols::{Protocol, remote_join};

/// FAT stores times with a 2-second resolution.
const MTIME_TOLERANCE_SECS: i64 = 2;
/// Leftovers of an interrupted transfer (see `protocols::temp_file_name`).
const TEMP_FILE_PATTERN: &str = ".omniscp-*.part";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncMode {
    Upload,
    Download,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncOptions {
    pub mode: SyncMode,
    /// Patterns with `*` and `?`, matched on the name, or on the relative path if they contain `/`.
    pub excludes: Vec<String>,
    /// Remove what is missing from the source. Ignored in `Both` mode, where
    /// nothing tells a deleted file from a new one.
    pub delete_orphans: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryMeta {
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<i64>,
}

impl EntryMeta {
    fn same_file(&self, other: &EntryMeta) -> bool {
        self.size == other.size && newer_side(self, other).is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncActionKind {
    Upload,
    Download,
    CreateRemoteDir,
    CreateLocalDir,
    DeleteRemote,
    DeleteLocal,
    /// No safe default: the user chooses a direction or leaves it alone.
    Conflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncReason {
    New,
    SourceNewer,
    /// One-way copy that would replace a more recent file.
    TargetNewer,
    SameDateDifferentSize,
    /// A file on one side, a folder on the other.
    TypeMismatch,
    Orphan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncAction {
    /// Relative to both roots, `/`-separated.
    pub path: String,
    pub kind: SyncActionKind,
    pub reason: SyncReason,
    pub local: Option<EntryMeta>,
    pub remote: Option<EntryMeta>,
    /// Whether the preview ticks this action by default.
    pub selected: bool,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct SyncPlan {
    pub actions: Vec<SyncAction>,
    /// Folders that could not be read; nothing below them is planned.
    pub problems: Vec<String>,
    pub skipped_links: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Scan {
    entries: BTreeMap<String, EntryMeta>,
    problems: Vec<String>,
    skipped_links: Vec<String>,
    /// Unreadable paths and links to folders: their content is unknown.
    blocked: BTreeSet<String>,
    /// Folders holding something the plan does not see, so they must not be removed.
    kept_dirs: BTreeSet<String>,
}

impl Scan {
    fn keep_ancestors(&mut self, path: &str) {
        let mut rest = path;
        while let Some(i) = rest.rfind('/') {
            rest = &rest[..i];
            self.kept_dirs.insert(rest.to_string());
        }
    }

    fn block(&mut self, path: &str) {
        self.blocked.insert(path.to_string());
        self.keep_ancestors(path);
    }
}

fn is_under(path: &str, parent: &str) -> bool {
    path.strip_prefix(parent).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// Joins a relative path below the scan root.
fn child_path(parent: &str, name: &str) -> String {
    if parent.is_empty() { name.to_string() } else { format!("{parent}/{name}") }
}

/// `*` matches any run of characters but `/`, `?` any single one.
fn glob_match(pattern: &[char], text: &[char]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some(('*', rest)) => {
            glob_match(rest, text)
                || text.first().is_some_and(|c| *c != '/' && glob_match(pattern, &text[1..]))
        }
        Some(('?', rest)) => {
            text.first().is_some_and(|c| *c != '/') && glob_match(rest, &text[1..])
        }
        Some((p, rest)) => text.first() == Some(p) && glob_match(rest, &text[1..]),
    }
}

fn is_excluded(path: &str, patterns: &[String]) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let matches = |pattern: &str, text: &str| {
        let pattern: Vec<char> = pattern.chars().collect();
        let text: Vec<char> = text.chars().collect();
        glob_match(&pattern, &text)
    };
    matches(TEMP_FILE_PATTERN, name)
        || patterns.iter().any(|pattern| {
            let pattern = pattern.trim().trim_end_matches('/');
            !pattern.is_empty() && matches(pattern, if pattern.contains('/') { path } else { name })
        })
}

fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) { Err(OmniError::Cancelled) } else { Ok(()) }
}

pub fn local_meta(path: &Path) -> Option<EntryMeta> {
    // Follows links: a link to a file syncs as that file.
    let meta = std::fs::metadata(path).ok()?;
    Some(EntryMeta {
        is_dir: meta.is_dir(),
        size: if meta.is_dir() { 0 } else { meta.len() },
        modified: meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64),
    })
}

pub fn remote_meta(conn: &impl Protocol, path: &str) -> Option<EntryMeta> {
    let entry = conn.stat(path).ok()?;
    Some(EntryMeta {
        is_dir: entry.is_dir,
        size: if entry.is_dir { 0 } else { entry.size },
        modified: entry.modified,
    })
}

pub fn scan_local(root: &Path, excludes: &[String], cancel: &AtomicBool) -> Result<Scan> {
    let mut scan = Scan::default();
    let top = std::fs::read_dir(root).map_err(|e| OmniError::io_context(root.display(), e))?;
    scan_local_entries(top, "", excludes, cancel, &mut scan)?;
    Ok(scan)
}

fn scan_local_entries(
    entries: std::fs::ReadDir,
    rel: &str,
    excludes: &[String],
    cancel: &AtomicBool,
    scan: &mut Scan,
) -> Result<()> {
    for entry in entries {
        check_cancel(cancel)?;
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                scan.problems.push(format!("{rel}: {e}"));
                scan.block(rel);
                continue;
            }
        };
        let path = child_path(rel, &entry.file_name().to_string_lossy());
        if is_excluded(&path, excludes) {
            scan.keep_ancestors(&path);
            continue;
        }
        let is_link = entry.file_type().is_ok_and(|t| t.is_symlink());
        let Some(meta) = local_meta(&entry.path()) else {
            if is_link {
                scan.skipped_links.push(path.clone());
            } else {
                scan.problems.push(format!("{path}: unreadable"));
            }
            scan.block(&path);
            continue;
        };
        if is_link && meta.is_dir {
            scan.skipped_links.push(path.clone());
            scan.block(&path);
            continue;
        }
        if meta.is_dir {
            match std::fs::read_dir(entry.path()) {
                Ok(children) => {
                    scan.entries.insert(path.clone(), meta);
                    scan_local_entries(children, &path, excludes, cancel, scan)?;
                }
                Err(e) => {
                    scan.problems.push(format!("{path}: {e}"));
                    scan.block(&path);
                }
            }
        } else {
            scan.entries.insert(path, meta);
        }
    }
    Ok(())
}

pub fn scan_remote(conn: &impl Protocol, root: &str, excludes: &[String], cancel: &AtomicBool) -> Result<Scan> {
    let mut scan = Scan::default();
    let top = conn.list_dir(root)?;
    scan_remote_entries(conn, root, top, "", excludes, cancel, &mut scan)?;
    Ok(scan)
}

fn scan_remote_entries(
    conn: &impl Protocol,
    root: &str,
    entries: Vec<crate::protocols::FileEntry>,
    rel: &str,
    excludes: &[String],
    cancel: &AtomicBool,
    scan: &mut Scan,
) -> Result<()> {
    for entry in entries {
        check_cancel(cancel)?;
        let path = child_path(rel, &entry.name);
        if is_excluded(&path, excludes) {
            scan.keep_ancestors(&path);
            continue;
        }
        if entry.is_symlink && entry.is_dir {
            scan.skipped_links.push(path.clone());
            scan.block(&path);
            continue;
        }
        let meta = EntryMeta {
            is_dir: entry.is_dir,
            size: if entry.is_dir { 0 } else { entry.size },
            modified: entry.modified,
        };
        if entry.is_dir {
            match conn.list_dir(&remote_join(root, &path)) {
                Ok(children) => {
                    scan.entries.insert(path.clone(), meta);
                    scan_remote_entries(conn, root, children, &path, excludes, cancel, scan)?;
                }
                // Every further listing would fail too; better to stop than to report them all.
                Err(e) if !conn.is_alive() => return Err(e),
                Err(e) => {
                    scan.problems.push(format!("{path}: {e}"));
                    scan.block(&path);
                }
            }
        } else {
            scan.entries.insert(path, meta);
        }
    }
    Ok(())
}

#[derive(PartialEq)]
enum Side {
    Local,
    Remote,
}

/// `None` when the dates are too close to tell, or unknown.
fn newer_side(local: &EntryMeta, remote: &EntryMeta) -> Option<Side> {
    let difference = local.modified? - remote.modified?;
    if difference > MTIME_TOLERANCE_SECS {
        Some(Side::Local)
    } else if difference < -MTIME_TOLERANCE_SECS {
        Some(Side::Remote)
    } else {
        None
    }
}

pub fn build_plan(local: Scan, remote: Scan, options: &SyncOptions) -> SyncPlan {
    let mode = options.mode;
    let delete_orphans = options.delete_orphans && mode != SyncMode::Both;
    let blocked: Vec<&String> = local.blocked.iter().chain(&remote.blocked).collect();
    let paths: BTreeSet<&String> = local.entries.keys().chain(remote.entries.keys()).collect();

    let mut actions = Vec::new();
    for path in paths {
        if blocked.iter().any(|b| is_under(path, b)) {
            continue;
        }
        let l = local.entries.get(path).copied();
        let r = remote.entries.get(path).copied();
        let planned = match (l, r) {
            (Some(a), Some(b)) if a.is_dir && b.is_dir => None,
            (Some(a), Some(b)) if a.is_dir != b.is_dir => {
                Some((SyncActionKind::Conflict, SyncReason::TypeMismatch, false))
            }
            (Some(a), Some(b)) if a.same_file(&b) => None,
            (Some(a), Some(b)) => Some(plan_changed_file(mode, &a, &b)),
            (Some(a), None) if mode == SyncMode::Download => {
                (delete_orphans && !(a.is_dir && local.kept_dirs.contains(path)))
                    .then_some((SyncActionKind::DeleteLocal, SyncReason::Orphan, true))
            }
            (Some(a), None) => Some((
                if a.is_dir { SyncActionKind::CreateRemoteDir } else { SyncActionKind::Upload },
                SyncReason::New,
                true,
            )),
            (None, Some(b)) if mode == SyncMode::Upload => {
                (delete_orphans && !(b.is_dir && remote.kept_dirs.contains(path)))
                    .then_some((SyncActionKind::DeleteRemote, SyncReason::Orphan, true))
            }
            (None, Some(b)) => Some((
                if b.is_dir { SyncActionKind::CreateLocalDir } else { SyncActionKind::Download },
                SyncReason::New,
                true,
            )),
            (None, None) => None,
        };
        if let Some((kind, reason, selected)) = planned {
            actions.push(SyncAction {
                path: path.clone(),
                kind,
                reason,
                local: l,
                remote: r,
                selected,
            });
        }
    }

    let mut problems = local.problems;
    problems.extend(remote.problems);
    let mut skipped_links = local.skipped_links;
    skipped_links.extend(remote.skipped_links);
    SyncPlan { actions, problems, skipped_links }
}

fn plan_changed_file(mode: SyncMode, local: &EntryMeta, remote: &EntryMeta) -> (SyncActionKind, SyncReason, bool) {
    let newer = newer_side(local, remote);
    match mode {
        SyncMode::Upload => match newer {
            Some(Side::Local) => (SyncActionKind::Upload, SyncReason::SourceNewer, true),
            Some(Side::Remote) => (SyncActionKind::Upload, SyncReason::TargetNewer, false),
            None => (SyncActionKind::Upload, SyncReason::SameDateDifferentSize, false),
        },
        SyncMode::Download => match newer {
            Some(Side::Remote) => (SyncActionKind::Download, SyncReason::SourceNewer, true),
            Some(Side::Local) => (SyncActionKind::Download, SyncReason::TargetNewer, false),
            None => (SyncActionKind::Download, SyncReason::SameDateDifferentSize, false),
        },
        SyncMode::Both => match newer {
            Some(Side::Local) => (SyncActionKind::Upload, SyncReason::SourceNewer, true),
            Some(Side::Remote) => (SyncActionKind::Download, SyncReason::SourceNewer, true),
            None => (SyncActionKind::Conflict, SyncReason::SameDateDifferentSize, false),
        },
    }
}

/// Plans come back from the frontend: make sure they stay below the roots.
pub fn validate_relative_path(path: &str) -> Result<()> {
    let valid = !path.is_empty()
        && path.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
        && !path.contains(['\\', '\0'])
        && !Path::new(path).is_absolute();
    if valid {
        Ok(())
    } else {
        Err(OmniError::Config(format!("Invalid relative path: \"{path}\"")))
    }
}

/// Whether what is at the destination is still what the preview showed.
pub fn unchanged_since_preview(expected: Option<EntryMeta>, current: Option<EntryMeta>) -> bool {
    match (expected, current) {
        (None, None) => true,
        (Some(a), Some(b)) => a.is_dir == b.is_dir && (a.is_dir || (a.size == b.size && a.modified == b.modified)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(size: u64, modified: i64) -> EntryMeta {
        EntryMeta { is_dir: false, size, modified: Some(modified) }
    }

    fn dir() -> EntryMeta {
        EntryMeta { is_dir: true, size: 0, modified: Some(1) }
    }

    fn scan(entries: &[(&str, EntryMeta)]) -> Scan {
        Scan {
            entries: entries.iter().map(|(p, m)| (p.to_string(), *m)).collect(),
            ..Scan::default()
        }
    }

    fn options(mode: SyncMode, delete_orphans: bool) -> SyncOptions {
        SyncOptions { mode, excludes: vec![], delete_orphans }
    }

    fn summary(plan: &SyncPlan) -> Vec<(&str, SyncActionKind, bool)> {
        plan.actions.iter().map(|a| (a.path.as_str(), a.kind, a.selected)).collect()
    }

    #[test]
    fn identical_trees_need_nothing() {
        let tree = [("d", dir()), ("d/a", file(3, 100))];
        // Within the tolerance, and with an unknown date, the same size means the same file.
        let remote = [
            ("d", dir()),
            ("d/a", file(3, 102)),
        ];
        let plan = build_plan(scan(&tree), scan(&remote), &options(SyncMode::Both, false));
        assert!(plan.actions.is_empty());

        let unknown = EntryMeta { is_dir: false, size: 3, modified: None };
        let plan = build_plan(scan(&[("a", file(3, 100))]), scan(&[("a", unknown)]), &options(SyncMode::Upload, false));
        assert!(plan.actions.is_empty());
    }

    #[test]
    fn one_way_copies_new_files_and_folders() {
        let local = [("d", dir()), ("d/a", file(3, 100))];
        let plan = build_plan(scan(&local), scan(&[]), &options(SyncMode::Upload, false));
        assert_eq!(summary(&plan), [
            ("d", SyncActionKind::CreateRemoteDir, true),
            ("d/a", SyncActionKind::Upload, true),
        ]);
        let plan = build_plan(scan(&[]), scan(&local), &options(SyncMode::Download, false));
        assert_eq!(summary(&plan), [
            ("d", SyncActionKind::CreateLocalDir, true),
            ("d/a", SyncActionKind::Download, true),
        ]);
    }

    #[test]
    fn replacing_a_newer_file_is_never_ticked_by_default() {
        let older = [("a", file(1, 100))];
        let newer = [("a", file(2, 500))];
        let plan = build_plan(scan(&newer), scan(&older), &options(SyncMode::Upload, false));
        assert_eq!(summary(&plan), [("a", SyncActionKind::Upload, true)]);
        let plan = build_plan(scan(&older), scan(&newer), &options(SyncMode::Upload, false));
        assert_eq!(summary(&plan), [("a", SyncActionKind::Upload, false)]);
        assert_eq!(plan.actions[0].reason, SyncReason::TargetNewer);
        let plan = build_plan(scan(&newer), scan(&older), &options(SyncMode::Download, false));
        assert_eq!(summary(&plan), [("a", SyncActionKind::Download, false)]);
    }

    #[test]
    fn same_date_different_size_is_not_ticked() {
        let a = [("a", file(1, 100))];
        let b = [("a", file(2, 100))];
        let plan = build_plan(scan(&a), scan(&b), &options(SyncMode::Upload, false));
        assert_eq!(summary(&plan), [("a", SyncActionKind::Upload, false)]);
        let plan = build_plan(scan(&a), scan(&b), &options(SyncMode::Both, false));
        assert_eq!(summary(&plan), [("a", SyncActionKind::Conflict, false)]);
    }

    #[test]
    fn two_way_copies_each_way_and_keeps_the_newer() {
        let local = [("only-local", file(1, 100)), ("changed", file(1, 500)), ("stale", file(1, 100))];
        let remote = [("only-remote", file(1, 100)), ("changed", file(2, 100)), ("stale", file(2, 500))];
        let plan = build_plan(scan(&local), scan(&remote), &options(SyncMode::Both, false));
        assert_eq!(summary(&plan), [
            ("changed", SyncActionKind::Upload, true),
            ("only-local", SyncActionKind::Upload, true),
            ("only-remote", SyncActionKind::Download, true),
            ("stale", SyncActionKind::Download, true),
        ]);
    }

    #[test]
    fn file_against_folder_is_a_conflict() {
        let plan = build_plan(scan(&[("x", file(1, 1))]), scan(&[("x", dir())]), &options(SyncMode::Upload, true));
        assert_eq!(summary(&plan), [("x", SyncActionKind::Conflict, false)]);
        assert_eq!(plan.actions[0].reason, SyncReason::TypeMismatch);
    }

    #[test]
    fn nothing_is_deleted_unless_asked() {
        let local = [("keep", file(1, 1))];
        let remote = [("keep", file(1, 1)), ("orphan", file(1, 1))];
        let plan = build_plan(scan(&local), scan(&remote), &options(SyncMode::Upload, false));
        assert!(plan.actions.is_empty());

        let plan = build_plan(scan(&local), scan(&remote), &options(SyncMode::Upload, true));
        assert_eq!(summary(&plan), [("orphan", SyncActionKind::DeleteRemote, true)]);
        let plan = build_plan(scan(&remote), scan(&local), &options(SyncMode::Download, true));
        assert_eq!(summary(&plan), [("orphan", SyncActionKind::DeleteLocal, true)]);
    }

    #[test]
    fn two_way_never_deletes() {
        let plan = build_plan(scan(&[("a", file(1, 1))]), scan(&[]), &options(SyncMode::Both, true));
        assert_eq!(summary(&plan), [("a", SyncActionKind::Upload, true)]);
    }

    #[test]
    fn unreadable_folder_is_not_treated_as_missing() {
        let mut local = scan(&[("ok", file(1, 1))]);
        local.problems.push("secret: denied".into());
        local.block("secret");
        let remote = [("ok", file(1, 1)), ("secret", dir()), ("secret/a", file(1, 1)), ("secret2", file(1, 1))];

        let plan = build_plan(local, scan(&remote), &options(SyncMode::Upload, true));

        // `secret2` only shares a prefix with the unreadable folder.
        assert_eq!(summary(&plan), [("secret2", SyncActionKind::DeleteRemote, true)]);
        assert_eq!(plan.problems, ["secret: denied"]);
    }

    #[test]
    fn unreadable_target_folder_blocks_copies_into_it() {
        let mut remote = scan(&[]);
        remote.block("locked");
        let local = [("locked", dir()), ("locked/a", file(1, 1))];
        let plan = build_plan(scan(&local), remote, &options(SyncMode::Upload, false));
        assert!(plan.actions.is_empty());
    }

    #[test]
    fn folder_holding_excluded_files_is_not_removed() {
        let mut remote = scan(&[("d", dir()), ("d/a", file(1, 1)), ("empty", dir())]);
        remote.keep_ancestors("d/secret.env");
        let plan = build_plan(scan(&[]), remote, &options(SyncMode::Upload, true));
        assert_eq!(summary(&plan), [
            ("d/a", SyncActionKind::DeleteRemote, true),
            ("empty", SyncActionKind::DeleteRemote, true),
        ]);
    }

    #[test]
    fn exclusion_patterns() {
        let patterns: Vec<String> =
            ["*.tmp", "node_modules/", "build/*.o", "?.log", "  "].map(String::from).to_vec();
        for excluded in ["a.tmp", "d/a.tmp", "node_modules", "d/node_modules", "build/x.o", "a.log", "d/.omniscp-1f-0.part"] {
            assert!(is_excluded(excluded, &patterns), "{excluded} should be excluded");
        }
        for kept in ["a.tmp2", "build/sub/x.o", "other/x.o", "ab.log", "keep"] {
            assert!(!is_excluded(kept, &patterns), "{kept} should be kept");
        }
    }

    #[test]
    fn relative_path_validation() {
        for ok in ["a", "a/b.txt", "..hidden/x"] {
            assert!(validate_relative_path(ok).is_ok(), "{ok}");
        }
        for bad in ["", "/etc/passwd", "../x", "a/../../x", "a//b", "a/./b", "a\\b"] {
            assert!(validate_relative_path(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn change_detection() {
        assert!(unchanged_since_preview(None, None));
        assert!(unchanged_since_preview(Some(file(1, 1)), Some(file(1, 1))));
        assert!(unchanged_since_preview(Some(dir()), Some(EntryMeta { modified: Some(9), ..dir() })));
        assert!(!unchanged_since_preview(Some(file(1, 1)), Some(file(1, 2))));
        assert!(!unchanged_since_preview(Some(file(1, 1)), Some(file(2, 1))));
        assert!(!unchanged_since_preview(Some(file(1, 1)), Some(dir())));
        assert!(!unchanged_since_preview(Some(file(1, 1)), None));
        assert!(!unchanged_since_preview(None, Some(file(1, 1))));
    }
}
