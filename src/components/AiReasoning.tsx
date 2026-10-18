import { useEffect, useState } from 'react';
import { ActivityIndicator, Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';
import { aiRequest } from '../../modules/govue-core';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';

export const reasoningName = (value?: string | null) => !value ? 'Default' : value === 'none' ? 'Off' : value === 'xhigh' ? 'Extra high' : value[0].toUpperCase() + value.slice(1);
type Options = { levels: string[]; defaultEffort: string; group: boolean; discovered: boolean };

export function AiReasoning({ id, model, value, busy, deferred = false, onSelect }: { id: string; model: string; value: string | null; busy: boolean; deferred?: boolean; onSelect: (value: string) => void }) {
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  const [options, setOptions] = useState<Options | null>(null);
  const [loadError, setLoadError] = useState('');
  useEffect(() => {
    let active = true;
    setOptions(null); setLoadError('');
    void aiRequest<Options>({ op: 'ai_reasoning_options', id, model }).then(r => { if (active) setOptions(r); }).catch(e => { if (active) setLoadError(e.message); });
    return () => { active = false; };
  }, [id, model]);
  return <View style={s.panel}>
    {deferred && <Text style={s.help}>Applies to your next message. This response keeps its current setting.</Text>}
    {!options && !loadError && <ActivityIndicator size="small" color={colors.accent} style={s.loading} />}
    {options && <ScrollView horizontal keyboardShouldPersistTaps="always" showsHorizontalScrollIndicator={false} contentContainerStyle={s.levels}>
      {['', ...options.levels].map(level => {
        const selected = (value || '') === level;
        return <Pressable key={level} accessibilityRole="radio" accessibilityLabel={`Reasoning ${level ? reasoningName(level) : 'model default'}`} accessibilityHint={!level && options.defaultEffort ? `Saved default: ${reasoningName(options.defaultEffort)}` : 'Applies to this chat'} accessibilityState={{ checked: selected, disabled: busy }} disabled={busy} onPress={() => onSelect(level)} style={[s.choice, selected && s.selected, busy && s.disabled]}>
          <Text style={[s.label, selected && s.selectedLabel]}>{level === 'xhigh' ? 'XHigh' : reasoningName(level)}</Text>
        </Pressable>;
      })}
    </ScrollView>}
    {options && !options.discovered && <Text style={s.help}>Supported levels vary by model. Default uses its saved settings.</Text>}
    {!!value && options && !options.levels.includes(value) && <Text style={s.help}>{reasoningName(value)} selected; this model uses its closest supported level.</Text>}
    {!!loadError && <Text accessibilityRole="alert" style={s.error}>{loadError}</Text>}
  </View>;
}

const makeStyles = (colors: ThemeColors) => StyleSheet.create({
  panel: { paddingVertical: 5, borderTopWidth: StyleSheet.hairlineWidth, borderColor: colors.line, gap: 4 },
  levels: { paddingHorizontal: 10, gap: 4 }, choice: { minHeight: 44, minWidth: 44, paddingHorizontal: 8, alignItems: 'center', justifyContent: 'center', borderRadius: 12 },
  selected: { backgroundColor: colors.accentSoft }, label: { color: colors.muted, fontSize: 12, fontWeight: '500' }, selectedLabel: { color: colors.accent, fontWeight: '600' },
  loading: { height: 44 }, help: { color: colors.muted, fontSize: 11, lineHeight: 16, paddingHorizontal: 16 }, error: { color: colors.error, fontSize: 12, lineHeight: 18, paddingHorizontal: 16 }, disabled: { opacity: 0.4 },
});
