import { useEffect, useRef, useState } from 'react';
import type WebView from 'react-native-webview';
import { coreRequest } from '../../modules/lingoplay-core';
import { acceptTranscript } from '../core/transcripts';
import { extractionScript } from '../youtube/pageBridge';
import type { Transcript } from '../youtube/transcript';
import type { useMediaBrowser } from '../media/useMediaBrowser';
import { useSurfaceOrientation } from './useSurfaceOrientation';
import { usePipControls } from './usePipControls';

export type QueueItem = {
  videoId: string;
  title: string;
  author: string;
  position?: number;
  playlistId?: string | null;
};
export type QueueState = {
  queue: QueueItem[];
  history: QueueItem[];
  enabled: boolean;
  loopMode: 'next' | 'repeat' | 'pause' | 'shuffle';
  hasPrevious: boolean;
  hasNext: boolean;
  next: QueueItem | null;
};
const initial: QueueState = {
  queue: [],
  history: [],
  enabled: false,
  loopMode: 'next',
  hasPrevious: false,
  hasNext: false,
  next: null,
};

export function usePlayerSession(
  media: ReturnType<typeof useMediaBrowser>,
  webView: React.RefObject<WebView | null>,
  navigate: (url: string) => void,
  watchVisible = true,
) {
  const [queue, setQueue] = useState(initial);
  const [mini, updateMini] = useState(false);
  const [locked, setLocked] = useState(false);
  const [pip, setPip] = useState(false);
  const { fullscreen, setFullscreen } = useSurfaceOrientation({
    visible: watchVisible && !!media.playback,
    mini,
    locked,
    pip,
    casting: media.cast.state.active && !media.cast.state.link,
  });
  const setMini = (enabled: boolean) => {
    if (enabled && fullscreen) setFullscreen(false);
    updateMini(enabled);
  };
  const [caption, setCaption] = useState('');
  const [captions, setCaptions] = useState<Transcript | null>(null);
  const [captionBusy, setCaptionBusy] = useState(false);
  const [captionError, setCaptionError] = useState('');
  const pending = useRef<{
    id: string;
    videoId: string;
    timer: ReturnType<typeof setTimeout>;
  } | null>(null);
  const writes = useRef(Promise.resolve());
  const mediaRef = useRef(media);
  mediaRef.current = media;
  const navigateRef = useRef(navigate);
  navigateRef.current = navigate;
  const queueRef = useRef(queue);
  queueRef.current = queue;
  async function action(op: string, values: Record<string, unknown> = {}) {
    let result = initial;
    const id = mediaRef.current.playback?.videoId || '';
    const work = writes.current
      .catch(() => {})
      .then(async () => {
        result = await coreRequest<QueueState>({
          op: `player_${op}`,
          videoId: id,
          ...values,
        });
        setQueue(result);
        queueRef.current = result;
      });
    writes.current = work;
    await work;
    return result;
  }
  const go = (item: QueueItem) => {
    setMini(false);
    navigateRef.current(
      `https://m.youtube.com/watch?v=${item.videoId}${item.playlistId ? `&list=${encodeURIComponent(item.playlistId)}` : ''}${item.position ? `&t=${Math.floor(item.position)}s` : ''}`,
    );
  };
  async function advance(direction: 'next' | 'previous' | 'ended') {
    const current = mediaRef.current;
    const result = await action(direction);
    if (current.playback?.videoId !== mediaRef.current.playback?.videoId)
      return;
    if (direction === 'ended' && result.loopMode === 'repeat') {
      await current.replay();
      return;
    }
    if (result.next) {
      go(result.next);
      return;
    }
  }
  useEffect(() => {
    void action('state').catch(() => {});
  }, []);
  useEffect(() => {
    const playback = media.playback;
    if (!playback) return;
    void action('history', {
      item: {
        videoId: playback.videoId,
        title: playback.title,
        author: playback.author,
      },
    }).catch(() => {});
    const timer = setInterval(() => {
      if (mediaRef.current.settings.rememberPosition)
        void action('position', {
          position: mediaRef.current.getPosition(),
        }).catch(() => {});
    }, 5000);
    return () => clearInterval(timer);
  }, [media.playback?.videoId]);
  useEffect(() => {
    media.player.loop = queue.loopMode === 'repeat';
  }, [queue.loopMode, media.player]);
  useEffect(() => {
    const listener = media.player.addListener('playToEnd', () => {
      const cast = mediaRef.current.cast.stateRef.current;
      if (cast.active && cast.acknowledged && !cast.link) return;
      if (queueRef.current.loopMode !== 'repeat')
        void advance('ended').catch(() => {});
    });
    return () => listener.remove();
  }, [media.player]);
  const remoteEnd = useRef('');
  useEffect(() => {
    const cast = media.cast.state;
    if (!cast.active || !cast.acknowledged || cast.link) return;
    if (cast.phase !== 'ended') {
      remoteEnd.current = '';
      return;
    }
    const key = `${cast.id}:${cast.videoId}`;
    if (remoteEnd.current === key) return;
    remoteEnd.current = key;
    void advance('ended').catch(() => {});
  }, [media.cast.state.id, media.cast.state.phase]);
  function selectCaption(key: string, force = false) {
    if (pending.current) clearTimeout(pending.current.timer);
    pending.current = null;
    setCaption('');
    setCaptionError('');
    setCaptions(null);
    const next =
      !force && key === mediaRef.current.settings.subtitleKey ? '' : key;
    media.updateSettings({ subtitleKey: next });
    if (!next || !media.playback) {
      setCaptionBusy(false);
      return;
    }
    const id = `caption:${Date.now()}`;
    const videoId = media.playback.videoId;
    const timer = setTimeout(() => {
      if (pending.current?.id === id) {
        pending.current = null;
        setCaptionBusy(false);
        setCaptionError('Captions could not load. Try another language.');
      }
    }, 45000);
    pending.current = { id, videoId, timer };
    setCaptionBusy(true);
    webView.current?.injectJavaScript(extractionScript(id, next, false));
  }
  useEffect(() => {
    if (pending.current) clearTimeout(pending.current.timer);
    pending.current = null;
    setCaption('');
    setCaptions(null);
    setCaptionBusy(false);
    setCaptionError('');
    if (
      media.playback &&
      media.settings.subtitleKey &&
      media.catalog?.subtitles?.some((t) => t.id === media.settings.subtitleKey)
    )
      selectCaption(media.settings.subtitleKey, true);
    return () => {
      if (pending.current) clearTimeout(pending.current.timer);
      pending.current = null;
    };
  }, [media.playback?.videoId]);
  useEffect(() => {
    if (!captions) return;
    let active = true;
    let busy = false;
    const sample = async () => {
      if (busy) return;
      busy = true;
      try {
        const text = await coreRequest<string>({
          op: 'player_caption',
          videoId: captions.videoId,
          trackId: captions.trackId,
          position: mediaRef.current.getPosition(),
        });
        if (active) setCaption(text);
      } catch {
      } finally {
        busy = false;
      }
    };
    void sample();
    const listener = media.player.addListener('timeUpdate', () => {
      void sample();
    });
    return () => {
      active = false;
      listener.remove();
    };
  }, [captions, media.player]);
  async function handleMessage(
    raw: string,
    origin: string,
    message: { type?: string; requestId?: string; videoId?: string },
  ) {
    if (
      message.type !== 'transcript' ||
      message.requestId !== pending.current?.id
    )
      return false;
    const request = pending.current;
    if (!request) return false;
    try {
      const result = await acceptTranscript(
        raw,
        origin,
        request.id,
        request.videoId,
      );
      if (pending.current !== request) return true;
      if (result.ok) setCaptions(result.transcript);
      else setCaptionError(result.error);
    } catch (e) {
      if (pending.current === request)
        setCaptionError(
          e instanceof Error ? e.message : 'Captions could not load.',
        );
    } finally {
      if (pending.current === request) {
        clearTimeout(request.timer);
        pending.current = null;
        setCaptionBusy(false);
      }
    }
    return true;
  }
  function pageTerminated() {
    if (pending.current) {
      clearTimeout(pending.current.timer);
      pending.current = null;
      setCaptionBusy(false);
      setCaptionError(
        'The caption request stopped. Reload the page and try again.',
      );
    }
  }
  const pipControls = usePipControls({
    media,
    pip,
    hasPrevious: queue.hasPrevious,
    hasNext: queue.hasNext,
    advance,
  });
  return {
    queue,
    action,
    advance,
    go,
    fullscreen,
    setFullscreen,
    pip,
    setPip,
    pipControls,
    mini,
    setMini,
    locked,
    setLocked,
    caption,
    captions,
    captionBusy,
    captionError,
    selectCaption,
    handleMessage,
    pageTerminated,
  };
}
