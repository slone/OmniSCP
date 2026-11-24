//! Live tests of SCP against real servers; ignored by default. Run them on each
//! server: their `ls` differ (GNU coreutils, busybox).
//!
//!   docker build -t omniscp-scp-debian -f tests/scp-server/Dockerfile.debian tests/scp-server
//!   docker run -d --name omniscp-scp-debian -p 2223:22 omniscp-scp-debian
//!   docker build -t omniscp-scp-alpine -f tests/scp-server/Dockerfile.alpine tests/scp-server
//!   docker run -d --name omniscp-scp-alpine -p 2224:22 omniscp-scp-alpine
//!
//!   OMNISCP_TEST_PORT=2223 OMNISCP_TEST_FINGERPRINT=SHA256:... cargo test --test scp_live -- --ignored
//!
//! The fingerprint is printed by: ssh-keyscan -p 2223 localhost | ssh-keygen -lf -

use std::path::PathBuf;

use omniscp_lib::config::{AuthMethod, Profile};
use omniscp_lib::error::OmniError;
use omniscp_lib::protocols::scp::ScpConnection;
use omniscp_lib::protocols::{ConnectParams, Protocol, ProtocolType};

fn params(port: u16, known_hosts: PathBuf) -> ConnectParams {
    ConnectParams {
        profile: Profile {
            id: "test".into(),
            name: "test".into(),
            host: "localhost".into(),
            port,
            protocol: ProtocolType::Scp,
            username: "test".into(),
            auth_method: AuthMethod::Password,
            remote_directory: None,
            local_directory: None,
            description: None,
            tags: vec![],
            group: None,
            save_password: false,
            implicit_tls: false,
        },
        password: Some("test".into()),
        known_hosts_path: known_hosts,
        trusted_fingerprint: Some(std::env::var("OMNISCP_TEST_FINGERPRINT").expect("OMNISCP_TEST_FINGERPRINT")),
    }
}

/// A connection and a fresh folder on the server and locally.
fn setup(label: &str) -> (ScpConnection, String, PathBuf) {
    let port = std::env::var("OMNISCP_TEST_PORT").expect("OMNISCP_TEST_PORT").parse().unwrap();
    let tmp = std::env::temp_dir().join(format!("omniscp-scp-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let mut conn = ScpConnection::new(&params(port, tmp.join("known_hosts")));
    conn.connect().unwrap();
    let base = format!("/home/test/omniscp-scp-{label}");
    let _ = conn.delete(&base);
    conn.mkdir(&base).unwrap();
    (conn, base, tmp)
}

/// Commands the app never runs, to set up what it could not create itself.
fn server_shell(command: &str) {
    let port: u16 = std::env::var("OMNISCP_TEST_PORT").unwrap().parse().unwrap();
    let mut session = ssh2::Session::new().unwrap();
    session.set_tcp_stream(std::net::TcpStream::connect(("localhost", port)).unwrap());
    session.handshake().unwrap();
    session.userauth_password("test", "test").unwrap();
    let mut channel = session.channel_session().unwrap();
    channel.exec(command).unwrap();
    channel.send_eof().unwrap();
    let mut output = String::new();
    std::io::Read::read_to_string(&mut channel, &mut output).unwrap();
    channel.wait_close().unwrap();
    assert_eq!(channel.exit_status().unwrap(), 0, "{command}");
}

fn names(conn: &ScpConnection, dir: &str) -> Vec<String> {
    conn.list_dir(dir).unwrap().into_iter().map(|e| e.name).collect()
}

#[test]
#[ignore]
fn file_operations_round_trip() {
    let (conn, base, tmp) = setup("basics");
    assert_eq!(conn.realpath(&base).unwrap(), base);
    assert!(conn.is_alive());

    let payload: Vec<u8> = (0..1_000_000u32).map(|i| (i % 251) as u8).collect();
    let local = tmp.join("payload.bin");
    std::fs::write(&local, &payload).unwrap();

    let remote = format!("{base}/payload.bin");
    let mut reported = 0;
    conn.upload(&local, &remote, &mut |n| {
        reported += n;
        true
    })
    .unwrap();
    assert_eq!(reported, payload.len() as u64);
    assert_eq!(names(&conn, &base), ["payload.bin"], "no temporary file may be left behind");

    let entry = conn.stat(&remote).unwrap();
    assert_eq!(entry.size, payload.len() as u64);
    assert!(!entry.is_dir && !entry.is_symlink);

    // The source's date survives, to the second, both ways.
    let local_mtime = std::fs::metadata(&local).unwrap().modified().unwrap();
    let local_secs = local_mtime.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    assert_eq!(entry.modified, Some(local_secs));

    conn.rename(&remote, &format!("{base}/renamed.bin")).unwrap();
    let back = tmp.join("back.bin");
    conn.download(&format!("{base}/renamed.bin"), &back, &mut |_| true).unwrap();
    assert!(std::fs::read(&back).unwrap() == payload, "content must round-trip");
    assert_eq!(std::fs::metadata(&back).unwrap().modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64, local_secs);

    // An empty file is a file too.
    let empty = tmp.join("empty");
    std::fs::write(&empty, b"").unwrap();
    conn.upload(&empty, &format!("{base}/empty"), &mut |_| true).unwrap();
    conn.download(&format!("{base}/empty"), &back, &mut |_| true).unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), b"");

    conn.delete(&base).unwrap();
    assert!(!conn.exists(&base));
    std::fs::remove_dir_all(&tmp).unwrap();
}

