// FTP, and FTPS: the same protocol over TLS, started by the client (explicit,
// usually port 21) or from the first byte (implicit, usually port 990).
//
// Servers vary much more than SSH ones. Listings use MLSD when the server
// offers it, with exact dates, and fall back to `LIST`, in the `ls -l` format
// SCP also parses. As for SCP, a listing line that cannot be understood is an
// error, never skipped: sync would take the missing entry for a deleted file.
//
// A certificate signed by an authority the system trusts is accepted as is.
// Any other, such as a self-signed one, goes through the same confirmation as an
// unknown SSH host key, and is then remembered by its fingerprint.

use std::collections::HashMap;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, UNIX_EPOCH};

use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use sha2::{Digest, Sha256};
use suppaftp::types::{FileType, Mode};
use suppaftp::{FtpError, RustlsConnector, RustlsFtpStream, Status};

use crate::error::{Msg, OmniError, Result};
use crate::protocols::listing::{days_from_civil, now, parse_ls_line, LsLine};
use crate::protocols::ssh::base64_no_pad;
use crate::protocols::{
    ConnectParams, CopyError, FileEntry, Progress, Protocol, ProtocolType, copy_with_progress, format_permissions,
    not_found, remote_file_name, remote_join, remote_parent, sort_entries, temp_path_beside, write_local_atomically,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// For every reply and every block of data.
const IO_TIMEOUT: Duration = Duration::from_secs(30);
/// Server certificates accepted by the user, one `host:port SHA256:...` per line.
pub const KNOWN_CERTIFICATES_FILE: &str = "known_certificates";

/// What the server's FEAT reply says it can do beyond RFC 959.
#[derive(Debug, Default, Clone, Copy)]
struct Features {
    /// MLSD and MLST: exact, machine-readable listings.
    mlst: bool,
    /// MFMT: setting a file's date, so sync sees an upload as up to date.
    mfmt: bool,
    /// MDTM: reading a file's date.
    mdtm: bool,
}

pub struct FtpConnection {
    params: ConnectParams,
    ftp: Mutex<Option<RustlsFtpStream>>,
    features: Features,
}

impl FtpConnection {
    pub fn new(params: &ConnectParams) -> Self {
        Self { params: params.clone(), ftp: Mutex::new(None), features: Features::default() }
    }

    /// The session, locked for one command or one whole transfer.
    fn session(&self) -> Result<FtpGuard<'_>> {
        let guard = self.ftp.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            return Err(OmniError::Protocol(Msg::new("conn.notConnected")));
        }
        Ok(FtpGuard(guard))
    }

    /// After an interrupted transfer. Over TLS, servers that insist on reusing
    /// the TLS session for data (vsftpd) refuse the one the interrupted
    /// connection left, so the session starts over; otherwise only a dead one does.
    fn recover(&self, ftp: &mut FtpGuard<'_>) {
        let tls = self.params.profile.protocol == ProtocolType::Ftps;
        if (tls || ftp.noop().is_err())
            && let Ok((fresh, _)) = open(&self.params)
            && let Some(mut old) = ftp.0.replace(fresh)
        {
            let _ = old.quit();
        }
    }

    /// What `path` is, `None` if nothing; for a link, what it points to.
    fn target(&self, ftp: &mut RustlsFtpStream, path: &str) -> Option<Target> {
        if ftp.cwd(path).is_ok() {
            return Some(Target::Folder);
        }
        let reply = ftp.custom_command(format!("SIZE {path}"), &[Status::File]).ok()?;
        let size = String::from_utf8_lossy(&reply.body).split_whitespace().last()?.parse().ok()?;
        Some(Target::File(size))
    }

    /// One folder's entries as the server lists them, `.` and `..` left out.
    fn read_dir(&self, ftp: &mut RustlsFtpStream, path: &str) -> Result<Vec<Listed>> {
        let context = || Msg::new("remote.list").with("path", path);
        let mut listed = Vec::new();
        if self.features.mlst {
            let lines = ftp.mlsd(Some(path)).map_err(|e| ftp_err(context(), e))?;
            for line in lines.iter().filter(|l| !l.trim().is_empty()) {
                let entry = parse_mlsx(line).ok_or_else(|| listing_err(path, line))?;
                if !entry.is_parent_or_self {
                    listed.push(entry);
                }
            }
            // Many servers describe a link in MLSD as what it points to, and
            // leave out one that points nowhere. Taken for a folder, a link
            // would be followed by transfers and deletes: LIST, which shows
            // links, says which entries are.
            if let Ok(links) = self.list_lines(ftp, path) {
                for line in links.into_iter().filter(|l| l.is_symlink) {
                    match listed.iter_mut().find(|e| e.name == line.name) {
                        Some(entry) => {
                            entry.is_symlink = true;
                            entry.link_target = line.link_target;
                        }
                        None => listed.push(line),
                    }
                }
            }
        } else {
            listed = self.list_lines(ftp, path)?;
        }
        Ok(listed)
    }

    /// A folder's entries from LIST, `.` and `..` left out.
    fn list_lines(&self, ftp: &mut RustlsFtpStream, path: &str) -> Result<Vec<Listed>> {
        let context = || Msg::new("remote.list").with("path", path);
        // LIST names no folder, so that spaces and leading dashes in it cannot be misread.
        ftp.cwd(path).map_err(|e| ftp_err(context(), e))?;
        // `-a` shows hidden files on most servers; one that takes it for a name refuses it.
        let lines = match ftp.list(Some("-a")) {
            Ok(lines) => lines,
            Err(_) => ftp.list(None).map_err(|e| ftp_err(context(), e))?,
        };
        let now = now();
        let mut listed = Vec::new();
        for line in lines.iter().filter(|l| !l.trim().is_empty() && !l.starts_with("total ")) {
            let entry = parse_ls_line(line, false, now)
                .map(Listed::from)
                .or_else(|| parse_dos_line(line))
                .ok_or_else(|| listing_err(path, line))?;
            if entry.name != "." && entry.name != ".." {
                listed.push(entry);
            }
        }
        Ok(listed)
    }

    /// Builds the entry for `path`, finding out what a symlink points to.
    fn to_entry(&self, ftp: &mut RustlsFtpStream, path: String, listed: Listed) -> FileEntry {
        // A link to a file counts as that file; one pointing nowhere keeps its own size.
        let (is_dir, size) = match (listed.is_symlink, listed.is_symlink.then(|| self.target(ftp, &path)).flatten()) {
            (true, Some(Target::Folder)) => (true, 0),
            (true, Some(Target::File(size))) => (false, size),
            _ => (listed.is_dir, listed.size),
        };
        let owner_number = listed.owner.as_deref().and_then(|o| o.parse().ok());
        let group_number = listed.group.as_deref().and_then(|g| g.parse().ok());
        FileEntry {
            name: listed.name,
            path: PathBuf::from(path),
            is_dir,
            is_symlink: listed.is_symlink,
            size: if is_dir { 0 } else { size },
            modified: listed.modified,
            permissions: listed.mode.map(|m| format_permissions(m, is_dir)),
            mode: listed.mode,
            uid: owner_number,
            gid: group_number,
            owner: listed.owner.filter(|_| owner_number.is_none()),
            group: listed.group.filter(|_| group_number.is_none()),
        }
    }

    /// `path` as listed in its parent folder.
    fn lookup(&self, ftp: &mut RustlsFtpStream, path: &str) -> Result<Listed> {
        let missing = || OmniError::Protocol(Msg::new("remote.notFound").with("path", path));
        let Some(parent) = remote_parent(path) else {
            // The root lists in no parent; it is a folder.
            return Ok(Listed::folder("/"));
        };
        let name = remote_file_name(path);
        self.read_dir(ftp, &parent)?.into_iter().find(|e| e.name == name).ok_or_else(missing)
    }

    /// What `path` is. MLST answers for a file in one command; anything that
    /// may be a link to a folder is looked up in its parent's listing, which
    /// tells links apart.
    fn listed(&self, ftp: &mut RustlsFtpStream, path: &str) -> Result<Listed> {
        // A refusal of MLST does not say whether the file is missing; the parent's listing does.
        if self.features.mlst
            && let Some(entry) = ftp.mlst(Some(path)).ok().and_then(|line| parse_mlsx(&line))
            && !entry.is_dir
        {
            return Ok(entry);
        }
        self.lookup(ftp, path)
    }

    fn remove_file(&self, ftp: &mut RustlsFtpStream, path: &str) -> Result<()> {
        ftp.rm(path).map_err(|e| ftp_err(Msg::new("remote.delete").with("path", path), e))
    }

    fn rename_in(&self, ftp: &mut RustlsFtpStream, from: &str, to: &str) -> Result<()> {
        ftp.rename(from, to)
            .map_err(|e| ftp_err(Msg::new("remote.rename").with("from", from).with("to", to), e))
    }

    fn delete_in(&self, ftp: &mut RustlsFtpStream, path: &str) -> Result<()> {
        // A link to a folder is removed as a link, never followed.
        let listed = self.listed(ftp, path)?;
        if listed.is_dir && !listed.is_symlink {
            for entry in self.read_dir(ftp, path)? {
                self.delete_in(ftp, &remote_join(path, &entry.name))?;
            }
            ftp.rmdir(path).map_err(|e| ftp_err(Msg::new("remote.rmdir").with("path", path), e))
        } else {
            self.remove_file(ftp, path)
        }
    }

    /// The server's date for a file, when it can tell.
    fn modified(&self, ftp: &mut RustlsFtpStream, path: &str) -> Option<i64> {
        if !self.features.mdtm {
            return None;
        }
        let reply = ftp.custom_command(format!("MDTM {path}"), &[Status::File]).ok()?;
        let text = String::from_utf8_lossy(&reply.body);
        parse_ftp_time(text.split_whitespace().last()?)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Target {
    Folder,
    File(u64),
}

/// Keeps the session's lock and gives the stream itself.
struct FtpGuard<'a>(MutexGuard<'a, Option<RustlsFtpStream>>);

