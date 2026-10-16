import { ActivityIndicator, Alert, KeyboardAvoidingView, Modal, Platform, Pressable, ScrollView, StyleSheet, Switch, Text, View } from 'react-native';
import { Download, Play, Share2, Trash2, X } from 'lucide-react-native';
import { SafeAreaView } from 'react-native-safe-area-context';
import { colors } from '../theme';
import { IconButton } from './IconButton';
import { LanguageSettings } from './LanguageSettings';
import type { MediaCatalog, MediaSettings } from '../core/media';
import type { DownloadEntry, useDownloads } from '../media/downloads';

export type MediaSheetMode = 'download' | 'library' | 'settings' | null;
const categories = [['sponsor', 'Sponsors'], ['selfpromo', 'Self promotion'], ['interaction', 'Subscribe reminders'], ['intro', 'Intros'], ['outro', 'Outros'], ['preview', 'Previews'], ['music_offtopic', 'Non-music sections'], ['filler', 'Filler']] as const;
const action = (work: () => Promise<unknown>) => { work().catch(e => Alert.alert('LingoPlay', e instanceof Error ? e.message : 'Please try again.')); };
const bytes = (n: number) => n > 0 ? `${(n / 1024 / 1024).toFixed(1)} MB` : '';

export function MediaSheet({ mode, onClose, catalog, loading, error, onRefresh, onDownload, downloads, onPlay, onRetry, settings, onSettings, sponsorStatus }: {
  mode: MediaSheetMode; onClose: () => void; catalog: MediaCatalog | null; loading: boolean; error: string | null;
  onRefresh: () => void; onDownload: (itag: number) => void; downloads: ReturnType<typeof useDownloads>;
  onPlay: (entry: DownloadEntry) => void; onRetry: (entry: DownloadEntry) => void;
  settings: MediaSettings; onSettings: (changes: Partial<MediaSettings>) => void; sponsorStatus: string;
}) {
  return <Modal visible={mode !== null} animationType="slide" presentationStyle="pageSheet" onRequestClose={onClose}>
    <SafeAreaView style={styles.safe}>
      <View style={styles.heading}><Text accessibilityRole="header" style={styles.title}>{mode === 'settings' ? 'Settings' : mode === 'library' ? 'Downloads' : 'Download video'}</Text><IconButton icon={X} label="Close media sheet" onPress={onClose} /></View>
      <KeyboardAvoidingView style={styles.fill} behavior={Platform.OS === 'ios' ? 'padding' : 'height'}>
      <ScrollView contentContainerStyle={styles.body} keyboardShouldPersistTaps="handled">
        {mode === 'settings' ? <>
          <LanguageSettings />
          <Text accessibilityRole="header" style={styles.section}>Playback settings</Text>
          {([
            ['adBlock', 'Block ads', 'Hide YouTube promotions, block ad requests, and skip video ads.'],
            ['sponsorBlock', 'SponsorBlock', 'Skip community-marked sections using SponsorBlock.'],
            ['backgroundPlayback', 'Background playback', 'Videos opened in the browser continue playing outside the app or with the screen off.'],
            ['autoPip', 'Automatic picture-in-picture', 'Show a floating video when you leave the native player.'],
          ] as const).map(([key, title, help]) => <View key={key} style={styles.setting}>
            <View style={styles.description}><Text style={styles.label}>{title}</Text><Text style={styles.help}>{help}</Text></View>
            <Switch accessibilityLabel={title} value={settings[key]} onValueChange={value => onSettings({ [key]: value })} trackColor={{ true: colors.accent }} />
          </View>)}
          <Text style={styles.section}>Skip categories</Text>
          {categories.map(([key, title]) => <View key={key} style={styles.setting}><Text style={[styles.label, styles.description]}>{title}</Text>
            <Switch accessibilityLabel={`Skip ${title}`} disabled={!settings.sponsorBlock} value={settings.categories.includes(key)} onValueChange={enabled => onSettings({ categories: enabled ? [...settings.categories, key] : settings.categories.filter(c => c !== key) })} trackColor={{ true: colors.accent }} /></View>)}
          <Text accessibilityLiveRegion="polite" style={styles.help}>{sponsorStatus}</Text>
          <Text style={styles.help}>SponsorBlock is a community service. Only a hash prefix of the video ID is sent to its API.</Text>
        </> : mode === 'download' ? <>
          <Text style={styles.help}>Save a video with audio as MP4, or save audio as M4A. Files remain on this device and can be played offline or shared.</Text>
          {loading ? <View style={styles.busy}><ActivityIndicator color={colors.accent} /><Text style={styles.label}>Finding available formats…</Text></View> : error ? <Text accessibilityRole="alert" style={styles.error}>{error}</Text> : null}
          {catalog && <><Text style={styles.videoTitle}>{catalog.title}</Text>
            {catalog.live ? <Text style={styles.help}>Live streams are not available as file downloads.</Text> : catalog.streams.filter(s => {
              const audio = catalog.streams.some(a => a.kind === 'audio');
              if (s.kind === 'video') return audio;
              return s.kind !== 'combined' || !audio || !catalog.streams.some(v => v.kind === 'video' && v.height === s.height);
            }).map(stream => <Pressable key={stream.itag} accessibilityRole="button" accessibilityLabel={`Download ${stream.label}`} disabled={loading} onPress={() => onDownload(stream.itag)} style={styles.row}>
              <Download size={21} color={colors.accent} /><View style={styles.description}><Text style={styles.label}>{stream.label}</Text><Text style={styles.help}>{stream.kind === 'audio' ? `${Math.round(stream.bitrate / 1000)} kbps` : 'Video with audio'}{stream.bytes ? ` · ${stream.kind === 'video' ? 'Video ' : ''}${bytes(stream.bytes)}` : ''}</Text></View>
            </Pressable>)}
          </>}
          <Pressable accessibilityRole="button" onPress={onRefresh} disabled={loading} style={styles.button}><Text style={styles.buttonText}>Refresh formats</Text></Pressable>
        </> : <>
          {!downloads.ready && <ActivityIndicator color={colors.accent} />}
          {downloads.ready && !downloads.entries.length && <Text style={styles.help}>No downloads yet. Open a video and tap Download.</Text>}
          {downloads.entries.map(entry => <View key={entry.id} style={styles.download}>
            <Text style={styles.videoTitle}>{entry.title}</Text><Text style={styles.help}>{entry.label} · {entry.state === 'complete' ? 'Saved' : entry.state} · {bytes(entry.received)}{entry.total > 0 && entry.state !== 'complete' ? ` / ${bytes(entry.total)}` : ''}</Text>
            {entry.error && <Text accessibilityRole="alert" style={styles.error}>{entry.error}</Text>}
            <View style={styles.actions}>{entry.state === 'complete' ? <>
              <IconButton icon={Play} label={`Play ${entry.title} offline`} onPress={() => onPlay(entry)} />
              <IconButton icon={Share2} label={`Share ${entry.title}`} onPress={() => action(() => downloads.share(entry))} />
            </> : entry.state === 'downloading' || entry.state === 'paused' ? <>
              <Pressable accessibilityRole="button" style={styles.smallButton} onPress={() => action(() => downloads.pause(entry.id))}><Text style={styles.buttonText}>{entry.state === 'paused' ? 'Resume' : 'Pause'}</Text></Pressable>
              <Pressable accessibilityRole="button" style={styles.smallButton} onPress={() => action(() => downloads.cancel(entry.id))}><Text style={styles.buttonText}>Cancel</Text></Pressable>
            </> : entry.state === 'failed' ? <Pressable accessibilityRole="button" style={styles.smallButton} onPress={() => onRetry(entry)}><Text style={styles.buttonText}>Open video to retry</Text></Pressable> : <ActivityIndicator color={colors.accent} />}
              {['complete', 'failed'].includes(entry.state) && <IconButton icon={Trash2} label={`Remove ${entry.title}`} onPress={() => Alert.alert('Remove download?', 'This removes the saved file from this device.', [{ text: 'Keep', style: 'cancel' }, { text: 'Remove', style: 'destructive', onPress: () => action(() => downloads.remove(entry)) }])} />}
            </View>
          </View>)}
        </>}
      </ScrollView>
      </KeyboardAvoidingView>
    </SafeAreaView>
  </Modal>;
}

