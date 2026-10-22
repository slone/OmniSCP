import { ref, computed } from 'vue';
import { defineStore } from 'pinia';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { ConflictStrategy, TransferDirection, TransferEvent, TransferJob } from '../types';
import { useLogStore } from './logs';
import { useLocalFilesStore } from './localFiles';
import { useConnectionStore } from './connection';
import { errorMessage } from '../utils/errors';

export interface QueuedTransfer extends TransferJob {
  /// Directory the item is copied into, used to refresh the matching pane.
  dest_dir: string;
  error?: string;
}

const FINISHED = new Set(['Completed', 'Skipped', 'Cancelled', 'Failed']);

export const useTransfersStore = defineStore('transfers', () => {
  const jobs = ref<QueuedTransfer[]>([]);
  let listening = false;
  // Events can arrive before enqueue_transfer resolves (fast jobs); keep the latest per id.
  const early = new Map<string, TransferEvent>();

  const active = computed(() => jobs.value.filter((j) => !FINISHED.has(j.status)));

  async function ensureListener() {
    if (listening) return;
    listening = true;
    await listen<TransferEvent>('transfer-progress', (event) => onProgress(event.payload));
  }

  function onProgress(event: TransferEvent) {
    const job = jobs.value.find((j) => j.id === event.id);
    if (!job) {
      early.set(event.id, event);
      return;
    }
    // Already closed locally (e.g. marked cancelled on disconnect).
    if (FINISHED.has(job.status)) return;

    job.status = event.status;
    job.total_bytes = event.total_bytes;
    job.transferred_bytes = event.transferred_bytes;
    if (event.destination) job.destination = event.destination;
    if (event.error) job.error = event.error;

    if (FINISHED.has(event.status)) {
      onFinished(job);
    }
  }

  function onFinished(job: QueuedTransfer) {
    const logStore = useLogStore();
    const action = job.direction === 'Upload' ? 'Upload' : 'Download';
    switch (job.status) {
      case 'Completed':
        logStore.addLog('Info', action, 'OK', `${job.source} → ${job.destination}`);
        break;
      case 'Skipped':
        logStore.addLog('Info', action, 'Ignoré', `${job.source} — la destination existe déjà`);
        break;
      case 'Cancelled':
        logStore.addLog('Info', action, 'Annulé', job.source);
        break;
      case 'Failed':
        logStore.addLog('Error', action, 'Échec', `${job.source} — ${job.error ?? ''}`);
        break;
    }

    // Show the new files if the destination directory is on screen.
    const pane = job.direction === 'Upload' ? useConnectionStore() : useLocalFilesStore();
    if (job.status === 'Completed' && pane.currentPath === job.dest_dir) {
      pane.refresh();
    }
  }

  async function enqueue(
    direction: TransferDirection,
    source: string,
    destDir: string,
    conflict: ConflictStrategy,
  ) {
    await ensureListener();
    try {
      const id = await invoke<string>('enqueue_transfer', {
        direction,
        source,
        destDir,
        conflict,
      });
      jobs.value.push({
        id,
        source,
        destination: destDir,
        dest_dir: destDir,
        direction,
        status: 'Pending',
        total_bytes: 0,
        transferred_bytes: 0,
        conflict_strategy: conflict,
      });
      const buffered = early.get(id);
      if (buffered) {
        early.delete(id);
        onProgress(buffered);
      }
    } catch (err) {
      useLogStore().addLog('Error', direction, 'Échec', `${source} — ${errorMessage(err)}`);
    }
  }

  async function cancel(id: string) {
    await invoke('cancel_transfer', { id });
  }

  function clearFinished() {
    jobs.value = jobs.value.filter((j) => !FINISHED.has(j.status));
  }

  /// The backend drops its queue on disconnect; mirror that here.
  function cancelAllLocally() {
    for (const job of jobs.value) {
      if (!FINISHED.has(job.status)) job.status = 'Cancelled';
    }
  }

  return { jobs, active, enqueue, cancel, clearFinished, cancelAllLocally };
});
