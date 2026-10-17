import { useEffect, useState, type ReactNode } from 'react';
import { Alert, Keyboard, KeyboardAvoidingView, Modal, Platform, Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';
import { ChevronDown, ChevronLeft, Gauge, History, MessageSquare, SquarePen, Trash2, X } from 'lucide-react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { useAi } from '../ai/useAi';
import type { Transcript } from '../youtube/transcript';
import { colors } from '../theme';
import { IconButton } from './IconButton';
import { AiChat } from './AiChat';
import { AiProviders } from './AiProviders';
import { AiModels } from './AiModels';
import { AiUsageSheet } from './AiUsageSheet';
import { reasoningName } from './AiReasoning';
import { useChats, type ChatTranscript, type SavedChat } from '../ai/useChats';

export type SummaryRequest = { id: number; videoId: string; trackId: string };

function Button({ title, onPress, disabled = false }: { title: string; onPress: () => void; disabled?: boolean }) {
  return <Pressable accessibilityRole="button" disabled={disabled} onPress={onPress} style={[styles.button, disabled && styles.disabled]}><Text style={styles.buttonText}>{title}</Text></Pressable>;
}

export function AiSheet({ visible, onClose, transcript, videoId, summaryRequest, browserControl }: { visible: boolean; onClose: () => void; transcript: Transcript | null; videoId: string | null; summaryRequest: SummaryRequest | null; browserControl?: ReactNode }) {
  const ai = useAi(visible, `${videoId || ''}:${transcript?.trackId || ''}`);
  const chats = useChats(visible, `${videoId || ''}:${transcript?.trackId || ''}`);
  const [page, setPage] = useState<'chat' | 'providers' | 'history' | 'models'>('chat');
  const [usageVisible, setUsageVisible] = useState(false);
  useEffect(() => { if (!visible) setUsageVisible(false); }, [visible]);
  const [prompt, setPrompt] = useState('');
  const [attachment, setAttachment] = useState<ChatTranscript | null>(null);
  const [pendingSummary, setPendingSummary] = useState<SummaryRequest | null>(null);
  const targets = [...ai.providers, ...ai.groups.map(g => ({ id: g.id, label: g.name, model: g.name, connected: !!g.connected, enabled: true, needsReauth: false }))];
  const active = targets.find(p => p.id === ai.activeId);
  const selected = targets.find(p => p.id === (chats.conversation?.providerId || ai.activeId));
  const selectedModel = chats.conversation?.model || selected?.model;
  const savedReasoning = ai.providers.find(p => p.id === selected?.id)?.models.find(m => m.id === selectedModel)?.reasoning || ai.groups.find(g => g.id === selected?.id)?.reasoning;
  const attached = !!transcript && transcript.videoId === videoId;
  const availableTranscript: ChatTranscript | null = attached ? { videoId: transcript.videoId, trackId: transcript.trackId, title: transcript.title, language: transcript.language } : null;
  useEffect(() => { if (chats.conversation) setAttachment(chats.conversation.transcript); }, [chats.conversation?.id]);
  useEffect(() => { if (!chats.conversation) setAttachment(null); }, [videoId, transcript?.trackId]);
  useEffect(() => { if (!visible) { setPendingSummary(null); setPage('chat'); } }, [visible]);
  useEffect(() => {
    if (!visible || !summaryRequest) return;
    chats.newChat(); setPage('chat'); setPrompt('');
    setAttachment(availableTranscript); setPendingSummary(summaryRequest);
  }, [visible, summaryRequest]);
  useEffect(() => {
    if (!visible || !pendingSummary) return;
    if (!attached || transcript.videoId !== pendingSummary.videoId || transcript.trackId !== pendingSummary.trackId) {
      setPendingSummary(null);
      return;
    }
    if (!ai.ready || !chats.ready || !active?.connected || !active.model || ai.busy || chats.busy) return;
    setPendingSummary(null); setPage('chat');
    void chats.send(active.id, 'Summarize this video', availableTranscript, 'summary');
  }, [visible, pendingSummary, attached, transcript, ai.ready, active, ai.busy, chats.ready, chats.busy]);

  function showPage(next: typeof page) { Keyboard.dismiss(); ai.clear(); setPage(next); }
  function newChat() { chats.newChat(); setPrompt(''); setAttachment(null); setPendingSummary(null); showPage('chat'); }
  async function choose(id: string, model: string) {
    const p = targets.find(p => p.id === id);
    if (!p?.connected || !p.enabled || p.needsReauth) { showPage('providers'); return; }
    if (await ai.select(id, model)) {
      if (chats.conversation && (chats.conversation.providerId !== id || chats.conversation.model !== model)) newChat();
      showPage('chat');
    }
  }
  async function openChat(id: string) {
    setPendingSummary(null); setPrompt('');
    const chat = await chats.open(id);
    if (!chat) return;
    setAttachment(chat.transcript);
    if (targets.some(p => p.id === chat.providerId)) await ai.select(chat.providerId);
    showPage('chat');
  }
  function deleteChat(chat: SavedChat) {
    Alert.alert('Delete chat?', chat.title, [{ text: 'Keep', style: 'cancel' }, { text: 'Delete', style: 'destructive', onPress: () => { void chats.remove(chat.id); } }]);
  }
  async function sendMessage() {
    if (!selected?.connected || !selected.enabled || selected.needsReauth || !selectedModel || !prompt.trim()) return;
    const message = prompt.trim(); setPrompt(''); setPendingSummary(null);
    if (!await chats.send(selected.id, message, attachment)) setPrompt(current => current || message);
  }
  function attachCurrent() {
    if (!availableTranscript) return;
    if (chats.conversation?.transcript && chats.conversation.transcript.videoId !== availableTranscript.videoId) newChat();
    setAttachment(availableTranscript);
  }
  function summarize() {
    if (!availableTranscript || !active) return;
    newChat(); setAttachment(availableTranscript);
    void chats.send(active.id, 'Summarize this video', availableTranscript, 'summary');
  }

  return <Modal visible={visible} animationType="slide" presentationStyle="pageSheet" onRequestClose={() => page === 'chat' ? onClose() : showPage('chat')}>
    <SafeAreaView style={styles.safe}>
      <KeyboardAvoidingView style={styles.safe} behavior={Platform.OS === 'ios' ? 'padding' : 'height'}>
        <View style={styles.heading}>
          {page === 'chat' ? <IconButton icon={History} label="Chat history" onPress={() => { void chats.refresh().catch(() => {}); showPage('history'); }} /> : <IconButton icon={ChevronLeft} label="Back to chat" onPress={() => { ai.stop(); showPage('chat'); }} />}
          {page === 'chat' ? <Pressable accessibilityRole="button" accessibilityLabel="Choose AI model" onPress={() => showPage('models')} style={styles.chatTitle}>
            <Text numberOfLines={1} style={styles.brand}>{chats.conversation?.title || 'LingoPlay'}</Text>
            <View style={styles.modelLabel}><Text numberOfLines={1} style={styles.modelName}>{selectedModel?.replace(/^gpt-/, 'GPT-') || 'Choose a model'}</Text><ChevronDown size={12} color={colors.muted} /></View>
          </Pressable> : <Text accessibilityRole="header" style={styles.pageTitle}>{page === 'history' ? 'Chats' : page === 'models' ? 'Choose a model' : 'Providers'}</Text>}
          {(page === 'chat' || page === 'history') && <IconButton icon={SquarePen} label="New chat" onPress={newChat} />}
          {page === 'chat' && <IconButton icon={Gauge} label="Token usage" onPress={() => { Keyboard.dismiss(); setUsageVisible(true); }} />}
          <IconButton icon={X} label="Close AI assistant" onPress={onClose} />
        </View>
        {browserControl}
        {page === 'chat' ? <>
          {pendingSummary && ai.ready && (!active?.connected || !active.model) && <View style={styles.banner}><Text style={styles.help}>Choose a model to summarize this video.</Text><Button title="Choose model" onPress={() => showPage('models')} /></View>}
          {chats.conversation && ai.ready && !selected?.connected && <View style={styles.banner}><Text style={styles.help}>Reconnect {chats.conversation.providerLabel} to continue this chat.</Text><Button title="Providers" onPress={() => showPage('providers')} /></View>}
          <AiChat chat={chats} draft={prompt} onDraftChange={setPrompt} attachment={attachment} availableTranscript={availableTranscript}
            connected={!!selected?.connected && !!selected?.enabled && !selected?.needsReauth && !!selectedModel && !ai.busy} loading={!ai.ready || !chats.ready || chats.loading}
            reasoning={reasoningName(chats.reasoningEffort ?? savedReasoning)} providerId={selected?.id} model={selectedModel}
            onSend={() => { void sendMessage(); }} onRetry={() => { if (selected?.connected) void chats.send(selected.id, '', attachment, undefined, true); }}
            onAttach={attachCurrent} onDetach={() => setAttachment(null)} onSummarize={summarize} onModels={() => showPage('models')} />
        </> : page === 'history' ? <ScrollView contentContainerStyle={[styles.body, !chats.history.length && styles.emptyHistory]} keyboardShouldPersistTaps="handled">
          {chats.history.length ? <>
            <Text style={styles.help}>Saved on this device</Text>
            {chats.history.map(chat => <View key={chat.id} style={[styles.historyRow, chat.id === chats.conversation?.id && styles.currentChat]}>
              <Pressable accessibilityRole="button" accessibilityLabel={`Open chat ${chat.title}`} onPress={() => { void openChat(chat.id); }} style={styles.historyEntry}>
                <MessageSquare size={18} color={colors.muted} /><View style={styles.grow}><Text numberOfLines={2} style={styles.label}>{chat.title}</Text><Text numberOfLines={1} style={styles.historyMeta}>{chat.model} · {new Date(chat.updatedAt * 1000).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })}</Text></View>
              </Pressable><IconButton icon={Trash2} label={`Delete chat ${chat.title}`} onPress={() => deleteChat(chat)} />
            </View>)}
          </> : <View style={styles.historyWelcome}><MessageSquare size={30} color={colors.muted} /><Text style={styles.pageTitle}>No chats yet</Text><Text style={styles.help}>Your conversations will appear here.</Text><Button title="Start a chat" onPress={newChat} /></View>}
          {!!chats.error && <Text accessibilityRole="alert" style={styles.error}>{chats.error}</Text>}
        </ScrollView> : page === 'models' ? <AiModels ai={ai} selectedId={selected?.id} selectedModel={selectedModel} onSelect={(id, model) => { void choose(id, model); }} onManage={() => showPage('providers')} /> : <AiProviders ai={ai} onUsage={() => { Keyboard.dismiss(); setUsageVisible(true); }} />}
        <AiUsageSheet visible={visible && usageVisible} chatId={chats.conversation?.id} onClose={() => setUsageVisible(false)} />

      </KeyboardAvoidingView>
    </SafeAreaView>
  </Modal>;
}

