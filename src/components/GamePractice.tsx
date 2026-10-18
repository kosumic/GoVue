import { useEffect, useMemo, useRef, useState } from 'react';
import { ActivityIndicator, Animated, AppState, Keyboard, PanResponder, Pressable, ScrollView, StyleSheet, Text, TextInput, useWindowDimensions, View } from 'react-native';
import { ArrowLeft, Check, ExternalLink, GripHorizontal, Library, Pencil, Play, RotateCw, Undo2, X } from 'lucide-react-native';
import { usePractice } from '../ai/usePractice';
import type { GameAction, GameSource, LearningGame } from '../ai/games';
import { GameSurface } from './LearningGameCard';
import { IconButton } from './IconButton';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import { useReducedMotion } from '../player/useReducedMotion';
import { useHaptic } from '../player/Haptics';
import { PlayerSheet } from '../player/PlayerSheet';

function timestamp(seconds: number) {
  const value = Math.floor(seconds);
  return `${Math.floor(value / 60)}:${String(value % 60).padStart(2, '0')}`;
}
type SourceAction = (videoId: string, start?: number) => void;
function SourceLink({ source, onOpen }: { source?: GameSource | null; onOpen?: SourceAction }) {
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  if (!source || !onOpen) return null;
  return <Pressable accessibilityRole="link" accessibilityLabel={`Open original YouTube video${source.start !== null ? ` at ${timestamp(source.start)}` : ''}`} onPress={() => onOpen(source.videoId, source.start ?? undefined)} style={s.source}>
    <ExternalLink size={14} color={colors.muted} /><Text numberOfLines={1} style={s.sourceText}>Open original{source.start !== null ? ` · ${timestamp(source.start)}` : ''}</Text>
  </Pressable>;
}

