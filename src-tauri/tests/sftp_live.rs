//! Live test against a real SFTP server; ignored by default.
//!
//!   docker run -d --name omniscp-sftp-test -p 2222:22 atmoz/sftp test:test:::upload
//!   OMNISCP_TEST_FINGERPRINT=SHA256:... cargo test --test sftp_live -- --ignored
//!
//! The fingerprint is printed by: ssh-keyscan -p 2222 localhost | ssh-keygen -lf -

use omniscp_lib::config::{AuthMethod, Profile};
use omniscp_lib::error::OmniError;
use omniscp_lib::protocols::sftp::SftpConnection;
use omniscp_lib::protocols::{ConnectParams, Protocol, ProtocolType};

fn params(fingerprint: Option<String>, known_hosts: std::path::PathBuf) -> ConnectParams {
    ConnectParams {
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
            group: None,
            save_password: false,
            implicit_tls: false,
        },
        password: Some("test".into()),
        known_hosts_path: known_hosts,
        trusted_fingerprint: fingerprint,
    }
}

#[test]
#[ignore]
fn file_operations_round_trip() {
    let fingerprint = std::env::var("OMNISCP_TEST_FINGERPRINT").expect("OMNISCP_TEST_FINGERPRINT");
    let tmp = std::env::temp_dir().join(format!("omniscp-live-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let known_hosts = tmp.join("known_hosts");

    // Unknown host without confirmation is refused, with the fingerprint attached.
    let mut conn = SftpConnection::new(&params(None, known_hosts.clone()));
    match conn.connect() {
        Err(OmniError::HostKeyUnknown(fp)) => assert_eq!(fp, fingerprint),
        other => panic!("expected HostKeyUnknown, got {:?}", other.err()),
    }

    // Confirmed fingerprint connects and is recorded; the next connection needs no confirmation.
    SftpConnection::new(&params(Some(fingerprint.clone()), known_hosts.clone())).connect().unwrap();
    let mut conn = SftpConnection::new(&params(None, known_hosts.clone()));
    conn.connect().unwrap();

    let base = "/upload/omniscp-live";
    let _ = conn.delete(base);
    conn.mkdir(base).unwrap();
    conn.mkdir(&format!("{base}/sub")).unwrap();

    let payload: Vec<u8> = (0..1_000_000u32).map(|i| (i % 251) as u8).collect();
    let local = tmp.join("payload.bin");
    std::fs::write(&local, &payload).unwrap();

    let mut reported = 0;
    conn.upload(&local, &format!("{base}/sub/payload.bin"), &mut |n| {
        reported += n;
        true
    })
    .unwrap();
    assert_eq!(reported, payload.len() as u64);

    conn.rename(&format!("{base}/sub/payload.bin"), &format!("{base}/sub/renamed.bin")).unwrap();
    let back = tmp.join("back.bin");
    conn.download(&format!("{base}/sub/renamed.bin"), &back, &mut |_| true).unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), payload);

    // Cancelling stops after the first chunk.
    let cancelled = conn.download(&format!("{base}/sub/renamed.bin"), &back, &mut |_| false);
    assert!(matches!(cancelled, Err(OmniError::Cancelled)));

    // Permission errors are reported as such (root of the chroot is not writable).
    assert!(matches!(conn.mkdir("/forbidden"), Err(OmniError::Permission(_))));

    conn.delete(base).unwrap();
    assert!(!conn.exists(base));
    conn.disconnect().unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
}