impl std::ops::Deref for FtpGuard<'_> {
    type Target = RustlsFtpStream;
    fn deref(&self) -> &RustlsFtpStream {
        self.0.as_ref().expect("checked when locked")
    }
}

impl std::ops::DerefMut for FtpGuard<'_> {
    fn deref_mut(&mut self) -> &mut RustlsFtpStream {
        self.0.as_mut().expect("checked when locked")
    }
}

impl Protocol for FtpConnection {
    fn connect(&mut self) -> Result<()> {
        let (ftp, features) = open(&self.params)?;
        self.features = features;
        *self.ftp.lock().unwrap_or_else(|e| e.into_inner()) = Some(ftp);
        Ok(())
    }

    fn disconnect(&mut self) -> Result<()> {
        if let Some(mut ftp) = self.ftp.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = ftp.quit();
        }
        Ok(())
    }

    fn is_alive(&self) -> bool {
        self.session().is_ok_and(|mut ftp| ftp.noop().is_ok())
    }

    /// Skipped while a transfer holds the session: FTP allows no command then.
    fn keepalive(&self) {
        let Ok(mut guard) = self.ftp.try_lock() else { return };
        if let Some(ftp) = guard.as_mut() {
            let _ = ftp.noop();
        }
    }

    fn realpath(&self, path: &str) -> Result<String> {
        let mut ftp = self.session()?;
        let context = || Msg::new("remote.realpath").with("path", path);
        if path != "." {
            ftp.cwd(path).map_err(|e| ftp_err(context(), e))?;
        }
        ftp.pwd().map_err(|e| ftp_err(context(), e))
    }

    fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>> {
        let mut ftp = self.session()?;
        let listed = self.read_dir(&mut ftp, path)?;
        let mut entries: Vec<FileEntry> = listed
            .into_iter()
            .map(|l| {
                let entry_path = remote_join(path, &l.name);
                self.to_entry(&mut ftp, entry_path, l)
            })
            .collect();
        sort_entries(&mut entries);
        Ok(entries)
    }

    fn stat(&self, path: &str) -> Result<FileEntry> {
        let mut ftp = self.session()?;
        let listed = self.listed(&mut ftp, path)?;
        let mut entry = self.to_entry(&mut ftp, path.to_string(), listed);
        entry.name = remote_file_name(path).to_string();
        // Editing compares dates to see whether the server's copy changed.
        if !entry.is_dir && !self.features.mlst {
            entry.modified = self.modified(&mut ftp, path).or(entry.modified);
        }
        Ok(entry)
    }

    fn exact_modified(&self, path: &str, listed: Option<i64>) -> Option<i64> {
        if self.features.mlst {
            return listed;
        }
        let mut ftp = self.session().ok()?;
        self.modified(&mut ftp, path).or(listed)
    }

    fn exists(&self, path: &str) -> bool {
        let Ok(mut ftp) = self.session() else { return false };
        if self.features.mlst {
            return ftp.mlst(Some(path)).is_ok();
        }
        self.target(&mut ftp, path).is_some() || self.lookup(&mut ftp, path).is_ok()
    }

    fn mkdir(&self, path: &str) -> Result<()> {
        self.session()?
            .mkdir(path)
            .map_err(|e| ftp_err(Msg::new("remote.mkdir").with("path", path), e))
    }

    fn rename(&self, from: &str, to: &str) -> Result<()> {
        let mut ftp = self.session()?;
        self.rename_in(&mut ftp, from, to)
    }

    fn remove_empty_dir(&self, path: &str) -> Result<()> {
        self.session()?
            .rmdir(path)
            .map_err(|e| ftp_err(Msg::new("remote.rmdir").with("path", path), e))
    }

    fn delete(&self, path: &str) -> Result<()> {
        let mut ftp = self.session()?;
        self.delete_in(&mut ftp, path)
    }

    fn set_permissions(&self, path: &str, mode: u32) -> Result<()> {
        let context = || Msg::new("remote.chmod").with("path", path);
        self.session()?.site(format!("CHMOD {:o} {path}", mode & 0o7777)).map(|_| ()).map_err(|e| {
            match ftp_err(context(), e) {
                // Servers word it vaguely ("SITE CHMOD command failed"); on an
                // existing file, a refused chmod is a matter of rights.
                OmniError::Protocol(m) if m.code != "remote.notFound" => OmniError::Permission(m),
                other => other,
            }
        })
    }

    fn read_link(&self, path: &str) -> Result<String> {
        let mut ftp = self.session()?;
        self.lookup(&mut ftp, path)?
            .link_target
            .ok_or_else(|| OmniError::Protocol(Msg::new("remote.readlink").with("path", path)))
    }

    fn upload(&self, local: &Path, remote: &str, progress: Progress) -> Result<()> {
        let mut ftp = self.session()?;
        let existing = self.target(&mut ftp, remote);
        if existing == Some(Target::Folder) {
            return Err(OmniError::Transfer(Msg::new("transfer.targetIsFolder").with("path", remote)));
        }
        // The permissions of a file being replaced are kept.
        let replaced_mode = existing.and_then(|_| self.listed(&mut ftp, remote).ok()).and_then(|l| l.mode);

        let mut local_file =
            std::fs::File::open(local).map_err(|e| OmniError::io_context(local.display(), e))?;
        let temp = temp_path_beside(remote);
        let write_err = |e: std::io::Error| OmniError::Transfer(Msg::new("remote.write").with("path", remote).detail(e));

        let mut stream = ftp
            .put_with_stream(&temp)
            .map_err(|e| ftp_err(Msg::new("remote.tempFile").with("path", remote), e))?;
        let copied = copy_with_progress(&mut local_file, &mut stream, progress);
        let result = match copied {
            Ok(()) => stream
                .finish()
                .map_err(|e| ftp_err(Msg::new("remote.write").with("path", remote), e)),
            Err(e) => {
                // Closing the data connection ends the transfer; the server's
                // reply to it must still be read. ABOR is not used: a server
                // that already had everything answers it twice.
                let _ = stream.finish();
                Err(match e {
                    CopyError::Cancelled => OmniError::Cancelled,
                    CopyError::Read(e) => OmniError::io_context(local.display(), e),
                    CopyError::Write(e) => write_err(e),
                })
            }
        };

        let result = result.and_then(|()| {
            // Keep the source's date (sync compares them); not worth failing the upload over.
            let mtime = local_file
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64);
            if let Some(mtime) = mtime {
                let time = format_ftp_time(mtime);
                // MFMT is the standard; vsftpd and a few others take a date before the name in MDTM.
                let command = if self.features.mfmt { format!("MFMT {time} {temp}") } else { format!("MDTM {time} {temp}") };
                if self.features.mfmt || self.features.mdtm {
                    let _ = ftp.custom_command(command, &[Status::File]);
                }
            }
            if let Some(mode) = replaced_mode {
                let _ = ftp.site(format!("CHMOD {mode:o} {temp}"));
            }
            // Some servers refuse to rename over a file: the old one is set aside until the new one is in place.
            if existing.is_none() {
                return self.rename_in(&mut ftp, &temp, remote);
            }
            let backup = temp_path_beside(remote);
            self.rename_in(&mut ftp, remote, &backup)?;
            if let Err(e) = self.rename_in(&mut ftp, &temp, remote) {
                let _ = self.rename_in(&mut ftp, &backup, remote);
                return Err(e);
            }
            let _ = self.remove_file(&mut ftp, &backup);
            Ok(())
        });

        if result.is_err() {
            self.recover(&mut ftp);
            let _ = ftp.rm(&temp);
        }
        result
    }

    fn download(&self, remote: &str, local: &Path, progress: Progress) -> Result<()> {
        let mut ftp = self.session()?;
        let mtime = self.modified(&mut ftp, remote).map(|t| t.max(0) as u64);
        let mut stream = Some(
            ftp.retr_as_stream(remote)
                .map_err(|e| ftp_err(Msg::new("remote.open").with("path", remote), e))?,
        );

        let result = write_local_atomically(local, mtime, |local_file| {
            let data = stream.as_mut().expect("taken only below");
            copy_with_progress(data, local_file, progress).map_err(|e| match e {
                CopyError::Cancelled => OmniError::Cancelled,
                CopyError::Read(e) => OmniError::Transfer(Msg::new("remote.read").with("path", remote).detail(e)),
                CopyError::Write(e) => OmniError::io_context(local.display(), e),
            })?;
            // A file the server could not send in full only shows in its final
            // reply, which must come before the local file is replaced.
            let data = stream.take().expect("still there");
            data.finish().map_err(|e| ftp_err(Msg::new("remote.read").with("path", remote), e))
        });
        // Interrupted: closing the data connection stops the server, whose reply is read.
        if let Some(data) = stream.take() {
            let _ = data.finish();
            self.recover(&mut ftp);
        }
        result
    }
}