function SwipeGame({ game, busy, reduced, onReview, onAction, onReset, onRename, onBeforeAudio, onOpenSource }: {
  game: LearningGame; busy: boolean; reduced: boolean; onReview: (verdict: 'forgot' | 'know') => Promise<void>;
  onAction: GameAction; onReset: () => Promise<void>; onRename: () => void; onBeforeAudio?: () => Promise<void> | void; onOpenSource?: SourceAction;
}) {
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  const feedback = useHaptic();
  const width = useWindowDimensions().width;
  const x = useRef(new Animated.Value(0)).current;
  const y = useRef(new Animated.Value(0)).current;
  const entrance = useRef(new Animated.Value(reduced ? 1 : 0)).current;
  const committing = useRef(false);
  const [leaving, setLeaving] = useState(false);
  const [foreground, setForeground] = useState(AppState.currentState === 'active');
  const [reload, setReload] = useState(0);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    const animation = Animated.timing(entrance, { toValue: 1, duration: reduced ? 0 : 160, useNativeDriver: true });
    animation.start();
    const subscription = AppState.addEventListener('change', state => setForeground(state === 'active'));
    return () => { alive.current = false; animation.stop(); x.stopAnimation(); y.stopAnimation(); subscription.remove(); };
  }, [entrance, x, y, reduced]);
  function restore() {
    Animated.parallel([x, y].map(value => Animated.spring(value, { toValue: 0, useNativeDriver: true, tension: 130, friction: 15 }))).start();
  }
  async function grade(verdict: 'forgot' | 'know') {
    if (committing.current) return;
    if (busy) { restore(); return; }
    committing.current = true; setLeaving(true);
    await new Promise<void>(resolve => Animated.timing(x, { toValue: verdict === 'know' ? width * 1.2 : -width * 1.2, duration: reduced ? 0 : 180, useNativeDriver: true }).start(() => resolve()));
    if (!alive.current) return;
    try { await onReview(verdict); feedback(); }
    catch { if (alive.current) { committing.current = false; setLeaving(false); restore(); } }
  }
  const latest = useRef({ busy, width, grade, restore });
  latest.current = { busy, width, grade, restore };
  // Capture horizontal intent across the whole card, including its WebView.
  // Leave taps and vertical scrolling with the generated game until a drag wins.
  const handlers = useMemo(() => PanResponder.create({
    onMoveShouldSetPanResponderCapture: (_, gesture) => !committing.current && !latest.current.busy && gesture.numberActiveTouches === 1 && Math.abs(gesture.dx) > 8 && Math.abs(gesture.dx) > Math.abs(gesture.dy) * 1.2,
    onPanResponderGrant: () => { x.stopAnimation(); y.stopAnimation(); },
    onPanResponderMove: (_, gesture) => { x.setValue(gesture.dx); y.setValue(gesture.dy); },
    onPanResponderRelease: (_, gesture) => {
      const distance = Math.min(latest.current.width * 0.23, 110);
      if (Math.abs(gesture.dx) >= distance || Math.abs(gesture.dx) > 35 && Math.abs(gesture.vx) > 0.65) void latest.current.grade(gesture.dx > 0 ? 'know' : 'forgot');
      else latest.current.restore();
    },
    onPanResponderTerminate: () => latest.current.restore(),
    onPanResponderTerminationRequest: () => false,
    onShouldBlockNativeResponder: () => true,
  }).panHandlers, [x, y]);
  const inactive = busy || leaving;
  return <View style={s.workspace}>
    <View pointerEvents="none" style={s.stackBack} />
    <Animated.View {...handlers} style={[s.gameCard, { opacity: entrance, transform: [{ translateX: x }, { translateY: y }, { rotate: reduced ? '0deg' : x.interpolate({ inputRange: [-width, 0, width], outputRange: ['-14deg', '0deg', '14deg'] }) }, { scale: entrance.interpolate({ inputRange: [0, 1], outputRange: [0.98, 1] }) }] }]}>
      <View style={s.cardHeading} accessibilityLabel="Swipe card left for Forgot or right for Know">
        <View style={s.cardTitle}><GripHorizontal size={16} color={colors.muted} /><Text accessibilityRole="header" numberOfLines={2} style={s.title}>{game.definition.title}</Text></View>
        <IconButton icon={Pencil} label="Rename activity" disabled={inactive} onPress={onRename} />
        <IconButton icon={RotateCw} label="Restart activity" disabled={inactive} onPress={() => { void onReset().then(() => { if (alive.current) setReload(value => value + 1); }).catch(() => {}); }} />
      </View>
      <View pointerEvents="none" style={s.stamps}>
        <Animated.Text style={[s.stamp, { opacity: x.interpolate({ inputRange: [-90, -12, 0], outputRange: [1, 0, 0], extrapolate: 'clamp' }) }]}>FORGOT</Animated.Text>
        <Animated.Text style={[s.stamp, s.knowStamp, { opacity: x.interpolate({ inputRange: [0, 12, 90], outputRange: [0, 0, 1], extrapolate: 'clamp' }) }]}>KNOW</Animated.Text>
      </View>
      <View style={s.gameBody}>
        <View style={s.swipeEdge} />
        <View style={s.gameContent} pointerEvents={leaving ? 'none' : 'auto'}>
          {foreground && <GameSurface key={`${game.id}-${reload}`} game={game} practice chatId="" messageId="" active enabled expanded
            onActivate={() => {}} onDeactivate={() => {}} onAction={onAction} onBeforeAudio={onBeforeAudio} />}
        </View>
        <View style={s.swipeEdge} />
      </View>
      <SourceLink source={game.source} onOpen={onOpenSource} />
      <View style={s.swipeHint}><Text style={s.help}>← Forgot</Text><Text style={s.help}>Drag anywhere</Text><Text style={s.help}>Know →</Text></View>
    </Animated.View>
    <View style={s.reviewButtons}>
      <Pressable accessibilityRole="button" accessibilityLabel="Forgot this activity" disabled={inactive} onPress={() => void grade('forgot')} style={[s.gradeButton, inactive && s.disabled]}><X size={22} color={colors.ink} /><Text style={s.buttonText}>Forgot</Text></Pressable>
      <Pressable accessibilityRole="button" accessibilityLabel="Know this activity" disabled={inactive} onPress={() => void grade('know')} style={[s.gradeButton, s.knowButton, inactive && s.disabled]}><Check size={22} color={colors.inverted} /><Text style={[s.buttonText, s.knowText]}>Know</Text></Pressable>
    </View>
  </View>;
}

