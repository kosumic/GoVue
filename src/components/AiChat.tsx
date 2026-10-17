import { memo, useEffect, useRef, useState } from 'react';
import { AccessibilityInfo, ActivityIndicator, Animated, FlatList, Linking, Pressable, StyleSheet, Text, TextInput, View } from 'react-native';
import Clipboard from '@react-native-clipboard/clipboard';
import { ArrowDown, ArrowUp, Brain, Captions, ChevronDown, ChevronRight, Copy, MessageCircle, Mic, Plus, RotateCw, Square, X } from 'lucide-react-native';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import type { ChatController, ChatMessage, ChatTranscript } from '../ai/useChats';
import { AiMarkdown } from './AiMarkdown';
import { AiReasoning } from './AiReasoning';
import { useVoiceInput } from '../ai/useVoiceInput';
import { LearningGameCard } from './LearningGameCard';
import type { GameAction } from '../ai/games';

const Message = memo(function Message({ message, last, busy, onRetry, reduceMotion, chatId, activeGameId, onActiveGame, gamesEnabled, onGameAction, onBeforeGameAudio }: { message: ChatMessage; last: boolean; busy: boolean; onRetry: () => void; reduceMotion: boolean; chatId: string; activeGameId: string | null; onActiveGame: (id: string | null) => void; gamesEnabled: boolean; onGameAction: GameAction; onBeforeGameAudio?: () => Promise<void> | void }) {
  const colors = useThemeColors();
  const styles = useThemeStyles(makeStyles);
  const entrance = useRef(new Animated.Value(reduceMotion ? 1 : 0)).current;
  useEffect(() => {
    const animation = Animated.timing(entrance, { toValue: 1, duration: reduceMotion ? 0 : 180, useNativeDriver: true });
    animation.start(); return () => animation.stop();
  }, [entrance, reduceMotion]);
  const user = message.role === 'user';
  const [showReasoning, setShowReasoning] = useState(false);
  return <Animated.View style={[styles.message, user && styles.userMessage, { opacity: entrance, transform: [{ translateY: entrance.interpolate({ inputRange: [0, 1], outputRange: [6, 0] }) }] }]}>
    {user ? <View style={styles.userBubble}><Text selectable style={styles.userText}>{message.content}</Text></View> : <>
      {!!message.reasoning && <View><Pressable accessibilityRole="button" accessibilityState={{ expanded: showReasoning }} onPress={() => setShowReasoning(!showReasoning)} style={styles.messageAction}><Text style={styles.actionText}>{showReasoning ? 'Hide reasoning' : 'Show reasoning'}</Text></Pressable>{showReasoning && <AiMarkdown content={message.reasoning} streaming={message.status === 'streaming' && busy} />}</View>}
      {message.content ? <AiMarkdown content={message.content} streaming={message.status === 'streaming' && busy} /> : message.status === 'streaming' && busy ? <View accessibilityLiveRegion="polite" style={styles.thinking}><ActivityIndicator size="small" color={colors.accent} /><Text style={styles.muted}>Thinking…</Text></View> : null}
      {message.games?.map(game => <LearningGameCard key={game.id} game={game} chatId={chatId} messageId={message.id} active={game.id === activeGameId}
        enabled={gamesEnabled && message.status !== 'streaming'} onActivate={() => onActiveGame(game.id)} onDeactivate={() => onActiveGame(null)} onAction={onGameAction} onBeforeAudio={onBeforeGameAudio} />)}
      {message.error && <Text accessibilityRole="alert" style={styles.error}>{message.error}</Text>}
      {message.status === 'stopped' && !message.error && <Text style={styles.muted}>Response stopped</Text>}
      {message.status !== 'streaming' && <View style={styles.messageActions}>
        {!!message.content && <Pressable accessibilityRole="button" accessibilityLabel="Copy response" onPress={() => Clipboard.setString(message.content)} style={styles.messageAction}><Copy size={16} color={colors.muted} /></Pressable>}
        {last && <Pressable accessibilityRole="button" accessibilityLabel={message.status === 'completed' ? 'Regenerate response' : 'Try again'} disabled={busy} onPress={onRetry} style={[styles.messageAction, busy && styles.disabled]}><RotateCw size={16} color={colors.muted} /></Pressable>}
      </View>}
    </>}
  </Animated.View>;
});

