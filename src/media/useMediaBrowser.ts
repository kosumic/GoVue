import { useEffect, useRef, useState } from 'react';
import { Alert, AppState } from 'react-native';
import { useVideoPlayer, type VideoSource } from 'expo-video';
import type WebView from 'react-native-webview';
import { acceptMedia, defaultMediaSettings, fetchSponsors, mediaSettings, type MediaCatalog, type MediaSettings, type SponsorSegment } from '../core/media';
import { mediaNativeScript, mediaResolveScript, mediaSettingsScript, mediaSponsorsScript } from '../youtube/mediaBridge';
import { seekScript } from '../youtube/pageBridge';
import { coreRequest } from '../../modules/lingoplay-core';
import type { PlayerRuntimeHandle } from './PlayerRuntime';
import { acceptPlayerLayout, type PlayerLayout } from './playerLayout';

type Playback = { videoId: string; title: string; author: string; offline: boolean; uri: string };
type Pending = { requestId: string; videoId: string; timer: ReturnType<typeof setTimeout>; resolve: (value: MediaCatalog) => void; reject: (error: Error) => void };

export function useMediaBrowser(webView: React.RefObject<WebView | null>, videoId: string | null, userAgent: string, runtime: React.RefObject<PlayerRuntimeHandle | null>) {
  const [settings, setSettings] = useState(defaultMediaSettings);
  const [playback, setPlayback] = useState<Playback | null>(null);
  const [catalog, setCatalog] = useState<MediaCatalog | null>(null);
  const [busy, setBusy] = useState(false);
  const [preparing, setPreparing] = useState(false);
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
  const identity = useRef(videoId); identity.current = videoId;
  const playbackRef = useRef<Playback | null>(null);
  const pending = useRef<Pending | null>(null);
  const sequence = useRef(0);
  const nativeGeneration = useRef(0);
  const replacements = useRef(Promise.resolve());
  const lastSkip = useRef({ key: '', at: 0 });
  const settingsWrites = useRef(Promise.resolve());
  const player = useVideoPlayer(null, player => {
    player.staysActiveInBackground = true;
    player.showNowPlayingNotification = true;
    player.timeUpdateEventInterval = 0.25;
    player.audioMixingMode = 'doNotMix';
  });
  useEffect(() => {
    if (process.env.EXPO_PUBLIC_LINGOPLAY_DEBUG !== '1') return;
    const timer = setInterval(() => { coreRequest({ op: 'media_transport_status' }).then(value => console.info('LingoPlay transport', JSON.stringify(value))).catch(() => {}); }, 3000);
    return () => clearInterval(timer);
  }, [playback]);

  const inject = (script: string) => webView.current?.injectJavaScript(script);
  function cancelPending() {
    if (pending.current) { clearTimeout(pending.current.timer); pending.current.reject(Error('The video changed. Try again.')); pending.current = null; }
  }
  useEffect(() => {
    let active = true;
    mediaSettings().then(value => { if (active) setSettings(value); }).catch(() => {});
    return () => { active = false; cancelPending(); };
  }, []);
  useEffect(() => {
    inject(mediaSettingsScript(settings));
    player.staysActiveInBackground = settings.backgroundPlayback;
    player.showNowPlayingNotification = true;
  }, [settings, player]);
  useEffect(() => {
    const subscription = AppState.addEventListener('change', state => {
      if (state === 'background' && !settings.backgroundPlayback && !settings.autoPip) player.pause();
    });
    return () => subscription.remove();
  }, [settings.backgroundPlayback, settings.autoPip, player]);
  useEffect(() => {
    preparation.current = null; setPreparing(false); manualWeb.current = null; autoRetryAt.current = 0;
    cancelPending(); setCatalog(null); setError(null); setBusy(false); setPlayerLayout(null); segments.current = [];
    if (playbackRef.current && playbackRef.current.videoId !== videoId) closePlayer(false);
  }, [videoId]);
  useEffect(() => {
    const controller = new AbortController();
    segments.current = [];
    sponsorOwner.current = sponsorVideoId;
    if (videoId) inject(mediaSponsorsScript(videoId, []));
    if (!sponsorVideoId || !settings.sponsorBlock) { setSponsorStatus(settings.sponsorBlock ? 'Open a YouTube video to load skip segments.' : 'SponsorBlock is off.'); return; }
    setSponsorStatus('Loading SponsorBlock segments…');
    const timer = setTimeout(() => controller.abort(), 12000);
    fetchSponsors(sponsorVideoId, controller.signal).then(value => {
      if (controller.signal.aborted || sponsorOwner.current !== sponsorVideoId) return;
      segments.current = value.filter(s => settings.categories.includes(s.category));
      if (identity.current === sponsorVideoId) inject(mediaSponsorsScript(sponsorVideoId, segments.current));
      setSponsorStatus(`${segments.current.length} skip segment${segments.current.length === 1 ? '' : 's'} loaded.`);
    }).catch(() => {
      if (sponsorOwner.current === sponsorVideoId) setSponsorStatus('SponsorBlock could not connect. Playback continues.');
    }).finally(() => clearTimeout(timer));
    return () => { controller.abort(); clearTimeout(timer); };
  }, [sponsorVideoId, settings.sponsorBlock, settings.categories]);
  useEffect(() => {
    const listener = player.addListener('timeUpdate', ({ currentTime }) => {
      const current = playbackRef.current;
      if (!current || current.videoId !== sponsorOwner.current || !settings.sponsorBlock || !player.playing) return;
      const segment = segments.current.find(s => currentTime >= s.start && currentTime < s.end - 0.05);
      if (!segment) return;
      const key = `${current.videoId}:${segment.start}:${segment.end}`;
      if (lastSkip.current.key === key && Date.now() - lastSkip.current.at < 1500) return;
      lastSkip.current = { key, at: Date.now() };
      player.currentTime = segment.end;
      setNotice(`Skipped ${segment.category.replaceAll('_', ' ')}`);
    });
    return () => listener.remove();
  }, [player, settings.sponsorBlock]);
  useEffect(() => {
    if (!notice) return;
    const timer = setTimeout(() => setNotice(null), 4000);
    return () => clearTimeout(timer);
  }, [notice]);

  function configurePage() {
    inject(mediaSettingsScript(settings));
    if (identity.current) inject(mediaSponsorsScript(identity.current, segments.current));
    if (identity.current && (playbackRef.current || preparation.current)) inject(mediaNativeScript(identity.current, true));
  }
  function updateSettings(changes: Partial<MediaSettings>) {
    const next = { ...settings, ...changes }; setSettings(next);
    settingsWrites.current = settingsWrites.current.catch(() => {}).then(async () => { await mediaSettings(next); }).catch(() => Alert.alert('Could not save playback settings'));
  }
  function resolveMedia() {
    const id = identity.current;
    if (!id) return Promise.reject(Error('Open a YouTube video first.'));
    cancelPending(); setBusy(true); setError(null); setCatalog(null);
    return new Promise<MediaCatalog>((resolve, reject) => {
      const requestId = `media:${Date.now()}:${++sequence.current}`;
      const timer = setTimeout(() => {
        if (pending.current?.requestId !== requestId) return;
        pending.current = null; setBusy(false);
        const error = Error('Media request timed out. Play the video and try again.'); setError(error.message); reject(error);
      }, 45000);
      pending.current = { videoId: id, requestId, timer, resolve, reject };
      inject(mediaResolveScript(requestId));
    });
  }
  async function handleMessage(raw: string, origin: string, message: { type?: string; videoId?: string; requestId?: string; category?: string; evalId?: string; code?: string; layout?: unknown }) {
    if (message.type === 'playerLayout') {
      if (message.videoId !== identity.current || !(playbackRef.current || preparation.current)) return;
      const layout = acceptPlayerLayout(message.layout);
      if (layout) setPlayerLayout(layout);
      return;
    }
    if (message.type === 'mediaReady') {
      if (message.videoId === identity.current && manualWeb.current !== identity.current && !playbackRef.current && !preparation.current && !pending.current &&
          Date.now() >= autoRetryAt.current && AppState.currentState === 'active' && (settings.backgroundPlayback || settings.adBlock || settings.autoPip)) {
        autoRetryAt.current = Date.now() + 30000;
        void startNative().catch(failure => { if (identity.current === message.videoId) setError(failure instanceof Error ? failure.message : 'Video could not start.'); });
      }
      return;
    }
    if (message.type === 'mediaEval') {
      const request = pending.current;
      if (!request || message.requestId !== request.requestId || message.videoId !== request.videoId || identity.current !== request.videoId ||
          typeof message.code !== 'string' || message.code.length > 2_000_000 || typeof message.evalId !== 'string' || !/^\d{1,12}$/.test(message.evalId)) return;
      let values: Record<string, string> | null = null;
      try { values = await runtime.current?.evaluate(message.code) ?? null; } catch {}
      if (pending.current === request && identity.current === request.videoId) inject(`window.__lingoplayEvalReply?.(${JSON.stringify(message.evalId)},${JSON.stringify(values)}); true;`);
      return;
    }
    if (message.type === 'sponsorSkipped' && message.videoId === identity.current && typeof message.category === 'string') {
      setNotice(`Skipped ${message.category.replaceAll('_', ' ')}`); return;
    }
    const request = pending.current;
    if (message.type !== 'media' || !request || message.requestId !== request.requestId || message.videoId !== request.videoId || identity.current !== request.videoId) return;
    try {
      const data = await acceptMedia(raw, origin, request.requestId, request.videoId);
      if (pending.current !== request || identity.current !== request.videoId) return;
      clearTimeout(request.timer); pending.current = null; setBusy(false); setCatalog(data); request.resolve(data);
    } catch (error) {
      if (pending.current !== request) return;
      clearTimeout(request.timer); pending.current = null; setBusy(false);
      const failure = error instanceof Error ? error : Error('Could not load media.'); setError(failure.message); request.reject(failure);
    }
  }
  async function openPlayer(data: Playback, source: VideoSource, position: number) {
    const generation = ++nativeGeneration.current;
    // Keep replacements ordered so an old asset cannot overwrite a newer one.
    replacements.current = replacements.current.catch(() => {}).then(async () => {
      if (generation !== nativeGeneration.current) return;
      await player.replaceAsync(source);
      if (generation !== nativeGeneration.current) return;
      const previous = playbackRef.current;
      if (previous && !previous.offline) void coreRequest({ op: 'media_proxy_release', uri: previous.uri }).catch(() => {});
      player.currentTime = Math.max(0, position); player.play();
      playbackRef.current = data; setPlayback(data); setError(null);
      if (identity.current) inject(mediaNativeScript(identity.current, true));
    });
    await replacements.current;
  }
  async function startNative() {
    const id = identity.current;
    if (!id) throw Error('Open a YouTube video first.');
    if (preparation.current) return;
    const attempt = { videoId: id };
    preparation.current = attempt; setPreparing(true); manualWeb.current = null;
    inject(mediaNativeScript(id, true));
    try {
    const data = await resolveMedia();
    if (preparation.current !== attempt) return;
    if (identity.current !== data.videoId) return;
    const stream = data.streams.filter(s => s.kind === 'combined').sort((a, b) => b.height - a.height)[0];
    if (!stream || !stream.bytes) throw Error('No combined video stream is available yet. Play the video and try again.');
    const uri = await coreRequest<string>({ op: 'media_proxy', url: stream.url, length: stream.bytes, userAgent: stream.userAgent || userAgent, origin: data.origin });
    if (identity.current !== data.videoId) { await coreRequest({ op: 'media_proxy_release', uri }); return; }
    try {
      await openPlayer({ videoId: data.videoId, title: data.title, author: data.author, offline: false, uri }, {
        uri, contentType: 'progressive', metadata: { title: data.title, artist: data.author },
      }, data.position);
    } finally {
      if (playbackRef.current?.uri !== uri) await coreRequest({ op: 'media_proxy_release', uri }).catch(() => {});
    }
    } catch (failure) {
      if (identity.current === id && preparation.current === attempt) inject(mediaNativeScript(id, false));
      throw failure;
    } finally {
      if (preparation.current === attempt) { preparation.current = null; setPreparing(false); }
    }
  }
  async function playOffline(data: { videoId: string; title: string; author: string }, uri: string) {
    preparation.current = null; setPreparing(false); cancelPending(); manualWeb.current = identity.current;
    await openPlayer({ ...data, offline: true, uri }, { uri, metadata: { title: data.title, artist: data.author } }, 0);
  }
  function closePlayer(resumeWeb = true) {
    preparation.current = null; setPreparing(false); cancelPending(); setBusy(false); setError(null);
    if (resumeWeb) manualWeb.current = identity.current;
    ++nativeGeneration.current;
    const current = playbackRef.current;
    const position = player.currentTime;
    player.pause(); playbackRef.current = null; setPlayback(null);
    if (current && !current.offline) void coreRequest({ op: 'media_proxy_release', uri: current.uri }).catch(() => {});
    replacements.current = replacements.current.catch(() => {}).then(() => player.replaceAsync(null));
    if (identity.current) {
      inject(mediaNativeScript(identity.current, false));
      if (resumeWeb) inject(seekScript(identity.current, current ? position : catalog?.position || 0));
    }
  }
  function seek(seconds: number) {
    if (!playbackRef.current || playbackRef.current.videoId !== identity.current) return false;
    player.currentTime = seconds; player.play(); return true;
  }
  return { settings, updateSettings, playback, preparing, playerLayout, player, catalog, busy: busy || preparing, error, sponsorStatus, notice, configurePage, resolveMedia, handleMessage, startNative, playOffline, closePlayer, seek };
}
