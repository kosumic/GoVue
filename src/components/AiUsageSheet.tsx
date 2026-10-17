import { useEffect, useState, type ReactNode } from 'react';
import { ActivityIndicator, Pressable, StyleSheet, Text, useWindowDimensions, View } from 'react-native';
import { ChevronRight, RefreshCw } from 'lucide-react-native';
import { formatTokens, useUsage, type ModelUsage, type UsageStats } from '../ai/usage';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import { PlayerSheet, SheetRow } from '../player/PlayerSheet';
import { IconButton } from './IconButton';
import { reasoningName } from './AiReasoning';

function Stat({ label, value, suffix = '' }: { label: string; value: number | null | undefined; suffix?: string }) {
  const s = useThemeStyles(makeStyles);
  const display = value == null ? '—' : suffix ? `${value.toFixed(1)}${suffix}` : formatTokens(value);
  return <View style={s.stat} accessible accessibilityLabel={`${label}: ${value == null ? 'Not reported' : `${value.toLocaleString()}${suffix}`}`}>
    <Text style={s.label}>{label}</Text><Text style={s.number}>{display}</Text>
  </View>;
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  const s = useThemeStyles(makeStyles);
  return <View style={s.section}><Text accessibilityRole="header" style={s.sectionTitle}>{title}</Text>{children}</View>;
}

function Counts({ stats, detail = false }: { stats: UsageStats; detail?: boolean }) {
  return <>
    <Stat label="Input (including cache)" value={stats.inputTokens} />
    <Stat label="Output" value={stats.outputTokens} />
    {(stats.reasoningTokens ?? 0) > 0 && <Stat label="Reasoning (included in output)" value={stats.reasoningTokens} />}
    {(stats.cacheReadTokens ?? 0) > 0 && <Stat label="Cache read" value={stats.cacheReadTokens} />}
    {(stats.cacheCreationTokens ?? 0) > 0 && <Stat label="Cache write" value={stats.cacheCreationTokens} />}
    {stats.cacheHitRate != null && <Stat label="Cache hit rate" value={stats.cacheHitRate} suffix="%" />}
    <Stat label="Output speed" value={stats.outputTokensPerSecond} suffix=" tok/s" />
    <Stat label="Recorded replies" value={stats.replyCount} />
    {detail && <>
      {!stats.legacyUsage && <Stat label="Uncached input" value={stats.uncachedInputTokens} />}
      <Stat label="Average tokens per chat" value={stats.averagePerChat} />
      <Stat label="Average tokens per day" value={stats.averagePerDay} />
      <Stat label="Chats" value={stats.chatCount} />
      <Stat label="Active days (UTC)" value={stats.activeDays} />
    </>}
  </>;
}

function ModelRow({ model }: { model: ModelUsage }) {
  const [expanded, setExpanded] = useState(false);
  const s = useThemeStyles(makeStyles);
  const colors = useThemeColors();
  return <View>
    <Pressable accessibilityRole="button" accessibilityLabel={`Usage for ${model.modelName}`} accessibilityState={{ expanded }} onPress={() => setExpanded(value => !value)} style={s.model}>
      <View style={s.grow}>
        <Text style={s.label}>{model.modelName}</Text>
        {model.estimated && <Text style={s.help}>Legacy model attribution</Text>}
        {model.removed && <Text style={s.help}>Not in current configuration</Text>}
      </View>
      <Text style={s.help}>{formatTokens(model.stats.inputTokens)} in / {formatTokens(model.stats.outputTokens)} out</Text>
      <ChevronRight size={20} color={colors.muted} style={{ transform: [{ rotate: expanded ? '90deg' : '0deg' }] }} />
    </Pressable>
    {expanded && <View style={s.details}><Counts stats={model.stats} detail /></View>}
  </View>;
}

