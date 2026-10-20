import { useRef, useState } from 'react';
import { StyleSheet, Text, View } from 'react-native';

type Labels = { time: string; quality: string };

/** Use the same native font metrics as the visible controls, including font scaling. */
export function usePlayerChromeMeasure(
  labels: Labels,
  fontSize: number,
  fontScale: number,
  width: number,
  hasChapters = false,
) {
  const key = JSON.stringify([labels, fontSize, fontScale]);
  const current = useRef(key);
  current.current = key;
  const [measurement, setMeasurement] = useState<{
    key: string;
    widths: Partial<Record<keyof Labels, number>>;
  } | null>(null);
  const widths = measurement?.key === key ? measurement.widths : {};
  const ready = widths.time !== undefined && widths.quality !== undefined;
  const showSpeed =
    ready && widths.time! + 4 * 44 + (hasChapters ? 44 : 0) + 24 <= width;
  const showChapterTitle =
    ready && widths.time! + (showSpeed ? 4 : 3) * 44 + 96 + 24 <= width;
  const compact =
    !ready ||
    widths.time! +
      Math.max(44, widths.quality! + 12) +
      (showSpeed ? 4 : 3) * 44 + // Transcript, AI, fullscreen, and speed if it fits.
      (hasChapters ? (showChapterTitle ? 96 : 44) : 0) +
      24 >
      width;
  const probe = (
    <View
      pointerEvents="none"
      accessibilityElementsHidden
      importantForAccessibility="no-hide-descendants"
      style={s.probe}
    >
      {(Object.keys(labels) as (keyof Labels)[]).map((name) => (
        <Text
          key={`${key}:${name}`}
          style={[s.label, { fontSize }, name === 'time' && s.time]}
          onTextLayout={({ nativeEvent }) => {
            const measured = nativeEvent.lines[0]?.width;
            if (
              current.current !== key ||
              measured === undefined ||
              !Number.isFinite(measured) ||
              measured < 0
            )
              return;
            setMeasurement((previous) => {
              const widths = previous?.key === key ? previous.widths : {};
              if (previous?.key === key && widths[name] === measured)
                return previous;
              return { key, widths: { ...widths, [name]: measured } };
            });
          }}
        >
          {labels[name]}
        </Text>
      ))}
    </View>
  );
  return { compact, showSpeed, showChapterTitle, probe };
}

export function playerSpeedLabel(speed: number) {
  return `${Number.isInteger(speed) ? speed.toFixed(1) : speed}x`;
}

const s = StyleSheet.create({
  probe: { position: 'absolute', width: 10000, opacity: 0, left: 0, top: 0 },
  label: { color: 'white' },
  time: { fontVariant: ['tabular-nums'] },
});
