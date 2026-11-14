//! Live tests of the transfer queue and sync against a real server; ignored by default.
//! Run them one at a time: one test restarts the container.
//!
//! Over SFTP, on the server of sftp_live.rs:
//!
//!   OMNISCP_TEST_FINGERPRINT=SHA256:... cargo test --test transfer_live -- --ignored --test-threads=1
//!
//! Over SCP, on a server of scp_live.rs, with its variables as well:
//!
//!   OMNISCP_TEST_PROTOCOL=scp OMNISCP_TEST_PORT=2223 OMNISCP_TEST_BASE=/home/test \\
//!   OMNISCP_TEST_CONTAINER=omniscp-scp-debian OMNISCP_TEST_FINGERPRINT=SHA256:... \\
//!   cargo test --test transfer_live -- --ignored --test-threads=1
//!
//! `OMNISCP_TEST_CHROOT` is where the server's paths live inside the container
//! (/home/test for the SFTP server), and `OMNISCP_TEST_OWNER` the name expected
//! for the test account, empty when the server's account files are out of reach.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use omniscp_lib::config::{AuthMethod, Profile};
use omniscp_lib::protocols::{ConnectParams, Protocol, ProtocolType, for_profile, probe};
use omniscp_lib::sync::{self, SyncAction, SyncActionKind, SyncMode, SyncOptions, SyncPlan};
use omniscp_lib::transfer::queue::{SyncRequest, TransferEvent, TransferQueue, TransferRequest};
use omniscp_lib::transfer::{ConflictStrategy, TransferDirection, TransferStatus};
use omniscp_lib::tree::{TreeSize, remote_tree_size};
use omniscp_lib::error::OmniError;

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

struct Harness {
    tmp: PathBuf,
    base: String,
    conn: Box<dyn Protocol>,
    params: ConnectParams,
    queue: TransferQueue,
    events: Receiver<TransferEvent>,
    next_id: u32,
}

impl Harness {
    fn new(label: &str) -> Self {
        Self::with_workers(label, 1)
    }

