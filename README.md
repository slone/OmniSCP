# OmniSCP

[![CI](https://github.com/slone/OmniSCP/actions/workflows/ci.yml/badge.svg)](https://github.com/slone/OmniSCP/actions/workflows/ci.yml)

A cross-platform file transfer client for SFTP, SCP, FTP and FTPS, in the spirit of WinSCP: a local and a remote pane side by side, a transfer queue, folder synchronization with a preview, and connection profiles whose passwords stay in the system keychain.

Built with [Tauri 2](https://tauri.app) (Rust) and Vue 3. The interface is in French and English.

> **Status: early (0.1).** Developed and tested on macOS. Linux and Windows build and pass the tests in CI, but have not been tried by hand yet.

## Download

Installers for macOS (universal), Windows (`.msi` or setup `.exe`) and Linux (`.deb`, `.rpm`, `.AppImage`) are attached to each [release](https://github.com/slone/OmniSCP/releases).

They are not signed yet. On macOS, the first launch is refused: allow it under System Settings → Privacy & Security, "Open Anyway". On Windows, SmartScreen warns first: choose "More info", then "Run anyway".

## Features

**Browsing**
- Local and remote panes with sorting, a name filter, back/forward history and hidden files on or off
- Create folders, rename, delete, see properties, change Unix permissions, compute a folder's size
- Keyboard first: arrow keys, Home/End and Shift to select, plus the shortcuts below

**Transfers**
- A background queue with several transfers at once, each on its own session, so browsing never waits
- Pause and resume a single transfer or the whole queue (a paused transfer keeps its connection), cancel, and retry only what is missing
- Copy with F5 or by dragging between panes; move with F6 (originals are deleted only if everything was copied)
- Drop files or folders from Finder, Explorer or your file manager onto the remote pane to upload them
- When a file already exists: ask, overwrite, skip or rename, with "apply to all"
- Files are written to a temporary name and swapped in when complete, and keep their modification date

**Synchronization**
- Upload, download or both ways, with exclusion patterns
- A preview of every action before anything runs; deletions are off by default and need a second confirmation
- Nothing is touched if it changed since the preview

**Remote editing**
- F4 opens a remote file in its usual local application; every save is sent back
- If the file changed on the server in the meantime, nothing is overwritten: you choose

**Profiles and security**
- Profiles with groups, tags, search, duplication and a connection test
- Passwords and passphrases are kept in the system keychain (macOS Keychain, Windows Credential Manager, Secret Service on Linux), never in the profiles file, the logs or exports
- Import and export of profiles, without any secret
- Server keys are checked against `~/.ssh/known_hosts` and the app's own list, and FTPS certificates against the system's authorities; an unknown key or certificate is shown for confirmation, a changed one refuses the connection
- Authentication by password, key file, key with passphrase or SSH agent
- An operation log, which can be exported

## Keyboard shortcuts

| Key | Action |
| --- | --- |
| F5 | Copy the selection to the other pane |
| F6 | Move the selection to the other pane |
| F4 | Edit a remote file in a local application |
| F7 | New folder |
| F2 | Rename |
| Delete | Delete |
| Ctrl+I / ⌘I | Properties and permissions |
| Ctrl+R / ⌘R | Refresh |
| Alt+← / Alt+→ | Back / forward |
| Backspace | Parent folder |
| Ctrl+A / ⌘A | Select all |

## Protocols

- **SFTP** and **SCP**, both over SSH. SCP runs shell commands on the server for everything but the file contents (listing, renaming, deleting), so it needs a POSIX shell there; prefer SFTP when the server offers it.
- **FTPS**, FTP over TLS, explicit (port 21) or implicit (port 990). A certificate from a recognised authority is accepted as is; any other, such as a self-signed one, is shown for confirmation like an unknown SSH key, then remembered.
- **FTP**, unencrypted: the password and the files cross the network in clear. Use it only where nothing better is offered.

With FTP and FTPS, dates are exact to the second when the server supports MLSD or MDTM, which most do, and kept on upload when it supports MFMT or vsftpd's MDTM. Passive mode only.

WebDAV is planned.

## Building

You need:

- [Node.js](https://nodejs.org) 20.19+ or 22.12+
- [Rust](https://rustup.rs), stable (1.85 or later, for the 2024 edition)
- The [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your system
- Perl and a C compiler, to build the bundled OpenSSL

Then:

```bash
npm install
npm run tauri dev      # run with hot reload
npm run tauri build    # build the application and its installers
```

On Linux, saved passwords need a Secret Service provider such as GNOME Keyring or KWallet.

## Tests

```bash
cd src-tauri
cargo test             # unit tests
cargo clippy           # lints
```

Tests that need a real server are ignored by default. They run against Docker containers: an SFTP server, two SCP servers whose `ls` differ (Debian and Alpine), and two FTP servers (vsftpd, also serving FTPS, and ProFTPD). How to start them and which variables to set is written at the top of each file in `src-tauri/tests/`.

GitHub Actions runs the unit tests and Clippy on Linux, macOS and Windows, and the live tests against those servers on Linux (`.github/workflows/ci.yml`).

## Where things are kept

Profiles, settings and accepted server keys are in the application's configuration folder (on macOS, `~/Library/Application Support/com.guillaumechantraine.omniscp`). The log is in the system's log folder, and copies of files being edited are in the cache folder; they are removed after a week.

## Project layout

```
src/                 Vue 3 interface
  components/        panes, dialogs, queue, sync, profiles
  stores/            Pinia stores (connection, transfers, profiles…)
  i18n/              French and English texts
src-tauri/src/       Rust backend
  protocols/         SSH session, SFTP, SCP, FTP and FTPS
  transfer/          transfer queue
  sync/              synchronization plans
  config/            settings, profiles, keychain
  commands/          commands called by the interface
```

How a release is made and signed is in [`RELEASING.md`](RELEASING.md). The functional specification, in French, is in [`cahier-des-charges.md`](cahier-des-charges.md).

## License

OmniSCP is free software, released under the [GNU General Public License v3.0 or later](LICENSE).
