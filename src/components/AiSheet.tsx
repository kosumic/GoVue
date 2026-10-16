import { useEffect, useState } from 'react';
import { ActivityIndicator, Alert, KeyboardAvoidingView, Linking, Modal, Platform, Pressable, ScrollView, StyleSheet, Switch, Text, TextInput, View } from 'react-native';
import Clipboard from '@react-native-clipboard/clipboard';
import { Check, ChevronLeft, Plus, X } from 'lucide-react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { useAi, type Model, type Provider, type ProviderDraft, type ProviderKind } from '../ai/useAi';
import type { Transcript } from '../youtube/transcript';
import { colors } from '../theme';
import { IconButton } from './IconButton';
import { AiMarkdown } from './AiMarkdown';

const kinds: [ProviderKind, string][] = [['chatgpt', 'ChatGPT plan'], ['openai', 'OpenAI API'], ['anthropic', 'Anthropic'], ['gemini', 'Google Gemini'], ['openrouter', 'OpenRouter'], ['custom', 'OpenAI compatible']];
const draftOf = (p: Provider): ProviderDraft => ({ id: p.id, label: p.label, kind: p.kind, baseUrl: p.baseUrl, model: p.model });
const blank = (kind: ProviderKind): ProviderDraft => ({ id: '', kind, label: kinds.find(k => k[0] === kind)![1], baseUrl: '', model: '' });
const external = (url: string) => { void Linking.openURL(url).catch(() => Alert.alert('Could not open browser', 'Please try again.')); };
const summaryPrompt = 'Summarize this video using the attached transcript. Start with a concise overview, then list the key points with timestamps. Use only information supported by the transcript.';
export type SummaryRequest = { id: number; videoId: string; trackId: string };

function Button({ title, onPress, disabled = false }: { title: string; onPress: () => void; disabled?: boolean }) {
  return <Pressable accessibilityRole="button" disabled={disabled} onPress={onPress} style={[styles.button, disabled && styles.disabled]}><Text style={styles.buttonText}>{title}</Text></Pressable>;
}