    fn with_workers(label: &str, workers: u32) -> Self {
        let fingerprint = std::env::var("OMNISCP_TEST_FINGERPRINT").expect("OMNISCP_TEST_FINGERPRINT");
        let tmp = std::env::temp_dir().join(format!("omniscp-queue-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let params = ConnectParams {
            profile: Profile {
                id: "test".into(),
                name: "test".into(),
                host: "localhost".into(),
                port: env_or("OMNISCP_TEST_PORT", "2222").parse().unwrap(),
                protocol: if env_or("OMNISCP_TEST_PROTOCOL", "sftp") == "scp" {
                    ProtocolType::Scp
                } else {
                    ProtocolType::Sftp
                },
                username: "test".into(),
                auth_method: AuthMethod::Password,
                remote_directory: None,
                description: None,
                tags: vec![],
                group: None,
                save_password: false,
            },
            password: Some("test".into()),
            known_hosts_path: tmp.join("known_hosts"),
            trusted_fingerprint: Some(fingerprint),
        };
        let mut conn = for_profile(&params).unwrap();
        conn.connect().unwrap();
        let base = format!("{}/omniscp-queue-{label}", env_or("OMNISCP_TEST_BASE", "/upload"));
        let _ = conn.delete(&base);
        conn.mkdir(&base).unwrap();

        let (sender, events) = mpsc::channel();
        let sender = std::sync::Mutex::new(sender);
        let queue = TransferQueue::start(
            Arc::new(move |event| {
                let _ = sender.lock().unwrap().send(event);
            }),
            params.clone(),
            workers,
        );
        Self { tmp, base, conn, params, queue, events, next_id: 0 }
    }

    fn enqueue(&mut self, direction: TransferDirection, source: &str, dest_dir: &str, conflict: ConflictStrategy) -> String {
        self.enqueue_with(direction, source, dest_dir, conflict, false)
    }

    fn enqueue_with(
        &mut self,
        direction: TransferDirection,
        source: &str,
        dest_dir: &str,
        conflict: ConflictStrategy,
        delete_source: bool,
    ) -> String {
        self.next_id += 1;
        let id = self.next_id.to_string();
        self.queue
            .enqueue(TransferRequest {
                id: id.clone(),
                direction,
                source: source.into(),
                dest_dir: dest_dir.into(),
                conflict,
                dest_name: None,
                delete_source,
            })
            .unwrap();
        id
    }

    fn plan(&self, local_root: &Path, remote_root: &str, options: &SyncOptions) -> SyncPlan {
        let cancel = AtomicBool::new(false);
        let local = sync::scan_local(local_root, &options.excludes, &cancel).unwrap();
        let remote = sync::scan_remote(self.conn.as_ref(), remote_root, &options.excludes, &cancel).unwrap();
        sync::build_plan(local, remote, options)
    }

    fn run_sync(&mut self, local_root: &Path, remote_root: &str, actions: Vec<SyncAction>) -> TransferEvent {
        self.next_id += 1;
        let id = self.next_id.to_string();
        self.queue
            .enqueue_sync(SyncRequest {
                id: id.clone(),
                local_root: local_root.to_str().unwrap().into(),
                remote_root: remote_root.into(),
                actions,
            })
            .unwrap();
        self.wait_finished(&id)
    }

    fn wait_finished(&self, id: &str) -> TransferEvent {
        loop {
            let event = self.events.recv_timeout(Duration::from_secs(60)).expect("job never finished");
            if event.id == id
                && !matches!(event.status, TransferStatus::Pending | TransferStatus::InProgress | TransferStatus::Paused)
            {
                return event;
            }
        }
    }

    fn upload(&mut self, source: &Path, dest_dir: &str, conflict: ConflictStrategy) -> TransferEvent {
        let id = self.enqueue(TransferDirection::Upload, source.to_str().unwrap(), dest_dir, conflict);
        self.wait_finished(&id)
    }

    fn download(&mut self, source: &str, dest_dir: &Path, conflict: ConflictStrategy) -> TransferEvent {
        let id = self.enqueue(TransferDirection::Download, source, dest_dir.to_str().unwrap(), conflict);
        self.wait_finished(&id)
    }

    /// The next event for `id`, skipping progress reports.
    fn wait_status(&self, id: &str, status: TransferStatus) -> TransferEvent {
        loop {
            let event = self.events.recv_timeout(Duration::from_secs(60)).expect("status never reported");
            if event.id == id && event.status == status {
                return event;
            }
        }
    }

    /// Fails if any job reports anything during `wait`.
    fn assert_quiet(&self, wait: Duration) {
        if let Ok(event) = self.events.recv_timeout(wait) {
            panic!("expected no activity, got {:?} for job {}", event.status, event.id);
        }
    }

    fn remote_names(&self, dir: &str) -> Vec<String> {
        self.conn.list_dir(dir).unwrap().into_iter().map(|e| e.name).collect()
    }

    fn cleanup(self) {
        self.conn.delete(&self.base).unwrap();
        let _ = std::fs::remove_dir_all(&self.tmp);
    }
}

fn write(path: &Path, content: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

#[test]
#[ignore]
fn folder_round_trip() {
    let mut h = Harness::new("round-trip");
    let source = h.tmp.join("tree");
    write(&source.join("a.txt"), b"aaa");
    write(&source.join("sub/b.txt"), b"bbbbb");

    let base = h.base.clone();
    let up = h.upload(&source, &base, ConflictStrategy::Skip);
    assert_eq!(up.status, TransferStatus::Completed);
    assert_eq!(up.transferred_bytes, 8);
    assert_eq!(up.total_bytes, 8);

    let dest = h.tmp.join("dest");
    std::fs::create_dir_all(&dest).unwrap();
    let down = h.download(&format!("{base}/tree"), &dest, ConflictStrategy::Skip);
    assert_eq!(down.status, TransferStatus::Completed);
    assert_eq!(std::fs::read(dest.join("tree/a.txt")).unwrap(), b"aaa");
    assert_eq!(std::fs::read(dest.join("tree/sub/b.txt")).unwrap(), b"bbbbb");
    h.cleanup();
}

#[test]
#[ignore]
fn conflict_strategies() {
    let mut h = Harness::new("conflicts");
    let base = h.base.clone();
    let file = h.tmp.join("f.txt");
    write(&file, b"one");
    assert_eq!(h.upload(&file, &base, ConflictStrategy::Skip).status, TransferStatus::Completed);

    write(&file, b"two");
    assert_eq!(h.upload(&file, &base, ConflictStrategy::Skip).status, TransferStatus::Skipped);
    assert_eq!(h.upload(&file, &base, ConflictStrategy::Ask).status, TransferStatus::Skipped);

    let renamed = h.upload(&file, &base, ConflictStrategy::Rename);
    assert_eq!(renamed.status, TransferStatus::Completed);
    assert_eq!(renamed.destination.as_deref(), Some(format!("{base}/f (1).txt").as_str()));

    assert_eq!(h.upload(&file, &base, ConflictStrategy::Overwrite).status, TransferStatus::Completed);
    let back = h.tmp.join("back");
    h.conn.download(&format!("{base}/f.txt"), &back, &mut |_| true).unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), b"two");
    h.cleanup();
}

