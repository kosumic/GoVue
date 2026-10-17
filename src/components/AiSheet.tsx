import { useEffect, useRef, useState, type ReactNode } from 'react';
import { Alert, Keyboard, KeyboardAvoidingView, Modal, Platform, Pressable, StyleSheet, Text, View } from 'react-native';
import { ChevronDown, ChevronLeft, Menu, Play, SquarePen, X } from 'lucide-react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { useAi } from '../ai/useAi';
import type { Transcript } from '../youtube/transcript';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import { IconButton } from './IconButton';
import { AiChat } from './AiChat';
import { AiProviders } from './AiProviders';
import { AiModels } from './AiModels';
import { AiUsageSheet } from './AiUsageSheet';
import { reasoningName } from './AiReasoning';
import { useChats, type ChatTranscript, type SavedChat } from '../ai/useChats';
import { AiHistoryDrawer } from './AiHistoryDrawer';
import { extractTranscript, restoreTranscript } from '../core/transcripts';
import { TranscriptPanel } from './TranscriptPanel';

function Button({ title, onPress, disabled = false }: { title: string; onPress: () => void; disabled?: boolean }) {
  const styles = useThemeStyles(makeStyles);
  return <Pressable accessibilityRole="button" disabled={disabled} onPress={onPress} style={[styles.button, disabled && styles.disabled]}><Text style={styles.buttonText}>{title}</Text></Pressable>;
}

