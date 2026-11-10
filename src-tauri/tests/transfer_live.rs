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

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use omniscp_lib::config::{AuthMethod, Profile};
use omniscp_lib::protocols::{ConnectParams, Protocol, ProtocolType, for_profile};
use omniscp_lib::sync::{self, SyncAction, SyncActionKind, SyncMode, SyncOptions, SyncPlan};
use omniscp_lib::transfer::queue::{SyncRequest, TransferEvent, TransferQueue, TransferRequest};
use omniscp_lib::transfer::{ConflictStrategy, TransferDirection, TransferStatus};

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
        );
        Self { tmp, base, conn, params, queue, events, next_id: 0 }
    }

    fn enqueue(&mut self, direction: TransferDirection, source: &str, dest_dir: &str, conflict: ConflictStrategy) -> String {
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
            if event.id == id && !matches!(event.status, TransferStatus::Pending | TransferStatus::InProgress) {
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
