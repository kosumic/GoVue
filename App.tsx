import { useCallback, useEffect, useRef, useState } from 'react';
import { ActivityIndicator, Alert, BackHandler, Keyboard, KeyboardAvoidingView, Platform, Pressable, StyleSheet, Text, TextInput, View } from 'react-native';
import { SafeAreaProvider, SafeAreaView, useSafeAreaInsets } from 'react-native-safe-area-context';
import { StatusBar } from 'expo-status-bar';
import WebView, { type WebViewMessageEvent } from 'react-native-webview';
import { ArrowLeft, ArrowRight, Bookmark, Captions, Download, FolderDown, Globe2, House, Minimize2, PictureInPicture2, Play, RotateCw, Settings2, Sparkles, X } from 'lucide-react-native';
import { useEvent } from 'expo';
import { VideoView } from 'expo-video';
import { browserAddress, isWebUrl, isYouTubeUrl, youtubeVideoId } from './src/browser/address';
import { IconButton } from './src/components/IconButton';
import { StartPage } from './src/components/StartPage';
import { TranscriptPanel } from './src/components/TranscriptPanel';
import { colors } from './src/theme';
import { extractionScript, seekScript, YOUTUBE_BRIDGE } from './src/youtube/pageBridge';
import { type Transcript } from './src/youtube/transcript';
import { createTranscriptTrace, TRANSCRIPT_TRACING, type TranscriptTrace } from './src/youtube/transcriptTrace';
import { acceptTranscript, restoreTranscript } from './src/core/transcripts';
import { MediaSheet, type MediaSheetMode } from './src/components/MediaSheet';
import { useDownloads } from './src/media/downloads';
import { useMediaBrowser } from './src/media/useMediaBrowser';
import { mediaSettingsScript } from './src/youtube/mediaBridge';
import { PLAYER_DECODER } from './src/youtube/generated/playerDecoder';
import { PlayerRuntime, type PlayerRuntimeHandle } from './src/media/PlayerRuntime';
import { embeddedPlayerFrame } from './src/media/playerLayout';
import { useTranscriptPosition } from './src/media/useTranscriptPosition';
import { AiSheet, type SummaryRequest } from './src/components/AiSheet';
import { SavedWordsSheet } from './src/components/SavedWordsSheet';
import { PlayerControls } from './src/components/PlayerControls';
import { BrowserResumeBar } from './src/components/BrowserResumeBar';
import { useWebViewRenderer } from './src/browser/useWebViewRenderer';

type PendingRequest = { id: string; videoId: string; timer: ReturnType<typeof setTimeout>; trace: TranscriptTrace };

