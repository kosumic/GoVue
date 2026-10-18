import { memo, useEffect, useRef, useState, type ReactElement } from 'react';
import { AccessibilityInfo, ActivityIndicator, Animated, FlatList, Linking, Pressable, StyleSheet, Text, TextInput, View } from 'react-native';
import { ArrowDown, ArrowUp, Brain, Captions, Check, ChevronDown, ChevronRight, Copy, MessageCircle, Mic, Pencil, Plus, Square, X } from 'lucide-react-native';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import type { ChatController, ChatMessage, ChatTranscript } from '../ai/useChats';
import { AiMarkdown } from './AiMarkdown';
import { AiReasoning } from './AiReasoning';
import { ReasoningActivity, ToolActivity } from './AiActivity';
import { useVoiceInput } from '../ai/useVoiceInput';
import { useResponseSpeech } from '../ai/useExampleSpeech';
import { stopCaptionAudio } from '../ai/useGameAudio';
import { LearningGameCard } from './LearningGameCard';
import type { GameAction } from '../ai/games';
import { AiMessageDetails } from './AiMessageDetails';
import { useCopyFeedback } from './useCopyFeedback';
import { ChatExpansionContext, useChatViewport, type ChatViewportStore } from './useChatViewport';
import { ChatInputSelection } from './ChatInputSelection';
import { SelectableChatText } from './SelectableChatText';
import { useChatFind, type ChatFindController } from '../ai/useChatFind';
import { ChatFindBar } from './ChatSearch';

function UserPrompt({ content, onEdit, disabled }: { content: string; onEdit: () => void; disabled: boolean }) {
  const colors = useThemeColors();
  const styles = useThemeStyles(makeStyles);
  const { copied, copyText } = useCopyFeedback();
  return <View style={[styles.message, styles.userMessage]}>
    <View style={styles.userBubble}><SelectableChatText text={content} /></View>
    <View style={styles.userActions}>
    <Pressable accessibilityRole="button" accessibilityLabel="Edit message" disabled={disabled} onPress={onEdit} style={[styles.messageAction, disabled && styles.disabled]}><Pencil size={16} color={colors.muted} /></Pressable>
    <Pressable accessibilityRole="button" accessibilityLabel={copied ? 'Prompt copied' : 'Copy prompt'} accessibilityLiveRegion="polite" onPress={() => copyText(content)} style={styles.messageAction}>
      {copied ? <Check size={16} color={colors.action} /> : <Copy size={16} color={colors.muted} />}
    </Pressable>
    </View>
  </View>;
}