export function AiSheet({ visible, onClose, transcript, videoId, onOpenVideo, browserControl }: { visible: boolean; onClose: () => void; transcript: Transcript | null; videoId: string | null; onOpenVideo?: (videoId: string) => void; browserControl?: ReactNode }) {
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
  const [page, setPage] = useState<'chat' | 'providers' | 'models'>('chat');
  const [historyVisible, setHistoryVisible] = useState(false);
  const [usageVisible, setUsageVisible] = useState(false);
  useEffect(() => { if (!visible) { setUsageVisible(false); setHistoryVisible(false); setTranscriptVisible(false); } }, [visible]);
  useEffect(() => { setTranscriptVisible(false); }, [chatVideoId]);
  const [prompt, setPrompt] = useState('');
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

  function showPage(next: typeof page) { Keyboard.dismiss(); setHistoryVisible(false); ai.clear(); setPage(next); }
  function newChat() { contextAbort.current?.abort(); setChatVideoId(null); chats.newChat(); setPrompt(''); setAttachment(null); showPage('chat'); }
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
  async function openChat(id: string) {
    contextAbort.current?.abort(); setContextLoading(false);
    setPrompt('');
    const saved = chats.history.find(chat => chat.id === id);
    if (saved?.videoId) {
      setChatVideoId(saved.videoId); setContextRetry(value => value + 1); showPage('chat'); return;
    }
    const chat = await chats.open(id);
    if (!chat) return;
    setChatVideoId(chat.videoId ?? chat.transcript?.videoId ?? null);
    setAttachment(chat.transcript);
    if (targets.some(p => p.id === chat.providerId)) await ai.select(chat.providerId);
    showPage('chat');
  }
  function deleteChat(chat: SavedChat) {
    Alert.alert('Delete chat?', chat.title, [{ text: 'Keep', style: 'cancel' }, { text: 'Delete', style: 'destructive', onPress: () => { if (chats.conversation?.id === chat.id) newChat(); void chats.remove(chat.id); } }]);
  }
  async function sendMessage() {
    if (!contextReady || !selected?.connected || !selected.enabled || selected.needsReauth || !selectedModel || !prompt.trim()) return;
    const message = prompt.trim(); setPrompt('');
    if (!await chats.send(selected.id, message, attachment)) setPrompt(current => current || message);
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

  return <Modal visible={visible} animationType="slide" presentationStyle="fullScreen" onRequestClose={() => transcriptVisible ? setTranscriptVisible(false) : historyVisible ? setHistoryVisible(false) : page === 'chat' ? onClose() : showPage('chat')}>
    <SafeAreaView style={styles.safe}>
      <KeyboardAvoidingView style={styles.safe} behavior={Platform.OS === 'ios' ? 'padding' : 'height'}>
        <View style={styles.safe} accessibilityElementsHidden={historyVisible || transcriptVisible} importantForAccessibility={historyVisible || transcriptVisible ? 'no-hide-descendants' : 'auto'}>
        <View style={styles.heading}>
          {page === 'chat' ? <IconButton icon={Menu} label="Open conversations" onPress={() => { Keyboard.dismiss(); void chats.refresh().catch(() => {}); setHistoryVisible(true); }} /> : <IconButton icon={ChevronLeft} label="Back to chat" onPress={() => { ai.stop(); showPage('chat'); }} />}
          {page === 'chat' ? <Pressable accessibilityRole="button" accessibilityLabel="Choose AI model" disabled={!!chatVideoId && chats.conversation?.videoId !== chatVideoId} onPress={() => showPage('models')} style={styles.chatTitle}>
            <Text numberOfLines={1} style={styles.brand}>{chats.conversation?.title || 'LingoPlay'}</Text>
            <View style={styles.modelLabel}><Text numberOfLines={1} style={styles.modelName}>{selectedModel?.replace(/^gpt-/, 'GPT-') || 'Choose a model'}</Text><ChevronDown size={12} color={colors.muted} /></View>
          </Pressable> : <Text accessibilityRole="header" style={styles.pageTitle}>{page === 'models' ? 'Choose a model' : 'Providers'}</Text>}
          {page === 'chat' && <IconButton icon={SquarePen} label="New chat" onPress={newChat} />}
          <IconButton icon={X} label="Close AI assistant" onPress={onClose} />
        </View>
        {page === 'chat' && chatVideoId && onOpenVideo && <Pressable accessibilityRole="button" accessibilityLabel="Open video" onPress={() => { Keyboard.dismiss(); onOpenVideo(chatVideoId); }} style={styles.videoLink}>
          <Play size={16} color={colors.ink} /><Text style={styles.videoLinkText}>Open video</Text>
        </Pressable>}
        {browserControl}
        {page === 'chat' ? <>
          {!!contextError && <View style={styles.banner}><Text accessibilityRole="alert" style={styles.help}>{contextError}</Text><Button title="Retry transcript" onPress={() => setContextRetry(value => value + 1)} /></View>}
          {chats.conversation?.providerId && ai.ready && !selected?.connected && <View style={styles.banner}><Text style={styles.help}>Reconnect {chats.conversation.providerLabel} to continue this chat.</Text><Button title="Providers" onPress={() => showPage('providers')} /></View>}
          <AiChat chat={chats} draft={prompt} onDraftChange={setPrompt} attachment={attachment} availableTranscript={availableTranscript}
            connected={!!selected?.connected && !!selected?.enabled && !selected?.needsReauth && !!selectedModel && !ai.busy} loading={!ai.ready || !chats.ready || chats.loading || contextLoading} contextReady={contextReady}
            reasoning={reasoningName(chats.reasoningEffort ?? savedReasoning)} providerId={selected?.id} model={selectedModel}
            onSend={() => { void sendMessage(); }} onRetry={() => { if (contextReady && selected?.connected) void chats.send(selected.id, '', attachment, undefined, true); }}
            onAttach={attachCurrent} onDetach={chatVideoId ? undefined : () => setAttachment(null)} onOpenTranscript={() => { Keyboard.dismiss(); setTranscriptVisible(true); }} onSummarize={summarize} onModels={() => showPage('models')} />
        </> : page === 'models' ? <AiModels ai={ai} selectedId={selected?.id} selectedModel={selectedModel} onSelect={(id, model) => { void choose(id, model); }} onManage={() => showPage('providers')} /> : <AiProviders ai={ai} onUsage={() => { Keyboard.dismiss(); setUsageVisible(true); }} />}
        </View>
        <AiHistoryDrawer visible={historyVisible} chats={chats.history} selectedId={chats.conversation?.id} error={chats.error}
          onClose={() => setHistoryVisible(false)} onNew={newChat} onOpen={id => { void openChat(id); }} onDelete={deleteChat}
          onProviders={() => showPage('providers')} onUsage={() => { setHistoryVisible(false); setUsageVisible(true); }} />
        <AiUsageSheet visible={visible && usageVisible} chatId={chats.conversation?.id} onClose={() => setUsageVisible(false)} />
        {transcriptVisible && <View style={styles.transcriptReader} accessibilityViewIsModal>
          <TranscriptPanel data={contextTranscript} loading={contextLoading} error={contextError || null} cached={false} position={null} pauseAfterLine={false} onClose={() => setTranscriptVisible(false)} />
        </View>}
      </KeyboardAvoidingView>
    </SafeAreaView>
  </Modal>;
}

const makeStyles = (colors: ThemeColors) => StyleSheet.create({
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
