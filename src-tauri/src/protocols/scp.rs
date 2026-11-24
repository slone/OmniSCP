// SCP only copies bytes: listing, renaming and deleting go through commands run
// over the same SSH session, so this requires a POSIX shell on the server.
//
// Two things must hold here. Paths reach a shell, so every one goes through
// `shell_quote`. And a listing line that cannot be understood is an error, never
// skipped: sync would take the missing entry for a deleted file.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use ssh2::Session;

use crate::error::{Msg, OmniError, Result};
use crate::protocols::listing::{LsLine, mode_bits, now, parse_ls_line};
use crate::protocols::ssh::open_session;
use crate::protocols::{
    AccountNames, ConnectParams, CopyError, FileEntry, Progress, Protocol, copy_with_progress, format_permissions,
    not_found, remote_file_name, remote_join, sort_entries, temp_path_beside, write_local_atomically,
};

/// Account files larger than this are only read in part for names.
const MAX_ACCOUNT_FILE: u64 = 4 * 1024 * 1024;
const DEFAULT_FILE_MODE: i32 = 0o644;

/// `ls` flags to get exact dates, best first; the first one the server accepts is kept.
const LS_VARIANTS: [LsVariant; 4] = [
    // GNU coreutils; -b escapes names so that any of them fits on one line.
    LsVariant { env: GNU_LS_ENV, flags: "-b --time-style=+%s", escaped: true },
    // BSD, macOS.
    LsVariant { env: PLAIN_LS_ENV, flags: "-D %s", escaped: false },
    // busybox.
    LsVariant { env: PLAIN_LS_ENV, flags: "--full-time", escaped: false },
    // Anything else, with dates only to the minute.
    LsVariant { env: PLAIN_LS_ENV, flags: "", escaped: false },
];

/// Octal escapes need no particular character set.
const GNU_LS_ENV: &str = "LC_ALL=C";
/// English dates and messages, but the C locale would turn accented names into `?`.
const PLAIN_LS_ENV: &str = "LC_ALL= LC_TIME=C LC_MESSAGES=C";

#[derive(Clone, Copy)]
struct LsVariant {
    env: &'static str,
    flags: &'static str,
    escaped: bool,
}

/// Quotes a string for a POSIX shell; nothing inside single quotes is interpreted.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn command_err(context: Msg, stderr: &str) -> OmniError {
    let detail = stderr.trim();
    let context = if detail.is_empty() { context } else { context.detail(detail) };
    if detail.contains("Permission denied") || detail.contains("Operation not permitted") {
        OmniError::Permission(context)
    } else if detail.contains("No such file") {
        OmniError::Protocol(not_found(&context))
    } else {
        OmniError::Protocol(context)
    }
}

fn scp_err(context: Msg, err: ssh2::Error) -> OmniError {
    command_err(context, &err.to_string())
}

struct CommandOutput {
    stdout: Vec<u8>,
    stderr: String,
    success: bool,
}

pub struct ScpConnection {
    params: ConnectParams,
    session: Option<Session>,
    ls: LsVariant,
}

impl ScpConnection {
    pub fn new(params: &ConnectParams) -> Self {
        Self { params: params.clone(), session: None, ls: LS_VARIANTS[3] }
    }

    fn session(&self) -> Result<&Session> {
        self.session
            .as_ref()
            .ok_or_else(|| OmniError::Protocol(Msg::new("conn.notConnected")))
    }

    fn run(&self, command: &str) -> Result<CommandOutput> {
        let failed = |e: ssh2::Error| OmniError::Protocol(Msg::new("scp.command").detail(e));
        let io_failed = |e: std::io::Error| OmniError::Protocol(Msg::new("scp.command").detail(e));

        let mut channel = self.session()?.channel_session().map_err(failed)?;
        channel.exec(command).map_err(failed)?;
        // Nothing to send: a command reading its input ends instead of waiting.
        channel.send_eof().map_err(failed)?;

        let mut stdout = Vec::new();
        channel.read_to_end(&mut stdout).map_err(io_failed)?;
        let mut stderr = Vec::new();
        channel.stderr().read_to_end(&mut stderr).map_err(io_failed)?;
        channel.wait_close().map_err(failed)?;
        Ok(CommandOutput {
            stdout,
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
            success: channel.exit_status().map_err(failed)? == 0,
        })
    }