/// Opens an authenticated session, in binary mode, and says what the server can do.
///
/// FTPS starts with TLS 1.2: every data connection must resume the control
/// connection's TLS session on many servers, and TLS 1.3 tickets, good for one
/// resumption each, run out. A server that refuses TLS 1.2 gets TLS 1.3.
fn open(params: &ConnectParams) -> Result<(RustlsFtpStream, Features)> {
    match open_with(params, false) {
        Err(e) if e.code() == "ftps.handshake" => open_with(params, true),
        other => other,
    }
}

fn open_with(params: &ConnectParams, tls13: bool) -> Result<(RustlsFtpStream, Features)> {
    let profile = &params.profile;
    let address = resolve(&profile.host, profile.port)?;
    let tls = (profile.protocol == ProtocolType::Ftps).then(|| TlsSetup::new(params, tls13));
    // A refused certificate shows as a failed handshake; the verifier knows why.
    let tls_err = |tls: &TlsSetup, e: FtpError| tls.refusal().unwrap_or_else(|| ftp_err(Msg::new("ftps.handshake"), e));

    let mut ftp = match &tls {
        Some(tls) if profile.implicit_tls => {
            let ftp = RustlsFtpStream::connect_secure_implicit(address, tls.connector(), &profile.host)
                .map_err(|e| tls_err(tls, e))?;
            let _ = ftp.get_ref().set_read_timeout(Some(IO_TIMEOUT));
            let _ = ftp.get_ref().set_write_timeout(Some(IO_TIMEOUT));
            // Explicit mode asks for encrypted data itself; implicit mode must too.
            let mut ftp = ftp;
            for command in ["PBSZ 0", "PROT P"] {
                ftp.custom_command(command, &[Status::CommandOk]).map_err(|e| tls_err(tls, e))?;
            }
            ftp
        }
        _ => RustlsFtpStream::connect_with_stream(open_tcp(address)?)
            .map_err(|e| ftp_err(Msg::new("net.connect").with("address", address), e))?,
    };
    if let Some(tls) = &tls
        && !profile.implicit_tls
    {
        ftp = ftp.into_secure(tls.connector(), &profile.host).map_err(|e| tls_err(tls, e))?;
    }

    // A server behind NAT often gives its private address for data
    // connections; the address already reached is the one to use.
    ftp.set_mode(if address.is_ipv6() { Mode::ExtendedPassive } else { Mode::Passive });
    ftp.set_passive_nat_workaround(true);
    let mut ftp = ftp.passive_stream_builder(|addr| open_tcp(addr).map_err(|_| {
        FtpError::ConnectionError(std::io::Error::new(std::io::ErrorKind::TimedOut, "data connection"))
    }));

    let password = params.password.as_deref().unwrap_or_default();
    ftp.login(profile.username.as_str(), password).map_err(|e| match e {
        FtpError::UnexpectedResponse(r) => OmniError::Auth(
            Msg::new("auth.password").with("user", &profile.username).detail(reply_text(&r.body)),
        ),
        e => ftp_err(Msg::new("auth.failed"), e),
    })?;

    if let Some(tls) = &tls {
        tls.remember_accepted()?;
    }

    ftp.transfer_type(FileType::Binary)
        .map_err(|e| ftp_err(Msg::new("ftp.binary"), e))?;
    let features = ftp.feat().unwrap_or_default();
    let has = |name: &str| features.keys().any(|k| k.eq_ignore_ascii_case(name));
    if has("UTF8") {
        let _ = ftp.opts("UTF8", Some("ON"));
    }
    Ok((ftp, Features { mlst: has("MLST"), mfmt: has("MFMT"), mdtm: has("MDTM") }))
}


