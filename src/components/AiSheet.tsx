import { useEffect, useRef, useState, type ReactNode } from 'react';
import { Alert, BackHandler, Keyboard, KeyboardAvoidingView, Platform, Pressable, StyleSheet, Text, View } from 'react-native';
import { ChevronDown, ChevronLeft, Menu, Play, Search, SquarePen, X } from 'lucide-react-native';
import { useAi } from '../ai/useAi';
import type { Transcript } from '../youtube/transcript';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import { IconButton } from './IconButton';
import { AiChat } from './AiChat';
import type { ChatViewportStore } from './useChatViewport';
import { AiProviders } from './AiProviders';
import { AiModels } from './AiModels';
import { AiUsageSheet } from './AiUsageSheet';
import { reasoningName } from './AiReasoning';
import { useChats, type ChatTranscript, type SavedChat } from '../ai/useChats';
import { AiHistoryDrawer } from './AiHistoryDrawer';
import { GamePractice } from './GamePractice';
import { extractTranscript, restoreTranscript } from '../core/transcripts';
import { TranscriptPanel } from './TranscriptPanel';
import { useTranscriptAudio } from '../ai/useGameAudio';
import { usePlayerTranscript } from '../player/usePlayerTranscript';

function ChatTranscript({ data, chatId, onClose, onBeforeAudio }: { data: Transcript; chatId: string; onClose: () => void; onBeforeAudio?: () => Promise<void> | void }) {
  const reader = usePlayerTranscript(data.videoId, data);
  const [line, setLine] = useState<{ source: Transcript; index: number } | null>(null);
  const audio = useTranscriptAudio({ chatId, videoId: data.videoId, trackId: line?.source.trackId ?? data.trackId }, onBeforeAudio);
  useEffect(() => {
    if (line) void audio.play(`cue-${line.index}`).catch(() => {});
  }, [line]);
  return <TranscriptPanel data={reader.data ?? data} comparison={reader.comparison} trackIds={reader.trackIds}
    comparisonLoading={reader.comparisonLoading} comparisonError={reader.comparisonError} onSelectTrack={reader.selectTrack}
    loading={reader.loading} error={reader.error} cached={false} pauseAfterLine={false} onClose={onClose}
    position={audio.phase !== 'idle' && line !== null ? line.source.segments[line.index]?.start ?? null : null} audio={audio}
    onBeforeWordAudio={onBeforeAudio} onPlayLine={(index, source = data) => setLine({ source, index })} />;
}

function Button({ title, onPress, disabled = false }: { title: string; onPress: () => void; disabled?: boolean }) {
  const styles = useThemeStyles(makeStyles);
  return <Pressable accessibilityRole="button" disabled={disabled} onPress={onPress} style={[styles.button, disabled && styles.disabled]}><Text style={styles.buttonText}>{title}</Text></Pressable>;
}

