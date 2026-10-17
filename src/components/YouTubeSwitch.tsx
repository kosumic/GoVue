import { useEffect, useRef } from 'react';
import {
  Animated,
  I18nManager,
  Pressable,
  StyleSheet,
  View,
} from 'react-native';
import { useThemeColors } from '../theme';
import { useHaptic } from '../player/Haptics';
import { useReducedMotion } from '../player/useReducedMotion';

/** LiTube's 36×20 switch canvas inside its 48 dp interaction target. */
export function YouTubeSwitch({
  value,
  onValueChange,
  disabled = false,
  accessibilityLabel,
}: {
  value: boolean;
  onValueChange?: (value: boolean) => void;
  disabled?: boolean;
  accessibilityLabel?: string;
}) {
  const colors = useThemeColors();
  const vibrate = useHaptic();
  const reduced = useReducedMotion();
  const fraction = useRef(new Animated.Value(value ? 1 : 0)).current;
  useEffect(() => {
    if (reduced) {
      fraction.setValue(value ? 1 : 0);
      return;
    }
    const animation = Animated.spring(fraction, {
      toValue: value ? 1 : 0,
      speed: 30,
      bounciness: 0,
      useNativeDriver: true,
    });
    animation.start();
    return () => animation.stop();
  }, [value, fraction, reduced]);
  const alpha = disabled ? 0.38 : 1;
  return (
    <Pressable
      accessible={!!onValueChange}
      accessibilityRole="switch"
      accessibilityLabel={accessibilityLabel}
      accessibilityState={{ checked: value, disabled }}
      disabled={disabled || !onValueChange}
      pointerEvents={onValueChange ? 'auto' : 'none'}
      onPress={() => {
        vibrate();
        onValueChange?.(!value);
      }}
      style={s.target}
    >
      <View style={s.canvas}>
        <View
          style={[
            s.track,
            {
              backgroundColor: value ? colors.action : colors.muted,
              opacity: alpha,
            },
          ]}
        />
        <Animated.View
          style={[
            s.thumb,
            {
              opacity: alpha,
              transform: [
                {
                  translateX: fraction.interpolate({
                    inputRange: [0, 1],
                    outputRange: I18nManager.isRTL ? [16, 0] : [0, 16],
                  }),
                },
              ],
            },
          ]}
        />
      </View>
    </Pressable>
  );
}
const s = StyleSheet.create({
  target: {
    width: 48,
    height: 48,
    alignItems: 'center',
    justifyContent: 'center',
  },
  canvas: { width: 36, height: 20 },
  track: { ...StyleSheet.absoluteFill, borderRadius: 10 },
  thumb: {
    position: 'absolute',
    left: 2,
    top: 2,
    width: 16,
    height: 16,
    borderRadius: 8,
    backgroundColor: 'white',
  },
});
