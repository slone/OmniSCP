//! Profile files for moving profiles between machines.
//! They never contain secrets: saved passwords stay in the credential store of the
//! machine that saved them, and imported profiles start without one.

use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use super::Profile;
use crate::error::{OmniError, Result};

pub const FORMAT: &str = "omniscp-profiles";
pub const VERSION: u32 = 1;
/// Far above any real profile list; guards against picking the wrong file.
const MAX_FILE: u64 = 8 * 1024 * 1024;

#[derive(Serialize)]
struct ProfileFile<'a> {
    format: &'a str,
    version: u32,
    profiles: Vec<Profile>,
}

/// Writes `profiles` to `path`, replacing it. Returns how many were written.
pub fn export(path: &Path, profiles: &[Profile]) -> Result<usize> {
    let file = ProfileFile {
        format: FORMAT,
        version: VERSION,
        profiles: profiles
            .iter()
            .map(|p| Profile {
                save_password: false,
                ..p.clone()
            })
            .collect(),
    };
    let data = serde_json::to_string_pretty(&file).map_err(|e| OmniError::Config(e.to_string()))?;
    let tmp = path.with_extension("omniscp-tmp");
    std::fs::write(&tmp, data).map_err(|e| OmniError::io_context(tmp.display(), e))?;
    std::fs::rename(&tmp, path).map_err(|e| OmniError::io_context(path.display(), e))?;
    Ok(file.profiles.len())
}

/// Reads a profile file written by `export`, or a bare JSON array of profiles such as
/// `profiles.json`. Each profile gets a fresh id from `new_id`, so nothing existing is replaced.
pub fn import(path: &Path, mut new_id: impl FnMut() -> String) -> Result<Vec<Profile>> {
    let size = std::fs::metadata(path)
        .map_err(|e| OmniError::io_context(path.display(), e))?
        .len();
    if size > MAX_FILE {
        return Err(OmniError::Config(format!("{}: too large for a profile file", path.display())));
    }
    let data = std::fs::read_to_string(path).map_err(|e| OmniError::io_context(path.display(), e))?;
    let value: Value = serde_json::from_str(&data)
        .map_err(|e| OmniError::Config(format!("{}: not valid JSON ({e})", path.display())))?;

    let entries = match value {
        Value::Array(entries) => entries,
        Value::Object(mut file) => {
            if file.get("format").and_then(Value::as_str) != Some(FORMAT) {
                return Err(OmniError::Config(format!("{}: not an OmniSCP profile file", path.display())));
            }
            let version = file.get("version").and_then(Value::as_u64).unwrap_or(0);
            if version > VERSION as u64 {
                return Err(OmniError::Config(format!(
                    "{}: written by a newer version of OmniSCP (format {version})",
                    path.display()
                )));
            }
            match file.remove("profiles") {
                Some(Value::Array(entries)) => entries,
                _ => return Err(OmniError::Config(format!("{}: no profile list", path.display()))),
            }
        }
        _ => return Err(OmniError::Config(format!("{}: not an OmniSCP profile file", path.display()))),
    };

    entries
        .into_iter()
        .enumerate()
        .map(|(i, entry)| {
            let profile: Profile = serde_json::from_value(entry)
                .map_err(|e| OmniError::Config(format!("profile {}: {e}", i + 1)))?;
            let profile = cleaned(profile).map_err(|e| OmniError::Config(format!("profile {}: {e}", i + 1)))?;
            Ok(Profile {
                id: new_id(),
                ..profile
            })
        })
        .collect()
}

