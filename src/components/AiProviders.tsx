import { useEffect, useState } from 'react';
import { ActivityIndicator, Alert, BackHandler, Linking, Pressable, ScrollView, StyleSheet, Switch, Text, TextInput, View } from 'react-native';
import Clipboard from '@react-native-clipboard/clipboard';
import * as FileSystem from 'expo-file-system/legacy';
import * as Sharing from 'expo-sharing';
import { ArrowDown, ArrowUp, ChevronLeft, ChevronRight, Gauge, Plus } from 'lucide-react-native';
import { aiRequest } from '../../modules/lingoplay-core';
import type { AiController, ConnectionPreview, Group, Model, Provider, ProviderChoice, ProviderDraft } from '../ai/useAi';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import { AiMarkdown } from './AiMarkdown';

const protocolNames: [string, string][] = [['', 'Provider default'], ['chat', 'Chat Completions'], ['responses', 'Responses'], ['anthropic', 'Anthropic Messages'], ['gemini', 'Gemini']];
const levels = ['', 'none', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max', 'ultra'];
const draftOf = (p: Provider): ProviderDraft => ({ id: p.id, label: p.label, kind: p.kind, baseUrl: p.baseUrl, model: p.model, enabled: p.enabled, auth: p.auth, protocol: p.protocol, azure: p.azure, appendV1Suffix: !!p.appendV1Suffix, customUserAgent: p.customUserAgent || '', models: p.models });
function Action({ title, onPress, disabled = false }: { title: string; onPress: () => void; disabled?: boolean }) {
  const s = useThemeStyles(makeStyles);
  return <Pressable accessibilityRole="button" disabled={disabled} onPress={onPress} style={[s.action, disabled && s.disabled]}><Text style={s.actionText}>{title}</Text></Pressable>;
}
function Field({ label, value, onChange, secure = false, numeric = false, editable = true, multiline = false, placeholder }: { label: string; value: string; onChange: (v: string) => void; secure?: boolean; numeric?: boolean; editable?: boolean; multiline?: boolean; placeholder?: string }) {
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  return <View style={s.field}><Text style={s.label}>{label}</Text><TextInput accessibilityLabel={label} value={value} onChangeText={onChange} secureTextEntry={secure} autoCapitalize="none" autoCorrect={false} autoComplete="off" editable={editable} keyboardType={numeric ? 'decimal-pad' : 'default'} multiline={multiline} placeholder={placeholder} placeholderTextColor={colors.muted} style={[s.input, multiline && s.multiline, !editable && s.disabled]} /></View>;
}
function Toggle({ label, value, onChange }: { label: string; value: boolean; onChange: (v: boolean) => void }) {
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  return <View style={s.row}><Text style={[s.label, s.grow]}>{label}</Text><Switch accessibilityLabel={label} value={value} onValueChange={onChange} trackColor={{ true: colors.accent }} /></View>;
}
function Choices({ items, value, onChange }: { items: [string, string][]; value: string; onChange: (v: string) => void }) {
  const s = useThemeStyles(makeStyles);
  return <View style={s.choices}>{items.map(([id, label]) => <Pressable accessibilityRole="radio" accessibilityState={{ checked: value === id }} key={id} onPress={() => onChange(id)} style={[s.choice, value === id && s.selected]}><Text style={[s.help, value === id && s.selectedText]}>{label}</Text></Pressable>)}</View>;
}
function Order({ up, down }: { up?: () => void; down?: () => void }) {
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  return <View style={s.order}><Pressable accessibilityRole="button" accessibilityLabel="Move up" disabled={!up} onPress={up} style={[s.orderButton, !up && s.disabled]}><ArrowUp size={16} color={colors.muted} /></Pressable><Pressable accessibilityRole="button" accessibilityLabel="Move down" disabled={!down} onPress={down} style={[s.orderButton, !down && s.disabled]}><ArrowDown size={16} color={colors.muted} /></Pressable></View>;
}

export function AiProviders({ ai, onUsage }: { ai: AiController; onUsage?: () => void }) {
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  const [draft, setDraft] = useState<ProviderDraft | null>(null);
  const [model, setModel] = useState<Model | null>(null);
  const [group, setGroup] = useState<Group | null>(null);
  const [key, setKey] = useState('');
  const [headers, setHeaders] = useState('{}');
  const [body, setBody] = useState('{}');
  const [context, setContext] = useState('');
  const [output, setOutput] = useState('');
  const [temperature, setTemperature] = useState('');
  const [topP, setTopP] = useState('');
  const [query, setQuery] = useState('');
  const [transfer, setTransfer] = useState<string | null>(null);
  const [advanced, setAdvanced] = useState(false);
  const [agentDraft, setAgentDraft] = useState<string | null>(null);
  const [agentSaved, setAgentSaved] = useState(false);
  const [error, setError] = useState('');
  const [saving, setSaving] = useState(false);
  const [preview, setPreview] = useState<ConnectionPreview | null>(null);
  const [keySettings, setKeySettings] = useState<Pick<ProviderDraft, 'baseUrl' | 'protocol' | 'azure' | 'appendV1Suffix'> | null>(null);
  const editing = ai.providers.find(p => p.id === draft?.id);
  const choice = ai.choices.find(c => c.kind === draft?.kind);
  const oauth = draft?.auth === 'oauth' || ['chatgpt', 'codex', 'copilot'].includes(draft?.kind || '');
  const authChanged = !!editing && (draft?.auth || 'key') !== (editing.auth || 'key');
  const connected = !!editing?.connected && !authChanged;
  const busy = !!ai.busy || saving;
  const agentMd = agentDraft ?? ai.agentMd;
  useEffect(() => {
    let active = true;
    setPreview(null);
    if (!draft) return;
    const timer = setTimeout(() => {
      void aiRequest<ConnectionPreview>({ op: 'ai_connection_preview', provider: { ...draft, models: undefined } }).then(result => { if (active) setPreview(result); }).catch(e => { if (active) setError(e.message); });
    }, 120);
    return () => { active = false; clearTimeout(timer); };
  }, [draft]);
  function clear() { ai.clear(); setError(''); setKey(''); setQuery(''); }
  function editProvider(p: Provider) { clear(); setKeySettings(null); setDraft(draftOf(p)); }
  function add(c: ProviderChoice) { clear(); setKeySettings(null); setDraft({ id: '', kind: c.kind, label: c.name, auth: c.auth[0], baseUrl: '', model: '', models: [], enabled: true, protocol: '', azure: false, appendV1Suffix: false, customUserAgent: '' }); }
  function renderChoice(c: ProviderChoice) {
    return <Pressable accessibilityRole="button" key={c.kind} onPress={() => add(c)} style={s.entry}>
      <Plus size={18} color={colors.accent} />
      <View style={s.grow}><Text style={s.label}>{c.name}</Text><Text style={s.help}>{c.description}</Text></View>
    </Pressable>;
  }
  function changeAuth(auth: string) {
    if (!draft || auth === (draft.auth || 'key')) return;
    setKey('');
    const defaults = { baseUrl: '', protocol: '', azure: false, appendV1Suffix: false };
    if (auth === 'oauth') {
      setKeySettings({ baseUrl: draft.baseUrl, protocol: draft.protocol, azure: draft.azure, appendV1Suffix: draft.appendV1Suffix });
      setDraft({ ...draft, auth, ...defaults });
    } else setDraft({ ...draft, auth, ...(keySettings || defaults) });
  }
  async function saveProvider() {
    if (!draft || saving) return null;
    setSaving(true);
    try {
      // Fetch/model actions may have updated the library since this form opened.
      const next = await ai.save({ ...draft, models: editing?.models || draft.models, model: editing ? editing.model : draft.model }, key || undefined);
      if (!next) return null;
      const p = next.providers.find(p => p.id === draft.id) || next.providers.at(-1);
      if (p) { setDraft(draftOf(p)); setKey(''); }
      return p;
    } finally { setSaving(false); }
  }
  async function signIn(consent = false) {
    const p = await saveProvider();
    if (!p) return;
    const result = await ai.run('login', p.id, { consent });
    if (result) await ai.run('models', p.id);
  }
  function connect() {
    if (draft?.kind === 'copilot') {
      Alert.alert('Unofficial Copilot connection', 'This uses the VS Code Copilot client identity. GitHub may restrict accounts for unsupported clients. Continue only if you accept that risk.', [{ text: 'Cancel', style: 'cancel' }, { text: 'Continue', onPress: () => { void signIn(true); } }]);
    } else void signIn();
  }
  async function editModel(m: Model) {
    if (!editing) return;
    clear(); setSaving(true);
    try {
      const options = await aiRequest<{ headers?: Record<string, string>; body?: Record<string, unknown> } | null>({ op: 'ai_model_options', id: editing.id, model: m.id });
      setHeaders(JSON.stringify(options?.headers || {}, null, 2)); setBody(JSON.stringify(options?.body || {}, null, 2));
      setContext(m.contextWindow?.toString() || ''); setOutput(m.maxOutputTokens?.toString() || '');
      setTemperature(m.temperature?.toString() || ''); setTopP(m.topP?.toString() || ''); setModel({ ...m });
    } catch (e) { setError((e as Error).message); } finally { setSaving(false); }
  }
  async function saveModel() {
    if (!model || !editing) return;
    setError('');
    try {
      const number = (v: string) => { if (!v.trim()) return null; const n = Number(v); if (!Number.isFinite(n)) throw new Error('Enter valid numeric model limits.'); return n; };
      const result = await ai.mutate({ op: 'ai_save_model', id: editing.id, model: { ...model, contextWindow: number(context), maxOutputTokens: number(output), temperature: number(temperature), topP: number(topP) }, options: { headers: JSON.parse(headers), body: JSON.parse(body) } });
      if (result) setModel(null);
    } catch (e) { setError(e instanceof SyntaxError ? 'Custom headers and request parameters must be JSON objects.' : (e as Error).message); }
  }
  function editGroup(g: Group) { clear(); setGroup({ ...g, members: [...g.members] }); setContext(g.contextLimit?.toString() || ''); }
  async function saveGroup() {
    if (!group) return;
    const n = context.trim() ? Number(context) : null;
    if (n !== null && (!Number.isSafeInteger(n) || n < 256)) { setError('Enter a context limit of at least 256 tokens, or leave it blank.'); return; }
    const { connected: _, ...value } = group;
    if (await ai.mutate({ op: 'ai_save_group', group: { ...value, contextLimit: n } })) setGroup(null);
  }
  async function reorder(ids: string[], index: number, delta: number, extra: Record<string, string> = {}) {
    const next = [...ids]; [next[index], next[index + delta]] = [next[index + delta], next[index]];
    await ai.mutate({ op: 'ai_reorder', ids: next, ...extra });
  }
  function removeProvider(p: Provider) {
    Alert.alert('Remove provider?', `Remove ${p.label}, its saved models and device credentials?`, [{ text: 'Keep', style: 'cancel' }, { text: 'Remove', style: 'destructive', onPress: () => { void ai.run('remove', p.id).then(r => { if (r) { setDraft(null); clear(); } }); } }]);
  }
  async function exportSettings() {
    setSaving(true); setError('');
    try {
      const value = await aiRequest({ op: 'ai_export' });
      const text = JSON.stringify(value, null, 2);
      const path = `${FileSystem.cacheDirectory}lingoplay-providers.json`;
      await FileSystem.writeAsStringAsync(path, text);
      if (await Sharing.isAvailableAsync()) await Sharing.shareAsync(path, { mimeType: 'application/json', dialogTitle: 'Export AI providers' });
      else { Clipboard.setString(text); Alert.alert('Configuration copied', 'Provider and model settings were copied without credentials.'); }
    } catch (e) { setError((e as Error).message); } finally { setSaving(false); }
  }
  const title = model ? 'Model settings' : group ? (group.id ? 'Edit model group' : 'New model group') : draft ? (draft.id ? draft.label : 'New connection') : transfer !== null ? 'Import providers' : '';
  const screen = model ? 'model' : group ? 'group' : draft ? 'provider' : transfer !== null ? 'import' : 'providers';
  const back = () => { if (busy) return; if (model) setModel(null); else { setDraft(null); setGroup(null); setTransfer(null); } clear(); };
  useEffect(() => {
    if (!title) return;
    const subscription = BackHandler.addEventListener('hardwareBackPress', () => { back(); return true; });
    return () => subscription.remove();
  });
  // Start each editor at its first field rather than inheriting the previous page's scroll.
  return <ScrollView key={screen} keyboardShouldPersistTaps="handled" contentContainerStyle={s.page}>
    {!!title && <View style={s.row}><Pressable accessibilityRole="button" accessibilityLabel="Back in provider settings" onPress={back} disabled={busy} style={s.back}><ChevronLeft size={22} color={colors.ink} /></Pressable><Text style={[s.title, s.grow]}>{title}</Text></View>}
    {model ? <>
      <Field label="Model ID" value={model.id} onChange={id => setModel({ ...model, id })} editable={!editing?.models.some(m => m.id === model.id)} />
      <Field label="Display name" value={model.name} onChange={name => setModel({ ...model, name })} />
      <Toggle label="Hide from model picker" value={!!model.hidden} onChange={hidden => setModel({ ...model, hidden })} />
      <Field label="Context window (tokens)" numeric value={context} onChange={setContext} placeholder="Provider default" />
      <Field label="Maximum output tokens" numeric value={output} onChange={setOutput} placeholder="Provider default" />
      <Text style={s.label}>Reasoning effort</Text><Choices value={model.reasoning || ''} items={(model.reasoningEfforts?.length ? ['', ...model.reasoningEfforts] : levels).map(v => [v, v || 'Provider default'])} onChange={reasoning => setModel({ ...model, reasoning })} />
      <Field label="Temperature" numeric value={temperature} onChange={setTemperature} placeholder="Provider default (0–2)" />
      <Field label="Top P" numeric value={topP} onChange={setTopP} placeholder="Provider default (0–1)" />
      <Text style={s.help}>{[model.inputModalities?.length ? `Input: ${model.inputModalities.join(', ')}` : '', model.outputModalities?.length ? `Output: ${model.outputModalities.join(', ')}` : ''].filter(Boolean).join(' · ')}</Text>
      <Field label="Custom headers (JSON)" value={headers} onChange={setHeaders} multiline />
      <Field label="Extra request parameters (JSON)" value={body} onChange={setBody} multiline />
      <Text style={s.help}>Headers and extra parameters use secure storage and are omitted from exports.</Text>
      <Action title="Save model" disabled={busy || !model.id.trim()} onPress={() => { void saveModel(); }} />
      {!!editing && <Action title="Remove saved model" disabled={busy} onPress={() => Alert.alert('Remove model?', model.name || model.id, [{ text: 'Keep', style: 'cancel' }, { text: 'Remove', style: 'destructive', onPress: () => { void ai.mutate({ op: 'ai_remove_model', id: editing.id, model: model.id }).then(r => { if (r) setModel(null); }); } }])} />}
    </> : group ? <>
      <Field label="Group name" value={group.name} onChange={name => setGroup({ ...group, name })} />
      <Text style={s.label}>Routing strategy</Text><Choices value={group.strategy} items={[["fallback", "Ordered fallback"], ["loadBalance", "Round robin"]]} onChange={strategy => setGroup({ ...group, strategy: strategy as Group['strategy'] })} />
      <Text style={s.help}>Fallback tries the next model after a rate limit or server error. Round robin rotates the first model for each turn. An answer already streaming stays with that model.</Text>
      <Toggle label="Fallback on any error" value={group.fallbackOnAnyError} onChange={fallbackOnAnyError => setGroup({ ...group, fallbackOnAnyError })} />
      <Field label="Group context limit (tokens)" numeric value={context} onChange={setContext} placeholder="Use each model’s context window" />
      <Text style={s.label}>Default reasoning effort</Text><Choices value={group.reasoning} items={levels.map(v => [v, v || 'Use model setting'])} onChange={reasoning => setGroup({ ...group, reasoning })} />
      <Text style={s.section}>Members · {group.members.length}</Text>
      {group.members.map((member, i) => <View style={s.row} key={`${member.providerId}:${member.model}`}><View style={s.grow}><Text style={s.label}>{member.model}</Text><Text style={s.help}>{ai.providers.find(p => p.id === member.providerId)?.label || 'Removed connection'}</Text></View><Order up={i ? () => { const members = [...group.members]; [members[i], members[i-1]] = [members[i-1], members[i]]; setGroup({ ...group, members }); } : undefined} down={i < group.members.length - 1 ? () => { const members = [...group.members]; [members[i], members[i+1]] = [members[i+1], members[i]]; setGroup({ ...group, members }); } : undefined} /><Action title="Remove" onPress={() => setGroup({ ...group, members: group.members.filter((_, index) => index !== i) })} /></View>)}
      <Field label="Find a saved model" value={query} onChange={setQuery} />
      {ai.providers.flatMap(p => p.models.filter(m => !m.hidden && `${m.name} ${m.id} ${p.label}`.toLowerCase().includes(query.toLowerCase()) && !group.members.some(e => e.providerId === p.id && e.model === m.id)).slice(0, 30).map(m => <Pressable key={`${p.id}:${m.id}`} accessibilityRole="button" accessibilityLabel={`Add ${m.name} from ${p.label} to group`} onPress={() => setGroup({ ...group, members: [...group.members, { providerId: p.id, model: m.id }] })} style={s.entry}><Plus size={17} color={colors.accent} /><View style={s.grow}><Text style={s.label}>{m.name}</Text><Text style={s.help}>{p.label}</Text></View></Pressable>))}
      <Action title="Save group" disabled={busy || !group.name.trim() || !group.members.length} onPress={() => { void saveGroup(); }} />
      {!!group.id && <Action title="Delete group" disabled={busy} onPress={() => Alert.alert('Delete model group?', group.name, [{ text: 'Keep', style: 'cancel' }, { text: 'Delete', style: 'destructive', onPress: () => { void ai.mutate({ op: 'ai_remove_group', id: group.id }).then(r => { if (r) setGroup(null); }); } }])} />}
    </> : draft ? <>
      <Text style={s.help}>{choice?.description}</Text>
      <Field label="Connection name" value={draft.label} onChange={label => setDraft({ ...draft, label })} />
      <Toggle label="Enabled" value={draft.enabled !== false} onChange={enabled => setDraft({ ...draft, enabled })} />
      {!!choice && choice.auth.length > 1 && <><Text style={s.label}>Authentication</Text><Choices value={draft.auth || 'key'} items={choice.auth.map(v => [v, v === 'oauth' ? 'Account sign-in' : 'API key'])} onChange={changeAuth} /></>}
      {authChanged && <Text style={s.help}>Saving this authentication change signs out the previous credentials. Your models, groups and chats stay linked to this connection.</Text>}
      {oauth ? <>
        {connected && editing?.account && <Text style={s.label}>{editing.account}</Text>}
        <Text style={s.help}>{!authChanged && editing?.needsReauth ? 'Your account needs to sign in again.' : connected ? 'Account connected' : 'Sign in to connect this account.'}</Text>
        <Action title={connected ? 'Reconnect account' : `Continue with ${choice?.name || draft.label}`} disabled={busy} onPress={connect} />
        {draft.kind === 'chatgpt' && <Text style={s.help}>Available models depend on ChatGPT plan sharing eligibility. You can also add a separate ChatGPT / Codex connection.</Text>}
        {connected && editing && <Action title="Disconnect account" disabled={busy} onPress={() => { setKey(''); void ai.run('disconnect', editing.id); }} />}
        {draft.kind === 'chatgpt' && <Action title="Manage ChatGPT usage" onPress={() => { void Linking.openURL('https://chatgpt.com/settings/usage').catch(() => setError('Could not open account settings.')); }} />}
      </> : <>
        <Field label={connected ? 'Replace API key (optional)' : 'API key'} secure value={key} onChange={setKey} placeholder={connected ? 'Leave blank to keep saved key' : draft.kind === 'custom' ? 'Optional for a keyless endpoint' : 'Enter your API key'} />
        <Field label="API base URL" value={draft.baseUrl} onChange={baseUrl => setDraft({ ...draft, baseUrl })} placeholder={draft.kind === 'custom' ? 'https://example.com' : 'Use provider endpoint'} />
        <Text style={s.help}>HTTPS is required except on localhost. The saved API key will be used with this endpoint.</Text>
        <Text style={s.label}>API protocol</Text><Choices value={draft.protocol || ''} items={protocolNames} onChange={protocol => setDraft({ ...draft, protocol, ...(protocol === 'gemini' ? { appendV1Suffix: false } : {}) })} />
        <Toggle label="Azure OpenAI deployment" value={!!draft.azure} onChange={azure => setDraft({ ...draft, azure, ...(azure ? { appendV1Suffix: false } : {}) })} />
        {draft.azure && <Text style={s.help}>Use the Azure resource endpoint with its api-version query. Model IDs are deployment names.</Text>}
        {(preview?.canAppendV1 || draft.appendV1Suffix) && <Toggle label="Append /v1 automatically" value={!!draft.appendV1Suffix} onChange={appendV1Suffix => setDraft({ ...draft, appendV1Suffix })} />}
      </>}
      <Field label="User-Agent" value={draft.customUserAgent || ''} onChange={customUserAgent => setDraft({ ...draft, customUserAgent })} placeholder="Use provider default" />
      <Text style={s.help}>Applies to model discovery and AI replies. A model-specific User-Agent takes precedence for that model.</Text>
      {!!preview?.effectiveBaseUrl && <View style={s.field}><Text style={s.label}>Effective API base URL</Text><Text selectable style={s.help}>{preview.effectiveBaseUrl}</Text></View>}
      {oauth && <Text style={s.help}>Account sign-in uses this provider’s own endpoint.</Text>}
      {!!preview?.error && <Text accessibilityRole="alert" style={s.error}>{preview.error}</Text>}
      <Action title="Save connection" disabled={busy || !draft.label.trim()} onPress={() => { void saveProvider(); }} />
      {editing && <>
        <Text style={s.section}>Saved models · {editing.models.length}</Text>
        <View style={s.row}><View style={s.grow}><Action title="Refresh model list" disabled={busy || !connected || !editing.enabled} onPress={() => { void saveProvider().then(p => { if (p) void ai.run('models', p.id); }); }} /></View><Action title="Add model ID" disabled={busy} onPress={() => { void editModel({ id: '', name: '', reasoning: '', hidden: false }); }} /></View>
        <Field label="Search saved models" value={query} onChange={setQuery} />
        {editing.models.filter(m => `${m.id} ${m.name}`.toLowerCase().includes(query.toLowerCase())).slice(0, 80).map(m => {
          const i = editing.models.indexOf(m);
          return <View key={m.id} style={[s.row, m.hidden && s.disabled]}><Pressable accessibilityRole="button" accessibilityLabel={`Configure ${m.name}`} onPress={() => { void editModel(m); }} style={s.grow}><Text style={s.label}>{m.name}</Text><Text style={s.help}>{m.id}{m.hidden ? ' · Hidden' : ''}{editing.model === m.id ? ' · Selected' : ''}</Text></Pressable><Order up={!busy && i ? () => { void reorder(editing.models.map(m => m.id), i, -1, { id: editing.id }); } : undefined} down={!busy && i < editing.models.length - 1 ? () => { void reorder(editing.models.map(m => m.id), i, 1, { id: editing.id }); } : undefined} /><ChevronRight size={17} color={colors.muted} /></View>;
        })}
        {editing.models.length > 80 && <Text style={s.help}>Search to narrow the model list.</Text>}
        <Action title="Test selected model" disabled={busy || !connected || !editing.model || !editing.enabled} onPress={() => { void saveProvider().then(p => { if (p) void ai.run('chat', p.id, { prompt: 'Reply with exactly: Connection successful.', includeTranscript: false }); }); }} />
        {!!ai.text && <AiMarkdown content={ai.text} streaming={ai.busy === 'chat'} />}
        <Action title="Remove connection" disabled={busy} onPress={() => removeProvider(editing)} />
      </>}
    </> : transfer !== null ? <>
      <Text style={s.help}>Import a LingoPlay provider configuration. It adds separate connections and model groups. Reconnect imported accounts and keys on this device.</Text>
      <Action title="Paste configuration" onPress={() => { void Clipboard.getString().then(setTransfer).catch(() => setError('Could not read the clipboard.')); }} />
      <Field label="Provider configuration JSON" value={transfer} onChange={setTransfer} multiline />
      <Action title="Import configuration" disabled={busy || !transfer.trim()} onPress={() => { try { const data = JSON.parse(transfer); void ai.mutate({ op: 'ai_import', data }).then(r => { if (r) setTransfer(null); }); } catch { setError('Enter valid provider configuration JSON.'); } }} />
    </> : <>
      {onUsage && <Pressable accessibilityRole="button" accessibilityLabel="Token usage" onPress={onUsage} style={s.entry}><Gauge size={24} color={colors.ink} /><View style={s.grow}><Text style={s.label}>Token usage</Text><Text style={s.help}>Totals, cache and model breakdowns</Text></View><ChevronRight size={20} color={colors.muted} /></Pressable>}
      <Text style={s.section}>AGENT.md</Text>
      <Text style={s.help}>Add instructions for every AI chat and video summary, such as your preferred language, learning level, or response style. Saved changes apply to your next message. Clear and save to remove them.</Text>
      <Field label="AGENT.md instructions" multiline editable={!busy} value={agentMd} onChange={value => { setAgentDraft(value); setAgentSaved(false); }} placeholder={'# Learning preferences\nReply in English. Explain unfamiliar words with examples.'} />
      <Action title="Save instructions" disabled={busy || agentMd === ai.agentMd} onPress={() => { setError(''); void ai.mutate({ op: 'ai_settings', agentMd }).then(next => { if (next) { setAgentDraft(null); setAgentSaved(true); } }); }} />
      {agentSaved && <Text accessibilityLiveRegion="polite" style={s.help}>Instructions saved.</Text>}
      <Text style={s.help}>Connect accounts, choose your models, and control how requests are routed.</Text>
      <Text style={s.section}>Connections</Text>
      {ai.providers.map((p, i) => <View key={p.id} style={s.row}><Pressable accessibilityRole="button" accessibilityLabel={`Edit provider ${p.label}`} style={s.grow} onPress={() => editProvider(p)}><Text style={s.label}>{p.label}</Text><Text style={s.help}>{!p.enabled ? 'Disabled' : p.needsReauth ? 'Sign-in expired' : p.connected ? `${p.models.filter(m => !m.hidden).length} models` : 'Connect to get started'}</Text></Pressable><Order up={!busy && i ? () => { void reorder(ai.providers.map(p => p.id), i, -1); } : undefined} down={!busy && i < ai.providers.length - 1 ? () => { void reorder(ai.providers.map(p => p.id), i, 1); } : undefined} /><ChevronRight size={18} color={colors.muted} /></View>)}
      <Text style={s.section}>Add a connection</Text>
      {ai.choices.filter(c => !c.advanced).map(renderChoice)}
      {ai.choices.some(c => c.advanced) && <>
        <Pressable accessibilityRole="button" accessibilityLabel="Advanced connections" accessibilityState={{ expanded: advanced }} onPress={() => setAdvanced(value => !value)} style={s.entry}>
          <ChevronRight size={18} color={colors.muted} style={{ transform: [{ rotate: advanced ? '90deg' : '0deg' }] }} />
          <Text style={s.label}>Advanced connections</Text>
        </Pressable>
        {advanced && ai.choices.filter(c => c.advanced).map(renderChoice)}
      </>}
      <Text style={s.section}>Model groups</Text>
      <Text style={s.help}>Group models across connections for fallback or round robin routing.</Text>
      {ai.groups.map((g, i) => <View key={g.id} style={s.row}><Pressable accessibilityRole="button" style={s.grow} onPress={() => editGroup(g)}><Text style={s.label}>{g.name}</Text><Text style={s.help}>{g.members.length} models · {g.strategy === 'loadBalance' ? 'Round robin' : 'Fallback'}</Text></Pressable><Order up={!busy && i ? () => { void reorder(ai.groups.map(g => g.id), i, -1, { kind: 'groups' }); } : undefined} down={!busy && i < ai.groups.length - 1 ? () => { void reorder(ai.groups.map(g => g.id), i, 1, { kind: 'groups' }); } : undefined} /><ChevronRight size={18} color={colors.muted} /></View>)}
      <Action title="Create model group" onPress={() => editGroup({ id: '', name: '', members: [], strategy: 'fallback', fallbackOnAnyError: false, reasoning: '', contextLimit: null })} />
      <Text style={s.section}>Configuration</Text><Text style={s.help}>Exports contain connections, models and groups. API keys, account tokens, custom headers and extra request parameters stay on this device.</Text>
      <View style={s.row}><Action title="Import" disabled={busy} onPress={() => { clear(); setTransfer(''); }} /><Action title="Export" disabled={busy} onPress={() => { void exportSettings(); }} /></View>
    </>}
    {busy && <View style={s.row}><ActivityIndicator color={colors.accent} /><Text style={s.help}>{ai.busy === 'login' ? 'Complete account sign-in, then return here.' : 'Saving or contacting provider…'}</Text></View>}
    {ai.busy === 'login' && <Action title="Cancel sign-in" onPress={ai.stop} />}
    {!!ai.notice && <Text accessibilityLiveRegion="polite" selectable style={s.help}>{ai.notice}</Text>}
    {!!(error || ai.error || editing?.credentialError) && <Text accessibilityRole="alert" style={s.error}>{error || ai.error || editing?.credentialError}</Text>}
  </ScrollView>;
}
const makeStyles = (colors: ThemeColors) => StyleSheet.create({
  page: { padding: 20, gap: 14, paddingBottom: 48, width: '100%', maxWidth: 720, alignSelf: 'center' }, row: { flexDirection: 'row', alignItems: 'center', gap: 10, minHeight: 48 }, grow: { flex: 1 },
  title: { color: colors.ink, fontSize: 18, fontWeight: '600' }, section: { color: colors.ink, fontSize: 17, fontWeight: '600', marginTop: 16 }, label: { color: colors.ink, fontSize: 14, fontWeight: '500' }, help: { color: colors.muted, fontSize: 12, lineHeight: 19 },
  field: { gap: 7 }, input: { color: colors.ink, borderWidth: StyleSheet.hairlineWidth, borderColor: colors.line, borderRadius: 10, padding: 12, backgroundColor: colors.surface, fontSize: 14 }, multiline: { minHeight: 100, textAlignVertical: 'top', fontFamily: 'monospace' },
  action: { paddingHorizontal: 14, paddingVertical: 12, minHeight: 44, justifyContent: 'center', alignItems: 'center', backgroundColor: colors.accentSoft, borderRadius: 10 }, actionText: { color: colors.accent, fontSize: 13, fontWeight: '600' },
  choices: { flexDirection: 'row', flexWrap: 'wrap', gap: 8 }, choice: { paddingHorizontal: 12, paddingVertical: 10, minHeight: 40, borderRadius: 9, backgroundColor: colors.surface, borderWidth: StyleSheet.hairlineWidth, borderColor: colors.line }, selected: { backgroundColor: colors.accentSoft, borderColor: colors.accent }, selectedText: { color: colors.accent },
  entry: { flexDirection: 'row', alignItems: 'center', gap: 12, paddingVertical: 8, minHeight: 48 }, back: { minWidth: 40, minHeight: 44, justifyContent: 'center' }, order: { flexDirection: 'row' }, orderButton: { width: 36, minHeight: 44, alignItems: 'center', justifyContent: 'center' },
  error: { color: colors.error, fontSize: 13, lineHeight: 20 }, disabled: { opacity: 0.4 },
});
