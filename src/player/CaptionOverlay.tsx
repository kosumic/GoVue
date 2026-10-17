import { useState } from 'react';
import { StyleSheet, Text, View } from 'react-native';
import type { SubtitleStyle } from '../core/media';

export function CaptionOverlay({
  text,
  appearance,
  width,
  height,
  controls,
}: {
  text: string;
  appearance: SubtitleStyle;
  width: number;
  height: number;
  controls: boolean;
}) {
  const [cueHeight, setCueHeight] = useState(0);
  const clear = controls ? 52 : 12;
  return (
    <View
      pointerEvents="none"
      onLayout={(event) => setCueHeight(event.nativeEvent.layout.height)}
      style={[
        s.cues,
        {
          width: width * 0.92,
          left: (width * 0.08 * (appearance.horizontal + 1)) / 2,
          top:
            clear +
            Math.max(0, height - clear * 2 - cueHeight) * appearance.vertical,
          gap: 2 * appearance.scale,
        },
      ]}
    >
      {text
        .split('\n')
        .filter(Boolean)
        .map((line, index) => (
          <Text
            key={index}
            style={[
              s.cue,
              {
                fontSize: 14 * appearance.scale,
                color: appearance.color,
                fontWeight: appearance.bold ? '700' : '400',
                paddingHorizontal: 8 * appearance.scale,
                paddingVertical: 2 * appearance.scale,
                backgroundColor:
                  appearance.background === 'none'
                    ? 'transparent'
                    : appearance.background === 'opaque'
                      ? '#000'
                      : '#0009',
              },
            ]}
          >
            {line}
          </Text>
        ))}
    </View>
  );
}
const s = StyleSheet.create({
  cues: { position: 'absolute', alignItems: 'center' },
  cue: { textAlign: 'center', borderRadius: 4, maxWidth: '100%' },
});