/// An entry as a listing line describes it.
#[derive(Debug, Clone, PartialEq)]
struct Listed {
    name: String,
    is_dir: bool,
    is_symlink: bool,
    size: u64,
    modified: Option<i64>,
    mode: Option<u32>,
    owner: Option<String>,
    group: Option<String>,
    link_target: Option<String>,
    /// MLSD's own entries for the folder and its parent.
    is_parent_or_self: bool,
}

impl Listed {
    fn folder(name: &str) -> Self {
        Self {
            name: name.to_string(),
            is_dir: true,
            is_symlink: false,
            size: 0,
            modified: None,
            mode: None,
            owner: None,
            group: None,
            link_target: None,
            is_parent_or_self: false,
        }
    }
}

impl From<LsLine> for Listed {
    fn from(line: LsLine) -> Self {
        Self {
            name: line.name,
            is_dir: line.is_dir,
            is_symlink: line.is_symlink,
            size: line.size,
            modified: line.modified,
            mode: Some(line.full_mode),
            owner: Some(line.owner),
            group: Some(line.group),
            link_target: line.link_target,
            is_parent_or_self: false,
        }
    }
}

/// Parses an MLSD or MLST line (RFC 3659): `type=file;size=12;modify=20261105163703; name`.
fn parse_mlsx(line: &str) -> Option<Listed> {
    let (facts, name) = line.trim_start().split_once(' ')?;
    if name.is_empty() {
        return None;
    }
    // MLST gives the whole path; only its last part is the name.
    let name = if name.len() > 1 { remote_file_name(name) } else { name };
    let facts: HashMap<String, &str> = facts
        .split(';')
        .filter_map(|f| f.split_once('='))
        .map(|(k, v)| (k.to_ascii_lowercase(), v))
        .collect();
    let kind = facts.get("type")?.to_ascii_lowercase();
    let link_target = kind.strip_prefix("os.unix=slink:").or(kind.strip_prefix("os.unix=symlink:")).map(|_| {
        // Keep the target's case, which the lowercased copy lost.
        let original = facts["type"];
        original[original.find(':').map_or(0, |i| i + 1)..].to_string()
    });
    let is_symlink = kind.starts_with("os.unix=slink") || kind.starts_with("os.unix=symlink");
    Some(Listed {
        name: name.to_string(),
        is_dir: matches!(kind.as_str(), "dir" | "cdir" | "pdir"),
        is_symlink,
        size: facts.get("size").and_then(|s| s.parse().ok()).unwrap_or(0),
        modified: facts.get("modify").and_then(|t| parse_ftp_time(t)),
        mode: facts.get("unix.mode").and_then(|m| u32::from_str_radix(m, 8).ok()).map(|m| m & 0o7777),
        owner: facts.get("unix.ownername").or(facts.get("unix.owner")).map(|s| s.to_string()),
        group: facts.get("unix.groupname").or(facts.get("unix.group")).map(|s| s.to_string()),
        link_target,
        is_parent_or_self: matches!(kind.as_str(), "cdir" | "pdir"),
    })
}

