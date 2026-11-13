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

use crate::error::{OmniError, Result};
use crate::protocols::ssh::open_session;
use crate::protocols::{
    AccountNames, ConnectParams, CopyError, FileEntry, Progress, Protocol, copy_with_progress, format_permissions,
    parse_permissions,
    remote_file_name, remote_join, sort_entries, temp_path_beside, write_local_atomically,
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

fn command_err(context: String, stderr: &str) -> OmniError {
    let detail = stderr.trim();
    if detail.contains("Permission denied") || detail.contains("Operation not permitted") {
        OmniError::Permission(format!("{context}: {detail}"))
    } else if detail.contains("No such file") {
        OmniError::Protocol(format!("{context}: no such file or directory"))
    } else {
        OmniError::Protocol(format!("{context}: {detail}"))
    }
}

fn scp_err(context: String, err: ssh2::Error) -> OmniError {
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
            .ok_or_else(|| OmniError::Protocol("Not connected".into()))
    }

    fn run(&self, command: &str) -> Result<CommandOutput> {
        let failed = |e: ssh2::Error| OmniError::Protocol(format!("Failed to run a command on the server: {e}"));
        let io_failed = |e: std::io::Error| OmniError::Protocol(format!("Failed to run a command on the server: {e}"));

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
    fn run_checked(&self, command: &str, context: impl FnOnce() -> String) -> Result<()> {
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
            Err(command_err(format!("Failed to list {path}"), &output.stderr))
        }
    }

    /// One `ls` line about `path` itself. `follow` describes the target of a symlink.
    fn stat_line(&self, path: &str, follow: bool) -> Result<LsLine> {
        let stdout = self.list_lines(if follow { "-ldL" } else { "-ld" }, path)?;
        let text = String::from_utf8_lossy(&stdout);
        let line = text.lines().next().unwrap_or_default();
        parse_ls_line(line, self.ls.escaped, now())
            .ok_or_else(|| OmniError::Protocol(format!("Could not understand the listing of {path}")))
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

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
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
        Err(OmniError::Protocol(
            "This server does not allow running the commands SCP needs (it may only allow SFTP)".into(),
        ))
    }

    fn disconnect(&mut self) -> Result<()> {
        if let Some(session) = self.session.take() {
            let _ = session.disconnect(None, "bye", None);
        }
        Ok(())
    }

    fn is_alive(&self) -> bool {
        self.run("true").is_ok_and(|output| output.success)
    }

    fn realpath(&self, path: &str) -> Result<String> {
        let output = self.run(&format!("cd -- {} && pwd -P", shell_quote(path)))?;
        if !output.success {
            return Err(command_err(format!("realpath failed for {path}"), &output.stderr));
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
                OmniError::Protocol(format!("Could not understand the listing of {path}: \"{line}\""))
            })?;
            let name = parsed.name.clone();
            // Without escapes, `ls` may print `?` for what it cannot show. Believing it
            // would give a name that does not exist, so check; quoting prevents any globbing.
            if !self.ls.escaped
                && name.contains('?')
                && self.stat_line(&remote_join(path, &name), false).is_err()
            {
                return Err(OmniError::Protocol(format!(
                    "Could not read the name of an item in {path}: the server shows it as \"{name}\""
                )));
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
            format!("Failed to create directory {path}")
        })
    }

    fn rename(&self, from: &str, to: &str) -> Result<()> {
        self.run_checked(
            &format!("env LC_ALL=C mv -- {} {}", shell_quote(from), shell_quote(to)),
            || format!("Failed to rename {from} -> {to}"),
        )
    }

    fn set_permissions(&self, path: &str, mode: u32) -> Result<()> {
        self.run_checked(
            &format!("env LC_ALL=C chmod {:o} -- {}", mode & 0o7777, shell_quote(path)),
            || format!("Failed to change the permissions of {path}"),
        )
    }

    fn read_link(&self, path: &str) -> Result<String> {
        let output = self.run(&format!("env LC_ALL=C readlink -- {}", shell_quote(path)))?;
        if !output.success {
            return Err(command_err(format!("Failed to read the link {path}"), &output.stderr));
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
            format!("Failed to remove directory {path}")
        })
    }

    fn delete(&self, path: &str) -> Result<()> {
        // `rm` removes a symlink as a link, never following it.
        self.run_checked(&format!("env LC_ALL=C rm -rf -- {}", shell_quote(path)), || {
            format!("Failed to delete {path}")
        })
    }

    fn upload(&self, local: &Path, remote: &str, progress: Progress) -> Result<()> {
        // Follows links: `mv` would otherwise drop the file inside a linked folder.
        let existing = self.stat(remote).ok();
        if existing.as_ref().is_some_and(|e| e.is_dir) {
            return Err(OmniError::Transfer(format!("{remote} is a directory")));
        }
        // A replaced file keeps its permissions.
        let mode = existing
            .and_then(|e| e.permissions)
            .map_or(DEFAULT_FILE_MODE, |p| mode_bits(&p) as i32);

        let mut local_file = std::fs::File::open(local)
            .map_err(|e| OmniError::io_context(format!("Failed to open {}", local.display()), e))?;
        let metadata = local_file
            .metadata()
            .map_err(|e| OmniError::io_context(format!("Failed to read {}", local.display()), e))?;
        let size = metadata.len();
        // Keep the source's date, which sync compares.
        let times = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| (d.as_secs(), d.as_secs()));

        let temp = temp_path_beside(remote);
        let write_err = |e: ssh2::Error| OmniError::Transfer(format!("Failed to write {remote}: {e}"));

        let sent = (|| {
            let mut channel = self
                .session()?
                .scp_send(Path::new(&temp), mode, size, times)
                .map_err(|e| scp_err(format!("Failed to create a temporary file for {remote}"), e))?;

            // The server was promised exactly `size` bytes.
            let mut copied = 0;
            let mut limited = (&mut local_file).take(size);
            copy_with_progress(&mut limited, &mut channel, &mut |n| {
                copied += n;
                progress(n)
            })
            .map_err(|e| match e {
                CopyError::Cancelled => OmniError::Cancelled,
                CopyError::Read(e) => OmniError::io_context(format!("Failed to read {}", local.display()), e),
                CopyError::Write(e) => OmniError::Transfer(format!("Failed to write {remote}: {e}")),
            })?;
            if copied != size {
                return Err(OmniError::Transfer(format!("{} changed while it was being sent", local.display())));
            }

            // The SCP protocol ends a file with a zero byte, which libssh2 leaves to the caller.
            // Without it the server sees a lost connection and does not apply the date.
            channel
                .write_all(&[0])
                .map_err(|e| OmniError::Transfer(format!("Failed to write {remote}: {e}")))?;
            channel.send_eof().map_err(write_err)?;
            channel.wait_eof().map_err(write_err)?;
            channel.close().map_err(write_err)?;
            channel.wait_close().map_err(write_err)?;
            if channel.exit_status().map_err(write_err)? != 0 {
                let mut detail = String::new();
                let _ = channel.stderr().read_to_string(&mut detail);
                return Err(command_err(format!("The server failed to write {remote}"), &detail));
            }
            Ok(())
        })();

        // `mv` replaces the destination in one step.
        let result = sent.and_then(|()| {
            self.run_checked(
                &format!("env LC_ALL=C mv -f -- {} {}", shell_quote(&temp), shell_quote(remote)),
                || format!("Failed to write {remote}"),
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
            return Err(OmniError::Transfer(format!("{remote} is a directory")));
        }
        let (channel, stat) = self
            .session()?
            .scp_recv(Path::new(remote))
            .map_err(|e| scp_err(format!("Failed to open remote file {remote}"), e))?;
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
                CopyError::Read(e) => OmniError::Transfer(format!("Failed to read {remote}: {e}")),
                CopyError::Write(e) => OmniError::io_context(format!("Failed to write {}", local.display()), e),
            })?;
            // A connection dropping mid-file can look like an early end of file.
            if copied != size {
                return Err(OmniError::Transfer(format!(
                    "Failed to read {remote}: got {copied} of {size} bytes"
                )));
            }
            Ok(())
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
struct LsLine {
    is_dir: bool,
    is_symlink: bool,
    mode: u32,
    /// With setuid, setgid and sticky bits, for display and chmod.
    full_mode: u32,
    owner: String,
    group: String,
    size: u64,
    modified: Option<i64>,
    name: String,
}

