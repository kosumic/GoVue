import Clipboard from '@react-native-clipboard/clipboard';
import { Copy } from './icons';
import { Pressable, StyleSheet, Text, View } from 'react-native';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import type { PlaybackDiagnostics } from './usePlaybackDiagnostics';

export type InfoLine = { label: string; value: string };
export function playbackInfoRows(
  d: PlaybackDiagnostics | null,
  video: {
    title: string;
    author: string;
    videoId: string;
    quality: string;
    duration: string;
    live: boolean;
    speed: number;
    state: string;
  },
): InfoLine[] {
  const unavailable = 'Unavailable';
  const fps = (v?: number | null) =>
    v !== null && v !== undefined ? `${v.toFixed(1)} fps` : unavailable;
  const kbps = (v?: number | null) =>
    v ? `${Math.floor(v / 1000)} kbps` : unavailable;
  const rows: InfoLine[] = [
    { label: 'Media FPS', value: fps(d?.nominalFps) },
    { label: 'Rendered FPS · 5 s', value: fps(d?.renderedFps) },
    { label: 'Expected FPS', value: fps(d?.expectedFps) },
    { label: 'Video decoder', value: d?.videoDecoder || unavailable },
    { label: 'Audio decoder', value: d?.audioDecoder || unavailable },
    { label: 'Video codec', value: d?.videoCodec || unavailable },
    { label: 'Audio codec', value: d?.audioCodec || unavailable },
    {
      label: 'Dropped frames / total',
      value: d
        ? `${d.droppedFrames} / ${d.renderedFrames + d.droppedFrames} (${d.droppedRatio !== null ? `${(d.droppedRatio * 100).toFixed(1)}%` : unavailable})`
        : unavailable,
    },
    { label: 'Playback speed', value: `${video.speed}×` },
    {
      label: 'Playback / buffering',
      value: d
        ? `${d.playbackState} · ${d.bufferingMs} ms · Buffers/min: ${d.bufferCount}`
        : video.state,
    },
    {
      label: 'First frame time',
      value:
        d?.firstFrameMs !== null && d?.firstFrameMs !== undefined
          ? `${d.firstFrameMs} ms`
          : unavailable,
    },
    { label: 'Title', value: video.title },
    { label: 'Author', value: video.author || unavailable },
    { label: 'Quality', value: video.quality },
    { label: 'Duration', value: video.duration },
    ...(video.live ? [{ label: 'Type', value: 'LIVE' }] : []),
    { label: 'Video ID', value: video.videoId },
    {
      label: 'Resolution',
      value: d?.width && d.height ? `${d.width}×${d.height}` : unavailable,
    },
    { label: 'Video bitrate', value: kbps(d?.videoBitrate) },
    { label: 'Audio bitrate', value: kbps(d?.audioBitrate) },
  ];
  if (d?.audioSampleRate)
    rows.push({ label: 'Sample rate', value: `${d.audioSampleRate} Hz` });
  if (d?.audioChannels)
    rows.push({ label: 'Channels', value: String(d.audioChannels) });
  if (d?.errorCode) rows.push({ label: 'Error', value: d.errorCode });
  return rows;
}

export function copyPlaybackInfo(rows: InfoLine[], copied: () => void) {
  Clipboard.setString(
    rows.map((row) => `${row.label}: ${row.value}`).join('\n'),
  );
  copied();
}
export function PlaybackInfo({
  rows,
  onCopy,
}: {
  rows: InfoLine[];
  onCopy: () => void;
}) {
  const colors = useThemeColors();
  const s = useThemeStyles(styles);
  return (
    <View style={s.content}>
      {rows.map((row) => (
        <View key={row.label} style={s.line}>
          <View style={s.heading}>
            <Text style={s.label}>{row.label}</Text>
            <Pressable
              accessibilityRole="button"
              accessibilityLabel={`Copy ${row.label}`}
              onPress={() => copyPlaybackInfo([row], onCopy)}
              style={s.copy}
            >
              <Copy size={20} color={colors.muted} />
            </Pressable>
          </View>
          <Text selectable style={s.value}>
            {row.value}
          </Text>
        </View>
      ))}
    </View>
  );
}
export function PlaybackInfoActions({
  rows,
  onCopy,
  onClose,
}: {
  rows: InfoLine[];
  onCopy: () => void;
  onClose: () => void;
}) {
  const s = useThemeStyles(styles);
  return (
    <View style={s.actions}>
      <Pressable accessibilityRole="button" onPress={onClose} style={s.button}>
        <Text style={s.action}>Close</Text>
      </Pressable>
      <Pressable
        accessibilityRole="button"
        onPress={() => copyPlaybackInfo(rows, onCopy)}
        style={s.button}
      >
        <Text style={s.action}>Copy all</Text>
      </Pressable>
    </View>
  );
}
const styles = (colors: ThemeColors) =>
  StyleSheet.create({
    content: { paddingHorizontal: 16 },
    line: {
      paddingBottom: 12,
      marginBottom: 8,
      borderBottomWidth: StyleSheet.hairlineWidth,
      borderBottomColor: colors.line,
    },
    heading: { flexDirection: 'row', alignItems: 'center' },
    label: { flex: 1, color: colors.muted, fontSize: 12 },
    copy: {
      width: 48,
      height: 48,
      borderRadius: 24,
      alignItems: 'center',
      justifyContent: 'center',
    },
    value: { color: colors.ink, fontSize: 14 },
    actions: {
      flexDirection: 'row',
      justifyContent: 'flex-end',
      paddingHorizontal: 16,
      paddingBottom: 12,
    },
    button: {
      minWidth: 64,
      minHeight: 48,
      paddingHorizontal: 12,
      alignItems: 'center',
      justifyContent: 'center',
    },
    action: { color: colors.action, fontSize: 14, fontWeight: '500' },
  });
