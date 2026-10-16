import { useEffect, useRef, useState } from 'react';
import type { VideoPlayer } from 'expo-video';
import type WebView from 'react-native-webview';
import { isYouTubeUrl, youtubeVideoId } from '../browser/address';
import { playbackPositionScript } from '../youtube/pageBridge';

type Request = { id: string; videoId: string };

export function useTranscriptPosition(webView: React.RefObject<WebView | null>, player: VideoPlayer,
  videoId: string | null, enabled: boolean, nativeVideoId: string | null) {
  const pending = useRef<Request | null>(null);
  const sequence = useRef(0);
  const [sample, setSample] = useState<{ request: Request; seconds: number | null } | null>(null);
  useEffect(() => {
    if (!enabled || !videoId || (nativeVideoId && nativeVideoId !== videoId)) return;
    const request = { id: `position:${Date.now()}:${++sequence.current}`, videoId };
    pending.current = request;
    const update = (seconds: number) => {
      if (pending.current === request) setSample({ request, seconds: Number.isFinite(seconds) && seconds >= 0 ? seconds : null });
    };
    if (nativeVideoId) {
      update(player.currentTime);
      const listener = player.addListener('timeUpdate', ({ currentTime }) => update(currentTime));
      return () => { pending.current = null; listener.remove(); };
    }
    const poll = () => webView.current?.injectJavaScript(playbackPositionScript(request.id, videoId));
    poll();
    const timer = setInterval(poll, 250);
    return () => { pending.current = null; clearInterval(timer); };
  }, [webView, player, videoId, enabled, nativeVideoId]);

  function handlePosition(message: { requestId?: unknown; videoId?: unknown; seconds?: unknown }, origin: string) {
    const request = pending.current;
    if (!request || nativeVideoId || !enabled || request.videoId !== videoId || message.videoId !== videoId || message.requestId !== request.id || !isYouTubeUrl(origin)) return;
    // Android may supply just the HTTPS origin; document URLs must match too.
    const url = new URL(origin);
    const originOnly = url.pathname === '/' && !url.search && !url.hash;
    if (url.username || url.password || (url.port && url.port !== '443') || (!originOnly && youtubeVideoId(origin) !== videoId)) return;
    if (message.seconds !== null && (typeof message.seconds !== 'number' || !Number.isFinite(message.seconds) || message.seconds < 0)) return;
    setSample({ request, seconds: message.seconds });
  }
  const position = enabled && sample?.request === pending.current && sample.request.videoId === videoId ? sample.seconds : null;
  return { position, handlePosition };
}
