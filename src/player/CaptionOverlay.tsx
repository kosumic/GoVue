import { useState } from 'react';
import { StyleSheet, Text, View } from 'react-native';
import type { SubtitleStyle } from '../core/media';
import type { CaptionLayout } from './captions';

export function CaptionOverlay({
  text,
  sourceText,
  segmentIndex,
  onLayout,
  appearance,
  width,
  height,
  controls,
}: {
  text: string;
  sourceText: string;
  segmentIndex: number;
  onLayout: (layout: CaptionLayout) => void;
  appearance: SubtitleStyle;
  width: number;
  height: number;
  controls: boolean;
}) {
  const [cueHeight, setCueHeight] = useState(0);
  const clear = controls ? 52 : 12;
  const cueStyle = [s.cue, {
    fontSize: 14 * appearance.scale,
    color: appearance.color,
    fontWeight: appearance.bold ? '700' as const : '400' as const,
    paddingHorizontal: 8 * appearance.scale,
    paddingVertical: 2 * appearance.scale,
    backgroundColor: appearance.background === 'none' ? 'transparent' : appearance.background === 'opaque' ? '#000' : '#0009',
  }];
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
        },
      ]}
    >
      <Text
        accessible={false}
        textBreakStrategy="simple"
        accessibilityElementsHidden
        importantForAccessibility="no-hide-descendants"
        style={[cueStyle, s.measurement]}
        onTextLayout={({ nativeEvent }) => onLayout({ sourceText, segmentIndex, lines: nativeEvent.lines.map(line => line.text) })}
      >
        {sourceText}
      </Text>
      <Text
        numberOfLines={2}
        ellipsizeMode="clip"
        textBreakStrategy="simple"
        style={cueStyle}
      >
        {text}
      </Text>
    </View>
  );
}
const s = StyleSheet.create({
  cues: { position: 'absolute', alignItems: 'center' },
  cue: { textAlign: 'center', borderRadius: 4, maxWidth: '100%' },
  measurement: { position: 'absolute', top: 0, width: '100%', opacity: 0 },
});