export function AiSheet({ visible, onClose, transcript, videoId, onOpenVideo, browserControl, onBeforeGameAudio, keyboardVerticalOffset }: { visible: boolean; onClose: () => void; transcript: Transcript | null; videoId: string | null; onOpenVideo?: (videoId: string, start?: number) => void; browserControl?: ReactNode; onBeforeGameAudio?: () => Promise<void> | void; keyboardVerticalOffset: number }) {
  const colors = useThemeColors();
  const styles = useThemeStyles(makeStyles);
  const [chatVideoId, setChatVideoId] = useState(videoId);
  const [contextLoading, setContextLoading] = useState(false);
  const [contextError, setContextError] = useState('');
  const [contextRetry, setContextRetry] = useState(0);
  const [contextTranscript, setContextTranscript] = useState<Transcript | null>(null);
  const [transcriptVisible, setTranscriptVisible] = useState(false);
  const contextAbort = useRef<AbortController | null>(null);
  const ai = useAi(visible, chatVideoId || '');
  const chats = useChats(visible, chatVideoId || '');
  const chatViewports = useRef<ChatViewportStore>(new Map());
  const [page, setPage] = useState<'chat' | 'providers' | 'models' | 'practice'>('chat');
  const [historyVisible, setHistoryVisible] = useState(false);
  const [usageVisible, setUsageVisible] = useState(false);
  const [find, setFind] = useState<{ chatId: string; query: string } | null>(null);
  useEffect(() => {
    if (!visible) return;
    const subscription = BackHandler.addEventListener('hardwareBackPress', () => {
      if (transcriptVisible) setTranscriptVisible(false);
      else if (historyVisible) setHistoryVisible(false);
      else if (find) closeFind();
      else if (page !== 'chat') showPage('chat');
      else close();
      return true;
    });
    return () => subscription.remove();
  }, [visible, transcriptVisible, historyVisible, find, page, onClose]);
  useEffect(() => { if (!visible) { setUsageVisible(false); setHistoryVisible(false); setTranscriptVisible(false); setFind(null); } }, [visible]);
  useEffect(() => { setTranscriptVisible(false); }, [chatVideoId]);
  const [prompt, setPrompt] = useState('');
  const [newChatVersion, setNewChatVersion] = useState(0);
  const [attachment, setAttachment] = useState<ChatTranscript | null>(null);
  const targets = [...ai.providers, ...ai.groups.map(g => ({ id: g.id, label: g.name, model: g.name, connected: !!g.connected, enabled: true, needsReauth: false }))];
  const selected = targets.find(p => p.id === (chats.conversation?.providerId || ai.activeId));
  const selectedModel = chats.conversation?.model || selected?.model;
  const savedReasoning = ai.providers.find(p => p.id === selected?.id)?.models.find(m => m.id === selectedModel)?.reasoning || ai.groups.find(g => g.id === selected?.id)?.reasoning;
  const availableTranscript = chats.conversation?.transcript?.videoId === chatVideoId ? chats.conversation.transcript : null;
  const contextReady = !chatVideoId || (!contextLoading && !contextError && chats.conversation?.videoId === chatVideoId && !!availableTranscript);
  useEffect(() => { if (chats.conversation) setAttachment(chats.conversation.transcript); }, [chats.conversation?.id, chats.conversation?.transcript]);
  useEffect(() => { if (visible) setChatVideoId(videoId ?? chats.conversation?.videoId ?? null); else setPage('chat'); }, [visible, videoId]);
  useEffect(() => {
    setContextTranscript(null);
    if (!visible || !chatVideoId) { setContextLoading(false); setContextError(''); return; }
    const controller = new AbortController();
    contextAbort.current = controller;
    setContextLoading(true); setContextError(''); setAttachment(null); setPrompt('');
    void (async () => {
      const chat = await chats.openVideo(chatVideoId, transcript?.videoId === chatVideoId ? transcript.title : undefined);
      if (!chat || controller.signal.aborted) return;
      const trackId = chat.transcript?.trackId;
      let data = transcript?.videoId === chatVideoId && (!trackId || transcript.trackId === trackId) ? transcript : null;
      if (!data) {
        const cached = await restoreTranscript(chatVideoId).catch(() => null);
        if (controller.signal.aborted) return;
        data = cached && (!trackId || cached.trackId === trackId) ? cached : await extractTranscript(chatVideoId, trackId ?? 'default', controller.signal);
      }
      if (controller.signal.aborted) return;
      await chats.openVideo(chatVideoId, data.title, { videoId: data.videoId, trackId: data.trackId, title: data.title, language: data.language });
      if (!controller.signal.aborted) { setContextTranscript(data); setContextLoading(false); }
    })().catch(error => {
      if (!controller.signal.aborted) { setContextError(error instanceof Error ? error.message : 'Could not load this video’s transcript.'); setContextLoading(false); }
    });
    return () => { controller.abort(); if (contextAbort.current === controller) contextAbort.current = null; };
  }, [visible, chatVideoId, contextRetry]);

  function close() { Keyboard.dismiss(); onClose(); }
  function closeFind() { Keyboard.dismiss(); setFind(null); }
  function showPage(next: typeof page) {
    Keyboard.dismiss(); setHistoryVisible(false); setFind(null); ai.clear();
    if (page === 'practice' && next === 'chat') void chats.refreshGames().catch(() => {});
    setPage(next);
  }
  function newChat() { contextAbort.current?.abort(); setChatVideoId(null); chats.newChat(); setNewChatVersion(value => value + 1); setPrompt(''); setAttachment(null); showPage('chat'); }
  async function choose(id: string, model: string) {
    const p = targets.find(p => p.id === id);
    if (!p?.connected || !p.enabled || p.needsReauth) { showPage('providers'); return; }
    if (await ai.select(id, model)) {
      if (chatVideoId && chats.conversation?.videoId === chatVideoId) {
        if (!await chats.selectModel(id)) return;
      } else if (chats.conversation && (chats.conversation.providerId !== id || chats.conversation.model !== model)) newChat();
      showPage('chat');
    }
  }
  async function openChat(id: string, query?: string) {
    contextAbort.current?.abort(); setContextLoading(false);
    setPrompt('');
    const saved = chats.history.find(chat => chat.id === id);
    if (saved?.videoId) {
      setChatVideoId(saved.videoId); setContextRetry(value => value + 1); showPage('chat');
      if (query) setFind({ chatId: id, query });
      return;
    }
    const chat = await chats.open(id);
    if (!chat) return;
    setChatVideoId(chat.videoId ?? chat.transcript?.videoId ?? null);
    setAttachment(chat.transcript);
    if (targets.some(p => p.id === chat.providerId)) await ai.select(chat.providerId);
    showPage('chat');
    if (query) setFind({ chatId: chat.id, query });
  }
  function deleteChat(chat: SavedChat) {
    Alert.alert('Delete chat?', chat.title, [{ text: 'Keep', style: 'cancel' }, { text: 'Delete', style: 'destructive', onPress: () => { if (chats.conversation?.id === chat.id) newChat(); void chats.remove(chat.id).then(() => chatViewports.current.delete(chat.id)); } }]);
  }
  async function sendMessage(editMessageId?: string) {
    if (!contextReady || !selected?.connected || !selected.enabled || selected.needsReauth || !selectedModel || !prompt.trim()) return false;
    const message = prompt.trim(); setPrompt('');
    const sent = await chats.send(selected.id, message, attachment, undefined, false, undefined, editMessageId);
    if (!sent) setPrompt(current => current || message);
    return sent;
  }
  function attachCurrent() {
    if (!availableTranscript) return;
    if (chats.conversation?.transcript && chats.conversation.transcript.videoId !== availableTranscript.videoId) newChat();
    setAttachment(availableTranscript);
  }
  function summarize() {
    if (!contextReady || !availableTranscript || !selected?.connected) return;
    setAttachment(availableTranscript);
    void chats.send(selected.id, 'Summarize this video', availableTranscript, 'summary');
  }

  if (!visible) return null;
  // Share the app window so the mounted VideoView can float above chat and
  // enter system PiP without a full-screen dialog covering either surface.
  return <View style={styles.overlay}>
      {/* Keyboard coordinates include the safe area above this app-window overlay. */}
      <KeyboardAvoidingView style={styles.safe} behavior={Platform.OS === 'ios' ? 'padding' : 'height'} keyboardVerticalOffset={keyboardVerticalOffset}>
        <View style={styles.safe} accessibilityElementsHidden={historyVisible || transcriptVisible} importantForAccessibility={historyVisible || transcriptVisible ? 'no-hide-descendants' : 'auto'}>
        <View style={styles.heading}>
          {page === 'chat' ? <IconButton icon={Menu} label="Open conversations" onPress={() => { Keyboard.dismiss(); setFind(null); void chats.refresh().catch(() => {}); setHistoryVisible(true); }} /> : <IconButton icon={ChevronLeft} label="Back to chat" onPress={() => { ai.stop(); showPage('chat'); }} />}
          {page === 'chat' ? <Pressable accessibilityRole="button" accessibilityLabel="Choose AI model" disabled={!!chatVideoId && chats.conversation?.videoId !== chatVideoId} onPress={() => showPage('models')} style={styles.chatTitle}>
            <Text numberOfLines={1} style={styles.brand}>{chats.conversation?.title || 'GoVue'}</Text>
            <View style={styles.modelLabel}><Text numberOfLines={1} style={styles.modelName}>{selectedModel?.replace(/^gpt-/, 'GPT-') || 'Choose a model'}</Text><ChevronDown size={12} color={colors.muted} /></View>
          </Pressable> : <Text accessibilityRole="header" style={styles.pageTitle}>{page === 'models' ? 'Choose a model' : page === 'practice' ? 'Practice' : 'Providers'}</Text>}
          {page === 'chat' && !!chats.conversation?.messages.length && <IconButton icon={Search} label="Find in chat" active={find?.chatId === chats.conversation.id} onPress={() => { Keyboard.dismiss(); setFind(current => current?.chatId === chats.conversation!.id ? null : { chatId: chats.conversation!.id, query: '' }); }} />}
          {page === 'chat' && <IconButton icon={SquarePen} label="New chat" onPress={newChat} />}
          <IconButton icon={X} label="Close AI assistant" onPress={close} />
        </View>
        {page === 'chat' && chatVideoId && onOpenVideo && <Pressable accessibilityRole="button" accessibilityLabel="Open video" onPress={() => { Keyboard.dismiss(); onOpenVideo(chatVideoId); }} style={styles.videoLink}>
          <Play size={16} color={colors.ink} /><Text style={styles.videoLinkText}>Open video</Text>
        </Pressable>}
        {browserControl}
        {page === 'chat' ? <>
          {!!contextError && <View style={styles.banner}><Text accessibilityRole="alert" style={styles.help}>{contextError}</Text><Button title="Retry transcript" onPress={() => setContextRetry(value => value + 1)} /></View>}
          {chats.conversation?.providerId && ai.ready && !selected?.connected && <View style={styles.banner}><Text style={styles.help}>Reconnect {chats.conversation.providerLabel} to continue this chat.</Text><Button title="Providers" onPress={() => showPage('providers')} /></View>}
          <AiChat key={newChatVersion} viewports={chatViewports.current} chat={chats} draft={prompt} onDraftChange={setPrompt} attachment={attachment} availableTranscript={availableTranscript} voiceEnabled={visible && !historyVisible && !usageVisible && !transcriptVisible && !find}
            findQuery={find?.chatId === chats.conversation?.id ? find?.query : undefined} onCloseFind={closeFind}
            onBeforeGameAudio={onBeforeGameAudio}
            connected={!!selected?.connected && !!selected?.enabled && !selected?.needsReauth && !!selectedModel && !ai.busy} loading={!ai.ready || !chats.ready || chats.loading || contextLoading} contextReady={contextReady}
            reasoning={reasoningName(chats.reasoningEffort ?? savedReasoning)} providerId={selected?.id} model={selectedModel}
            onSend={sendMessage} onRetry={messageId => { if (contextReady && selected?.connected && selected.enabled && !selected.needsReauth && selectedModel) void chats.send(selected.id, '', attachment, undefined, true, undefined, undefined, messageId); }}
            onAttach={attachCurrent} onDetach={chatVideoId ? undefined : () => setAttachment(null)} onOpenTranscript={() => { Keyboard.dismiss(); setTranscriptVisible(true); }} onSummarize={summarize} onModels={() => showPage('models')} />
        </> : page === 'practice' ? <GamePractice onBeforeAudio={onBeforeGameAudio} onOpenSource={onOpenVideo} /> : page === 'models' ? <AiModels ai={ai} selectedId={selected?.id} selectedModel={selectedModel} onSelect={(id, model) => { void choose(id, model); }} onManage={() => showPage('providers')} /> : <AiProviders ai={ai} onUsage={() => { Keyboard.dismiss(); setUsageVisible(true); }} />}
        </View>
        <AiHistoryDrawer visible={historyVisible} chats={chats.history} selectedId={chats.conversation?.id} error={chats.error}
          onClose={() => setHistoryVisible(false)} onNew={newChat} onOpen={(id, query) => { void openChat(id, query); }} onDelete={deleteChat}
          onChange={chats.change}
          onProviders={() => showPage('providers')} onUsage={() => { setHistoryVisible(false); setUsageVisible(true); }} onPractice={() => showPage('practice')} />
        <AiUsageSheet visible={visible && usageVisible} chatId={chats.conversation?.id} onClose={() => setUsageVisible(false)} />
        {visible && transcriptVisible && <View style={styles.transcriptReader} accessibilityViewIsModal>
          {contextTranscript && chats.conversation?.transcript?.videoId === contextTranscript.videoId && chats.conversation.transcript.trackId === contextTranscript.trackId
            ? <ChatTranscript key={`${chats.conversation.id}:${contextTranscript.videoId}:${contextTranscript.trackId}`} data={contextTranscript} chatId={chats.conversation.id} onBeforeAudio={onBeforeGameAudio} onClose={() => setTranscriptVisible(false)} />
            : <TranscriptPanel data={null} loading={contextLoading} error={contextError || null} cached={false} position={null} pauseAfterLine={false} onClose={() => setTranscriptVisible(false)} />}
        </View>}
      </KeyboardAvoidingView>
  </View>;
}

