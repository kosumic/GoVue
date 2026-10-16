import { useState } from 'react';
import { ActivityIndicator, Alert, Modal, Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { Check, ChevronDown, X } from 'lucide-react-native';
import { useEvent } from 'expo';
import type { VideoPlayer } from 'expo-video';
import type { PlaybackQuality } from '../core/media';
import { colors } from '../theme';
import { IconButton } from './IconButton';

export function PlayerControls({ player, speeds, qualities, height, offline, busy, onSpeed, onQuality }: {
  player: VideoPlayer; speeds: number[]; qualities: PlaybackQuality[]; height: number;
  offline: boolean; busy: boolean; onSpeed: (speed: number) => void; onQuality: (height: number) => Promise<void>;
}) {
  const [menu, setMenu] = useState<'speed' | 'quality' | null>(null);
  const rate = useEvent(player, 'playbackRateChange', { playbackRate: player.playbackRate });
  const track = useEvent(player, 'videoTrackChange', { videoTrack: player.videoTrack });
  const currentHeight = track?.videoTrack?.size.height;
  const quality = offline ? 'Saved file' : height ? `${height}p` : `Auto${currentHeight ? ` · ${currentHeight}p` : ''}`;
  const speed = rate?.playbackRate ?? player.playbackRate;
  const options = menu === 'speed' ? speeds.map(value => ({ value, label: `${value}×` }))
    : [{ value: 0, label: 'Auto' }, ...qualities.map(q => ({ value: q.height, label: q.label }))];
  return <>
    <View style={styles.controls}>
      <Pressable accessibilityRole="button" accessibilityLabel="Playback speed" accessibilityValue={{ text: `${speed}×` }}
        disabled={!speeds.length} onPress={() => setMenu('speed')} style={styles.control}>
        <Text style={styles.caption}>Speed</Text><Text style={styles.value}>{speed}×</Text><ChevronDown size={15} color={colors.accent} />
      </Pressable>
      <Pressable accessibilityRole="button" accessibilityLabel="Video resolution" accessibilityValue={{ text: quality }}
        accessibilityState={{ disabled: offline || busy || !qualities.length }} disabled={offline || busy || !qualities.length}
        onPress={() => setMenu('quality')} style={styles.control}>
        <Text style={styles.caption}>Quality</Text><Text style={styles.value}>{quality}</Text>
        {busy ? <ActivityIndicator size="small" color={colors.accent} /> : !offline && <ChevronDown size={15} color={colors.accent} />}
      </Pressable>
    </View>
    <Modal visible={menu !== null} transparent animationType="fade" onRequestClose={() => setMenu(null)}>
      <View style={styles.modal}>
        <Pressable accessibilityRole="button" accessibilityLabel="Close player settings" style={styles.backdrop} onPress={() => setMenu(null)} />
        <SafeAreaView edges={['bottom', 'left', 'right']} style={styles.sheet}>
          <View style={styles.heading}><Text accessibilityRole="header" style={styles.title}>{menu === 'speed' ? 'Playback speed' : 'Video resolution'}</Text>
            <IconButton icon={X} label="Close player settings" onPress={() => setMenu(null)} /></View>
          <ScrollView contentContainerStyle={styles.options}>
            {options.map(option => <Pressable key={option.value} accessibilityRole="radio" accessibilityLabel={option.label}
              accessibilityState={{ checked: option.value === (menu === 'speed' ? speed : height) }} style={styles.option}
              onPress={() => {
                const selectedMenu = menu; setMenu(null);
                if (selectedMenu === 'speed') onSpeed(option.value);
                else onQuality(option.value).catch(error => Alert.alert('Could not change resolution', error instanceof Error ? error.message : 'Please try again.'));
              }}>
              <Text style={styles.label}>{option.label}</Text>
              {option.value === (menu === 'speed' ? speed : height) && <Check size={20} color={colors.accent} />}
            </Pressable>)}
          </ScrollView>
        </SafeAreaView>
      </View>
    </Modal>
  </>;
}

const styles = StyleSheet.create({
  controls: { flexDirection: 'row', justifyContent: 'space-between', paddingHorizontal: 20, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line, gap: 16 },
  control: { flexDirection: 'row', alignItems: 'center', minHeight: 44, gap: 7 },
  caption: { color: colors.muted, fontSize: 12 },
  value: { color: colors.accent, fontSize: 13, fontWeight: '600' },
  modal: { flex: 1, justifyContent: 'flex-end' },
  backdrop: { ...StyleSheet.absoluteFill, backgroundColor: '#00000066' },
  sheet: { maxHeight: '75%', backgroundColor: colors.background, borderTopLeftRadius: 18, borderTopRightRadius: 18 },
  heading: { flexDirection: 'row', alignItems: 'center', paddingLeft: 20, paddingRight: 8, paddingVertical: 8, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line },
  title: { flex: 1, color: colors.ink, fontSize: 20, fontWeight: '600' },
  options: { paddingHorizontal: 20, paddingBottom: 16 },
  option: { flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', minHeight: 48, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line },
  label: { color: colors.ink, fontSize: 16 },
});
