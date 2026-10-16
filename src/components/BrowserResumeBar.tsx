import { useEvent } from 'expo';
import type { VideoPlayer } from 'expo-video';
import { ArrowUpRight, Globe2, Pause, Play } from 'lucide-react-native';
import { Pressable, StyleSheet, Text, View } from 'react-native';
import { colors } from '../theme';
import { IconButton } from './IconButton';

export function BrowserResumeBar({ title, player, hasPlayback, onRestore }: {
  title: string;
  player: VideoPlayer;
  hasPlayback: boolean;
  onRestore: () => void;
}) {
  const playing = useEvent(player, 'playingChange', { isPlaying: player.playing });
  const isPlaying = playing?.isPlaying ?? player.playing;
  return <View style={styles.bar}>
    <Pressable accessibilityRole="button" accessibilityLabel="Return to browser" onPress={onRestore} style={styles.restore}>
      <Globe2 size={21} color={colors.accent} />
      <View style={styles.label}><Text style={styles.action}>Return to browser</Text><Text numberOfLines={1} style={styles.title}>{title}</Text></View>
      <ArrowUpRight size={18} color={colors.accent} />
    </Pressable>
    {hasPlayback && <IconButton icon={isPlaying ? Pause : Play} label={isPlaying ? 'Pause video' : 'Play video'} onPress={() => isPlaying ? player.pause() : player.play()} />}
  </View>;
}

const styles = StyleSheet.create({
  bar: { flexDirection: 'row', alignItems: 'center', backgroundColor: colors.accentSoft, borderTopWidth: StyleSheet.hairlineWidth, borderColor: colors.line, paddingHorizontal: 12 },
  restore: { flex: 1, flexDirection: 'row', alignItems: 'center', gap: 12, paddingVertical: 12, paddingRight: 10, minHeight: 60 },
  label: { flex: 1, gap: 3 },
  action: { color: colors.accent, fontSize: 13, fontWeight: '600' },
  title: { color: colors.muted, fontSize: 12 },
});
