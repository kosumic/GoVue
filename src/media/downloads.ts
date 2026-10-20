import { useEffect, useRef, useState } from 'react';
import { AppState, PermissionsAndroid, Platform } from 'react-native';
import * as FileSystem from 'expo-file-system/legacy';
import * as Sharing from 'expo-sharing';
import * as Network from 'expo-network';
import {
  coreRequest,
  muxMedia,
  setDownloadActivity,
  saveFileToDevice,
  publishDownloadToLibrary,
  deletePublishedDownload,
} from '../../modules/govue-core';
import {
  downloadPreferences,
  type DownloadArtifact,
  type DownloadConfig,
  type DownloadEntry,
  type DownloadPreferences,
  type DownloadSnapshot,
} from '../core/downloads';
import type { MediaResolverHandle } from './MediaResolver';
import type { QueueItem } from '../player/usePlayerSession';

export type { DownloadEntry } from '../core/downloads';
const directory = `${FileSystem.documentDirectory}downloads/`;
const fileUri = (file: string) => {
  if (!/^[\w-][\w.-]{0,254}$/.test(file) || file.includes('..'))
    throw Error('Invalid saved file.');
  return directory + file;
};
type Running = {
  id: string;
  cancelled: boolean;
  paused: boolean;
  transfer: boolean;
  controller: AbortController;
};
type TransferStatus = {
  done: boolean;
  networkWaiting: boolean;
  error: string | null;
  received: number;
  total: number;
  files: string[];
  coverFile: string | null;
};

