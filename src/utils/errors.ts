// Errors rejected by Tauri commands, serialized from OmniError (src-tauri/src/error.rs).

import { t } from '../i18n';

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
  /// Key in the `errors` section of the dictionaries.
  code: string;
  params: Record<string, string> | null;
  /// Text from the system, the server or a library: shown as is.
  detail: string | null;
  /// The error this one wraps.
  cause: BackendError | null;
  /// Untranslated, for when the code is unknown.
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

type Template = (params: Record<string, string>) => string;

/// The translated sentence, then what it wraps, then the raw detail in parentheses.
export function describeError(err: BackendError): string {
  const template = (t.errors as Record<string, Template>)[err.code];
  let text = template ? template(err.params ?? {}) : err.message;
  if (err.cause) text += t.common.colon + describeError(err.cause);
  if (err.detail) text += ` (${err.detail})`;
  return text;
}

export function errorMessage(err: unknown): string {
  if (isBackendError(err)) return describeError(err);
  if (err instanceof Error) return err.message;
  return String(err);
}