export function AiChat({ chat, draft, onDraftChange, attachment, availableTranscript, connected, loading, contextReady = true, voiceEnabled = true, onBeforeGameAudio, onSend, onRetry, onAttach, onDetach, onOpenTranscript, onSummarize, onModels, reasoning, providerId, model }: {
  chat: ChatController;
  draft: string;
  onDraftChange: (value: string) => void;
  attachment: ChatTranscript | null;
  availableTranscript: ChatTranscript | null;
  connected: boolean;
  loading: boolean;
  contextReady?: boolean;
  voiceEnabled?: boolean;
  onBeforeGameAudio?: () => Promise<void> | void;
  onSend: () => void;
  onRetry: () => void;
  onAttach: () => void;
  onDetach?: () => void;
  onOpenTranscript: () => void;
  onSummarize: () => void;
  onModels: () => void;
  reasoning: string;
  providerId?: string;
  model?: string;
}) {
  const colors = useThemeColors();
  const styles = useThemeStyles(makeStyles);
  const list = useRef<FlatList<ChatMessage>>(null);
  const input = useRef<TextInput>(null);
  const nearBottom = useRef(true);
  const frame = useRef<number | null>(null);
  const [showJump, setShowJump] = useState(false);
  const [reduceMotion, setReduceMotion] = useState(false);
  const [showReasoning, setShowReasoning] = useState(false);
  const [activeGameId, setActiveGameId] = useState<string | null>(null);
  useEffect(() => { setActiveGameId(null); }, [chat.conversation?.id]);
  const messages = chat.conversation?.messages ?? [];
  const voice = useVoiceInput({ enabled: voiceEnabled && !loading && contextReady, chatId: chat.conversation?.id, transcript: attachment ?? availableTranscript, draft, onDraftChange });
  function scrollToEnd(animated = false) {
    nearBottom.current = true; setShowJump(false);
    list.current?.scrollToEnd({ animated });
  }
  function follow() {
    if (!nearBottom.current || frame.current !== null) return;
    frame.current = requestAnimationFrame(() => { frame.current = null; if (nearBottom.current) list.current?.scrollToEnd({ animated: false }); });
  }
  useEffect(() => {
    void AccessibilityInfo.isReduceMotionEnabled().then(setReduceMotion);
    const subscription = AccessibilityInfo.addEventListener('reduceMotionChanged', setReduceMotion);
    return () => { subscription.remove(); if (frame.current !== null) cancelAnimationFrame(frame.current); };
  }, []);
  useEffect(() => { nearBottom.current = true; setShowJump(false); follow(); }, [chat.conversation?.id]);
  useEffect(() => { setShowReasoning(false); }, [chat.conversation?.id, providerId, model]);
  function send() { nearBottom.current = true; setShowJump(false); setShowReasoning(false); onSend(); }
  const canSend = contextReady && connected && !!draft.trim() && !loading && !chat.settingsBusy && !voice.active;
  return <View style={styles.workspace}>
    <View style={styles.thread}>
      <FlatList ref={list} data={messages} keyExtractor={m => m.id} extraData={chat.busy}
        contentContainerStyle={[styles.messages, !messages.length && styles.emptyMessages]}
        keyboardShouldPersistTaps="handled" keyboardDismissMode="on-drag" removeClippedSubviews={false}
        initialNumToRender={10} maxToRenderPerBatch={8} windowSize={7}
        onContentSizeChange={follow} onLayout={follow} scrollEventThrottle={32}
        onScroll={({ nativeEvent: e }) => {
          const atBottom = e.contentSize.height - e.layoutMeasurement.height - e.contentOffset.y < 90;
          nearBottom.current = atBottom; setShowJump(!atBottom);
        }}
        ListEmptyComponent={loading ? <View style={styles.welcome}><ActivityIndicator color={colors.accent} /></View> : <View style={styles.welcome}>
          <Text accessibilityRole="header" style={styles.welcomeTitle}>How can I help?</Text>
          <Text style={styles.welcomeHelp}>{connected ? 'Ask a question or explore a video.' : 'Connect a model to start a conversation.'}</Text>
          {connected ? <View style={styles.suggestions}>
            {availableTranscript && <Pressable accessibilityRole="button" disabled={!contextReady || chat.busy} onPress={onSummarize} style={[styles.suggestion, (!contextReady || chat.busy) && styles.disabled]}><Captions size={17} color={colors.accent} /><Text style={styles.suggestionText}>Summarize this video</Text></Pressable>}
            <Pressable accessibilityRole="button" onPress={() => { voice.changeDraft('Explain this phrase: '); input.current?.focus(); }} style={styles.suggestion}><MessageCircle size={17} color={colors.muted} /><Text style={styles.suggestionText}>Explain a phrase</Text></Pressable>
          </View> : <Pressable accessibilityRole="button" onPress={onModels} style={styles.connect}><Text style={styles.connectText}>Choose a model</Text></Pressable>}
        </View>}
        renderItem={({ item, index }) => <Message message={item} last={index === messages.length - 1} busy={chat.busy} onRetry={onRetry} reduceMotion={reduceMotion}
          chatId={chat.conversation?.id ?? ''} activeGameId={activeGameId} onActiveGame={setActiveGameId} gamesEnabled={voiceEnabled && contextReady && !loading && !voice.active}
          onGameAction={chat.gameAction} onBeforeGameAudio={onBeforeGameAudio} />}
      />
      {showJump && <Pressable accessibilityRole="button" accessibilityLabel="Jump to latest message" onPress={() => scrollToEnd(!reduceMotion)} style={styles.jump}><ArrowDown size={20} color={colors.ink} /></Pressable>}
    </View>
    {!!chat.error && <Text accessibilityRole="alert" style={[styles.error, styles.composerError]}>{chat.error}</Text>}
    <View style={styles.composerDock}>
      {attachment && <View style={styles.attachment}>
        <Pressable accessibilityRole="button" accessibilityLabel={`Open ${attachment.language} transcript`} accessibilityHint="Read the captions used as AI context" onPress={onOpenTranscript} style={styles.attachmentLink}>
          <Captions size={17} color={colors.accent} /><View style={styles.attachmentText}><Text numberOfLines={1} style={styles.attachmentTitle}>{attachment.title}</Text><Text style={styles.attachmentHelp}>{attachment.language} transcript · AI context</Text></View><ChevronRight size={16} color={colors.muted} />
        </Pressable>
        {onDetach && <Pressable accessibilityRole="button" accessibilityLabel="Remove transcript attachment" disabled={chat.busy} onPress={onDetach} style={styles.messageAction}><X size={17} color={colors.muted} /></Pressable>}
      </View>}
      <View style={styles.composer}>
        <TextInput ref={input} accessibilityLabel="Message LingoPlay" placeholder={attachment ? 'Ask about this video…' : 'Ask anything…'} placeholderTextColor={colors.muted}
          value={draft} onChangeText={voice.changeDraft} multiline maxLength={8000} style={styles.input} selectionColor={colors.accent} editable={!loading} />
        {!!voice.interimText && <Text accessibilityLabel="Speech preview" accessibilityLiveRegion="polite" numberOfLines={3} style={styles.voicePreview}>{voice.interimText}</Text>}
        {showReasoning && providerId && model && <AiReasoning id={providerId} model={model} value={chat.reasoningEffort} busy={chat.settingsBusy} deferred={chat.busy} onSelect={level => { void chat.setReasoning(level).then(saved => { if (saved) setShowReasoning(false); }); }} />}
        <View style={styles.composerTools}>
          {availableTranscript && !attachment && <Pressable accessibilityRole="button" accessibilityLabel="Attach current transcript" disabled={chat.busy || loading} onPress={onAttach} style={[styles.attachButton, chat.busy && styles.disabled]}><Plus size={22} color={colors.ink} /></Pressable>}
          <Pressable accessibilityRole="button" accessibilityLabel={`Reasoning effort: ${reasoning}`} accessibilityHint="Changes apply to your next message" accessibilityState={{ expanded: showReasoning, disabled: !connected || loading || chat.settingsBusy }} disabled={!connected || loading || chat.settingsBusy} onPress={() => setShowReasoning(!showReasoning)} style={[styles.reasoningButton, (!connected || loading || chat.settingsBusy) && styles.disabled]}><Brain size={16} color={colors.accent} /><Text numberOfLines={1} style={styles.reasoningText}>{reasoning}</Text><ChevronDown size={12} color={colors.muted} style={showReasoning ? styles.expanded : undefined} /></Pressable>
          <View style={styles.spacer} />
          <Pressable accessibilityRole="button" accessibilityLabel={voice.active ? 'Stop voice input' : 'Start voice input'} accessibilityHint="Dictate a message using words from this chat and transcript as hints" accessibilityState={{ selected: voice.active, disabled: voice.disabled }}
            disabled={voice.disabled} onPress={() => { input.current?.blur(); voice.toggle(); }} style={[styles.attachButton, voice.active && styles.voiceActive, voice.disabled && styles.disabled]}>
            {voice.phase === 'preparing' || voice.phase === 'finishing' ? <ActivityIndicator size="small" color={colors.accent} /> : voice.active ? <Square size={18} fill={colors.accent} color={colors.accent} /> : <Mic size={22} color={colors.ink} />}
          </Pressable>
          <Pressable accessibilityRole="button" accessibilityLabel={chat.busy ? 'Stop response' : 'Send message'} disabled={!chat.busy && !canSend}
            onPress={chat.busy ? chat.stop : send} style={({ pressed }) => [styles.send, (!chat.busy && !canSend || pressed) && styles.disabled]}>
            {chat.busy ? <Square size={16} fill={colors.surface} color={colors.surface} /> : <ArrowUp size={22} strokeWidth={2.3} color={colors.surface} />}
          </Pressable>
        </View>
        {voice.active && <Text accessibilityLiveRegion="polite" style={styles.voiceStatus}>{voice.phase === 'preparing' ? 'Starting voice input…' : voice.phase === 'finishing' ? 'Finishing voice input…' : 'Listening… Tap the microphone to finish.'}</Text>}
      </View>
      {!!voice.error && <Text accessibilityRole="alert" style={styles.error}>{voice.error}</Text>}
      {voice.needsSettings && <Pressable accessibilityRole="button" onPress={() => { void Linking.openSettings(); }} style={styles.messageAction}><Text style={styles.actionText}>Open Settings</Text></Pressable>}
    </View>
  </View>;
}

