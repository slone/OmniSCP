# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

MultiSCP is a cross-platform file transfer application inspired by WinSCP. It enables users to connect to remote servers, browse files, transfer data, and manage connection profiles through a graphical interface.

**Language:** Rust (mandatory requirement)
**Platforms:** macOS, Linux, Windows
**Status:** Requirements/specification phase — `cahier-des-charges.md` contains the complete functional specification (in French)

## Supported Protocols

SFTP (over SSH), SCP (over SSH), FTP, FTPS (FTP over TLS), WebDAV / WebDAV over HTTPS

## Architecture (as specified)

The codebase must be organized into clearly separated modules:

- **UI** — graphical interface (dual-pane file browser, profile manager, operation queue, sync screen, logs panel, settings)
- **Connections & Protocols** — multi-protocol connection handling with authentication (password, SSH keys, client certificates)
- **File Transfers** — bidirectional transfers with queue, progress tracking, pause/resume/cancel, conflict resolution
- **Configuration & Profiles** — connection profile CRUD, secure credential storage, import/export (secrets excluded by default)
- **Logging & Error Handling** — operation journal with timestamps, no secrets in logs

## Key Functional Requirements

- Dual-pane local/remote file navigation
- Drag-and-drop transfers; non-blocking UI during transfers
- Directory synchronization with preview before execution, automatic deletion disabled by default
- Conflict handling: overwrite, skip, rename, apply-to-all
- Server identity verification; encrypted protocol preference
- Optional CLI/scripting interface with verifiable exit codes
- Distinguish network errors, auth errors, and permission errors when possible

## Decisions Pending (Section 13 of spec)

Before implementation, these must be resolved:
- Minimum OS versions per platform
- Required vs optional protocols
- UI framework choice (must be Rust-compatible and cross-platform)
- Credential storage method
- Concurrent transfer limits
- File size / directory depth limits
- CLI/scripting support scope
- Log retention policy
- i18n scope (French confirmed; others TBD)