const Message = memo(function Message({ message, last, busy, onRetry, retryDisabled, onEdit, editDisabled, reduceMotion, chatId, activeGameId, onActiveGame, gamesEnabled, onGameAction, onBeforeGameAudio, reading, readDisabled, onRead }: { message: ChatMessage; last: boolean; busy: boolean; onRetry: () => void; retryDisabled: boolean; onEdit: () => void; editDisabled: boolean; reduceMotion: boolean; chatId: string; activeGameId: string | null; onActiveGame: (id: string | null) => void; gamesEnabled: boolean; onGameAction: GameAction; onBeforeGameAudio?: () => Promise<void> | void; reading: boolean; readDisabled: boolean; onRead: () => void }) {
  const colors = useThemeColors();
  const styles = useThemeStyles(makeStyles);
  const entrance = useRef(new Animated.Value(reduceMotion ? 1 : 0)).current;
  useEffect(() => {
    const animation = Animated.timing(entrance, { toValue: 1, duration: reduceMotion ? 0 : 180, useNativeDriver: true });
    animation.start(); return () => animation.stop();
  }, [entrance, reduceMotion]);
  const user = message.role === 'user';
  const streaming = message.status === 'streaming' && busy;
  const blocks = message.blocks ?? [];
  const linkedGames = new Set(blocks.flatMap(block => block.kind === 'tool' && block.gameId ? [block.gameId] : []));
  const gameCard = (game: NonNullable<ChatMessage['games']>[number]) => <LearningGameCard key={game.id} game={game} chatId={chatId} messageId={message.id} active={game.id === activeGameId}
    enabled={gamesEnabled} onActivate={() => onActiveGame(game.id)} onDeactivate={() => onActiveGame(null)} onAction={onGameAction} onBeforeAudio={onBeforeGameAudio} />;
  return <Animated.View style={[styles.message, user && styles.userMessage, { opacity: entrance, transform: [{ translateY: entrance.interpolate({ inputRange: [0, 1], outputRange: [6, 0] }) }] }]}>
    {user ? <UserPrompt content={message.content} onEdit={onEdit} disabled={editDisabled} /> : <>
      {blocks.length ? blocks.map(block => <View key={block.id}>
        {block.kind === 'text' ? <AiMarkdown content={block.content} streaming={streaming && block.status === 'running'} />
          : block.kind === 'reasoning' ? <ReasoningActivity id={`${message.id}:${block.id}`} content={block.content} active={streaming && block.status === 'running'} />
          : <><ToolActivity block={block} messageStatus={message.status} busy={busy} />{message.games?.filter(game => game.id === block.gameId).map(gameCard)}</>}
      </View>) : <>
        {!!message.reasoning && <ReasoningActivity id={`${message.id}:reasoning`} content={message.reasoning} active={streaming && !message.content} />}
        {!!message.content && <AiMarkdown content={message.content} streaming={streaming} />}
      </>}
      {streaming && (blocks.length ? blocks.at(-1)?.kind === 'tool' && !blocks.some(block => block.status === 'preparing' || block.status === 'running') : !message.content && !message.reasoning) && <View accessibilityLiveRegion="polite" style={styles.thinking}><ActivityIndicator size="small" color={colors.accent} /><Text style={styles.muted}>Thinking…</Text></View>}
      {message.games?.filter(game => !linkedGames.has(game.id)).map(gameCard)}
      {message.error && <Text accessibilityRole="alert" style={styles.error}>{message.error}</Text>}
      {message.status === 'stopped' && !message.error && <Text style={styles.muted}>Response stopped</Text>}
      <AiMessageDetails message={message} last={last} retryDisabled={retryDisabled} onRetry={onRetry} reading={reading} readDisabled={readDisabled} onRead={onRead} />
    </>}
  </Animated.View>;
});

function ChatThread({ chatId, messages, viewports, jumpRequest, reduceMotion, empty, renderMessage, search, onCloseFind }: {
  chatId: string; messages: ChatMessage[]; viewports: ChatViewportStore; jumpRequest: number; reduceMotion: boolean;
  empty: ReactElement; renderMessage: (message: ChatMessage, index: number) => ReactElement;
  search?: ChatFindController; onCloseFind: () => void;
}) {
  const colors = useThemeColors();
  const styles = useThemeStyles(makeStyles);
  const viewport = useChatViewport(chatId, messages, viewports, !!search);
  const previousJump = useRef(jumpRequest);
  useEffect(() => {
    if (previousJump.current !== jumpRequest) viewport.jumpToLatest();
    previousJump.current = jumpRequest;
  }, [jumpRequest, viewport.jumpToLatest]);
  const hit = search?.hit;
  useEffect(() => {
    if (hit?.messageId) viewport.revealMessage(hit.messageId);
  }, [hit?.messageId, hit?.offset, search?.results.query, search?.navigation, viewport.revealMessage]);
  return <ChatExpansionContext.Provider value={viewport.expansion}>
    {search && <ChatFindBar search={search} onClose={onCloseFind} />}
    <View collapsable={false} pointerEvents={viewport.ready ? 'auto' : 'none'} accessibilityElementsHidden={!viewport.ready} importantForAccessibility={viewport.ready ? 'auto' : 'no-hide-descendants'} style={[styles.thread, { opacity: viewport.ready ? 1 : 0 }]}>
      <FlatList ref={viewport.list} data={messages} keyExtractor={m => m.id} {...viewport.listProps}
        contentContainerStyle={[styles.messages, !messages.length && styles.emptyMessages]}
        keyboardShouldPersistTaps="handled" keyboardDismissMode="on-drag" removeClippedSubviews={false}
        initialNumToRender={10} maxToRenderPerBatch={8} windowSize={7}
        ListEmptyComponent={empty} extraData={hit?.messageId} renderItem={({ item, index }) => <View style={item.id === hit?.messageId ? styles.searchHit : undefined}>{renderMessage(item, index)}</View>} />
      {viewport.showJump && <Pressable accessibilityRole="button" accessibilityLabel="Jump to latest message" onPress={() => viewport.jumpToLatest(!reduceMotion)} style={styles.jump}><ArrowDown size={20} color={colors.ink} /></Pressable>}
    </View>
  </ChatExpansionContext.Provider>;
}