#[test]
#[ignore]
fn cancelling_keeps_the_destination_and_the_queue_running() {
    let mut h = Harness::new("cancel");
    let base = h.base.clone();
    let file = h.tmp.join("big.bin");
    write(&file, b"original");
    h.upload(&file, &base, ConflictStrategy::Skip);

    write(&file, &vec![7u8; 64 * 1024 * 1024]);
    let id = h.enqueue(TransferDirection::Upload, file.to_str().unwrap(), &base, ConflictStrategy::Overwrite);
    // Cancel as soon as the job reports that it started.
    loop {
        let event = h.events.recv_timeout(Duration::from_secs(60)).unwrap();
        if event.id == id && event.status == TransferStatus::InProgress {
            h.queue.cancel(&id);
            break;
        }
    }
    assert_eq!(h.wait_finished(&id).status, TransferStatus::Cancelled);
    assert_eq!(h.remote_names(&base), ["big.bin"]);
    let back = h.tmp.join("back");
    h.conn.download(&format!("{base}/big.bin"), &back, &mut |_| true).unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), b"original");

    // The next job still runs.
    let other = h.tmp.join("other.txt");
    write(&other, b"x");
    assert_eq!(h.upload(&other, &base, ConflictStrategy::Skip).status, TransferStatus::Completed);
    h.cleanup();
}

/// Runs a shell command inside the test container, for setup the app never does.
fn in_container(script: &str) {
    let status = Command::new("docker")
        .args(["exec", "-u", "test", &env_or("OMNISCP_TEST_CONTAINER", "omniscp-sftp-test"), "sh", "-c", script])
        .status()
        .unwrap();
    assert!(status.success(), "{script}");
}

#[test]
#[ignore]
fn permissions_links_owner_names_and_folder_size() {
    let mut h = Harness::new("props");
    let base = h.base.clone();
    let file = h.tmp.join("real file.txt");
    write(&file, b"12345");
    h.upload(&file, &base, ConflictStrategy::Skip);
    h.conn.mkdir(&format!("{base}/sub")).unwrap();
    let remote_file = format!("{base}/real file.txt");

    let stat = |path: &str| h.conn.stat(path).unwrap();
    h.conn.set_permissions(&remote_file, 0o4751).unwrap();
    assert_eq!(stat(&remote_file).mode, Some(0o4751));
    assert_eq!(stat(&remote_file).permissions.as_deref(), Some("-rwsr-x--x"));
    h.conn.set_permissions(&remote_file, 0o640).unwrap();
    assert_eq!(stat(&remote_file).permissions.as_deref(), Some("-rw-r-----"));
    h.conn.set_permissions(&format!("{base}/sub"), 0o1777).unwrap();
    assert_eq!(stat(&format!("{base}/sub")).permissions.as_deref(), Some("drwxrwxrwt"));
    // Never recursive, and refused where the account has no right.
    assert!(matches!(h.conn.set_permissions("/", 0o777), Err(OmniError::Permission(_))));

    let server_base = format!("{}{base}", env_or("OMNISCP_TEST_CHROOT", ""));
    in_container(&format!("cd '{server_base}' && ln -s 'real file.txt' 'the link' && ln -s ../nowhere dangling"));
    assert_eq!(h.conn.read_link(&format!("{base}/the link")).unwrap(), "real file.txt");
    assert_eq!(h.conn.read_link(&format!("{base}/dangling")).unwrap(), "../nowhere");
    assert!(h.conn.read_link(&remote_file).is_err(), "a plain file is not a link");

    let mut entries = h.conn.list_dir(&base).unwrap();
    assert!(entries.iter().all(|e| e.uid.is_some() && e.gid.is_some()));
    h.conn.account_names().name_entries(&mut entries);
    let expected = env_or("OMNISCP_TEST_OWNER", "test");
    let owner = entries.iter().find(|e| e.name == "real file.txt").unwrap().owner.clone();
    assert_eq!(owner.unwrap_or_default(), expected);

    let size = remote_tree_size(h.conn.as_ref(), std::slice::from_ref(&base), &AtomicBool::new(false)).unwrap();
    // The link to the file counts as that file, the dangling one as its own 10 bytes.
    assert_eq!(size, TreeSize { bytes: 20, files: 3, folders: 2, unreadable: 0 });
    let missing = remote_tree_size(h.conn.as_ref(), &[format!("{base}/nope")], &AtomicBool::new(false)).unwrap();
    assert_eq!(missing, TreeSize { unreadable: 1, ..TreeSize::default() });
    h.cleanup();
}

