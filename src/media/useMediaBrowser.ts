import { useEffect, useRef, useState } from 'react';
import { Alert, AppState } from 'react-native';
import { useVideoPlayer, type VideoSource, type VideoTrack } from 'expo-video';
import type WebView from 'react-native-webview';
import {
  extractMedia,
  defaultMediaSettings,
  fetchSponsors,
  mediaSettings,
  mediaSponsorCategory,
  playbackChoices,
  playbackSpeeds,
  preparePlayback,
  type MediaCatalog,
  type MediaSettings,
  type PlaybackQuality,
  type SponsorSegment,
} from '../core/media';
import {
  mediaNativeScript,
  mediaSettingsScript,
  mediaSponsorsScript,
} from '../youtube/mediaBridge';
import { seekScript } from '../youtube/pageScripts';
import { coreRequest } from '../../modules/govue-core';
import { acceptPlayerLayout, type PlayerLayout } from './playerLayout';
import { useCastSession } from '../player/useCastSession';
import { useTranscriptPosition } from './useTranscriptPosition';

type Playback = {
  videoId: string;
  title: string;
  author: string;
  offline: boolean;
  uri: string;
  qualityHeight: number;
  qualityFps?: number;
};
type LiveTrackPlayer = {
  govueSelectVideoTrack?: (
    expectedUri: string,
    trackId: string | null,
  ) => Promise<boolean>;
};
const liveTrackMetadata = (tracks: VideoTrack[]) =>
  tracks.map((track) => ({
    height: track.size.height,
    fps: track.frameRate || 0,
    supported: track.isSupported,
    bitrate: track.averageBitrate || track.peakBitrate || 0,
  }));
type Pending = {
  requestId: string;
  videoId: string;
  timer: ReturnType<typeof setTimeout>;
  controller: AbortController;
  resolve: (value: MediaCatalog) => void;
  reject: (error: Error) => void;
};
type SponsorUi = {
  countdown: number | null;
  chip: boolean;
  highlight: boolean;
  target: number | null;
  seek: number | null;
  category?: string | null;
};
const emptySponsor: SponsorUi = {
  countdown: null,
  chip: false,
  highlight: false,
  target: null,
  seek: null,
};

