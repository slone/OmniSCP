import { intlLocale, t } from '../i18n';

export function formatSize(bytes: number): string {
  if (bytes === 0) return '—';
  const units = t.common.sizeUnits;
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = (bytes / Math.pow(1024, i)).toLocaleString(intlLocale(), {
    minimumFractionDigits: i > 0 ? 1 : 0,
    maximumFractionDigits: i > 0 ? 1 : 0,
  });
  return value + ' ' + units[i];
}

export function formatDate(timestamp?: number | null): string {
  if (!timestamp) return '—';
  return new Date(timestamp * 1000).toLocaleDateString(intlLocale(), {
    day: '2-digit',
    month: '2-digit',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

export function formatDateTime(timestamp?: number | null): string {
  return timestamp ? new Date(timestamp * 1000).toLocaleString(intlLocale()) : '—';
}

export function formatTime(ms: number): string {
  return new Date(ms).toLocaleTimeString(intlLocale());
}
