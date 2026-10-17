import { FlatList, Modal, Pressable, StyleSheet, Text, View } from 'react-native';
import { X } from 'lucide-react-native';
import { useThemeStyles, type ThemeColors } from '../theme';
import type { CaptionTrack } from '../youtube/transcript';
import { IconButton } from './IconButton';

export function TranscriptTranslations({ visible, tracks, selected, onSelect, onClose }: {
  visible: boolean;
  tracks: CaptionTrack[];
  selected: string[];
  onSelect: (id: string) => void;
  onClose: () => void;
}) {
  const styles = useThemeStyles(makeStyles);
  return <Modal visible={visible} transparent animationType="slide" onRequestClose={onClose}>
    <View style={styles.backdrop}>
      <Pressable style={StyleSheet.absoluteFill} accessibilityLabel="Close translation languages" onPress={onClose} />
      <View style={styles.sheet} accessibilityViewIsModal>
        <View style={styles.heading}><Text accessibilityRole="header" style={styles.title}>Translate captions</Text>
          <IconButton icon={X} label="Close translation languages" onPress={onClose} /></View>
        <Text style={styles.help}>YouTube machine translations{selected.length >= 2 ? ' · Remove a selected language before adding another.' : ' · Select a language to compare with the original.'}</Text>
        <FlatList data={tracks} keyExtractor={track => track.id} contentContainerStyle={styles.list}
          renderItem={({ item }) => {
            const checked = selected.includes(item.id);
            const disabled = !checked && selected.length >= 2;
            return <Pressable accessibilityRole="checkbox" accessibilityState={{ checked, disabled }} disabled={disabled}
              onPress={() => { onSelect(item.id); onClose(); }} style={[styles.language, disabled && styles.disabled]}>
              <Text style={styles.label}>{checked ? '✓ ' : ''}{item.label}</Text>
            </Pressable>;
          }} />
      </View>
    </View>
  </Modal>;
}
const makeStyles = (colors: ThemeColors) => StyleSheet.create({
  backdrop: { flex: 1, justifyContent: 'flex-end', backgroundColor: '#0008' },
  sheet: { height: '75%', backgroundColor: colors.background, borderTopLeftRadius: 20, borderTopRightRadius: 20 },
  heading: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', paddingLeft: 20, paddingRight: 8, paddingTop: 10 },
  title: { color: colors.ink, fontSize: 19, fontWeight: '600' },
  help: { color: colors.muted, fontSize: 13, lineHeight: 20, paddingHorizontal: 20, paddingVertical: 12 },
  list: { paddingBottom: 40 },
  language: { paddingHorizontal: 20, minHeight: 52, justifyContent: 'center', borderBottomWidth: StyleSheet.hairlineWidth, borderBottomColor: colors.line },
  label: { color: colors.ink, fontSize: 15, lineHeight: 22, paddingVertical: 12 },
  disabled: { opacity: 0.4 },
});
