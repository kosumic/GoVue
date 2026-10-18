import { useEffect, useRef, useState } from 'react';
import { AccessibilityInfo, ActivityIndicator, Animated, Keyboard, Pressable, ScrollView, StyleSheet, Text, TextInput, useWindowDimensions, View } from 'react-native';
import { Archive, ArchiveRestore, Gauge, MessageCircle, Pencil, Pin, PinOff, Search, Settings2, SquarePen, Trash2, X } from 'lucide-react-native';
import type { SavedChat } from '../ai/useChats';
import { aiRequest } from '../../modules/govue-core';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import { IconButton } from './IconButton';
import { SearchExcerpt } from './ChatSearch';
import { PlayerSheet, SheetRow } from '../player/PlayerSheet';

export function AiHistoryDrawer({ visible, chats, selectedId, error, onClose, onNew, onOpen, onDelete, onChange, onProviders, onUsage }: {
  visible: boolean;
  chats: SavedChat[];
  selectedId?: string;
  error: string;
  onClose: () => void;
  onNew: () => void;
  onOpen: (id: string, query?: string) => void;
  onDelete: (chat: SavedChat) => void;
  onChange: (id: string, values: { title?: string; pinned?: boolean; archived?: boolean }) => Promise<void>;
  onProviders: () => void;
  onUsage: () => void;
}) {
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  const width = Math.min(360, useWindowDimensions().width * 0.86);
  const progress = useRef(new Animated.Value(0)).current;
  const [query, setQuery] = useState('');
  const [archived, setArchived] = useState(false);
  const [results, setResults] = useState<{ query: string; archived: boolean; chats: SavedChat[] } | null>(null);
  const [searchError, setSearchError] = useState('');
  const [managing, setManaging] = useState<SavedChat | null>(null);
  const [renaming, setRenaming] = useState(false);
  const [name, setName] = useState('');
  const [actionBusy, setActionBusy] = useState(false);
  const [actionError, setActionError] = useState('');
  const searching = !!query.trim();
  const remote = searching || archived;
  const ready = !remote || results?.query === query.trim() && results.archived === archived;
  const rows = remote ? ready ? results?.chats ?? [] : [] : chats;
  useEffect(() => { if (!visible) { setQuery(''); setArchived(false); setResults(null); setSearchError(''); setManaging(null); } }, [visible]);
  useEffect(() => {
    if (!visible || !remote) { setSearchError(''); return; }
    let active = true;
    setSearchError('');
    const timer = setTimeout(() => {
      void aiRequest<{ query: string; chats: SavedChat[] }>({ op: 'ai_chats', query, archived }).then(value => {
        if (active) setResults({ ...value, archived });
      }).catch(reason => { if (active) setSearchError(reason instanceof Error ? reason.message : 'Could not search conversations.'); });
    }, 150);
    return () => { active = false; clearTimeout(timer); };
  }, [visible, query, chats, archived, remote]);
  function manage(chat: SavedChat) { Keyboard.dismiss(); setManaging(chat); setRenaming(false); setName(chat.title); setActionError(''); }
  async function change(values: { title?: string; pinned?: boolean; archived?: boolean }) {
    if (!managing || actionBusy) return;
    setActionBusy(true); setActionError('');
    try { await onChange(managing.id, values); Keyboard.dismiss(); setManaging(null); }
    catch (reason) { setActionError(reason instanceof Error ? reason.message : 'Could not update this chat.'); }
    finally { setActionBusy(false); }
  }
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
      <View style={s.heading}><Text accessibilityRole="header" style={s.brand}>GoVue</Text><IconButton icon={X} label="Close conversations" onPress={onClose} /></View>
      <Pressable accessibilityRole="button" onPress={onNew} style={s.newChat}><SquarePen size={20} color={colors.ink} /><Text style={s.newLabel}>New chat</Text></Pressable>
      <View style={s.search}>
        <Search size={18} color={colors.muted} />
        <TextInput accessibilityLabel="Search conversations" placeholder="Search conversations" placeholderTextColor={colors.muted}
          value={query} onChangeText={setQuery} autoCapitalize="none" autoCorrect={false} maxLength={200} returnKeyType="search" style={s.searchInput} selectionColor={colors.action + '55'} cursorColor={colors.ink} />
        {!!query && <IconButton compact icon={X} label="Clear conversation search" onPress={() => setQuery('')} />}
      </View>
      <Text accessibilityLiveRegion="polite" style={s.section}>{searchError ? 'Search unavailable' : searching ? ready ? `${rows.length} conversation${rows.length === 1 ? '' : 's'}` : 'Searching…' : archived ? 'Archived conversations' : 'Conversations'}</Text>
      <ScrollView style={s.list} contentContainerStyle={s.history} keyboardShouldPersistTaps="handled" keyboardDismissMode="on-drag">
        {rows.map(chat => <View key={chat.id} style={[s.row, chat.id === selectedId && s.selected]}>
          <Pressable accessibilityRole="button" accessibilityLabel={`Open chat ${chat.title}${chat.pinned ? ', pinned' : ''}${chat.searchMatch ? `, ${chat.searchMatch.before}${chat.searchMatch.matched}${chat.searchMatch.after}` : ''}`} accessibilityHint="Long press for chat options" accessibilityActions={[{ name: 'longpress', label: 'Chat options' }]} onAccessibilityAction={event => { if (event.nativeEvent.actionName === 'longpress') manage(chat); }} accessibilityState={{ selected: chat.id === selectedId }} onPress={() => onOpen(chat.id, chat.searchMatch?.messageId ? query.trim() : undefined)} onLongPress={() => manage(chat)} style={s.entry}>
            <View style={s.titleRow}>{chat.pinned && <Pin size={13} color={colors.muted} />}<Text numberOfLines={1} style={[s.title, s.titleText]}>{chat.title}</Text></View>
            {searching && chat.searchMatch ? <SearchExcerpt hit={chat.searchMatch} /> : <Text numberOfLines={1} style={s.meta}>{chat.model}</Text>}
          </Pressable>
        </View>)}
        {!ready && !searchError && <ActivityIndicator style={s.empty} color={colors.muted} />}
        {ready && !rows.length && <View style={s.empty}><MessageCircle size={24} color={colors.muted} /><Text style={s.help}>{searching ? 'No conversations found. Try another word or phrase.' : archived ? 'No archived conversations.' : 'Your conversations will appear here.'}</Text></View>}
        {!!(searchError || error) && <Text accessibilityRole="alert" style={s.error}>{searchError || error}</Text>}
      </ScrollView>
      <View style={s.footer}>
        <Pressable accessibilityRole="button" accessibilityLabel={archived ? 'Back to conversations' : 'Archived chats'} onPress={() => { Keyboard.dismiss(); setSearchError(''); setArchived(!archived); }} style={s.action}><Archive size={20} color={colors.ink} /><Text style={s.actionLabel}>{archived ? 'Back to conversations' : 'Archived chats'}</Text></Pressable>
        <Pressable accessibilityRole="button" onPress={onProviders} style={s.action}><Settings2 size={20} color={colors.ink} /><Text style={s.actionLabel}>Models & providers</Text></Pressable>
        <Pressable accessibilityRole="button" onPress={onUsage} style={s.action}><Gauge size={20} color={colors.ink} /><Text style={s.actionLabel}>Token usage</Text></Pressable>
      </View>
    </Animated.View>
    <PlayerSheet visible={!!managing} title={renaming ? 'Rename chat' : 'Chat options'} onClose={() => { if (!actionBusy) setManaging(null); }} presentation={renaming ? 'dialog' : 'sheet'}
      footer={renaming ? <View style={[s.renameButtons, s.renameFooter]}>
        <Pressable accessibilityRole="button" disabled={actionBusy} onPress={() => { Keyboard.dismiss(); setManaging(null); }} style={s.renameButton}><Text style={s.actionLabel}>Cancel</Text></Pressable>
        <Pressable accessibilityRole="button" disabled={actionBusy || !name.trim()} onPress={() => { void change({ title: name }); }} style={[s.renameButton, s.saveButton, (actionBusy || !name.trim()) && s.disabled]}><Text style={s.saveLabel}>{actionBusy ? 'Saving…' : 'Save'}</Text></Pressable>
      </View> : undefined}>
      {managing && (renaming ? <View style={s.rename}>
        <TextInput accessibilityLabel="Chat name" value={name} onChangeText={setName} maxLength={200} autoFocus selectTextOnFocus editable={!actionBusy} style={s.nameInput} placeholder="Chat name" placeholderTextColor={colors.muted} selectionColor={colors.action + '55'} cursorColor={colors.ink} returnKeyType="done" onSubmitEditing={() => { void change({ title: name }); }} />
        {!!actionError && <Text accessibilityRole="alert" style={s.error}>{actionError}</Text>}
      </View> : <>
        <Text numberOfLines={2} style={s.menuTitle}>{managing.title}</Text>
        <SheetRow title={managing.pinned ? 'Unpin' : 'Pin'} icon={managing.pinned ? <PinOff /> : <Pin />} disabled={actionBusy} onPress={() => { void change({ pinned: !managing.pinned }); }} />
        <SheetRow title={managing.archived ? 'Unarchive' : 'Archive'} icon={managing.archived ? <ArchiveRestore /> : <Archive />} disabled={actionBusy} onPress={() => { void change({ archived: !managing.archived }); }} />
        <SheetRow title="Rename" icon={<Pencil />} disabled={actionBusy} onPress={() => setRenaming(true)} />
        <SheetRow title="Delete" icon={<Trash2 />} labelColor={colors.error} iconColor={colors.error} disabled={actionBusy} onPress={() => { const chat = managing; setManaging(null); onDelete(chat); }} />
        {!!actionError && <Text accessibilityRole="alert" style={s.error}>{actionError}</Text>}
      </>)}
    </PlayerSheet>
  </View>;
}

