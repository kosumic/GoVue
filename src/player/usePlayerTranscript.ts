import { useCallback, useEffect, useRef, useState } from 'react';
import { selectTranscriptTracks } from '../core/transcripts';
import { coreRequest, observeCoreUpdates } from '../../modules/govue-core';
import type { Transcript } from '../youtube/transcript';

type Request = { videoId: string; audioKey: string; trackIds: string[]; fresh: boolean };
type Document = { data: Transcript; cached: boolean };
type Result = { request: Request; trackIds: string[]; documents: Document[]; loading: boolean; error: string | null };

export function usePlayerTranscript(videoId: string | null, initial?: Transcript, audioKey = '') {
  const [sessionId] = useState(() => `reader:${Date.now()}:${Math.random().toString(36).slice(2)}`);
  const revision = useRef(0);
  const [request, setRequest] = useState<Request | null>(() => videoId ? {
    videoId, audioKey, trackIds: ['default'], fresh: false,
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
  const selected = matches ? current?.trackIds ?? request.trackIds : [];
  const data = available.find(d => selected.includes('default') || selected[0] === d.data.trackId)?.data ?? null;
  const comparison = available.find(d => selected[1] === d.data.trackId)?.data ?? null;
  const close = useCallback(() => {
    ++selectionVersion.current;
    setVisibleVideoId(null);
  }, []);
  useEffect(() => () => {
    void coreRequest({ op: 'transcript_session_dispose', sessionId, version: revision.current }).catch(() => {});
  }, [sessionId]);
  useEffect(() => {
    // Loading belongs to the video session; showing the reader only changes presentation.
    if (!matches) {
      documents.current = [];
      if (request?.videoId !== videoId) close();
      updateRequest(videoId ? { videoId, audioKey, trackIds: ['default'], fresh: false } : null);
      return;
    }
    if (!request) return;
    const version = ++revision.current;
    let active = true, snapshotRevision = 0;
    type Snapshot = { version: number; trackIds: string[]; documents: Document[]; loading: boolean; error: string | null };
    const apply = (value: Snapshot | null) => {
      if (!active || !value || value.version !== version) return;
      documents.current = value.documents;
      setResult({ request, trackIds: value.trackIds, documents: value.documents, loading: value.loading, error: value.error });
    };
    const refresh = async () => {
      const current = ++snapshotRevision;
      const value = await coreRequest<Snapshot>({ op: 'transcript_session_snapshot', sessionId });
      if (current === snapshotRevision) apply(value);
    };
    const subscription = observeCoreUpdates(`transcript:${sessionId}`, () => { void refresh().catch(() => {}); });
    void coreRequest<Snapshot>({ op: 'transcript_session_load', sessionId, version, ...request, initial }).then(refresh).catch(error => {
      if (active) setResult({ request, trackIds: request.trackIds, documents: documents.current, loading: false,
        error: error instanceof Error ? error.message : 'The transcript could not load.' });
    });
    return () => {
      active = false; subscription?.remove();
      void coreRequest({ op: 'transcript_session_release', sessionId, version }).catch(() => {});
    };
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