/// Parses a listing line from a Windows server: `11-05-26  04:37PM  <DIR>  name`.
fn parse_dos_line(line: &str) -> Option<Listed> {
    let mut words = line.split_whitespace();
    let date = words.next()?;
    let time = words.next()?;
    let size_or_dir = words.next()?;
    // The name starts after the third word and keeps its own spaces.
    let after_third = {
        let mut rest = line.trim_start();
        for _ in 0..3 {
            rest = rest[rest.find(char::is_whitespace)?..].trim_start();
        }
        rest
    };
    if after_third.is_empty() {
        return None;
    }

    let mut parts = date.split('-');
    let (month, day, year) = (parts.next()?.parse::<i64>().ok()?, parts.next()?.parse::<i64>().ok()?, parts.next()?);
    let year: i64 = year.parse().ok()?;
    let year = if year < 100 { if year < 70 { 2000 + year } else { 1900 + year } } else { year };
    let (clock, half) = time.split_at(time.len().checked_sub(2)?);
    let (hours, minutes) = clock.split_once(':')?;
    let mut hours: i64 = hours.parse().ok()?;
    let minutes: i64 = minutes.parse().ok()?;
    match half.to_ascii_uppercase().as_str() {
        "AM" if hours == 12 => hours = 0,
        "PM" if hours < 12 => hours += 12,
        "AM" | "PM" => {}
        _ => return None,
    }
    let is_dir = size_or_dir.eq_ignore_ascii_case("<DIR>");
    let size = if is_dir { 0 } else { size_or_dir.replace(',', "").parse().ok()? };
    Some(Listed {
        size,
        modified: Some(days_from_civil(year, month, day) * 86400 + hours * 3600 + minutes * 60),
        ..Listed { is_dir, ..Listed::folder(after_third) }
    })
}

