import type { LucideIcon } from 'lucide-react-native';
import { Pressable, StyleSheet } from 'react-native';
import { colors } from '../theme';

export function IconButton({ icon: Icon, label, onPress, disabled = false, active = false }: {
  icon: LucideIcon;
  label: string;
  onPress: () => void;
  disabled?: boolean;
  active?: boolean;
}) {
  return (
    <Pressable accessibilityRole="button" accessibilityLabel={label} accessibilityState={{ disabled, selected: active }}
      disabled={disabled} onPress={onPress}
      style={({ pressed }) => [styles.button, active && styles.active, (disabled || pressed) && styles.dim]}>
      <Icon size={21} strokeWidth={1.8} color={active ? colors.accent : colors.ink} />
    </Pressable>
  );
}

const styles = StyleSheet.create({
  button: { width: 46, height: 46, alignItems: 'center', justifyContent: 'center', borderRadius: 14 },
  active: { backgroundColor: colors.accentSoft },
  dim: { opacity: 0.35 },
});
