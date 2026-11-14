// Errors rejected by Tauri commands, serialized from OmniError (src-tauri/src/error.rs).

export type BackendErrorKind =
  | 'Network'
  | 'Auth'
  | 'Permission'
  | 'Protocol'
  | 'Transfer'
  | 'Config'
  | 'HostKeyUnknown'
  | 'HostKeyMismatch'
  | 'ConnectionLost'
  | 'Conflict'
  | 'Cancelled'
  | 'Io';

export interface BackendError {
  kind: BackendErrorKind;
  message: string;
  fingerprint: string | null;
}

export function isBackendError(err: unknown): err is BackendError {
  return (
    typeof err === 'object' &&
    err !== null &&
    typeof (err as BackendError).kind === 'string' &&
    typeof (err as BackendError).message === 'string'
  );
}

export function errorMessage(err: unknown): string {
  if (isBackendError(err)) return err.message;
  if (err instanceof Error) return err.message;
  return String(err);
}