fn cleaned(p: Profile) -> std::result::Result<Profile, String> {
    let non_empty = |s: Option<String>| s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let name = p.name.trim().to_string();
    let host = p.host.trim().to_string();
    let username = p.username.trim().to_string();
    if name.is_empty() {
        return Err("missing name".into());
    }
    if host.is_empty() {
        return Err(format!("« {name} »: missing host"));
    }
    if username.is_empty() {
        return Err(format!("« {name} »: missing user name"));
    }
    if p.port == 0 {
        return Err(format!("« {name} »: port 0"));
    }
    let mut tags: Vec<String> = Vec::new();
    for tag in p.tags.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
        if !tags.iter().any(|t| t == tag) {
            tags.push(tag.to_string());
        }
    }
    Ok(Profile {
        name,
        host,
        username,
        tags,
        group: non_empty(p.group),
        description: non_empty(p.description),
        remote_directory: non_empty(p.remote_directory),
        save_password: false,
        ..p
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AuthMethod;
    use crate::protocols::ProtocolType;

    fn profile(name: &str) -> Profile {
        Profile {
            id: format!("id-{name}"),
            name: name.into(),
            host: "example.com".into(),
            port: 22,
            protocol: ProtocolType::Sftp,
            username: "me".into(),
            auth_method: AuthMethod::KeyWithPassphrase { path: "~/.ssh/id_ed25519".into() },
            remote_directory: Some("/srv".into()),
            description: None,
            tags: vec!["prod".into()],
            group: Some("Clients".into()),
            save_password: true,
        }
    }

    fn ids() -> impl FnMut() -> String {
        let mut n = 0;
        move || {
            n += 1;
            format!("new-{n}")
        }
    }

    #[test]
    fn export_then_import_gives_new_ids_and_no_saved_secret() {
        let dir = std::env::temp_dir().join(format!("omniscp-exchange-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("profiles export.json");

        assert_eq!(export(&path, &[profile("a"), profile("b")]).unwrap(), 2);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(FORMAT));
        assert!(!text.contains("\"save_password\": true"));

        let imported = import(&path, ids()).unwrap();
        assert_eq!(imported.len(), 2);
        assert_eq!(imported[0].id, "new-1");
        assert_eq!(imported[1].id, "new-2");
        assert_eq!(imported[0].name, "a");
        assert_eq!(imported[0].group.as_deref(), Some("Clients"));
        assert!(matches!(&imported[0].auth_method, AuthMethod::KeyWithPassphrase { path } if path == "~/.ssh/id_ed25519"));
        assert!(imported.iter().all(|p| !p.save_password));

        // A bare list, as in profiles.json, with untidy fields and no id.
        std::fs::write(
            &path,
            r#"[{"name":" web ","host":"h","port":2222,"protocol":"Scp","username":"u",
                 "auth_method":{"type":"Agent"},"tags":[" x ","","x"],"group":"  ","save_password":true}]"#,
        )
        .unwrap();
        let imported = import(&path, ids()).unwrap();
        assert_eq!(imported[0].name, "web");
        assert_eq!(imported[0].tags, vec!["x".to_string()]);
        assert_eq!(imported[0].group, None);
        assert!(!imported[0].save_password);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn import_rejects_foreign_newer_or_invalid_files() {
        let dir = std::env::temp_dir().join(format!("omniscp-exchange-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.json");
        let expect_error = |content: &str, needle: &str| {
            std::fs::write(&path, content).unwrap();
            match import(&path, ids()) {
                Err(OmniError::Config(msg)) => assert!(msg.contains(needle), "{msg}"),
                other => panic!("expected a Config error for {content}, got {other:?}"),
            }
        };

        expect_error("not json", "not valid JSON");
        expect_error(r#"{"settings":{}}"#, "not an OmniSCP profile file");
        expect_error(r#"{"format":"omniscp-profiles","version":9,"profiles":[]}"#, "newer version");
        expect_error(r#"[{"name":"a","host":"h","port":22,"protocol":"Sftp","username":"u"}]"#, "profile 1");
        expect_error(
            r#"[{"name":"a","host":" ","port":22,"protocol":"Sftp","username":"u","auth_method":{"type":"Agent"}}]"#,
            "missing host",
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
