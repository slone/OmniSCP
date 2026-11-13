// Mirror types from Rust backend (src-tauri/src/)

export type ProtocolType = 'Sftp' | 'Scp' | 'Ftp' | 'Ftps' | 'WebDav' | 'WebDavs';

export type AuthMethod =
  | { type: 'Password' }
  | { type: 'KeyFile'; path: string }
  | { type: 'KeyWithPassphrase'; path: string }
  | { type: 'Agent' };

export interface Profile {
  id: string;
  name: string;
  host: string;
  port: number;
  protocol: ProtocolType;
  username: string;
  auth_method: AuthMethod;
  remote_directory?: string;
  description?: string;
  tags: string[];
  /// Password/passphrase kept in the OS credential store, never in this object.
  save_password?: boolean;
}

export interface FileEntry {
  name: string;
  path: string;
  /// For a symlink, describes the target.
  is_dir: boolean;
  is_symlink: boolean;
  size: number;
  modified?: number;
  permissions?: string;
  /// Permission bits including setuid/setgid/sticky; absent where Unix permissions do not exist.
  mode?: number;
  uid?: number;
  gid?: number;
  /// Account names, when the system could tell them.
  owner?: string;
  group?: string;
}

/// Payload of `local_folder_size` / `remote_folder_size` (src-tauri/src/tree.rs).
export interface TreeSize {
  bytes: number;
  files: number;
  folders: number;
  /// Folders that could not be listed: the totals are then a minimum.
  unreadable: number;
}

export type TransferDirection = 'Upload' | 'Download';

export type TransferStatus =
  | 'Pending'
  | 'InProgress'
  | 'Completed'
  | 'CompletedWithWarnings'
  | 'Skipped'
  | 'Cancelled'
  | 'Failed';

export type ConflictStrategy = 'Overwrite' | 'Skip' | 'Rename' | 'Ask' | 'Merge';

export interface TransferJob {
  id: string;
  source: string;
  destination: string;
  direction: TransferDirection;
  status: TransferStatus;
  total_bytes: number;
  transferred_bytes: number;
  conflict_strategy: ConflictStrategy;
}

/// Payload of the `transfer-progress` event (src-tauri/src/transfer/queue.rs).
export interface TransferEvent {
  id: string;
  status: TransferStatus;
  destination: string | null;
  total_bytes: number;
  transferred_bytes: number;
  error: string | null;
}

export type PaneSide = 'local' | 'remote';

export type SyncMode = 'Upload' | 'Download' | 'Both';

export type SyncActionKind =
  | 'Upload'
  | 'Download'
  | 'CreateRemoteDir'
  | 'CreateLocalDir'
  | 'DeleteRemote'
  | 'DeleteLocal'
  | 'Conflict';

export type SyncReason =
  | 'New'
  | 'SourceNewer'
  | 'TargetNewer'
  | 'SameDateDifferentSize'
  | 'TypeMismatch'
  | 'Orphan';

export interface EntryMeta {
  is_dir: boolean;
  size: number;
  modified: number | null;
}

export interface SyncAction {
  path: string;
  kind: SyncActionKind;
  reason: SyncReason;
  local: EntryMeta | null;
  remote: EntryMeta | null;
  selected: boolean;
}

export interface SyncPlan {
  actions: SyncAction[];
  problems: string[];
  skipped_links: string[];
}

export type LogLevel = 'Debug' | 'Info' | 'Warning' | 'Error';

export interface LogEntry {
  timestamp: number;
  level: LogLevel;
  action: string;
  status: string;
  detail?: string;
}

export type Theme = 'system' | 'dark' | 'light';

export type DefaultConflict = 'Ask' | 'Overwrite' | 'Skip' | 'Rename';

/// Mirrors `AppConfig` in src-tauri/src/config/mod.rs.
export interface AppConfig {
  language: string;
  theme: Theme;
  /// Applies from the next connection.
  concurrent_transfers: number;
  show_hidden_files: boolean;
  default_conflict: DefaultConflict;
}
