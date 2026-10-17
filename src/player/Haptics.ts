import { createContext, useContext } from 'react';
import * as Haptics from 'expo-haptics';

export const HapticStrength = createContext(30);
export function haptic(strength: number) {
  if (strength <= 0) return;
  const style =
    strength > 66
      ? Haptics.ImpactFeedbackStyle.Heavy
      : strength > 33
        ? Haptics.ImpactFeedbackStyle.Medium
        : Haptics.ImpactFeedbackStyle.Light;
  void Haptics.impactAsync(style).catch(() => {});
}
export function useHaptic() {
  const strength = useContext(HapticStrength);
  return () => haptic(strength);
}