#[test]
#[ignore]
fn awkward_names_survive_and_never_reach_the_shell() {
    let (conn, base, tmp) = setup("names");
    let canary = format!("{base}/CANARY");
    let source = tmp.join("source");
    std::fs::write(&source, b"data").unwrap();

    let awkward = [
        "with space".to_string(),
        "two  spaces".to_string(),
        " leading".to_string(),
        "trailing ".to_string(),
        "it's".to_string(),
        "\"double\"".to_string(),
        "-rf".to_string(),
        "--help".to_string(),
        "a -> b".to_string(),
        "star*".to_string(),
        "caf\u{e9} \u{65e5}\u{672c}\u{8a9e}".to_string(),
        "back\\slash".to_string(),
        format!("$(touch {canary})"),
        format!("`touch {canary}`"),
        format!("; touch {canary};"),
        format!("'; touch {canary}; '"),
    ];
    for name in &awkward {
        // `/` cannot appear in a name, so these hold the path in a parent-less way.
        let name = name.replace('/', "_");
        let remote = format!("{base}/{name}");
        conn.upload(&source, &remote, &mut |_| true).unwrap_or_else(|e| panic!("upload {name:?}: {e}"));
        let entry = conn.stat(&remote).unwrap_or_else(|e| panic!("stat {name:?}: {e}"));
        assert_eq!(entry.size, 4, "{name:?}");

        let back = tmp.join("back");
        conn.download(&remote, &back, &mut |_| true).unwrap_or_else(|e| panic!("download {name:?}: {e}"));
        assert_eq!(std::fs::read(&back).unwrap(), b"data", "{name:?}");

        let renamed = format!("{remote}.renamed");
        conn.rename(&remote, &renamed).unwrap_or_else(|e| panic!("rename {name:?}: {e}"));
        conn.rename(&renamed, &remote).unwrap();
    }

    let listed = names(&conn, &base);
    assert!(!listed.iter().any(|n| n == "CANARY"), "a file name was executed: {listed:?}");
    for name in &awkward {
        let name = name.replace('/', "_");
        assert!(listed.contains(&name), "{name:?} missing from {listed:?}");
    }

    // Deleting them one by one leaves nothing, and nothing else was touched.
    for name in &awkward {
        conn.delete(&format!("{base}/{}", name.replace('/', "_"))).unwrap();
    }
    assert_eq!(names(&conn, &base), Vec::<String>::new());

    conn.delete(&base).unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
}

#[test]
#[ignore]
fn folders_with_awkward_names_work_too() {
    let (conn, base, tmp) = setup("dirs");
    let dir = format!("{base}/a dir's $(name)");
    conn.mkdir(&dir).unwrap();
    assert!(conn.stat(&dir).unwrap().is_dir);
    conn.mkdir(&format!("{dir}/inner")).unwrap();

    let source = tmp.join("source");
    std::fs::write(&source, b"x").unwrap();
    conn.upload(&source, &format!("{dir}/inner/f"), &mut |_| true).unwrap();
    assert_eq!(names(&conn, &format!("{dir}/inner")), ["f"]);
    assert_eq!(conn.realpath(&dir).unwrap(), dir);

    // Only an empty folder can go this way.
    assert!(conn.remove_empty_dir(&dir).is_err());
    assert!(conn.exists(&format!("{dir}/inner/f")));

    conn.delete(&dir).unwrap();
    assert!(!conn.exists(&dir));
    conn.delete(&base).unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
}

