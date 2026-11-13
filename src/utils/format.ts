export function formatSize(bytes: number): string {
  if (bytes === 0) return '—';
  const units = ['o', 'Ko', 'Mo', 'Go'];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = (bytes / Math.pow(1024, i)).toLocaleString('fr-FR', {
    minimumFractionDigits: i > 0 ? 1 : 0,
    maximumFractionDigits: i > 0 ? 1 : 0,
  });
  return value + ' ' + units[i];
}

export function formatDate(timestamp?: number | null): string {
  if (!timestamp) return '—';
  return new Date(timestamp * 1000).toLocaleDateString('fr-FR', {
    day: '2-digit',
    month: '2-digit',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}