const makeStyles = (colors: ThemeColors) => StyleSheet.create({
  workspace: { flex: 1 }, thread: { flex: 1 }, messages: { width: '100%', maxWidth: 800, alignSelf: 'center', paddingHorizontal: 20, paddingTop: 24, paddingBottom: 24, gap: 28 }, emptyMessages: { flexGrow: 1 },
  message: { width: '100%' }, userMessage: { alignItems: 'flex-end' },
  userBubble: { maxWidth: '88%', paddingHorizontal: 18, paddingVertical: 12, borderRadius: 24, backgroundColor: colors.raised },
  userText: { color: colors.ink, fontSize: 16, lineHeight: 25 },
  thinking: { flexDirection: 'row', alignItems: 'center', gap: 10, minHeight: 32 }, muted: { fontSize: 13, color: colors.muted, lineHeight: 20 },
  messageActions: { flexDirection: 'row', alignItems: 'center', gap: 4, marginTop: 4, marginLeft: -10 }, messageAction: { minWidth: 44, minHeight: 44, paddingHorizontal: 10, flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 7 }, actionText: { color: colors.muted, fontSize: 12 },
  error: { color: colors.error, fontSize: 13, lineHeight: 20, marginTop: 8 }, composerError: { paddingHorizontal: 20, paddingBottom: 8 },
  welcome: { flex: 1, alignItems: 'center', justifyContent: 'center', paddingHorizontal: 20, paddingVertical: 28, gap: 16 },
  welcomeTitle: { color: colors.ink, fontSize: 28, fontWeight: '600', letterSpacing: -0.7 }, welcomeHelp: { color: colors.muted, fontSize: 14, lineHeight: 22, textAlign: 'center', maxWidth: 260 },
  suggestions: { marginTop: 16, gap: 4 }, suggestion: { flexDirection: 'row', alignItems: 'center', gap: 10, minHeight: 48, paddingHorizontal: 14 }, suggestionText: { color: colors.ink, fontSize: 14 },
  connect: { marginTop: 8, minHeight: 46, paddingHorizontal: 22, alignItems: 'center', justifyContent: 'center', borderRadius: 14, backgroundColor: colors.accent }, connectText: { color: colors.surface, fontSize: 14, fontWeight: '600' },
  composerDock: { width: '100%', maxWidth: 800, alignSelf: 'center', paddingHorizontal: 12, paddingTop: 8, paddingBottom: 10, gap: 8 }, composer: { backgroundColor: colors.raised, borderRadius: 28, paddingBottom: 7 },
  input: { color: colors.ink, fontSize: 16, lineHeight: 24, minHeight: 56, maxHeight: 160, textAlignVertical: 'top', paddingTop: 18, paddingBottom: 10, paddingHorizontal: 18 },
  composerTools: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', paddingLeft: 7, paddingRight: 7, gap: 10 }, spacer: { flex: 1 },
  reasoningButton: { flexDirection: 'row', alignItems: 'center', gap: 5, minHeight: 44, paddingHorizontal: 9, flexShrink: 1 }, reasoningText: { color: colors.accent, fontSize: 12, flexShrink: 1 },
  expanded: { transform: [{ rotate: '180deg' }] },
  attachButton: { minHeight: 44, minWidth: 44, alignItems: 'center', justifyContent: 'center' }, send: { width: 42, height: 42, borderRadius: 21, backgroundColor: colors.accent, alignItems: 'center', justifyContent: 'center' },
  voiceActive: { backgroundColor: colors.accentSoft, borderRadius: 22 }, voiceStatus: { color: colors.accent, fontSize: 12, paddingHorizontal: 18, paddingTop: 6, paddingBottom: 7 },
  voicePreview: { color: colors.muted, fontSize: 15, lineHeight: 22, paddingHorizontal: 18, paddingBottom: 10 },
  attachment: { flexDirection: 'row', alignItems: 'center', borderRadius: 14, backgroundColor: colors.accentSoft }, attachmentLink: { flex: 1, flexDirection: 'row', alignItems: 'center', gap: 9, paddingHorizontal: 12, minHeight: 56 }, attachmentText: { flex: 1, paddingVertical: 9, gap: 2 }, attachmentTitle: { color: colors.ink, fontSize: 12, fontWeight: '500' }, attachmentHelp: { color: colors.muted, fontSize: 10 },
  jump: { position: 'absolute', alignSelf: 'center', bottom: 8, width: 40, height: 40, borderRadius: 20, backgroundColor: colors.surface, borderWidth: 1, borderColor: colors.line, alignItems: 'center', justifyContent: 'center' }, disabled: { opacity: 0.38 },
});