/// `YYYYMMDDHHMMSS[.sss]`, always UTC.
fn parse_ftp_time(text: &str) -> Option<i64> {
    let digits = text.split('.').next()?;
    if digits.len() != 14 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let field = |range: std::ops::Range<usize>| digits[range].parse::<i64>().ok();
    let days = days_from_civil(field(0..4)?, field(4..6)?, field(6..8)?);
    Some(days * 86400 + field(8..10)? * 3600 + field(10..12)? * 60 + field(12..14)?)
}

fn format_ftp_time(epoch: i64) -> String {
    let days = epoch.div_euclid(86400);
    let seconds = epoch.rem_euclid(86400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}{:02}{:02}{:02}",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60
    )
}

fn listing_err(path: &str, line: &str) -> OmniError {
    OmniError::Protocol(Msg::new("ftp.listing").with("path", path).detail(line.trim()))
}

fn reply_text(body: &[u8]) -> String {
    String::from_utf8_lossy(body).trim().to_string()
}

/// Maps a failed command, telling missing files and refusals apart where the reply allows.
fn ftp_err(context: Msg, err: FtpError) -> OmniError {
    match err {
        FtpError::UnexpectedResponse(response) => {
            let text = reply_text(&response.body);
            let lower = text.to_ascii_lowercase();
            match response.status.code() {
                421 => OmniError::ConnectionLost,
                530 => OmniError::Auth(context.detail(text)),
                550 | 553
                    if ["denied", "permission", "not allowed", "not permitted"].iter().any(|w| lower.contains(w)) =>
                {
                    OmniError::Permission(context.detail(text))
                }
                550 if lower.contains("no such") || lower.contains("not found") || lower.contains("does not exist") => {
                    OmniError::Protocol(not_found(&context))
                }
                _ => OmniError::Protocol(context.detail(text)),
            }
        }
        FtpError::ConnectionError(e) => OmniError::Network(context.detail(e)),
        other => OmniError::Protocol(context.detail(other)),
    }
}

fn resolve(host: &str, port: u16) -> Result<SocketAddr> {
    let address = format!("{host}:{port}");
    address
        .to_socket_addrs()
        .map_err(|e| OmniError::Network(Msg::new("net.resolve").with("address", &address).detail(e)))?
        .next()
        .ok_or_else(|| OmniError::Network(Msg::new("net.noAddress").with("address", address)))
}

fn open_tcp(address: SocketAddr) -> Result<TcpStream> {
    let tcp = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)
        .map_err(|e| OmniError::Network(Msg::new("net.connect").with("address", address).detail(e)))?;
    let _ = tcp.set_read_timeout(Some(IO_TIMEOUT));
    let _ = tcp.set_write_timeout(Some(IO_TIMEOUT));
    Ok(tcp)
}

// --- Certificates -----------------------------------------------------------

