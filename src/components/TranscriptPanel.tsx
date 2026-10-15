import { useEffect, useState } from 'react';
import { ActivityIndicator, Alert, FlatList, Pressable, ScrollView, Share, StyleSheet, Text, TextInput, useWindowDimensions, View } from 'react-native';
import Clipboard from '@react-native-clipboard/clipboard';
import { Copy, RotateCw, Search, Share2, X } from 'lucide-react-native';
import { colors } from '../theme';
import { timestamp, type Segment, type Transcript } from '../youtube/transcript';
import { exportTranscript, searchTranscript } from '../core/transcripts';
import { IconButton } from './IconButton';

export function TranscriptPanel({ data, loading, error, cached, onClose, onExtract, onSeek }: {
  data: Transcript | null;
  loading: boolean;
  error: string | null;
  cached: boolean;
  onClose: () => void;
  onExtract: (track?: string) => void;
  onSeek: (seconds: number) => void;
}) {
  const [query, setQuery] = useState('');
  const [copied, setCopied] = useState(false);
  const [matches, setMatches] = useState<{ query: string; data: Transcript; segments: Segment[] } | null>(null);
  const [searchError, setSearchError] = useState<string | null>(null);
  const { fontScale } = useWindowDimensions();
  useEffect(() => {
    let active = true;
    setSearchError(null);
    if (!data || !query.trim()) return;
    const timer = setTimeout(() => {
      searchTranscript(data, query).then(segments => {
        if (active) setMatches({ query, data, segments });
      }).catch(() => { if (active) setSearchError('Could not search this transcript. Try refreshing it.'); });
    }, 100);
    return () => { active = false; clearTimeout(timer); };
  }, [data, query]);
  const searching = !!query.trim() && !(matches?.query === query && matches.data === data) && !searchError;
  const segments = !query.trim() ? data?.segments ?? [] : matches?.query === query && matches.data === data ? matches.segments : [];
  return (
    <View style={styles.panel}>
      <View style={styles.heading}>
        <View style={styles.headingText}><Text accessibilityRole="header" style={styles.title}>Transcript</Text>
          <Text numberOfLines={1} style={styles.subtitle}>{cached ? 'Saved · ' : ''}{data?.title || 'YouTube captions'}</Text></View>
        <IconButton icon={RotateCw} label="Refresh transcript" disabled={loading} onPress={() => { setCopied(false); onExtract(); }} />
        <IconButton icon={Copy} label={copied ? 'Transcript copied' : 'Copy transcript'} disabled={!data || loading} onPress={async () => {
          if (!data) return;
          try { Clipboard.setString(await exportTranscript(data)); setCopied(true); } catch { Alert.alert('Could not copy transcript'); }
        }} />
        <IconButton icon={Share2} label="Share transcript" disabled={!data || loading} onPress={async () => {
          if (!data) return;
          try { await Share.share({ message: await exportTranscript(data), title: data.title }); } catch { Alert.alert('Could not share transcript'); }
        }} />
        <IconButton icon={X} label="Close transcript" onPress={onClose} />
      </View>
      {copied && <Text accessibilityLiveRegion="polite" style={styles.feedback}>Transcript copied</Text>}
      {loading ? <View style={styles.empty}><ActivityIndicator color={colors.accent} /><Text style={styles.emptyTitle}>Extracting captions…</Text><Text style={styles.help}>You can keep watching while this loads.</Text></View> : error ? (
        <View style={styles.empty}><Text style={styles.emptyTitle}>Transcript unavailable</Text><Text accessibilityRole="alert" style={styles.help}>{error}</Text>
          <Pressable accessibilityRole="button" onPress={() => onExtract()} style={styles.retry}><Text style={styles.retryText}>Try again</Text></Pressable></View>
      ) : data ? <>
        <ScrollView horizontal showsHorizontalScrollIndicator={false} style={[styles.tracks, { height: Math.max(48, Math.ceil(20 * fontScale) + 28) }]} contentContainerStyle={styles.trackContent}>
          {data.tracks.map(track => <Pressable key={track.id} accessibilityRole="button" accessibilityState={{ selected: track.id === data.trackId }}
            onPress={() => { setCopied(false); onExtract(track.id); }} style={[styles.track, track.id === data.trackId && styles.selectedTrack]}>
            <Text style={[styles.trackText, track.id === data.trackId && styles.selectedText]}>{track.label}{track.automatic ? ' · Auto' : ''}</Text>
          </Pressable>)}
        </ScrollView>
        <View style={styles.search}><Search size={16} color={colors.muted} /><TextInput accessibilityLabel="Search transcript" placeholder="Find in transcript" placeholderTextColor={colors.muted}
          value={query} onChangeText={setQuery} style={styles.searchInput} />
          {searching ? <ActivityIndicator size="small" color={colors.accent} /> : <Text style={styles.count}>{query.trim() ? `${segments.length} of ${data.segments.length}` : segments.length}</Text>}
          {!!query && <IconButton icon={X} label="Clear transcript search" onPress={() => setQuery('')} />}</View>
        <FlatList data={segments} keyExtractor={(item, index) => `${item.start}:${index}`} contentContainerStyle={styles.list}
          keyboardShouldPersistTaps="handled" ListEmptyComponent={<Text style={styles.help}>{searchError || (searching ? 'Searching…' : 'No matching lines.')}</Text>}
          ListHeaderComponent={<Text style={styles.language}>{query.trim() ? `Search results for “${query.trim()}”` : data.language} · Tap a line to jump and play</Text>}
          renderItem={({ item }) => <Pressable accessibilityRole="button" accessibilityLabel={`${timestamp(item.start)} · ${item.text}`}
            accessibilityHint="Jump to this line and play the video" onPress={() => onSeek(item.start)}
            style={({ pressed }) => [styles.segment, pressed && styles.pressedSegment]}>
            <Text style={styles.time}>{timestamp(item.start)}</Text>
            <Text style={styles.segmentText}>{item.text}</Text>
          </Pressable>} />
      </> : null}
    </View>
  );
}

