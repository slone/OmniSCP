<script setup lang="ts">
import { useTransfersStore, type QueuedTransfer } from '../../stores/transfers';

const transfers = useTransfersStore();

const directionIcon = { Upload: '↑', Download: '↓', Sync: '⇄' };

function statusLabel(status: string): string {
  const map: Record<string, string> = {
    Pending: 'En attente',
    InProgress: 'En cours',
    Paused: 'En pause',
    Completed: 'Terminé',
    CompletedWithWarnings: 'Terminé avec avertissements',
    Skipped: 'Ignoré',
    Cancelled: 'Annulé',
    Failed: 'Échoué',
  };
  return map[status] ?? status;
}

function progressPercent(job: QueuedTransfer): number {
  if (job.status === 'Completed') return 100;
  if (job.total_bytes === 0) return 0;
  return Math.min(100, Math.round((job.transferred_bytes / job.total_bytes) * 100));
}

function canCancel(job: QueuedTransfer): boolean {
  return job.status === 'Pending' || job.status === 'InProgress' || job.status === 'Paused';
}

function canPause(job: QueuedTransfer): boolean {
  return job.status === 'Pending' || job.status === 'InProgress';
}

function directionTitle(job: QueuedTransfer): string {
  if (job.direction === 'Sync') return 'Synchronisation';
  const way = job.direction === 'Upload' ? 'vers le serveur' : 'depuis le serveur';
  return job.delete_source ? `Déplacement ${way} : la source est supprimée une fois tout copié` : `Copie ${way}`;
}
</script>

<template>
  <div class="transfer-queue">
    <div class="queue-toolbar">
      <span class="queue-count">{{ transfers.active.length }} en cours</span>
      <span v-if="transfers.queuePaused" class="queue-paused" role="status">
        File en pause : aucun transfert ne démarre ni n'avance
      </span>
      <div class="queue-spacer" />
      <button
        class="queue-btn"
        :class="{ 'queue-btn-on': transfers.queuePaused }"
        :aria-pressed="transfers.queuePaused"
        :title="transfers.queuePaused ? 'Reprendre tous les transferts' : 'Suspendre tous les transferts, y compris ceux en cours'"
        @click="transfers.setQueuePaused(!transfers.queuePaused)"
      >
        {{ transfers.queuePaused ? '▶ Tout reprendre' : '⏸ Tout suspendre' }}
      </button>
      <button
        class="queue-btn"
        :disabled="!transfers.jobs.some(transfers.canRetry)"
        @click="transfers.retryAll"
      >
        Réessayer les échecs
      </button>
      <button
        class="queue-btn"
        :disabled="transfers.jobs.length === transfers.active.length"
        @click="transfers.clearFinished"
      >
        Effacer les terminés
      </button>
    </div>
    <table v-if="transfers.jobs.length > 0" class="queue-table">
      <thead>
        <tr>
          <th>Direction</th>
          <th>Source</th>
          <th>Destination</th>
          <th>Progression</th>
          <th>Statut</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="job in transfers.jobs" :key="job.id" class="queue-row">
          <td>
            <span class="direction-icon" :title="directionTitle(job)">{{ directionIcon[job.direction] }}</span>
            <span v-if="job.delete_source" class="move-mark" :title="directionTitle(job)">déplacer</span>
          </td>
          <td class="path-cell" :title="job.source">{{ job.source }}</td>
          <td class="path-cell" :title="job.destination">{{ job.destination }}</td>
          <td>
            <div class="progress-bar">
              <div
                class="progress-fill"
                :style="{ width: progressPercent(job) + '%' }"
              />
              <span class="progress-text">{{ progressPercent(job) }}%</span>
            </div>
          </td>
          <td>
            <span
              :class="['status-badge', 'status-' + job.status.toLowerCase()]"
              :title="job.error"
            >
              {{ statusLabel(job.status) }}
            </span>
          </td>
          <td class="row-actions">
            <button
              v-if="canPause(job)"
              class="queue-btn"
              title="Mettre en pause : un transfert en cours s'arrête entre deux blocs, sa connexion reste ouverte"
              aria-label="Mettre en pause"
              @click="transfers.pause(job.id)"
            >
              ⏸
            </button>
            <button
              v-else-if="job.status === 'Paused'"
              class="queue-btn"
              title="Reprendre"
              aria-label="Reprendre"
              @click="transfers.resume(job.id)"
            >
              ▶
            </button>
            <button v-if="canCancel(job)" class="queue-btn" title="Annuler" aria-label="Annuler" @click="transfers.cancel(job.id)">
              ✕
            </button>
            <button
              v-else-if="transfers.canRetry(job)"
              class="queue-btn"
              title="Réessayer : seul ce qui manque est copié, sauf si vous aviez choisi d'écraser"
              @click="transfers.retry(job)"
            >
              ↻
            </button>
          </td>
        </tr>
      </tbody>
    </table>
    <div v-else class="queue-empty">
      Aucun transfert — sélectionnez des fichiers puis Upload/Download (F5), ou glissez-les vers l'autre panneau
    </div>
  </div>
