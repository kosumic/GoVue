import { useCallback, useEffect, useRef, useState } from 'react';
import { ActivityIndicator, Alert, BackHandler, Keyboard, KeyboardAvoidingView, Platform, Pressable, StyleSheet, Text, TextInput, View } from 'react-native';
import { SafeAreaProvider, SafeAreaView, useSafeAreaInsets } from 'react-native-safe-area-context';
import { StatusBar } from 'expo-status-bar';
import WebView, { type WebViewMessageEvent } from 'react-native-webview';
import { ArrowLeft, ArrowRight, Captions, Download, FolderDown, Globe2, House, PictureInPicture2, Play, RotateCw, Settings2, Sparkles, X } from 'lucide-react-native';
import { useEvent } from 'expo';
import { VideoView } from 'expo-video';
import { browserAddress, isWebUrl, isYouTubeUrl, youtubeVideoId } from './src/browser/address';
import { IconButton } from './src/components/IconButton';
import { StartPage } from './src/components/StartPage';
import { TranscriptPanel } from './src/components/TranscriptPanel';
import { colors } from './src/theme';
import { extractionScript, seekScript, YOUTUBE_BRIDGE } from './src/youtube/pageBridge';
import { type Transcript } from './src/youtube/transcript';
import { acceptTranscript, restoreTranscript } from './src/core/transcripts';
import { MediaSheet, type MediaSheetMode } from './src/components/MediaSheet';
import { useDownloads } from './src/media/downloads';
import { useMediaBrowser } from './src/media/useMediaBrowser';
import { mediaSettingsScript } from './src/youtube/mediaBridge';
import { PLAYER_DECODER } from './src/youtube/generated/playerDecoder';
import { PlayerRuntime, type PlayerRuntimeHandle } from './src/media/PlayerRuntime';
import { embeddedPlayerFrame } from './src/media/playerLayout';
import { useTranscriptPosition } from './src/media/useTranscriptPosition';
import { AiSheet } from './src/components/AiSheet';

type PendingRequest = { id: string; videoId: string; timer: ReturnType<typeof setTimeout> };

