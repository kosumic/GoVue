import { useCallback, useEffect, useRef, useState } from 'react';
import {
  ActivityIndicator,
  Alert,
  Animated,
  BackHandler,
  Image,
  Linking,
  Platform,
  Pressable,
  Share,
  StyleSheet,
  Text,
  useColorScheme,
  View,
} from 'react-native';
import {
  SafeAreaProvider,
  SafeAreaView,
  useSafeAreaInsets,
} from 'react-native-safe-area-context';
import { StatusBar } from 'expo-status-bar';
import WebView, { type WebViewMessageEvent } from 'react-native-webview';
import type { VideoView } from 'expo-video';
import * as ScreenOrientation from 'expo-screen-orientation';
import { NavigationBar } from 'expo-navigation-bar';
import {
  ArrowLeft,
  Download,
  Plus as ListPlus,
  Play,
  Share2,
  X,
} from './src/player/icons';
import { coreRequest } from './modules/govue-core';
import { BrowserTab, type TabHandle } from './src/browser/BrowserTab';
import { isWebUrl, isYouTubeUrl, youtubeVideoId } from './src/browser/address';
import { useMediaBrowser } from './src/media/useMediaBrowser';
import { useDownloads } from './src/media/downloads';
import { embeddedPlayerFrame } from './src/media/playerLayout';
import {
  usePlayerSession,
  type QueueItem,
} from './src/player/usePlayerSession';
import { PlayerSurface } from './src/player/PlayerSurface';
import { PlayerSheet, SheetRow } from './src/player/PlayerSheet';
import { ExtensionSettings } from './src/components/ExtensionSettings';
import {
  DownloadConfirm,
  type DownloadTarget,
} from './src/components/DownloadConfirm';
import { DownloadLibrary } from './src/components/DownloadLibrary';
import { TranscriptPanel } from './src/components/TranscriptPanel';
import { AiSheet } from './src/components/AiSheet';
import { useTranscriptPosition } from './src/media/useTranscriptPosition';
import {
  MediaResolver,
  type MediaResolverHandle,
} from './src/media/MediaResolver';
import {
  BROWSER_BRIDGE,
  browserPreferencesScript,
} from './src/youtube/browserBridge';
import { TokenRuntime } from './src/media/TokenRuntime';
import { mediaSettingsScript } from './src/youtube/mediaBridge';
import type { MediaSettings } from './src/core/media';
import { Gallery } from './src/components/Gallery';
import { validateGallery } from './src/core/gallery';
import { HapticStrength } from './src/player/Haptics';
import { useMiniPlayer } from './src/player/useMiniPlayer';
import { useBrowserIdentity } from './src/browser/useBrowserIdentity';
import { useBrowserLaunches } from './src/browser/useBrowserLaunches';
import {
  colors,
  lightColors,
  ThemeContext,
  type ThemeColors,
} from './src/theme';

type Tab = { id: number; url: string; kind: string; generation?: number };
type BrowserState = {
  tabs: Tab[];
  activeId: number;
  canGoBack: boolean;
  target: { id: number; url: string; load?: boolean; dismiss?: boolean } | null;
  transient: 'none' | 'search' | 'media_menu';
  revision: number;
  suspendedWatchId?: number | null;
};
const initial: BrowserState = {
  tabs: [{ id: 1, url: 'https://m.youtube.com/', kind: 'home' }],
  activeId: 1,
  canGoBack: false,
  target: null,
  revision: 0,
  transient: 'none',
};
type Page = 'settings' | 'ai' | 'data' | 'about' | null;

