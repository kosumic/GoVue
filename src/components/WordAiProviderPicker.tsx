import { useState } from 'react';
import { ActivityIndicator, KeyboardAvoidingView, Modal, Platform, Pressable, StyleSheet, Text, View } from 'react-native';
import { ChevronDown, ChevronLeft, X } from 'lucide-react-native';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import type { AiController } from '../ai/useAi';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import { AiModels } from './AiModels';
import { AiProviders } from './AiProviders';
import { IconButton } from './IconButton';

export function WordAiProviderPicker({ ai }: { ai: AiController }) {
  const colors = useThemeColors();
  const styles = useThemeStyles(makeStyles);
  const insets = useSafeAreaInsets();
  const [page, setPage] = useState<'models' | 'providers' | null>(null);
  const [selecting, setSelecting] = useState(false);
  const provider = ai.providers.find(p => p.id === ai.activeId);
  const group = ai.groups.find(g => g.id === ai.activeId);
  const model = provider?.models.find(m => m.id === provider.model);
  function close() { ai.stop(); setPage(null); }
  async function choose(id: string, model: string) {
    if (selecting) return;
    setSelecting(true);
    try { if (await ai.select(id, model)) setPage(null); }
    finally { setSelecting(false); }
  }
  return <>
    <Pressable accessibilityRole="button" accessibilityLabel="Choose AI provider and model" onPress={() => setPage('models')} style={styles.selector}>
      <View style={styles.grow}>
        <Text style={styles.label}>AI provider · examples & grammar</Text>
        <Text style={styles.value}>{group?.name || provider?.label || 'Choose a provider and model'}</Text>
        {provider && <Text style={styles.label}>{model?.name || provider.model}{!provider.enabled ? ' · Disabled' : !provider.connected || provider.needsReauth ? ' · Reconnect' : ''}</Text>}
      </View>
      <ChevronDown size={18} color={colors.ink} />
    </Pressable>
    <Modal visible={page !== null} animationType="slide" presentationStyle="fullScreen" onRequestClose={close}>
      <KeyboardAvoidingView style={[styles.page, { paddingTop: insets.top, paddingBottom: insets.bottom }]} behavior={Platform.OS === 'ios' ? 'padding' : 'height'}>
        <View style={styles.heading}>
          {page === 'providers' && <IconButton icon={ChevronLeft} label="Back to AI models" onPress={() => { ai.stop(); setPage('models'); }} />}
          <Text accessibilityRole="header" style={styles.title}>{page === 'providers' ? 'AI providers' : 'Choose AI provider and model'}</Text>
          <IconButton icon={X} label="Close AI provider picker" onPress={close} />
        </View>
        {ai.ready ? <View style={styles.grow} pointerEvents={selecting ? 'none' : 'auto'}>
          {page === 'providers' ? <AiProviders ai={ai} /> : <AiModels ai={ai} selectedId={ai.activeId ?? undefined} selectedModel={group?.name || provider?.model} onSelect={(id, model) => { void choose(id, model); }} onManage={() => setPage('providers')} />}
        </View> : ai.error ? <Text accessibilityRole="alert" style={styles.error}>{ai.error}</Text> : <ActivityIndicator accessibilityLabel="Loading AI providers" color={colors.accent} />}
        {selecting && <ActivityIndicator accessibilityLabel="Selecting AI provider" color={colors.accent} />}
      </KeyboardAvoidingView>
    </Modal>
  </>;
}

const makeStyles = (colors: ThemeColors) => StyleSheet.create({
  selector: { flexDirection: 'row', alignItems: 'center', gap: 12, minHeight: 64, padding: 12, borderWidth: StyleSheet.hairlineWidth, borderColor: colors.line, borderRadius: 10 },
  grow: { flex: 1, gap: 4 },
  label: { color: colors.muted, fontSize: 12, lineHeight: 18 },
  value: { color: colors.ink, fontSize: 14, fontWeight: '600' },
  page: { flex: 1, backgroundColor: colors.background },
  heading: { flexDirection: 'row', alignItems: 'center', paddingLeft: 16, paddingRight: 8, minHeight: 56 },
  title: { flex: 1, color: colors.ink, fontSize: 18, fontWeight: '600' },
  error: { color: colors.error, fontSize: 14, padding: 20 },
});