#[test]
#[ignore]
fn jobs_run_in_parallel_on_separate_sessions() {
    let mut h = Harness::with_workers("parallel", 3);
    let base = h.base.clone();
    let files: Vec<PathBuf> = (0..3u8)
        .map(|i| {
            let file = h.tmp.join(format!("f{i}.bin"));
            write(&file, &vec![i; 16 * 1024 * 1024]);
            file
        })
        .collect();
    let ids: Vec<String> = files
        .iter()
        .map(|f| h.enqueue(TransferDirection::Upload, f.to_str().unwrap(), &base, ConflictStrategy::Skip))
        .collect();

    let mut running = std::collections::HashSet::new();
    let mut most_at_once = 0;
    let mut finished = std::collections::HashMap::new();
    while finished.len() < ids.len() {
        let event = h.events.recv_timeout(Duration::from_secs(60)).expect("jobs never finished");
        if event.status == TransferStatus::InProgress {
            running.insert(event.id.clone());
        } else if event.status != TransferStatus::Pending {
            running.remove(&event.id);
            finished.insert(event.id.clone(), event.status);
        }
        most_at_once = most_at_once.max(running.len());
    }
    assert!(most_at_once >= 2, "jobs ran one after the other");
    assert!(ids.iter().all(|id| finished[id] == TransferStatus::Completed), "{finished:?}");
    for (i, file) in files.iter().enumerate() {
        let back = h.tmp.join("back.bin");
        h.conn.download(&format!("{base}/f{i}.bin"), &back, &mut |_| true).unwrap();
        assert!(std::fs::read(&back).unwrap() == std::fs::read(file).unwrap(), "f{i}.bin differs");
    }
    h.cleanup();
}

#[cfg(unix)]
#[test]
#[ignore]
fn one_bad_file_does_not_stop_a_folder_upload() {
    use std::os::unix::fs::PermissionsExt;

    let mut h = Harness::new("partial");
    let base = h.base.clone();
    let source = h.tmp.join("tree");
    write(&source.join("a-good.txt"), b"a");
    write(&source.join("b-locked.txt"), b"b");
    write(&source.join("c-good.txt"), b"c");
    std::os::unix::fs::symlink(&source, source.join("d-loop")).unwrap();
    let locked = source.join("b-locked.txt");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();

    let event = h.upload(&source, &base, ConflictStrategy::Skip);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();

    assert_eq!(event.status, TransferStatus::CompletedWithWarnings);
    let error = event.error.unwrap();
    assert!(error.contains("b-locked.txt"), "{error}");
    assert!(error.contains("d-loop"), "{error}");
    let mut names = h.remote_names(&format!("{base}/tree"));
    names.sort();
    assert_eq!(names, ["a-good.txt", "c-good.txt"]);
    h.cleanup();
}

#[test]
#[ignore]
fn failures_are_not_reported_as_success() {
    let mut h = Harness::new("failures");
    let base = h.base.clone();

    let missing = h.tmp.join("missing.txt");
    let event = h.upload(&missing, &base, ConflictStrategy::Skip);
    assert_eq!(event.status, TransferStatus::Failed);
    assert!(event.error.is_some());

    // Root of the chroot is not writable.
    let file = h.tmp.join("f.txt");
    write(&file, b"x");
    assert_eq!(h.upload(&file, "/", ConflictStrategy::Skip).status, TransferStatus::Failed);

    // A link to a folder selected on its own is refused rather than silently ignored.
    #[cfg(unix)]
    {
        let link = h.tmp.join("link");
        std::os::unix::fs::symlink(&h.tmp, &link).unwrap();
        assert_eq!(h.upload(&link, &base, ConflictStrategy::Skip).status, TransferStatus::Failed);
    }
    h.cleanup();
}

