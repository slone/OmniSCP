//! Live tests against real FTP servers; ignored by default. Two servers cover
//! what varies most: vsftpd only lists with `LIST` and serves FTPS with a
//! self-signed certificate, ProFTPD lists with MLSD and sets dates with MFMT.
//!
//!   cd tests/ftp-server
//!   docker build -t omniscp-ftp-vsftpd -f Dockerfile.vsftpd .
//!   docker build -t omniscp-ftp-proftpd -f Dockerfile.proftpd .
//!   docker run -d --init --name omniscp-ftp-vsftpd -p 2121:21 -p 2990:990 -p 30000-30009:30000-30009 omniscp-ftp-vsftpd
//!   docker run -d --init --name omniscp-ftp-proftpd -p 2131:21 -p 30010-30019:30010-30019 omniscp-ftp-proftpd
//!
//! (`--init`: vsftpd, as the container's first process, dies with a session that crashes.)
//!
//! Then, for each setup:
//!
//!   OMNISCP_TEST_PROTOCOL=ftp  OMNISCP_TEST_PORT=2131 OMNISCP_TEST_CONTAINER=omniscp-ftp-proftpd ...
//!   OMNISCP_TEST_PROTOCOL=ftp  OMNISCP_TEST_PORT=2121 OMNISCP_TEST_CONTAINER=omniscp-ftp-vsftpd ...
//!   OMNISCP_TEST_PROTOCOL=ftps OMNISCP_TEST_PORT=2121 OMNISCP_TEST_CONTAINER=omniscp-ftp-vsftpd ...
//!   OMNISCP_TEST_PROTOCOL=ftps OMNISCP_TEST_PORT=2990 OMNISCP_TEST_IMPLICIT=1 OMNISCP_TEST_CONTAINER=omniscp-ftp-vsftpd ...
//!
//!   ... OMNISCP_TEST_FINGERPRINT=SHA256:... cargo test --test ftp_live -- --ignored --test-threads=1
//!
//! The certificate's fingerprint (for FTPS) is printed by:
//!
//!   openssl s_client -connect localhost:2121 -starttls ftp </dev/null 2>/dev/null \
//!     | openssl x509 -outform der | openssl dgst -sha256 -binary | base64 | tr -d = | sed 's/^/SHA256:/'

use std::path::PathBuf;
use std::process::Command;

use omniscp_lib::config::{AuthMethod, Profile};
use omniscp_lib::error::OmniError;
use omniscp_lib::protocols::{ConnectParams, Protocol, ProtocolType, for_profile};

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

fn is_ftps() -> bool {
    env_or("OMNISCP_TEST_PROTOCOL", "ftp") == "ftps"
}

fn params(tmp: &std::path::Path, fingerprint: Option<String>, password: &str) -> ConnectParams {
    ConnectParams {
        profile: Profile {
            id: "test".into(),
            name: "test".into(),
            host: "localhost".into(),
            port: env_or("OMNISCP_TEST_PORT", "2131").parse().unwrap(),
            protocol: if is_ftps() { ProtocolType::Ftps } else { ProtocolType::Ftp },
            username: "test".into(),
            auth_method: AuthMethod::Password,
            remote_directory: None,
            local_directory: None,
            description: None,
            tags: vec![],
            group: None,
            save_password: false,
            implicit_tls: env_or("OMNISCP_TEST_IMPLICIT", "") == "1",
        },
        password: Some(password.into()),
        known_hosts_path: tmp.join("known_hosts"),
        trusted_fingerprint: fingerprint,
    }
}

