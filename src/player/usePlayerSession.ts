import { useCallback, useEffect, useRef, useState } from 'react';
import type WebView from 'react-native-webview';
import { coreRequest } from '../../modules/govue-core';
import { extractTranscript } from '../core/transcripts';
import type { Transcript } from '../youtube/transcript';
import type { useMediaBrowser } from '../media/useMediaBrowser';
import { useSurfaceOrientation } from './useSurfaceOrientation';
import { usePipControls } from './usePipControls';
import { usePlayerTranscript } from './usePlayerTranscript';
import { emptyCaptionFrame, type CaptionFrame, type CaptionLayout } from './captions';

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
  videoId: string | null,
  watchVisible = true,
  chatVisible = false,
) {
  const [queue, setQueue] = useState(initial);
  const [minimized, updateMini] = useState(false);
  // Chat changes the presentation without replacing playback or the previous
  // embedded/fullscreen mode, which is restored when chat closes.
  const mini = minimized || (chatVisible && !!media.playback && media.settings.miniPlayer);
  const [locked, setLocked] = useState(false);
  const [pip, setPip] = useState(false);
  const transcript = usePlayerTranscript(videoId, undefined, media.settings.audioKey);
  useEffect(() => {
    if (mini || pip || locked) transcript.close();
  }, [mini, pip, locked, transcript.close]);
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
  const [captionFrame, setCaptionFrame] = useState(emptyCaptionFrame);
  const captionLayout = useRef<CaptionLayout | null>(null);
  const refreshCaption = useRef<(() => void) | null>(null);
  const setCaptionLayout = useCallback((layout: CaptionLayout) => {
    const previous = captionLayout.current;
    if (previous?.sourceText === layout.sourceText && previous.segmentIndex === layout.segmentIndex &&
      previous.lines.length === layout.lines.length && previous.lines.every((line, index) => line === layout.lines[index])) return;
    captionLayout.current = layout;
    refreshCaption.current?.();
  }, []);
  const [captions, setCaptions] = useState<Transcript | null>(null);
  const [captionBusy, setCaptionBusy] = useState(false);
  const [captionError, setCaptionError] = useState('');
  const pending = useRef<AbortController | null>(null);
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
    pending.current?.abort();
    pending.current = null;
    setCaptionFrame(emptyCaptionFrame);
    captionLayout.current = null;
    setCaptionError('');
    setCaptions(null);
    const next =
      !force && key === mediaRef.current.settings.subtitleKey ? '' : key;
    media.updateSettings({ subtitleKey: next });
    if (!next || !media.playback) {
      setCaptionBusy(false);
      return;
    }
    const videoId = media.playback.videoId;
    const controller = new AbortController();
    pending.current = controller;
    setCaptionBusy(true);
    void extractTranscript(videoId, next, controller.signal).then(data => {
      if (pending.current === controller) setCaptions(data);
    }).catch(error => {
      if (pending.current === controller && !controller.signal.aborted)
        setCaptionError(error instanceof Error ? error.message : 'Captions could not load.');
    }).finally(() => {
      if (pending.current === controller) { pending.current = null; setCaptionBusy(false); }
    });
  }
  useEffect(() => {
    pending.current?.abort();
    pending.current = null;
    setCaptionFrame(emptyCaptionFrame);
    captionLayout.current = null;
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
      pending.current?.abort();
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
        const frame = await coreRequest<CaptionFrame>({
          op: 'player_caption_frame',
          videoId: captions.videoId,
          trackId: captions.trackId,
          position: mediaRef.current.getPosition(),
          layout: captionLayout.current,
        });
        if (active) setCaptionFrame(previous => previous.text === frame.text &&
          previous.sourceText === frame.sourceText && previous.segmentIndex === frame.segmentIndex ? previous : frame);
      } catch {
      } finally {
        busy = false;
      }
    };
    refreshCaption.current = () => { void sample(); };
    void sample();
    const listener = media.player.addListener('timeUpdate', () => {
      void sample();
    });
    return () => {
      active = false;
      refreshCaption.current = null;
      listener.remove();
    };
  }, [captions, media.player]);
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
    fullscreen: fullscreen && !chatVisible,
    setFullscreen,
    pip,
    setPip,
    pipControls,
    mini,
    setMini,
    locked,
    setLocked,
    caption: captionFrame.text,
    captionFrame,
    setCaptionLayout,
    captions,
    captionBusy,
    captionError,
    selectCaption,
    transcript,
  };
}