export function AiChat({ chat, draft, onDraftChange, attachment, availableTranscript, connected, loading, contextReady = true, voiceEnabled = true, viewports, findQuery, onCloseFind = () => {}, onBeforeGameAudio, onSend, onRetry, onAttach, onDetach, onOpenTranscript, onSummarize, onModels, reasoning, providerId, model }: {
  chat: ChatController;
  draft: string;
  onDraftChange: (value: string) => void;
  attachment: ChatTranscript | null;
  availableTranscript: ChatTranscript | null;
  connected: boolean;
  loading: boolean;
  contextReady?: boolean;
  voiceEnabled?: boolean;
  viewports?: ChatViewportStore;
  findQuery?: string;
  onCloseFind?: () => void;
  onBeforeGameAudio?: () => Promise<void> | void;
  onSend: (editMessageId?: string) => void | Promise<boolean>;
  onRetry: (messageId: string) => void;
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
  const input = useRef<TextInput>(null);
  const localViewports = useRef<ChatViewportStore>(new Map());
  const [jumpRequest, setJumpRequest] = useState(0);
  const [reduceMotion, setReduceMotion] = useState(false);
  const [showReasoning, setShowReasoning] = useState(false);
  const [editing, setEditing] = useState<{ id: string; originalDraft: string } | null>(null);
  const [activeGameId, setActiveGameId] = useState<string | null>(null);
  const messages = chat.conversation?.messages ?? [];
  const findOpen = findQuery !== undefined;
  const search = useChatFind(chat.conversation?.id, messages, findOpen && !chat.loading, findQuery ?? '');
  const latestGameId = messages.flatMap(message => message.games ?? []).at(-1)?.id ?? null;
  useEffect(() => { setActiveGameId(latestGameId); }, [chat.conversation?.id, latestGameId]);
  const voice = useVoiceInput({ enabled: voiceEnabled && !loading && contextReady, chatId: chat.conversation?.id, transcript: attachment ?? availableTranscript, draft, onDraftChange });
  const readEnabled = voiceEnabled && contextReady && !loading && !voice.active;
  const speech = useResponseSpeech(JSON.stringify([chat.conversation?.id, readEnabled]), async () => { stopCaptionAudio(); await onBeforeGameAudio?.(); });
  useEffect(() => {
    if (speech.active && !messages.some(message => message.id === speech.active && message.status !== 'streaming')) void speech.stop();
  }, [messages, speech.active, speech.stop]);
  async function beforeGameAudio() { await speech.stop(); await onBeforeGameAudio?.(); }
  useEffect(() => {
    void AccessibilityInfo.isReduceMotionEnabled().then(setReduceMotion);
    const subscription = AccessibilityInfo.addEventListener('reduceMotionChanged', setReduceMotion);
    return () => { subscription.remove(); };
  }, []);
  useEffect(() => { setShowReasoning(false); }, [chat.conversation?.id, providerId, model]);
  useEffect(() => { setEditing(null); }, [chat.conversation?.id]);
  function edit(message: ChatMessage) {
    setEditing(current => ({ id: message.id, originalDraft: current?.originalDraft ?? draft }));
    onDraftChange(message.content); setShowReasoning(false); input.current?.focus();
  }
  function cancelEdit() { if (editing) onDraftChange(editing.originalDraft); setEditing(null); }
  function addSelectionToInput(text: string) {
    if (loading || !text.trim()) return;
    voice.changeDraft(`${draft}${draft && !draft.endsWith('\n') ? '\n\n' : ''}${text}`.slice(0, 8000));
    setShowReasoning(false); input.current?.focus();
  }
  function send() {
    setJumpRequest(value => value + 1); setShowReasoning(false);
    const id = editing?.id;
    void Promise.resolve(onSend(id)).then(sent => { if (sent !== false) setEditing(current => current?.id === id ? null : current); });
  }
  const canSend = contextReady && connected && !!draft.trim() && !loading && !chat.settingsBusy && !voice.active;
  return <View style={styles.workspace}>
    <ChatInputSelection.Provider value={loading ? undefined : addSelectionToInput}>
    <ChatThread key={chat.conversation?.id ?? 'new'} chatId={chat.conversation?.id ?? 'new'} messages={messages} viewports={viewports ?? localViewports.current} jumpRequest={jumpRequest} reduceMotion={reduceMotion}
        search={findOpen ? search : undefined} onCloseFind={onCloseFind}
        empty={loading ? <View style={styles.welcome}><ActivityIndicator color={colors.accent} /></View> : <View style={styles.welcome}>
          <Text accessibilityRole="header" style={styles.welcomeTitle}>How can I help?</Text>
          <Text style={styles.welcomeHelp}>{connected ? 'Ask a question or explore a video.' : 'Connect a model to start a conversation.'}</Text>
          {connected ? <View style={styles.suggestions}>
            {availableTranscript && <Pressable accessibilityRole="button" disabled={!contextReady || chat.busy} onPress={onSummarize} style={[styles.suggestion, (!contextReady || chat.busy) && styles.disabled]}><Captions size={17} color={colors.accent} /><Text style={styles.suggestionText}>Summarize this video</Text></Pressable>}
            <Pressable accessibilityRole="button" onPress={() => { voice.changeDraft('Explain this phrase: '); input.current?.focus(); }} style={styles.suggestion}><MessageCircle size={17} color={colors.muted} /><Text style={styles.suggestionText}>Explain a phrase</Text></Pressable>
          </View> : <Pressable accessibilityRole="button" onPress={onModels} style={styles.connect}><Text style={styles.connectText}>Choose a model</Text></Pressable>}
        </View>}
        renderMessage={(item, index) => <Message message={item} last={index === messages.length - 1} busy={chat.busy} onRetry={() => onRetry(item.id)} retryDisabled={chat.busy || loading || chat.settingsBusy || !connected || !contextReady} onEdit={() => edit(item)} editDisabled={chat.busy || loading || chat.settingsBusy} reduceMotion={reduceMotion}
          chatId={chat.conversation?.id ?? ''} activeGameId={activeGameId} onActiveGame={setActiveGameId} gamesEnabled={voiceEnabled && contextReady && !loading && !voice.active}
          onGameAction={chat.gameAction} onBeforeGameAudio={beforeGameAudio} reading={speech.active === item.id} readDisabled={!readEnabled}
          onRead={() => { if (speech.active === item.id) void speech.stop(); else void speech.speak(item.id, item.content); }} />}
      />
    </ChatInputSelection.Provider>
    {!!chat.error && <Text accessibilityRole="alert" style={[styles.error, styles.composerError]}>{chat.error}</Text>}
    {!!speech.error && <Text accessibilityRole="alert" style={[styles.error, styles.composerError]}>{speech.error}</Text>}
    {!findOpen && <View style={styles.composerDock}>
      {editing && <View style={styles.editing}>
        <View style={styles.attachmentText}><Text style={styles.attachmentTitle}>Editing message</Text><Text style={styles.attachmentHelp}>Sending replaces later messages and replies.</Text></View>
        <Pressable accessibilityRole="button" accessibilityLabel="Cancel edit" disabled={chat.busy} onPress={cancelEdit} style={styles.messageAction}><X size={17} color={colors.muted} /></Pressable>
      </View>}
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
            disabled={voice.disabled} onPress={() => { input.current?.blur(); void speech.stop(); voice.toggle(); }} style={[styles.attachButton, voice.active && styles.voiceActive, voice.disabled && styles.disabled]}>
            {voice.phase === 'preparing' || voice.phase === 'finishing' ? <ActivityIndicator size="small" color={colors.accent} /> : voice.active ? <Square size={18} fill={colors.accent} color={colors.accent} /> : <Mic size={22} color={colors.ink} />}
          </Pressable>
          <Pressable accessibilityRole="button" accessibilityLabel={chat.busy ? 'Stop response' : editing ? 'Send edited message' : 'Send message'} disabled={!chat.busy && !canSend}
            onPress={chat.busy ? chat.stop : send} style={({ pressed }) => [styles.send, (!chat.busy && !canSend || pressed) && styles.disabled]}>
            {chat.busy ? <Square size={16} fill={colors.surface} color={colors.surface} /> : <ArrowUp size={22} strokeWidth={2.3} color={colors.surface} />}
          </Pressable>
        </View>
        {voice.active && <Text accessibilityLiveRegion="polite" style={styles.voiceStatus}>{voice.phase === 'preparing' ? 'Starting voice input…' : voice.phase === 'finishing' ? 'Finishing voice input…' : 'Listening… Tap the microphone to finish.'}</Text>}
      </View>
      {!!voice.error && <Text accessibilityRole="alert" style={styles.error}>{voice.error}</Text>}
      {voice.needsSettings && <Pressable accessibilityRole="button" onPress={() => { void Linking.openSettings(); }} style={styles.messageAction}><Text style={styles.actionText}>Open Settings</Text></Pressable>}
    </View>}
  </View>;
}

const makeStyles = (colors: ThemeColors) => StyleSheet.create({
  workspace: { flex: 1 }, thread: { flex: 1 }, messages: { width: '100%', maxWidth: 800, alignSelf: 'center', paddingHorizontal: 20, paddingTop: 24, paddingBottom: 24, gap: 28 }, emptyMessages: { flexGrow: 1 },
  searchHit: { backgroundColor: colors.accentSoft, borderRadius: 12 },
  message: { width: '100%' }, userMessage: { alignItems: 'flex-end' },
  // Native Markdown can overmeasure an auto-width code block. Assign its width
  // so Fabric accepts the measurement and the prompt stays visible.
  userBubble: { width: '88%', paddingHorizontal: 18, paddingVertical: 12, borderRadius: 24, backgroundColor: colors.raised },
  userActions: { flexDirection: 'row', alignSelf: 'flex-end' }, editing: { flexDirection: 'row', alignItems: 'center', paddingLeft: 12, borderLeftWidth: 2, borderColor: colors.action },
  thinking: { flexDirection: 'row', alignItems: 'center', gap: 10, minHeight: 32 }, muted: { fontSize: 13, color: colors.muted, lineHeight: 20 },
  messageAction: { minWidth: 44, minHeight: 44, paddingHorizontal: 10, flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 7 }, actionText: { color: colors.muted, fontSize: 12 },
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
  jump: { position: 'absolute', alignSelf: 'center', bottom: 12, width: 44, height: 44, borderRadius: 22, backgroundColor: colors.raised, borderWidth: 1, borderColor: colors.line, alignItems: 'center', justifyContent: 'center', elevation: 4, boxShadow: '0 2px 8px #00000026' }, disabled: { opacity: 0.38 },
});
