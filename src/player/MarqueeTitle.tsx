import { useEffect, useRef, useState } from 'react';
import {
  Animated,
  Easing,
  StyleSheet,
  Text,
  useWindowDimensions,
  View,
  type StyleProp,
  type TextStyle,
} from 'react-native';
import { useReducedMotion } from './useReducedMotion';

/** LiTube's clipped, continuously scrolling title; the author stays stationary. */
export function MarqueeTitle({
  children,
  style,
  active = true,
}: {
  children: string;
  style: StyleProp<TextStyle>;
  active?: boolean;
}) {
  const reduced = useReducedMotion();
  const { fontScale } = useWindowDimensions();
  const [width, setWidth] = useState(0);
  const [measurement, setMeasurement] = useState({ key: '', width: 0 });
  const textStyle = StyleSheet.flatten(style);
  const key = JSON.stringify([
    children,
    textStyle?.fontSize,
    textStyle?.lineHeight,
    fontScale,
  ]);
  const owner = useRef(key);
  owner.current = key;
  const offset = useRef(new Animated.Value(0)).current;
  const textWidth = measurement.key === key ? measurement.width : 0;
  const moving = active && !reduced && width > 0 && textWidth > width + 1;
  const gap = width / 3;
  const distance = textWidth + gap;
  const cycle = 1200 + (distance / 30) * 1000;
  useEffect(() => {
    offset.stopAnimation();
    offset.setValue(0);
    if (!moving) return;
    const animation = Animated.loop(
      Animated.timing(offset, {
        toValue: 1,
        duration: cycle,
        easing: Easing.linear,
        useNativeDriver: true,
        isInteraction: false,
      }),
    );
    animation.start();
    return () => animation.stop();
  }, [key, moving, cycle, offset]);
  return (
    <View
      onLayout={(event) => setWidth(event.nativeEvent.layout.width)}
      accessible
      accessibilityRole="text"
      accessibilityLabel={children}
      style={[
        s.clip,
        {
          height:
            (textStyle?.lineHeight || (textStyle?.fontSize || 15) * 1.4) *
            fontScale,
        },
      ]}
    >
      <View
        pointerEvents="none"
        accessibilityElementsHidden
        importantForAccessibility="no-hide-descendants"
        style={s.measure}
      >
        <Text
          key={key}
          style={style}
          onTextLayout={({ nativeEvent }) => {
            const width = nativeEvent.lines[0]?.width;
            if (
              owner.current === key &&
              width !== undefined &&
              Number.isFinite(width)
            )
              setMeasurement((previous) =>
                previous.key === key && previous.width === width
                  ? previous
                  : { key, width },
              );
          }}
        >
          {children}
        </Text>
      </View>
      {moving ? (
        <Animated.View
          style={[
            s.row,
            {
              width: textWidth * 2 + gap,
              gap,
              transform: [
                {
                  translateX: offset.interpolate({
                    inputRange: [0, 1200 / cycle, 1],
                    outputRange: [0, 0, -distance],
                  }),
                },
              ],
            },
          ]}
        >
          <Text
            accessible={false}
            numberOfLines={1}
            style={[style, { width: textWidth }]}
          >
            {children}
          </Text>
          <Text
            accessible={false}
            numberOfLines={1}
            style={[style, { width: textWidth }]}
          >
            {children}
          </Text>
        </Animated.View>
      ) : (
        <Text
          accessible={false}
          numberOfLines={1}
          ellipsizeMode="clip"
          style={style}
        >
          {children}
        </Text>
      )}
    </View>
  );
}
const s = StyleSheet.create({
  clip: { overflow: 'hidden' },
  measure: { position: 'absolute', top: 0, left: 0, width: 100000, opacity: 0 },
  row: { flexDirection: 'row' },
});