const styles = StyleSheet.create({
  safe: { flex: 1, backgroundColor: colors.background },
  fill: { flex: 1 },
  heading: { flexDirection: 'row', alignItems: 'center', paddingHorizontal: 18, paddingVertical: 8, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line },
  title: { flex: 1, color: colors.ink, fontSize: 22, fontWeight: '600' },
  body: { padding: 20, gap: 16, paddingBottom: 48 },
  setting: { flexDirection: 'row', alignItems: 'center', gap: 18, paddingVertical: 8 },
  description: { flex: 1 }, label: { color: colors.ink, fontSize: 15, fontWeight: '500' },
  help: { color: colors.muted, fontSize: 13, lineHeight: 20, marginTop: 4 },
  error: { color: colors.error, fontSize: 13, lineHeight: 20 },
  section: { color: colors.ink, fontSize: 18, fontWeight: '600', marginTop: 12 },
  videoTitle: { color: colors.ink, fontSize: 17, fontWeight: '600', lineHeight: 24 },
  row: { flexDirection: 'row', alignItems: 'center', gap: 16, paddingVertical: 16, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line },
  busy: { gap: 12, alignItems: 'center', padding: 20 },
  button: { backgroundColor: colors.accentSoft, borderRadius: 12, padding: 16, alignItems: 'center' },
  buttonText: { color: colors.accent, fontSize: 14, fontWeight: '600' },
  download: { paddingVertical: 12, borderBottomWidth: StyleSheet.hairlineWidth, borderColor: colors.line, gap: 4 },
  actions: { flexDirection: 'row', alignItems: 'center', gap: 8, marginTop: 8 },
  smallButton: { padding: 12, backgroundColor: colors.accentSoft, borderRadius: 10 },
});
