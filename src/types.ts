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
  owner?: string;
  group?: string;
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

export type ConflictStrategy = 'Overwrite' | 'Skip' | 'Rename' | 'Ask';

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

export type LogLevel = 'Debug' | 'Info' | 'Warning' | 'Error';

export interface LogEntry {
  timestamp: number;
  level: LogLevel;
  action: string;
  status: string;
  detail?: string;
}

export interface AppConfig {
  language: string;
  theme: string;
  concurrent_transfers: number;
}
