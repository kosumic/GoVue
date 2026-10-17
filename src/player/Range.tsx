import { useRef, useState } from 'react';
import { PanResponder, StyleSheet, View } from 'react-native';

export function Range({
  value,
  min = 0,
  max = 1,
  onChange,
  label,
  buffer = 0,
  markers = [],
  color = '#ff0000',
  trackColor = '#ffffff66',
  onPreview,
  timeBar = false,
  disabled = false,
}: {
  value: number;
  min?: number;
  max?: number;
  onChange: (value: number) => void;
  label: string;
  buffer?: number;
  markers?: { start: number; end: number; color: string }[];
  color?: string;
  trackColor?: string;
  onPreview?: (value: number | null) => void;
  timeBar?: boolean;
  disabled?: boolean;
}) {
  const width = useRef(1);
  const [trackWidth, setTrackWidth] = useState(1);
  const [draft, setDraft] = useState<number | null>(null);
  const current = useRef({ value, min, max, onChange, onPreview, disabled });
  current.current = { value, min, max, onChange, onPreview, disabled };
  const fromX = (x: number) => {
    const p = current.current;
    return (
      p.min + Math.max(0, Math.min(1, x / width.current)) * (p.max - p.min)
    );
  };
  const responder = useRef(
    PanResponder.create({
      onStartShouldSetPanResponder: () => !current.current.disabled,
      onMoveShouldSetPanResponder: () => !current.current.disabled,
      onPanResponderGrant: (e) => {
        const value = fromX(e.nativeEvent.locationX);
        setDraft(value);
        current.current.onPreview?.(value);
      },
      onPanResponderMove: (e) => {
        const value = fromX(e.nativeEvent.locationX);
        setDraft(value);
        current.current.onPreview?.(value);
      },
      onPanResponderRelease: (e) => {
        current.current.onChange(fromX(e.nativeEvent.locationX));
        setDraft(null);
        current.current.onPreview?.(null);
      },
      onPanResponderTerminate: () => {
        setDraft(null);
        current.current.onPreview?.(null);
      },
    }),
  ).current;
  const ratio = Math.max(
    0,
    Math.min(1, ((draft ?? value) - min) / (max - min || 1)),
  );
  return (
    <View
      accessibilityRole="adjustable"
      accessibilityLabel={label}
      accessibilityState={{ disabled }}
      accessibilityValue={{
        min,
        max,
        now: Math.max(
          min,
          Math.min(max, Math.round((draft ?? value) * 100) / 100),
        ),
      }}
      accessibilityActions={[{ name: 'increment' }, { name: 'decrement' }]}
      onAccessibilityAction={(e) =>
        !disabled &&
        onChange(
          Math.max(
            min,
            Math.min(
              max,
              value +
                ((e.nativeEvent.actionName === 'increment' ? 1 : -1) *
                  (max - min)) /
                  20,
            ),
          ),
        )
      }
      onLayout={(e) => {
        width.current = Math.max(1, e.nativeEvent.layout.width);
        setTrackWidth(width.current);
      }}
      style={[s.touch, timeBar && s.timeBarTouch]}
      {...responder.panHandlers}
    >
      <View
        pointerEvents="none"
        style={[s.track, { backgroundColor: trackColor }]}
      >
        <View
          style={[
            s.buffer,
            {
              width: `${Math.max(0, Math.min(1, buffer / (max || 1))) * 100}%`,
            },
          ]}
        />
        {markers
          .filter(
            (m) =>
              Number.isFinite(m.start) &&
              Number.isFinite(m.end) &&
              m.end >= m.start &&
              m.end >= min &&
              m.start <= max,
          )
          .map((m, i) => (
            <View
              key={i}
              style={[
                s.marker,
                {
                  left:
                    m.start === m.end
                      ? Math.max(
                          0,
                          Math.min(
                            trackWidth - 4,
                            ((m.start - min) / (max - min || 1)) * trackWidth -
                              2,
                          ),
                        )
                      : `${Math.max(0, Math.min(1, (m.start - min) / (max - min || 1))) * 100}%`,
                  width:
                    m.start === m.end
                      ? 4
                      : `${Math.max(0, (Math.min(max, m.end) - Math.max(min, m.start)) / (max - min || 1)) * 100}%`,
                  minWidth: 1,
                  backgroundColor: m.color,
                },
              ]}
            />
          ))}
        <View
          style={[
            s.progress,
            { width: `${ratio * 100}%`, backgroundColor: color },
          ]}
        />
        <View
          style={[
            s.thumb,
            !timeBar && s.settingThumb,
            { left: `${ratio * 100}%`, backgroundColor: color },
            draft !== null && timeBar && s.dragging,
          ]}
        />
      </View>
    </View>
  );
}
const s = StyleSheet.create({
  touch: { height: 48, justifyContent: 'center' },
  timeBarTouch: { height: 48, marginBottom: -24 },
  track: { height: 2, backgroundColor: '#ffffff66' },
  buffer: { position: 'absolute', height: 2, backgroundColor: '#fff' },
  progress: { position: 'absolute', height: 2, backgroundColor: '#f00' },
  marker: { position: 'absolute', height: 2, top: 0, zIndex: 2 },
  thumb: {
    position: 'absolute',
    width: 10,
    height: 10,
    top: -4,
    marginLeft: -5,
    borderRadius: 5,
    backgroundColor: '#f00',
    zIndex: 3,
  },
  dragging: { width: 14, height: 14, marginLeft: -7, top: -6, borderRadius: 7 },
  settingThumb: {
    width: 12,
    height: 12,
    marginLeft: -6,
    top: -5,
    borderRadius: 6,
  },
});
