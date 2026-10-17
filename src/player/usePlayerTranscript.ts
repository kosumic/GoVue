import { useCallback, useEffect, useRef, useState } from 'react';
import { extractTranscript, restoreTranscript, selectTranscriptTracks } from '../core/transcripts';
import type { Transcript } from '../youtube/transcript';

type Request = { videoId: string; trackIds: string[]; fresh: boolean };
type Document = { data: Transcript; cached: boolean };
type Result = { request: Request; documents: Document[]; loading: boolean; error: string | null };

export function usePlayerTranscript(videoId: string | null, initial?: Transcript, audioKey = '') {
  const [request, setRequest] = useState<Request | null>(() => initial ? { videoId: initial.videoId, trackIds: [initial.trackId], fresh: false } : null);
  const [result, setResult] = useState<Result | null>(null);
  const documents = useRef<Document[]>([]);
  const activeRequest = useRef(request);
  activeRequest.current = request;
  const updateRequest = useCallback((next: Request | null) => {
    activeRequest.current = next;
    setRequest(next);
  }, []);
  const open = !!videoId && request?.videoId === videoId;
  const current = open && result?.request === request ? result : null;
  const available = open ? (current?.documents ?? documents.current).filter(d => d.data.videoId === videoId) : [];
  const selected = request?.trackIds ?? [];
  const data = available.find(d => selected.includes('default') || selected[0] === d.data.trackId)?.data ?? null;
  const comparison = available.find(d => selected[1] === d.data.trackId)?.data ?? null;
  const close = useCallback(() => updateRequest(null), [updateRequest]);
  useEffect(() => {
    if (!request) { documents.current = []; return; }
    if (request.videoId !== videoId) { close(); return; }
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
  }, [request, videoId, initial, audioKey, close]);
  return {
    open, data, comparison,
    trackIds: selected[0] === 'default' && data ? [data.trackId] : selected,
    loading: open && !data && (!current || current.loading),
    comparisonLoading: !!data && !!selected[1] && !comparison && (!current || current.loading),
    cached: !!data && available.every(d => d.cached),
    error: !data ? current?.error ?? null : null,
    comparisonError: data ? current?.error ?? null : null,
    close,
    toggle: () => updateRequest(activeRequest.current?.videoId === videoId || !videoId
      ? null : { videoId, trackIds: ['default'], fresh: false }),
    extract: () => {
      if (videoId) updateRequest({ videoId, trackIds: data ? [data.trackId, ...selected.slice(1)] : selected.length ? selected : ['default'], fresh: true });
    },
    selectTrack: (trackId: string) => {
      if (!data || !request) return;
      const expected = request;
      void selectTranscriptTracks(data, [data.trackId, ...selected.slice(1)], trackId).then(trackIds => {
        if (activeRequest.current !== expected) return;
        updateRequest({ videoId: data.videoId, trackIds, fresh: false });
      }).catch(error => {
        if (activeRequest.current !== expected) return;
        setResult(previous => previous ? { ...previous, error: error instanceof Error ? error.message : 'Could not select this language.' } : previous);
      });
    },
  };
}
