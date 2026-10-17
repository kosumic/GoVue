import { useEffect, useRef, useState } from 'react';
import type { VideoPlayer } from 'expo-video';
import type WebView from 'react-native-webview';
import { isYouTubeUrl, youtubeVideoId } from '../browser/address';
import { cancelPositionScript, pauseScript, playbackPositionScript } from '../youtube/pageScripts';
import { advanceTranscriptPlayback } from '../core/transcripts';
import type { Transcript } from '../youtube/transcript';

type Sample = { position: number | null; playing: boolean; seeking: boolean; rate: number; at: number };
type Request = { id: string; videoId: string; update: (sample: Sample) => void; reset: () => void };

export function useTranscriptPosition(webView: React.RefObject<WebView | null>, player: VideoPlayer,
  videoId: string | null, enabled: boolean, nativeVideoId: string | null, transcript: Transcript | null = null, pauseAfterLine = false) {
  const pending = useRef<Request | null>(null);
  const sequence = useRef(0);
  const [sample, setSample] = useState<{ request: Request; seconds: number | null } | null>(null);
  useEffect(() => {
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
    } };
    pending.current = request;
    async function update(sample: Sample) {
      if (pending.current !== request) return;
      setSample(previous => previous?.request === request && previous.seconds === sample.position
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
      player.timeUpdateEventInterval = 0.1;
      const sample = () => update({ position: player.currentTime, playing: player.playing, seeking: false, rate: player.playbackRate, at: Date.now() });
      void sample();
      const listener = player.addListener('timeUpdate', sample);
      const playing = player.addListener('playingChange', sample);
      return () => { pending.current = null; listener.remove(); playing.remove(); player.timeUpdateEventInterval = interval; };
    }
    const poll = () => webView.current?.injectJavaScript(playbackPositionScript(request.id, videoId, pauseAfterLine));
    poll();
    const timer = setInterval(poll, 100);
    return () => { pending.current = null; clearInterval(timer); webView.current?.injectJavaScript(cancelPositionScript(request.id)); };
  }, [webView, player, videoId, enabled, nativeVideoId, transcript, pauseAfterLine]);

  function handlePosition(message: { requestId?: unknown; videoId?: unknown; seconds?: unknown; playing?: unknown; seeking?: unknown; rate?: unknown }, origin: string) {
    const request = pending.current;
    if (!request || nativeVideoId || !enabled || request.videoId !== videoId || message.videoId !== videoId || message.requestId !== request.id || !isYouTubeUrl(origin)) return;
    // Android may supply just the HTTPS origin; document URLs must match too.
    const url = new URL(origin);
    const originOnly = url.pathname === '/' && !url.search && !url.hash;
    if (url.username || url.password || (url.port && url.port !== '443') || (!originOnly && youtubeVideoId(origin) !== videoId)) return;
    if (message.seconds !== null && (typeof message.seconds !== 'number' || !Number.isFinite(message.seconds) || message.seconds < 0)) return;
    if (pauseAfterLine && (typeof message.playing !== 'boolean' || typeof message.seeking !== 'boolean' || typeof message.rate !== 'number' || !Number.isFinite(message.rate) || message.rate <= 0 || message.rate > 16)) return;
    request.update({ position: message.seconds, playing: message.playing === true, seeking: message.seeking === true,
      rate: typeof message.rate === 'number' ? message.rate : 1, at: Date.now() });
  }
  const position = enabled && sample?.request === pending.current && sample.request.videoId === videoId ? sample.seconds : null;
  return { position, handlePosition, reset: () => pending.current?.reset() };
}