#[test]
#[ignore]
fn a_name_with_a_newline_works_or_fails_loudly() {
    let (conn, base, tmp) = setup("newline");
    server_shell(&format!("touch '{base}/first\nsecond' '{base}/plain'"));

    // Either understood exactly (GNU, escaped), or the whole listing is refused: an
    // entry silently missing would make sync believe the file was deleted.
    match conn.list_dir(&base) {
        Ok(entries) => {
            let mut names: Vec<String> = entries.into_iter().map(|e| e.name).collect();
            names.sort();
            assert_eq!(names, ["first\nsecond", "plain"]);
        }
        Err(e) => assert!(matches!(e, OmniError::Protocol(_)), "{e}"),
    }

    conn.delete(&base).unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
}

#[test]
#[ignore]
fn cancelling_keeps_the_existing_file() {
    let (conn, base, tmp) = setup("cancel");
    let remote = format!("{base}/file.bin");
    let (old, new) = (tmp.join("old.bin"), tmp.join("new.bin"));
    std::fs::write(&old, vec![1u8; 1_000_000]).unwrap();
    std::fs::write(&new, vec![2u8; 1_000_000]).unwrap();
    conn.upload(&old, &remote, &mut |_| true).unwrap();

    assert!(matches!(conn.upload(&new, &remote, &mut |_| false), Err(OmniError::Cancelled)));
    assert_eq!(names(&conn, &base), ["file.bin"], "no temporary file may be left behind");

    // The session is still usable, and so is the file.
    let back = tmp.join("back.bin");
    conn.download(&remote, &back, &mut |_| true).unwrap();
    assert!(std::fs::read(&back).unwrap() == std::fs::read(&old).unwrap(), "original must survive");

    // Same for a download.
    std::fs::write(&back, b"precious").unwrap();
    assert!(matches!(conn.download(&remote, &back, &mut |_| false), Err(OmniError::Cancelled)));
    assert!(std::fs::read(&back).unwrap() == b"precious", "original must survive");
    assert!(conn.is_alive());
    assert_eq!(names(&conn, &base), ["file.bin"]);

    conn.delete(&base).unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
}

#[test]
#[ignore]
fn overwrite_replaces_content_and_keeps_permissions() {
    let (conn, base, tmp) = setup("overwrite");
    let remote = format!("{base}/script.sh");
    let (old, new) = (tmp.join("old"), tmp.join("new"));
    std::fs::write(&old, b"old").unwrap();
    std::fs::write(&new, b"new content").unwrap();
    conn.upload(&old, &remote, &mut |_| true).unwrap();
    server_shell(&format!("chmod 750 '{remote}'"));

    conn.upload(&new, &remote, &mut |_| true).unwrap();

    let entries = conn.list_dir(&base).unwrap();
    assert_eq!(entries.len(), 1, "temporary files must be gone");
    assert_eq!(entries[0].permissions.as_deref(), Some("-rwxr-x---"));
    let back = tmp.join("back");
    conn.download(&remote, &back, &mut |_| true).unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), b"new content");

    // A file cannot replace a directory, directly or through a link.
    conn.mkdir(&format!("{base}/dir")).unwrap();
    server_shell(&format!("ln -s dir '{base}/link'"));
    for target in ["dir", "link"] {
        let result = conn.upload(&new, &format!("{base}/{target}"), &mut |_| true);
        assert!(matches!(result, Err(OmniError::Transfer(_))), "{target}");
    }
    assert_eq!(names(&conn, &format!("{base}/dir")), Vec::<String>::new());

    conn.delete(&base).unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
}