/// `SHA256:` and the base64 digest of the certificate, as SSH fingerprints are written.
fn fingerprint(cert: &CertificateDer<'_>) -> String {
    format!("SHA256:{}", base64_no_pad(&Sha256::digest(cert.as_ref())))
}

fn crypto() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// The system's trusted authorities, read once.
fn system_roots() -> Arc<RootCertStore> {
    static ROOTS: OnceLock<Arc<RootCertStore>> = OnceLock::new();
    ROOTS
        .get_or_init(|| {
            let mut roots = RootCertStore::empty();
            roots.add_parsable_certificates(rustls_native_certs::load_native_certs().certs);
            Arc::new(roots)
        })
        .clone()
}

/// Remembered certificates: `host:port` to fingerprint.
fn read_known(path: &Path) -> HashMap<String, String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once(' '))
        .map(|(k, v)| (k.to_string(), v.trim().to_string()))
        .collect()
}

fn write_known(path: &Path, known: &HashMap<String, String>) -> Result<()> {
    let mut lines: Vec<String> = known.iter().map(|(k, v)| format!("{k} {v}")).collect();
    lines.sort();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| OmniError::io_context(dir.display(), e))?;
    }
    std::fs::write(path, lines.join("\n") + "\n").map_err(|e| OmniError::io_context(path.display(), e))
}

#[derive(Debug, Clone, PartialEq)]
enum CertOutcome {
    Unknown(String),
    Changed(String),
    /// Accepted only because the user just confirmed it.
    Confirmed(String),
}

/// What one connection needs to check the server's certificate.
#[derive(Clone)]
struct TlsSetup {
    config: Arc<ClientConfig>,
    verifier: Arc<CertVerifier>,
}

impl TlsSetup {
    fn new(params: &ConnectParams, tls13: bool) -> Self {
        let profile = &params.profile;
        let known_path = params.known_hosts_path.with_file_name(KNOWN_CERTIFICATES_FILE);
        let key = format!("{}:{}", profile.host, profile.port);
        let provider = crypto();
        let roots = system_roots();
        let webpki = (!roots.is_empty())
            .then(|| WebPkiServerVerifier::builder_with_provider(roots, provider.clone()).build().ok())
            .flatten();
        let verifier = Arc::new(CertVerifier {
            known: read_known(&known_path).remove(&key),
            trusted: params.trusted_fingerprint.clone(),
            key,
            known_path,
            webpki,
            algorithms: provider.signature_verification_algorithms,
            outcome: Mutex::new(None),
        });
        // One config for the control connection and every data connection, so
        // they share TLS sessions: many servers insist on it.
        let config = ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[if tls13 { &rustls::version::TLS13 } else { &rustls::version::TLS12 }])
            .expect("the ring provider supports TLS 1.2 and 1.3")
            .dangerous()
            .with_custom_certificate_verifier(verifier.clone())
            .with_no_client_auth();
        Self { config: Arc::new(config), verifier }
    }

    fn connector(&self) -> RustlsConnector {
        RustlsConnector::from(self.config.clone())
    }

    /// The error to report when the handshake failed over the certificate.
    fn refusal(&self) -> Option<OmniError> {
        match self.verifier.outcome() {
            Some(CertOutcome::Unknown(fp)) => Some(OmniError::HostKeyUnknown(fp)),
            Some(CertOutcome::Changed(fp)) => Some(OmniError::HostKeyMismatch(fp)),
            _ => None,
        }
    }

    /// Records a certificate the user confirmed, once the login worked.
    fn remember_accepted(&self) -> Result<()> {
        let Some(CertOutcome::Confirmed(fp)) = self.verifier.outcome() else {
            return Ok(());
        };
        let mut known = read_known(&self.verifier.known_path);
        known.insert(self.verifier.key.clone(), fp);
        write_known(&self.verifier.known_path, &known)
    }
}

struct CertVerifier {
    /// `host:port`, under which the certificate is remembered.
    key: String,
    known_path: PathBuf,
    /// The fingerprint remembered for this server, if any.
    known: Option<String>,
    /// The fingerprint the user has just confirmed.
    trusted: Option<String>,
    /// Checks against the system's authorities; `None` when it has none.
    webpki: Option<Arc<WebPkiServerVerifier>>,
    algorithms: WebPkiSupportedAlgorithms,
    outcome: Mutex<Option<CertOutcome>>,
}

impl std::fmt::Debug for CertVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CertVerifier").field("key", &self.key).finish_non_exhaustive()
    }
}

