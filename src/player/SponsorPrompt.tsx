import { Pressable, StyleSheet, Text, View } from 'react-native';
import { SkipForward } from './icons';

export function SponsorPrompt({
  seconds,
  highlight,
  onSkip,
  onCancel,
}: {
  seconds: number | null;
  highlight: boolean;
  onSkip: () => void;
  onCancel: () => void;
}) {
  if (seconds === null)
    return (
      <Pressable accessibilityRole="button" onPress={onSkip} style={s.chip}>
        <Text style={s.chipText}>{highlight ? 'Jump' : 'Skip'}</Text>
        <SkipForward size={14} color="white" />
      </Pressable>
    );
  return (
    <View style={s.countdown}>
      <Pressable
        accessibilityRole="button"
        accessibilityLabel="Skip now"
        onPress={onSkip}
      >
        <Text style={s.label}>Skipping in {seconds} s</Text>
      </Pressable>
      <Pressable
        accessibilityRole="button"
        accessibilityLabel="Cancel automatic skip"
        onPress={onCancel}
        style={s.cancel}
      >
        <Text style={s.cancelText}>Cancel</Text>
      </Pressable>
    </View>
  );
}
const s = StyleSheet.create({
  countdown: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 10,
    backgroundColor: '#000c',
    borderRadius: 12,
    paddingHorizontal: 14,
    paddingVertical: 8,
  },
  label: { color: 'white', fontSize: 13 },
  cancel: {
    borderRadius: 10,
    backgroundColor: '#ffffff29',
    paddingHorizontal: 8,
    paddingVertical: 3,
  },
  cancelText: { color: 'white', fontSize: 12, fontWeight: '500' },
  chip: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 6,
    backgroundColor: '#000c',
    borderRadius: 12,
    paddingHorizontal: 12,
    paddingVertical: 8,
  },
  chipText: { color: 'white', fontSize: 12, fontWeight: '500' },
});