#[test]
#[ignore]
fn survives_a_dropped_connection() {
    let mut h = Harness::new("reconnect");
    let base = h.base.clone();
    let file = h.tmp.join("f.txt");
    write(&file, b"x");
    assert_eq!(h.upload(&file, &base, ConflictStrategy::Skip).status, TransferStatus::Completed);
    assert!(h.conn.is_alive());

    let restarted = Command::new("docker")
        .args(["restart", &env_or("OMNISCP_TEST_CONTAINER", "omniscp-sftp-test")])
        .status()
        .unwrap();
    assert!(restarted.success());
    let mut ready = false;
    for _ in 0..30 {
        let mut probe = for_profile(&h.params).unwrap();
        if probe.connect().is_ok() {
            ready = true;
            break;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    assert!(ready, "server did not come back");

    // Both sessions died without any local sign, and only a real request shows it.
    assert!(!h.conn.is_alive());

    // The worker notices and reconnects instead of failing every job.
    let second = h.tmp.join("g.txt");
    write(&second, b"y");
    let event = h.upload(&second, &base, ConflictStrategy::Skip);
    assert_eq!(event.status, TransferStatus::Completed, "{:?}", event.error);

    h.conn.connect().unwrap();
    h.cleanup();
}

fn options(mode: SyncMode, delete_orphans: bool) -> SyncOptions {
    SyncOptions { mode, excludes: vec![], delete_orphans }
}

fn paths(plan: &SyncPlan) -> Vec<(&str, SyncActionKind)> {
    plan.actions.iter().map(|a| (a.path.as_str(), a.kind)).collect()
}

#[test]
#[ignore]
fn sync_upload_then_nothing_left_to_do() {
    let mut h = Harness::new("sync-up");
    let base = h.base.clone();
    let root = h.tmp.join("root");
    write(&root.join("a.txt"), b"aaa");
    write(&root.join("deep/er/b.txt"), b"bbb");
    std::fs::create_dir_all(root.join("empty")).unwrap();
    write(&root.join(".omniscp-1-0.part"), b"leftover");

    let plan = h.plan(&root, &base, &options(SyncMode::Upload, false));
    assert_eq!(paths(&plan), [
        ("a.txt", SyncActionKind::Upload),
        ("deep", SyncActionKind::CreateRemoteDir),
        ("deep/er", SyncActionKind::CreateRemoteDir),
        ("deep/er/b.txt", SyncActionKind::Upload),
        ("empty", SyncActionKind::CreateRemoteDir),
    ]);
    let event = h.run_sync(&root, &base, plan.actions);
    assert_eq!(event.status, TransferStatus::Completed, "{:?}", event.error);

    // Dates were preserved, so nothing differs any more, in any mode.
    assert!(h.plan(&root, &base, &options(SyncMode::Upload, false)).actions.is_empty());
    assert!(h.plan(&root, &base, &options(SyncMode::Both, false)).actions.is_empty());
    h.cleanup();
}

#[test]
#[ignore]
fn sync_download_creates_missing_parents_then_nothing_left_to_do() {
    let mut h = Harness::new("sync-down");
    let base = h.base.clone();
    let seed = h.tmp.join("seed");
    write(&seed.join("deep/b.txt"), b"bbb");
    assert_eq!(h.run_sync(&seed, &base, h.plan(&seed, &base, &options(SyncMode::Upload, false)).actions).status, TransferStatus::Completed);

    let root = h.tmp.join("root");
    std::fs::create_dir_all(&root).unwrap();
    // Only the file is ticked: its parent must be created anyway.
    let plan = h.plan(&root, &base, &options(SyncMode::Download, false));
    let only_file: Vec<SyncAction> = plan.actions.into_iter().filter(|a| a.kind == SyncActionKind::Download).collect();
    assert_eq!(h.run_sync(&root, &base, only_file).status, TransferStatus::Completed);
    assert_eq!(std::fs::read(root.join("deep/b.txt")).unwrap(), b"bbb");
    assert!(h.plan(&root, &base, &options(SyncMode::Both, false)).actions.is_empty());
    h.cleanup();
}

#[test]
#[ignore]
fn sync_deletes_only_when_asked_and_keeps_excluded_content() {
    let mut h = Harness::new("sync-delete");
    let base = h.base.clone();
    let seed = h.tmp.join("seed");
    write(&seed.join("keep.txt"), b"k");
    write(&seed.join("gone.txt"), b"g");
    write(&seed.join("olddir/x.txt"), b"x");
    write(&seed.join("mixed/y.txt"), b"y");
    write(&seed.join("mixed/secret.env"), b"s");
    h.run_sync(&seed, &base, h.plan(&seed, &base, &options(SyncMode::Upload, false)).actions);

    let root = h.tmp.join("root");
    write(&root.join("keep.txt"), b"k");
    let keep_time = std::fs::metadata(seed.join("keep.txt")).unwrap().modified().unwrap();
    std::fs::File::options().write(true).open(root.join("keep.txt")).unwrap().set_modified(keep_time).unwrap();

    assert!(h.plan(&root, &base, &options(SyncMode::Upload, false)).actions.is_empty());

    let delete = SyncOptions { mode: SyncMode::Upload, excludes: vec!["*.env".into()], delete_orphans: true };
    let plan = h.plan(&root, &base, &delete);
    assert_eq!(paths(&plan), [
        ("gone.txt", SyncActionKind::DeleteRemote),
        ("mixed/y.txt", SyncActionKind::DeleteRemote),
        ("olddir", SyncActionKind::DeleteRemote),
        ("olddir/x.txt", SyncActionKind::DeleteRemote),
    ]);
    let event = h.run_sync(&root, &base, plan.actions);
    assert_eq!(event.status, TransferStatus::Completed, "{:?}", event.error);

    let mut names = h.remote_names(&base);
    names.sort();
    assert_eq!(names, ["keep.txt", "mixed"]);
    assert_eq!(h.remote_names(&format!("{base}/mixed")), ["secret.env"]);
    h.cleanup();
}

#[test]
#[ignore]
fn sync_leaves_files_changed_since_the_preview() {
    let mut h = Harness::new("sync-changed");
    let base = h.base.clone();
    let root = h.tmp.join("root");
    write(&root.join("a.txt"), b"old");
    write(&root.join("b.txt"), b"old");
    h.run_sync(&root, &base, h.plan(&root, &base, &options(SyncMode::Upload, false)).actions);

    // Local changes, planned for upload...
    std::thread::sleep(Duration::from_secs(3));
    write(&root.join("a.txt"), b"local");
    write(&root.join("b.txt"), b"local");
    let plan = h.plan(&root, &base, &options(SyncMode::Upload, false));
    assert_eq!(plan.actions.len(), 2);

    // ...then someone else writes a.txt on the server before execution.
    let other = h.tmp.join("other/a.txt");
    write(&other, b"someone else");
    h.conn.upload(&other, &format!("{base}/a.txt"), &mut |_| true).unwrap();

    let event = h.run_sync(&root, &base, plan.actions);
    assert_eq!(event.status, TransferStatus::CompletedWithWarnings);
    assert!(event.error.unwrap().contains("a.txt"));

    let back = h.tmp.join("back");
    h.conn.download(&format!("{base}/a.txt"), &back, &mut |_| true).unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), b"someone else");
    h.conn.download(&format!("{base}/b.txt"), &back, &mut |_| true).unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), b"local");
    h.cleanup();
}