    /// Runs a command that only reports success or failure.
    fn run_checked(&self, command: &str, context: impl FnOnce() -> Msg) -> Result<()> {
        let output = self.run(command)?;
        if output.success {
            Ok(())
        } else {
            Err(command_err(context(), &output.stderr))
        }
    }

    fn ls_command(&self, flags: &str, path: &str) -> String {
        format!("env {} ls -n {} {flags} -- {}", self.ls.env, self.ls.flags, shell_quote(path))
    }

    fn list_lines(&self, flags: &str, path: &str) -> Result<Vec<u8>> {
        let output = self.run(&self.ls_command(flags, path))?;
        if output.success {
            Ok(output.stdout)
        } else {
            Err(command_err(Msg::new("remote.list").with("path", path), &output.stderr))
        }
    }

    /// One `ls` line about `path` itself. `follow` describes the target of a symlink.
    fn stat_line(&self, path: &str, follow: bool) -> Result<LsLine> {
        let stdout = self.list_lines(if follow { "-ldL" } else { "-ld" }, path)?;
        let text = String::from_utf8_lossy(&stdout);
        let line = text.lines().next().unwrap_or_default();
        parse_ls_line(line, self.ls.escaped, now())
            .ok_or_else(|| OmniError::Protocol(Msg::new("scp.listing").with("path", path)))
    }

    fn to_entry(&self, path: String, name: String, line: LsLine) -> FileEntry {
        // A dangling link has no target to describe.
        let described = if line.is_symlink {
            self.stat_line(&path, true).unwrap_or_else(|_| line.clone())
        } else {
            line.clone()
        };
        FileEntry {
            name,
            path: PathBuf::from(path),
            is_dir: described.is_dir,
            is_symlink: line.is_symlink,
            size: if described.is_dir { 0 } else { described.size },
            modified: described.modified,
            permissions: Some(format_permissions(described.full_mode, described.is_dir)),
            mode: Some(described.full_mode),
            // `ls -n` gives numbers; a server that still prints a name keeps it.
            uid: described.owner.parse().ok(),
            gid: described.group.parse().ok(),
            owner: described.owner.parse::<u32>().is_err().then(|| described.owner.clone()),
            group: described.group.parse::<u32>().is_err().then(|| described.group.clone()),
        }
    }
}


impl Protocol for ScpConnection {
    fn connect(&mut self) -> Result<()> {
        self.session = Some(open_session(&self.params)?);

        for variant in LS_VARIANTS {
            self.ls = variant;
            let usable = self
                .run(&self.ls_command("-ld", "/"))
                .is_ok_and(|output| {
                    output.success
                        && String::from_utf8_lossy(&output.stdout)
                            .lines()
                            .next()
                            .is_some_and(|line| parse_ls_line(line, variant.escaped, now()).is_some())
                });
            if usable {
                return Ok(());
            }
        }
        self.disconnect()?;
        Err(OmniError::Protocol(Msg::new("scp.noShell")))
    }

    fn disconnect(&mut self) -> Result<()> {
        if let Some(session) = self.session.take() {
            let _ = session.disconnect(None, "bye", None);
        }
        Ok(())
    }

    fn keepalive(&self) {
        if let Some(session) = &self.session {
            let _ = session.keepalive_send();
        }
    }

    fn is_alive(&self) -> bool {
        self.run("true").is_ok_and(|output| output.success)
    }

    fn realpath(&self, path: &str) -> Result<String> {
        let output = self.run(&format!("cd -- {} && pwd -P", shell_quote(path)))?;
        if !output.success {
            return Err(command_err(Msg::new("remote.realpath").with("path", path), &output.stderr));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim_end_matches('\n').to_string())
    }

    fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>> {
        // The trailing slash makes `ls` list what is behind a link to a folder.
        let listed = format!("{}/", path.trim_end_matches('/'));
        let stdout = self.list_lines("-lA", &listed)?;
        let text = String::from_utf8_lossy(&stdout);

        let mut files = Vec::new();
        for line in text.lines() {
            if line.is_empty() || line.starts_with("total ") {
                continue;
            }
            let parsed = parse_ls_line(line, self.ls.escaped, now()).ok_or_else(|| {
                OmniError::Protocol(Msg::new("scp.listing").with("path", path).detail(line))
            })?;
            let name = parsed.name.clone();
            // Without escapes, `ls` may print `?` for what it cannot show. Believing it
            // would give a name that does not exist, so check; quoting prevents any globbing.
            if !self.ls.escaped
                && name.contains('?')
                && self.stat_line(&remote_join(path, &name), false).is_err()
            {
                return Err(OmniError::Protocol(
                    Msg::new("scp.unreadableName").with("path", path).with("name", &name),
                ));
            }
            files.push(self.to_entry(remote_join(path, &name), name, parsed));
        }
        sort_entries(&mut files);
        Ok(files)
    }

    fn stat(&self, path: &str) -> Result<FileEntry> {
        let line = self.stat_line(path, false)?;
        Ok(self.to_entry(path.to_string(), remote_file_name(path).to_string(), line))
    }

    fn mkdir(&self, path: &str) -> Result<()> {
        self.run_checked(&format!("env LC_ALL=C mkdir -- {}", shell_quote(path)), || {
            Msg::new("remote.mkdir").with("path", path)
        })
    }

    fn rename(&self, from: &str, to: &str) -> Result<()> {
        self.run_checked(
            &format!("env LC_ALL=C mv -- {} {}", shell_quote(from), shell_quote(to)),
            || Msg::new("remote.rename").with("from", from).with("to", to),
        )
    }

    fn set_permissions(&self, path: &str, mode: u32) -> Result<()> {
        self.run_checked(
            &format!("env LC_ALL=C chmod {:o} -- {}", mode & 0o7777, shell_quote(path)),
            || Msg::new("remote.chmod").with("path", path),
        )
    }

    fn read_link(&self, path: &str) -> Result<String> {
        let output = self.run(&format!("env LC_ALL=C readlink -- {}", shell_quote(path)))?;
        if !output.success {
            return Err(command_err(Msg::new("remote.readlink").with("path", path), &output.stderr));
        }
        let mut target = output.stdout;
        // Only the line end readlink adds: a target may itself end with a newline.
        if target.last() == Some(&b'\n') {
            target.pop();
        }
        Ok(String::from_utf8_lossy(&target).into_owned())
    }

    fn account_names(&self) -> AccountNames {
        let read = |path: &str| {
            self.run(&format!("head -c {MAX_ACCOUNT_FILE} {path}"))
                .ok()
                .filter(|o| o.success)
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                .unwrap_or_default()
        };
        AccountNames::parse(&read("/etc/passwd"), &read("/etc/group"))
    }

    fn remove_empty_dir(&self, path: &str) -> Result<()> {
        self.run_checked(&format!("env LC_ALL=C rmdir -- {}", shell_quote(path)), || {
            Msg::new("remote.rmdir").with("path", path)
        })
    }

    fn delete(&self, path: &str) -> Result<()> {
        // `rm` removes a symlink as a link, never following it.
        self.run_checked(&format!("env LC_ALL=C rm -rf -- {}", shell_quote(path)), || {
            Msg::new("remote.delete").with("path", path)
        })
    }

    fn upload(&self, local: &Path, remote: &str, progress: Progress) -> Result<()> {
        // Follows links: `mv` would otherwise drop the file inside a linked folder.
        let existing = self.stat(remote).ok();
        if existing.as_ref().is_some_and(|e| e.is_dir) {
            return Err(OmniError::Transfer(Msg::new("transfer.targetIsFolder").with("path", remote)));
        }
        // A replaced file keeps its permissions.
        let mode = existing
            .and_then(|e| e.permissions)
            .map_or(DEFAULT_FILE_MODE, |p| mode_bits(&p) as i32);

        let mut local_file = std::fs::File::open(local)
            .map_err(|e| OmniError::io_context(local.display(), e))?;
        let metadata = local_file
            .metadata()
            .map_err(|e| OmniError::io_context(local.display(), e))?;
        let size = metadata.len();
        // Keep the source's date, which sync compares.
        let times = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| (d.as_secs(), d.as_secs()));

