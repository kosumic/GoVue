import { memo, useEffect, useRef, useState } from 'react';
import { AccessibilityInfo, ActivityIndicator, Animated, FlatList, Pressable, StyleSheet, Text, TextInput, View } from 'react-native';
import Clipboard from '@react-native-clipboard/clipboard';
import { ArrowDown, ArrowUp, Brain, Captions, ChevronDown, Copy, MessageCircle, RotateCw, Sparkles, Square, X } from 'lucide-react-native';
import { colors } from '../theme';
import type { ChatController, ChatMessage, ChatTranscript } from '../ai/useChats';
import { AiMarkdown } from './AiMarkdown';
import { AiReasoning, reasoningName } from './AiReasoning';

const Message = memo(function Message({ message, last, busy, onRetry, reduceMotion }: { message: ChatMessage; last: boolean; busy: boolean; onRetry: () => void; reduceMotion: boolean }) {
  const entrance = useRef(new Animated.Value(reduceMotion ? 1 : 0)).current;
  useEffect(() => {
    const animation = Animated.timing(entrance, { toValue: 1, duration: reduceMotion ? 0 : 180, useNativeDriver: true });
    animation.start(); return () => animation.stop();
  }, [entrance, reduceMotion]);
  const user = message.role === 'user';
  const [showReasoning, setShowReasoning] = useState(false);
  return <Animated.View style={[styles.message, user && styles.userMessage, { opacity: entrance, transform: [{ translateY: entrance.interpolate({ inputRange: [0, 1], outputRange: [6, 0] }) }] }]}>
    {user ? <View style={styles.userBubble}><Text selectable style={styles.userText}>{message.content}</Text></View> : <>
      <View style={styles.assistantLabel}><Sparkles size={13} color={colors.accent} /><Text style={styles.assistantName}>LingoPlay</Text></View>
      {!!message.reasoning && <View><Pressable accessibilityRole="button" accessibilityState={{ expanded: showReasoning }} onPress={() => setShowReasoning(!showReasoning)} style={styles.messageAction}><Text style={styles.actionText}>{showReasoning ? 'Hide reasoning' : 'Show reasoning'}</Text></Pressable>{showReasoning && <AiMarkdown content={message.reasoning} streaming={message.status === 'streaming' && busy} />}</View>}
      {message.content ? <AiMarkdown content={message.content} streaming={message.status === 'streaming' && busy} /> : message.status === 'streaming' && busy ? <View accessibilityLiveRegion="polite" style={styles.thinking}><ActivityIndicator size="small" color={colors.accent} /><Text style={styles.muted}>Thinking…</Text></View> : null}
      {message.error && <Text accessibilityRole="alert" style={styles.error}>{message.error}</Text>}
      {message.status === 'stopped' && !message.error && <Text style={styles.muted}>Response stopped</Text>}
      {!!message.model && message.status !== 'streaming' && <Text style={styles.actionText}>{message.providerLabel} · {message.model}{message.reasoningEffort ? ` · Reasoning: ${reasoningName(message.reasoningEffort)}` : ''}{message.usage?.inputTokens != null ? ` · ${message.usage.inputTokens.toLocaleString()} in` : ''}{message.usage?.outputTokens != null ? ` / ${message.usage.outputTokens.toLocaleString()} out` : ''}</Text>}
      {message.status !== 'streaming' && !!(message.usage?.latestContextTokens || message.usage?.cacheReadTokens || message.usage?.cacheCreationTokens) && <Text style={styles.actionText}>{[message.usage.latestContextTokens ? `Context: ${message.usage.latestContextTokens.toLocaleString()}` : '', message.usage.cacheReadTokens ? `Cache read: ${message.usage.cacheReadTokens.toLocaleString()}` : '', message.usage.cacheCreationTokens ? `Cache write: ${message.usage.cacheCreationTokens.toLocaleString()}` : ''].filter(Boolean).join(' · ')}</Text>}
      {message.status !== 'streaming' && <View style={styles.messageActions}>
        {!!message.content && <Pressable accessibilityRole="button" accessibilityLabel="Copy response" onPress={() => Clipboard.setString(message.content)} style={styles.messageAction}><Copy size={16} color={colors.muted} /></Pressable>}
        {last && <Pressable accessibilityRole="button" accessibilityLabel="Regenerate response" disabled={busy} onPress={onRetry} style={[styles.messageAction, busy && styles.disabled]}><RotateCw size={16} color={colors.muted} /><Text style={styles.actionText}>{message.status === 'completed' ? 'Regenerate' : 'Try again'}</Text></Pressable>}
      </View>}
    </>}
  </Animated.View>;
});

