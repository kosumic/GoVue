import { useCallback, useEffect, useState } from 'react';
import { ActivityIndicator, Keyboard, Pressable, StyleSheet, Text, View } from 'react-native';
import { Check, Pencil, Plus, Trash2 } from 'lucide-react-native';
import { languageLevels, languageSettings, type LanguageLevel, type LanguageSettings as Settings } from '../core/languages';
import { colors } from '../theme';
import { IconButton } from './IconButton';
import { LanguagePicker } from './LanguagePicker';

type Group = keyof Settings;
type Editor = { group: Group; index: number | null; language: string; level: LanguageLevel };
const groups: { key: Group; title: string; help: string }[] = [
  { key: 'known', title: 'Languages I know', help: 'Choose your current level in each language, including your native language.' },
  { key: 'learning', title: 'Languages I want to learn', help: 'Choose the level you want to reach. A language can also be in your known list at its current level.' },
];

export function LanguageSettings() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [editor, setEditor] = useState<Editor | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState('');
  const load = useCallback(async () => {
    setLoading(true); setError(null);
    try { setSettings(await languageSettings()); }
    catch (failure) { setError(failure instanceof Error ? failure.message : 'Could not load your languages.'); }
    finally { setLoading(false); }
  }, []);
  useEffect(() => { void load(); }, [load]);

  function edit(value: Editor) {
    setEditor(value); setError(null); setNotice('');
  }
  async function save(next: Settings) {
    setSaving(true); setError(null); setNotice('');
    try {
      setSettings(await languageSettings(next));
      setEditor(null); setNotice('Languages saved on this device.'); Keyboard.dismiss();
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : 'Could not save your languages. Please try again.');
    } finally { setSaving(false); }
  }
  function saveEditor() {
    if (!settings || !editor || saving) return;
    const entries = [...settings[editor.group]];
    const entry = { language: editor.language, level: editor.level };
    if (editor.index === null) entries.push(entry);
    else entries[editor.index] = entry;
    void save({ ...settings, [editor.group]: entries });
  }

  return <View style={styles.container}>
    <Text style={styles.intro}>Your language levels</Text>
    <Text style={styles.help}>Use the CEFR scale (Common European Framework of Reference): A1 is beginner and C2 is proficient. Choose a level to see what it means.</Text>
    {loading ? <ActivityIndicator accessibilityLabel="Loading language settings" color={colors.accent} /> : settings ? groups.map(group => <View key={group.key} style={styles.group}>
      <Text accessibilityRole="header" style={styles.title}>{group.title}</Text>
      <Text style={styles.help}>{group.help}</Text>
      {!settings[group.key].length && <Text style={styles.empty}>No languages added yet.</Text>}
      {settings[group.key].map((entry, index) => {
        const level = languageLevels.find(option => option.value === entry.level)!;
        return <View key={index} style={styles.row}>
          <View style={styles.description}><Text style={styles.label}>{entry.language}</Text><Text style={styles.help}>{group.key === 'known' ? 'Current' : 'Target'}: {entry.level === 'native' ? 'Native' : `${entry.level} · ${level.title}`}</Text></View>
          <IconButton icon={Pencil} label={`Edit ${entry.language} ${group.key === 'known' ? 'current' : 'target'} level`} disabled={saving || editor !== null} onPress={() => edit({ group: group.key, index, ...entry })} />
          <IconButton icon={Trash2} label={`Remove ${entry.language} from ${group.title.toLowerCase()}`} disabled={saving || editor !== null} onPress={() => { if (!saving) void save({ ...settings, [group.key]: settings[group.key].filter((_, i) => i !== index) }); }} />
        </View>;
      })}
      {editor?.group === group.key ? <View style={styles.editor}>
        <Text style={styles.label}>{editor.index === null ? 'Add a language' : 'Edit language'}</Text>
        <LanguagePicker label={group.key === 'known' ? 'Language I know' : 'Language I want to learn'} value={editor.language} onChange={language => setEditor({ ...editor, language })}
          disabled={saving} excluded={settings[group.key].filter((_, index) => index !== editor.index).map(entry => entry.language)} />
        <Text style={styles.label}>{group.key === 'known' ? 'Current level' : 'Target level'}</Text>
        <View accessibilityRole="radiogroup" accessibilityLabel={group.key === 'known' ? 'Current language level' : 'Target language level'} style={styles.levels}>
          {languageLevels.filter(level => group.key === 'known' || level.value !== 'native').map(level => <Pressable key={level.value} accessibilityRole="radio"
            accessibilityLabel={level.value === 'native' ? level.title : `${level.value}, ${level.title}`} accessibilityState={{ checked: editor.level === level.value, disabled: saving }} disabled={saving}
            onPress={() => setEditor({ ...editor, level: level.value })} style={[styles.level, editor.level === level.value && styles.selectedLevel]}>
            <Text style={[styles.levelText, editor.level === level.value && styles.selectedText]}>{level.value === 'native' ? 'Native' : level.value}</Text>
          </Pressable>)}
        </View>
        {languageLevels.filter(level => level.value === editor.level).map(level => <View key={level.value} accessibilityLiveRegion="polite">
          <Text style={styles.label}>{level.value === 'native' ? 'Native language' : `${level.value} · ${level.title}`}</Text><Text style={styles.help}>{level.explanation}</Text>
        </View>)}
        {error && <Text accessibilityRole="alert" style={styles.error}>{error}</Text>}
        <View style={styles.actions}>
          <Pressable accessibilityRole="button" accessibilityLabel="Save language" accessibilityState={{ disabled: saving || !editor.language.trim() }} disabled={saving || !editor.language.trim()} onPress={saveEditor} style={[styles.button, (saving || !editor.language.trim()) && styles.dim]}>
            {saving ? <ActivityIndicator size="small" color={colors.accent} /> : <Check size={18} color={colors.accent} />}<Text style={styles.buttonText}>Save language</Text>
          </Pressable>
          <Pressable accessibilityRole="button" accessibilityState={{ disabled: saving }} disabled={saving} style={styles.cancel} onPress={() => { setEditor(null); setError(null); Keyboard.dismiss(); }}><Text style={styles.buttonText}>Cancel</Text></Pressable>
        </View>
      </View> : <Pressable accessibilityRole="button" accessibilityLabel={group.key === 'known' ? 'Add a language I know' : 'Add a language I want to learn'} accessibilityState={{ disabled: saving || editor !== null }} disabled={saving || editor !== null}
        onPress={() => edit({ group: group.key, index: null, language: '', level: 'A1' })} style={[styles.button, (saving || editor !== null) && styles.dim]}>
        <Plus size={18} color={colors.accent} /><Text style={styles.buttonText}>Add language</Text>
      </Pressable>}
    </View>) : null}
    {error && !editor && <Text accessibilityRole="alert" style={styles.error}>{error}</Text>}
    {!loading && !settings && <Pressable accessibilityRole="button" style={styles.button} onPress={() => void load()}><Text style={styles.buttonText}>Retry loading languages</Text></Pressable>}
    {!!notice && <Text accessibilityLiveRegion="polite" style={styles.help}>{notice}</Text>}
  </View>;
}