        let temp = temp_path_beside(remote);
        let write_err = |e: ssh2::Error| OmniError::Transfer(Msg::new("remote.write").with("path", remote).detail(e));

        let sent = (|| {
            let mut channel = self
                .session()?
                .scp_send(Path::new(&temp), mode, size, times)
                .map_err(|e| scp_err(Msg::new("remote.tempFile").with("path", remote), e))?;

            // The server was promised exactly `size` bytes.
            let mut copied = 0;
            let mut limited = (&mut local_file).take(size);
            copy_with_progress(&mut limited, &mut channel, &mut |n| {
                copied += n;
                progress(n)
            })
            .map_err(|e| match e {
                CopyError::Cancelled => OmniError::Cancelled,
                CopyError::Read(e) => OmniError::io_context(local.display(), e),
                CopyError::Write(e) => OmniError::Transfer(Msg::new("remote.write").with("path", remote).detail(e)),
            })?;
            if copied != size {
                return Err(OmniError::Transfer(Msg::new("transfer.changedWhileSending").with("path", local.display())));
            }

            // The SCP protocol ends a file with a zero byte, which libssh2 leaves to the caller.
            // Without it the server sees a lost connection and does not apply the date.
            channel
                .write_all(&[0])
                .map_err(|e| OmniError::Transfer(Msg::new("remote.write").with("path", remote).detail(e)))?;
            channel.send_eof().map_err(write_err)?;
            channel.wait_eof().map_err(write_err)?;
            channel.close().map_err(write_err)?;
            channel.wait_close().map_err(write_err)?;
            if channel.exit_status().map_err(write_err)? != 0 {
                let mut detail = String::new();
                let _ = channel.stderr().read_to_string(&mut detail);
                return Err(command_err(Msg::new("remote.write").with("path", remote), &detail));
            }
            Ok(())
        })();

        // `mv` replaces the destination in one step.
        let result = sent.and_then(|()| {
            self.run_checked(
                &format!("env LC_ALL=C mv -f -- {} {}", shell_quote(&temp), shell_quote(remote)),
                || Msg::new("remote.write").with("path", remote),
            )
        });
        if result.is_err() {
            let _ = self.run(&format!("rm -f -- {}", shell_quote(&temp)));
        }
        result
    }

    fn download(&self, remote: &str, local: &Path, progress: Progress) -> Result<()> {
        let entry = self.stat(remote)?;
        if entry.is_dir {
            return Err(OmniError::Transfer(Msg::new("transfer.targetIsFolder").with("path", remote)));
        }
        let (channel, stat) = self
            .session()?
            .scp_recv(Path::new(remote))
            .map_err(|e| scp_err(Msg::new("remote.open").with("path", remote), e))?;
        let size = stat.size();
        let mut limited = channel.take(size);
        let mtime = entry.modified.and_then(|m| u64::try_from(m).ok());

        write_local_atomically(local, mtime, |local_file| {
            let mut copied = 0;
            copy_with_progress(&mut limited, local_file, &mut |n| {
                copied += n;
                progress(n)
            })
            .map_err(|e| match e {
                CopyError::Cancelled => OmniError::Cancelled,
                CopyError::Read(e) => OmniError::Transfer(Msg::new("remote.read").with("path", remote).detail(e)),
                CopyError::Write(e) => OmniError::io_context(local.display(), e),
            })?;
            // A connection dropping mid-file can look like an early end of file.
            if copied != size {
                return Err(OmniError::Transfer(
                    Msg::new("remote.readShort").with("path", remote).with("got", copied).with("size", size),
                ));
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_neutralizes_the_shell() {
        assert_eq!(shell_quote("plain"), "'plain'");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("$(rm -rf ~)"), "'$(rm -rf ~)'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_quote("`x`;y\nz"), "'`x`;y\nz'");
    }
}
