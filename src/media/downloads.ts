import { useEffect, useRef, useState } from 'react';
import * as FileSystem from 'expo-file-system/legacy';
import * as Sharing from 'expo-sharing';
import { coreRequest, muxMedia } from '../../modules/lingoplay-core';
import { planDownload, type MediaCatalog } from '../core/media';

export type DownloadEntry = {
  id: string; videoId: string; title: string; author: string; label: string; itag: number;
  state: 'downloading' | 'paused' | 'combining' | 'complete' | 'failed';
  received: number; total: number; file: string; mimeType: string; error?: string;
};
const directory = `${FileSystem.documentDirectory}downloads/`;
type Running = { id: string; task?: FileSystem.DownloadResumable; cancelled: boolean; resume?: () => void; paused: boolean };
const fileUri = (name: string) => {
  if (!/^[\w.-]+\.(?:mp4|m4a)$/.test(name)) throw Error('Invalid saved file.');
  return directory + name;
};

export function useDownloads() {
  const [entries, setEntries] = useState<DownloadEntry[]>([]);
  const [ready, setReady] = useState(false);
  const records = useRef<DownloadEntry[]>([]);
  const running = useRef<Running | null>(null);
  const saves = useRef(Promise.resolve());
  const update = (id: string, changes: Partial<DownloadEntry>, persist = true) => {
    const next = records.current.map(e => e.id === id ? { ...e, ...changes } : e);
    records.current = next; setEntries(next);
    if (persist) saves.current = saves.current.catch(() => {}).then(() => coreRequest<void>({ op: 'downloads_save', entries: next }));
  };
  useEffect(() => {
    let active = true;
    FileSystem.makeDirectoryAsync(directory, { intermediates: true }).then(() => coreRequest<DownloadEntry[]>({ op: 'downloads_list' })).then(async saved => {
      const restored = await Promise.all(saved.map(async e => {
        if (e.state === 'failed') return e;
        if (e.state === 'complete') {
          try { if ((await FileSystem.getInfoAsync(fileUri(e.file))).exists) return e; } catch {}
          return { ...e, state: 'failed' as const, error: 'Saved file is missing. Download again.' };
        }
        return { ...e, state: 'failed' as const, error: 'Download interrupted. Open the video and download again.' };
      }));
      if (active) { records.current = restored; setEntries(restored); setReady(true); }
    }).catch(() => { if (active) setReady(true); });
    return () => { active = false; };
  }, []);

  async function start(catalog: MediaCatalog, itag: number, userAgent: string) {
    if (!ready) throw Error('Downloads are still loading.');
    if (running.current) throw Error('Finish or cancel the current download first.');
    if (records.current.length >= 200) throw Error('Remove an old download before adding another.');
    const plan = await planDownload(catalog, itag);
    // The plan contains short-lived signed URLs. Never write it to history.
    if (running.current) throw Error('A download is already running.');
    const id = `${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
    const file = `${catalog.videoId}-${id}.${plan.extension}`;
    const entry: DownloadEntry = { id, videoId: catalog.videoId, title: catalog.title, author: catalog.author, label: plan.label, itag,
      state: 'downloading', received: 0, total: plan.parts.reduce((n, p) => n + p.bytes, 0), file, mimeType: plan.mimeType };
    records.current = [entry, ...records.current]; setEntries(records.current);
    update(id, {});
    const operation: Running = { id, cancelled: false, paused: false };
    running.current = operation;
    const temporary: string[] = [];
    const partFiles: string[] = [];
    try {
      let completedBytes = 0;
      for (let i = 0; i < plan.parts.length; i++) {
        if (operation.cancelled) throw Error('Download cancelled.');
        const part = plan.parts[i];
        const path = directory + `${id}-part-${i}.tmp`;
        temporary.push(path); partFiles.push(path);
        if (!(part.bytes > 0)) throw Error('YouTube did not provide the complete media length. Refresh formats.');
        const uri = await coreRequest<string>({ op: 'media_proxy', url: part.url, length: part.bytes, userAgent: part.userAgent || userAgent, origin: catalog.origin });
        try {
          const task = FileSystem.createDownloadResumable(uri, path, { sessionType: FileSystem.FileSystemSessionType.BACKGROUND }, progress => {
            if (running.current === operation && !operation.cancelled) update(id, { received: completedBytes + progress.totalBytesWritten }, false);
          });
          operation.task = task;
          let result = await task.downloadAsync();
          while (!result && !operation.cancelled) {
            await new Promise<void>(resolve => { operation.resume = resolve; if (!operation.paused) resolve(); });
            if (operation.cancelled) break;
            result = await task.resumeAsync();
          }
          if (operation.cancelled) throw Error('Download cancelled.');
          if (!result || result.status < 200 || result.status >= 300) throw Error('Download failed. Refresh the video and try again.');
          const info = await FileSystem.getInfoAsync(path);
          if (!info.exists || info.size !== part.bytes) throw Error('The media file is incomplete. Refresh the video and try again.');
        } finally {
          await coreRequest({ op: 'media_proxy_release', uri }).catch(() => {});
        }
        completedBytes += part.bytes;
        update(id, { received: completedBytes });
      }
      operation.task = undefined;
      if (operation.cancelled) throw Error('Download cancelled.');
      const destination = fileUri(file);
      if (partFiles.length === 2) {
        update(id, { state: 'combining' });
        await muxMedia(partFiles[0], partFiles[1], destination);
      } else await FileSystem.moveAsync({ from: partFiles[0], to: destination });
      if (operation.cancelled) { await FileSystem.deleteAsync(destination, { idempotent: true }); throw Error('Download cancelled.'); }
      update(id, { state: 'complete', received: completedBytes, total: completedBytes, error: undefined });
    } catch (error) {
      update(id, { state: 'failed', error: error instanceof Error ? error.message : 'Download failed. Try again.' });
    } finally {
      await Promise.all(temporary.map(path => FileSystem.deleteAsync(path, { idempotent: true }).catch(() => {})));
      if (running.current === operation) running.current = null;
    }
  }

  async function pause(id: string) {
    const operation = running.current;
    if (operation?.id !== id || !operation.task) return;
    if (operation.paused) {
      operation.paused = false; update(id, { state: 'downloading' }); operation.resume?.();
    } else {
      operation.paused = true;
      try { await operation.task.pauseAsync(); update(id, { state: 'paused' }); }
      catch (error) { operation.paused = false; throw error; }
    }
  }
  async function cancel(id: string) {
    const operation = running.current;
    if (operation?.id !== id) return;
    operation.cancelled = true; operation.paused = false; operation.resume?.();
    try { await operation.task?.cancelAsync(); } catch {}
    update(id, { state: 'failed', error: 'Download cancelled.' });
  }
  async function remove(entry: DownloadEntry) {
    if (running.current?.id === entry.id) throw Error('Cancel this download before removing it.');
    await FileSystem.deleteAsync(fileUri(entry.file), { idempotent: true });
    await Promise.all([0, 1].flatMap(part => ['', '-chunk'].map(suffix => FileSystem.deleteAsync(`${directory}${entry.id}-part-${part}${suffix}.tmp`, { idempotent: true }))));
    const next = records.current.filter(e => e.id !== entry.id);
    records.current = next; setEntries(next);
    saves.current = saves.current.catch(() => {}).then(() => coreRequest<void>({ op: 'downloads_save', entries: next }));
    await saves.current;
  }
  async function share(entry: DownloadEntry) {
    if (!await Sharing.isAvailableAsync()) throw Error('File sharing is unavailable on this device.');
    await Sharing.shareAsync(fileUri(entry.file), { mimeType: entry.mimeType, dialogTitle: entry.title });
  }
  return { entries, ready, start, pause, cancel, remove, share, uri: (entry: DownloadEntry) => fileUri(entry.file) };
}