function Browser() {
  const insets = useSafeAreaInsets();
  const systemAppearance = useColorScheme();
  const [appearance, setAppearance] = useState<boolean | null>(null);
  const dark = appearance ?? systemAppearance !== 'light';
  const palette = dark ? colors : lightColors;
  const s = makeStyles(palette);
  useEffect(() => {
    if (Platform.OS === 'android')
      NavigationBar.setStyle(dark ? 'dark' : 'light');
  }, [dark]);
  const browserIdentity = useBrowserIdentity();
  const [browser, setBrowser] = useState(initial);
  const browserRef = useRef(browser);
  browserRef.current = browser;
  const handles = useRef(new Map<number, TabHandle>());
  const browserWrites = useRef(Promise.resolve());
  const navigationSequence = useRef(0);
  const backAvailable = useRef(new Map<number, boolean>());
  const [watch, setWatch] = useState<{ id: number; videoId: string } | null>(
    null,
  );
  const watchRef = useRef(watch);
  watchRef.current = watch;
  const mediaView = useRef<WebView | null>(null);
  const nativeVideo = useRef<VideoView | null>(null);
  const [pageAgent, setPageAgent] = useState('');
  const [page, setPage] = useState<Page>(null);
  const [aiVisible, setAiVisible] = useState(false);
  const [aiVideoId, setAiVideoId] = useState<string | null>(null);
  const [sheet, setSheet] = useState<'library' | null>(null);
  const [card, setCard] = useState<QueueItem | null>(null);
  const [gallery, setGallery] = useState<{
    urls: string[];
    index: number;
  } | null>(null);
  const [frame, setFrame] = useState({ width: 0, height: 0 });
  const [workspaceSize, setWorkspaceSize] = useState({ width: 0, height: 0 });
  const [downloadTarget, setDownloadTarget] = useState<DownloadTarget | null>(
    null,
  );
  const [notice, setNotice] = useState('');
  const resolver = useRef<MediaResolverHandle | null>(null);
  const downloads = useDownloads(resolver);
  const completedDownloads = downloads.entries
    .filter((entry) => entry.watchPageDownloaded)
    .map((entry) => entry.videoId)
    .sort()
    .join(',');
  useEffect(() => {
    const completed = new Set(completedDownloads.split(','));
    for (const tab of browser.tabs) {
      const videoId = youtubeVideoId(tab.url);
      if (!videoId) continue;
      handles.current
        .get(tab.id)
        ?.inject(
          `window.dispatchEvent(new CustomEvent('downloadStatus',{detail:${JSON.stringify({ videoId, watchPageDownloaded: completed.has(videoId) })}}));true;`,
        );
    }
  }, [completedDownloads, browser.tabs]);
  const media = useMediaBrowser(
    mediaView,
    watch?.videoId || null,
    pageAgent,
  );
  const run = (fn: () => Promise<unknown>) => {
    void fn().catch((e) => {
      if (e instanceof Error && e.name === 'AbortError') return;
      Alert.alert(
        'GoVue',
        e instanceof Error ? e.message : 'Please try again.',
      );
    });
  };
  async function browserAction(
    op: string,
    values: Record<string, unknown> = {},
  ) {
    const work = browserWrites.current
      .catch(() => {})
      .then(() =>
        coreRequest<BrowserState>({
          op: `browser_${op}`,
          ...values,
        }),
      );
    browserWrites.current = work.then(
      () => {},
      () => {},
    );
    const next = await work;
    if (next.revision >= browserRef.current.revision) {
      setBrowser(next);
      browserRef.current = next;
      if (next.target?.dismiss)
        handles.current
          .get(next.target.id)
          ?.inject('window.Lite?.ui?.dismiss();true;');
    }
    return next;
  }
  const navigate = useCallback((target: string | { videoId: string }) => {
    sessionRef.current?.transcript.close();
    const sequence = ++navigationSequence.current;
    run(async () => {
      const current = mediaRef.current;
      const next = await browserAction('open', {
        ...(typeof target === 'string' ? { url: target } : target),
        suspendWatch:
          !!current.playback &&
          !current.playback.offline &&
          current.settings.miniPlayer,
      });
      if (sequence !== navigationSequence.current) return;
      if (next.target?.load)
        handles.current.get(next.target.id)?.navigate(next.target.url);
      setPage(null);
      setSheet(null);
      setCard(null);
    });
  }, []);
  useBrowserLaunches(navigate, setNotice);
  const session = usePlayerSession(
    media,
    mediaView,
    navigate,
    media.playback?.offline ? media.playback.videoId : watch?.videoId ?? null,
    !!watch &&
      browser.activeId === watch.id &&
      !page &&
      !aiVisible &&
      !media.playback?.offline,
    aiVisible,
  );
  const sessionRef = useRef(session);
  sessionRef.current = session;
  useEffect(() => {
    if (
      session.fullscreen || session.pip ||
      workspaceSize.width <= 0 || workspaceSize.height <= 0
    ) return;
    // PiP and layout events can arrive in either order. Reconcile again when
    // PiP ends even if the workspace's final layout event already arrived.
    setFrame(previous =>
      previous.width === workspaceSize.width && previous.height === workspaceSize.height
        ? previous : { width: workspaceSize.width, height: workspaceSize.height });
  }, [session.fullscreen, session.pip, workspaceSize.width, workspaceSize.height]);
  const mediaRef = useRef(media);
  mediaRef.current = media;
  useEffect(() => {
    if (!watch || browser.tabs.some((tab) => tab.id === watch.id)) return;
    if (!mediaRef.current.playback?.offline)
      mediaRef.current.closePlayer(false, true);
    sessionRef.current.setMini(false);
    watchRef.current = null;
    mediaView.current = null;
    setWatch(null);
  }, [browser.tabs, watch?.id]);
  useEffect(() => {
    if (
      browser.suspendedWatchId &&
      !media.playback &&
      !media.preparing &&
      !media.busy
    )
      run(() => browserAction('drop_watch'));
  }, [browser.suspendedWatchId, media.playback, media.preparing, media.busy]);
  useEffect(() => {
    run(() => browserAction('state'));
  }, []);
  useEffect(() => {
    if (!notice) return;
    const timer = setTimeout(() => setNotice(''), 2500);
    return () => clearTimeout(timer);
  }, [notice]);
  useEffect(() => {
    if (session.fullscreen)
      void ScreenOrientation.lockAsync(
        ScreenOrientation.OrientationLock.LANDSCAPE,
      ).catch(() => {});
    else void ScreenOrientation.unlockAsync().catch(() => {});
  }, [session.fullscreen]);
  useEffect(() => {
    if (Platform.OS === 'android') NavigationBar.setHidden(session.fullscreen);
    return () => {
      if (Platform.OS === 'android') NavigationBar.setHidden(false);
    };
  }, [session.fullscreen]);
  useEffect(() => {
    const visible =
      watch &&
      browser.activeId === watch.id &&
      youtubeVideoId(browser.tabs.find((t) => t.id === watch.id)?.url || '') ===
        watch.videoId;
    if (!media.settings.miniPlayer || media.playback?.offline)
      session.setMini(false);
    else if (watch && !visible && media.playback && media.settings.miniPlayer)
      session.setMini(true);
    else if (visible) session.setMini(false);
  }, [
    browser.activeId,
    browser.tabs,
    watch?.id,
    watch?.videoId,
    media.playback?.videoId,
    media.playback?.offline,
    media.settings.miniPlayer,
  ]);
  const openDownload = (id?: string) => {
    const target = id || media.playback?.videoId || watch?.videoId;
    if (!target) return;
    const metadata =
      media.catalog?.videoId === target
        ? media.catalog
        : card?.videoId === target
          ? card
          : session.queue.queue.find((i) => i.videoId === target);
    setDownloadTarget({
      name: '',
      items: [
        {
          videoId: target,
          title: metadata?.title || 'Video',
          author: metadata?.author || '',
        },
      ],
    });
  };
  const goBack = useCallback(() => {
    // The chat handles its reader, drawer and screen before browser navigation.
    if (aiVisible) return false;
    const state = sessionRef.current;
    if (state.locked) {
      state.setLocked(false);
      return true;
    }
    if (state.transcript.open) {
      state.transcript.close();
      return true;
    }
    if (state.fullscreen) {
      state.setFullscreen(false);
      if (mediaRef.current.playback?.offline) {
        state.setMini(false);
        mediaRef.current.closePlayer(false, true);
        setSheet('library');
      }
      return true;
    }
    if (page) {
      setPage(page === 'data' || page === 'ai' ? 'settings' : null);
      return true;
    }
    if (gallery) {
      setGallery(null);
      return true;
    }
    if (card) {
      setCard(null);
      return true;
    }
    const id = browserRef.current.activeId;
    if (
      browserRef.current.transient &&
      browserRef.current.transient !== 'none'
    ) {
      run(() => browserAction('dismiss'));
      return true;
    }
    if (
      watchRef.current?.id === id &&
      youtubeVideoId(
        browserRef.current.tabs.find((t) => t.id === id)?.url || '',
      ) === watchRef.current.videoId &&
      mediaRef.current.playback &&
      mediaRef.current.settings.miniPlayer
    ) {
      state.setMini(true);
      run(() => browserAction('back', { suspendWatch: true }));
      return true;
    }
    if (backAvailable.current.get(id)) {
      handles.current.get(id)?.back();
      return true;
    }
    if (browserRef.current.canGoBack) {
      run(() => browserAction('back'));
      return true;
    }
    return false;
  }, [page, gallery, card, aiVisible]);
  useEffect(() => {
    const sub = BackHandler.addEventListener('hardwareBackPress', goBack);
    return () => sub.remove();
  }, [goBack]);
  function updateUrl(id: number, url: string, back: boolean) {
    if (!isWebUrl(url)) return;
    if (id === watchRef.current?.id && youtubeVideoId(url) !== watchRef.current.videoId)
      sessionRef.current.transcript.close();
    backAvailable.current.set(id, back);
    const tab = browserRef.current.tabs.find((t) => t.id === id);
    if (tab?.url !== url)
      void browserAction('update', { id, url }).catch(() => {});
    const videoId = youtubeVideoId(url);
    if (
      videoId &&
      tab?.kind !== 'shorts' &&
      !new URL(url).pathname.startsWith('/shorts/') &&
      (tab?.kind === 'watch' || browserRef.current.activeId === id)
    ) {
      mediaView.current = handles.current.get(id)?.view.current || null;
      if (
        watchRef.current?.id !== id ||
        watchRef.current?.videoId !== videoId
      ) {
        setWatch({ id, videoId });
        watchRef.current = { id, videoId };
      }
    }
  }
  function handleMessage(id: number, event: WebViewMessageEvent) {
    const { data: raw, url: origin } = event.nativeEvent;
    if (!isYouTubeUrl(origin) || raw.length > 8000000) return;
    let message: Record<string, any>;
    try {
      message = JSON.parse(raw);
    } catch {
      return;
    }
    if (!message || typeof message !== 'object') return;
    if (
      message.type === 'page' &&
      typeof message.url === 'string' &&
      isYouTubeUrl(message.url) &&
      new URL(message.url).origin === new URL(origin).origin
    ) {
      const knownUrl = browserRef.current.tabs.find(
        (tab) => tab.id === id,
      )?.url;
      if (!knownUrl) return;
      void coreRequest<boolean>({
        op: 'browser_message_origin',
        origin,
        documentUrl: message.url,
        knownUrl,
      })
        .then((trusted) => {
          if (
            !trusted ||
            browserRef.current.tabs.find((tab) => tab.id === id)?.url !==
              knownUrl
          )
            return;
          updateUrl(id, message.url, backAvailable.current.get(id) || false);
          if (
            typeof message.userAgent === 'string' &&
            message.userAgent.length < 1000 &&
            id === watchRef.current?.id
          )
            setPageAgent(message.userAgent);
        })
        .catch(() => {});
      return;
    }
    if (message.type === 'litube') {
      if (
        typeof message.documentUrl !== 'string' ||
        !isYouTubeUrl(message.documentUrl) ||
        new URL(message.documentUrl).origin !== new URL(origin).origin
      )
        return;
      // UI commands can arrive while YouTube commits an SPA URL. Bind them to
      // this origin and the active tab; media replies still require their exact
      // pending request and video below, including context from the parked watch.
      const payload = message.payload;
      if (
        message.action === 'context' &&
        id === watchRef.current?.id &&
        payload?.videoId === watchRef.current.videoId
      ) {
        run(() => session.action('context', payload));
        return;
      }
      if (id !== browserRef.current.activeId) return;
      if (
        message.action === 'transient' &&
        typeof payload?.kind === 'string' &&
        Number.isSafeInteger(payload.generation)
      ) {
        void browserAction('transient', {
          id,
          origin,
          documentUrl: message.documentUrl,
          generation: payload.generation,
          kind: payload.kind,
        }).catch(() => {});
        return;
      }
      if (
        message.action === 'appearance' &&
        typeof payload?.dark === 'boolean' &&
        payload.url === message.documentUrl
      ) {
        setAppearance(payload.dark);
        return;
      }
      if (message.action === 'queueError') {
        setNotice('Could not add this video to the queue.');
        return;
      }
      if (
        message.action === 'shortsReady' ||
        message.action === 'shortsBlocked'
      ) {
        const tab = browserRef.current.tabs.find((tab) => tab.id === id);
        if (
          tab?.kind !== 'shorts' ||
          message.documentGeneration !== tab.generation ||
          payload?.url !== message.documentUrl ||
          youtubeVideoId(payload.url) !== youtubeVideoId(tab.url) ||
          !new URL(payload.url).pathname.startsWith('/shorts/')
        )
          return;
        if (message.action === 'shortsReady') media.closePlayer(false, true);
        else setNotice('Tap the Short to start playback.');
        return;
      }
      if (message.action === 'settings') {
        setPage('settings');
        return;
      }
      if (message.action === 'library') {
        setSheet('library');
        return;
      }
      if (message.action === 'chat') {
        setAiVideoId(youtubeVideoId(message.documentUrl));
        setAiVisible(true);
        return;
      }
      if (message.action === 'about') {
        setPage('about');
        return;
      }
      if (message.action === 'openTab' && typeof payload?.url === 'string') {
        navigate(payload.url);
        return;
      }
      if (message.action === 'back') {
        goBack();
        return;
      }
      if (
        message.action === 'share' &&
        typeof payload?.url === 'string' &&
        isYouTubeUrl(payload.url)
      ) {
        run(() => Share.share({ message: payload.url }));
        return;
      }
      if (message.action === 'queueAdd' && payload) {
        run(async () => {
          await session.action('add', { item: payload });
          setNotice('Added to queue');
        });
        return;
      }
      if (
        message.action === 'card' &&
        payload &&
        typeof payload.videoId === 'string' &&
        /^[\w-]{11}$/.test(payload.videoId)
      ) {
        setCard({
          videoId: payload.videoId,
          title: String(payload.title || 'Video').slice(0, 1000),
          author: String(payload.author || '').slice(0, 500),
        });
        return;
      }
      if (message.action === 'download' && payload) {
        if (payload.type === 'openManager') {
          setSheet('library');
          return;
        }
        if (
          payload.type === 'openConfirm' &&
          typeof payload.videoId === 'string' &&
          /^[\w-]{11}$/.test(payload.videoId)
        ) {
          openDownload(payload.videoId);
          return;
        }
        if (payload.type === 'openBatch') {
          run(async () => {
            const target = await coreRequest<DownloadTarget>({
              op: 'downloads_targets',
              name: payload.name,
              items: payload.items,
            });
            setDownloadTarget(target);
          });
          return;
        }
        if (
          payload.type === 'requestStatus' &&
          typeof payload.videoId === 'string' &&
          /^[\w-]{11}$/.test(payload.videoId)
        ) {
          const complete = downloads.entries.some(
            (e) => e.videoId === payload.videoId && e.watchPageDownloaded,
          );
          handles.current
            .get(id)
            ?.inject(
              `window.dispatchEvent(new CustomEvent('downloadStatus',{detail:${JSON.stringify({ videoId: payload.videoId, watchPageDownloaded: complete })}}));true;`,
            );
        }
      }
      if (message.action === 'gallery' && payload) {
        run(async () =>
          setGallery(await validateGallery(payload.urls, payload.index)),
        );
      }

      return;
    }
    if (id !== watchRef.current?.id) return;
    mediaView.current = handles.current.get(id)?.view.current || null;
    if (
      [
        'mediaReady',
        'playbackPosition',
        'playerLayout',
        'sponsorSkipped',
      ].includes(String(message.type))
    )
      void media.handleMessage(raw, origin, message);
  }
  const mini = useMiniPlayer({
    size: frame,
    bottomInset: insets.bottom,
    active: session.mini,
    strength: media.settings.hapticStrength,
    onDismiss: () => media.closePlayer(false, true),
    onError: () => setNotice('Could not update mini-player placement.'),
  });
  const embeddedFrame = media.playback?.offline
    ? { top: 48, left: 0, width: frame.width, height: (frame.width * 9) / 16 }
    : embeddedPlayerFrame(media.playerLayout, frame.width, frame.height);
  const playerHidden =
    (aiVisible && !session.mini) ||
    (!session.fullscreen &&
      !session.mini &&
      !media.playback?.offline &&
      (media.playerLayout?.visible !== true ||
        browser.activeId !== watch?.id ||
        youtubeVideoId(
          browser.tabs.find((t) => t.id === browser.activeId)?.url || '',
        ) !== watch?.videoId));
  const transcriptVisible = session.transcript.open && !session.mini && !session.pip && !page && !aiVisible && !playerHidden;
  const transcriptBeside = workspaceSize.width > workspaceSize.height;
  const transcriptWidth = Math.min(400, workspaceSize.width * 0.4);
  const portraitVideoHeight = Math.min(workspaceSize.height * 0.45, workspaceSize.width * 9 / 16);
  const nativeFrame = session.fullscreen
    ? transcriptVisible
      ? transcriptBeside
        ? { top: 0, left: 0, bottom: 0, right: transcriptWidth }
        : { top: 0, left: 0, right: 0, height: portraitVideoHeight }
      : StyleSheet.absoluteFill
    : session.mini
      ? mini.style
      : transcriptVisible && transcriptBeside
        ? { top: 0, left: 0, bottom: 0, right: transcriptWidth }
        : embeddedFrame;
  const transcriptFrame = transcriptBeside
    ? { top: 0, right: 0, bottom: 0, width: transcriptWidth }
    : { top: session.fullscreen ? portraitVideoHeight : Math.max(0, embeddedFrame.top + embeddedFrame.height), left: 0, right: 0, bottom: 0 };
  const remoteTranscript = media.cast.state.active && media.cast.state.acknowledged && !media.cast.state.link;
  const transcriptPosition = useTranscriptPosition(mediaView, media.player, media.playback?.videoId ?? null,
    transcriptVisible && !remoteTranscript, media.playback?.videoId ?? null,
    session.transcript.data, media.settings.pauseAfterLine);
  return (
    <ThemeContext.Provider value={palette}>
      <HapticStrength.Provider value={media.settings.hapticStrength}>
        <SafeAreaView
          style={s.safe}
          edges={session.fullscreen ? [] : ['top', 'bottom', 'left', 'right']}
        >
          <StatusBar
            style={dark || session.fullscreen ? 'light' : 'dark'}
            hidden={session.fullscreen}
          />
          <View
            style={s.workspace}
            onLayout={(e) => {
              setWorkspaceSize(e.nativeEvent.layout);
            }}
          >
            {browserIdentity.ready &&
              browser.tabs.map((tab) => (
                <BrowserTab
                  key={tab.id}
                  tab={tab}
                  userAgent={browserIdentity.userAgent}
                  ref={(handle) => {
                    if (handle) {
                      handles.current.set(tab.id, handle);
                      if (tab.id === watchRef.current?.id)
                        mediaView.current = handle.view.current;
                    } else handles.current.delete(tab.id);
                  }}
                  active={
                    !page && !aiVisible && browser.activeId === tab.id && !session.fullscreen
                  }
                  width={frame.width}
                  height={frame.height}
                  bottomInset={Platform.OS === 'android' && isYouTubeUrl(tab.url) ? insets.bottom : 0}
                  settings={media.settings}
                  onMessage={handleMessage}
                  onUrl={updateUrl}
                  onReady={(id, url) => {
                    if (
                      id === watchRef.current?.id &&
                      youtubeVideoId(url) === watchRef.current.videoId
                    )
                      media.configurePage();
                  }}
                  onDocumentStart={(id, url, generation) => {
                    if (isWebUrl(url))
                      void browserAction('document', {
                        id,
                        url,
                        generation,
                      }).catch(() => {});
                    if (id === watchRef.current?.id) {
                      media.pageTerminated('The page changed. Try again.');
                    }
                  }}
                  onDead={(id) => {
                    void browserAction('invalidate', { id }).catch(() => {});
                    if (id === watchRef.current?.id) {
                      media.pageTerminated();
                    }
                  }}
                />
              ))}
            <View pointerEvents="box-none" style={s.overlays}>
              <AiSheet
                keyboardVerticalOffset={session.fullscreen ? 0 : insets.top}
                onBeforeGameAudio={media.pause}
                visible={aiVisible}
                onClose={() => setAiVisible(false)}
                transcript={session.transcript.data}
                videoId={aiVideoId}
                onOpenVideo={videoId => {
                  navigate({ videoId });
                  session.setMini(false);
                  session.setLocked(false);
                  setAiVisible(false);
                }}
              />
              {mini.dropTarget}
              {(media.playback || media.preparing) && (
                <Animated.View
                  pointerEvents={playerHidden ? 'none' : 'auto'}
                  style={[s.native, nativeFrame, mini.motionStyle, playerHidden && s.hidden]}
                  {...(session.mini ? mini.panHandlers : {})}
                >
                  <PlayerSurface
                    media={media}
                    session={session}
                    videoRef={nativeVideo}
                    onMiniClose={mini.dismiss}
                    onError={(e) =>
                      Alert.alert(
                        'GoVue',
                        e instanceof Error ? e.message : 'Please try again.',
                      )
                    }
                    onDownload={(id) => openDownload(id)}
                    onAi={() => {
                      setAiVideoId(media.playback?.videoId ?? null);
                      setAiVisible(true);
                    }}
                    onDownloadQueue={() =>
                      setDownloadTarget({
                        name: 'Queue',
                        items: session.queue.queue,
                      })
                    }
                    onRestore={() => {
                      setAiVisible(false);
                      session.setMini(false);
                      if (media.playback?.offline) {
                        session.setFullscreen(true);
                        return;
                      }
                      const id = media.playback?.videoId;
                      if (
                        id &&
                        youtubeVideoId(
                          browser.tabs.find((t) => t.id === watch?.id)?.url || '',
                        ) !== id
                      )
                        navigate(`https://m.youtube.com/watch?v=${id}`);
                      else run(() => browserAction('restore_watch'));
                    }}
                    onBack={goBack}
                    onPip={() =>
                      run(async () => {
                        await nativeVideo.current?.startPictureInPicture();
                      })
                    }
                  />
                </Animated.View>
              )}
              {transcriptVisible && (
                <View style={[s.transcript, transcriptFrame]}>
                  <TranscriptPanel
                    data={session.transcript.data}
                    comparison={session.transcript.comparison}
                    trackIds={session.transcript.trackIds}
                    comparisonLoading={session.transcript.comparisonLoading}
                    comparisonError={session.transcript.comparisonError}
                    onSelectTrack={session.transcript.selectTrack}
                    loading={session.transcript.loading}
                    cached={session.transcript.cached}
                    error={session.transcript.error}
                    position={remoteTranscript ? media.cast.state.position ?? null : transcriptPosition.position}
                    pauseAfterLine={media.settings.pauseAfterLine}
                    onPauseAfterLine={remoteTranscript ? undefined : pauseAfterLine => media.updateSettings({ pauseAfterLine })}
                    onClose={session.transcript.close}
                    onExtract={session.transcript.extract}
                    onBeforeWordAudio={() => media.pause()}
                    onSeek={seconds => run(async () => {
                      const videoId = session.transcript.data?.videoId;
                      if (!videoId || videoId !== mediaRef.current.playback?.videoId) return;
                      transcriptPosition.reset();
                      await media.seekTo(seconds);
                      if (videoId === mediaRef.current.playback?.videoId) await media.play();
                    })}
                    onDownload={() => openDownload()}
                    downloadDisabled={!media.playback}
                  />
                </View>
              )}
              {!!page && !aiVisible && (
                <View style={s.nativePage}>
                  <ExtensionSettings
                    page={page}
                    settings={media.settings}
                    onClearCache={() => {
                      for (const handle of handles.current.values())
                        handle.view.current?.clearCache?.(true);
                    }}
                    onSettings={media.updateSettings}
                    onCategoryChange={media.updateSponsorCategory}
                    onClose={() =>
                      setPage(page === 'data' || page === 'ai' ? 'settings' : null)
                    }
                    onNavigate={(target) =>
                      target === 'library' ? setSheet('library') : setPage(target)
                    }
                  />
                </View>
              )}
              {!!(notice || media.notice) && (
                <View pointerEvents="none" style={s.toast}>
                  <Text style={s.toastText}>{notice || media.notice}</Text>
                </View>
              )}
              {media.error && !media.playback && !sheet && (
                <View style={s.mediaError}>
                  <Text style={s.errorText}>{media.error}</Text>
                  <Pressable
                    style={s.retry}
                    onPress={() => run(() => media.startNative())}
                  >
                    <Text style={s.errorText}>Retry</Text>
                  </Pressable>
                  <Pressable style={s.retry} onPress={() => media.closePlayer()}>
                    <Text style={s.errorText}>Web player</Text>
                  </Pressable>
                </View>
              )}
            </View>
          </View>
          <TokenRuntime />
          <MediaResolver ref={resolver} />
          <DownloadConfirm
            target={downloadTarget}
            onClose={() => setDownloadTarget(null)}
            resolver={resolver}
            currentCatalog={media.catalog}
            downloads={downloads}
            onError={(error) =>
              Alert.alert(
                'GoVue',
                error instanceof Error ? error.message : 'Download failed.',
              )
            }
            onSubmitted={(added, skipped) => {
              setDownloadTarget(null);
              setNotice(
                added
                  ? `${added} added to downloads${skipped ? ` · ${skipped} already present` : ''}`
                  : 'Already in downloads',
              );
            }}
          />
          <DownloadLibrary
            visible={sheet === 'library'}
            onClose={() => setSheet(null)}
            downloads={downloads}
            onError={(error) =>
              Alert.alert(
                'GoVue',
                error instanceof Error ? error.message : 'Download failed.',
              )
            }
            onOpenVideo={(id) =>
              navigate(`https://m.youtube.com/watch?v=${id}`)
            }
            onPlay={(entry) => {
              run(async () => {
                await media.playOffline(entry, downloads.uri(entry));
                setSheet(null);
                session.setMini(false);
                session.setLocked(false);
                session.setFullscreen(true);
              });
            }}
          />
          <PlayerSheet
            visible={!!card}
            title={card?.title || 'Video'}
            onClose={() => setCard(null)}
          >
            {card && (
              <>
                <Image
                  source={{
                    uri: `https://i.ytimg.com/vi/${card.videoId}/hqdefault.jpg`,
                  }}
                  style={s.cardImage}
                />
                <Text style={s.cardAuthor}>{card.author}</Text>
                <SheetRow
                  title="Play"
                  icon={<Play color="white" size={22} />}
                  onPress={() => {
                    navigate(`https://m.youtube.com/watch?v=${card.videoId}`);
                    setCard(null);
                  }}
                />
                <SheetRow
                  title="Add to queue"
                  icon={<ListPlus color="white" size={22} />}
                  onPress={() =>
                    run(async () => {
                      await session.action('add', { item: card });
                      setCard(null);
                      setNotice('Added to queue');
                    })
                  }
                />
                <SheetRow
                  title="Download"
                  icon={<Download color="white" size={22} />}
                  onPress={() => {
                    openDownload(card.videoId);
                    setCard(null);
                  }}
                />
                <SheetRow
                  title="Share"
                  icon={<Share2 color="white" size={22} />}
                  onPress={() =>
                    run(() =>
                      Share.share({
                        message: `https://www.youtube.com/watch?v=${card.videoId}`,
                      }),
                    )
                  }
                />
              </>
            )}
          </PlayerSheet>
          {gallery && (
            <Gallery target={gallery} onClose={() => setGallery(null)} />
          )}
        </SafeAreaView>
      </HapticStrength.Provider>
    </ThemeContext.Provider>
  );
}
export default function App() {
  return (
    <SafeAreaProvider>
      <Browser />
    </SafeAreaProvider>
  );
}
const makeStyles = (colors: ThemeColors) =>
  StyleSheet.create({
    safe: { flex: 1, backgroundColor: colors.background },
    fill: { flex: 1, backgroundColor: colors.background },
    // The browser paints through the bottom safe area; native overlays stay clipped.
    workspace: { flex: 1 },
    overlays: { ...StyleSheet.absoluteFill, overflow: 'hidden' },
    tab: { position: 'absolute', top: 0, left: 0 },
    parked: { opacity: 0, zIndex: -1 },
    native: {
      position: 'absolute',
      backgroundColor: '#000',
      overflow: 'hidden',
      zIndex: 2,
    },
    hidden: { opacity: 0 },
    transcript: { position: 'absolute', zIndex: 3, backgroundColor: colors.background },
    nativePage: {
      ...StyleSheet.absoluteFill,
      zIndex: 3,
      backgroundColor: colors.background,
    },
    progress: {
      position: 'absolute',
      top: 0,
      left: 0,
      right: 0,
      height: 2,
      backgroundColor: '#f00',
    },
    browserError: {
      position: 'absolute',
      bottom: 0,
      left: 0,
      right: 0,
      backgroundColor: colors.surface,
      padding: 16,
      flexDirection: 'row',
      alignItems: 'center',
      gap: 12,
    },
    errorText: { color: colors.ink, fontSize: 13, flexShrink: 1 },
    retry: { padding: 12 },
    mediaError: {
      position: 'absolute',
      top: 60,
      left: 12,
      right: 12,
      padding: 16,
      backgroundColor: colors.surface,
      zIndex: 4,
    },
    toast: {
      position: 'absolute',
      bottom: 36,
      alignSelf: 'center',
      backgroundColor: colors.raised,
      paddingHorizontal: 18,
      paddingVertical: 12,
      borderRadius: 6,
      zIndex: 5,
    },
    toastText: { color: colors.ink, fontSize: 13 },
    cardImage: { width: '100%', height: 180 },
    cardAuthor: { color: colors.muted, fontSize: 13, padding: 16 },
    gallery: { width: '100%', height: 420 },
    galleryNavigation: {
      flexDirection: 'row',
      justifyContent: 'space-between',
      padding: 20,
    },
  });
