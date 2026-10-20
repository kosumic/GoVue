import { useEffect, useRef, useState } from 'react';
import type { VideoPlayer } from 'expo-video';
import type WebView from 'react-native-webview';
import { browserRequest } from '../../modules/govue-core';
import { cancelPositionScript, pauseScript, playbackPositionScript } from '../youtube/pageScripts';
import { advanceTranscriptPlayback } from '../core/transcripts';
import type { Transcript } from '../youtube/transcript';

const NATIVE_CLOCK_INTERVAL_MS = 50;

type Sample = { position: number | null; playing: boolean; seeking: boolean; rate: number; at: number };
type Request = { id: string; videoId: string; update: (sample: Sample) => void; reset: () => void };

export function useTranscriptPosition(webView: React.RefObject<WebView | null>, player: VideoPlayer,
  videoId: string | null, enabled: boolean, nativeVideoId: string | null, transcript: Transcript | null = null, pauseAfterLine = false, onPosition?: (position: number | null) => void) {
  const pending = useRef<Request | null>(null);
  const sequence = useRef(0);
  const [sample, setSample] = useState<{ request: Request; seconds: number | null } | null>(null);
  const publish = useRef(onPosition);
  publish.current = onPosition;
  useEffect(() => {
    publish.current?.(null);
    if (!enabled || !videoId || (nativeVideoId && nativeVideoId !== videoId)) return;
    let cursor: unknown = null;
    let lastAt = 0;
    let revision = 0;
    let processing = false;
    let queued: Sample | null = null;
    const nextId = () => `position:${Date.now()}:${++sequence.current}`;
    const request: Request = { id: nextId(), videoId, update, reset() {
      webView.current?.injectJavaScript(cancelPositionScript(request.id));
      request.id = nextId();
      cursor = null; lastAt = 0; queued = null; ++revision;
      publish.current?.(null);
    } };
    pending.current = request;
    async function update(sample: Sample) {
      if (pending.current !== request) return;
      if (publish.current) {
        publish.current(sample.position);
      } else setSample(previous => previous?.request === request && previous.seconds === sample.position
        ? previous : { request, seconds: sample.position });
      if (!pauseAfterLine || transcript?.videoId !== videoId) return;
      queued = sample;
      if (processing) return;
      processing = true;
      try {
        while (queued && pending.current === request) {
          const current = queued; queued = null;
          const version = revision;
          const elapsed = lastAt ? (current.at - lastAt) / 1000 : 0;
          const result = await advanceTranscriptPlayback(transcript, cursor, { ...current, elapsed });
          if (pending.current !== request) return;
          if (version !== revision) continue;
          lastAt = current.at;
          cursor = result.cursor;
          if (Date.now() - current.at > 500) { cursor = null; continue; }
          if (result.pause && current.position !== null) {
            if (nativeVideoId) {
              if (player.playing && Math.abs(player.currentTime - current.position) <= 0.5) player.pause();
            } else webView.current?.injectJavaScript(pauseScript(request.id, videoId!, current.position));
          }
        }
      } catch { cursor = null; }
      finally { processing = false; }
    }
    if (nativeVideoId) {
      const interval = player.timeUpdateEventInterval;
      const publishing = !!publish.current;
      if (!publishing) player.timeUpdateEventInterval = 0.1;
      const sample = () => update({ position: player.currentTime, playing: player.playing, seeking: false, rate: player.playbackRate, at: Date.now() });
      void sample();
      // The reader clock must not force the entire player-control tree to update.
      const timer = publishing ? setInterval(sample, NATIVE_CLOCK_INTERVAL_MS) : null;
      const listener = publishing ? null : player.addListener('timeUpdate', sample);
      const playing = player.addListener('playingChange', sample);
      return () => { pending.current = null; publish.current?.(null); if (timer) clearInterval(timer); listener?.remove(); playing.remove(); player.timeUpdateEventInterval = interval; };
    }
    const poll = () => webView.current?.injectJavaScript(playbackPositionScript(request.id, videoId, pauseAfterLine));
    poll();
    const timer = setInterval(poll, 100);
    return () => { pending.current = null; publish.current?.(null); clearInterval(timer); webView.current?.injectJavaScript(cancelPositionScript(request.id)); };
  }, [webView, player, videoId, enabled, nativeVideoId, transcript, pauseAfterLine]);

  function handlePosition(message: { requestId?: unknown; videoId?: unknown; seconds?: unknown; playing?: unknown; seeking?: unknown; rate?: unknown }, origin: string) {
    const request = pending.current;
    if (!request || nativeVideoId || !enabled || request.videoId !== videoId) return;
    try {
      const sample = browserRequest<Omit<Sample, 'at'> | null>({ op: 'browser_position',
        message, origin, videoId, requestId: request.id, playback: pauseAfterLine });
      if (sample) request.update({ ...sample, at: Date.now() });
    } catch { /* Untrusted messages cannot interrupt playback. */ }
  }
  const position = enabled && sample?.request === pending.current && sample.request.videoId === videoId ? sample.seconds : null;
  return { position, handlePosition, reset: () => pending.current?.reset() };
}