const styles = StyleSheet.create({
  panel: { flex: 1, backgroundColor: colors.background, borderTopWidth: 1, borderTopColor: colors.line },
  heading: { flexDirection: 'row', alignItems: 'center', paddingLeft: 20, paddingRight: 6, paddingVertical: 8 },
  headingText: { flex: 1, gap: 3 },
  title: { fontSize: 19, fontWeight: '600', letterSpacing: -0.4, color: colors.ink },
  subtitle: { fontSize: 11, color: colors.muted },
  feedback: { color: colors.accent, fontSize: 12, paddingHorizontal: 20, paddingBottom: 8 },
  empty: { flex: 1, justifyContent: 'center', alignItems: 'center', padding: 28, gap: 12 },
  emptyTitle: { fontSize: 17, fontWeight: '600', color: colors.ink },
  help: { fontSize: 13, lineHeight: 21, color: colors.muted, textAlign: 'center', maxWidth: 320 },
  retry: { backgroundColor: colors.accent, paddingHorizontal: 22, paddingVertical: 13, borderRadius: 12, marginTop: 8 },
  retryText: { color: colors.surface, fontWeight: '600' },
  tracks: { flexGrow: 0, flexShrink: 0 },
  trackContent: { paddingHorizontal: 20, paddingBottom: 10, gap: 8 },
  track: { paddingVertical: 9, paddingHorizontal: 13, borderRadius: 10, backgroundColor: colors.surface },
  selectedTrack: { backgroundColor: colors.accentSoft },
  trackText: { color: colors.muted, fontSize: 12 },
  selectedText: { color: colors.accent, fontWeight: '600' },
  search: { flexDirection: 'row', alignItems: 'center', marginHorizontal: 20, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line, gap: 8 },
  searchInput: { flex: 1, paddingVertical: 11, color: colors.ink, fontSize: 13 },
  count: { color: colors.muted, fontSize: 11 },
  list: { paddingHorizontal: 20, paddingBottom: 24 },
  language: { color: colors.muted, fontSize: 10, marginTop: 14, marginBottom: 8 },
  segment: { flexDirection: 'row', gap: 12, paddingVertical: 9, minHeight: 44, borderRadius: 6 },
  pressedSegment: { backgroundColor: colors.accentSoft },
  time: { minWidth: 46, paddingTop: 3, fontSize: 12, color: colors.accent, fontVariant: ['tabular-nums'] },
  segmentText: { flex: 1, fontSize: 15, lineHeight: 24, color: colors.ink },
});
