import { useEffect, useState } from 'react';
import { ActivityIndicator, Pressable, ScrollView, StyleSheet, Text, useWindowDimensions, View } from 'react-native';
import { Brain, Check, ChevronDown, ChevronRight, CircleAlert, Gamepad2, Square } from 'lucide-react-native';
import type { ChatBlock } from '../ai/useChats';
import { PlayerSheet } from '../player/PlayerSheet';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import { AiMarkdown } from './AiMarkdown';

export function ReasoningActivity({ content, active }: { content: string; active: boolean }) {
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  const [expanded, setExpanded] = useState(false);
  useEffect(() => { if (!active) setExpanded(false); }, [active]);
  return <View style={s.reasoning}>
    <Pressable accessibilityRole="button" accessibilityLabel={active ? 'Thinking' : 'Reasoning'} accessibilityState={{ expanded }} onPress={() => setExpanded(!expanded)} style={s.reasoningHeader}>
      {active ? <ActivityIndicator size="small" color={colors.muted} /> : <Brain size={16} color={colors.muted} />}
      <Text style={s.label}>{active ? 'Thinking…' : 'Reasoning'}</Text>
      <ChevronDown size={14} color={colors.muted} style={expanded ? s.expanded : undefined} />
    </Pressable>
    {expanded && <ScrollView style={s.reasoningBody} nestedScrollEnabled keyboardShouldPersistTaps="handled"><AiMarkdown content={content} streaming={active} /></ScrollView>}
  </View>;
}

type ToolBlock = Extract<ChatBlock, { kind: 'tool' }>;
export function ToolActivity({ block, messageStatus, busy }: { block: ToolBlock; messageStatus: string; busy: boolean }) {
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  const { height } = useWindowDimensions();
  const [details, setDetails] = useState(false);
  const pending = block.status === 'preparing' || block.status === 'running';
  const active = pending && messageStatus === 'streaming' && busy;
  const status = pending && !active ? messageStatus === 'failed' ? 'failed' : 'stopped' : block.status;
  const exercise = block.name === 'create_interactive_game';
  const label = active ? (exercise ? 'Creating exercise…' : 'Running tool…')
    : status === 'completed' ? (exercise ? 'Exercise ready' : 'Tool completed')
    : status === 'failed' ? (exercise ? 'Exercise creation failed' : 'Tool failed')
    : (exercise ? 'Exercise creation stopped' : 'Tool stopped');
  const tint = status === 'failed' ? colors.error : colors.muted;
  const Icon = status === 'failed' ? CircleAlert : status === 'completed' ? Check : status === 'stopped' ? Square : Gamepad2;
  const duration = block.durationMs > 0 ? `${(block.durationMs / 1000).toFixed(1)}s` : null;
  return <View style={s.tool}>
    <Pressable accessibilityRole="button" accessibilityLabel={`${label}${duration ? `, ${duration}` : ''}. Show details`} accessibilityHint="View tool status and exercise details" onPress={() => setDetails(true)} style={({ pressed }) => [s.toolRow, pressed && s.pressed]}>
      {active ? <ActivityIndicator size="small" color={colors.muted} /> : <Icon size={16} color={tint} />}
      <Text style={[s.toolLabel, { color: tint }]} numberOfLines={1} accessibilityLiveRegion="polite">{label}</Text>
      {!!duration && <Text style={s.duration}>{duration}</Text>}
      <ChevronRight size={14} color={colors.muted} />
    </Pressable>
    {details && <PlayerSheet visible title="Tool activity" onClose={() => setDetails(false)} maxContentHeight={height * 0.6}>
      <View style={s.details}>
        <Text accessibilityRole="header" style={s.detailTitle}>{label}</Text>
        {!!block.title && <Text selectable style={s.result}>{block.title}</Text>}
        <View style={s.detailRow}><Text style={s.label}>Tool</Text><Text selectable style={s.value}>{block.name || 'Receiving tool call'}</Text></View>
        <View style={s.detailRow}><Text style={s.label}>Status</Text><Text style={s.value}>{active ? block.status === 'preparing' ? 'Receiving tool call' : 'Running' : status === 'completed' ? 'Completed' : status === 'failed' ? 'Failed' : 'Stopped'}</Text></View>
        {!!duration && <View style={s.detailRow}><Text style={s.label}>Duration</Text><Text style={s.value}>{duration}</Text></View>}
        <View style={s.detailRow}><Text style={s.label}>Started</Text><Text style={s.value}>{new Date(block.startedAt).toLocaleTimeString()}</Text></View>
        {!!block.gameId && <Text style={s.label}>The playable exercise is in this conversation.</Text>}
        {!!block.error && <Text selectable accessibilityRole="alert" style={s.error}>{block.error}</Text>}
      </View>
    </PlayerSheet>}
  </View>;
}

const makeStyles = (colors: ThemeColors) => StyleSheet.create({
  reasoning: { marginBottom: 8 }, reasoningHeader: { flexDirection: 'row', alignItems: 'center', alignSelf: 'flex-start', gap: 8, minHeight: 44 },
  label: { color: colors.muted, fontSize: 13, lineHeight: 20 }, expanded: { transform: [{ rotate: '180deg' }] },
  reasoningBody: { maxHeight: 240, paddingLeft: 12, borderLeftWidth: 2, borderColor: colors.line, marginBottom: 8 },
  tool: { marginVertical: 6 }, toolRow: { flexDirection: 'row', alignItems: 'center', alignSelf: 'flex-start', maxWidth: '100%', minHeight: 44, borderRadius: 22, backgroundColor: colors.raised, paddingHorizontal: 12, gap: 8 },
  toolLabel: { flexShrink: 1, fontSize: 13, lineHeight: 20 }, duration: { color: colors.muted, fontSize: 11, fontVariant: ['tabular-nums'] }, pressed: { opacity: 0.65 },
  details: { paddingHorizontal: 20, paddingBottom: 20, gap: 12 }, detailTitle: { color: colors.ink, fontSize: 18, fontWeight: '600' }, result: { color: colors.ink, fontSize: 15, lineHeight: 23 },
  detailRow: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', gap: 16, minHeight: 40, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line },
  value: { flexShrink: 1, color: colors.ink, fontSize: 13, lineHeight: 20, textAlign: 'right' }, error: { color: colors.error, fontSize: 13, lineHeight: 20 },
});
