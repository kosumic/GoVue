import { useEffect, useState } from 'react';
import { ActivityIndicator, Alert, Image, Modal, Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';
import { FileText, X } from 'lucide-react-native';
import { File } from 'expo-file-system';
import { useSafeAreaInsets } from 'react-native-safe-area-context';
import type { ChatAttachment } from '../ai/useChatAttachments';
import { aiRequest } from '../../modules/govue-core';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import { IconButton } from './IconButton';
import { ZoomableChatImage } from './ZoomableChatImage';

export function ChatAttachments({ attachments, onRemove, disabled = false }: { attachments: ChatAttachment[]; onRemove?: (id: string) => void; disabled?: boolean }) {
  const colors = useThemeColors();
  const styles = useThemeStyles(makeStyles);
  const insets = useSafeAreaInsets();
  const [preview, setPreview] = useState<ChatAttachment | null>(null);
  const [text, setText] = useState<string | null>(null);
  const [error, setError] = useState('');
  useEffect(() => {
    if (preview?.kind !== 'text') return;
    let active = true;
    setText(null); setError('');
    void aiRequest<{ text: string }>({ op: 'ai_chat_attachment_preview', id: preview.id }).then(result => {
      if (active) setText(result.text);
    }).catch(e => { if (active) setError(e instanceof Error ? e.message : 'Could not open this file.'); });
    return () => { active = false; };
  }, [preview?.id, preview?.kind]);
  function open(file: ChatAttachment) {
    if (file.kind === 'pdf') {
      void File.preview(new File(file.uri), { mimeType: file.mimeType, title: file.name }).catch(() => Alert.alert('Could not preview PDF', 'Install a PDF viewer to open this file. You can still send it to the model.'));
    } else { setText(null); setError(''); setPreview(file); }
  }
  if (!attachments.length) return null;
  return <>
    <ScrollView horizontal showsHorizontalScrollIndicator={false} contentContainerStyle={styles.attachments}>
      {attachments.map(file => <View key={file.id} style={file.kind === 'image' ? styles.imageTile : styles.fileTile}>
        <Pressable accessibilityRole="button" accessibilityLabel={`Preview ${file.name}`} onPress={() => open(file)} style={styles.tileButton}>
          {file.kind === 'image' ? <Image source={{ uri: file.uri }} accessibilityLabel={file.name} style={styles.thumbnail} resizeMode="cover" />
            : <View style={styles.file}><FileText size={25} color={colors.accent} /><Text numberOfLines={2} style={styles.name}>{file.name}</Text><Text style={styles.detail}>{file.kind.toUpperCase()} · {Math.max(1, Math.ceil(file.bytes / 1024))} KB</Text></View>}
        </Pressable>
        {onRemove && <Pressable accessibilityRole="button" accessibilityLabel={`Remove ${file.name}`} disabled={disabled} onPress={() => onRemove(file.id)} style={styles.remove}><X size={16} color={colors.ink} /></Pressable>}
      </View>)}
    </ScrollView>
    <Modal visible={!!preview} transparent={false} animationType="fade" onRequestClose={() => setPreview(null)}>
      <View style={[styles.preview, { paddingTop: insets.top, paddingBottom: insets.bottom }]}>
        <View style={styles.toolbar}><Text numberOfLines={1} style={styles.heading}>{preview?.name}</Text><IconButton icon={X} label="Close attachment preview" onPress={() => setPreview(null)} /></View>
        {preview?.kind === 'image' ? <ZoomableChatImage key={preview.id} uri={preview.uri} name={preview.name} width={preview.width} height={preview.height} />
          : error ? <Text accessibilityRole="alert" style={styles.error}>{error}</Text>
          : text === null ? <ActivityIndicator color={colors.accent} />
          : <ScrollView contentContainerStyle={styles.textPreview}><Text selectable style={styles.text}>{text}</Text></ScrollView>}
      </View>
    </Modal>
  </>;
}
const makeStyles = (colors: ThemeColors) => StyleSheet.create({
  attachments: { gap: 10, paddingVertical: 8, paddingHorizontal: 4 }, tileButton: { flex: 1 },
  imageTile: { width: 88, height: 100 }, thumbnail: { width: 88, height: 100, borderRadius: 12, backgroundColor: colors.raised },
  fileTile: { width: 176, height: 100 }, file: { flex: 1, padding: 10, gap: 3, backgroundColor: colors.raised, borderRadius: 12 },
  name: { fontSize: 12, color: colors.ink, flexShrink: 1, paddingRight: 8 }, detail: { color: colors.muted, fontSize: 10 },
  remove: { position: 'absolute', top: -8, right: -8, minWidth: 36, minHeight: 36, alignItems: 'center', justifyContent: 'center', borderRadius: 18, backgroundColor: colors.surface },
  preview: { flex: 1, backgroundColor: colors.background }, toolbar: { flexDirection: 'row', alignItems: 'center', paddingHorizontal: 12 }, heading: { flex: 1, color: colors.ink, fontSize: 14 },
  fullImage: { flex: 1, width: '100%' }, textPreview: { padding: 20 }, text: { color: colors.ink, fontSize: 14, lineHeight: 21, fontFamily: 'monospace' }, error: { color: colors.error, padding: 20 },
});