#[cfg(unix)]
#[test]
#[ignore]
fn sync_does_not_delete_what_it_could_not_read() {
    use std::os::unix::fs::PermissionsExt;

    let mut h = Harness::new("sync-unreadable");
    let base = h.base.clone();
    let root = h.tmp.join("root");
    write(&root.join("locked/a.txt"), b"a");
    h.run_sync(&root, &base, h.plan(&root, &base, &options(SyncMode::Upload, false)).actions);

    let locked = root.join("locked");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let plan = h.plan(&root, &base, &options(SyncMode::Upload, true));
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert!(plan.actions.is_empty(), "{:?}", plan.actions);
    assert_eq!(plan.problems.len(), 1);
    assert!(plan.problems[0].contains("locked"));
    h.cleanup();
}

#[cfg(unix)]
#[test]
#[ignore]
fn retry_copies_only_what_is_missing() {
    use std::os::unix::fs::PermissionsExt;

    let mut h = Harness::new("retry");
    let base = h.base.clone();
    let source = h.tmp.join("tree");
    write(&source.join("good.txt"), b"first");
    write(&source.join("locked.txt"), b"late");
    let locked = source.join("locked.txt");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    assert_eq!(h.upload(&source, &base, ConflictStrategy::Skip).status, TransferStatus::CompletedWithWarnings);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();

    // Changed locally since: the retry must not overwrite what is there.
    write(&source.join("good.txt"), b"second");
    let event = h.upload(&source, &base, ConflictStrategy::Merge);
    assert_eq!(event.status, TransferStatus::Completed, "{:?}", event.error);

    let back = h.tmp.join("back");
    h.conn.download(&format!("{base}/tree/good.txt"), &back, &mut |_| true).unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), b"first");
    h.conn.download(&format!("{base}/tree/locked.txt"), &back, &mut |_| true).unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), b"late");

    // Nothing missing any more.
    assert_eq!(h.upload(&source, &base, ConflictStrategy::Merge).status, TransferStatus::Skipped);
    h.cleanup();
}

