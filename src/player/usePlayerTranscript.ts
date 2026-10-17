import { useCallback, useEffect, useState } from 'react';
import { extractTranscript, restoreTranscript } from '../core/transcripts';
import type { Transcript } from '../youtube/transcript';

type Request = { videoId: string; trackId: string; fresh: boolean };
type Result = {
  request: Request;
  data: Transcript | null;
  loading: boolean;
  cached: boolean;
  error: string | null;
};

export function usePlayerTranscript(videoId: string | null) {
  const [request, setRequest] = useState<Request | null>(null);
  const [result, setResult] = useState<Result | null>(null);
  const open = !!videoId && request?.videoId === videoId;
  const current = open && result?.request === request ? result : null;
  const close = useCallback(() => setRequest(null), []);
  useEffect(() => {
    if (!request) return;
    if (request.videoId !== videoId) {
      setRequest(null);
      return;
    }
    const controller = new AbortController();
    setResult({ request, data: null, loading: true, cached: false, error: null });
    void (async () => {
      const saved = request.fresh
        ? null : await restoreTranscript(request.videoId).catch(() => null);
      if (controller.signal.aborted) return;
      const cached = !!saved && (request.trackId === 'default' || saved.trackId === request.trackId);
      const data = cached ? saved
        : await extractTranscript(request.videoId, request.trackId, controller.signal, request.fresh);
      if (!controller.signal.aborted) setResult({ request, data, loading: false, cached, error: null });
    })().catch(error => {
      if (!controller.signal.aborted) setResult({
        request, data: null, loading: false, cached: false,
        error: error instanceof Error ? error.message : 'The transcript could not load.',
      });
    });
    return () => controller.abort();
  }, [request, videoId]);
  return {
    open,
    data: current?.data ?? null,
    loading: open && (!current || current.loading),
    cached: current?.cached ?? false,
    error: current?.error ?? null,
    close,
    toggle: () => setRequest(previous => previous?.videoId === videoId || !videoId
      ? null : { videoId, trackId: 'default', fresh: false }),
    extract: (trackId?: string) => {
      if (videoId) setRequest({ videoId, trackId: trackId ?? current?.data?.trackId ?? 'default', fresh: !trackId });
    },
  };
}
