import { useCallback, useEffect, useRef, useState } from 'react';
import { ActivityIndicator, Alert, FlatList, Pressable, ScrollView, Share, StyleSheet, Switch, Text, TextInput, useWindowDimensions, View } from 'react-native';
import Clipboard from '@react-native-clipboard/clipboard';
import { Copy, Download, LocateFixed, RotateCw, Search, Share2, Sparkles, X } from 'lucide-react-native';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import { currentSegmentIndex, timestamp, type Segment, type Transcript } from '../youtube/transcript';
import { exportTranscript, searchTranscript } from '../core/transcripts';
import { IconButton } from './IconButton';
import { TranscriptWords } from './TranscriptWords';
import { saveWord } from '../core/vocabulary';

export function TranscriptPanel({ data, loading, error, cached, position, pauseAfterLine, onPauseAfterLine, onClose, onExtract, onSeek, onSummarize, onDownload, downloadDisabled }: {
  data: Transcript | null;
  loading: boolean;
  error: string | null;
  cached: boolean;
  position: number | null;
  pauseAfterLine: boolean;
  onPauseAfterLine?: (enabled: boolean) => void;
  onClose: () => void;
  onExtract: (track?: string) => void;
  onSeek: (seconds: number) => void;
  onSummarize?: () => void;
  onDownload: () => void;
  downloadDisabled: boolean;
}) {
  const colors = useThemeColors();
  const styles = useThemeStyles(makeStyles);
  const [query, setQuery] = useState('');
  const [copied, setCopied] = useState(false);
  const [wordFeedback, setWordFeedback] = useState('');
  const savingWord = useRef(false);
  const activeData = useRef(data);
  activeData.current = data;
  useEffect(() => {
    activeData.current = data;
    setWordFeedback('');
    return () => { activeData.current = null; };
  }, [data]);
  const [matches, setMatches] = useState<{ query: string; data: Transcript; segments: Segment[] } | null>(null);
  const [searchError, setSearchError] = useState<string | null>(null);
  const [followPlayback, setFollowPlayback] = useState(true);
  const list = useRef<FlatList<Segment>>(null);
  const scrollTarget = useRef<{ index: number; attempts: number } | null>(null);
  const scrollRetry = useRef<ReturnType<typeof setTimeout> | null>(null);
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
  const activeIndex = currentSegmentIndex(data?.segments ?? [], position);
  const activeSegment = data?.segments[activeIndex];
  const following = followPlayback && !query.trim();
  const scrollToCurrent = useCallback(() => {
    const target = scrollTarget.current;
    if (target && target.index < (list.current?.props.data?.length ?? 0)) {
      list.current?.scrollToIndex({ index: target.index, viewPosition: 0.3, animated: true });
    }
  }, []);
  useEffect(() => {
    scrollTarget.current = following && activeIndex >= 0 && !loading && !error ? { index: activeIndex, attempts: 0 } : null;
    scrollToCurrent();
    return () => {
      scrollTarget.current = null;
      if (scrollRetry.current) clearTimeout(scrollRetry.current);
    };
  }, [activeIndex, data, following, loading, error, scrollToCurrent]);
  const stopFollowing = () => {
    scrollTarget.current = null;
    if (scrollRetry.current) clearTimeout(scrollRetry.current);
    setFollowPlayback(false);
  };
  async function saveCaptionWord(segment: Segment, tokenIndex: number, word: string) {
    if (!data || savingWord.current) return;
    const source = data;
    stopFollowing();
    savingWord.current = true;
    setWordFeedback(`Saving “${word}”…`);
    try {
      const result = await saveWord(source, source.segments.indexOf(segment), tokenIndex, word);
      if (activeData.current === source) setWordFeedback(result.saved ? `Saved “${result.entry.word}”` : `“${result.entry.word}” is already saved`);
    } catch (e) {
      if (activeData.current === source) {
        setWordFeedback('');
        Alert.alert('Could not save word', e instanceof Error ? e.message : 'Please try again.');
      }
    } finally { savingWord.current = false; }
  }
  return (
    <View style={styles.panel}>
      <View style={styles.heading}>
        <View style={styles.headingText}><Text accessibilityRole="header" numberOfLines={1} style={styles.title}>Transcript</Text>
          <Text numberOfLines={1} style={styles.subtitle}>{cached ? 'Saved · ' : ''}{data?.title || 'YouTube captions'}</Text></View>
        <IconButton compact icon={RotateCw} label="Refresh transcript" disabled={loading} onPress={() => { setCopied(false); onExtract(); }} />
        <IconButton compact icon={Copy} label={copied ? 'Transcript copied' : 'Copy transcript'} disabled={!data || loading} onPress={async () => {
          if (!data) return;
          try { Clipboard.setString(await exportTranscript(data)); setCopied(true); } catch { Alert.alert('Could not copy transcript'); }
        }} />
        <IconButton compact icon={Share2} label="Share transcript" disabled={!data || loading} onPress={async () => {
          if (!data) return;
          try { await Share.share({ message: await exportTranscript(data), title: data.title }); } catch { Alert.alert('Could not share transcript'); }
        }} />
        {onSummarize && <IconButton compact icon={Sparkles} label="Summarize video with AI" disabled={!data || loading || !!error} onPress={onSummarize} />}
        <IconButton compact icon={Download} label="Download this video" disabled={downloadDisabled} onPress={onDownload} />
        <IconButton compact icon={X} label="Close transcript" onPress={onClose} />
      </View>
      {copied && <Text accessibilityLiveRegion="polite" style={styles.feedback}>Transcript copied</Text>}
      {!!wordFeedback && <Text accessibilityLiveRegion="polite" style={styles.feedback}>{wordFeedback}</Text>}
      {loading ? <View style={styles.empty}><ActivityIndicator color={colors.accent} /><Text style={styles.emptyTitle}>Extracting captions…</Text><Text style={styles.help}>You can keep watching while this loads.</Text></View> : error ? (
        <View style={styles.empty}><Text style={styles.emptyTitle}>Transcript unavailable</Text><Text accessibilityRole="alert" style={styles.help}>{error}</Text>
          <Pressable accessibilityRole="button" onPress={() => onExtract()} style={styles.retry}><Text style={styles.retryText}>Try again</Text></Pressable></View>
      ) : data ? <>
        {onPauseAfterLine && <View style={styles.pauseRow}>
          <Text style={styles.pauseLabel}>Pause after each line</Text>
          <Switch accessibilityLabel="Pause after each transcript line" accessibilityHint="Resume playback when you are ready for the next line"
            value={pauseAfterLine} onValueChange={onPauseAfterLine} trackColor={{ true: colors.accent }} />
        </View>}
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
        <View style={styles.languageRow}>
          <Text style={styles.language}>{query.trim() ? `Search results for “${query.trim()}”` : data.language} · Tap a line to play · Hold a word to save</Text>
          {!query.trim() && <Pressable accessibilityRole="button" accessibilityLabel="Follow playback" accessibilityState={{ selected: following }}
            onPress={() => setFollowPlayback(true)} style={[styles.follow, following && styles.following]}>
            <LocateFixed size={13} color={following ? colors.accent : colors.muted} />
            <Text style={[styles.followText, following && styles.selectedText]}>{following ? 'Following' : 'Follow playback'}</Text>
          </Pressable>}
        </View>
        <FlatList ref={list} data={segments} extraData={activeSegment} keyExtractor={(item, index) => `${item.start}:${index}`} contentContainerStyle={styles.list}
          onScrollBeginDrag={stopFollowing} onLayout={scrollToCurrent}
          onScrollToIndexFailed={({ index, averageItemLength }) => {
            const target = scrollTarget.current;
            if (!target || target.index !== index || target.attempts++ >= 3) return;
            list.current?.scrollToOffset({ offset: Math.max(0, (index - 1) * averageItemLength), animated: false });
            if (scrollRetry.current) clearTimeout(scrollRetry.current);
            scrollRetry.current = setTimeout(scrollToCurrent, 100);
          }}
          keyboardShouldPersistTaps="handled" ListEmptyComponent={<Text style={styles.help}>{searchError || (searching ? 'Searching…' : 'No matching lines.')}</Text>}
          renderItem={({ item }) => <Pressable accessible={false}
            accessibilityState={{ selected: item === activeSegment }}
            accessibilityHint={item === activeSegment ? 'Current line. Jump to this line and play the video' : 'Jump to this line and play the video'}
            onPress={() => { setFollowPlayback(true); onSeek(item.start); }}
            style={({ pressed }) => [styles.segment, item === activeSegment && styles.activeSegment, pressed && styles.pressedSegment]}>
            <Text style={styles.time} accessibilityRole="button" accessibilityLabel={`Play line at ${timestamp(item.start)}`}
              onPress={() => { setFollowPlayback(true); onSeek(item.start); }}>{timestamp(item.start)}</Text>
            <TranscriptWords text={item.text} selected={item === activeSegment} style={[styles.segmentText, item === activeSegment && styles.activeText]}
              onSeek={() => { setFollowPlayback(true); onSeek(item.start); }}
              onSave={(tokenIndex, word) => { void saveCaptionWord(item, tokenIndex, word); }} />
          </Pressable>} />
      </> : null}
    </View>
  );
}