export function useDownloads(
  resolver: React.RefObject<MediaResolverHandle | null>,
) {
  const [entries, setEntries] = useState<DownloadEntry[]>([]);
  const [ready, setReady] = useState(false);
  const [waitingForWifi, setWaitingForWifi] = useState(new Set<string>());
  const [preferences, setPreferences] = useState<DownloadPreferences | null>(
    null,
  );
  const records = useRef(entries);
  const running = useRef(new Map<string, Running>());
  const writes = useRef(Promise.resolve());
  const mounted = useRef(true);
  const claiming = useRef(false);
  const preferencesRef = useRef(preferences);
  preferencesRef.current = preferences;
  const network = Network.useNetworkState();
  useEffect(() => {
    const active = entries.filter(
      (e) =>
        ['resolving', 'downloading', 'combining'].includes(e.state) ||
        (e.state === 'queued' &&
          (!preferences?.wifiOnly ||
            network.type === Network.NetworkStateType.WIFI)),
    );
    void setDownloadActivity(
      active.length,
      active[0]?.title || '',
      active.reduce((n, e) => n + e.received, 0),
      active.reduce((n, e) => n + e.total, 0),
    ).catch(() => {});
  }, [entries, preferences?.wifiOnly, network.type]);
  async function action(op: string, values: Record<string, unknown> = {}) {
    let result: DownloadSnapshot = { entries: [], claimed: [], plan: null };
    const work = writes.current
      .catch(() => {})
      .then(async () => {
        result = await coreRequest<DownloadSnapshot>({
          op: `downloads_${op}`,
          ...values,
        });
        records.current = result.entries;
        if (mounted.current) setEntries(result.entries);
      });
    writes.current = work;
    await work;
    return result;
  }
  async function event(
    id: string,
    event: string,
    values: Record<string, unknown> = {},
  ) {
    return action('event', { id, event, ...values });
  }
  async function configure(changes: Partial<DownloadPreferences>) {
    const next = await downloadPreferences(changes);
    setPreferences(next);
    return next;
  }
  useEffect(() => {
    mounted.current = true;
    void (async () => {
      await FileSystem.makeDirectoryAsync(directory, { intermediates: true });
      const result = await action('restore');
      for (const entry of result.entries.filter(
        (e) => e.state === 'complete',
      )) {
        if (entry.artifacts?.some((a) => a.publishedUri)) {
          try {
            await publishDownloadToLibrary(entry.id);
          } catch {
            await event(entry.id, 'missing');
            continue;
          }
        }
        const files = entry.artifacts?.length
          ? entry.artifacts
          : [{ file: entry.file }];
        const info = await Promise.all(
          files.map((a) => FileSystem.getInfoAsync(fileUri(a.file))),
        );
        if (info.some((f) => !f.exists)) await event(entry.id, 'missing');
      }
      const prefs = await downloadPreferences();
      if (mounted.current) {
        setPreferences(prefs);
        setReady(true);
      }
    })().catch(() => {
      if (mounted.current) setReady(true);
    });
    return () => {
      mounted.current = false;
    };
  }, []);
  async function run(entry: DownloadEntry) {
    const operation: Running = {
      id: entry.id,
      cancelled: false,
      paused: false,
      transfer: false,
      controller: new AbortController(),
    };
    running.current.set(entry.id, operation);
    const id = entry.id;
    try {
      if (!resolver.current)
        throw Error('The media resolver is still starting.');
      const { catalog, srt } = await resolver.current.resolve(
        entry.videoId,
        entry.config,
        operation.controller.signal,
      );
      if (operation.cancelled || operation.paused) return;
      const result = await action('prepared', { id, catalog });
      const plan = result.plan;
      if (!plan) throw Error('Download options are unavailable.');
      if (operation.cancelled || operation.paused) return;
      const destination = fileUri(entry.file);
      await FileSystem.deleteAsync(destination, { idempotent: true });
      let status = await coreRequest<TransferStatus>({
        op: 'downloads_transfer_start',
        id,
        catalog,
        userAgent: '',
      });
      operation.transfer = true;
      if (operation.cancelled)
        await coreRequest({ op: 'downloads_transfer_cancel', id });
      else if (operation.paused)
        await coreRequest({ op: 'downloads_transfer_pause', id, paused: true });
      while (!status.done) {
        const networkWaiting = status.networkWaiting;
        setWaitingForWifi((previous) => {
          if (previous.has(id) === networkWaiting) return previous;
          const next = new Set(previous);
          if (networkWaiting) next.add(id);
          else next.delete(id);
          return next;
        });
        await new Promise<void>((resolve) => setTimeout(resolve, 500));
        if (operation.cancelled) throw Error('Download cancelled.');
        status = await coreRequest<TransferStatus>({
          op: 'downloads_transfer_poll',
          id,
        });
        if (
          !operation.paused &&
          status.received !== records.current.find((e) => e.id === id)?.received
        )
          await event(id, 'progress', { received: status.received });
      }
      if (status.error) throw Error(status.error);
      while (operation.paused && !operation.cancelled)
        await new Promise<void>((resolve) => setTimeout(resolve, 250));
      if (operation.cancelled) throw Error('Download cancelled.');
      if (!plan.attachmentsOnly) {
        if (status.files.length === 2) {
          await event(id, 'combining');
          await muxMedia(status.files[0], status.files[1], destination);
        } else if (status.files.length === 1)
          await FileSystem.copyAsync({
            from: status.files[0],
            to: destination,
          });
        else throw Error('The media download is incomplete.');
      }
      if (plan.subtitle) {
        if (!srt) throw Error('Subtitles could not be loaded.');
        await FileSystem.writeAsStringAsync(fileUri(`${id}.srt`), srt);
      }
      if (plan.coverUrl) {
        if (!status.coverFile)
          throw Error('The cover image could not be downloaded.');
        await FileSystem.copyAsync({
          from: status.coverFile,
          to: fileUri(`${id}.jpg`),
        });
      }
      if (operation.cancelled) throw Error('Download cancelled.');
      if (
        Platform.OS === 'android' &&
        records.current.find((e) => e.id === id)?.state === 'downloading'
      )
        await event(id, 'combining');
      const published = await publishDownloadToLibrary(id);
      if (operation.cancelled) throw Error('Download cancelled.');
      await event(id, 'complete', { publicationRequired: published });
    } catch (error) {
      if (!operation.cancelled && !operation.paused)
        await event(id, 'failed', {
          error:
            error instanceof Error
              ? error.message
              : 'Download failed. Try again.',
        }).catch(() => {});
    } finally {
      if (mounted.current)
        setWaitingForWifi((previous) => {
          if (!previous.has(id)) return previous;
          const next = new Set(previous);
          next.delete(id);
          return next;
        });
      if (operation.transfer)
        await coreRequest({ op: 'downloads_transfer_release', id }).catch(
          () => {},
        );
      running.current.delete(id);
      if (operation.cancelled) {
        const record = records.current.find((e) => e.id === id);
        if (record) await deleteFiles(record).catch(() => {});
      }
    }
  }
  async function claim() {
    if (
      !ready ||
      !resolver.current ||
      claiming.current ||
      !records.current.some((e) => e.state === 'queued')
    )
      return;
    claiming.current = true;
    try {
      const network = await Network.getNetworkStateAsync();
      const result = await action('claim', {
        wifi: network.type === Network.NetworkStateType.WIFI,
      });
      for (const id of result.claimed) {
        const entry = result.entries.find((e) => e.id === id);
        if (entry && !running.current.has(id)) void run(entry);
      }
    } finally {
      claiming.current = false;
    }
  }
  useEffect(() => {
    if (!ready) return;
    void claim().catch(() => {});
    const timer = setInterval(() => {
      void claim().catch(() => {});
    }, 1500);
    const sub = AppState.addEventListener('change', (state) => {
      if (state === 'active') void claim().catch(() => {});
    });
    return () => {
      clearInterval(timer);
      sub.remove();
    };
  }, [ready]);
  async function enqueue(
    items: QueueItem[],
    config: DownloadConfig,
    name = '',
    replace = false,
  ) {
    if (Platform.OS === 'android' && Number(Platform.Version) >= 33)
      await PermissionsAndroid.request(
        PermissionsAndroid.PERMISSIONS.POST_NOTIFICATIONS,
      ).catch(() => {});
    await configure({ lastConfig: config });
    return action('enqueue', { items, config, name, replace });
  }
  async function pause(id: string, desired?: boolean) {
    const entry = records.current.find((e) => e.id === id);
    if (!entry) return;
    if (desired !== undefined && (entry.state === 'paused') === desired) return;
    if (!['queued', 'resolving', 'downloading', 'paused'].includes(entry.state))
      return;
    const operation = running.current.get(id);
    if (entry.state === 'paused') {
      if (
        preferencesRef.current?.wifiOnly &&
        (await Network.getNetworkStateAsync()).type !==
          Network.NetworkStateType.WIFI
      )
        throw Error('This download is waiting for Wi-Fi.');
      if (operation?.transfer) {
        await event(id, 'resume', { retained: true });
        operation.paused = false;
        await coreRequest({
          op: 'downloads_transfer_pause',
          id,
          paused: false,
        });
      } else {
        if (operation)
          throw Error(
            'The current request is finishing. Try Resume again in a moment.',
          );
        await event(id, 'resume');
      }
    } else {
      await event(id, 'pause');
      if (operation) {
        operation.paused = true;
        if (!operation.transfer) operation.controller.abort();
        if (operation.transfer)
          await coreRequest({
            op: 'downloads_transfer_pause',
            id,
            paused: true,
          });
      }
    }
  }
  async function cancel(id: string) {
    await event(id, 'cancel');
    const operation = running.current.get(id);
    if (operation) {
      operation.cancelled = true;
      operation.paused = false;
      operation.controller.abort();
      if (operation.transfer)
        await coreRequest({ op: 'downloads_transfer_cancel', id }).catch(
          () => {},
        );
    }
  }
  async function deleteFiles(entry: DownloadEntry) {
    for (const uri of new Set(
      (entry.artifacts || [])
        .map((a) => a.publishedUri)
        .filter((uri): uri is string => !!uri),
    ))
      await deletePublishedDownload(uri);
    await Promise.all(
      [entry.file, ...(entry.artifacts || []).map((a) => a.file)].map((file) =>
        FileSystem.deleteAsync(fileUri(file), { idempotent: true }).catch(
          () => {},
        ),
      ),
    );
  }
  async function remove(entry: DownloadEntry, deleteMedia = true) {
    if (running.current.has(entry.id))
      throw Error(
        'Cancel this download and wait for it to stop before removing it.',
      );
    if (deleteMedia) await deleteFiles(entry);
    await action('remove', { id: entry.id });
  }
  async function retry(id: string) {
    await event(id, 'retry');
  }
  async function removeBatch(ids: string[], deleteMedia: boolean) {
    for (const id of ids) {
      const entry = records.current.find((e) => e.id === id);
      if (
        entry &&
        ['queued', 'resolving', 'downloading', 'paused', 'combining'].includes(
          entry.state,
        )
      )
        await cancel(id);
    }
    const deadline = Date.now() + 60000;
    while (ids.some((id) => running.current.has(id))) {
      if (Date.now() > deadline)
        throw Error(
          'The cancelled requests are finishing. Try removing the batch again in a moment.',
        );
      await new Promise<void>((resolve) => setTimeout(resolve, 100));
    }
    for (const id of ids) {
      const entry = records.current.find((e) => e.id === id);
      if (entry) await remove(entry, deleteMedia);
    }
  }
  async function share(
    entry: DownloadEntry,
    artifact?: DownloadArtifact,
    save = false,
  ) {
    const file = artifact?.file || entry.file;
    const name = await coreRequest<string>({
      op: 'downloads_export_name',
      id: entry.id,
      file,
    });
    const mime = artifact?.mimeType || entry.mimeType;
    if (save) {
      const saved = await saveFileToDevice(fileUri(file), name, mime);
      if (saved !== null) return saved;
    }
    if (!(await Sharing.isAvailableAsync()))
      throw Error('File sharing is unavailable on this device.');
    const exportDirectory = `${FileSystem.cacheDirectory}download-exports/`;
    await FileSystem.makeDirectoryAsync(exportDirectory, {
      intermediates: true,
    });
    const uri = exportDirectory + encodeURIComponent(name);
    await FileSystem.copyAsync({ from: fileUri(file), to: uri });
    await Sharing.shareAsync(uri, {
      mimeType: artifact?.mimeType || entry.mimeType,
      dialogTitle: entry.name || entry.title,
    });
  }
  return {
    entries,
    waitingForWifi,
    ready,
    preferences,
    configure,
    enqueue,
    pause,
    cancel,
    remove,
    retry,
    removeBatch,
    clearHistory: async (deleteMedia: boolean) => {
      const terminal = records.current.filter((e) =>
        ['complete', 'failed', 'cancelled', 'skipped'].includes(e.state),
      );
      if (deleteMedia) for (const entry of terminal) await deleteFiles(entry);
      await action('clear_history', { ids: terminal.map((e) => e.id) });
    },
    share,
    save: (entry: DownloadEntry, artifact?: DownloadArtifact) =>
      share(entry, artifact, true),
    uri: (entry: DownloadEntry) =>
      entry.artifacts?.find((a) => a.file === entry.file && a.published)
        ?.publishedUri || fileUri(entry.file),
  };
}