#[test]
#[ignore]
fn connection_test_checks_login_and_starting_folder() {
    let h = Harness::new("probe");
    let with = |change: &dyn Fn(&mut ConnectParams)| {
        let mut params = h.params.clone();
        change(&mut params);
        probe(&params)
    };

    let home = with(&|_| {}).unwrap();
    assert!(home.initial_path.starts_with('/'), "{}", home.initial_path);
    assert_eq!(home.directory_error, None);

    let folder = with(&|p| p.profile.remote_directory = Some(h.base.clone())).unwrap();
    assert_eq!(folder.initial_path, h.base);
    assert_eq!(folder.directory_error, None);

    // The login works, the folder doesn't: reported, not an error.
    let missing = with(&|p| p.profile.remote_directory = Some(format!("{}/missing", h.base))).unwrap();
    assert!(missing.directory_error.is_some());

    assert!(matches!(with(&|p| p.password = Some("wrong".into())), Err(OmniError::Auth(_))));

    let unknown = with(&|p| {
        p.known_hosts_path = h.tmp.join("other_known_hosts");
        p.trusted_fingerprint = None;
    });
    assert!(matches!(unknown, Err(OmniError::HostKeyUnknown(_))), "{unknown:?}");
}

#[test]
#[ignore]
fn pausing_holds_jobs_and_keeps_the_session() {
    let mut h = Harness::new("pause");
    let base = h.base.clone();
    let big = h.tmp.join("big.bin");
    let size = 32 * 1024 * 1024;
    write(&big, &vec![3u8; size]);
    let small = h.tmp.join("small.txt");
    write(&small, b"small");
    let up = |h: &mut Harness, path: &Path| {
        h.enqueue(TransferDirection::Upload, path.to_str().unwrap(), &base, ConflictStrategy::Overwrite)
    };

    // One worker: a paused running job holds it, a paused waiting job is passed over.
    let a = up(&mut h, &big);
    let b = up(&mut h, &small);
    let c = up(&mut h, &small);
    h.queue.pause(&c);
    h.wait_status(&a, TransferStatus::InProgress);
    h.queue.pause(&a);
    let paused = h.wait_status(&a, TransferStatus::Paused);
    assert!(paused.transferred_bytes < size as u64);
    // Longer than the keepalive interval, so the session is pinged mid-transfer.
    h.assert_quiet(Duration::from_secs(17));

    h.queue.resume(&a);
    let done = h.wait_finished(&a);
    assert_eq!(done.status, TransferStatus::Completed, "{:?}", done.error);
    assert_eq!(h.conn.stat(&format!("{base}/big.bin")).unwrap().size, size as u64);
    assert_eq!(h.wait_finished(&b).status, TransferStatus::Completed);
    h.assert_quiet(Duration::from_secs(1));
    h.queue.resume(&c);
    assert_eq!(h.wait_finished(&c).status, TransferStatus::Completed);

    // The whole queue: nothing starts until it is released.
    h.queue.set_paused(true);
    let d = up(&mut h, &small);
    h.assert_quiet(Duration::from_secs(1));
    h.queue.set_paused(false);
    assert_eq!(h.wait_finished(&d).status, TransferStatus::Completed);

    // A held job can still be cancelled.
    h.queue.set_paused(true);
    let e = up(&mut h, &small);
    h.queue.pause(&e);
    h.wait_status(&e, TransferStatus::Paused);
    h.queue.cancel(&e);
    assert_eq!(h.wait_finished(&e).status, TransferStatus::Cancelled);
    h.queue.set_paused(false);
    h.cleanup();
}

