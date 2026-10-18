import { useCallback, useEffect, useRef, useState } from 'react';
import { extractTranscript, restoreTranscript, selectTranscriptTracks } from '../core/transcripts';
import type { Transcript } from '../youtube/transcript';

type Request = { videoId: string; audioKey: string; trackIds: string[]; fresh: boolean };
type Document = { data: Transcript; cached: boolean };
type Result = { request: Request; documents: Document[]; loading: boolean; error: string | null };

export function usePlayerTranscript(videoId: string | null, initial?: Transcript, audioKey = '') {
  const [request, setRequest] = useState<Request | null>(() => videoId ? {
    videoId, audioKey, trackIds: [initial?.videoId === videoId ? initial.trackId : 'default'], fresh: false,
  } : null);
  const [visibleVideoId, setVisibleVideoId] = useState<string | null>(initial?.videoId ?? null);
  const [result, setResult] = useState<Result | null>(null);
  const documents = useRef<Document[]>([]);
  const selectionVersion = useRef(0);
  const matches = !!videoId && request?.videoId === videoId && request.audioKey === audioKey;
  const activeRequest = useRef(request);
  activeRequest.current = matches ? request : null;
  const updateRequest = useCallback((next: Request | null) => {
    activeRequest.current = next;
    setRequest(next);
  }, []);
  const open = matches && visibleVideoId === videoId;
  const current = matches && result?.request === request ? result : null;
  const available = matches ? (current?.documents ?? documents.current).filter(d => d.data.videoId === videoId) : [];
  const selected = matches ? request.trackIds : [];
  const data = available.find(d => selected.includes('default') || selected[0] === d.data.trackId)?.data ?? null;
  const comparison = available.find(d => selected[1] === d.data.trackId)?.data ?? null;
  const close = useCallback(() => {
    ++selectionVersion.current;
    setVisibleVideoId(null);
  }, []);
  useEffect(() => {
    // Loading belongs to the video session; showing the reader only changes presentation.
    if (!matches) {
      documents.current = [];
      if (request?.videoId !== videoId) close();
      updateRequest(videoId ? { videoId, audioKey, trackIds: ['default'], fresh: false } : null);
      return;
    }
    if (!request) return;
    const controller = new AbortController();
    const retained = request.fresh ? [] : documents.current.filter(d => d.data.videoId === videoId && request.trackIds.includes(d.data.trackId));
    documents.current = retained;
    setResult({ request, documents: retained, loading: true, error: null });
    void (async () => {
      // Primary first: adding a comparison never hides a usable transcript.
      const loaded: Document[] = [];
      for (const trackId of request.trackIds) {
        const existing = retained.find(d => d.data.trackId === trackId);
        const seed = !request.fresh && initial?.videoId === videoId && initial.trackId === trackId ? initial : null;
        const saved = existing?.data ?? seed ?? (request.fresh ? null : await restoreTranscript(request.videoId, trackId === 'default' ? undefined : trackId, audioKey).catch(() => null));
        if (controller.signal.aborted) return;
        const cached = existing?.cached ?? !!saved;
        const data = saved ?? await extractTranscript(request.videoId, trackId, controller.signal, request.fresh, audioKey);
        if (controller.signal.aborted) return;
        loaded.push({ data, cached });
        documents.current = [...loaded];
        setResult({ request, documents: [...loaded], loading: loaded.length < request.trackIds.length, error: null });
      }
    })().catch(error => {
      if (!controller.signal.aborted) setResult({ request, documents: documents.current, loading: false,
        error: error instanceof Error ? error.message : 'The transcript could not load.' });
    });
    return () => controller.abort();
  }, [request, videoId, initial, audioKey, matches, close, updateRequest]);
  return {
    open, data, comparison,
    trackIds: selected[0] === 'default' && data ? [data.trackId] : selected,
    loading: !!videoId && !data && (!current || current.loading),
    comparisonLoading: !!data && !!selected[1] && !comparison && (!current || current.loading),
    cached: !!data && available.every(d => d.cached),
    error: !data ? current?.error ?? null : null,
    comparisonError: data ? current?.error ?? null : null,
    close,
    toggle: () => {
      if (open || !videoId) { close(); return; }
      setVisibleVideoId(videoId);
      if (current?.error && !data)
        updateRequest({ videoId, audioKey, trackIds: selected.length ? selected : ['default'], fresh: false });
    },
    extract: () => {
      // Retrying a comparison reuses the primary and its guest session. An
      // explicit refresh after a successful load still bypasses saved data.
      if (videoId) updateRequest({ videoId, audioKey, trackIds: data ? [data.trackId, ...selected.slice(1)] : selected.length ? selected : ['default'], fresh: !current?.error });
    },
    selectTrack: (trackId: string) => {
      if (!data || !request) return;
      const expected = request;
      const version = selectionVersion.current;
      void selectTranscriptTracks(data, [data.trackId, ...selected.slice(1)], trackId).then(trackIds => {
        if (activeRequest.current !== expected || selectionVersion.current !== version) return;
        updateRequest({ videoId: data.videoId, audioKey, trackIds, fresh: false });
      }).catch(error => {
        if (activeRequest.current !== expected || selectionVersion.current !== version) return;
        setResult(previous => previous ? { ...previous, error: error instanceof Error ? error.message : 'Could not select this language.' } : previous);
      });
    },
  };
}