export function AiSheet({ visible, onClose, transcript, videoId, summaryRequest }: { visible: boolean; onClose: () => void; transcript: Transcript | null; videoId: string | null; summaryRequest: SummaryRequest | null }) {
  const ai = useAi(visible, `${videoId || ''}:${transcript?.trackId || ''}`);
  const [tab, setTab] = useState<'ask' | 'providers'>('providers');
  const [draft, setDraft] = useState<ProviderDraft | null>(null);
  const [key, setKey] = useState('');
  const [models, setModels] = useState<Model[]>([]);
  const [modelQuery, setModelQuery] = useState('');
  const [prompt, setPrompt] = useState('');
  const [includeTranscript, setIncludeTranscript] = useState(false);
  const [saving, setSaving] = useState(false);
  const [pendingSummary, setPendingSummary] = useState<SummaryRequest | null>(null);
  const [summaryView, setSummaryView] = useState(false);
  const active = ai.providers.find(p => p.id === ai.activeId);
  const editing = ai.providers.find(p => p.id === draft?.id);
  const attached = !!transcript && transcript.videoId === videoId;
  const matchingModels = models.filter(m => `${m.id} ${m.name}`.toLocaleLowerCase().includes(modelQuery.trim().toLocaleLowerCase()));
  useEffect(() => { setIncludeTranscript(false); }, [videoId, transcript?.trackId, ai.activeId]);
  useEffect(() => { setSummaryView(false); }, [videoId, transcript?.trackId]);
  useEffect(() => { if (!visible) { setKey(''); setDraft(null); setModels([]); setIncludeTranscript(false); setPendingSummary(null); } }, [visible]);
  useEffect(() => {
    if (!visible || !summaryRequest) { setSummaryView(false); return; }
    ai.clear(); setTab('ask'); setDraft(null); setKey(''); setModels([]);
    setPrompt(summaryPrompt); setIncludeTranscript(true); setSummaryView(true); setPendingSummary(summaryRequest);
  }, [visible, summaryRequest]);
  useEffect(() => {
    if (!visible || !pendingSummary) return;
    if (!attached || transcript.videoId !== pendingSummary.videoId || transcript.trackId !== pendingSummary.trackId) {
      setPendingSummary(null);
      return;
    }
    if (!ai.ready || !active?.connected || !active.model || ai.busy) return;
    setPendingSummary(null); setTab('ask'); setIncludeTranscript(true);
    void ai.run('chat', active.id, { prompt: summaryPrompt, includeTranscript: true, videoId: pendingSummary.videoId, trackId: pendingSummary.trackId });
  }, [visible, pendingSummary, attached, transcript, ai.ready, active, ai.busy]);

  async function save() {
    if (!draft || saving) return;
    setSaving(true);
    try {
      const result = await ai.save(draft, key);
      if (result) {
        setKey('');
        const saved = result.providers.find(p => p.id === draft.id) || result.providers[result.providers.length - 1];
        if (saved) setDraft(draftOf(saved));
      }
      return result;
    } finally { setSaving(false); }
  }
  function edit(p: Provider) { ai.stop(); ai.clear(); setDraft(draftOf(p)); setKey(''); setModels([]); }
  async function connect() {
    if (!draft) return;
    let id = draft.id;
    if (!id) {
      const result = await save();
      id = result?.providers[result.providers.length - 1]?.id || '';
    }
    if (id) await ai.run('login', id);
  }
  function remove(p: Provider) {
    Alert.alert('Remove provider?', `Remove ${p.label} and its saved credentials from this device?`, [
      { text: 'Keep', style: 'cancel' }, { text: 'Remove', style: 'destructive', onPress: () => { void ai.run('remove', p.id).then(result => { if (result) { setDraft(null); setKey(''); setModels([]); } }); } },
    ]);
  }
  function ask(question = prompt, attachTranscript = includeTranscript) {
    if (!active || !question.trim()) return;
    setPendingSummary(null);
    setPrompt(question);
    void ai.run('chat', active.id, { prompt: question, includeTranscript: attachTranscript && attached, ...(attachTranscript && attached ? { videoId: transcript.videoId, trackId: transcript.trackId } : {}) });
  }
  return <Modal visible={visible} animationType="slide" presentationStyle="pageSheet" onRequestClose={onClose}>
    <SafeAreaView style={styles.safe}>
      <KeyboardAvoidingView style={styles.safe} behavior={Platform.OS === 'ios' ? 'padding' : 'height'}>
        <View style={styles.heading}><Text accessibilityRole="header" style={styles.title}>{summaryView ? 'Video summary' : 'AI assistant'}</Text><IconButton icon={X} label="Close AI assistant" onPress={onClose} /></View>
        <View style={styles.tabs}>{(['ask', 'providers'] as const).map(value => <Pressable key={value} accessibilityRole="tab" accessibilityState={{ selected: tab === value }} style={[styles.tab, tab === value && styles.selected]} onPress={() => { setTab(value); setKey(''); }}><Text style={styles.buttonText}>{value === 'ask' ? summaryView ? 'Summary' : 'Ask' : 'Providers'}</Text></Pressable>)}</View>
        <ScrollView keyboardShouldPersistTaps="handled" contentContainerStyle={styles.body}>
          {tab === 'ask' ? <>
            {active ? <><Text style={styles.label}>{active.label}</Text><Text style={styles.help}>{active.model || 'Choose a model in Providers.'}</Text></> : <><Text style={styles.label}>Connect a model</Text><Text style={styles.help}>Add a provider and choose a model to ask questions here.</Text><Button title="Set up a provider" onPress={() => setTab('providers')} /></>}
            {pendingSummary && ai.ready && <Text style={styles.help}>Connect a provider and select a model to summarize this video. Once ready, this transcript will be sent to that provider.</Text>}
            {pendingSummary && ai.ready && active && (!active.connected || !active.model) && <Button title="Set up a provider" onPress={() => setTab('providers')} />}
            {summaryView && attached ? <>
              <Text numberOfLines={2} style={styles.label}>{transcript.title}</Text>
              <Text style={styles.help}>The summary uses this video’s {transcript.language} captions and includes key points with timestamps.</Text>
              {ai.busy === 'chat' ? <Button title="Stop response" onPress={ai.stop} /> : <Button title="Summarize again" disabled={!active?.connected || !active.model || !!ai.busy || !!pendingSummary} onPress={() => ask(summaryPrompt, true)} />}
            </> : <>
              {attached && <View style={styles.row}><View style={styles.grow}><Text style={styles.label}>Include transcript</Text><Text style={styles.help}>{includeTranscript ? `The transcript will be sent to ${active?.label || 'your selected provider'} with your next question.` : 'Enable to ask about the current video. Only saved caption text and its title are attached.'}</Text></View><Switch accessibilityLabel="Include current transcript in AI request" value={includeTranscript} disabled={!!ai.busy} onValueChange={value => { setPendingSummary(null); setIncludeTranscript(value); }} trackColor={{ true: colors.accent }} /></View>}
              {attached && <Text numberOfLines={2} style={styles.help}>{transcript.title}</Text>}
              <TextInput accessibilityLabel="Question for AI assistant" placeholder={includeTranscript ? 'Ask about this transcript…' : 'Ask a question…'} placeholderTextColor={colors.muted} value={prompt} onChangeText={value => { setPendingSummary(null); setPrompt(value); }} multiline maxLength={8000} editable={!ai.busy} style={[styles.input, styles.composer]} />
              {ai.busy === 'chat' ? <Button title="Stop response" onPress={ai.stop} /> : <Button title="Send question" disabled={!active?.connected || !active.model || !prompt.trim() || !!ai.busy} onPress={() => ask()} />}
              {includeTranscript && <Button title="Summarize this transcript" disabled={!active?.connected || !active.model || !!ai.busy} onPress={() => ask(summaryPrompt)} />}
            </>}
            {ai.text ? <View style={styles.answer}><AiMarkdown content={ai.text} streaming={ai.busy === 'chat'} /><Button title="Copy answer" onPress={() => Clipboard.setString(ai.text)} /></View> : null}
            {summaryView && <Button title="Ask about this video" disabled={!!ai.busy || !!pendingSummary} onPress={() => { setSummaryView(false); setPrompt(''); setIncludeTranscript(attached); }} />}
            <Text style={styles.help}>{summaryView ? 'The transcript goes directly to your selected provider. Summaries are not saved.' : 'Each question starts a new conversation. Your question and any enabled transcript go directly to the selected provider. Answers are not saved.'}</Text>
          </> : draft ? <>
            <View style={styles.row}><IconButton icon={ChevronLeft} label="Back to providers" disabled={!!ai.busy || saving} onPress={() => { setDraft(null); setKey(''); setModels([]); ai.clear(); }} /><Text style={styles.label}>{kinds.find(k => k[0] === draft.kind)?.[1]}</Text></View>
            {editing?.credentialError && <Text accessibilityRole="alert" style={styles.error}>{editing.credentialError}</Text>}
            <Text style={styles.label}>Name</Text><TextInput accessibilityLabel="Provider name" value={draft.label} onChangeText={label => setDraft({ ...draft, label })} editable={!ai.busy && !saving} maxLength={100} style={styles.input} />
            {draft.kind === 'custom' && <><Text style={styles.label}>API base URL</Text><TextInput accessibilityLabel="Provider API base URL" placeholder="https://example.com/v1" value={draft.baseUrl} onChangeText={baseUrl => setDraft({ ...draft, baseUrl })} editable={!editing && !saving} autoCapitalize="none" autoCorrect={false} keyboardType="url" style={styles.input} /><Text style={styles.help}>Use the base path before /chat/completions. Add another provider to change a saved endpoint.</Text></>}
            {draft.kind === 'chatgpt' ? <>
              {editing?.account && <Text style={styles.label}>{editing.account}</Text>}
              <Text style={styles.help}>Connect your ChatGPT account and grant access to your plan. Available models and limits depend on your account and this app’s eligibility.</Text>
              <Button title={editing?.connected ? 'Reconnect with ChatGPT' : 'Continue with ChatGPT'} disabled={!!ai.busy || saving || !draft.label.trim()} onPress={() => { void connect(); }} />
              {editing?.connected && <>
                <Text style={styles.help}>{editing.planEnabled ? 'ChatGPT plan usage is enabled.' : 'Connected; plan usage is not enabled.'}</Text>
                <Button title="Manage ChatGPT usage" onPress={() => external('https://chatgpt.com/settings/usage')} />
                <Button title="Disconnect account" disabled={!!ai.busy} onPress={() => { void ai.run('disconnect', draft.id); }} />
              </>}
            </> : <>
              <Text style={styles.label}>{editing?.connected ? 'Replace API key (optional)' : 'API key'}</Text>
              <TextInput accessibilityLabel="Provider API key" value={key} onChangeText={setKey} placeholder={editing?.connected ? 'Leave blank to keep saved key' : 'Enter your API key'} secureTextEntry autoCapitalize="none" autoCorrect={false} autoComplete="off" textContentType="none" editable={!ai.busy && !saving} style={styles.input} />
              <Text style={styles.help}>The key is protected by this device’s secure storage. API usage is billed by your provider.</Text>
            </>}
            <Text style={styles.label}>Model ID</Text><TextInput accessibilityLabel="Provider model ID" value={draft.model} onChangeText={model => setDraft({ ...draft, model })} placeholder="Fetch models or enter an ID" autoCapitalize="none" autoCorrect={false} maxLength={200} editable={!ai.busy && !saving} style={styles.input} />
            <Button title={saving ? 'Saving…' : 'Save changes'} disabled={!!ai.busy || saving || !draft.label.trim() || (draft.kind === 'custom' && !draft.baseUrl.trim())} onPress={() => { void save(); }} />
            {draft.id && <>
              <Button title="Fetch available models" disabled={!!ai.busy || !editing?.connected || saving} onPress={() => { void ai.run('models', draft.id).then(result => { if (result?.models) setModels(result.models); }); }} />
              {models.length > 0 && <><TextInput accessibilityLabel="Search available models" placeholder="Search models…" value={modelQuery} onChangeText={setModelQuery} autoCapitalize="none" autoCorrect={false} style={styles.input} /><Text style={styles.help}>{matchingModels.length} matching models. Choose a model, then save changes.{matchingModels.length > 80 ? ' Narrow the search to see more.' : ''}</Text></>}
              {matchingModels.slice(0, 80).map(m => <Pressable key={m.id} accessibilityRole="button" accessibilityLabel={`Choose model ${m.id}`} onPress={() => { setDraft({ ...draft, model: m.id }); setModels([]); setModelQuery(''); }} style={styles.model}><Text style={styles.label}>{m.name}</Text><Text style={styles.help}>{m.id}</Text></Pressable>)}
              <Button title="Test saved model" disabled={!!ai.busy || !editing?.connected || !editing.model || saving} onPress={() => { void ai.run('chat', draft.id, { prompt: 'Reply with exactly: Connection successful.', includeTranscript: false }); }} />
              {ai.text ? <AiMarkdown content={ai.text} streaming={ai.busy === 'chat'} /> : null}
              <Button title="Remove provider" disabled={!!ai.busy || saving} onPress={() => { if (editing) remove(editing); }} />
            </>}
          </> : <>
            {ai.providers.map(p => <View key={p.id} style={styles.provider}>
              <Pressable accessibilityRole="radio" accessibilityState={{ checked: p.id === ai.activeId }} accessibilityLabel={`Use provider ${p.label}`} onPress={() => { void ai.select(p.id); }} style={[styles.row, styles.grow]}><View style={styles.grow}><Text style={styles.label}>{p.label}</Text><Text style={styles.help}>{p.connected ? p.model || 'Choose a model' : 'Connect to get started'}</Text></View>{p.id === ai.activeId && <Check size={20} color={colors.accent} />}</Pressable>
              <Button title="Edit" disabled={!!ai.busy} onPress={() => edit(p)} />
            </View>)}
            <Text style={styles.section}>Add a provider</Text>
            {kinds.map(([kind, label]) => <Pressable key={kind} accessibilityRole="button" style={styles.row} onPress={() => { ai.clear(); setDraft(blank(kind)); setModels([]); setKey(''); }}><Plus size={19} color={colors.accent} /><Text style={styles.label}>{label}</Text></Pressable>)}
            <Text style={styles.help}>Use your own API keys or connect a ChatGPT plan. You can save separate accounts and choose which provider handles your questions.</Text>
          </>}
          {!!ai.busy && <View style={styles.row}><ActivityIndicator color={colors.accent} /><Text accessibilityLiveRegion="polite" style={[styles.help, styles.grow]}>{ai.busy === 'login' ? 'Complete sign-in in the in-app browser.' : 'Contacting provider…'}</Text></View>}
          {ai.busy === 'login' && <Button title="Cancel sign-in" onPress={ai.stop} />}
          {!!ai.notice && <Text accessibilityLiveRegion="polite" style={styles.help}>{ai.notice}</Text>}
          {!!ai.error && <Text accessibilityRole="alert" style={styles.error}>{ai.error}</Text>}
        </ScrollView>
      </KeyboardAvoidingView>
    </SafeAreaView>
  </Modal>;
}