export function AiChat({ chat, draft, onDraftChange, attachment, availableTranscript, connected, loading, onSend, onRetry, onAttach, onDetach, onSummarize, onModels, reasoning, providerId, model }: {
  chat: ChatController;
  draft: string;
  onDraftChange: (value: string) => void;
  attachment: ChatTranscript | null;
  availableTranscript: ChatTranscript | null;
  connected: boolean;
  loading: boolean;
  onSend: () => void;
  onRetry: () => void;
  onAttach: () => void;
  onDetach: () => void;
  onSummarize: () => void;
  onModels: () => void;
  reasoning: string;
  providerId?: string;
  model?: string;
}) {
  const list = useRef<FlatList<ChatMessage>>(null);
  const input = useRef<TextInput>(null);
  const nearBottom = useRef(true);
  const frame = useRef<number | null>(null);
  const [showJump, setShowJump] = useState(false);
  const [reduceMotion, setReduceMotion] = useState(false);
  const [showReasoning, setShowReasoning] = useState(false);
  const messages = chat.conversation?.messages ?? [];
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
  const canSend = connected && !!draft.trim() && !loading && !chat.settingsBusy;
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
          <Sparkles size={32} strokeWidth={1.5} color={colors.accent} />
          <Text accessibilityRole="header" style={styles.welcomeTitle}>Let’s talk.</Text>
          <Text style={styles.welcomeHelp}>{connected ? 'Ask a question or explore a video.' : 'Connect a model to start a conversation.'}</Text>
          {connected ? <View style={styles.suggestions}>
            {availableTranscript && <Pressable accessibilityRole="button" onPress={onSummarize} style={styles.suggestion}><Captions size={17} color={colors.accent} /><Text style={styles.suggestionText}>Summarize this video</Text></Pressable>}
            <Pressable accessibilityRole="button" onPress={() => { onDraftChange('Explain this phrase: '); input.current?.focus(); }} style={styles.suggestion}><MessageCircle size={17} color={colors.muted} /><Text style={styles.suggestionText}>Explain a phrase</Text></Pressable>
          </View> : <Pressable accessibilityRole="button" onPress={onModels} style={styles.connect}><Text style={styles.connectText}>Choose a model</Text></Pressable>}
        </View>}
        renderItem={({ item, index }) => <Message message={item} last={index === messages.length - 1} busy={chat.busy} onRetry={onRetry} reduceMotion={reduceMotion} />}
      />
      {showJump && <Pressable accessibilityRole="button" accessibilityLabel="Jump to latest message" onPress={() => scrollToEnd(!reduceMotion)} style={styles.jump}><ArrowDown size={20} color={colors.ink} /></Pressable>}
    </View>
    {!!chat.error && <Text accessibilityRole="alert" style={[styles.error, styles.composerError]}>{chat.error}</Text>}
    <View style={styles.composerDock}>
      {attachment && <View style={styles.attachment}>
        <Captions size={17} color={colors.accent} /><View style={styles.attachmentText}><Text numberOfLines={1} style={styles.attachmentTitle}>{attachment.title}</Text><Text style={styles.attachmentHelp}>{attachment.language} transcript</Text></View>
        <Pressable accessibilityRole="button" accessibilityLabel="Remove transcript attachment" disabled={chat.busy} onPress={onDetach} style={styles.messageAction}><X size={17} color={colors.muted} /></Pressable>
      </View>}
      <View style={styles.composer}>
        <TextInput ref={input} accessibilityLabel="Message LingoPlay" placeholder={attachment ? 'Ask about this video…' : 'Message LingoPlay…'} placeholderTextColor={colors.muted}
          value={draft} onChangeText={onDraftChange} multiline maxLength={8000} style={styles.input} selectionColor={colors.accent} editable={!loading} />
        {showReasoning && providerId && model && <AiReasoning id={providerId} model={model} value={chat.reasoningEffort} busy={chat.settingsBusy} onSelect={level => { void chat.setReasoning(level).then(saved => { if (saved) setShowReasoning(false); }); }} />}
        <View style={styles.composerTools}>
          <Pressable accessibilityRole="button" accessibilityLabel={`Reasoning effort: ${reasoning}`} accessibilityState={{ expanded: showReasoning, disabled: !connected || chat.busy || loading || chat.settingsBusy }} disabled={!connected || chat.busy || loading || chat.settingsBusy} onPress={() => setShowReasoning(!showReasoning)} style={[styles.reasoningButton, (!connected || chat.busy || loading || chat.settingsBusy) && styles.disabled]}><Brain size={16} color={colors.accent} /><Text numberOfLines={1} style={styles.reasoningText}>Reasoning · {reasoning}</Text><ChevronDown size={12} color={colors.muted} style={showReasoning ? styles.expanded : undefined} /></Pressable>
          {availableTranscript && !attachment ? <Pressable accessibilityRole="button" accessibilityLabel="Attach current transcript" disabled={chat.busy || loading} onPress={onAttach} style={[styles.attachButton, chat.busy && styles.disabled]}><Captions size={18} color={colors.muted} /><Text style={styles.actionText}>Transcript</Text></Pressable> : <View style={styles.spacer} />}
          <Pressable accessibilityRole="button" accessibilityLabel={chat.busy ? 'Stop response' : 'Send message'} disabled={!chat.busy && !canSend}
            onPress={chat.busy ? chat.stop : send} style={({ pressed }) => [styles.send, (!chat.busy && !canSend || pressed) && styles.disabled]}>
            {chat.busy ? <Square size={16} fill={colors.surface} color={colors.surface} /> : <ArrowUp size={22} strokeWidth={2.3} color={colors.surface} />}
          </Pressable>
        </View>
      </View>
    </View>
  </View>;
}

