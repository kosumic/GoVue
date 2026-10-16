import { useEffect, useState, type ReactNode } from 'react';
import { ActivityIndicator, Alert, FlatList, Modal, Pressable, StyleSheet, Text, TextInput, View } from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { Play, RotateCw, Search, Trash2, X } from 'lucide-react-native';
import { listWords, removeWord, type SavedWord } from '../core/vocabulary';
import { timestamp } from '../youtube/transcript';
import { colors } from '../theme';
import { IconButton } from './IconButton';

export function SavedWordsSheet({ visible, onClose, onOpen, browserControl }: {
  visible: boolean;
  onClose: () => void;
  onOpen: (entry: SavedWord) => void;
  browserControl?: ReactNode;
}) {
  const [query, setQuery] = useState('');
  const [words, setWords] = useState<SavedWord[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [revision, setRevision] = useState(0);
  const [removing, setRemoving] = useState(false);
  useEffect(() => {
    if (!visible) return;
    let active = true;
    setLoading(true); setError('');
    const timer = setTimeout(() => {
      listWords(query).then(entries => { if (active) setWords(entries); })
        .catch(e => { if (active) setError(e instanceof Error ? e.message : 'Could not load your saved words.'); })
        .finally(() => { if (active) setLoading(false); });
    }, 100);
    return () => { active = false; clearTimeout(timer); };
  }, [visible, query, revision]);

  async function remove(entry: SavedWord) {
    setRemoving(true);
    try { await removeWord(entry.id); setRevision(value => value + 1); }
    catch (e) { Alert.alert('Could not remove word', e instanceof Error ? e.message : 'Please try again.'); }
    finally { setRemoving(false); }
  }

  return <Modal visible={visible} animationType="slide" presentationStyle="pageSheet" onRequestClose={onClose}>
    <SafeAreaView style={styles.safe}>
      <View style={styles.heading}><Text accessibilityRole="header" style={styles.title}>Saved words</Text>
        <IconButton icon={X} label="Close saved words" onPress={onClose} /></View>
      {browserControl}
      <View style={styles.search}><Search size={17} color={colors.muted} />
        <TextInput accessibilityLabel="Search saved words" placeholder="Find a word or caption" placeholderTextColor={colors.muted}
          value={query} onChangeText={setQuery} style={styles.input} autoCorrect={false} />
        {!!query && <IconButton icon={X} label="Clear saved words search" onPress={() => setQuery('')} />}</View>
      {loading ? <ActivityIndicator style={styles.status} color={colors.accent} /> : error ? <View style={styles.status}>
        <Text accessibilityRole="alert" style={styles.error}>{error}</Text>
        <IconButton icon={RotateCw} label="Retry loading saved words" onPress={() => setRevision(value => value + 1)} />
      </View> : <FlatList data={words} keyExtractor={entry => entry.id} contentContainerStyle={styles.list} keyboardShouldPersistTaps="handled"
        ListEmptyComponent={<Text style={styles.help}>{query.trim() ? 'No matching words.' : 'Hold a word in the transcript to save it here with its caption and video.'}</Text>}
        renderItem={({ item }) => <View style={styles.entry}>
          <View style={styles.wordRow}><View style={styles.label}><Text style={styles.word}>{item.word}</Text><Text style={styles.language}>{item.language}</Text></View>
            <IconButton icon={Trash2} label={`Remove ${item.word}`} disabled={removing}
              onPress={() => Alert.alert(`Remove “${item.word}”?`, 'This removes the word from your saved words.', [
                { text: 'Keep', style: 'cancel' }, { text: 'Remove', style: 'destructive', onPress: () => { void remove(item); } },
              ])} /></View>
          <Text style={styles.context}>{item.context}</Text>
          <Pressable accessibilityRole="button" accessibilityLabel={`Play ${item.title} at ${timestamp(item.start)}`}
            onPress={() => onOpen(item)} style={styles.source}>
            <Play size={15} color={colors.accent} /><Text style={styles.sourceText}>{timestamp(item.start)} · {item.title || 'YouTube video'}</Text>
          </Pressable>
        </View>} />}
    </SafeAreaView>
  </Modal>;
}

const styles = StyleSheet.create({
  safe: { flex: 1, backgroundColor: colors.background },
  heading: { flexDirection: 'row', alignItems: 'center', paddingHorizontal: 18, paddingVertical: 8, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line },
  title: { flex: 1, color: colors.ink, fontSize: 22, fontWeight: '600' },
  search: { flexDirection: 'row', alignItems: 'center', gap: 8, marginHorizontal: 20, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line },
  input: { flex: 1, color: colors.ink, fontSize: 14, paddingVertical: 14 },
  status: { padding: 24, alignItems: 'center', gap: 12 },
  error: { color: colors.error, fontSize: 13, lineHeight: 20 },
  list: { paddingHorizontal: 20, paddingBottom: 32 },
  help: { color: colors.muted, fontSize: 14, lineHeight: 22, textAlign: 'center', paddingVertical: 32 },
  entry: { paddingVertical: 16, gap: 8, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line },
  wordRow: { flexDirection: 'row', alignItems: 'center', gap: 12 },
  label: { flex: 1, gap: 4 },
  word: { color: colors.ink, fontSize: 21, fontWeight: '600' },
  language: { color: colors.muted, fontSize: 12 },
  context: { color: colors.ink, fontSize: 15, lineHeight: 24 },
  source: { flexDirection: 'row', alignItems: 'center', gap: 8, minHeight: 44, paddingVertical: 8 },
  sourceText: { flex: 1, color: colors.accent, fontSize: 13, lineHeight: 20 },
});