function Browser() {
  const insets = useSafeAreaInsets();
  const webView = useRef<WebView>(null);
  const [pageAgent, setPageAgent] = useState('');
  const addressInput = useRef<TextInput>(null);
  const [source, setSource] = useState<{ uri: string } | null>(null);
  const [home, setHome] = useState(true);
  const [url, setUrl] = useState('');
  const nativeVideo = useRef<VideoView>(null);
  const currentUrl = useRef('');
  const currentVideo = useRef<string | null>(null);
  const [address, setAddress] = useState('');
  const [editing, setEditing] = useState(false);
  const [navigation, setNavigation] = useState({ back: false, forward: false });
  const [loading, setLoading] = useState(false);
  const [progress, setProgress] = useState(0);
  const [browserError, setBrowserError] = useState<string | null>(null);
  const [panel, setPanel] = useState(false);
  const [viewport, setViewport] = useState({ width: 0, height: 0 });
  const [transcript, setTranscript] = useState<Transcript | null>(null);
  const [cachedTranscript, setCachedTranscript] = useState(false);
  const [extracting, setExtracting] = useState(false);
  const [transcriptError, setTranscriptError] = useState<string | null>(null);
  const pending = useRef<PendingRequest | null>(null);
  const sequence = useRef(0);
  const [mediaSheet, setMediaSheet] = useState<MediaSheetMode>(null);
  const [aiVisible, setAiVisible] = useState(false);
  const downloads = useDownloads();
  const playerRuntime = useRef<PlayerRuntimeHandle>(null);
  const media = useMediaBrowser(webView, youtubeVideoId(url), pageAgent, playerRuntime);
  const transcriptPlayback = useTranscriptPosition(webView, media.player, youtubeVideoId(url), panel && !home && !!transcript, media.playback?.videoId ?? null);
  const playerState = useEvent(media.player, 'statusChange', { status: media.player.status, error: undefined });
  const playerStatus = playerState?.status ?? media.player.status;
  const playerError = playerState?.error;
  const mediaAction = (work: () => Promise<unknown>) => { work().catch(error => Alert.alert('LingoPlay', error instanceof Error ? error.message : 'Please try again.')); };

  const cancelRequest = useCallback(() => {
    if (pending.current) clearTimeout(pending.current.timer);
    pending.current = null;
    webView.current?.injectJavaScript('window.__lingoplay?.cancel(); true;');
    setExtracting(false);
  }, []);

  const updateUrl = useCallback((next: string) => {
    if (!isWebUrl(next)) return;
    const video = youtubeVideoId(next);
    if (video !== currentVideo.current) {
      cancelRequest();
      setTranscript(null);
      setTranscriptError(null);
      setPanel(false);
    }
    currentUrl.current = next;
    currentVideo.current = video;
    setUrl(next);
  }, [cancelRequest]);

  useEffect(() => () => { if (pending.current) clearTimeout(pending.current.timer); }, []);

  const closePanel = useCallback(() => { setPanel(false); cancelRequest(); }, [cancelRequest]);
  const showHome = useCallback(() => {
    closePanel(); setHome(true); setBrowserError(null); Keyboard.dismiss();
  }, [closePanel]);

  const goBack = useCallback(() => {
    if (panel) { closePanel(); return true; }
    if (home) return false;
    if (navigation.back) webView.current?.goBack();
    else showHome();
    return true;
  }, [panel, home, navigation.back, closePanel, showHome]);

  useEffect(() => {
    const subscription = BackHandler.addEventListener('hardwareBackPress', goBack);
    return () => subscription.remove();
  }, [goBack]);

  function navigate(input: string) {
    try {
      const next = browserAddress(input);
      cancelRequest(); setPanel(false); setTranscript(null); setTranscriptError(null);
      updateUrl(next); setAddress(next); setBrowserError(null); setHome(false); setEditing(false);
      Keyboard.dismiss();
      if (!source) setSource({ uri: next });
      else webView.current?.injectJavaScript(`window.location.assign(${JSON.stringify(next)}); true;`);
    } catch (error) {
      setBrowserError(error instanceof Error ? error.message : 'Could not open this address.');
    }
  }

  async function requestTranscript(track = 'default', preferCache = false) {
    const videoId = currentVideo.current;
    if (!videoId || home) return;
    cancelRequest();
    setPanel(true); setExtracting(true); setTranscriptError(null); setCachedTranscript(false);
    Keyboard.dismiss();
    const id = `${Date.now()}:${++sequence.current}`;
    const timer = setTimeout(() => {
      if (pending.current?.id !== id) return;
      cancelRequest();
      setTranscriptError('Transcript request timed out. Play the video or check your connection, then try again.');
    }, 45000);
    pending.current = { id, videoId, timer };
    if (preferCache) {
      try {
        const cached = await restoreTranscript(videoId);
        if (pending.current?.id !== id || currentVideo.current !== videoId) return;
        if (cached) {
          clearTimeout(timer); pending.current = null;
          setTranscript(cached); setCachedTranscript(true); setExtracting(false);
          return;
        }
      } catch { /* A missing cache must not block a fresh YouTube request. */ }
      if (pending.current?.id !== id || currentVideo.current !== videoId) return;
    }
    webView.current?.injectJavaScript(extractionScript(id, track));
  }

  function handleMessage(event: WebViewMessageEvent) {
    const origin = event.nativeEvent.url;
    if (!isYouTubeUrl(origin)) return;
    const raw = event.nativeEvent.data;
    if (raw.length > 8_000_000) return;
    try {
      const message = JSON.parse(raw);
      if (message?.type === 'playbackPosition') {
        if (message.videoId === currentVideo.current) transcriptPlayback.handlePosition(message, origin);
        return;
      }
      if (message?.type === 'media' || message?.type === 'mediaEval' || message?.type === 'mediaReady' || message?.type === 'playerLayout' || message?.type === 'sponsorSkipped') {
        void media.handleMessage(raw, origin, message); return;
      }
      if (message?.type === 'page' && typeof message.url === 'string' && isYouTubeUrl(message.url) &&
          youtubeVideoId(message.url) === message.videoId && new URL(origin).origin === new URL(message.url).origin) {
        updateUrl(message.url);
        if (typeof message.userAgent === 'string' && message.userAgent.length < 1000) setPageAgent(message.userAgent);
        return;
      }
      if (message?.type !== 'transcript' || message.requestId !== pending.current?.id || message.videoId !== currentVideo.current) return;
    } catch { return; }
    handleTranscriptMessage(event);
  }

  async function handleTranscriptMessage(event: WebViewMessageEvent) {
    const request = pending.current;
    if (!request || request.videoId !== currentVideo.current) return;
    try {
      const reply = await acceptTranscript(event.nativeEvent.data, event.nativeEvent.url, request.id, request.videoId);
      if (pending.current !== request || request.videoId !== currentVideo.current) return;
      clearTimeout(request.timer); pending.current = null; setExtracting(false);
      if (reply.ok) { setTranscript(reply.transcript); setCachedTranscript(false); setTranscriptError(null); }
      else setTranscriptError(reply.error);
    } catch (error) {
      if (pending.current !== request || request.videoId !== currentVideo.current) return;
      clearTimeout(request.timer); pending.current = null; setExtracting(false);
      setTranscriptError(error instanceof Error ? error.message : 'Could not process this transcript. Try again.');
    }
  }

  const canExtract = !home && !!youtubeVideoId(url) && !!pageAgent;
  const expandedPlayer = panel || media.playback?.offline;
  const playerHidden = !expandedPlayer && media.playerLayout?.visible === false;
  return (
    <SafeAreaView style={styles.safe} edges={['top', 'bottom', 'left', 'right']}>
      <StatusBar style="dark" />
      <KeyboardAvoidingView style={styles.fill} behavior={Platform.OS === 'ios' ? 'padding' : 'height'} keyboardVerticalOffset={insets.top}>
        <View style={styles.topbar}><Text style={styles.brand}>lingoplay<Text style={styles.brandDot}>.</Text></Text>
          <View style={styles.mediaControls}><IconButton icon={Sparkles} label="Open AI assistant" onPress={() => setAiVisible(true)} /><IconButton icon={FolderDown} label="Open downloads" onPress={() => setMediaSheet('library')} /><IconButton icon={Settings2} label="Settings" onPress={() => setMediaSheet('settings')} /></View></View>
        {!home && (canExtract || media.playback) && <View style={styles.mediaToolbar}>
          <Pressable accessibilityRole="button" accessibilityLabel={media.playback?.offline ? 'Close offline player' : media.playback ? 'Return to web player' : 'Use native player for background playback'} disabled={media.busy} style={styles.mediaButton} onPress={() => {
            if (media.playback) { const offline = media.playback.offline; media.closePlayer(!offline); if (offline && !source) setHome(true); }
            else mediaAction(() => media.startNative());
          }}>
            {media.busy ? <ActivityIndicator size="small" color={colors.accent} /> : <Play size={17} color={colors.accent} />}<Text style={styles.mediaLabel}>{media.playback?.offline ? 'Close player' : media.playback ? 'Web player' : 'Player'}</Text></Pressable>
          <Pressable accessibilityRole="button" accessibilityLabel="Download this video" style={styles.mediaButton} disabled={!canExtract || media.busy} onPress={() => { setMediaSheet('download'); media.resolveMedia().catch(() => {}); }}><Download size={17} color={colors.accent} /><Text style={styles.mediaLabel}>Download</Text></Pressable>
          <IconButton icon={PictureInPicture2} label="Picture in picture" disabled={!media.playback || playerStatus !== 'readyToPlay'} onPress={() => mediaAction(async () => {
            if (!nativeVideo.current) throw Error('The video player is not ready yet.');
            if (process.env.EXPO_PUBLIC_LINGOPLAY_DEBUG === '1') console.info('LingoPlay PiP requested');
            await nativeVideo.current.startPictureInPicture();
          })} />
        </View>}
        <View style={styles.workspace} onLayout={event => setViewport({ width: event.nativeEvent.layout.width, height: event.nativeEvent.layout.height })}>
          <View style={[styles.viewport, panel && styles.compact]}>
            {source && <WebView ref={webView} source={source} style={[styles.webview, home && styles.hidden]}
              // Clip the full-size page instead of resizing it: YouTube treats
              // a small WebView window as picture-in-picture and pauses playback.
              containerStyle={[styles.page, viewport.height > 0 && { height: viewport.height }]}
              javaScriptEnabled domStorageEnabled sharedCookiesEnabled allowsInlineMediaPlayback allowsBackForwardNavigationGestures
              webviewDebuggingEnabled={process.env.EXPO_PUBLIC_LINGOPLAY_DEBUG === '1'}
              allowsPictureInPictureMediaPlayback
              // Native caption taps cannot pass a browser user-gesture token.
              mediaPlaybackRequiresUserAction={!isYouTubeUrl(url)}
              setSupportMultipleWindows={false} geolocationEnabled={false} allowFileAccess={false}
              allowFileAccessFromFileURLs={false} allowUniversalAccessFromFileURLs={false} mixedContentMode="never"
              originWhitelist={['*']} injectedJavaScript={PLAYER_DECODER + '\n' + YOUTUBE_BRIDGE + '\n' + mediaSettingsScript(media.settings)}
              injectedJavaScriptBeforeContentLoaded={mediaSettingsScript(media.settings)} onMessage={handleMessage}
              onShouldStartLoadWithRequest={request => request.url === 'about:blank' || isWebUrl(request.url)}
              onNavigationStateChange={state => {
                updateUrl(state.url); setNavigation({ back: state.canGoBack, forward: state.canGoForward });
              }}
              onLoadStart={() => { setLoading(true); setBrowserError(null); }}
              onLoadProgress={event => {
                setProgress(event.nativeEvent.progress);
                // YouTube can leave subresources loading after its document is usable.
                if (event.nativeEvent.progress >= 0.5 && isYouTubeUrl(event.nativeEvent.url)) {
                  webView.current?.injectJavaScript(YOUTUBE_BRIDGE);
                  media.configurePage();
                }
              }}
              onLoadEnd={() => { setLoading(false); webView.current?.injectJavaScript(YOUTUBE_BRIDGE); media.configurePage(); }}
              onError={event => { setLoading(false); setBrowserError(event.nativeEvent.description || 'This page could not be loaded.'); }}
              onRenderProcessGone={() => { cancelRequest(); setLoading(false); setBrowserError('The browser stopped. Reload this page to continue.'); }}
              onContentProcessDidTerminate={() => { cancelRequest(); setLoading(false); setBrowserError('The browser stopped. Reload this page to continue.'); }} />}
            {home && <StartPage onNavigate={navigate} onFocusAddress={() => addressInput.current?.focus()} />}
            {(media.playback || media.preparing) && !home && <View pointerEvents={playerHidden ? 'none' : 'auto'} style={[styles.nativePlayer,
              expandedPlayer ? StyleSheet.absoluteFill : embeddedPlayerFrame(media.playerLayout, viewport.width, viewport.height), playerHidden && styles.playerHidden]}>
              <VideoView ref={nativeVideo} style={styles.fill} player={media.player} nativeControls contentFit="contain" surfaceType="textureView" allowsPictureInPicture startsPictureInPictureAutomatically={media.settings.autoPip} fullscreenOptions={{ enable: true }}
                onPictureInPictureStart={() => { if (process.env.EXPO_PUBLIC_LINGOPLAY_DEBUG === '1') console.info('LingoPlay PiP started'); }}
                onPictureInPictureStop={() => { if (process.env.EXPO_PUBLIC_LINGOPLAY_DEBUG === '1') console.info('LingoPlay PiP stopped'); }} />
              {(media.preparing || playerStatus === 'loading') && <ActivityIndicator style={styles.playerLoading} color="white" />}
              {playerError && media.playback && <View style={styles.playerError}><Text style={styles.playerErrorText}>Video could not load. Check your connection or refresh the video.</Text><Pressable accessibilityRole="button" onPress={() => media.closePlayer()}><Text style={styles.playerErrorText}>Return to web player</Text></Pressable></View>}
            </View>}
            {!home && loading && <View pointerEvents="none" style={[styles.progress, { width: `${Math.max(4, progress * 100)}%` }]} />}
          </View>
          {panel && <View style={styles.transcript}><TranscriptPanel key={currentVideo.current} data={transcript} loading={extracting} error={transcriptError} cached={cachedTranscript}
            position={transcriptPlayback.position}
            onClose={closePanel} onExtract={requestTranscript} onSeek={seconds => {
              Keyboard.dismiss();
              if (!media.seek(seconds) && currentVideo.current) webView.current?.injectJavaScript(seekScript(currentVideo.current, seconds));
            }} /></View>}
        </View>
        {media.notice && <Text accessibilityLiveRegion="polite" style={styles.mediaNotice}>{media.notice}</Text>}
        {media.error && !mediaSheet && !media.preparing && !media.playback && <View style={styles.error}>
          <Text accessibilityRole="alert" style={styles.errorText}>{media.error}</Text>
          <Pressable onPress={() => mediaAction(() => media.startNative())}><Text style={styles.mediaLabel}>Retry</Text></Pressable>
          <Pressable onPress={() => media.closePlayer()} style={{ padding: 12 }}><Text style={styles.mediaLabel}>Use web player</Text></Pressable>
        </View>}
        {browserError && <View style={styles.error}><Text accessibilityRole="alert" style={styles.errorText}>{browserError}</Text>
          <IconButton icon={X} label="Dismiss browser error" onPress={() => setBrowserError(null)} /></View>}
        <View style={styles.chrome}>
          <View style={styles.addressBar}><Globe2 color={colors.muted} size={18} strokeWidth={1.7} />
            <TextInput ref={addressInput} accessibilityLabel="Website address or search" value={editing ? address : home ? '' : url}
              placeholder="Search or enter a website" placeholderTextColor={colors.muted} style={styles.address}
              autoCapitalize="none" autoCorrect={false} keyboardType="web-search" returnKeyType="go" selectTextOnFocus
              onFocus={() => { setAddress(home ? '' : currentUrl.current); setEditing(true); }} onBlur={() => setEditing(false)}
              onChangeText={setAddress} onSubmitEditing={event => navigate(event.nativeEvent.text)} />
            {!home && <IconButton icon={loading ? X : RotateCw} label={loading ? 'Stop loading' : 'Reload page'} onPress={() => {
              if (loading) { webView.current?.stopLoading(); setLoading(false); }
              else { cancelRequest(); setBrowserError(null); webView.current?.reload(); }
            }} />}</View>
          <View style={styles.controls}>
            <View style={styles.history}><IconButton icon={ArrowLeft} label="Go back" disabled={home && !source} onPress={() => {
              if (home && source) setHome(false); else goBack();
            }} /><IconButton icon={ArrowRight} label="Go forward" disabled={home || !navigation.forward} onPress={() => webView.current?.goForward()} />
              <IconButton icon={House} label="Start page" active={home} onPress={showHome} /></View>
            <Pressable accessibilityRole="button" accessibilityLabel={panel ? 'Close transcript' : 'Extract YouTube transcript'} accessibilityState={{ disabled: !canExtract, expanded: panel }}
              disabled={!canExtract} onPress={() => panel ? closePanel() : transcript ? setPanel(true) : requestTranscript('default', true)}
              style={({ pressed }) => [styles.transcriptButton, !canExtract && styles.disabled, pressed && styles.pressed]}>
              {extracting ? <ActivityIndicator color={colors.accent} size="small" /> : <Captions size={19} color={canExtract ? colors.accent : colors.muted} />}
              <Text style={[styles.transcriptLabel, !canExtract && styles.disabledLabel]}>Transcript</Text>
            </Pressable>
          </View>
        </View>
      </KeyboardAvoidingView>
      <PlayerRuntime ref={playerRuntime} />
      <AiSheet visible={aiVisible} onClose={() => setAiVisible(false)} transcript={transcript} videoId={youtubeVideoId(url)} />
      <MediaSheet mode={mediaSheet} onClose={() => setMediaSheet(null)} catalog={media.catalog} loading={media.busy} error={media.error}
        onRefresh={() => { media.resolveMedia().catch(() => {}); }} downloads={downloads}
        settings={media.settings} onSettings={media.updateSettings} sponsorStatus={media.sponsorStatus}
        onDownload={itag => { const catalog = media.catalog; if (!catalog) return; setMediaSheet('library'); mediaAction(() => downloads.start(catalog, itag, pageAgent)); }}
        onPlay={entry => { setMediaSheet(null); setHome(false); mediaAction(() => media.playOffline(entry, downloads.uri(entry))); }}
        onRetry={entry => { setMediaSheet(null); navigate(`https://m.youtube.com/watch?v=${entry.videoId}`); }} />
    </SafeAreaView>
  );
}