const styles = StyleSheet.create({
  safe: { flex: 1, backgroundColor: colors.background },
  heading: { flexDirection: 'row', alignItems: 'center', paddingHorizontal: 18, paddingVertical: 8, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line },
  title: { flex: 1, color: colors.ink, fontSize: 22, fontWeight: '600' },
  tabs: { flexDirection: 'row', padding: 12, gap: 8 }, tab: { padding: 12, flex: 1, alignItems: 'center', borderRadius: 10 }, selected: { backgroundColor: colors.accentSoft },
  body: { padding: 20, gap: 14, paddingBottom: 48 }, row: { flexDirection: 'row', alignItems: 'center', gap: 12, paddingVertical: 8 }, grow: { flex: 1 },
  label: { color: colors.ink, fontSize: 15, fontWeight: '500' }, help: { color: colors.muted, fontSize: 13, lineHeight: 20 },
  section: { color: colors.ink, fontSize: 18, fontWeight: '600', marginTop: 12 },
  input: { color: colors.ink, fontSize: 15, borderWidth: 1, borderColor: colors.line, backgroundColor: colors.surface, padding: 14, borderRadius: 10 }, composer: { minHeight: 100, textAlignVertical: 'top' },
  button: { backgroundColor: colors.accentSoft, borderRadius: 10, padding: 14, alignItems: 'center' }, buttonText: { color: colors.accent, fontSize: 14, fontWeight: '600' }, disabled: { opacity: 0.45 },
  provider: { flexDirection: 'row', alignItems: 'center', gap: 12, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line, paddingBottom: 14 },
  model: { paddingVertical: 12, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line },
  answer: { gap: 16, paddingVertical: 16 }, error: { color: colors.error, fontSize: 13, lineHeight: 20 },
});