#[test]
#[ignore]
fn symlinks_are_described_and_deleted_as_links() {
    let (conn, base, tmp) = setup("symlink");
    conn.mkdir(&format!("{base}/real")).unwrap();
    let source = tmp.join("source");
    std::fs::write(&source, b"12345").unwrap();
    conn.upload(&source, &format!("{base}/real/inner"), &mut |_| true).unwrap();
    server_shell(&format!(
        "cd '{base}' && ln -s real link-to-dir && ln -s real/inner link-to-file && ln -s nowhere dangling"
    ));

    let entries = conn.list_dir(&base).unwrap();
    let get = |name: &str| entries.iter().find(|e| e.name == name).unwrap();
    assert!(get("link-to-dir").is_dir && get("link-to-dir").is_symlink);
    assert!(!get("link-to-file").is_dir && get("link-to-file").is_symlink);
    assert_eq!(get("link-to-file").size, 5);
    assert!(!get("dangling").is_dir && get("dangling").is_symlink);

    // Browsing through the link works, and reading through a file link gets the content.
    assert_eq!(names(&conn, &format!("{base}/link-to-dir")), ["inner"]);
    let back = tmp.join("back");
    conn.download(&format!("{base}/link-to-file"), &back, &mut |_| true).unwrap();
    assert_eq!(std::fs::read(&back).unwrap(), b"12345");

    // Deleting a link leaves its target.
    conn.delete(&format!("{base}/link-to-dir")).unwrap();
    conn.delete(&format!("{base}/link-to-file")).unwrap();
    assert!(conn.exists(&format!("{base}/real/inner")));

    conn.delete(&base).unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
}

#[test]
#[ignore]
fn permission_problems_are_reported_as_such() {
    let (conn, base, tmp) = setup("permissions");
    let source = tmp.join("source");
    std::fs::write(&source, b"x").unwrap();

    assert!(matches!(conn.mkdir("/forbidden"), Err(OmniError::Permission(_))));
    assert!(matches!(conn.list_dir("/root"), Err(OmniError::Permission(_))));
    assert!(matches!(conn.delete("/etc/hostname"), Err(OmniError::Permission(_))));
    assert!(conn.exists("/etc/hostname"));
    let refused = conn.upload(&source, "/etc/omniscp-test", &mut |_| true);
    assert!(refused.is_err(), "uploading to /etc must fail");
    assert!(!conn.exists("/etc/omniscp-test"));

    assert!(matches!(conn.stat("/no/such/path"), Err(OmniError::Protocol(_))));
    assert!(!conn.exists("/no/such/path"));
    let back = tmp.join("back");
    assert!(conn.download("/no/such/path", &back, &mut |_| true).is_err());
    assert!(!back.exists());

    conn.delete(&base).unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
}

#[test]
#[ignore]
fn large_file_round_trip() {
    let (conn, base, tmp) = setup("large");
    // Larger than SSH's flow-control window, with content that would reveal reordering.
    let payload: Vec<u8> = (0..64 * 1024 * 1024u32).map(|i| (i ^ (i >> 8) ^ (i >> 16)) as u8).collect();
    let local = tmp.join("large.bin");
    std::fs::write(&local, &payload).unwrap();

    let remote = format!("{base}/large.bin");
    conn.upload(&local, &remote, &mut |_| true).unwrap();
    assert_eq!(conn.stat(&remote).unwrap().size, payload.len() as u64);

    let back = tmp.join("back.bin");
    conn.download(&remote, &back, &mut |_| true).unwrap();
    assert!(std::fs::read(&back).unwrap() == payload, "content must round-trip");

    conn.delete(&base).unwrap();
    std::fs::remove_dir_all(&tmp).unwrap();
}

/// Set OMNISCP_TEST_SFTP_ONLY_PORT and the fingerprint of the server of sftp_live.rs.
#[test]
#[ignore]
fn a_server_without_a_shell_is_refused_quickly_and_clearly() {
    let Ok(port) = std::env::var("OMNISCP_TEST_SFTP_ONLY_PORT") else { return };
    let tmp = std::env::temp_dir().join(format!("omniscp-scp-sftp-only-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let mut conn = ScpConnection::new(&params(port.parse().unwrap(), tmp.join("known_hosts")));

    let started = std::time::Instant::now();
    let refused = conn.connect();
    assert!(started.elapsed().as_secs() < 10, "took {:?}", started.elapsed());
    match refused {
        Err(OmniError::Protocol(message)) => assert_eq!(message.code, "scp.noShell"),
        other => panic!("expected a refusal, got {:?}", other.err()),
    }
    std::fs::remove_dir_all(&tmp).unwrap();
}