impl CertVerifier {
    fn outcome(&self) -> Option<CertOutcome> {
        self.outcome.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn record(&self, outcome: CertOutcome) {
        *self.outcome.lock().unwrap_or_else(|e| e.into_inner()) = Some(outcome);
    }
}

impl ServerCertVerifier for CertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        let signed_by_authority = self.webpki.as_ref().is_some_and(|v| {
            v.verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now).is_ok()
        });
        if signed_by_authority {
            return Ok(ServerCertVerified::assertion());
        }
        let fp = fingerprint(end_entity);
        if self.known.as_deref() == Some(fp.as_str()) {
            return Ok(ServerCertVerified::assertion());
        }
        if self.trusted.as_deref() == Some(fp.as_str()) {
            self.record(CertOutcome::Confirmed(fp));
            return Ok(ServerCertVerified::assertion());
        }
        self.record(if self.known.is_some() { CertOutcome::Changed(fp) } else { CertOutcome::Unknown(fp) });
        Err(rustls::Error::General("server certificate not trusted".into()))
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mlsd_lines() {
        let file = parse_mlsx("type=file;size=1234;modify=20261105163703.123;UNIX.mode=0644;UNIX.owner=1000; report final.pdf").unwrap();
        assert_eq!(file.name, "report final.pdf");
        assert_eq!((file.size, file.modified, file.mode), (1234, Some(1793896623), Some(0o644)));
        assert_eq!(file.owner.as_deref(), Some("1000"));
        assert!(!file.is_dir && !file.is_symlink && !file.is_parent_or_self);

        let dir = parse_mlsx("Type=dir;Modify=20261105163703;UNIX.ownername=www-data; sub").unwrap();
        assert!(dir.is_dir);
        assert_eq!(dir.owner.as_deref(), Some("www-data"));

        assert!(parse_mlsx("type=cdir;modify=20261105163703; .").unwrap().is_parent_or_self);
        assert!(parse_mlsx("type=pdir; ..").unwrap().is_parent_or_self);

        let link = parse_mlsx("type=OS.unix=slink:/var/Target;size=10; current").unwrap();
        assert!(link.is_symlink);
        assert_eq!(link.link_target.as_deref(), Some("/var/Target"));

        // MLST names the whole path.
        assert_eq!(parse_mlsx(" type=file;size=1; /home/test/a.txt").unwrap().name, "a.txt");
        assert_eq!(parse_mlsx("type=dir; /").unwrap().name, "/");

        for bad in ["", "no facts", "type=file;size=1;", "size=1; noname"] {
            assert!(parse_mlsx(bad).is_none(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn windows_listing_lines() {
        let dir = parse_dos_line("11-05-26  04:37PM       <DIR>          My Documents").unwrap();
        assert!(dir.is_dir);
        assert_eq!(dir.name, "My Documents");
        assert_eq!(dir.modified, Some(days_from_civil(2026, 11, 5) * 86400 + 16 * 3600 + 37 * 60));

        let file = parse_dos_line("01-02-2024  12:05AM            1,234,567 setup.exe").unwrap();
        assert_eq!((file.size, file.is_dir, file.name.as_str()), (1234567, false, "setup.exe"));
        assert_eq!(file.modified, Some(days_from_civil(2024, 1, 2) * 86400 + 5 * 60));

        for bad in ["", "garbage line here", "11-05-26  04:37XX  12 a", "11-05-26  04:37PM  12"] {
            assert!(parse_dos_line(bad).is_none(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn ftp_times_round_trip() {
        assert_eq!(parse_ftp_time("20261105163703"), Some(1793896623));
        assert_eq!(parse_ftp_time("20261105163703.5"), Some(1793896623));
        assert_eq!(format_ftp_time(1793896623), "20261105163703");
        assert_eq!(format_ftp_time(0), "19700101000000");
        assert_eq!(parse_ftp_time(&format_ftp_time(951782400)), Some(951782400)); // 2000-02-29
        assert_eq!(parse_ftp_time("2026110516370"), None);
        assert_eq!(parse_ftp_time("2026-11-05 16:37"), None);
    }

    #[test]
    fn replies_tell_refusals_from_missing_files() {
        let reply = |code: u32, text: &str| {
            FtpError::UnexpectedResponse(suppaftp::types::Response { status: Status::from(code), body: text.as_bytes().to_vec() })
        };
        let context = || Msg::new("remote.delete").with("path", "/a");
        assert!(matches!(ftp_err(context(), reply(550, "550 Permission denied.")), OmniError::Permission(_)));
        assert!(matches!(ftp_err(context(), reply(553, "553 Not allowed")), OmniError::Permission(_)));
        let missing = ftp_err(context(), reply(550, "550 No such file or directory"));
        assert!(matches!(&missing, OmniError::Protocol(m) if m.code == "remote.notFound"), "{missing:?}");
        assert!(matches!(ftp_err(context(), reply(421, "421 Timeout.")), OmniError::ConnectionLost));
        assert!(matches!(ftp_err(context(), reply(550, "550 Failed.")), OmniError::Protocol(_)));
    }

    #[test]
    fn known_certificates_file_round_trip() {
        let dir = std::env::temp_dir().join(format!("omniscp-certs-{}", std::process::id()));
        let path = dir.join(KNOWN_CERTIFICATES_FILE);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(read_known(&path).is_empty());
        let mut known = HashMap::new();
        known.insert("ftp.example.com:21".to_string(), "SHA256:abc".to_string());
        known.insert("10.0.0.1:990".to_string(), "SHA256:def".to_string());
        write_known(&path, &known).unwrap();
        assert_eq!(read_known(&path), known);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
