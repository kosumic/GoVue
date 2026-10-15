import { ArrowUpRight, Captions, Globe2, Play } from 'lucide-react-native';
import { Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';
import { YOUTUBE_HOME } from '../browser/address';
import { colors } from '../theme';

export function StartPage({ onNavigate, onFocusAddress }: { onNavigate: (url: string) => void; onFocusAddress: () => void }) {
  return (
    <ScrollView contentContainerStyle={styles.content} keyboardShouldPersistTaps="handled">
      <View style={styles.mark}><Captions color={colors.accent} size={34} strokeWidth={1.5} /></View>
      <Text style={styles.eyebrow}>YOUR BROWSER, WITH WORDS.</Text>
      <Text accessibilityRole="header" style={styles.title}>LingoPlay<Text style={styles.dot}>.</Text></Text>
      <Text style={styles.description}>Browse the web. Read along with YouTube.</Text>
      <Pressable accessibilityRole="button" onPress={() => onNavigate(YOUTUBE_HOME)} style={({ pressed }) => [styles.link, pressed && styles.pressed]}>
        <Play size={25} color={colors.ink} strokeWidth={1.6} />
        <View style={styles.linkText}><Text style={styles.linkTitle}>Open YouTube</Text><Text style={styles.linkDescription}>Find a video to read along with</Text></View>
        <ArrowUpRight size={21} color={colors.muted} />
      </Pressable>
      <Pressable accessibilityRole="button" onPress={onFocusAddress} style={({ pressed }) => [styles.link, pressed && styles.pressed]}>
        <Globe2 size={25} color={colors.ink} strokeWidth={1.6} />
        <View style={styles.linkText}><Text style={styles.linkTitle}>Go anywhere</Text><Text style={styles.linkDescription}>Enter a website or search below</Text></View>
        <ArrowUpRight size={21} color={colors.muted} />
      </Pressable>
      <View style={styles.note}><Captions size={19} color={colors.accent} /><Text style={styles.noteText}>On a YouTube video, tap Transcript to extract its available captions.</Text></View>
    </ScrollView>
  );
}

const styles = StyleSheet.create({
  content: { flexGrow: 1, justifyContent: 'center', padding: 28, paddingVertical: 42, maxWidth: 600, width: '100%', alignSelf: 'center' },
  mark: { marginBottom: 34 },
  eyebrow: { fontSize: 10, letterSpacing: 2, fontWeight: '700', color: colors.muted, marginBottom: 12 },
  title: { fontSize: 52, fontWeight: '600', letterSpacing: -3, color: colors.ink },
  dot: { color: colors.accent },
  description: { fontSize: 17, lineHeight: 25, color: colors.muted, marginTop: 12, marginBottom: 40, maxWidth: 270 },
  link: { flexDirection: 'row', alignItems: 'center', gap: 16, paddingVertical: 22, borderTopWidth: StyleSheet.hairlineWidth, borderTopColor: colors.line },
  linkText: { flex: 1, gap: 5 },
  linkTitle: { fontSize: 16, fontWeight: '600', color: colors.ink },
  linkDescription: { fontSize: 12, color: colors.muted },
  pressed: { opacity: 0.5 },
  note: { flexDirection: 'row', gap: 12, marginTop: 30, maxWidth: 320 },
  noteText: { flex: 1, color: colors.muted, fontSize: 12, lineHeight: 19 },
});
