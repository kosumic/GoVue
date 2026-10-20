import { useMemo, useRef, useState } from 'react';
import { PanResponder, StyleSheet, View } from 'react-native';

export function Range({
  value,
  min = 0,
  max = 1,
  onChange,
  label,
  buffer = 0,
  markers = [],
  chapterStarts,
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
  chapterStarts?: number[];
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
  const span = max - min || 1;
  const stops = useMemo(
    () => [
      min,
      ...[...new Set(chapterStarts)]
        .filter((start) => Number.isFinite(start) && start > min && start < max)
        .sort((a, b) => a - b),
      max,
    ],
    [chapterStarts, min, max],
  );
  const buffered = Math.max(0, Math.min(1, (buffer - min) / span));
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
        style={[
          s.track,
          timeBar && s.videoTrack,
          draft !== null && timeBar && s.activeTrack,
        ]}
      >
        {stops.slice(0, -1).map((start, index) => {
          const left = ((start - min) / span) * trackWidth;
          const right = ((stops[index + 1] - min) / span) * trackWidth;
          // Clip every layer together so chapter gaps also cut the buffered
          // track and SponsorBlock markers, without covering the picture.
          const gap = timeBar ? Math.min(2, (right - left) / 3) : 0;
          const inset = index > 0 ? gap / 2 : 0;
          const sectionWidth = Math.max(
            0,
            right - left - inset - (index < stops.length - 2 ? gap / 2 : 0),
          );
          const fillWidth = (fraction: number) =>
            Math.max(
              0,
              Math.min(sectionWidth, fraction * trackWidth - left - inset),
            );
          return (
            <View
              key={start}
              style={[
                s.section,
                { left: left + inset, width: sectionWidth, backgroundColor: trackColor },
              ]}
            >
              <View style={[s.fill, s.buffer, { width: fillWidth(buffered) }]} />
              <View style={[s.fill, { width: fillWidth(ratio), backgroundColor: color }]} />
              {markers.filter((marker) =>
                Number.isFinite(marker.start) &&
                Number.isFinite(marker.end) &&
                marker.end >= marker.start &&
                marker.end >= start &&
                marker.start <= stops[index + 1],
              ).map((marker, markerIndex) => (
                <View
                  key={markerIndex}
                  style={[s.fill, {
                    left: marker.start === marker.end
                      ? Math.max(0, Math.min(trackWidth - 4, ((marker.start - min) / span) * trackWidth - 2)) - left - inset
                      : ((marker.start - min) / span) * trackWidth - left - inset,
                    width: marker.start === marker.end ? 4 : Math.max(1, ((marker.end - marker.start) / span) * trackWidth),
                    backgroundColor: marker.color,
                  }]}
                />
              ))}
            </View>
          );
        })}
        <View
          style={[
            s.thumb,
            timeBar ? s.videoThumb : s.settingThumb,
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
  timeBarTouch: { height: 32 },
  track: { height: 2 },
  videoTrack: { height: 3 },
  activeTrack: { height: 5 },
  section: { position: 'absolute', top: 0, bottom: 0, borderRadius: 1, overflow: 'hidden' },
  fill: { position: 'absolute', top: 0, bottom: 0 },
  buffer: { backgroundColor: '#ffffff80' },
  thumb: {
    position: 'absolute',
    width: 10,
    height: 10,
    top: '50%',
    marginTop: -5,
    marginLeft: -5,
    borderRadius: 5,
    backgroundColor: '#f00',
    zIndex: 3,
  },
  videoThumb: { width: 12, height: 12, marginLeft: -6, marginTop: -6, borderRadius: 6 },
  dragging: { width: 16, height: 16, marginLeft: -8, marginTop: -8, borderRadius: 8 },
  settingThumb: {
    width: 12,
    height: 12,
    marginLeft: -6,
    marginTop: -6,
    borderRadius: 6,
  },
});
