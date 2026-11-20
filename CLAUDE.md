# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

OmniSCP is a cross-platform file transfer application inspired by WinSCP. It enables users to connect to remote servers, browse files, transfer data, and manage connection profiles through a graphical interface.

**Stack:** Tauri v2 (Rust backend) + Vue 3 (TypeScript frontend)
**Platforms:** macOS, Linux, Windows
**Specification:** `cahier-des-charges.md` (functional spec, in French)

## Build & Development Commands

```bash
npm install                  # Install frontend dependencies
npm run tauri dev            # Run app in development mode (hot-reload)
npm run tauri build          # Build production binary
npm run dev                  # Run frontend only (Vite dev server on :1420)
npm run build                # Build frontend only (vue-tsc + vite)
```

Rust backend is built automatically by Tauri. To work on the Rust side directly:

```bash
cd src-tauri
cargo build                  # Build backend
cargo test                   # Run backend tests
cargo clippy                 # Lint backend
```

Tests needing a real server are `#[ignore]`d. Their setup is at the top of each file in `src-tauri/tests/`: `sftp_live.rs`, `scp_live.rs` (run on both the Debian and Alpine servers, whose `ls` differ) and `transfer_live.rs` (queue and sync, over either protocol).

GitHub Actions (`.github/workflows/ci.yml`) builds the frontend and runs `cargo clippy -- -D warnings` and `cargo test` on Linux, macOS and Windows, and the ignored live tests on Linux against Docker servers. A Clippy warning fails the build, including one that only shows on another platform.

## Project Structure

```
├── src/                     # Vue 3 frontend (TypeScript)
│   ├── main.ts              # Vue app entry point
│   ├── App.vue              # Root component
│   └── assets/              # Static assets
├── src-tauri/               # Rust backend (Tauri)
│   ├── Cargo.toml           # Rust dependencies
│   ├── tauri.conf.json      # Tauri configuration
│   ├── capabilities/        # Tauri permission capabilities
│   ├── icons/               # App icons (all platforms)
│   └── src/
│       ├── main.rs          # Desktop entry point
│       └── lib.rs           # Tauri app builder, commands
├── index.html               # Vite entry HTML
├── package.json             # Frontend dependencies & scripts
├── vite.config.ts           # Vite config (port 1420, Tauri HMR)
└── cahier-des-charges.md    # Functional specification
```

## Supported Protocols

Available: SFTP and SCP, both over SSH. These two are the v1 scope. SCP runs shell commands on the server for everything but the file bytes, so it needs a POSIX shell there.

Planned for later: FTP, FTPS (FTP over TLS), WebDAV / WebDAV over HTTPS. Each implements the `Protocol` trait in `src-tauri/src/protocols/` and registers in `for_profile`.

## Architecture

The Rust backend must be organized into clearly separated modules:

- **Connections & Protocols** — multi-protocol connection handling with authentication (password, SSH keys, client certificates)
- **File Transfers** — bidirectional transfers with queue, progress tracking, pause/resume/cancel, conflict resolution
- **Configuration & Profiles** — connection profile CRUD, secure credential storage, import/export (secrets excluded by default)
- **Logging & Error Handling** — operation journal with timestamps, no secrets in logs

The Vue frontend handles the UI:

- Dual-pane local/remote file browser
- Profile manager, connection editor
- Operation queue with progress
- Sync screen, conflict dialogs
- Logs panel, application settings

Communication between frontend and backend uses Tauri commands (`#[tauri::command]` in Rust, `invoke()` in TypeScript).

### Content Security Policy

The webview runs under a strict CSP (`app.security.csp` in `tauri.conf.json`): only the app's own scripts and stylesheets, and no network but Tauri's IPC. Keep it that way: no inline `<style>` or `style="…"` strings built at run time, no `v-html`, nothing loaded from a CDN. Vue's `:style` bindings are fine (they go through the CSSOM). `devCsp` only relaxes what Vite's dev server needs.

### Languages

The interface is in French and English (setting: system, fr or en). Every visible text lives in `src/i18n/fr.ts`, the reference, and `src/i18n/en.ts`, which TypeScript holds to the same shape; components read them through `t` (`t.toolbar.connect`, `t.common.items(n)`). The backend never sends sentences: an `OmniError` carries a `Msg` with a code, its values and the raw system or server text as `detail` (`src-tauri/src/error.rs`), and the frontend translates the code from the `errors` section. A unit test fails when a code built in Rust has no translation in either dictionary. Journal entries are written in the language of the moment.

## Key Functional Requirements

- Dual-pane local/remote file navigation
- Drag-and-drop transfers; non-blocking UI during transfers
- Directory synchronization with preview before execution, automatic deletion disabled by default
- Conflict handling: overwrite, skip, rename, apply-to-all
- Server identity verification; encrypted protocol preference
- Optional CLI/scripting interface with verifiable exit codes
- Distinguish network errors, auth errors, and permission errors when possible

## Decisions Pending (Section 13 of spec)

- Minimum OS versions per platform
- Credential storage method
- Concurrent transfer limits
- File size / directory depth limits
- CLI/scripting support scope
- Log retention policy