/// Connects with the confirmed fingerprint, using a scratch directory unique to `label`.
fn live_connection(label: &str) -> (SftpConnection, std::path::PathBuf) {
    let fingerprint = std::env::var("OMNISCP_TEST_FINGERPRINT").expect("OMNISCP_TEST_FINGERPRINT");
    let tmp = std::env::temp_dir().join(format!("omniscp-live-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let mut conn = SftpConnection::new(&params(Some(fingerprint), tmp.join("known_hosts")));
    conn.connect().unwrap();
    (conn, tmp)
}

fn fresh_remote_dir(conn: &SftpConnection, label: &str) -> String {
    let base = format!("/upload/omniscp-live-{label}");
    let _ = conn.delete(&base);
    conn.mkdir(&base).unwrap();
    base
}

/// Raw session for server-side setup the app itself never does (symlinks, chmod).
fn raw_sftp() -> (ssh2::Session, ssh2::Sftp) {
    let mut session = ssh2::Session::new().unwrap();
    session.set_tcp_stream(std::net::TcpStream::connect("localhost:2222").unwrap());
    session.handshake().unwrap();
    session.userauth_password("test", "test").unwrap();
    let sftp = session.sftp().unwrap();
    (session, sftp)
}

#[test]
#[ignore]
fn cancelled_upload_keeps_the_existing_file() {
    let (conn, tmp) = live_connection("cancel-up");
    let base = fresh_remote_dir(&conn, "cancel-up");
    let remote = format!("{base}/file.bin");
    let (old, new) = (tmp.join("old.bin"), tmp.join("new.bin"));
    std::fs::write(&old, vec![1u8; 1_000_000]).unwrap();
    std::fs::write(&new, vec![2u8; 1_000_000]).unwrap();

    conn.upload(&old, &remote, &mut |_| true).unwrap();
    let cancelled = conn.upload(&new, &remote, &mut |_| false);
    assert!(matches!(cancelled, Err(OmniError::Cancelled)));

    let names: Vec<String> = conn.list_dir(&base).unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["file.bin"], "no temporary file may be left behind");
    let back = tmp.join("back.bin");
    conn.download(&remote, &back, &mut |_| true).unwrap();
    assert!(std::fs::read(&back).unwrap() == std::fs::read(&old).unwrap(), "original must survive");

    conn.delete(&base).unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
}

#[test]
#[ignore]
fn cancelled_download_keeps_the_existing_file() {
    let (conn, tmp) = live_connection("cancel-down");
    let base = fresh_remote_dir(&conn, "cancel-down");
    let remote = format!("{base}/file.bin");
    let source = tmp.join("source.bin");
    std::fs::write(&source, vec![2u8; 1_000_000]).unwrap();
    conn.upload(&source, &remote, &mut |_| true).unwrap();

    let dir = tmp.join("dest");
    std::fs::create_dir_all(&dir).unwrap();
    let existing = dir.join("file.bin");
    std::fs::write(&existing, b"precious").unwrap();

    let cancelled = conn.download(&remote, &existing, &mut |_| false);
    assert!(matches!(cancelled, Err(OmniError::Cancelled)));
    assert!(std::fs::read(&existing).unwrap() == b"precious", "original must survive");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no temporary file may be left behind");

    conn.delete(&base).unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
}

#[test]
#[ignore]
fn overwrite_replaces_content_and_keeps_permissions() {
    let (conn, tmp) = live_connection("overwrite");
    let base = fresh_remote_dir(&conn, "overwrite");
    let remote = format!("{base}/script.sh");
    let (old, new) = (tmp.join("old"), tmp.join("new"));
    std::fs::write(&old, b"old").unwrap();
    std::fs::write(&new, b"new content").unwrap();
    conn.upload(&old, &remote, &mut |_| true).unwrap();

    let (_session, sftp) = raw_sftp();
    let exec = ssh2::FileStat { size: None, uid: None, gid: None, perm: Some(0o750), atime: None, mtime: None };
    sftp.setstat(std::path::Path::new(&remote), exec).unwrap();

    conn.upload(&new, &remote, &mut |_| true).unwrap();

    let entries = conn.list_dir(&base).unwrap();
    assert_eq!(entries.len(), 1, "backup and temporary files must be gone");
    assert_eq!(entries[0].permissions.as_deref(), Some("-rwxr-x---"));
    let back = tmp.join("back");
    conn.download(&remote, &back, &mut |_| true).unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), b"new content");

    // A file cannot replace a directory.
    conn.mkdir(&format!("{base}/dir")).unwrap();
    assert!(matches!(
        conn.upload(&new, &format!("{base}/dir"), &mut |_| true),
        Err(OmniError::Transfer(_))
    ));
    assert_eq!(conn.list_dir(&format!("{base}/dir")).unwrap().len(), 0);

    conn.delete(&base).unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
}

#[test]
#[ignore]
fn symlink_to_directory_is_listed_as_a_directory() {
    let (conn, tmp) = live_connection("symlink");
    let base = fresh_remote_dir(&conn, "symlink");
    conn.mkdir(&format!("{base}/real")).unwrap();
    let (_session, sftp) = raw_sftp();
    sftp.symlink(std::path::Path::new("real"), std::path::Path::new(&format!("{base}/link"))).unwrap();

    let entries = conn.list_dir(&base).unwrap();
    let link = entries.iter().find(|e| e.name == "link").unwrap();
    assert!(link.is_dir, "a symlink to a directory must be navigable");
    assert!(link.is_symlink);
    // Listing through the link works, and deleting removes the link but not the target.
    conn.list_dir(&format!("{base}/link")).unwrap();
    conn.delete(&format!("{base}/link")).unwrap();
    assert!(conn.exists(&format!("{base}/real")));

    conn.delete(&base).unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
}
