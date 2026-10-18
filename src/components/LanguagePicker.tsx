import { useEffect, useState } from 'react';
import { ActivityIndicator, Keyboard, KeyboardAvoidingView, Modal, Platform, Pressable, SectionList, StyleSheet, Text, TextInput, View } from 'react-native';
import { Check, ChevronDown, RotateCw, X } from 'lucide-react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { languageChoices, type LanguageChoices } from '../core/languageCatalog';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import { IconButton } from './IconButton';

export function LanguagePicker({ value, onChange, excluded, disabled, label }: {
  value: string; onChange: (language: string) => void; excluded: readonly string[]; disabled: boolean; label: string;
}) {
  const colors = useThemeColors();
  const styles = useThemeStyles(makeStyles);
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [choices, setChoices] = useState<LanguageChoices | null>(null);
  const [error, setError] = useState('');
  const [revision, setRevision] = useState(0);
  // Playback updates can recreate this array without changing the exclusions.
  const excludedKey = JSON.stringify(excluded);
  useEffect(() => {
    if (!open) return;
    let active = true;
    setError('');
    const timer = setTimeout(() => {
      languageChoices(query, JSON.parse(excludedKey), value)
        .then(result => { if (active) setChoices(result); })
        .catch(failure => { if (active) setError(failure instanceof Error ? failure.message : 'Could not load languages.'); });
    }, 80);
    return () => { active = false; clearTimeout(timer); };
  }, [open, query, excludedKey, value, revision]);
  function close() { Keyboard.dismiss(); setOpen(false); setQuery(''); }

  return <>
    <Pressable accessibilityRole="button" accessibilityLabel={label} accessibilityValue={{ text: value || 'No language selected' }} accessibilityState={{ disabled, expanded: open }} disabled={disabled}
      onPress={() => { setQuery(''); setChoices(null); setError(''); setOpen(true); }} style={[styles.selector, disabled && styles.dim]}>
      <Text style={[styles.selection, !value && styles.placeholder]}>{value || 'Choose a language'}</Text><ChevronDown size={20} color={colors.muted} />
    </Pressable>
    <Modal visible={open} animationType="slide" presentationStyle="pageSheet" onRequestClose={close}>
      <SafeAreaView style={styles.safe}>
        <View style={styles.heading}><Text accessibilityRole="header" style={styles.title}>Choose a language</Text><IconButton icon={X} label="Close language picker" onPress={close} /></View>
        <KeyboardAvoidingView style={styles.fill} behavior={Platform.OS === 'ios' ? 'padding' : 'height'}>
          <View style={styles.searchRow}>
            <TextInput accessibilityLabel="Search languages" placeholder="Search languages (optional)" placeholderTextColor={colors.muted} value={query} onChangeText={setQuery}
              autoCapitalize="none" autoCorrect={false} maxLength={200} returnKeyType="done" onSubmitEditing={() => Keyboard.dismiss()} style={styles.search} />
            {!!query && <IconButton icon={X} label="Clear language search" onPress={() => setQuery('')} />}
          </View>
          {error ? <View style={styles.status}><Text accessibilityRole="alert" style={styles.error}>{error}</Text>
            <IconButton icon={RotateCw} label="Retry loading languages" onPress={() => setRevision(value => value + 1)} /></View> : !choices ?
            <ActivityIndicator accessibilityLabel="Loading languages" style={styles.status} color={colors.accent} /> :
          <SectionList sections={choices.sections} keyExtractor={option => option.code} keyboardShouldPersistTaps="handled" keyboardDismissMode="on-drag"
            stickySectionHeadersEnabled={false} contentContainerStyle={styles.list} ListEmptyComponent={<Text style={styles.empty}>No matching languages.</Text>}
            renderSectionHeader={({ section }) => <Text accessibilityRole="header" style={styles.section}>{section.title}</Text>}
            renderItem={({ item }) => <Pressable accessibilityRole="radio" accessibilityLabel={`Choose ${item.name}`} accessibilityState={{ checked: choices.selected?.code === item.code }}
              onPress={() => { onChange(item.name); close(); }} style={({ pressed }) => [styles.option, pressed && styles.pressed]}>
              <View style={styles.description}><Text style={styles.name}>{item.name}</Text>{!!item.nativeName && <Text style={styles.native}>{item.nativeName}</Text>}</View>
              {choices.selected?.code === item.code && <Check size={21} color={colors.accent} />}
            </Pressable>} />}
        </KeyboardAvoidingView>
      </SafeAreaView>
    </Modal>
  </>;
}

const makeStyles = (colors: ThemeColors) => StyleSheet.create({
  safe: { flex: 1, backgroundColor: colors.background },
  fill: { flex: 1 },
  heading: { flexDirection: 'row', alignItems: 'center', paddingHorizontal: 18, paddingVertical: 8, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line },
  title: { flex: 1, color: colors.ink, fontSize: 22, fontWeight: '600' },
  selector: { flexDirection: 'row', alignItems: 'center', gap: 12, backgroundColor: colors.surface, borderWidth: 1, borderColor: colors.line, borderRadius: 10, minHeight: 52, padding: 14 },
  selection: { flex: 1, color: colors.ink, fontSize: 16 },
  placeholder: { color: colors.muted },
  dim: { opacity: 0.4 },
  searchRow: { flexDirection: 'row', alignItems: 'center', margin: 20, backgroundColor: colors.surface, borderRadius: 10, borderWidth: 1, borderColor: colors.line },
  search: { flex: 1, minHeight: 50, color: colors.ink, padding: 14, fontSize: 16 },
  list: { paddingHorizontal: 20, paddingBottom: 32 },
  section: { color: colors.muted, fontSize: 13, fontWeight: '600', marginTop: 12, marginBottom: 8 },
  option: { flexDirection: 'row', alignItems: 'center', gap: 16, minHeight: 56, paddingVertical: 14, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line },
  pressed: { backgroundColor: colors.accentSoft },
  description: { flex: 1, gap: 4 },
  name: { color: colors.ink, fontSize: 16 },
  native: { color: colors.muted, fontSize: 13 },
  empty: { color: colors.muted, fontSize: 14, paddingVertical: 24 },
  status: { padding: 24, alignItems: 'center', gap: 12 },
  error: { color: colors.error, fontSize: 14 },
});
