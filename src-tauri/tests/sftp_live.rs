//! Live test against a real SFTP server; ignored by default.
//!
//!   docker run -d --name omniscp-sftp-test -p 2222:22 atmoz/sftp test:test:::upload
//!   OMNISCP_TEST_FINGERPRINT=SHA256:... cargo test --test sftp_live -- --ignored
//!
//! The fingerprint is printed by: ssh-keyscan -p 2222 localhost | ssh-keygen -lf -

use omniscp_lib::config::{AuthMethod, Profile};
use omniscp_lib::error::OmniError;
use omniscp_lib::protocols::sftp::{ConnectParams, SftpConnection};
use omniscp_lib::protocols::{Protocol, ProtocolType};

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
            save_password: false,
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
