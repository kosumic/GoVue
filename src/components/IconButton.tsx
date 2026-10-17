import type { LucideIcon } from 'lucide-react-native';
import { Pressable, StyleSheet } from 'react-native';
import { useThemeColors } from '../theme';
import { useHaptic } from '../player/Haptics';

export function IconButton({
  icon: Icon,
  label,
  onPress,
  disabled = false,
  active = false,
  compact = false,
  color,
}: {
  icon: LucideIcon;
  label: string;
  onPress: () => void;
  disabled?: boolean;
  active?: boolean;
  compact?: boolean;
  color?: string;
}) {
  const vibrate = useHaptic();
  const colors = useThemeColors();
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={label}
      accessibilityState={{ disabled, selected: active }}
      disabled={disabled}
      onPress={() => {
        vibrate();
        onPress();
      }}
      style={({ pressed }) => [
        styles.button,
        compact && styles.compact,
        active && { backgroundColor: colors.accentSoft },
        (disabled || pressed) && styles.dim,
      ]}
    >
      <Icon
        size={24}
        strokeWidth={1.8}
        color={color || (active ? colors.accent : colors.ink)}
      />
    </Pressable>
  );
}

const styles = StyleSheet.create({
  button: {
    width: 48,
    height: 48,
    alignItems: 'center',
    justifyContent: 'center',
    borderRadius: 24,
  },
  compact: { width: 40, height: 40 },
  dim: { opacity: 0.35 },
});