export function GamePractice({ onBeforeAudio, onOpenSource }: { onBeforeAudio?: () => Promise<void> | void; onOpenSource?: SourceAction }) {
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  const practice = usePractice();
  const reduced = useReducedMotion();
  const [showLibrary, setShowLibrary] = useState(false);
  const [startedHere, setStartedHere] = useState(false);
  const [renaming, setRenaming] = useState<{ id: string; title: string } | null>(null);
  const [name, setName] = useState('');
  const [renameError, setRenameError] = useState('');
  const library = practice.library;
  const session = library?.session;
  async function start(filter: 'all' | 'forgot' = 'all', gameId?: string) {
    try { await practice.start(filter, gameId); setShowLibrary(false); setStartedHere(true); } catch {}
  }
  function editName(game: { id: string; title: string }) { setRenaming(game); setName(game.title); setRenameError(''); }
  async function saveName() {
    if (!renaming || practice.busy) return;
    setRenameError('');
    try { await practice.rename(renaming.id, name); Keyboard.dismiss(); setRenaming(null); }
    catch (reason) { setRenameError(reason instanceof Error ? reason.message : 'Could not rename this activity.'); }
  }
  const reviewing = !!session?.started && !showLibrary && (!!session.game || startedHere || session.canUndo);
  return <View style={s.container}>
    {!!practice.error && <View style={s.errorRow}><Text accessibilityRole="alert" style={s.error}>{practice.error}</Text><Pressable accessibilityRole="button" onPress={() => void practice.load()} style={s.smallButton}><Text style={s.buttonText}>Reload</Text></Pressable></View>}
    {!library ? <View style={s.empty}><ActivityIndicator color={colors.muted} /><Text style={s.help}>Opening saved activities…</Text></View>
      : reviewing && session ? <>
        <View style={s.sessionHeading}>
          <Pressable accessibilityRole="button" accessibilityLabel="View saved activities" disabled={practice.busy} onPress={() => setShowLibrary(true)} style={s.smallButton}><Library size={18} color={colors.ink} /><Text style={s.buttonText}>Library</Text></Pressable>
          <Text accessibilityLiveRegion="polite" style={s.progress}>{session.game ? `${session.total - session.remaining + 1} / ${session.total}` : `${session.total} / ${session.total}`}</Text>
          <Pressable accessibilityRole="button" accessibilityLabel="Undo last swipe" disabled={!session.canUndo || practice.busy} onPress={() => void practice.undo().catch(() => {})} style={[s.smallButton, (!session.canUndo || practice.busy) && s.disabled]}><Undo2 size={18} color={colors.ink} /><Text style={s.buttonText}>Undo</Text></Pressable>
        </View>
        {session.game ? <SwipeGame key={session.game.id} game={session.game} busy={practice.busy} reduced={reduced} onReview={practice.review} onAction={practice.gameAction} onReset={practice.reset} onRename={() => editName({ id: session.game!.id, title: session.game!.definition.title })} onBeforeAudio={onBeforeAudio} onOpenSource={onOpenSource} />
          : <View style={s.empty}><Check size={36} color={colors.ink} /><Text accessibilityRole="header" style={s.emptyTitle}>Session complete</Text><Text style={s.help}>{session.known} known · {session.forgot} forgotten</Text>
            {!!session.forgot && <Pressable accessibilityRole="button" disabled={practice.busy} onPress={() => void start('forgot')} style={s.primary}><Text style={s.primaryText}>Practice forgotten activities</Text></Pressable>}
            <Pressable accessibilityRole="button" disabled={practice.busy} onPress={() => void start()} style={s.smallButton}><RotateCw size={17} color={colors.ink} /><Text style={s.buttonText}>Review all again</Text></Pressable>
          </View>}
      </> : <>
        <View style={s.libraryHeading}><Text style={s.help}>{library.games.length} saved activit{library.games.length === 1 ? 'y' : 'ies'}</Text>
          {!!session?.game && <Pressable accessibilityRole="button" disabled={practice.busy} onPress={() => setShowLibrary(false)} style={s.smallButton}><Play size={16} color={colors.ink} /><Text style={s.buttonText}>Continue</Text></Pressable>}
        </View>
        {!library.games.length ? <View style={s.empty}><Library size={36} color={colors.muted} /><Text accessibilityRole="header" style={s.emptyTitle}>Your practice starts in chat</Text><Text style={s.emptyHelp}>Ask the assistant to create a word, grammar, or sentence activity. Saved games appear here.</Text></View>
          : <><Pressable accessibilityRole="button" disabled={practice.busy} onPress={() => void start()} style={[s.primary, s.reviewAll]}><Play size={18} color={colors.inverted} /><Text style={s.primaryText}>Review all</Text></Pressable>
          <ScrollView contentContainerStyle={s.libraryList}>
            {library.games.map(game => <View key={game.id} style={s.libraryRow}>
              <Pressable accessibilityRole="button" accessibilityLabel={`Practice ${game.title}`} disabled={practice.busy} onPress={() => void start('all', game.id)} style={s.libraryEntry}>
                <View style={s.libraryText}><Text numberOfLines={2} style={s.rowTitle}>{game.title}</Text><Text numberOfLines={1} style={s.help}>{game.source?.title || 'Learning activity'}{game.verdict ? ` · ${game.verdict === 'know' ? 'Known' : 'Forgotten'}` : ''}</Text></View><ArrowLeft size={18} color={colors.muted} style={{ transform: [{ rotate: '180deg' }] }} />
              </Pressable>
              <IconButton icon={Pencil} label={`Rename ${game.title}`} disabled={practice.busy} onPress={() => editName(game)} />
            </View>)}
          </ScrollView></>}
      </>}
    <PlayerSheet visible={!!renaming} title="Rename activity" presentation="dialog" onClose={() => { if (!practice.busy) { Keyboard.dismiss(); setRenaming(null); } }}
      footer={<View style={s.renameButtons}>
        <Pressable accessibilityRole="button" disabled={practice.busy} onPress={() => { Keyboard.dismiss(); setRenaming(null); }} style={s.smallButton}><Text style={s.buttonText}>Cancel</Text></Pressable>
        <Pressable accessibilityRole="button" accessibilityLabel="Save activity name" disabled={practice.busy || !name.trim()} onPress={() => void saveName()} style={[s.primary, (practice.busy || !name.trim()) && s.disabled]}><Text style={s.primaryText}>{practice.busy ? 'Saving…' : 'Save'}</Text></Pressable>
      </View>}>
      {renaming && <View style={s.renameForm}>
        <TextInput accessibilityLabel="Activity name" value={name} onChangeText={setName} maxLength={120} autoFocus selectTextOnFocus editable={!practice.busy} returnKeyType="done" onSubmitEditing={() => void saveName()} style={s.nameInput} selectionColor={colors.action + '55'} cursorColor={colors.ink} />
        {!!renameError && <Text accessibilityRole="alert" style={s.error}>{renameError}</Text>}
      </View>}
    </PlayerSheet>
  </View>;
}

