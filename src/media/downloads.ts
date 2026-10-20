import { useEffect, useRef, useState } from 'react';
import { AppState, PermissionsAndroid, Platform } from 'react-native';
import * as Sharing from 'expo-sharing';
import { coreRequest, observeCoreUpdates, saveFileToDevice } from '../../modules/govue-core';
import {
  downloadPreferences, type DownloadArtifact, type DownloadConfig,
  type DownloadEntry, type DownloadPreferences, type DownloadSnapshot,
} from '../core/downloads';
import type { MediaResolverHandle } from './MediaResolver';
import type { QueueItem } from '../player/usePlayerSession';

export type { DownloadEntry } from '../core/downloads';
type Snapshot = { entries: DownloadEntry[]; ready: boolean; waitingForWifi: string[]; preferences: DownloadPreferences | null };

// React owns presentation and OS permission/share dialogs. Rust owns every job.
export function useDownloads(_resolver: React.RefObject<MediaResolverHandle | null>) {
  const [state, setState] = useState<Snapshot>({ entries: [], ready: false, waitingForWifi: [], preferences: null });
  const mounted = useRef(false);
  const revision = useRef(0);
  async function refresh() {
    const version = ++revision.current;
    const value = await coreRequest<Snapshot>({ op: 'downloads_runner_snapshot' });
    if (mounted.current && version === revision.current) setState(value);
  }
  useEffect(() => {
    mounted.current = true;
    const subscription = observeCoreUpdates('downloads', () => { void refresh().catch(() => {}); });
    const foreground = AppState.addEventListener('change', state => { if (state === 'active') void refresh().catch(() => {}); });
    void coreRequest({ op: 'downloads_runner_start' }).then(refresh).catch(() => { if (mounted.current) setState(value => ({ ...value, ready: true })); });
    return () => { mounted.current = false; ++revision.current; subscription?.remove(); foreground.remove(); };
  }, []);
  async function action(op: string, values: Record<string, unknown> = {}) {
    const result = await coreRequest<DownloadSnapshot>({ op: `downloads_runner_${op}`, ...values });
    await refresh();
    return result;
  }
  async function configure(changes: Partial<DownloadPreferences>) {
    const next = await downloadPreferences(changes);
    await refresh();
    return next;
  }
  async function enqueue(items: QueueItem[], config: DownloadConfig, name = '', replace = false) {
    if (Platform.OS === 'android' && Number(Platform.Version) >= 33)
      await PermissionsAndroid.request(PermissionsAndroid.PERMISSIONS.POST_NOTIFICATIONS).catch(() => {});
    await configure({ lastConfig: config });
    return action('enqueue', { items, config, name, replace });
  }
  async function share(entry: DownloadEntry, artifact?: DownloadArtifact, save = false) {
    const mime = artifact?.mimeType || entry.mimeType;
    const result = await coreRequest<{ uri: string; name: string }>({ op: 'downloads_runner_export', id: entry.id, file: artifact?.file || entry.file });
    if (save && await saveFileToDevice(result.uri, result.name, mime) !== null) return;
    if (!(await Sharing.isAvailableAsync())) throw Error('File sharing is unavailable on this device.');
    await Sharing.shareAsync(result.uri, { mimeType: mime, dialogTitle: entry.name || entry.title });
  }
  return {
    entries: state.entries, ready: state.ready, preferences: state.preferences,
    waitingForWifi: new Set(state.waitingForWifi), configure, enqueue,
    pause: (id: string, paused?: boolean) => action('pause', { id, paused }),
    cancel: (id: string) => action('cancel', { id }),
    retry: (id: string) => action('retry', { id }),
    remove: (entry: DownloadEntry, deleteMedia = true) => action('remove', { ids: [entry.id], deleteMedia }),
    removeBatch: (ids: string[], deleteMedia: boolean) => action('remove', { ids, deleteMedia }),
    clearHistory: (deleteMedia: boolean) => action('remove', {
      ids: state.entries.filter(e => ['complete', 'failed', 'cancelled', 'skipped'].includes(e.state)).map(e => e.id), deleteMedia,
    }),
    share, save: (entry: DownloadEntry, artifact?: DownloadArtifact) => share(entry, artifact, true),
    uri: (entry: DownloadEntry) => coreRequest<string>({ op: 'downloads_runner_uri', id: entry.id }),
  };
}