const styles = StyleSheet.create({
  safe: { flex: 1, backgroundColor: colors.background },
  heading: { flexDirection: 'row', alignItems: 'center', paddingHorizontal: 6, paddingVertical: 5 },
  chatTitle: { flex: 1, paddingHorizontal: 5, paddingVertical: 4, gap: 2 }, brand: { color: colors.ink, fontSize: 17, fontWeight: '600', letterSpacing: -0.3 },
  modelLabel: { flexDirection: 'row', alignItems: 'center', gap: 4 }, modelName: { flexShrink: 1, color: colors.muted, fontSize: 11 }, pageTitle: { flex: 1, color: colors.ink, fontSize: 19, fontWeight: '600' },
  body: { padding: 20, gap: 14, paddingBottom: 48 }, row: { flexDirection: 'row', alignItems: 'center', gap: 12, paddingVertical: 8 }, grow: { flex: 1 },
  label: { color: colors.ink, fontSize: 15, fontWeight: '500' }, help: { color: colors.muted, fontSize: 13, lineHeight: 20 },
  section: { color: colors.ink, fontSize: 18, fontWeight: '600', marginTop: 12 },
  input: { color: colors.ink, fontSize: 15, borderWidth: 1, borderColor: colors.line, backgroundColor: colors.surface, padding: 14, borderRadius: 10 },
  button: { backgroundColor: colors.accentSoft, borderRadius: 10, padding: 14, alignItems: 'center' }, buttonText: { color: colors.accent, fontSize: 14, fontWeight: '600' }, disabled: { opacity: 0.45 },
  provider: { flexDirection: 'row', alignItems: 'center', gap: 12, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line, paddingBottom: 14 },
  model: { paddingVertical: 12, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line },
  error: { color: colors.error, fontSize: 13, lineHeight: 20 }, banner: { paddingHorizontal: 20, paddingBottom: 10, gap: 8 },
  emptyHistory: { flexGrow: 1 }, historyWelcome: { flex: 1, alignItems: 'center', justifyContent: 'center', gap: 16 },
  historyRow: { flexDirection: 'row', alignItems: 'center', borderRadius: 12, marginHorizontal: -8, paddingHorizontal: 8 }, currentChat: { backgroundColor: colors.accentSoft },
  historyEntry: { flex: 1, flexDirection: 'row', alignItems: 'center', gap: 12, minHeight: 74, paddingVertical: 12 }, historyMeta: { color: colors.muted, fontSize: 11, lineHeight: 18, marginTop: 3 },
  modelChoice: { flexDirection: 'row', alignItems: 'center', gap: 10, paddingVertical: 16, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line },
});