const styles = StyleSheet.create({
  container: { gap: 12, paddingBottom: 24, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line },
  intro: { color: colors.ink, fontSize: 20, fontWeight: '600' },
  title: { color: colors.ink, fontSize: 17, fontWeight: '600' },
  group: { gap: 8, marginTop: 8 },
  label: { color: colors.ink, fontSize: 15, fontWeight: '500' },
  help: { color: colors.muted, fontSize: 13, lineHeight: 20 },
  empty: { color: colors.muted, fontSize: 13, fontStyle: 'italic', paddingVertical: 8 },
  row: { flexDirection: 'row', alignItems: 'center', gap: 4, paddingVertical: 4, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line },
  description: { flex: 1, gap: 4 },
  editor: { gap: 12, paddingVertical: 12 },
  levels: { flexDirection: 'row', flexWrap: 'wrap', gap: 8 },
  level: { minWidth: 48, minHeight: 48, padding: 12, borderRadius: 10, borderWidth: 1, borderColor: colors.line, alignItems: 'center', justifyContent: 'center' },
  selectedLevel: { backgroundColor: colors.accentSoft, borderColor: colors.accent },
  levelText: { color: colors.ink, fontSize: 14 },
  selectedText: { color: colors.accent, fontWeight: '700' },
  actions: { flexDirection: 'row', alignItems: 'center', flexWrap: 'wrap', gap: 8 },
  button: { flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 8, backgroundColor: colors.accentSoft, borderRadius: 10, minHeight: 48, padding: 12 },
  buttonText: { color: colors.accent, fontSize: 14, fontWeight: '600' },
  cancel: { minHeight: 48, padding: 14, justifyContent: 'center' },
  dim: { opacity: 0.4 },
  error: { color: colors.error, fontSize: 13, lineHeight: 20 },
});