const makeStyles = (colors: ThemeColors) => StyleSheet.create({
  overlay: { position: 'absolute', top: 0, left: 0, right: 0, bottom: 0, zIndex: 10 }, scrim: { flex: 1, backgroundColor: '#0007' },
  drawer: { position: 'absolute', top: 0, left: 0, bottom: 0, backgroundColor: colors.surface, paddingHorizontal: 14 },
  heading: { flexDirection: 'row', alignItems: 'center', paddingTop: 5 }, brand: { flex: 1, paddingLeft: 10, fontSize: 20, fontWeight: '600', letterSpacing: -0.5, color: colors.ink },
  newChat: { minHeight: 50, flexDirection: 'row', alignItems: 'center', gap: 12, paddingHorizontal: 14, borderRadius: 14, backgroundColor: colors.raised, marginTop: 14, marginBottom: 12 }, newLabel: { color: colors.ink, fontSize: 15, fontWeight: '500' },
  search: { flexDirection: 'row', alignItems: 'center', gap: 8, paddingLeft: 12, paddingRight: 2, backgroundColor: colors.raised, borderRadius: 12, marginBottom: 18 }, searchInput: { flex: 1, minWidth: 0, minHeight: 48, color: colors.ink, fontSize: 14 }, list: { flex: 1 },
  section: { fontSize: 12, color: colors.muted, paddingHorizontal: 12, marginBottom: 8 }, history: { paddingBottom: 16 },
  row: { flexDirection: 'row', alignItems: 'center', borderRadius: 12, paddingRight: 2 }, selected: { backgroundColor: colors.raised },
  entry: { flex: 1, paddingHorizontal: 12, paddingVertical: 13, gap: 5 }, title: { color: colors.ink, fontSize: 14 }, meta: { fontSize: 11, color: colors.muted },
  titleRow: { flexDirection: 'row', alignItems: 'center', gap: 7 }, titleText: { flex: 1 },
  menuTitle: { color: colors.muted, fontSize: 14, lineHeight: 21, paddingHorizontal: 20, paddingBottom: 10 },
  rename: { paddingHorizontal: 20, paddingBottom: 20, gap: 16 }, nameInput: { color: colors.ink, fontSize: 16, minHeight: 52, borderWidth: 1, borderColor: colors.outline, borderRadius: 12, paddingHorizontal: 12 },
  renameButtons: { flexDirection: 'row', justifyContent: 'flex-end', gap: 10 }, renameFooter: { paddingHorizontal: 20, paddingBottom: 20 }, renameButton: { minHeight: 48, minWidth: 80, paddingHorizontal: 18, borderRadius: 24, justifyContent: 'center', alignItems: 'center' }, saveButton: { backgroundColor: colors.accent }, saveLabel: { color: colors.surface, fontSize: 14, fontWeight: '600' }, disabled: { opacity: 0.38 },
  empty: { paddingHorizontal: 12, paddingVertical: 24, gap: 14 }, help: { color: colors.muted, fontSize: 13, lineHeight: 20 }, error: { color: colors.error, fontSize: 13, padding: 12 },
  footer: { borderTopWidth: StyleSheet.hairlineWidth, borderColor: colors.line, paddingVertical: 10 }, action: { minHeight: 48, paddingHorizontal: 12, flexDirection: 'row', alignItems: 'center', gap: 12 }, actionLabel: { color: colors.ink, fontSize: 14 },
});