#[test]
#[ignore]
fn moving_removes_the_source_only_when_everything_was_copied() {
    let mut h = Harness::new("move");
    let base = h.base.clone();
    let tree = h.tmp.join("tree");
    write(&tree.join("a.txt"), b"aaa");
    write(&tree.join("sub/b.txt"), b"bbb");

    let id = h.enqueue_with(TransferDirection::Upload, tree.to_str().unwrap(), &base, ConflictStrategy::Skip, true);
    assert_eq!(h.wait_finished(&id).status, TransferStatus::Completed);
    assert!(!tree.exists(), "local source should be gone");
    assert_eq!(h.remote_names(&format!("{base}/tree/sub")), ["b.txt"]);

    let back = h.tmp.join("back");
    std::fs::create_dir_all(&back).unwrap();
    let id = h.enqueue_with(TransferDirection::Download, &format!("{base}/tree"), back.to_str().unwrap(), ConflictStrategy::Skip, true);
    assert_eq!(h.wait_finished(&id).status, TransferStatus::Completed);
    assert!(!h.conn.exists(&format!("{base}/tree")), "remote source should be gone");
    assert_eq!(std::fs::read(back.join("tree/sub/b.txt")).unwrap(), b"bbb");

    // One unreadable file: the copy is partial, so nothing at all is removed.
    let partial = h.tmp.join("partial");
    write(&partial.join("ok.txt"), b"ok");
    write(&partial.join("locked.txt"), b"locked");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(partial.join("locked.txt"), std::fs::Permissions::from_mode(0o000)).unwrap();
    let id = h.enqueue_with(TransferDirection::Upload, partial.to_str().unwrap(), &base, ConflictStrategy::Skip, true);
    let event = h.wait_finished(&id);
    std::fs::set_permissions(partial.join("locked.txt"), std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(event.status, TransferStatus::CompletedWithWarnings);
    assert!(partial.join("ok.txt").exists() && partial.join("locked.txt").exists());
    h.cleanup();
}

#[test]
#[ignore]
fn editing_sends_saves_back_and_detects_changes_on_the_server() {
    use omniscp_lib::edit::Edits;

    let h = Harness::new("edit");
    let base = h.base.clone();
    let remote = format!("{base}/notes.txt");
    let seed = h.tmp.join("seed.txt");
    write(&seed, b"version 1");
    h.conn.upload(&seed, &remote, &mut |_| true).unwrap();
    let conn = h.conn.as_ref();

    let edits = Edits::default();
    let root = h.tmp.join("edits");
    let info = edits.open(conn, &remote, &root, "e1".into()).unwrap();
    let local = PathBuf::from(&info.local_path);
    assert_eq!(std::fs::read(&local).unwrap(), b"version 1");
    // Opening it again reuses the copy, so nothing unsent is lost.
    assert_eq!(edits.open(conn, &remote, &root, "e2".into()).unwrap().id, "e1");
    assert!(edits.open(conn, &base, &root, "e3".into()).is_err(), "a folder cannot be edited");
    assert!(edits.poll().is_empty());

    // A save is reported once it stayed the same for one look.
    std::fs::write(&local, b"version 2, longer").unwrap();
    assert!(edits.poll().is_empty());
    let changed = edits.poll();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].remote_path, remote);
    assert!(edits.poll().is_empty());
    edits.upload(conn, "e1", false).unwrap();
    let back = h.tmp.join("back.txt");
    conn.download(&remote, &back, &mut |_| true).unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), b"version 2, longer");

    // Someone else changes the server's copy: the next save is held back.
    write(&seed, b"their version");
    conn.upload(&seed, &remote, &mut |_| true).unwrap();
    std::fs::write(&local, b"version 3").unwrap();
    assert!(matches!(edits.upload(conn, "e1", false), Err(OmniError::Conflict(_))));
    edits.upload(conn, "e1", true).unwrap();
    conn.download(&remote, &back, &mut |_| true).unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), b"version 3");

    // Deleted on the server: also a conflict, never silently recreated.
    conn.delete(&remote).unwrap();
    assert!(matches!(edits.upload(conn, "e1", false), Err(OmniError::Conflict(_))));

    edits.stop("e1");
    assert!(edits.upload(conn, "e1", true).is_err());
    assert!(local.exists(), "the local copy is kept");
    h.cleanup();
}
