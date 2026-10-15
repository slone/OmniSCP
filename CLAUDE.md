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

SFTP (over SSH), SCP (over SSH), FTP, FTPS (FTP over TLS), WebDAV / WebDAV over HTTPS

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
- Required vs optional protocols
- Credential storage method
- Concurrent transfer limits
- File size / directory depth limits
- CLI/scripting support scope
- Log retention policy
- i18n scope (French confirmed; others TBD)
