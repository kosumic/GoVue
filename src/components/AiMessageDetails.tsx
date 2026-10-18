import { Pressable, StyleSheet, Text, View } from 'react-native';
import { Check, ChevronDown, ChevronRight, CircleCheck, CircleHelp, CircleX, Copy, RotateCw, Square, Volume2 } from 'lucide-react-native';
import type { ChatMessage } from '../ai/useChats';
import { formatTokens } from '../ai/usage';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import { useCopyFeedback } from './useCopyFeedback';
import { useChatExpansion } from './useChatViewport';

const TOKEN_FIELDS = [
  ['inputTokens', 'Input (including cache)'], ['outputTokens', 'Output'],
  ['uncachedInputTokens', 'Uncached input'], ['cacheReadTokens', 'Cache read'],
  ['cacheCreationTokens', 'Cache write'], ['reasoningTokens', 'Reasoning (included in output)'],
] as const;
const CACHE_LABELS = { hit: 'Cache hit', miss: 'Cache miss', unknown: 'Cache unknown' };
function Detail({ label, value }: { label: string; value: string | undefined }) {
  const s = useThemeStyles(makeStyles);
  return value === undefined ? null : <View style={s.detail}><Text style={s.label}>{label}</Text><Text selectable style={s.value}>{value}</Text></View>;
}

export function AiMessageDetails({ message, last, retryDisabled, onRetry, reading, readDisabled, onRead }: { message: ChatMessage; last: boolean; retryDisabled: boolean; onRetry: () => void; reading: boolean; readDisabled: boolean; onRead: () => void }) {
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  const { copied, copyText } = useCopyFeedback();
  const [expanded, setExpanded] = useChatExpansion(`${message.id}:usage`);
  const usage = message.usage;
  const details = message.usageDetails;
  const cache = details?.cacheStatus ?? 'unknown';
  const CacheIcon = cache === 'hit' ? CircleCheck : cache === 'miss' ? CircleX : CircleHelp;
  const cachePercent = details?.cacheHitRate == null || cache !== 'hit' ? '' : details.cacheHitRate < 0.1 ? '<0.1%' : `${Number(details.cacheHitRate.toFixed(1))}%`;
  const cacheLabel = `${CACHE_LABELS[cache]}${cachePercent ? ` · ${cachePercent}` : ''}`;
  const tokens = details?.totalTokens != null ? `${formatTokens(details.totalTokens)} tokens`
    : usage?.inputTokens != null || usage?.outputTokens != null ? `${formatTokens(usage?.inputTokens)} in / ${formatTokens(usage?.outputTokens)} out` : message.status === 'streaming' ? 'Responding…' : 'Usage not reported';
  const streamTime = usage?.streamMs ? `${Number((usage.streamMs / 1000).toFixed(1))}s` : undefined;
  const summary = [message.modelName || message.model, tokens, streamTime ? `Stream duration: ${streamTime}` : null].filter(Boolean).join(' · ');
  const disabled = retryDisabled || message.status === 'streaming';
  const speechDisabled = readDisabled || message.status === 'streaming' || !message.content.trim();
  return <View style={s.footer}>
    <View style={s.row}>
      <Pressable accessibilityRole="button" accessibilityLabel="Response details" accessibilityHint={cacheLabel} accessibilityState={{ expanded }} onPress={() => setExpanded(!expanded)} style={s.summary}>
        <Text style={s.summaryText} numberOfLines={2}>{summary}</Text>
        <CacheIcon size={13} color={cache === 'hit' ? colors.action : colors.muted} />
        {!!cachePercent && <Text numberOfLines={1} style={s.cachePercent}>{cachePercent}</Text>}
        {expanded ? <ChevronDown size={13} color={colors.muted} /> : <ChevronRight size={13} color={colors.muted} />}
      </Pressable>
      <Pressable accessibilityRole="button" accessibilityLabel={reading ? 'Stop reading response' : 'Read response aloud'} accessibilityState={{ selected: reading, disabled: speechDisabled }} disabled={speechDisabled} onPress={onRead} style={[s.action, speechDisabled && s.disabled]}>
        {reading ? <Square size={16} color={colors.action} /> : <Volume2 size={16} color={colors.muted} />}
      </Pressable>
      {!!message.content && <Pressable accessibilityRole="button" accessibilityLabel={copied ? 'Response copied' : 'Copy response'} accessibilityLiveRegion="polite" onPress={() => copyText(message.content)} style={s.action}>
        {copied ? <Check size={16} color={colors.action} /> : <Copy size={16} color={colors.muted} />}
      </Pressable>}
      <Pressable accessibilityRole="button" accessibilityLabel={message.status === 'failed' || message.status === 'stopped' ? 'Try again' : 'Regenerate response'} accessibilityHint={last ? 'Generate a new response to this prompt' : 'Replaces this response and later messages'} accessibilityState={{ disabled }} disabled={disabled} onPress={onRetry} style={[s.action, disabled && s.disabled]}><RotateCw size={16} color={colors.muted} /></Pressable>
    </View>
    {expanded && <View style={s.details}>
      <Detail label="Model" value={message.modelName || message.model} />
      <Detail label="Provider" value={message.providerLabel} />
      <Detail label="Status" value={message.status === 'streaming' ? 'Responding' : message.status === 'completed' ? 'Completed' : message.status === 'failed' ? 'Failed' : 'Stopped'} />
      <Detail label="Finished" value={message.completedAt ? new Date(message.completedAt * 1000).toLocaleString() : undefined} />
      {TOKEN_FIELDS.map(([field, label]) => <Detail key={field} label={label} value={usage?.[field] == null ? undefined : usage[field].toLocaleString()} />)}
      <Detail label="Total tokens" value={details?.totalTokens?.toLocaleString()} />
      <Detail label="Cache" value={cacheLabel} />
      <Detail label="Stream duration" value={streamTime} />
      <Detail label="Output speed" value={details?.outputTokensPerSecond == null ? undefined : `${details.outputTokensPerSecond.toFixed(1)} tok/s`} />
      {usage?.inputTokens == null && usage?.outputTokens == null && <Text style={s.label}>{message.status === 'streaming' ? 'Token counts appear when reported by the provider.' : 'The provider did not report token counts for this response.'}</Text>}
      {(usage?.cacheReadTokens != null || usage?.reasoningTokens != null) && <Text style={s.label}>Cache tokens are included in input. Reasoning tokens are included in output.</Text>}
    </View>}
  </View>;
}

const makeStyles = (colors: ThemeColors) => StyleSheet.create({
  footer: { marginTop: 4 }, row: { flexDirection: 'row', alignItems: 'center', gap: 2 },
  summary: { flex: 1, minHeight: 44, flexDirection: 'row', alignItems: 'center', gap: 6, paddingRight: 4 },
  summaryText: { color: colors.muted, fontSize: 11, lineHeight: 17, flexShrink: 1 },
  cachePercent: { color: colors.muted, fontSize: 11, lineHeight: 17, flexShrink: 0 },
  action: { width: 44, height: 44, alignItems: 'center', justifyContent: 'center' }, disabled: { opacity: 0.38 },
  details: { gap: 8, borderLeftWidth: 1, borderColor: colors.line, paddingLeft: 12, paddingVertical: 8 },
  detail: { flexDirection: 'row', gap: 12 }, label: { color: colors.muted, fontSize: 11, lineHeight: 18, flex: 1 },
  value: { color: colors.ink, fontSize: 11, lineHeight: 18, flex: 1, fontVariant: ['tabular-nums'] },
});
