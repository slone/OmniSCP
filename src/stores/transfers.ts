import { ref, computed } from 'vue';
import { defineStore } from 'pinia';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { ConflictStrategy, SyncAction, TransferDirection, TransferEvent, TransferJob } from '../types';
import { useLogStore } from './logs';
import { useLocalFilesStore } from './localFiles';
import { useConnectionStore } from './connection';
import { errorMessage } from '../utils/errors';

export interface QueuedTransfer extends Omit<TransferJob, 'direction'> {
  direction: TransferDirection | 'Sync';
  /// Directory the item is copied into, used to refresh the matching pane.
  dest_dir: string;
  /// A retry must go to the same server.
  profile_id: string | null;
  /// Move: the source is removed once everything is copied.
  delete_source: boolean;
  error?: string;
}

const FINISHED = new Set(['Completed', 'CompletedWithWarnings', 'Skipped', 'Cancelled', 'Failed']);
const RETRYABLE = new Set(['CompletedWithWarnings', 'Cancelled', 'Failed']);

export const useTransfersStore = defineStore('transfers', () => {
  const jobs = ref<QueuedTransfer[]>([]);
  /// The whole queue is held; reset by a new connection, which starts a new queue.
  const queuePaused = ref(false);
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
    const action =
      job.direction === 'Sync' ? 'Synchronisation' : job.delete_source ? `Déplacement (${job.direction})` : job.direction;
    switch (job.status) {
      case 'Completed':
        logStore.addLog('Info', action, 'OK', `${job.source} → ${job.destination}`);
        break;
      case 'CompletedWithWarnings':
        logStore.addLog('Warning', action, 'Partiel', `${job.source} → ${job.destination} — ${job.error ?? ''}`);
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

    if (job.direction === 'Sync') {
      // Even a stopped sync may have changed both sides.
      useLocalFilesStore().refresh();
      useConnectionStore().refresh();
      return;
    }

    // Show the new files if the destination directory is on screen.
    const pane = job.direction === 'Upload' ? useConnectionStore() : useLocalFilesStore();
    const wroteFiles = job.status === 'Completed' || job.status === 'CompletedWithWarnings';
    if (wroteFiles && pane.currentPath === job.dest_dir) {
      pane.refresh();
    }
    // A move removes its source: show that on the other pane.
    if (job.delete_source && wroteFiles) {
      const source = job.direction === 'Upload' ? useLocalFilesStore() : useConnectionStore();
      if (source.files.some((f) => f.path === job.source)) source.refresh();
    }
  }

  async function enqueue(
    direction: TransferDirection,
    source: string,
    destDir: string,
    conflict: ConflictStrategy,
    destName?: string,
    deleteSource = false,
  ) {
    await ensureListener();
    try {
      const id = await invoke<string>('enqueue_transfer', {
        direction,
        source,
        destDir,
        conflict,
        destName,
        deleteSource,
      });
      track(id, direction, source, destDir, conflict, deleteSource);
      return true;
    } catch (err) {
      useLogStore().addLog('Error', direction, 'Échec', `${source} — ${errorMessage(err)}`);
      return false;
    }
  }

  async function enqueueSync(localRoot: string, remoteRoot: string, actions: SyncAction[]) {
    await ensureListener();
    try {
      const id = await invoke<string>('start_sync', { localRoot, remoteRoot, actions });
      track(id, 'Sync', localRoot, remoteRoot, 'Overwrite', false);
    } catch (err) {
      useLogStore().addLog('Error', 'Synchronisation', 'Échec', `${localRoot} — ${errorMessage(err)}`);
    }
  }

  function track(
    id: string,
    direction: QueuedTransfer['direction'],
    source: string,
    destDir: string,
    conflict: ConflictStrategy,
    deleteSource: boolean,
  ) {
    jobs.value.push({
      id,
      source,
      destination: destDir,
      dest_dir: destDir,
      direction,
      profile_id: useConnectionStore().profileId,
      status: 'Pending',
      total_bytes: 0,
      transferred_bytes: 0,
      conflict_strategy: conflict,
      delete_source: deleteSource,
    });
    const buffered = early.get(id);
    if (buffered) {
      early.delete(id);
      onProgress(buffered);
    }
  }

  /// A sync is not retried: it should go through a new comparison and preview.
  function canRetry(job: QueuedTransfer) {
    const connection = useConnectionStore();
    return (
      job.direction !== 'Sync' &&
      RETRYABLE.has(job.status) &&
      connection.connected &&
      job.profile_id === connection.profileId
    );
  }

  /// Picks up where the job stopped: an overwrite the user approved stays one;
  /// otherwise only what is missing is copied, to the name the job already chose.
  async function retry(job: QueuedTransfer) {
    if (!canRetry(job) || job.direction === 'Sync') return;
    const destName =
      job.destination !== job.dest_dir ? job.destination.split(/[\\/]/).pop() : undefined;
    const strategy =
      destName === undefined || job.conflict_strategy === 'Overwrite'
        ? job.conflict_strategy
        : 'Merge';
    if (await enqueue(job.direction, job.source, job.dest_dir, strategy, destName, job.delete_source)) {
      jobs.value = jobs.value.filter((j) => j.id !== job.id);
    }
  }

  async function retryAll() {
    for (const job of jobs.value.filter(canRetry)) {
      await retry(job);
    }
  }

  async function cancel(id: string) {
    await invoke('cancel_transfer', { id });
  }

  async function pause(id: string) {
    await invoke('pause_transfer', { id });
  }

  async function resume(id: string) {
    await invoke('resume_transfer', { id });
  }

  async function setQueuePaused(paused: boolean) {
    try {
      await invoke('set_queue_paused', { paused });
      queuePaused.value = paused;
      useLogStore().addLog('Info', "File d'attente", paused ? 'En pause' : 'Reprise', paused ? 'Transferts suspendus' : 'Transferts repris');
    } catch (err) {
      useLogStore().addLog('Error', "File d'attente", 'Échec', errorMessage(err));
    }
  }

  function clearFinished() {
    jobs.value = jobs.value.filter((j) => !FINISHED.has(j.status));
  }

  /// The backend drops its queue on disconnect; mirror that here.
  function cancelAllLocally() {
    queuePaused.value = false;
    for (const job of jobs.value) {
      if (!FINISHED.has(job.status)) job.status = 'Cancelled';
    }
  }

  return {
    jobs,
    active,
    queuePaused,
    pause,
    resume,
    setQueuePaused,
    enqueue,
    enqueueSync,
    canRetry,
    retry,
    retryAll,
    cancel,
    clearFinished,
    cancelAllLocally,
  };
});