const makeStyles = (colors: ThemeColors) => StyleSheet.create({
  pauseRow: { flexDirection: 'row', alignItems: 'center', gap: 12, paddingHorizontal: 20, minHeight: 48 },
  pauseLabel: { flex: 1, color: colors.ink, fontSize: 13 },
  panel: { flex: 1, backgroundColor: colors.background, borderTopWidth: 1, borderTopColor: colors.line },
  heading: { flexDirection: 'row', alignItems: 'center', paddingLeft: 12, paddingRight: 4, paddingVertical: 8 },
  headingText: { flex: 1, minWidth: 0, gap: 3 },
  title: { fontSize: 17, fontWeight: '600', letterSpacing: -0.4, color: colors.ink },
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
  languageRow: { flexDirection: 'row', alignItems: 'center', gap: 8, marginHorizontal: 20, marginTop: 8, marginBottom: 6 },
  language: { flex: 1, color: colors.muted, fontSize: 10 },
  follow: { flexDirection: 'row', alignItems: 'center', gap: 5, minHeight: 44, paddingHorizontal: 8, borderRadius: 8 },
  following: { backgroundColor: colors.accentSoft },
  followText: { fontSize: 11, color: colors.muted },
  segment: { flexDirection: 'row', gap: 12, paddingVertical: 9, paddingHorizontal: 8, minHeight: 44, borderRadius: 6, borderLeftWidth: 3, borderLeftColor: 'transparent' },
  activeSegment: { backgroundColor: colors.accentSoft, borderLeftColor: colors.accent },
  activeText: { color: colors.accent, fontWeight: '600' },
  pressedSegment: { backgroundColor: colors.accentSoft },
  time: { minWidth: 46, paddingTop: 3, fontSize: 12, color: colors.accent, fontVariant: ['tabular-nums'] },
  segmentText: { flex: 1, fontSize: 15, lineHeight: 24, color: colors.ink },
});