const makeStyles = (colors: ThemeColors) => StyleSheet.create({
  container: { flex: 1 }, workspace: { flex: 1, paddingHorizontal: 18, paddingTop: 8, paddingBottom: 16 },
  stackBack: { position: 'absolute', top: 16, left: 28, right: 28, bottom: 84, borderRadius: 24, backgroundColor: colors.raised, borderWidth: 1, borderColor: colors.line },
  gameCard: { flex: 1, borderRadius: 22, borderWidth: 1, borderColor: colors.line, backgroundColor: colors.background, overflow: 'hidden', marginBottom: 20 },
  cardHeading: { flexDirection: 'row', alignItems: 'center', paddingLeft: 16, paddingRight: 4, minHeight: 62, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line },
  cardTitle: { flex: 1, flexDirection: 'row', alignItems: 'center', gap: 10 }, title: { flex: 1, color: colors.ink, fontSize: 17, fontWeight: '600', lineHeight: 23, paddingVertical: 12 },
  gameBody: { flex: 1, flexDirection: 'row' }, gameContent: { flex: 1 }, swipeEdge: { width: 12 },
  swipeHint: { flexDirection: 'row', justifyContent: 'space-between', gap: 12, minHeight: 46, alignItems: 'center', paddingHorizontal: 18 },
  stamps: { position: 'absolute', left: 18, right: 18, top: 64, flexDirection: 'row', justifyContent: 'space-between', zIndex: 2 },
  stamp: { color: colors.ink, backgroundColor: colors.raised, borderColor: colors.ink, borderWidth: 2, borderRadius: 6, paddingHorizontal: 12, paddingVertical: 6, fontSize: 23, fontWeight: '800', transform: [{ rotate: '-9deg' }] }, knowStamp: { transform: [{ rotate: '9deg' }] },
  reviewButtons: { flexDirection: 'row', justifyContent: 'center', gap: 18 }, gradeButton: { minHeight: 56, minWidth: 132, paddingHorizontal: 22, borderRadius: 30, backgroundColor: colors.raised, flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 10 }, knowButton: { backgroundColor: colors.accent }, knowText: { color: colors.inverted },
  source: { minHeight: 40, flexDirection: 'row', alignItems: 'center', justifyContent: 'center', paddingHorizontal: 16, gap: 8 }, sourceText: { color: colors.muted, fontSize: 13 },
  sessionHeading: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', paddingHorizontal: 10, paddingVertical: 4 }, progress: { color: colors.muted, fontSize: 13, fontVariant: ['tabular-nums'] },
  smallButton: { minHeight: 44, paddingHorizontal: 12, flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 8 }, buttonText: { color: colors.ink, fontSize: 14, fontWeight: '500' },
  help: { color: colors.muted, fontSize: 13, lineHeight: 20 }, disabled: { opacity: 0.4 },
  errorRow: { paddingHorizontal: 20, flexDirection: 'row', alignItems: 'center', gap: 8 }, error: { flex: 1, color: colors.error, fontSize: 13, lineHeight: 20 },
  empty: { flex: 1, padding: 28, justifyContent: 'center', alignItems: 'center', gap: 18 }, emptyTitle: { color: colors.ink, fontSize: 22, fontWeight: '600', textAlign: 'center' }, emptyHelp: { color: colors.muted, fontSize: 14, lineHeight: 22, textAlign: 'center', maxWidth: 320 },
  primary: { minHeight: 50, borderRadius: 26, paddingHorizontal: 24, backgroundColor: colors.accent, flexDirection: 'row', justifyContent: 'center', alignItems: 'center', gap: 10 }, primaryText: { color: colors.inverted, fontSize: 15, fontWeight: '600' }, reviewAll: { marginHorizontal: 22, marginBottom: 18 },
  libraryHeading: { paddingHorizontal: 22, minHeight: 54, flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between' }, libraryList: { paddingHorizontal: 22, paddingBottom: 24 },
  libraryRow: { minHeight: 86, flexDirection: 'row', alignItems: 'center', borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line }, libraryEntry: { flex: 1, flexDirection: 'row', alignItems: 'center', gap: 14 }, libraryText: { flex: 1, gap: 5, paddingVertical: 14 }, rowTitle: { color: colors.ink, fontSize: 16, fontWeight: '500', lineHeight: 22 },
  renameForm: { paddingHorizontal: 20, paddingBottom: 20, gap: 12 }, nameInput: { minHeight: 52, borderWidth: 1, borderColor: colors.outline, borderRadius: 12, paddingHorizontal: 12, color: colors.ink, fontSize: 16 }, renameButtons: { flexDirection: 'row', justifyContent: 'flex-end', gap: 12, paddingHorizontal: 20, paddingBottom: 20 },
});