export function useMediaBrowser(
  webView: React.RefObject<WebView | null>,
  videoId: string | null,
  userAgent: string,
) {
  const [settings, setSettings] = useState(defaultMediaSettings);
  const [settingsReady, setSettingsReady] = useState(false);
  const [playback, setPlayback] = useState<Playback | null>(null);
  const [catalog, setCatalog] = useState<MediaCatalog | null>(null);
  const [busy, setBusy] = useState(false);
  const [preparing, setPreparing] = useState(false);
  const [qualities, setQualities] = useState<PlaybackQuality[]>([]);
  const [speeds, setSpeeds] = useState<number[]>([]);
  const [sponsorSegments, setSponsorSegments] = useState<SponsorSegment[]>([]);
  const [sponsor, setSponsor] = useState(emptySponsor);
  const sponsorWrites = useRef(Promise.resolve());
  const [switchingQuality, setSwitchingQuality] = useState(false);
  const qualitySequence = useRef(0);
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  const catalogRef = useRef(catalog);
  catalogRef.current = catalog;
  const [playerLayout, setPlayerLayout] = useState<PlayerLayout | null>(null);
  const preparation = useRef<{ videoId: string } | null>(null);
  const manualWeb = useRef<string | null>(null);
  const autoRetryAt = useRef(0);
  const [error, setError] = useState<string | null>(null);
  const [sponsorStatus, setSponsorStatus] = useState('SponsorBlock is ready.');
  const [notice, setNotice] = useState<string | null>(null);
  const segments = useRef<SponsorSegment[]>([]);
  const sponsorOwner = useRef<string | null>(null);
  const sponsorVideoId = playback?.videoId ?? videoId;
  const identity = useRef(videoId);
  identity.current = videoId;
  const playbackRef = useRef<Playback | null>(null);
  const pending = useRef<Pending | null>(null);
  const pageAvailable = useRef(true);
  const sequence = useRef(0);
  const nativeGeneration = useRef(0);
  const replacements = useRef(Promise.resolve());
  const lastSkip = useRef({ key: '', at: 0 });
  const settingsWrites = useRef(Promise.resolve());
  const categoryGeneration = useRef(0);
  const temporarySpeed = useRef<number | null>(null);
  const retryPlayer = useRef<() => Promise<void>>(async () => {});
  const recoveryTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const player = useVideoPlayer(null, (player) => {
    if (AppState.currentState === 'active') {
      player.staysActiveInBackground = true;
      player.showNowPlayingNotification = true;
    }
    player.timeUpdateEventInterval = 0.25;
    player.audioMixingMode = 'doNotMix';
    player.preservesPitch = true;
  });
  const pagePlayback = useTranscriptPosition(webView, player, videoId, !!videoId && !playback, null);
  const cast = useCastSession({
    player,
    videoId: playback?.videoId ?? null,
    catalog,
    resolve: resolveMedia,
    userAgent,
    height: playback?.qualityHeight || 0,
    fps: playback?.qualityFps || 0,
    audioKey: settings.audioKey,
    onError: setNotice,
  });
  function remoteState() {
    const value = cast.stateRef.current;
    return value.active &&
      value.acknowledged &&
      !value.link &&
      value.videoId === playbackRef.current?.videoId
      ? value
      : null;
  }
  function getPosition() {
    return remoteState()?.position ?? player.currentTime;
  }
  function isPlaying() {
    const remote = remoteState();
    return remote ? remote.phase === 'playing' : player.playing;
  }
  async function play() {
    if (!(await cast.control('play'))) player.play();
  }
  async function pause() {
    if (!(await cast.control('pause'))) player.pause();
  }
  async function replay() {
    if (remoteState()) {
      await cast.control('play');
    } else player.replay();
  }
  useEffect(() => {
    let active = true,
      revision = 0;
    const refresh = async () => {
      const owner = playbackRef.current;
      if (
        !owner ||
        owner.offline ||
        !catalogRef.current?.live ||
        catalogRef.current.videoId !== owner.videoId ||
        identity.current !== owner.videoId ||
        !(player as LiveTrackPlayer).govueSelectVideoTrack
      )
        return;
      const request = ++revision;
      try {
        const choices = await coreRequest<PlaybackQuality[]>({
          op: 'media_live_choices',
          tracks: liveTrackMetadata(player.availableVideoTracks),
        });
        if (active && request === revision && playbackRef.current === owner)
          setQualities(choices);
      } catch {}
    };
    void refresh();
    const loaded = player.addListener('sourceLoad', () => void refresh());
    const changed = player.addListener(
      'videoTrackChange',
      () => void refresh(),
    );
    return () => {
      active = false;
      loaded.remove();
      changed.remove();
    };
  }, [player, playback?.uri, catalog?.live]);
  useEffect(() => {
    if (process.env.EXPO_PUBLIC_GOVUE_DEBUG !== '1') return;
    const timer = setInterval(() => {
      coreRequest({ op: 'media_transport_status' })
        .then((value) =>
          console.info('GoVue transport', JSON.stringify(value)),
        )
        .catch(() => {});
    }, 3000);
    return () => clearInterval(timer);
  }, [playback]);

  const inject = (script: string) => {
    if (pageAvailable.current) webView.current?.injectJavaScript(script);
  };
  function cancelPending(message = 'The video changed. Try again.') {
    if (pending.current) {
      pending.current.controller.abort();
      clearTimeout(pending.current.timer);
      const cancelled = Error(message);
      cancelled.name = 'AbortError';
      pending.current.reject(cancelled);
      pending.current = null;
    }
  }
  function pageTerminated(
    message = 'The browser stopped. Reload the page and try again.',
  ) {
    pageAvailable.current = false;
    autoRetryAt.current = 0;
    setPlayerLayout(null);
    ++qualitySequence.current;
    setSwitchingQuality(false);
  }
  useEffect(() => {
    let active = true;
    mediaSettings()
      .then((value) => {
        if (active) {
          setSettings(value);
          setSettingsReady(true);
        }
      })
      .catch(() => { if (active) setSettingsReady(true); });
    playbackSpeeds()
      .then((value) => {
        if (active) setSpeeds(value);
      })
      .catch(() => {});
    return () => {
      active = false;
      cancelPending();
      void coreRequest({ op: 'extract_release', requestId: 'prepare:media' }).catch(() => {});
    };
  }, []);
  useEffect(() => {
    inject(mediaSettingsScript(settings));
    if (AppState.currentState === 'active' || !settings.backgroundPlayback)
      player.staysActiveInBackground = settings.backgroundPlayback;
    const notification = !remoteState();
    if (
      player.showNowPlayingNotification !== notification &&
      (AppState.currentState === 'active' || !notification)
    )
      player.showNowPlayingNotification = notification;
  }, [settings, player]);
  useEffect(() => {
    player.playbackRate = settings.playbackSpeed;
  }, [settings.playbackSpeed, player]);
  useEffect(() => {
    const subscription = AppState.addEventListener('change', (state) => {
      if (state === 'active') {
        // Retry a service start deferred by Android while this app was hidden.
        player.staysActiveInBackground = settingsRef.current.backgroundPlayback;
        player.showNowPlayingNotification = !remoteState();
        if (identity.current) autoStartNative(identity.current);
      }
      if (
        state === 'background' &&
        !settings.backgroundPlayback &&
        !settings.autoPip
      )
        player.pause();
    });
    return () => subscription.remove();
  }, [settingsReady, settings.backgroundPlayback, settings.adBlock, settings.autoPip, player]);
  useEffect(() => {
    const listener = player.addListener('statusChange', ({ status }) => {
      const current = playbackRef.current;
      if (
        status !== 'error' ||
        !current ||
        current.offline ||
        remoteState() ||
        identity.current !== current.videoId ||
        preparation.current ||
        Date.now() < autoRetryAt.current
      )
        return;
      autoRetryAt.current = Date.now() + 30000;
      recoveryTimer.current = setTimeout(() => {
        if (
          playbackRef.current === current &&
          identity.current === current.videoId
        )
          void retryPlayer.current().catch(() => {});
      }, 750);
    });
    return () => {
      listener.remove();
      if (recoveryTimer.current) clearTimeout(recoveryTimer.current);
    };
  }, [player]);
  useEffect(() => {
    preparation.current = null;
    setPreparing(false);
    manualWeb.current = null;
    autoRetryAt.current = 0;
    cancelPending();
    setCatalog(null);
    setQualities([]);
    setError(null);
    setBusy(false);
    setPlayerLayout(null);
    segments.current = [];
    ++qualitySequence.current;
    setSwitchingQuality(false);
    if (playbackRef.current && playbackRef.current.videoId !== videoId)
      closePlayer(false);
  }, [videoId]);
  useEffect(() => {
    if (videoId) autoStartNative(videoId);
  }, [videoId, settingsReady, settings.backgroundPlayback, settings.adBlock, settings.autoPip]);
  useEffect(() => {
    const controller = new AbortController();
    segments.current = [];
    setSponsorSegments([]);
    sponsorOwner.current = sponsorVideoId;
    setSponsor(emptySponsor);
    if (sponsorVideoId)
      void sponsorControl('segments', { segments: [] }).catch(() => {});
    if (videoId) inject(mediaSponsorsScript(videoId, []));
    if (!sponsorVideoId || !settings.sponsorBlock) {
      setSponsorStatus(
        settings.sponsorBlock
          ? 'Open a YouTube video to load skip segments.'
          : 'SponsorBlock is off.',
      );
      return;
    }
    setSponsorStatus('Loading SponsorBlock segments…');
    const timer = setTimeout(() => controller.abort(), 12000);
    fetchSponsors(sponsorVideoId, controller.signal)
      .then((value) => {
        if (
          controller.signal.aborted ||
          sponsorOwner.current !== sponsorVideoId
        )
          return;
        setSponsorSegments(value);
        segments.current = value.filter((s) =>
          settings.categories.includes(s.category),
        );
        void sponsorControl('segments', { segments: segments.current }).catch(
          () => {},
        );
        if (identity.current === sponsorVideoId)
          inject(mediaSponsorsScript(sponsorVideoId, segments.current));
        setSponsorStatus(
          `${segments.current.length} skip segment${segments.current.length === 1 ? '' : 's'} loaded.`,
        );
      })
      .catch(() => {
        if (sponsorOwner.current === sponsorVideoId)
          setSponsorStatus(
            'SponsorBlock could not connect. Playback continues.',
          );
      })
      .finally(() => clearTimeout(timer));
    return () => {
      controller.abort();
      clearTimeout(timer);
    };
  }, [sponsorVideoId, settings.sponsorBlock, settings.categories]);
  async function sponsorControl(
    action: string,
    values: Record<string, unknown> = {},
  ) {
    const videoId = sponsorOwner.current;
    if (!videoId) return;
    const position = getPosition();
    const work = sponsorWrites.current
      .catch(() => {})
      .then(async () => {
        if (videoId !== sponsorOwner.current) return;
        const result = await coreRequest<SponsorUi>({
          op: 'sponsor_control',
          videoId,
          action,
          position,
          countdown: settingsRef.current.sponsorCountdown,
          playing: isPlaying(),
          ...values,
        });
        if (videoId !== sponsorOwner.current) return;
        setSponsor(result);
        if (
          typeof result.seek === 'number' &&
          playbackRef.current?.videoId === videoId
        ) {
          if (!(await cast.control('seek', { position: result.seek })))
            player.currentTime = result.seek;
          setNotice('Segment skipped');
        }
      });
    sponsorWrites.current = work;
    await work;
  }
  useEffect(() => {
    let sampling = false;
    const sample = () => {
      if (
        sampling ||
        remoteState() ||
        !settingsRef.current.sponsorBlock ||
        playbackRef.current?.videoId !== sponsorOwner.current
      )
        return;
      sampling = true;
      void sponsorControl('tick')
        .catch(() => {})
        .finally(() => {
          sampling = false;
        });
    };
    const listener = player.addListener('timeUpdate', sample);
    return () => listener.remove();
  }, [player]);
  useEffect(() => {
    if (!notice) return;
    const timer = setTimeout(() => setNotice(null), 4000);
    return () => clearTimeout(timer);
  }, [notice]);

  function configurePage() {
    pageAvailable.current = true;
    inject(mediaSettingsScript(settings));
    if (identity.current)
      inject(mediaSponsorsScript(identity.current, segments.current));
    if (identity.current && (playbackRef.current || preparation.current))
      inject(mediaNativeScript(identity.current, true));
  }
  function refreshPlayerLayout() {
    const owner = playbackRef.current;
    if (owner && !owner.offline && owner.videoId === identity.current)
      inject(mediaNativeScript(owner.videoId, true));
  }
  function updateSettings(changes: Partial<MediaSettings>) {
    if (changes.categories !== undefined || changes.sponsorBlock !== undefined)
      ++categoryGeneration.current;
    const next = { ...settingsRef.current, ...changes };
    settingsRef.current = next;
    setSettings(next);
    settingsWrites.current = settingsWrites.current
      .catch(() => {})
      .then(async () => {
        await mediaSettings(changes);
      })
      .catch(() => {
        void mediaSettings().then((value) => {
          settingsRef.current = value;
          setSettings(value);
        });
        Alert.alert('Could not save playback settings');
      });
  }
  function updateSponsorCategory(id: string, enabled: boolean) {
    const generation = categoryGeneration.current;
    settingsWrites.current = settingsWrites.current
      .catch(() => {})
      .then(async () => {
        const saved = await mediaSponsorCategory(id, enabled);
        if (generation !== categoryGeneration.current) return;
        // Other controls may have optimistic edits waiting in this same queue.
        const next = {
          ...settingsRef.current,
          categories: saved.categories,
          sponsorBlock: saved.sponsorBlock,
        };
        settingsRef.current = next;
        setSettings(next);
      })
      .catch(() => Alert.alert('Could not save SponsorBlock settings'));
  }
  async function setSpeed(speed: number) {
    if (!speeds.includes(speed)) return;
    const owner = remoteState();
    try {
      if (owner) {
        await cast.control('speed', { speed });
        const current = remoteState();
        if (current?.id !== owner.id) return;
        speed = current.speed ?? speed;
      }
      player.playbackRate = speed;
      updateSettings({ playbackSpeed: speed });
    } catch (error) {
      setNotice(
        error instanceof Error ? error.message : 'Could not change Cast speed.',
      );
    }
  }
  useEffect(() => {
    const subscription = player.addListener(
      'playbackRateChange',
      ({ playbackRate }) => {
        if (temporarySpeed.current !== null) return;
        if (
          speeds.includes(playbackRate) &&
          playbackRate !== settingsRef.current.playbackSpeed
        )
          updateSettings({ playbackSpeed: playbackRate });
      },
    );
    return () => subscription.remove();
  }, [player, speeds]);
  function resolveMedia() {
    const id = identity.current;
    if (!id) return Promise.reject(Error('Open a YouTube video first.'));
    cancelPending();
    setBusy(true);
    setError(null);
    setCatalog(null);
    return new Promise<MediaCatalog>((resolve, reject) => {
      const requestId = 'media:' + Date.now() + ':' + ++sequence.current;
      const controller = new AbortController();
      const timer = setTimeout(() => {
        if (pending.current?.requestId !== requestId) return;
        controller.abort(); pending.current = null; setBusy(false);
        const error = Error('YouTube extraction timed out. Try again.');
        setError(error.message); reject(error);
      }, 95000);
      const request = { videoId: id, requestId, timer, controller, resolve, reject };
      pending.current = request;
      void extractMedia(id, controller.signal).then(async data => {
        const choices = await playbackChoices(data).catch(() => []);
        if (pending.current !== request || identity.current !== id) return;
        clearTimeout(timer); pending.current = null; setBusy(false);
        catalogRef.current = data; setCatalog(data); setQualities(choices);
        request.resolve(data);
      }).catch(error => {
        if (pending.current !== request) return;
        clearTimeout(timer); pending.current = null; setBusy(false);
        const failure = error instanceof Error ? error : Error('Could not extract media.');
        setError(failure.message); request.reject(failure);
      });
    });
  }
  function autoStartNative(owner: string) {
    if (
      !settingsReady ||
      identity.current !== owner ||
      manualWeb.current === owner ||
      playbackRef.current ||
      preparation.current ||
      pending.current ||
      Date.now() < autoRetryAt.current ||
      AppState.currentState !== 'active' ||
      !(settings.backgroundPlayback || settings.adBlock || settings.autoPip)
    ) return;
    autoRetryAt.current = Date.now() + 30000;
    void startNative().catch((failure) => {
      if (failure instanceof Error && failure.name === 'AbortError') return;
      if (pageAvailable.current && identity.current === owner)
        setError(failure instanceof Error ? failure.message : 'Video could not start.');
    });
  }
  async function handleMessage(
    raw: string,
    origin: string,
    message: {
      type?: string;
      videoId?: string;
      requestId?: string;
      category?: string;
      evalId?: string;
      code?: string;
      layout?: unknown;
    },
  ) {
    const owner = identity.current;
    if (!owner || message.videoId !== owner) return;
    const trusted = await coreRequest<boolean>({
      op: 'media_message_origin',
      origin,
      videoId: owner,
    }).catch(() => false);
    if (!trusted || identity.current !== owner) return;
    if (message.type === 'playbackPosition') {
      pagePlayback.handlePosition(message, origin);
      return;
    }
    if (message.type === 'playerLayout') {
      if (
        message.videoId !== identity.current ||
        !(playbackRef.current || preparation.current)
      )
        return;
      const layout = acceptPlayerLayout(message.layout);
      if (layout) setPlayerLayout(layout);
      return;
    }
    if (message.type === 'mediaReady') {
      if (
        message.videoId === identity.current &&
        (playbackRef.current || preparation.current)
      )
        inject(mediaNativeScript(message.videoId, true));
      autoStartNative(owner);
      return;
    }
    if (
      message.type === 'sponsorSkipped' &&
      message.videoId === identity.current &&
      typeof message.category === 'string'
    ) {
      setNotice(`Skipped ${message.category.replaceAll('_', ' ')}`);
      return;
    }
  }
  async function openPlayer(
    data: Playback,
    source: VideoSource,
    position: number | null,
    resume = true,
    stillCurrent: () => boolean = () => true,
    keepCast = false,
  ) {
    const generation = ++nativeGeneration.current;
    // Keep replacements ordered so an old asset cannot overwrite a newer one.
    replacements.current = replacements.current
      .catch(() => {})
      .then(async () => {
        if (generation !== nativeGeneration.current || !stillCurrent()) return;
        if (!keepCast) await cast.stop(false);
        if (generation !== nativeGeneration.current || !stillCurrent()) return;
        await player.replaceAsync(source);
        if (generation !== nativeGeneration.current || !stillCurrent()) return;
        const previous = playbackRef.current;
        if (previous && !previous.offline)
          void coreRequest({
            op: 'media_proxy_release',
            uri: previous.uri,
          }).catch(() => {});
        if (position !== null) player.currentTime = Math.max(0, position);
        player.playbackRate = settingsRef.current.playbackSpeed;
        if (resume) player.play();
        else player.pause();
        playbackRef.current = data;
        setPlayback(data);
        setError(null);
        if (identity.current) inject(mediaNativeScript(identity.current, true));
      });
    await replacements.current;
  }
  async function startNative() {
    const id = identity.current;
    if (!id) throw Error('Open a YouTube video first.');
    if (preparation.current) return;
    const attempt = { videoId: id };
    const previous =
      playbackRef.current?.videoId === id ? playbackRef.current : null;
    const previousPosition = previous ? player.currentTime : 0;
    const pagePosition = pagePlayback.position;
    const previousPlaying = previous
      ? player.status === 'error' || player.playing
      : true;
    const titleTask = settingsRef.current.useOriginalTitle
      ? (async () => {
          const controller = new AbortController();
          const timer = setTimeout(() => controller.abort(), 5000);
          try {
            const url = await coreRequest<string>({
              op: 'media_title_request',
              videoId: id,
            });
            const response = await fetch(url, { signal: controller.signal });
            if (!response.ok) return null;
            return await coreRequest<string>({
              op: 'media_title_accept',
              videoId: id,
              raw: await response.text(),
            });
          } catch {
            return null;
          } finally {
            clearTimeout(timer);
          }
        })()
      : Promise.resolve(null);
    preparation.current = attempt;
    setPreparing(true);
    manualWeb.current = null;
    inject(mediaNativeScript(id, true));
    try {
      const data = await resolveMedia();
      const originalTitle = await titleTask;
      if (originalTitle) data.title = originalTitle;
      if (preparation.current !== attempt) return;
      if (identity.current !== data.videoId) return;
      if (catalogRef.current?.videoId === data.videoId)
        data.chapters = catalogRef.current.chapters;
      catalogRef.current = data;
      setCatalog({ ...data });
      const preferences = settingsRef.current;
      if (!previous && !preferences.rememberSpeed)
        updateSettings({ playbackSpeed: 1 });
      if (!previous && !preferences.rememberResize)
        updateSettings({ resizeMode: 'fit' });
      const prepared = await preparePlayback(
        data,
        previous
          ? previous.qualityHeight
          : preferences.rememberQuality
            ? preferences.qualityHeight
            : 0,
        userAgent,
        true,
        preferences.audioKey,
        previous?.qualityFps || preferences.qualityFps,
      );
      const position =
        previous && !data.live
          ? previousPosition
          : await coreRequest<number | null>({
              op: 'player_resume',
              videoId: data.videoId,
              position: pagePosition ?? data.position,
              duration: data.duration,
              live: data.live,
            });
      const { uri } = prepared;
      if (
        identity.current !== data.videoId ||
        preparation.current !== attempt
      ) {
        await coreRequest({ op: 'media_proxy_release', uri });
        return;
      }
      try {
        await openPlayer(
          {
            videoId: data.videoId,
            title: data.title,
            author: data.author,
            offline: false,
            uri,
            qualityHeight: prepared.qualityHeight,
            qualityFps: prepared.qualityFps || 0,
          },
          {
            uri,
            contentType: prepared.contentType,
            headers: prepared.headers,
            metadata: { title: data.title, artist: data.author },
          },
          position,
          previousPlaying,
          () =>
            preparation.current === attempt &&
            identity.current === data.videoId,
        );
      } finally {
        if (playbackRef.current?.uri !== uri)
          await coreRequest({ op: 'media_proxy_release', uri }).catch(() => {});
      }
    } catch (failure) {
      if (identity.current === id && preparation.current === attempt)
        inject(mediaNativeScript(id, false));
      throw failure;
    } finally {
      if (preparation.current === attempt) {
        preparation.current = null;
        setPreparing(false);
      }
    }
  }
  retryPlayer.current = startNative;
  async function playOffline(
    data: { videoId: string; title: string; author: string },
    uri: string,
  ) {
    preparation.current = null;
    setPreparing(false);
    cancelPending();
    manualWeb.current = identity.current;
    catalogRef.current = null;
    setCatalog(null);
    setQualities([]);
    ++qualitySequence.current;
    setSwitchingQuality(false);
    await openPlayer(
      { ...data, offline: true, uri, qualityHeight: 0 },
      { uri, metadata: { title: data.title, artist: data.author } },
      0,
    );
  }
  async function setQuality(
    height: number,
    fps = 0,
    audioKey = settingsRef.current.audioKey,
  ) {
    const current = playbackRef.current;
    const data = catalogRef.current;
    if (
      !current ||
      current.offline ||
      !data ||
      data.videoId !== current.videoId ||
      identity.current !== data.videoId
    )
      return;
    const request = ++qualitySequence.current;
    setSwitchingQuality(true);
    let uri: string | undefined;
    try {
      const selectLive = (player as LiveTrackPlayer).govueSelectVideoTrack;
      if (data.live && selectLive) {
        const tracks = player.availableVideoTracks;
        const selected = await coreRequest<{
          height: number;
          fps: number;
          index: number;
        }>({
          op: 'media_live_select',
          height,
          fps,
          tracks: liveTrackMetadata(tracks),
        });
        if (
          request !== qualitySequence.current ||
          playbackRef.current !== current ||
          identity.current !== current.videoId
        )
          return;
        const id = selected.index < 0 ? null : tracks[selected.index]?.id;
        if (id === undefined)
          throw Error('This live resolution is unavailable. Try Auto.');
        const applied = await selectLive.call(player, current.uri, id);
        if (!applied)
          throw Error('The live video changed. Try the resolution again.');
        if (
          request === qualitySequence.current &&
          playbackRef.current === current
        ) {
          const next = {
            ...current,
            qualityHeight: selected.height,
            qualityFps: selected.fps,
          };
          playbackRef.current = next;
          setPlayback(next);
          updateSettings({
            qualityHeight: selected.height,
            qualityFps: selected.fps,
          });
        }
        return;
      }
      const prepared = await preparePlayback(
        data,
        height,
        userAgent,
        false,
        audioKey,
        fps,
      );
      uri = prepared.uri;
      const stillCurrent = () =>
        request === qualitySequence.current &&
        playbackRef.current?.videoId === current.videoId &&
        identity.current === current.videoId;
      if (!stillCurrent()) return;
      const remote = remoteState();
      const castId = remote?.id;
      const position = remote?.position ?? player.currentTime,
        resume = !remote && player.playing;
      await openPlayer(
        {
          ...current,
          uri,
          qualityHeight: height,
          qualityFps: prepared.qualityFps || 0,
        },
        {
          uri,
          contentType: prepared.contentType,
          headers: prepared.headers,
          metadata: { title: current.title, artist: current.author },
        },
        position,
        resume,
        stillCurrent,
        !!castId,
      );
      if (stillCurrent() && castId && cast.stateRef.current.id === castId)
        await cast.replace(data, height, fps, audioKey);
      if (stillCurrent())
        updateSettings({ qualityHeight: height, qualityFps: fps, audioKey });
    } finally {
      if (uri && playbackRef.current?.uri !== uri)
        await coreRequest({ op: 'media_proxy_release', uri }).catch(() => {});
      if (qualitySequence.current === request) setSwitchingQuality(false);
    }
  }
  function closePlayer(resumeWeb = true, stayClosed = false) {
    preparation.current = null;
    setPreparing(false);
    cancelPending();
    setBusy(false);
    setError(null);
    if (resumeWeb || stayClosed) manualWeb.current = identity.current;
    ++nativeGeneration.current;
    ++qualitySequence.current;
    setSwitchingQuality(false);
    const current = playbackRef.current;
    const position = getPosition();
    void cast.stop(false).catch(() => {});
    player.pause();
    playbackRef.current = null;
    setPlayback(null);
    if (current && !current.offline)
      void coreRequest({ op: 'media_proxy_release', uri: current.uri }).catch(
        () => {},
      );
    replacements.current = replacements.current
      .catch(() => {})
      .then(() => player.replaceAsync(null));
    if (identity.current) {
      inject(mediaNativeScript(identity.current, false));
      if (resumeWeb)
        inject(
          seekScript(
            identity.current,
            current ? position : catalog?.position || 0,
          ),
        );
    }
  }
  function seek(seconds: number) {
    if (
      !playbackRef.current ||
      playbackRef.current.videoId !== identity.current
    )
      return false;
    void seekTo(seconds).then(play);
    return true;
  }
  async function seekTo(seconds: number) {
    const videoId = playbackRef.current?.videoId;
    await sponsorControl('seek', { position: seconds });
    if (
      videoId === playbackRef.current?.videoId &&
      !(await cast.control('seek', { position: seconds }))
    )
      player.currentTime = seconds;
  }
  async function seekLive() {
    const id = playbackRef.current?.videoId;
    if (!id || !catalogRef.current?.live) return;
    const delta = await coreRequest<number>({
      op: 'player_live_seek',
      offset: player.currentOffsetFromLive,
      target: player.targetOffsetFromLive,
    });
    if (id !== playbackRef.current?.videoId) return;
    player.seekBy(delta);
    player.play();
  }
  const setAudio = (key: string) =>
    setQuality(
      playbackRef.current?.qualityHeight || 0,
      playbackRef.current?.qualityFps || 0,
      key,
    );
  function holdSpeed(enabled: boolean) {
    if (enabled) {
      if (temporarySpeed.current === null)
        temporarySpeed.current = remoteState()?.speed ?? player.playbackRate;
      player.playbackRate = 2;
      void cast.control('speed', { speed: 2 }).catch(() => {});
    } else if (temporarySpeed.current !== null) {
      player.playbackRate = temporarySpeed.current;
      void cast
        .control('speed', { speed: temporarySpeed.current })
        .catch(() => {});
      setTimeout(() => {
        temporarySpeed.current = null;
      }, 100);
    }
  }
  return {
    settings,
    updateSettings,
    updateSponsorCategory,
    playback,
    preparing,
    playerLayout,
    refreshPlayerLayout,
    player,
    cast,
    getPosition,
    isPlaying,
    play,
    pause,
    replay,
    catalog,
    qualities,
    speeds,
    sponsorSegments,
    sponsor,
    sponsorControl,
    switchingQuality,
    setSpeed,
    holdSpeed,
    setQuality,
    setAudio,
    busy: busy || preparing,
    error,
    sponsorStatus,
    notice,
    showNotice: setNotice,
    configurePage,
    resolveMedia,
    handleMessage,
    startNative,
    playOffline,
    closePlayer,
    seek,
    seekTo,
    seekLive,
    pageTerminated,
  };
}