function Browser() {
  const insets = useSafeAreaInsets();
  const renderer = useWebViewRenderer();
  const webView = renderer.view;
  const pageStopped = useRef(false);
  const [pageAgent, setPageAgent] = useState('');
  const addressInput = useRef<TextInput>(null);
  const [source, setSource] = useState<{ uri: string } | null>(null);
  const [home, setHome] = useState(true);
  const [browserMinimized, setBrowserMinimized] = useState(false);
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
  const renderingTranscript = useRef<{ videoId: string; trace: TranscriptTrace } | null>(null);
  const sequence = useRef(0);
  const [mediaSheet, setMediaSheet] = useState<MediaSheetMode>(null);
  const [aiVisible, setAiVisible] = useState(false);
  const [wordsVisible, setWordsVisible] = useState(false);
  const [aiSummaryRequest, setAiSummaryRequest] = useState<SummaryRequest | null>(null);
  const downloads = useDownloads();
  const playerRuntime = useRef<PlayerRuntimeHandle>(null);
  const media = useMediaBrowser(webView, youtubeVideoId(url), pageAgent, playerRuntime);
  const transcriptPlayback = useTranscriptPosition(webView, media.player, youtubeVideoId(url), panel && !home && !!transcript && !extracting && !transcriptError && (!pageStopped.current || !!media.playback), media.playback?.videoId ?? null, transcript, media.settings.pauseAfterLine);
  const playerState = useEvent(media.player, 'statusChange', { status: media.player.status, error: undefined });
  const playerStatus = playerState?.status ?? media.player.status;
  const playerError = playerState?.error;
  const mediaAction = (work: () => Promise<unknown>) => { work().catch(error => Alert.alert('LingoPlay', error instanceof Error ? error.message : 'Please try again.')); };

  const cancelRequest = useCallback((notifyPage = true) => {
    if (pending.current) { clearTimeout(pending.current.timer); pending.current.trace.finish(); }
    renderingTranscript.current?.trace.finish();
    renderingTranscript.current = null;
    pending.current = null;
    if (notifyPage) webView.current?.injectJavaScript('window.__lingoplay?.cancel(); true;');
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

  useEffect(() => () => {
    if (pending.current) { clearTimeout(pending.current.timer); pending.current.trace.finish(); }
    renderingTranscript.current?.trace.finish();
  }, []);

  useEffect(() => {
    const rendering = renderingTranscript.current;
    if (!rendering || extracting || !panel || transcript?.videoId !== rendering.videoId) return;
    rendering.trace.finish();
    renderingTranscript.current = null;
  }, [transcript, extracting, panel]);

  const closePanel = useCallback(() => { setPanel(false); cancelRequest(); }, [cancelRequest]);
  const showHome = useCallback(() => {
    closePanel(); setHome(true); setBrowserMinimized(false); setBrowserError(null); Keyboard.dismiss();
  }, [closePanel]);

  const restoreBrowser = useCallback(() => {
    Keyboard.dismiss(); setHome(false); setBrowserMinimized(false);
    setAiVisible(false); setAiSummaryRequest(null); setWordsVisible(false); setMediaSheet(null);
  }, []);

  const goBack = useCallback(() => {
    if (panel) { closePanel(); return true; }
    if (browserMinimized) { restoreBrowser(); return true; }
    if (home) return false;
    if (navigation.back) webView.current?.goBack();
    else showHome();
    return true;
  }, [panel, home, browserMinimized, navigation.back, closePanel, showHome, restoreBrowser]);

  useEffect(() => {
    const subscription = BackHandler.addEventListener('hardwareBackPress', goBack);
    return () => subscription.remove();
  }, [goBack]);

  const isCurrentPage = () => renderer.isCurrent() && !pageStopped.current;
  function pageTerminated() {
    if (!isCurrentPage()) return;
    pageStopped.current = true;
    if (Platform.OS === 'android') renderer.retire();
    cancelRequest(false);
    media.pageTerminated();
    setPageAgent(''); setLoading(false); setProgress(0);
    setNavigation({ back: false, forward: false });
    setBrowserError('The browser stopped. Reload this page to continue.');
    if (!transcript) setTranscriptError('The browser stopped. Reload the page, then try again.');
  }

  function reloadBrowser() {
    cancelRequest(!pageStopped.current); setBrowserError(null); setPageAgent(''); setProgress(0);
    if (!renderer.mounted) {
      const next = currentUrl.current;
      if (!isWebUrl(next)) return;
      setSource({ uri: next });
      renderer.recreate();
    } else webView.current?.reload();
    pageStopped.current = false;
  }

  function navigate(input: string) {
    try {
      const next = browserAddress(input);
      cancelRequest(); setPanel(false); setTranscript(null); setTranscriptError(null);
      updateUrl(next); setAddress(next); setBrowserError(null); setHome(false); setBrowserMinimized(false); setEditing(false);
      Keyboard.dismiss();
      if (!source || pageStopped.current || !renderer.mounted) {
        setSource({ uri: next });
        if (!renderer.mounted) renderer.recreate();
        pageStopped.current = false;
      }
      else webView.current?.injectJavaScript(`window.location.assign(${JSON.stringify(next)}); true;`);
    } catch (error) {
      setBrowserError(error instanceof Error ? error.message : 'Could not open this address.');
    }
  }

  async function requestTranscript(track = 'default', preferCache = false) {
    const videoId = currentVideo.current;
    if (!videoId || home) return;
    if (!isCurrentPage()) { setBrowserError('The browser stopped. Reload this page to continue.'); return; }
    cancelRequest();
    setPanel(true); setExtracting(true); setTranscriptError(null); setCachedTranscript(false);
    Keyboard.dismiss();
    const id = `${Date.now()}:${++sequence.current}`;
    const trace = createTranscriptTrace(id);
    const timer = setTimeout(() => {
      if (pending.current?.id !== id) return;
      cancelRequest();
      setTranscriptError('Transcript request timed out. Play the video or check your connection, then try again.');
    }, 45000);
    pending.current = { id, videoId, timer, trace };
    if (preferCache) {
      trace.event('cache.restore', true);
      try {
        const cached = await restoreTranscript(videoId);
        trace.event('cache.restore', false);
        if (pending.current?.id !== id || currentVideo.current !== videoId) return;
        if (cached) {
          clearTimeout(timer); pending.current = null;
          trace.event('reader.commit', true);
          renderingTranscript.current = { videoId, trace };
          setTranscript(cached); setCachedTranscript(true); setExtracting(false);
          return;
        }
      } catch { trace.event('cache.restore', false); /* A missing cache must not block a fresh YouTube request. */ }
      if (pending.current?.id !== id || currentVideo.current !== videoId) return;
    }
    trace.event('bridge.extract', true);
    webView.current?.injectJavaScript(extractionScript(id, track, TRANSCRIPT_TRACING));
  }

  function handleMessage(event: WebViewMessageEvent) {
    const origin = event.nativeEvent.url;
    if (!isYouTubeUrl(origin)) return;
    const raw = event.nativeEvent.data;
    if (raw.length > 8_000_000) return;
    try {
      const message = JSON.parse(raw);
      if (message?.type === 'transcriptTrace') {
        if (raw.length <= 500 && message.requestId === pending.current?.id && message.videoId === currentVideo.current &&
            typeof message.stage === 'string' && message.stage.startsWith('page.') &&
            (message.phase === 'begin' || message.phase === 'end')) {
          pending.current?.trace.event(message.stage, message.phase === 'begin');
        }
        return;
      }
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
    request.trace.event('bridge.extract', false);
    request.trace.event('core.accept', true);
    try {
      const reply = await acceptTranscript(event.nativeEvent.data, event.nativeEvent.url, request.id, request.videoId);
      request.trace.event('core.accept', false);
      if (pending.current !== request || request.videoId !== currentVideo.current) return;
      clearTimeout(request.timer); pending.current = null; setExtracting(false);
      if (reply.ok) {
        request.trace.event('reader.commit', true);
        renderingTranscript.current = { videoId: request.videoId, trace: request.trace };
        setTranscript(reply.transcript); setCachedTranscript(false); setTranscriptError(null);
      }
      else { request.trace.finish(); setTranscriptError(reply.error); }
    } catch (error) {
      if (pending.current !== request || request.videoId !== currentVideo.current) return;
      clearTimeout(request.timer); pending.current = null; setExtracting(false);
      request.trace.finish();
      setTranscriptError(error instanceof Error ? error.message : 'Could not process this transcript. Try again.');
    }
  }

  const canExtract = !home && !!youtubeVideoId(url) && !!pageAgent && isCurrentPage();
  const canReadTranscript = canExtract || (!home && !!transcript);
  const expandedPlayer = panel || media.playback?.offline || pageStopped.current;
  const playerHidden = !expandedPlayer && media.playerLayout?.visible === false;
  const browserHidden = home || browserMinimized;
  const browserControl = source || media.playback ? <BrowserResumeBar title={media.playback?.title || url} player={media.player}
    hasPlayback={!!media.playback} onRestore={restoreBrowser} /> : null;
  return (
    <SafeAreaView style={styles.safe} edges={['top', 'bottom', 'left', 'right']}>
      <StatusBar style="dark" />
      <KeyboardAvoidingView style={styles.fill} behavior={Platform.OS === 'ios' ? 'padding' : 'height'} keyboardVerticalOffset={insets.top}>
        <View style={styles.topbar}><Text style={styles.brand}>lingoplay<Text style={styles.brandDot}>.</Text></Text>
          <View style={styles.mediaControls}><IconButton icon={Bookmark} label="Open saved words" onPress={() => setWordsVisible(true)} /><IconButton icon={Sparkles} label="Open AI assistant" onPress={() => { setAiSummaryRequest(null); setAiVisible(true); }} /><IconButton icon={FolderDown} label="Open downloads" onPress={() => setMediaSheet('library')} /><IconButton icon={Settings2} label="Settings" onPress={() => setMediaSheet('settings')} /></View></View>
        {!browserHidden && (canExtract || media.playback) && <View style={styles.mediaToolbar}>
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
          <IconButton icon={Minimize2} label="Minimize browser" onPress={() => { Keyboard.dismiss(); setBrowserMinimized(true); }} />
        </View>}
        {!browserHidden && media.playback && <PlayerControls key={`${media.playback.videoId}:${media.playback.offline}`} player={media.player}
          speeds={media.speeds} qualities={media.qualities} height={media.playback.qualityHeight} offline={media.playback.offline}
          busy={media.switchingQuality} onSpeed={media.setSpeed} onQuality={media.setQuality} />}
        <View style={styles.workspace} onLayout={event => {
          if (!browserHidden || viewport.height === 0) setViewport({ width: event.nativeEvent.layout.width, height: event.nativeEvent.layout.height });
        }}>
          {/* Preserve both native views and the page size while the browser is minimized. */}
          <View pointerEvents={browserHidden ? 'none' : 'auto'} accessibilityElementsHidden={browserHidden} importantForAccessibility={browserHidden ? 'no-hide-descendants' : 'auto'}
            style={[styles.viewport, panel && styles.compact, browserHidden && [styles.parkedViewport, { height: viewport.height }]]}>
            {source && renderer.mounted && <WebView key={renderer.generation} ref={renderer.attach} source={source} style={styles.webview}
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
              injectedJavaScriptBeforeContentLoaded={mediaSettingsScript(media.settings)} onMessage={event => { if (isCurrentPage()) handleMessage(event); }}
              onShouldStartLoadWithRequest={request => isCurrentPage() && (request.url === 'about:blank' || isWebUrl(request.url))}
              onNavigationStateChange={state => {
                if (!isCurrentPage()) return;
                updateUrl(state.url); setNavigation({ back: state.canGoBack, forward: state.canGoForward });
              }}
              onLoadStart={() => { if (isCurrentPage()) { setLoading(true); setBrowserError(null); } }}
              onLoadProgress={event => {
                if (!isCurrentPage()) return;
                setProgress(event.nativeEvent.progress);
                // YouTube can leave subresources loading after its document is usable.
                if (event.nativeEvent.progress >= 0.5 && isYouTubeUrl(event.nativeEvent.url)) {
                  webView.current?.injectJavaScript(YOUTUBE_BRIDGE);
                  media.configurePage();
                }
              }}
              onLoadEnd={() => { if (isCurrentPage()) { setLoading(false); webView.current?.injectJavaScript(YOUTUBE_BRIDGE); media.configurePage(); } }}
              onError={event => { if (isCurrentPage()) { setLoading(false); setBrowserError(event.nativeEvent.description || 'This page could not be loaded.'); } }}
              onRenderProcessGone={pageTerminated}
              onContentProcessDidTerminate={pageTerminated} />}
            {(media.playback || media.preparing) && <View pointerEvents={playerHidden ? 'none' : 'auto'} style={[styles.nativePlayer,
              expandedPlayer ? StyleSheet.absoluteFill : embeddedPlayerFrame(media.playerLayout, viewport.width, viewport.height), playerHidden && styles.playerHidden]}>
              <VideoView ref={nativeVideo} style={styles.fill} player={media.player} nativeControls contentFit="contain" surfaceType="textureView" allowsPictureInPicture startsPictureInPictureAutomatically={media.settings.autoPip} fullscreenOptions={{ enable: true }}
                onPictureInPictureStart={() => { if (process.env.EXPO_PUBLIC_LINGOPLAY_DEBUG === '1') console.info('LingoPlay PiP started'); }}
                onPictureInPictureStop={() => { if (process.env.EXPO_PUBLIC_LINGOPLAY_DEBUG === '1') console.info('LingoPlay PiP stopped'); }} />
              {(media.preparing || playerStatus === 'loading') && <ActivityIndicator style={styles.playerLoading} color="white" />}
              {playerError && media.playback && <View style={styles.playerError}><Text style={styles.playerErrorText}>Video could not load. Check your connection or refresh the video.</Text><Pressable accessibilityRole="button" onPress={() => media.closePlayer()}><Text style={styles.playerErrorText}>Return to web player</Text></Pressable></View>}
            </View>}
            {!home && loading && <View pointerEvents="none" style={[styles.progress, { width: `${Math.max(4, progress * 100)}%` }]} />}
          </View>
          {browserHidden && !panel && <StartPage onNavigate={navigate} onFocusAddress={() => {
            setBrowserMinimized(false); requestAnimationFrame(() => addressInput.current?.focus());
          }} />}
          {panel && <View style={[styles.transcript, browserMinimized && styles.fill]}><TranscriptPanel key={currentVideo.current} data={transcript} loading={extracting} error={transcriptError} cached={cachedTranscript}
            position={transcriptPlayback.position}
            pauseAfterLine={media.settings.pauseAfterLine} onPauseAfterLine={pauseAfterLine => media.updateSettings({ pauseAfterLine })}
            onSummarize={() => {
              if (!transcript || extracting || transcriptError || transcript.videoId !== currentVideo.current) return;
              Keyboard.dismiss();
              setAiSummaryRequest({ id: ++sequence.current, videoId: transcript.videoId, trackId: transcript.trackId });
              setAiVisible(true);
            }}
            onClose={closePanel} onExtract={requestTranscript} onSeek={seconds => {
              Keyboard.dismiss();
              transcriptPlayback.reset();
              if (!media.seek(seconds) && currentVideo.current) webView.current?.injectJavaScript(seekScript(currentVideo.current, seconds));
            }} /></View>}
        </View>
        {browserHidden && browserControl}
        {media.notice && <Text accessibilityLiveRegion="polite" style={styles.mediaNotice}>{media.notice}</Text>}
        {media.error && !mediaSheet && !media.preparing && !media.playback && <View style={styles.error}>
          <Text accessibilityRole="alert" style={styles.errorText}>{media.error}</Text>
          <Pressable onPress={() => mediaAction(() => media.startNative())}><Text style={styles.mediaLabel}>Retry</Text></Pressable>
          <Pressable onPress={() => media.closePlayer()} style={{ padding: 12 }}><Text style={styles.mediaLabel}>Use web player</Text></Pressable>
        </View>}
        {browserError && <View style={styles.error}><Text accessibilityRole="alert" style={styles.errorText}>{browserError}</Text>
          <IconButton icon={X} label="Dismiss browser error" onPress={() => setBrowserError(null)} /></View>}
        {!browserMinimized && <View style={styles.chrome}>
          <View style={styles.addressBar}><Globe2 color={colors.muted} size={18} strokeWidth={1.7} />
            <TextInput ref={addressInput} accessibilityLabel="Website address or search" value={editing ? address : home ? '' : url}
              placeholder="Search or enter a website" placeholderTextColor={colors.muted} style={styles.address}
              autoCapitalize="none" autoCorrect={false} keyboardType="web-search" returnKeyType="go" selectTextOnFocus
              onFocus={() => { setAddress(home ? '' : currentUrl.current); setEditing(true); }} onBlur={() => setEditing(false)}
              onChangeText={setAddress} onSubmitEditing={event => navigate(event.nativeEvent.text)} />
            {!home && <IconButton icon={loading ? X : RotateCw} label={loading ? 'Stop loading' : 'Reload page'} onPress={() => {
              if (loading) { webView.current?.stopLoading(); setLoading(false); }
              else reloadBrowser();
            }} />}</View>
          <View style={styles.controls}>
            <View style={styles.history}><IconButton icon={ArrowLeft} label="Go back" disabled={home && !source} onPress={() => {
              if (home && source) setHome(false); else goBack();
            }} /><IconButton icon={ArrowRight} label="Go forward" disabled={home || !navigation.forward} onPress={() => webView.current?.goForward()} />
              <IconButton icon={House} label="Start page" active={home} onPress={showHome} /></View>
            <Pressable accessibilityRole="button" accessibilityLabel={panel ? 'Close transcript' : 'Extract YouTube transcript'} accessibilityState={{ disabled: !canReadTranscript, expanded: panel }}
              disabled={!canReadTranscript} onPress={() => panel ? closePanel() : transcript ? setPanel(true) : requestTranscript('default', true)}
              style={({ pressed }) => [styles.transcriptButton, !canReadTranscript && styles.disabled, pressed && styles.pressed]}>
              {extracting ? <ActivityIndicator color={colors.accent} size="small" /> : <Captions size={19} color={canReadTranscript ? colors.accent : colors.muted} />}
              <Text style={[styles.transcriptLabel, !canReadTranscript && styles.disabledLabel]}>Transcript</Text>
            </Pressable>
          </View>
        </View>}
      </KeyboardAvoidingView>
      <PlayerRuntime ref={playerRuntime} />
      <SavedWordsSheet visible={wordsVisible} browserControl={browserControl} onClose={() => setWordsVisible(false)} onOpen={entry => {
        setWordsVisible(false);
        if (!home && currentVideo.current === entry.videoId) {
          setBrowserMinimized(false);
          if (!media.seek(entry.start)) webView.current?.injectJavaScript(seekScript(entry.videoId, entry.start));
        } else navigate(`https://m.youtube.com/watch?v=${entry.videoId}&t=${Math.floor(entry.start)}s`);
      }} />
      <AiSheet visible={aiVisible} browserControl={browserControl} onClose={() => { setAiVisible(false); setAiSummaryRequest(null); }} transcript={transcript} videoId={youtubeVideoId(url)} summaryRequest={aiSummaryRequest} />
      <MediaSheet mode={mediaSheet} browserControl={browserControl} onClose={() => setMediaSheet(null)} catalog={media.catalog} loading={media.busy} error={media.error}
        onRefresh={() => { media.resolveMedia().catch(() => {}); }} downloads={downloads}
        settings={media.settings} onSettings={media.updateSettings} sponsorStatus={media.sponsorStatus}
        onDownload={itag => { const catalog = media.catalog; if (!catalog) return; setMediaSheet('library'); mediaAction(() => downloads.start(catalog, itag, pageAgent)); }}
        onPlay={entry => { restoreBrowser(); mediaAction(() => media.playOffline(entry, downloads.uri(entry))); }}
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
  parkedViewport: { position: 'absolute', top: 0, left: 0, right: 0, opacity: 0 },
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
