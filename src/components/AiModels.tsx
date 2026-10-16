import { useMemo, useState } from 'react';
import { FlatList, Pressable, StyleSheet, Text, TextInput, View } from 'react-native';
import { Check, Layers, Settings2 } from 'lucide-react-native';
import type { AiController } from '../ai/useAi';
import { colors } from '../theme';

type Row = { key: string; id: string; name: string; detail: string; model: string; connected: boolean; group: boolean };
export function AiModels({ ai, selectedId, selectedModel, onSelect, onManage }: { ai: AiController; selectedId?: string; selectedModel?: string; onSelect: (id: string, model: string) => void; onManage: () => void }) {
  const [query, setQuery] = useState('');
  const rows = useMemo(() => {
    const result: Row[] = ai.groups.map(g => ({ key: g.id, id: g.id, name: g.name, detail: `${g.members.length} models · ${g.strategy === 'loadBalance' ? 'Round robin' : 'Fallback'}`, model: g.name, connected: !!g.connected, group: true }));
    for (const p of ai.providers) {
      if (!p.enabled) continue;
      for (const m of p.models) {
        if (!m.hidden) result.push({ key: `${p.id}:${m.id}`, id: p.id, name: m.name || m.id, detail: `${p.label}${m.contextWindow ? ` · ${Math.round(m.contextWindow / 1000)}K context` : ''}${!p.connected || p.needsReauth ? ' · Reconnect' : ''}`, model: m.id, connected: p.connected && !p.needsReauth, group: false });
      }
    }
    const q = query.trim().toLowerCase();
    return result.filter(m => `${m.name} ${m.model} ${m.detail}`.toLowerCase().includes(q));
  }, [ai.providers, ai.groups, query]);
  return <View style={s.page}>
    <TextInput accessibilityLabel="Search AI models" placeholder="Search models or connections…" placeholderTextColor={colors.muted} autoCapitalize="none" autoCorrect={false} value={query} onChangeText={setQuery} style={s.search} />
    <FlatList data={rows} keyExtractor={m => m.key} keyboardShouldPersistTaps="handled" initialNumToRender={18} maxToRenderPerBatch={18}
      ListHeaderComponent={<Text style={s.help}>Choose any saved model or group. Changing models starts a new chat.</Text>}
      ListEmptyComponent={<View style={s.empty}><Text style={s.label}>{ai.providers.length ? 'No matching visible models' : 'Connect your first provider'}</Text><Text style={s.help}>Add a connection and refresh its models, or save a model ID.</Text></View>}
      renderItem={({ item: m }) => <Pressable accessibilityRole="radio" accessibilityState={{ checked: m.id === selectedId && m.model === selectedModel }} accessibilityLabel={`Use ${m.name}, ${m.detail}`} onPress={() => m.connected ? onSelect(m.id, m.model) : onManage()} style={s.row}>
        {m.group && <Layers size={18} color={colors.accent} />}<View style={s.grow}><Text style={s.label}>{m.name}</Text><Text style={s.help}>{m.detail}</Text></View>{m.id === selectedId && m.model === selectedModel && <Check size={19} color={colors.accent} />}
      </Pressable>}
    />
    <Pressable accessibilityRole="button" onPress={onManage} style={s.manage}><Settings2 size={18} color={colors.accent} /><Text style={s.manageText}>Manage providers and models</Text></Pressable>
    {!!ai.error && <Text accessibilityRole="alert" style={s.error}>{ai.error}</Text>}
  </View>;
}
const s = StyleSheet.create({
  page: { flex: 1, paddingHorizontal: 20, paddingTop: 12 }, search: { color: colors.ink, backgroundColor: colors.surface, borderWidth: StyleSheet.hairlineWidth, borderColor: colors.line, borderRadius: 12, padding: 13, fontSize: 14, marginBottom: 14 },
  row: { flexDirection: 'row', alignItems: 'center', gap: 10, paddingVertical: 16, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line }, grow: { flex: 1, gap: 3 }, label: { color: colors.ink, fontSize: 15, fontWeight: '500' }, help: { color: colors.muted, fontSize: 12, lineHeight: 19 },
  empty: { gap: 8, paddingVertical: 32 }, manage: { flexDirection: 'row', gap: 10, alignItems: 'center', paddingVertical: 18 }, manageText: { color: colors.accent, fontSize: 14, fontWeight: '500' }, error: { color: colors.error, fontSize: 13, marginBottom: 12 },
});
