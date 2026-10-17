import { useEffect, useRef } from 'react';
import { AccessibilityInfo, Animated, Pressable, ScrollView, StyleSheet, Text, useWindowDimensions, View } from 'react-native';
import { Gauge, MessageCircle, Settings2, SquarePen, Trash2, X } from 'lucide-react-native';
import type { SavedChat } from '../ai/useChats';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import { IconButton } from './IconButton';

export function AiHistoryDrawer({ visible, chats, selectedId, error, onClose, onNew, onOpen, onDelete, onProviders, onUsage }: {
  visible: boolean;
  chats: SavedChat[];
  selectedId?: string;
  error: string;
  onClose: () => void;
  onNew: () => void;
  onOpen: (id: string) => void;
  onDelete: (chat: SavedChat) => void;
  onProviders: () => void;
  onUsage: () => void;
}) {
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  const width = Math.min(360, useWindowDimensions().width * 0.86);
  const progress = useRef(new Animated.Value(0)).current;
  useEffect(() => {
    if (!visible) { progress.setValue(0); return; }
    let active = true;
    void AccessibilityInfo.isReduceMotionEnabled().then(reduced => {
      if (active) Animated.timing(progress, { toValue: 1, duration: reduced ? 0 : 200, useNativeDriver: true }).start();
    });
    return () => { active = false; progress.stopAnimation(); };
  }, [visible, progress]);
  if (!visible) return null;
  return <View style={s.overlay} accessibilityViewIsModal>
    <Animated.View style={[StyleSheet.absoluteFill, { opacity: progress }]}>
      <Pressable accessibilityRole="button" accessibilityLabel="Close conversations" onPress={onClose} style={s.scrim} />
    </Animated.View>
    <Animated.View style={[s.drawer, { width, transform: [{ translateX: progress.interpolate({ inputRange: [0, 1], outputRange: [-width, 0] }) }] }]}>
      <View style={s.heading}><Text accessibilityRole="header" style={s.brand}>LingoPlay</Text><IconButton icon={X} label="Close conversations" onPress={onClose} /></View>
      <Pressable accessibilityRole="button" onPress={onNew} style={s.newChat}><SquarePen size={20} color={colors.ink} /><Text style={s.newLabel}>New chat</Text></Pressable>
      <Text style={s.section}>Conversations</Text>
      <ScrollView contentContainerStyle={s.history} keyboardShouldPersistTaps="handled">
        {chats.map(chat => <View key={chat.id} style={[s.row, chat.id === selectedId && s.selected]}>
          <Pressable accessibilityRole="button" accessibilityLabel={`Open chat ${chat.title}`} accessibilityState={{ selected: chat.id === selectedId }} onPress={() => onOpen(chat.id)} style={s.entry}>
            <Text numberOfLines={1} style={s.title}>{chat.title}</Text>
            <Text numberOfLines={1} style={s.meta}>{chat.model}</Text>
          </Pressable>
          <IconButton compact icon={Trash2} label={`Delete chat ${chat.title}`} color={colors.muted} onPress={() => onDelete(chat)} />
        </View>)}
        {!chats.length && <View style={s.empty}><MessageCircle size={24} color={colors.muted} /><Text style={s.help}>Your conversations will appear here.</Text></View>}
        {!!error && <Text accessibilityRole="alert" style={s.error}>{error}</Text>}
      </ScrollView>
      <View style={s.footer}>
        <Pressable accessibilityRole="button" onPress={onProviders} style={s.action}><Settings2 size={20} color={colors.ink} /><Text style={s.actionLabel}>Models & providers</Text></Pressable>
        <Pressable accessibilityRole="button" onPress={onUsage} style={s.action}><Gauge size={20} color={colors.ink} /><Text style={s.actionLabel}>Token usage</Text></Pressable>
      </View>
    </Animated.View>
  </View>;
}

const makeStyles = (colors: ThemeColors) => StyleSheet.create({
  overlay: { position: 'absolute', top: 0, left: 0, right: 0, bottom: 0, zIndex: 10 }, scrim: { flex: 1, backgroundColor: '#0007' },
  drawer: { position: 'absolute', top: 0, left: 0, bottom: 0, backgroundColor: colors.surface, paddingHorizontal: 14 },
  heading: { flexDirection: 'row', alignItems: 'center', paddingTop: 5 }, brand: { flex: 1, paddingLeft: 10, fontSize: 20, fontWeight: '600', letterSpacing: -0.5, color: colors.ink },
  newChat: { minHeight: 50, flexDirection: 'row', alignItems: 'center', gap: 12, paddingHorizontal: 14, borderRadius: 14, backgroundColor: colors.raised, marginTop: 14, marginBottom: 28 }, newLabel: { color: colors.ink, fontSize: 15, fontWeight: '500' },
  section: { fontSize: 12, color: colors.muted, paddingHorizontal: 12, marginBottom: 8 }, history: { paddingBottom: 16 },
  row: { flexDirection: 'row', alignItems: 'center', borderRadius: 12, paddingRight: 2 }, selected: { backgroundColor: colors.raised },
  entry: { flex: 1, paddingHorizontal: 12, paddingVertical: 13, gap: 5 }, title: { color: colors.ink, fontSize: 14 }, meta: { fontSize: 11, color: colors.muted },
  empty: { paddingHorizontal: 12, paddingVertical: 24, gap: 14 }, help: { color: colors.muted, fontSize: 13, lineHeight: 20 }, error: { color: colors.error, fontSize: 13, padding: 12 },
  footer: { borderTopWidth: StyleSheet.hairlineWidth, borderColor: colors.line, paddingVertical: 10 }, action: { minHeight: 48, paddingHorizontal: 12, flexDirection: 'row', alignItems: 'center', gap: 12 }, actionLabel: { color: colors.ink, fontSize: 14 },
});