/// Permission bits of a string like `drwxr-sr-x`; special bits are not kept.
fn mode_bits(permissions: &str) -> u32 {
    permissions
        .chars()
        .skip(1)
        .take(9)
        .fold(0, |mode, c| mode << 1 | u32::from(matches!(c, 'r' | 'w' | 'x' | 's' | 't')))
}

/// Splits off the next whitespace-separated word, keeping the rest verbatim for names.
fn next_word(text: &str) -> Option<(&str, &str)> {
    let text = text.trim_start_matches(' ');
    let end = text.find(' ').unwrap_or(text.len());
    if end == 0 { None } else { Some((&text[..end], &text[end..])) }
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    // Howard Hinnant's algorithm, for the proleptic Gregorian calendar.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn month_number(name: &str) -> Option<i64> {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    MONTHS.iter().position(|m| *m == name).map(|i| i as i64 + 1)
}

fn clock_seconds(clock: &str) -> Option<i64> {
    let mut parts = clock.split(':');
    let hours: i64 = parts.next()?.parse().ok()?;
    let minutes: i64 = parts.next()?.parse().ok()?;
    let seconds: i64 = parts.next().map_or(Some(0), |s| s.split('.').next()?.parse().ok())?;
    Some(hours * 3600 + minutes * 60 + seconds)
}

