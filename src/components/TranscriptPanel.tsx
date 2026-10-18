import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { ActivityIndicator, Alert, FlatList, Pressable, ScrollView, Share, StyleSheet, Switch, Text, TextInput, useWindowDimensions, View } from 'react-native';
import Clipboard from '@react-native-clipboard/clipboard';
import { Copy, Download, Info, LocateFixed, RotateCw, Search, Share2, Sparkles, Square, X } from 'lucide-react-native';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import { currentSegmentIndex, timestamp, type Transcript } from '../youtube/transcript';
import { exportTranscript, readTranscripts, type TranscriptReader, type TranscriptRow } from '../core/transcripts';
import { IconButton } from './IconButton';
import { TranscriptWords } from './TranscriptWords';
import { TranscriptTranslations } from './TranscriptTranslations';
import { WordTranslationSheet } from './WordTranslationSheet';
import { captionDetails, saveWord, type WordSelection } from '../core/vocabulary';

export function TranscriptPanel({ data, comparison = null, trackIds, comparisonLoading = false, comparisonError, onSelectTrack, loading, error, cached, position, pauseAfterLine, onPauseAfterLine, onClose, onExtract, onSeek, onPlayLine, onBeforeWordAudio, audio, onSummarize, onDownload, downloadDisabled }: {
  data: Transcript | null;
  comparison?: Transcript | null;
  trackIds?: string[];
  comparisonLoading?: boolean;
  comparisonError?: string | null;
  onSelectTrack?: (trackId: string) => void;
  loading: boolean;
  error: string | null;
  cached: boolean;
  position: number | null;
  pauseAfterLine: boolean;
  onPauseAfterLine?: (enabled: boolean) => void;
  onClose: () => void;
  onExtract?: (track?: string) => void;
  onSeek?: (seconds: number) => void;
  onPlayLine?: (index: number, source: Transcript) => void;
  onBeforeWordAudio?: () => Promise<void> | void;
  audio?: { phase: 'idle' | 'loading' | 'playing'; error: string; stop: () => void };
  onSummarize?: () => void;
  onDownload?: () => void;
  downloadDisabled?: boolean;
}) {
  const colors = useThemeColors();
  const styles = useThemeStyles(makeStyles);
  const [query, setQuery] = useState('');
  const [searchVisible, setSearchVisible] = useState(false);
  const [copied, setCopied] = useState(false);
  const [wordFeedback, setWordFeedback] = useState('');
  const [translationsVisible, setTranslationsVisible] = useState(false);
  const [wordSelection, setWordSelection] = useState<(WordSelection & { anchor?: { x: number; y: number } }) | null>(null);
  const savingWord = useRef(false);
  const activeData = useRef([data, comparison]);
  activeData.current = [data, comparison];
  useEffect(() => {
    activeData.current = [data, comparison];
    setWordFeedback('');
    setCopied(false);
    setTranslationsVisible(false);
    setWordSelection(null);
    return () => { activeData.current = []; };
  }, [data, comparison]);
  const [matches, setMatches] = useState<{ query: string; data: Transcript; comparison: Transcript | null; reader: TranscriptReader } | null>(null);
  const [searchError, setSearchError] = useState<string | null>(null);
  const [followPlayback, setFollowPlayback] = useState(true);
  const list = useRef<FlatList<TranscriptRow>>(null);
  const trackList = useRef<ScrollView>(null);
  const scrollTarget = useRef<{ index: number; attempts: number } | null>(null);
  const scrollRetry = useRef<ReturnType<typeof setTimeout> | null>(null);
  const { fontScale } = useWindowDimensions();
  useEffect(() => {
    let active = true;
    setSearchError(null);
    if (!data) return;
    const timer = setTimeout(() => {
      readTranscripts(data, comparison, query).then(reader => {
        if (active) setMatches({ query, data, comparison, reader });
      }).catch(() => { if (active) setSearchError(query.trim() ? 'Could not search this transcript. Try refreshing it.' : 'Could not load transcript lines. Try refreshing it.'); });
    }, query.trim() ? 100 : 0);
    return () => { active = false; clearTimeout(timer); };
  }, [data, comparison, query]);
  const reader = matches?.query === query && matches.data === data && matches.comparison === comparison ? matches.reader : null;
  const searching = !!data && !reader && !searchError;
  const rows = reader?.rows ?? [];
  const sources = [data, comparison];
  const selectedTracks = trackIds ?? [data?.trackId ?? ''];
  const selectionKey = selectedTracks.join('\0');
  const tracks = data ? [
    ...selectedTracks.flatMap(id => data.tracks.filter(track => track.id === id)),
    ...data.tracks.filter(track => !selectedTracks.includes(track.id) && !track.translatedFrom),
  ] : [];
  const translations = data?.tracks.filter(track => !!track.translatedFrom) ?? [];
  useEffect(() => { trackList.current?.scrollTo({ x: 0, animated: true }); }, [selectionKey]);
  const activeIndices = sources.map(source => currentSegmentIndex(source?.segments ?? [], position));
  const rowIndices = useMemo(() => {
    const indices: number[][] = [[], []];
    rows.forEach((row, index) => row.lines.forEach(line => { indices[line.track][line.index] = index; }));
    return indices;
  }, [rows]);
  const activeIndex = rowIndices[0][activeIndices[0]] ?? rowIndices[1][activeIndices[1]] ?? -1;
  const activeRow = rows[activeIndex];
  const canPlay = !!(onSeek || onPlayLine);
  function playLine(source: Transcript, index: number) {
    setFollowPlayback(true);
    if (onPlayLine) onPlayLine(index, source);
    else onSeek?.(source.segments[index].start);
  }
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
  }, [activeIndex, reader, following, loading, error, scrollToCurrent]);
  const stopFollowing = () => {
    scrollTarget.current = null;
    if (scrollRetry.current) clearTimeout(scrollRetry.current);
    setFollowPlayback(false);
  };
  async function saveCaptionWord(source: Transcript, index: number, tokenIndex: number, word: string) {
    if (savingWord.current) return;
    stopFollowing();
    savingWord.current = true;
    setWordFeedback(`Saving “${word}”…`);
    try {
      const result = await saveWord(source, index, tokenIndex, word);
      if (activeData.current.includes(source)) setWordFeedback(result.saved ? `Saved “${result.entry.word}”` : `“${result.entry.word}” is already saved`);
    } catch (e) {
      if (activeData.current.includes(source)) {
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
        <IconButton compact icon={Search} label={searchVisible ? 'Close transcript search' : 'Search transcript'} active={searchVisible} disabled={!data || loading || !!error}
          onPress={() => { setSearchVisible(visible => !visible); setQuery(''); }} />
        {onExtract && <IconButton compact icon={RotateCw} label="Refresh transcript" disabled={loading} onPress={() => { setCopied(false); onExtract(); }} />}
        <IconButton compact icon={Info} label="Caption details" disabled={!data || loading} onPress={() => {
          if (data) void captionDetails(data).then(details => Alert.alert(`${data.language} captions`, details))
            .catch(() => Alert.alert('Caption details unavailable', 'Refresh the transcript and try again.'));
        }} />
        <IconButton compact icon={Copy} label={copied ? 'Transcript copied' : 'Copy transcript'} disabled={!data || loading || comparisonLoading} onPress={async () => {
          if (!data) return;
          try { Clipboard.setString(await exportTranscript(data, comparison)); setCopied(true); } catch { Alert.alert('Could not copy transcript'); }
        }} />
        <IconButton compact icon={Share2} label="Share transcript" disabled={!data || loading || comparisonLoading} onPress={async () => {
          if (!data) return;
          try { await Share.share({ message: await exportTranscript(data, comparison), title: data.title }); } catch { Alert.alert('Could not share transcript'); }
        }} />
        {onSummarize && <IconButton compact icon={Sparkles} label="Summarize video with AI" disabled={!data || loading || !!error} onPress={onSummarize} />}
        {onDownload && <IconButton compact icon={Download} label="Download this video" disabled={downloadDisabled} onPress={onDownload} />}
        <IconButton compact icon={X} label="Close transcript" onPress={onClose} />
      </View>
      {copied && <Text accessibilityLiveRegion="polite" style={styles.feedback}>Transcript copied</Text>}
      {!!wordFeedback && <Text accessibilityLiveRegion="polite" style={styles.feedback}>{wordFeedback}</Text>}
      {audio && audio.phase !== 'idle' && <View style={styles.audioRow}>
        {audio.phase === 'loading' && <ActivityIndicator size="small" color={colors.accent} />}
        <Text accessibilityLiveRegion="polite" style={styles.audioStatus}>{audio.phase === 'loading' ? 'Loading line audio…' : 'Playing line audio'}</Text>
        <IconButton compact icon={Square} label="Stop line audio" onPress={audio.stop} />
      </View>}
      {!!audio?.error && <Text accessibilityRole="alert" style={styles.feedback}>{audio.error}</Text>}
      {loading ? <View style={styles.empty}><ActivityIndicator color={colors.accent} /><Text style={styles.emptyTitle}>Extracting captions…</Text><Text style={styles.help}>You can keep watching while this loads.</Text></View> : error ? (
        <View style={styles.empty}><Text style={styles.emptyTitle}>Transcript unavailable</Text><Text accessibilityRole="alert" style={styles.help}>{error}</Text>
          {onExtract && <Pressable accessibilityRole="button" onPress={() => onExtract()} style={styles.retry}><Text style={styles.retryText}>Try again</Text></Pressable>}</View>
      ) : data ? <>
        {(onPauseAfterLine || (canPlay && !query.trim())) && <View style={styles.playbackControls}>
          {onPauseAfterLine && <View style={styles.pauseControl}>
            <Text style={styles.pauseLabel}>Pause after each line</Text>
            <Switch accessibilityLabel="Pause after each transcript line" accessibilityHint="Resume playback when you are ready for the next line"
              value={pauseAfterLine} onValueChange={onPauseAfterLine} trackColor={{ true: colors.accent }} />
          </View>}
          {canPlay && !query.trim() && <Pressable accessibilityRole="button" accessibilityLabel="Follow playback" accessibilityState={{ selected: following }}
            onPress={() => setFollowPlayback(true)} style={[styles.follow, following && styles.following]}>
            <LocateFixed size={13} color={following ? colors.accent : colors.muted} />
            <Text style={[styles.followText, following && styles.selectedText]}>{following ? 'Following' : 'Follow playback'}</Text>
          </Pressable>}
        </View>}
        {onSelectTrack && <>
          <ScrollView ref={trackList} horizontal showsHorizontalScrollIndicator={false} style={[styles.tracks, { height: Math.max(48, Math.ceil(20 * fontScale) + 28) }]} contentContainerStyle={styles.trackContent}>
            {tracks.map(track => {
              const selected = selectedTracks.includes(track.id);
              const disabled = !selected && selectedTracks.length >= 2;
              return <Pressable key={track.id} accessibilityRole="checkbox" accessibilityLabel={track.label + (track.automatic ? ' · Auto' : '')} accessibilityState={{ checked: selected, disabled }}
                disabled={disabled} onPress={() => onSelectTrack(track.id)} style={[styles.track, selected && styles.selectedTrack, disabled && styles.disabledTrack]}>
                <Text style={[styles.trackText, selected && styles.selectedText]}>{selected ? `${selectedTracks.indexOf(track.id) + 1} · ` : ''}{track.label}{track.automatic && !track.translatedFrom ? ' · Auto' : ''}</Text>
              </Pressable>;
            })}
            {!!translations.length && <Pressable accessibilityRole="button" accessibilityLabel="Translate captions" onPress={() => setTranslationsVisible(true)} style={styles.track}>
              <Text style={styles.trackText}>Translate…</Text>
            </Pressable>}
          </ScrollView>
          <TranscriptTranslations visible={translationsVisible} tracks={translations} selected={selectedTracks} onSelect={onSelectTrack} onClose={() => setTranslationsVisible(false)} />
        </>}
        {comparisonLoading && <View style={styles.comparisonStatus}><ActivityIndicator size="small" color={colors.accent} /><Text accessibilityLiveRegion="polite" style={styles.trackText}>Loading comparison…</Text></View>}
        {!!comparisonError && <View style={styles.comparisonStatus}><Text accessibilityRole="alert" style={styles.comparisonError}>{comparisonError}</Text>
          {onExtract && <Pressable accessibilityRole="button" onPress={() => onExtract()}><Text style={styles.selectedText}>Retry</Text></Pressable>}</View>}
        {searchVisible && <View style={styles.search}><Search size={16} color={colors.muted} /><TextInput autoFocus accessibilityLabel="Search transcript" placeholder="Find in transcript" placeholderTextColor={colors.muted}
          value={query} onChangeText={setQuery} style={styles.searchInput} />
          {searching ? <ActivityIndicator size="small" color={colors.accent} /> : <Text style={styles.count}>{query.trim() ? `${rows.length} of ${reader?.total ?? 0}` : rows.length}</Text>}
          {!!query && <IconButton icon={X} label="Clear transcript search" onPress={() => setQuery('')} />}</View>}
        <FlatList ref={list} data={rows} extraData={position} keyExtractor={item => item.lines.map(line => `${line.track}:${line.index}`).join('/')} contentContainerStyle={styles.list}
          onScrollBeginDrag={stopFollowing} onLayout={scrollToCurrent}
          onScrollToIndexFailed={({ index, averageItemLength }) => {
            const target = scrollTarget.current;
            if (!target || target.index !== index || target.attempts++ >= 3) return;
            list.current?.scrollToOffset({ offset: Math.max(0, (index - 1) * averageItemLength), animated: false });
            if (scrollRetry.current) clearTimeout(scrollRetry.current);
            scrollRetry.current = setTimeout(scrollToCurrent, 100);
          }}
          keyboardShouldPersistTaps="handled" ListEmptyComponent={<Text style={styles.help}>{searchError || (searching ? 'Searching…' : 'No matching lines.')}</Text>}
          renderItem={({ item }) => <View style={[styles.captionGroup, item === activeRow && styles.activeSegment]}>
            {item.lines.map(line => {
              const source = sources[line.track];
              const segment = source?.segments[line.index];
              if (!source || !segment) return null;
              const active = activeIndices[line.track] === line.index;
              return <Pressable key={`${line.track}:${line.index}`} accessible={false}
                accessibilityState={{ selected: active }}
                accessibilityHint={canPlay ? onPlayLine ? 'Listen to this transcript line' : 'Jump to this line and play the video' : undefined}
                onPress={canPlay ? () => playLine(source, line.index) : undefined}
                style={({ pressed }) => [styles.segment, pressed && styles.pressedSegment]}>
                <Text style={styles.time} accessibilityRole={canPlay ? 'button' : undefined} accessibilityLabel={canPlay ? `Play ${source.language} line at ${timestamp(segment.start)}` : undefined}
                  onPress={canPlay ? event => { event.stopPropagation(); playLine(source, line.index); } : undefined}>{timestamp(segment.start)}</Text>
                <View style={styles.lineText}>
                  {!!comparison && <Text style={styles.lineLanguage}>{source.language}</Text>}
                  <TranscriptWords source={source} segmentIndex={line.index} position={active && onSeek ? position : null} selected={active}
                    style={[styles.segmentText, line.track === 1 && styles.comparisonText, active && styles.activeText]} activeWordStyle={styles.activeWord}
                    separatorStyle={styles.speakerSeparator}
                    onSeek={canPlay ? () => playLine(source, line.index) : undefined}
                    onSeekWord={onSeek ? seconds => { setFollowPlayback(true); onSeek(seconds); } : undefined}
                    onTranslate={(tokenIndex, word, anchor) => { stopFollowing(); setWordSelection({ source, segmentIndex: line.index, tokenIndex, word, anchor }); }}
                    onSave={(tokenIndex, word) => { void saveCaptionWord(source, line.index, tokenIndex, word); }} />
                </View>
              </Pressable>;
            })}
          </View>} />
      </> : null}
      {wordSelection && sources.includes(wordSelection.source) && !loading && !error && <WordTranslationSheet selection={wordSelection} anchor={wordSelection.anchor} onBeforeAudio={onBeforeWordAudio} onClose={() => setWordSelection(null)} />}
    </View>
  );
}

const makeStyles = (colors: ThemeColors) => StyleSheet.create({
  activeWord: { backgroundColor: colors.accent, color: colors.background, fontWeight: '700' },
  speakerSeparator: { color: colors.muted },
  playbackControls: { flexDirection: 'row', alignItems: 'center', gap: 12, paddingHorizontal: 20, minHeight: 48 },
  pauseControl: { flex: 1, flexDirection: 'row', alignItems: 'center', gap: 8 },
  pauseLabel: { flexShrink: 1, color: colors.ink, fontSize: 13 },
  panel: { flex: 1, backgroundColor: colors.background, borderTopWidth: 1, borderTopColor: colors.line },
  heading: { flexDirection: 'row', alignItems: 'center', paddingLeft: 12, paddingRight: 4, paddingVertical: 8 },
  headingText: { flex: 1, minWidth: 0, gap: 3 },
  title: { fontSize: 17, fontWeight: '600', letterSpacing: -0.4, color: colors.ink },
  subtitle: { fontSize: 11, color: colors.muted },
  feedback: { color: colors.accent, fontSize: 12, paddingHorizontal: 20, paddingBottom: 8 },
  audioRow: { flexDirection: 'row', alignItems: 'center', gap: 8, paddingLeft: 20, paddingRight: 8 },
  audioStatus: { flex: 1, color: colors.accent, fontSize: 12 },
  empty: { flex: 1, justifyContent: 'center', alignItems: 'center', padding: 28, gap: 12 },
  emptyTitle: { fontSize: 17, fontWeight: '600', color: colors.ink },
  help: { fontSize: 13, lineHeight: 21, color: colors.muted, textAlign: 'center', maxWidth: 320 },
  retry: { backgroundColor: colors.accent, paddingHorizontal: 22, paddingVertical: 13, borderRadius: 12, marginTop: 8 },
  retryText: { color: colors.surface, fontWeight: '600' },
  tracks: { flexGrow: 0, flexShrink: 0 },
  disabledTrack: { opacity: 0.4 },
  comparisonStatus: { flexDirection: 'row', alignItems: 'center', gap: 8, marginHorizontal: 20, marginBottom: 8 },
  comparisonError: { flex: 1, color: colors.muted, fontSize: 12 },
  trackContent: { paddingHorizontal: 20, paddingBottom: 10, gap: 8 },
  track: { paddingVertical: 9, paddingHorizontal: 13, borderRadius: 10, backgroundColor: colors.surface },
  selectedTrack: { backgroundColor: colors.accentSoft },
  trackText: { color: colors.muted, fontSize: 12 },
  selectedText: { color: colors.accent, fontWeight: '600' },
  search: { flexDirection: 'row', alignItems: 'center', marginHorizontal: 20, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line, gap: 8 },
  searchInput: { flex: 1, paddingVertical: 11, color: colors.ink, fontSize: 13 },
  count: { color: colors.muted, fontSize: 11 },
  list: { paddingHorizontal: 20, paddingBottom: 24 },
  follow: { flexDirection: 'row', alignItems: 'center', gap: 5, minHeight: 44, paddingHorizontal: 8, borderRadius: 8 },
  following: { backgroundColor: colors.accentSoft },
  followText: { fontSize: 11, color: colors.muted },
  captionGroup: { borderRadius: 6, borderLeftWidth: 3, borderLeftColor: 'transparent', marginBottom: 3 },
  segment: { flexDirection: 'row', gap: 12, paddingVertical: 9, paddingHorizontal: 8, minHeight: 44, borderRadius: 6 },
  lineText: { flex: 1 },
  lineLanguage: { color: colors.muted, fontSize: 10, marginBottom: 3 },
  comparisonText: { color: colors.muted },
  activeSegment: { backgroundColor: colors.accentSoft, borderLeftColor: colors.accent },
  activeText: { color: colors.accent, fontWeight: '600' },
  pressedSegment: { backgroundColor: colors.accentSoft },
  time: { minWidth: 46, paddingTop: 3, fontSize: 12, color: colors.accent, fontVariant: ['tabular-nums'] },
  segmentText: { fontSize: 15, lineHeight: 24, color: colors.ink },
});