fn scratch(label: &str) -> PathBuf {
    let tmp = std::env::temp_dir().join(format!("omniscp-ftp-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    tmp
}

fn fingerprint() -> Option<String> {
    is_ftps().then(|| std::env::var("OMNISCP_TEST_FINGERPRINT").expect("OMNISCP_TEST_FINGERPRINT"))
}

fn connect(tmp: &std::path::Path) -> Box<dyn Protocol> {
    let mut conn = for_profile(&params(tmp, fingerprint(), "test")).unwrap();
    conn.connect().unwrap();
    conn
}

fn fresh_remote_dir(conn: &dyn Protocol, label: &str) -> String {
    let base = format!("/upload/omniscp-ftp-{label}");
    let _ = conn.delete(&base);
    conn.mkdir(&base).unwrap();
    base
}

/// Runs a shell script as the test account inside the server's container.
fn in_container(script: &str) {
    let status = Command::new("docker")
        .args(["exec", "-u", "test", &env_or("OMNISCP_TEST_CONTAINER", "omniscp-ftp-proftpd"), "sh", "-c", script])
        .status()
        .unwrap();
    assert!(status.success(), "container script failed: {script}");
}

#[test]
#[ignore]
fn certificate_is_confirmed_then_remembered() {
    if !is_ftps() {
        return;
    }
    let tmp = scratch("cert");
    let fingerprint = fingerprint().unwrap();

    // A self-signed certificate is refused until the user confirms it.
    let mut conn = for_profile(&params(&tmp, None, "test")).unwrap();
    match conn.connect() {
        Err(OmniError::HostKeyUnknown(fp)) => assert_eq!(fp, fingerprint),
        other => panic!("expected HostKeyUnknown, got {:?}", other.err()),
    }
    // Confirmed, it connects and is remembered.
    for_profile(&params(&tmp, Some(fingerprint.clone()), "test")).unwrap().connect().unwrap();
    let mut conn = for_profile(&params(&tmp, None, "test")).unwrap();
    conn.connect().unwrap();
    assert!(conn.is_alive());
    conn.disconnect().unwrap();

    // A different certificate later is refused as a change.
    let known = tmp.join("known_certificates");
    let text = std::fs::read_to_string(&known).unwrap();
    assert!(text.contains(&fingerprint), "{text}");
    std::fs::write(&known, text.replace(&fingerprint, "SHA256:somethingelse")).unwrap();
    let mut conn = for_profile(&params(&tmp, None, "test")).unwrap();
    match conn.connect() {
        Err(OmniError::HostKeyMismatch(fp)) => assert_eq!(fp, fingerprint),
        other => panic!("expected HostKeyMismatch, got {:?}", other.err()),
    }
    std::fs::remove_dir_all(&tmp).unwrap();
}

#[test]
#[ignore]
fn wrong_password_is_an_authentication_error() {
    let tmp = scratch("auth");
    let mut conn = for_profile(&params(&tmp, fingerprint(), "wrong")).unwrap();
    assert!(matches!(conn.connect(), Err(OmniError::Auth(_))));
    std::fs::remove_dir_all(&tmp).unwrap();
}

#[test]
#[ignore]
fn file_operations_round_trip() {
    let tmp = scratch("files");
    let conn = connect(&tmp);
    assert_eq!(conn.realpath(".").unwrap(), "/");
    let base = fresh_remote_dir(conn.as_ref(), "files");
    conn.mkdir(&format!("{base}/sub folder")).unwrap();

    let payload: Vec<u8> = (0..1_000_000u32).map(|i| (i % 251) as u8).collect();
    let local = tmp.join("payload.bin");
    std::fs::write(&local, &payload).unwrap();
    let mtime = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
    std::fs::File::options().write(true).open(&local).unwrap().set_modified(mtime).unwrap();

    let remote = format!("{base}/sub folder/données été.bin");
    let mut reported = 0;
    conn.upload(&local, &remote, &mut |n| {
        reported += n;
        true
    })
    .unwrap();
    assert_eq!(reported, payload.len() as u64);

    // The listing shows it with its size, and its date is kept to the second.
    let listed = conn.list_dir(&format!("{base}/sub folder")).unwrap();
    assert_eq!(listed.len(), 1, "no temporary file may be left: {listed:?}");
    assert_eq!((listed[0].name.as_str(), listed[0].size, listed[0].is_dir), ("données été.bin", 1_000_000, false));
    assert_eq!(conn.exact_modified(&remote, listed[0].modified), Some(1_700_000_000));
    let stat = conn.stat(&remote).unwrap();
    assert_eq!((stat.size, stat.modified), (1_000_000, Some(1_700_000_000)));
    assert!(conn.stat(&format!("{base}/sub folder")).unwrap().is_dir);

    let renamed = format!("{base}/sub folder/renamed.bin");
    conn.rename(&remote, &renamed).unwrap();
    let back = tmp.join("back.bin");
    conn.download(&renamed, &back, &mut |_| true).unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), payload);
    assert_eq!(std::fs::metadata(&back).unwrap().modified().unwrap(), mtime);

    // Cancelling either way leaves the existing file as it was, and the session usable.
    std::fs::write(&back, b"local copy").unwrap();
    assert!(matches!(conn.download(&renamed, &back, &mut |_| false), Err(OmniError::Cancelled)));
    assert_eq!(std::fs::read(&back).unwrap(), b"local copy");
    let other = tmp.join("other.bin");
    std::fs::write(&other, vec![7u8; 1_000_000]).unwrap();
    let cancelled = conn.upload(&other, &renamed, &mut |_| false);
    assert!(matches!(cancelled, Err(OmniError::Cancelled)), "{cancelled:?}");
    let names: Vec<String> = conn.list_dir(&format!("{base}/sub folder")).unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["renamed.bin"]);
    conn.download(&renamed, &back, &mut |_| true).unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), payload);

    // Replacing a file keeps its permissions.
    conn.set_permissions(&renamed, 0o600).unwrap();
    assert_eq!(conn.stat(&renamed).unwrap().mode, Some(0o600));
    conn.upload(&other, &renamed, &mut |_| true).unwrap();
    let replaced = conn.stat(&renamed).unwrap();
    assert_eq!((replaced.size, replaced.mode), (1_000_000, Some(0o600)));

    // Missing files are told apart from other failures.
    match conn.stat(&format!("{base}/nope")) {
        Err(OmniError::Protocol(m)) => assert_eq!(m.code, "remote.notFound", "{m:?}"),
        other => panic!("expected notFound, got {other:?}"),
    }
    assert!(conn.exists(&renamed));
    assert!(!conn.exists(&format!("{base}/nope")));

    conn.delete(&base).unwrap();
    assert!(!conn.exists(&base));
    std::fs::remove_dir_all(&tmp).unwrap();
}