/// Reads the date of an `ls` line; returns it with what follows. Which form appears
/// depends on the `ls` variant. Only the epoch form is exact across time zones.
fn parse_ls_date(text: &str, now: i64) -> Option<(i64, &str)> {
    let (first, rest) = next_word(text)?;
    // 1790000000
    if first.bytes().all(|b| b.is_ascii_digit()) {
        return Some((first.parse().ok()?, rest));
    }
    // 2026-11-05 16:17:03 +0000
    let mut ymd = first.split('-');
    if let (Some(y), Some(m), Some(d), None) = (ymd.next(), ymd.next(), ymd.next(), ymd.next()) {
        let days = days_from_civil(y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
        let (clock, rest) = next_word(rest)?;
        let (zone, rest) = next_word(rest)?;
        let sign = if zone.starts_with('-') { -1 } else { 1 };
        let digits: i64 = zone.trim_start_matches(['+', '-']).parse().ok()?;
        let offset = sign * (digits / 100 * 3600 + digits % 100 * 60);
        return Some((days * 86400 + clock_seconds(clock)? - offset, rest));
    }
    // Thu Nov  5 16:17:03 2026
    if month_number(first).is_none() {
        let (month, rest) = next_word(rest)?;
        let (day, rest) = next_word(rest)?;
        let (clock, rest) = next_word(rest)?;
        let (year, rest) = next_word(rest)?;
        let days = days_from_civil(year.parse().ok()?, month_number(month)?, day.parse().ok()?);
        return Some((days * 86400 + clock_seconds(clock)?, rest));
    }
    // Nov  5 16:17 (within about six months), or Nov  5  2024
    let month = month_number(first)?;
    let (day, rest) = next_word(rest)?;
    let day: i64 = day.parse().ok()?;
    let (third, rest) = next_word(rest)?;
    if third.contains(':') {
        let clock = clock_seconds(third)?;
        let at = |year| days_from_civil(year, month, day) * 86400 + clock;
        // No year means recent: the latest year that does not put the date in the future.
        let mut year = 1971 + now.div_euclid(31_556_952);
        while at(year) > now + 86400 {
            year -= 1;
        }
        Some((at(year), rest))
    } else {
        Some((days_from_civil(third.parse().ok()?, month, day) * 86400, rest))
    }
}

/// Undoes GNU `ls -b`: `\ `, `\\`, `\n`, octal bytes such as `\303\251`.
fn unescape_ls_name(name: &str) -> String {
    let bytes = name.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' || i + 1 == bytes.len() {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        let next = bytes[i + 1];
        let octal = bytes.get(i + 1..i + 4).filter(|d| d.iter().all(|b| (b'0'..=b'7').contains(b)));
        if let Some(digits) = octal {
            out.push(digits.iter().fold(0u32, |v, b| v * 8 + u32::from(b - b'0')) as u8);
            i += 4;
            continue;
        }
        out.push(match next {
            b'n' => b'\n',
            b't' => b'\t',
            b'r' => b'\r',
            b'a' => 0x07,
            b'b' => 0x08,
            b'f' => 0x0c,
            b'v' => 0x0b,
            other => other,
        });
        i += 2;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parses a line of `ls -ln`, such as:
/// `drwxr-xr-x 2 1000 1000 4096 1790000000 name`.
fn parse_ls_line(line: &str, escaped: bool, now: i64) -> Option<LsLine> {
    let (permissions, rest) = next_word(line)?;
    let kind = permissions.chars().next()?;
    if permissions.len() < 10 || !"-dlbcps".contains(kind) {
        return None;
    }
    let (_links, rest) = next_word(rest)?;
    let (owner, rest) = next_word(rest)?;
    let (group, rest) = next_word(rest)?;
    let (size, rest) = next_word(rest)?;
    // Devices show "major, minor" instead of a size.
    let (size, rest) = if size.ends_with(',') {
        (0, next_word(rest)?.1)
    } else if size.contains(',') {
        (0, rest)
    } else {
        (size.parse().ok()?, rest)
    };
    let (modified, rest) = parse_ls_date(rest, now)?;

    // One separator: anything after it belongs to the name.
    let name = rest.strip_prefix(' ')?;
    let name = if kind == 'l' { name.split(" -> ").next()? } else { name };
    if name.is_empty() {
        return None;
    }
    Some(LsLine {
        is_dir: kind == 'd',
        is_symlink: kind == 'l',
        mode: mode_bits(permissions),
        full_mode: parse_permissions(permissions),
        owner: owner.to_string(),
        group: group.to_string(),
        size,
        modified: Some(modified),
        name: if escaped { unescape_ls_name(name) } else { name.to_string() },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-11-05 12:00:00 UTC.
    const NOW: i64 = 1_793_880_000;

    fn parse(line: &str) -> LsLine {
        parse_ls_line(line, false, NOW).unwrap_or_else(|| panic!("could not parse {line:?}"))
    }

    #[test]
    fn quoting_neutralizes_the_shell() {
        assert_eq!(shell_quote("plain"), "'plain'");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("$(rm -rf ~)"), "'$(rm -rf ~)'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_quote("`x`;y\nz"), "'`x`;y\nz'");
    }

    #[test]
    fn gnu_line_with_epoch() {
        let line = parse("-rw-r--r-- 1 1000 1001 12345 1790000000 report.pdf");
        assert_eq!(line.name, "report.pdf");
        assert_eq!(line.size, 12345);
        assert_eq!(line.modified, Some(1790000000));
        assert_eq!((line.owner.as_str(), line.group.as_str()), ("1000", "1001"));
        assert_eq!(line.mode, 0o644);
        assert!(!line.is_dir && !line.is_symlink);
    }

    #[test]
    fn folders_links_and_special_files() {
        let dir = parse("drwxr-xr-x 2 0 0 4096 1790000000 sub");
        assert!(dir.is_dir);
        assert_eq!(dir.mode, 0o755);

        let link = parse("lrwxrwxrwx 1 0 0 4 1790000000 to dir -> target with -> arrow");
        assert!(link.is_symlink);
        assert_eq!(link.name, "to dir");

        let device = parse("crw-rw-rw- 1 0 0 1, 3 1790000000 null");
        assert_eq!((device.size, device.name.as_str()), (0, "null"));
        let packed = parse("crw-rw-rw- 1 0 0 1,3 1790000000 zero");
        assert_eq!((packed.size, packed.name.as_str()), (0, "zero"));

        // ACL and SELinux markers after the permissions.
        assert_eq!(parse("-rw-r--r--+ 1 0 0 1 1790000000 a").name, "a");
        assert_eq!(parse("-rw-r--r--. 1 0 0 1 1790000000 b").name, "b");
    }

    #[test]
    fn names_keep_their_spaces() {
        assert_eq!(parse("-rw-r--r-- 1 0 0 1 1790000000 two  spaces ").name, "two  spaces ");
        assert_eq!(parse("-rw-r--r-- 1 0 0 1 1790000000  leading").name, " leading");
    }

    #[test]
    fn every_date_form() {
        // Exact to the second, and agreeing with each other.
        let epoch = parse("-rw-r--r-- 1 0 0 1 1793896623 a").modified;
        assert_eq!(epoch, Some(1793896623));
        assert_eq!(parse("-rw-r--r-- 1 0 0 1 2026-11-05 16:37:03 +0000 a").modified, epoch);
        assert_eq!(parse("-rw-r--r-- 1 0 0 1 2026-11-05 18:37:03.123456 +0200 a").modified, epoch);
        assert_eq!(parse("-rw-r--r-- 1 0 0 1 2026-11-05 11:07:03 -0530 a").modified, epoch);
        assert_eq!(parse("-rw-r--r-- 1 0 0 1 Thu Nov  5 16:37:03 2026 a").modified, epoch);

        // Plain ls: to the minute.
        assert_eq!(parse("-rw-r--r-- 1 0 0 1 Nov  5 11:37 a").modified, Some(1793878620));
        // A date ahead of now without a year is from last year.
        let december = parse("-rw-r--r-- 1 0 0 1 Dec 25 10:00 a").modified.unwrap();
        assert!(december < NOW && NOW - december < 366 * 86400);
        // Just after New Year, December is still last year's, and January this year's.
        let new_year = days_from_civil(2027, 1, 1) * 86400 + 3600;
        let parsed = parse_ls_date("Dec 31 23:00 a", new_year).unwrap().0;
        assert_eq!(parsed, days_from_civil(2026, 12, 31) * 86400 + 23 * 3600);
        let parsed = parse_ls_date("Jan  1 00:30 a", new_year).unwrap().0;
        assert_eq!(parsed, days_from_civil(2027, 1, 1) * 86400 + 1800);
        assert_eq!(parse("-rw-r--r-- 1 0 0 1 Jan  2  2024 a").modified, Some(1704153600));
        assert_eq!(parse("-rw-r--r-- 1 0 0 1 Jan  2  2024 two words").name, "two words");
    }

    #[test]
    fn gnu_escapes_are_undone() {
        let name = |line: &str| parse_ls_line(line, true, NOW).unwrap().name;
        assert_eq!(name(r"-rw-r--r-- 1 0 0 1 1790000000 a\ b"), "a b");
        assert_eq!(name(r"-rw-r--r-- 1 0 0 1 1790000000 back\\slash"), "back\\slash");
        assert_eq!(name(r"-rw-r--r-- 1 0 0 1 1790000000 new\nline"), "new\nline");
        assert_eq!(name(r"-rw-r--r-- 1 0 0 1 1790000000 caf\303\251"), "café");
        assert_eq!(name(r"lrwxrwxrwx 1 0 0 1 1790000000 a\ ->\ b -> c"), "a -> b");
    }

    #[test]
    fn unknown_lines_are_refused_not_skipped() {
        for line in [
            "",
            "garbage",
            "total 12",
            "?????????? ? ? ? ? ? name",
            "-rw-r--r-- 1 0 0 notasize 1790000000 a",
            "-rw-r--r-- 1 0 0 1 someday a",
            "-rw-r--r-- 1 0 0 1 1790000000",
            "second half of a name with a newline",
        ] {
            assert!(parse_ls_line(line, false, NOW).is_none(), "{line:?} should be refused");
        }
    }

    #[test]
    fn permission_bits() {
        assert_eq!(mode_bits("-rwxr-x---"), 0o750);
        assert_eq!(mode_bits("drwxrwxrwt"), 0o777);
        assert_eq!(mode_bits("-rw-r--r--+"), 0o644);
        assert_eq!(mode_bits("-rw-r-Sr--"), 0o644);
    }
}
