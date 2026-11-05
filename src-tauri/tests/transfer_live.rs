//! Live tests of the transfer queue against a real SFTP server; ignored by default.
//! Same server as sftp_live.rs. Run them one at a time: one test restarts the container.
//!
//!   OMNISCP_TEST_FINGERPRINT=SHA256:... cargo test --test transfer_live -- --ignored --test-threads=1

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use omniscp_lib::config::{AuthMethod, Profile};
use omniscp_lib::protocols::sftp::{ConnectParams, SftpConnection};
use omniscp_lib::protocols::{Protocol, ProtocolType};
use omniscp_lib::transfer::queue::{TransferEvent, TransferQueue, TransferRequest};
use omniscp_lib::transfer::{ConflictStrategy, TransferDirection, TransferStatus};

struct Harness {
    tmp: PathBuf,
    base: String,
    conn: SftpConnection,
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
                port: 2222,
                protocol: ProtocolType::Sftp,
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
        let mut conn = SftpConnection::new(&params);
        conn.connect().unwrap();
        let base = format!("/upload/omniscp-queue-{label}");
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
            })
            .unwrap();
        id
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

    let restarted = Command::new("docker").args(["restart", "omniscp-sftp-test"]).status().unwrap();
    assert!(restarted.success());
    let mut ready = false;
    for _ in 0..30 {
        let mut probe = SftpConnection::new(&h.params);
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
