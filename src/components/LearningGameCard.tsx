import { useEffect, useRef, useState } from 'react';
import { ActivityIndicator, AppState, Modal, Pressable, StyleSheet, Text, View } from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { Expand, Minimize2, Play, RotateCw, Square, X } from 'lucide-react-native';
import WebView from 'react-native-webview';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import { gameDocument, scriptJSON } from '../ai/gameRuntime';
import { useGameAudio } from '../ai/useGameAudio';
import type { GameAction, LearningGame } from '../ai/games';

type Props = {
  game: LearningGame;
  chatId: string;
  messageId: string;
  active: boolean;
  enabled: boolean;
  onActivate: () => void;
  onDeactivate: () => void;
  onAction: GameAction;
  onBeforeAudio?: () => Promise<void> | void;
};

function GameSurface({ game, chatId, messageId, onAction, onBeforeAudio, expanded }: Props & { expanded: boolean }) {
  const colors = useThemeColors();
  const styles = useThemeStyles(makeStyles);
  const view = useRef<WebView>(null);
  const alive = useRef(true);
  const ready = useRef(false);
  const requests = useRef(new Set<string>());
  const [failed, setFailed] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [document] = useState(() => {
    const session = `game-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
    return { session, source: { baseUrl: 'about:blank', html: gameDocument({ definition: game.definition, state: game.state, cues: game.cues, session, colors: { ...colors, dark: colors.background === '#0F0F0F' } }) } };
  });
  const audio = useGameAudio({ chatId, messageId, gameId: game.id }, onBeforeAudio);
  function reply(id: string, state?: Record<string, unknown>, error?: string) {
    if (alive.current && view.current) view.current.injectJavaScript(`window.__lingoGameReply?.(${scriptJSON({ id, state, error })}); true;`);
  }
  function fail() {
    if (!alive.current) return;
    alive.current = false;
    audio.stop(); setFailed(true); setLoading(false);
  }
  useEffect(() => {
    alive.current = true;
    const timer = setTimeout(() => { if (!ready.current) fail(); }, 10000);
    return () => { alive.current = false; clearTimeout(timer); };
  }, []);
  useEffect(() => { if (ready.current) reply('state', game.state); }, [game.state, game.revision]);
  async function receive(raw: string) {
    if (!alive.current || failed || raw.length > 10000) return;
    let request: { id: string; method: string; action?: unknown; input?: unknown; cueId?: unknown };
    try {
      const message = JSON.parse(raw);
      if (message.session !== document.session || !message.request || message.request.v !== 1
        || typeof message.request.id !== 'string' || message.request.id.length > 80) return;
      request = message.request;
    } catch { return; }
    if (request.method === 'ready') { ready.current = true; setLoading(false); reply('state', game.state); return; }
    if (request.method === 'error') { setError('The generated interface encountered an error. You can reopen the game or ask the assistant to fix it.'); return; }
    if (!ready.current) return;
    if (!/^\d{1,8}$/.test(request.id) || requests.current.has(request.id) || requests.current.size >= 8) return;
    requests.current.add(request.id);
    const eventId = `${document.session}-${request.id}`;
    try {
      if (request.method === 'action') {
        if (typeof request.action !== 'string' || request.action === '__play_audio' || !request.input || typeof request.input !== 'object' || Array.isArray(request.input)) throw Error('Invalid game action.');
        const saved = await onAction(messageId, game.id, request.action, request.input as Record<string, unknown>, eventId);
        reply(request.id, saved.state);
      } else if (request.method === 'audio') {
        if (typeof request.cueId !== 'string' || !game.cues.some(cue => cue.id === request.cueId)) throw Error('This audio is not part of the game.');
        await onAction(messageId, game.id, '__play_audio', { cueId: request.cueId }, eventId);
        if (!alive.current) return;
        await audio.play(request.cueId);
        reply(request.id);
      }
    } catch (error) {
      const text = error instanceof Error ? error.message : 'The game action failed.';
      if (alive.current) { setError(text); reply(request.id, undefined, text); }
    } finally {
      requests.current.delete(request.id);
    }
  }
  return <View style={[styles.surface, expanded && styles.expanded]}>
    {failed ? <View style={styles.placeholder}><Text style={styles.help}>This interface could not be loaded. Reopen the game to try again, or ask the assistant to create a replacement.</Text></View> : <WebView
      ref={view} source={document.source} style={styles.webview} originWhitelist={['about:*']}
      javaScriptEnabled domStorageEnabled={false} cacheEnabled={false} sharedCookiesEnabled={false} thirdPartyCookiesEnabled={false}
      allowFileAccess={false} allowFileAccessFromFileURLs={false} allowUniversalAccessFromFileURLs={false}
      javaScriptCanOpenWindowsAutomatically={false} setSupportMultipleWindows={false} mixedContentMode="never"
      mediaPlaybackRequiresUserAction allowsInlineMediaPlayback={false}
      onShouldStartLoadWithRequest={request => request.url === 'about:blank' || request.url === 'about:srcdoc'}
      onOpenWindow={() => {}} onRenderProcessGone={fail} onContentProcessDidTerminate={fail} onError={fail}
      onMessage={event => {
        // Android's WebMessageListener reports the opaque document origin as
        // "null"; iOS and the older Android bridge report its URL. receive()
        // still requires the private host session before accepting a request.
        const { url, data } = event.nativeEvent;
        if (url === 'about:blank' || url === 'null') void receive(data);
      }}
    />}
    {loading && <View pointerEvents="none" style={styles.loading}><ActivityIndicator color={colors.accent} /><Text style={styles.help}>Opening game…</Text></View>}
    {audio.phase !== 'idle' && <View style={styles.audioStatus}>
      {audio.phase === 'loading' ? <ActivityIndicator size="small" color={colors.accent} /> : <Play size={14} color={colors.accent} />}
      <Text accessibilityLiveRegion="polite" style={styles.help}>{audio.phase === 'loading' ? 'Preparing audio…' : 'Playing video audio'}</Text>
      <Pressable accessibilityRole="button" accessibilityLabel="Stop game audio" onPress={audio.stop} style={styles.control}><Square size={16} color={colors.ink} /></Pressable>
    </View>}
    {!!error && <Text accessibilityRole="alert" style={styles.error}>{error}</Text>}
  </View>;
}

export function LearningGameCard(props: Props) {
  const colors = useThemeColors();
  const styles = useThemeStyles(makeStyles);
  const [expanded, setExpanded] = useState(false);
  const [reload, setReload] = useState(0);
  const [foreground, setForeground] = useState(AppState.currentState === 'active');
  useEffect(() => {
    const subscription = AppState.addEventListener('change', state => setForeground(state === 'active'));
    return () => subscription.remove();
  }, []);
  const active = props.active && props.enabled && foreground;
  useEffect(() => { if (!active) setExpanded(false); }, [active]);
  const controls = <View style={styles.controls}>
    <Text accessibilityRole="header" style={styles.title}>{props.game.definition.title}</Text>
    {active && <>
      <Pressable accessibilityRole="button" accessibilityLabel="Reload game" onPress={() => setReload(value => value + 1)} style={styles.control}><RotateCw size={17} color={colors.muted} /></Pressable>
      <Pressable accessibilityRole="button" accessibilityLabel={expanded ? 'Collapse game' : 'Expand game'} onPress={() => setExpanded(!expanded)} style={styles.control}>{expanded ? <Minimize2 size={17} color={colors.muted} /> : <Expand size={17} color={colors.muted} />}</Pressable>
      <Pressable accessibilityRole="button" accessibilityLabel="Close game" onPress={props.onDeactivate} style={styles.control}><X size={18} color={colors.muted} /></Pressable>
    </>}
  </View>;
  return <View style={styles.card}>
    {controls}
    {active && !expanded ? <GameSurface key={`${props.game.id}-${reload}`} {...props} expanded={false} /> : <Pressable accessibilityRole="button" disabled={!props.enabled} onPress={props.onActivate} style={styles.open}>
      <Play size={18} color={colors.accent} /><Text style={styles.openText}>{props.game.revision ? 'Continue game' : 'Play game'}</Text>
    </Pressable>}
    <Modal visible={active && expanded} animationType="slide" presentationStyle="fullScreen" onRequestClose={() => setExpanded(false)}>
      <SafeAreaView style={styles.fullscreen}>{controls}{active && expanded && <GameSurface key={`${props.game.id}-${reload}-expanded`} {...props} expanded />}</SafeAreaView>
    </Modal>
  </View>;
}

const makeStyles = (colors: ThemeColors) => StyleSheet.create({
  card: { borderWidth: 1, borderColor: colors.line, borderRadius: 16, overflow: 'hidden', marginVertical: 10, backgroundColor: colors.background },
  controls: { flexDirection: 'row', alignItems: 'center', paddingLeft: 16, paddingRight: 4, minHeight: 50 },
  title: { flex: 1, color: colors.ink, fontSize: 15, fontWeight: '600', paddingVertical: 12 },
  control: { width: 44, height: 44, alignItems: 'center', justifyContent: 'center' },
  open: { flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 10, padding: 24, backgroundColor: colors.raised },
  openText: { color: colors.accent, fontSize: 15, fontWeight: '600' },
  surface: { height: 400 }, expanded: { flex: 1, height: undefined }, webview: { flex: 1, backgroundColor: colors.background },
  fullscreen: { flex: 1, backgroundColor: colors.background }, placeholder: { flex: 1, padding: 24, justifyContent: 'center' },
  help: { color: colors.muted, fontSize: 13, lineHeight: 20 },
  loading: { position: 'absolute', inset: 0, alignItems: 'center', justifyContent: 'center', gap: 12, backgroundColor: colors.background },
  audioStatus: { flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 8, minHeight: 44, backgroundColor: colors.raised },
  error: { color: colors.error, paddingHorizontal: 16, paddingBottom: 10, fontSize: 12, lineHeight: 18 },
});