const makeStyles = (colors: ThemeColors) => StyleSheet.create({
  overlay: { ...StyleSheet.absoluteFill, zIndex: 1, backgroundColor: colors.background },
  safe: { flex: 1, backgroundColor: colors.background },
  transcriptReader: { position: 'absolute', top: 0, right: 0, bottom: 0, left: 0, backgroundColor: colors.background },
  heading: { flexDirection: 'row', alignItems: 'center', paddingHorizontal: 6, paddingVertical: 5 },
  chatTitle: { flex: 1, paddingHorizontal: 5, paddingVertical: 4, gap: 2 }, brand: { color: colors.ink, fontSize: 17, fontWeight: '600', letterSpacing: -0.3 },
  modelLabel: { flexDirection: 'row', alignItems: 'center', gap: 4 }, modelName: { flexShrink: 1, color: colors.muted, fontSize: 11 }, pageTitle: { flex: 1, color: colors.ink, fontSize: 19, fontWeight: '600' },
  videoLink: { alignSelf: 'flex-start', flexDirection: 'row', alignItems: 'center', gap: 8, minHeight: 44, marginHorizontal: 20, marginBottom: 6, paddingHorizontal: 14, borderRadius: 22, backgroundColor: colors.raised }, videoLinkText: { fontSize: 13, color: colors.ink, fontWeight: '500' },
  help: { color: colors.muted, fontSize: 13, lineHeight: 20 },
  button: { backgroundColor: colors.accentSoft, borderRadius: 10, padding: 14, alignItems: 'center' }, buttonText: { color: colors.accent, fontSize: 14, fontWeight: '600' }, disabled: { opacity: 0.45 },
  banner: { paddingHorizontal: 20, paddingBottom: 10, gap: 8 },
});