export default function App() {
  return <SafeAreaProvider><Browser /></SafeAreaProvider>;
}

const styles = StyleSheet.create({
  mediaControls: { flexDirection: 'row' },
  mediaToolbar: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', paddingHorizontal: 20, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line },
  mediaButton: { flexDirection: 'row', alignItems: 'center', gap: 8, paddingVertical: 12 },
  mediaLabel: { color: colors.accent, fontSize: 13, fontWeight: '600' },
  nativePlayer: { position: 'absolute', backgroundColor: '#000000' },
  playerHidden: { opacity: 0 },
  playerLoading: { ...StyleSheet.absoluteFill },
  playerError: { ...StyleSheet.absoluteFill, backgroundColor: '#202A24', padding: 24, justifyContent: 'center', gap: 24 },
  playerErrorText: { color: 'white', fontSize: 14, textAlign: 'center' },
  mediaNotice: { color: colors.accent, fontSize: 12, paddingHorizontal: 20, paddingVertical: 8 },
  fill: { flex: 1 },
  safe: { flex: 1, backgroundColor: colors.background },
  topbar: { height: 42, paddingHorizontal: 20, flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', gap: 24 },
  brand: { color: colors.ink, fontSize: 17, letterSpacing: -0.7, fontWeight: '600' },
  brandDot: { color: colors.accent },
  domain: { color: colors.muted, fontSize: 11, flexShrink: 1 },
  workspace: { flex: 1 },
  viewport: { flex: 1, overflow: 'hidden' },
  compact: { flex: 0.45 },
  page: { position: 'absolute', top: 0, left: 0, right: 0, flex: 0 },
  transcript: { flex: 0.55 },
  webview: { flex: 1, backgroundColor: colors.background },
  hidden: { display: 'none' },
  progress: { height: 2, backgroundColor: colors.accent, position: 'absolute', top: 0, left: 0 },
  chrome: { paddingHorizontal: 12, paddingTop: 10, borderTopWidth: StyleSheet.hairlineWidth, borderTopColor: colors.line },
  addressBar: { minHeight: 48, borderRadius: 15, backgroundColor: '#EAECE5', flexDirection: 'row', alignItems: 'center', paddingLeft: 14, paddingRight: 2, gap: 10 },
  address: { flex: 1, fontSize: 14, color: colors.ink, paddingVertical: 13, minWidth: 0 },
  controls: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', paddingVertical: 5 },
  history: { flexDirection: 'row', gap: 2 },
  transcriptButton: { flexDirection: 'row', gap: 7, alignItems: 'center', minHeight: 44, paddingHorizontal: 15, borderRadius: 12, backgroundColor: colors.accentSoft },
  transcriptLabel: { color: colors.accent, fontWeight: '600', fontSize: 12 },
  disabled: { backgroundColor: 'transparent' },
  disabledLabel: { color: colors.muted },
  pressed: { opacity: 0.5 },
  error: { flexDirection: 'row', alignItems: 'center', paddingLeft: 18, backgroundColor: '#F5E8E1' },
  errorText: { flex: 1, color: colors.error, fontSize: 12, lineHeight: 18, paddingVertical: 10 },
});
