import { StyleSheet, Text, TextInput, View } from 'react-native';
import { ChevronDown, ChevronUp, Search, X } from 'lucide-react-native';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import type { ChatFindController, SearchExcerpt as Excerpt } from '../ai/useChatFind';
import { IconButton } from './IconButton';

export function SearchExcerpt({ hit, lines = 2 }: { hit: Excerpt; lines?: number }) {
  const styles = useThemeStyles(makeStyles);
  return <Text numberOfLines={lines} style={styles.excerpt}>
    {hit.leading ? '…' : ''}{hit.before}<Text style={styles.match}>{hit.matched}</Text>{hit.after}{hit.trailing ? '…' : ''}
  </Text>;
}

export function ChatFindBar({ search, onClose }: { search: ChatFindController; onClose: () => void }) {
  const colors = useThemeColors();
  const styles = useThemeStyles(makeStyles);
  const hasQuery = !!search.query.trim();
  const count = search.results.matches.length;
  const canNavigate = hasQuery && search.ready && !search.busy && count > 0;
  const status = search.error || (!hasQuery ? 'Search messages in this chat' : !search.ready ? 'Searching…' : !count ? 'No matches' : `${(search.results.selected ?? 0) + 1} of ${count}${search.results.truncated ? '+' : ''}`);
  return <View style={styles.bar}>
    <View style={styles.row}>
      <Search size={19} color={colors.muted} />
      <TextInput accessibilityLabel="Find in chat" placeholder="Find in chat" placeholderTextColor={colors.muted}
        autoFocus autoCorrect={false} autoCapitalize="none" maxLength={200} value={search.query} onChangeText={search.setQuery}
        returnKeyType="search" onSubmitEditing={() => search.navigate('next')} style={styles.input} selectionColor={colors.action + '55'} cursorColor={colors.ink} />
      <IconButton compact icon={ChevronUp} label="Previous match" disabled={!canNavigate} onPress={() => search.navigate('previous')} />
      <IconButton compact icon={ChevronDown} label="Next match" disabled={!canNavigate} onPress={() => search.navigate('next')} />
      <IconButton compact icon={X} label="Close find in chat" onPress={onClose} />
    </View>
    <View style={styles.result}>
      <Text accessibilityLiveRegion="polite" accessibilityRole={search.error ? 'alert' : undefined} style={styles.status}>{status}{hasQuery && search.hit ? ` · ${search.hit.role === 'user' ? 'You' : 'LingoPlay'}` : ''}</Text>
      {hasQuery && search.hit && <SearchExcerpt hit={search.hit} />}
    </View>
  </View>;
}

const makeStyles = (colors: ThemeColors) => StyleSheet.create({
  bar: { marginHorizontal: 12, marginTop: 4, marginBottom: 8, borderRadius: 16, backgroundColor: colors.raised },
  row: { minHeight: 48, flexDirection: 'row', alignItems: 'center', paddingLeft: 14, paddingRight: 4, gap: 4 },
  input: { flex: 1, minWidth: 0, minHeight: 48, fontSize: 15, color: colors.ink, paddingHorizontal: 6 },
  result: { paddingHorizontal: 14, paddingBottom: 12, gap: 6 }, status: { fontSize: 12, lineHeight: 18, color: colors.muted },
  excerpt: { color: colors.muted, fontSize: 13, lineHeight: 20 }, match: { color: colors.ink, fontWeight: '700', backgroundColor: colors.action + '40' },
});