#[test]
#[ignore]
fn listings_show_hidden_files_links_and_odd_names() {
    let tmp = scratch("list");
    let conn = connect(&tmp);
    let base = fresh_remote_dir(conn.as_ref(), "list");
    in_container(&format!(
        "cd ~{base} && mkdir 'a dir' && echo x > .hidden && echo y > '-dash' && echo z > 'two  spaces' \
         && ln -s 'a dir' dirlink && ln -s .hidden filelink && touch -d @1600000000 .hidden"
    ));

    let entries = conn.list_dir(&base).unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["a dir", "dirlink", "-dash", ".hidden", "filelink", "two  spaces"]);
    let find = |name: &str| entries.iter().find(|e| e.name == name).unwrap();
    assert!(find("dirlink").is_symlink && find("dirlink").is_dir);
    assert!(find("filelink").is_symlink && !find("filelink").is_dir);
    assert_eq!(conn.read_link(&format!("{base}/dirlink")).unwrap(), "a dir");
    let hidden = find(".hidden");
    assert_eq!(conn.exact_modified(&hidden.path.to_string_lossy(), hidden.modified), Some(1_600_000_000));
    assert_eq!(conn.list_dir(&format!("{base}/dirlink")).unwrap().len(), 0);

    // A link to a folder is removed as a link, never followed.
    conn.delete(&format!("{base}/dirlink")).unwrap();
    assert!(conn.exists(&format!("{base}/a dir")));
    conn.delete(&base).unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
}