const styles = StyleSheet.create({
  workspace: { flex: 1 }, thread: { flex: 1 }, messages: { paddingHorizontal: 20, paddingTop: 20, paddingBottom: 20, gap: 24 }, emptyMessages: { flexGrow: 1 },
  message: { width: '100%' }, userMessage: { alignItems: 'flex-end' },
  userBubble: { maxWidth: '88%', paddingHorizontal: 16, paddingVertical: 12, borderRadius: 20, borderTopRightRadius: 6, backgroundColor: colors.accentSoft },
  userText: { color: colors.ink, fontSize: 15, lineHeight: 23 }, assistantLabel: { flexDirection: 'row', alignItems: 'center', gap: 6, marginBottom: 12 }, assistantName: { fontSize: 11, color: colors.muted, fontWeight: '500' },
  thinking: { flexDirection: 'row', alignItems: 'center', gap: 10, minHeight: 32 }, muted: { fontSize: 13, color: colors.muted, lineHeight: 20 },
  messageActions: { flexDirection: 'row', alignItems: 'center', gap: 4, marginTop: 4, marginLeft: -10 }, messageAction: { minWidth: 44, minHeight: 44, paddingHorizontal: 10, flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 7 }, actionText: { color: colors.muted, fontSize: 12 },
  error: { color: colors.error, fontSize: 13, lineHeight: 20, marginTop: 8 }, composerError: { paddingHorizontal: 20, paddingBottom: 8 },
  welcome: { flex: 1, alignItems: 'center', justifyContent: 'center', paddingHorizontal: 20, paddingVertical: 28, gap: 16 },
  welcomeTitle: { color: colors.ink, fontSize: 30, fontWeight: '600', letterSpacing: -0.8 }, welcomeHelp: { color: colors.muted, fontSize: 14, lineHeight: 22, textAlign: 'center', maxWidth: 260 },
  suggestions: { marginTop: 16, gap: 4 }, suggestion: { flexDirection: 'row', alignItems: 'center', gap: 10, minHeight: 48, paddingHorizontal: 14 }, suggestionText: { color: colors.ink, fontSize: 14 },
  connect: { marginTop: 8, minHeight: 46, paddingHorizontal: 22, alignItems: 'center', justifyContent: 'center', borderRadius: 14, backgroundColor: colors.accent }, connectText: { color: colors.surface, fontSize: 14, fontWeight: '600' },
  composerDock: { paddingHorizontal: 14, paddingTop: 8, paddingBottom: 8, gap: 8 }, composer: { backgroundColor: colors.surface, borderWidth: StyleSheet.hairlineWidth, borderColor: colors.line, borderRadius: 25, paddingBottom: 7 },
  input: { color: colors.ink, fontSize: 15, lineHeight: 23, minHeight: 50, maxHeight: 140, textAlignVertical: 'top', paddingTop: 15, paddingBottom: 8, paddingHorizontal: 16 },
  composerTools: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', paddingLeft: 7, paddingRight: 7, gap: 10 }, spacer: { flex: 1 },
  reasoningButton: { flexDirection: 'row', alignItems: 'center', gap: 5, minHeight: 44, paddingHorizontal: 9, flexShrink: 1 }, reasoningText: { color: colors.accent, fontSize: 12, flexShrink: 1 },
  expanded: { transform: [{ rotate: '180deg' }] },
  attachButton: { minHeight: 44, flexDirection: 'row', alignItems: 'center', gap: 7, paddingHorizontal: 9 }, send: { width: 42, height: 42, borderRadius: 21, backgroundColor: colors.accent, alignItems: 'center', justifyContent: 'center' },
  attachment: { flexDirection: 'row', alignItems: 'center', gap: 9, paddingLeft: 12, borderRadius: 14, backgroundColor: colors.accentSoft }, attachmentText: { flex: 1, paddingVertical: 9, gap: 2 }, attachmentTitle: { color: colors.ink, fontSize: 12, fontWeight: '500' }, attachmentHelp: { color: colors.muted, fontSize: 10 },
  jump: { position: 'absolute', alignSelf: 'center', bottom: 8, width: 40, height: 40, borderRadius: 20, backgroundColor: colors.surface, borderWidth: 1, borderColor: colors.line, alignItems: 'center', justifyContent: 'center' }, disabled: { opacity: 0.38 },
});