export function AiUsageSheet({ visible, onClose, chatId = null }: { visible: boolean; onClose: () => void; chatId?: string | null }) {
  const [selected, setSelected] = useState(chatId);
  useEffect(() => { if (visible) setSelected(chatId); }, [visible, chatId]);
  const { report, error, refresh } = useUsage(visible, selected);
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  const { height } = useWindowDimensions();
  return <PlayerSheet visible={visible} title="Token usage" onClose={onClose} onBack={selected ? () => setSelected(null) : undefined} maxContentHeight={height * 0.6} contentKey={selected || 'all'}>
    <View key={selected || 'all'} style={s.body}>
      <View style={s.toolbar}><Text style={[s.scope, s.grow]}>{report?.title || (selected ? 'Conversation' : 'All saved chats')}</Text><IconButton icon={RefreshCw} label="Refresh token usage" onPress={refresh} /></View>
      {!report && !error && <ActivityIndicator accessibilityLabel="Loading token usage" color={colors.ink} style={s.loading} />}
      {!!error && <><Text accessibilityRole="alert" style={s.error}>{error}</Text><SheetRow title="Try again" onPress={refresh} /></>}
      {report && <>
        {report.context && <Section title="Context at last recorded reply">
          <Text style={s.help}>{report.context.providerLabel} · {report.context.model}</Text>
          <Stat label="Context used" value={report.context.used} />
          <Stat label="Context window" value={report.context.window} />
          <Stat label="Maximum output" value={report.context.maxOutputTokens} />
          <View style={s.stat}><Text style={s.label}>Reasoning effort</Text><Text style={s.number}>{reasoningName(report.context.reasoningEffort)}</Text></View>
        </Section>}
        <Section title={selected ? 'Conversation totals' : 'Total usage'}><Counts stats={report.stats} /></Section>
        {report.stats.replyCount === 0 && <Text style={s.help}>No provider-reported token usage has been recorded for these saved chats yet.</Text>}
        <Text style={s.help}>Provider-reported counts from saved replies. Cache tokens are included in input once. Deleted chats are excluded.</Text>
        {report.stats.unreportedReplies > 0 && <Text style={s.help}>{report.stats.unreportedReplies} {report.stats.unreportedReplies === 1 ? 'reply has' : 'replies have'} no reported token counts.</Text>}
        {report.stats.legacyUsage && <Text style={s.help}>Older replies may not include cache details, completion dates, or output speed.</Text>}
        {report.providers.map(provider => <Section key={provider.id} title={provider.label}>{provider.models.map(model => <ModelRow key={model.key} model={model} />)}</Section>)}
        {!selected && report.chats.length > 0 && <Section title="By conversation">
          {report.chats.map(chat => <SheetRow key={chat.id} title={chat.title || 'Untitled chat'} detail={`${chat.providerLabel} · ${chat.model}`} trailing={<ChevronRight size={20} color={colors.muted} />} onPress={() => setSelected(chat.id)} />)}
        </Section>}
      </>}
    </View>
  </PlayerSheet>;
}

const makeStyles = (colors: ThemeColors) => StyleSheet.create({
  body: { paddingHorizontal: 20, paddingBottom: 12, gap: 16 },
  toolbar: { flexDirection: 'row', alignItems: 'center', gap: 8 },
  scope: { color: colors.ink, fontSize: 16, fontWeight: '500' }, grow: { flex: 1 },
  section: { gap: 2 }, sectionTitle: { color: colors.muted, fontSize: 12, fontWeight: '600', marginTop: 8, marginBottom: 8 },
  stat: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', gap: 12, minHeight: 42, paddingVertical: 8, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line },
  label: { flexShrink: 1, color: colors.ink, fontSize: 14, lineHeight: 20 },
  number: { color: colors.muted, fontSize: 14, fontVariant: ['tabular-nums'] },
  model: { flexDirection: 'row', alignItems: 'center', minHeight: 56, paddingVertical: 12, gap: 8 },
  details: { paddingLeft: 16, paddingBottom: 12 }, help: { color: colors.muted, fontSize: 12, lineHeight: 18 },
  loading: { padding: 32 }, error: { color: colors.error, fontSize: 14 },
});