</template>

<style scoped>
.transfer-queue {
  height: 100%;
  overflow-y: auto;
}

.queue-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}

.queue-table thead {
  position: sticky;
  top: 0;
  background: var(--color-surface);
}

.queue-table th {
  padding: 4px 8px;
  text-align: left;
  font-weight: 600;
  color: var(--color-text-muted);
  border-bottom: 1px solid var(--color-border);
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.3px;
}

.queue-row td {
  padding: 4px 8px;
  color: var(--color-text);
  white-space: nowrap;
}

.path-cell {
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  font-family: var(--font-mono);
  font-size: 11px;
}

.direction-icon {
  font-weight: bold;
  font-size: 14px;
}

.progress-bar {
  position: relative;
  width: 100px;
  height: 14px;
  background: var(--color-bg);
  border-radius: 2px;
  overflow: hidden;
}

.progress-fill {
  height: 100%;
  background: var(--color-accent);
  transition: width 0.3s;
}

.progress-text {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 10px;
  color: var(--color-text);
}

.status-badge {
  padding: 1px 6px;
  border-radius: 3px;
  font-size: 10px;
  font-weight: 600;
  text-transform: uppercase;
}

.status-completed {
  background: var(--bg-success);
  color: var(--color-success);
}

.status-completedwithwarnings {
  background: var(--bg-warning);
  color: var(--color-warning);
}

.status-inprogress {
  background: var(--bg-info);
  color: var(--color-info);
}

.status-paused {
  background: var(--bg-neutral);
  color: var(--color-text);
}

.row-actions {
  display: flex;
  gap: 4px;
}

.move-mark {
  margin-left: 4px;
  padding: 0 4px;
  border-radius: 3px;
  background: var(--bg-info);
  color: var(--color-info);
  font-size: 10px;
}

.queue-paused {
  padding: 0 6px;
  border-radius: 3px;
  background: var(--bg-warning);
  color: var(--color-warning);
}

.queue-btn-on {
  border-color: var(--color-warning);
  color: var(--color-warning);
}

.status-pending {
  background: var(--bg-warning);
  color: var(--color-warning);
}

.status-failed {
  background: var(--bg-danger);
  color: var(--color-danger);
}

.status-skipped {
  background: var(--bg-neutral);
  color: var(--color-text);
}

.queue-spacer {
  flex: 1;
}

.queue-toolbar {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 2px 8px;
  border-bottom: 1px solid var(--color-border);
  font-size: 11px;
  color: var(--color-text-muted);
}

.queue-btn {
  padding: 1px 8px;
  border: 1px solid var(--color-border);
  border-radius: 3px;
  background: none;
  color: var(--color-text);
  font-size: 11px;
  cursor: pointer;
}

.queue-btn:hover:not(:disabled) {
  background: var(--color-hover);
}

.queue-btn:disabled {
  opacity: 0.4;
  cursor: default;
}

.status-cancelled {
  background: var(--bg-neutral);
  color: var(--color-neutral);
}

.queue-empty {
  padding: 16px;
  text-align: center;
  color: var(--color-text-muted);
  font-size: 12px;
}
</style>
