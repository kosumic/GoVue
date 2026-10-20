import { useEffect, useMemo, useRef, useState } from 'react';
import {
  FlatList,
  Image,
  Pressable,
  StyleSheet,
  Text,
  useWindowDimensions,
  View,
} from 'react-native';
import type { MediaChapter, SponsorSegment } from '../core/media';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import { PlayerSheet } from './PlayerSheet';
import { Play, Share2 } from './icons';
import { useHaptic } from './Haptics';

type Row = {
  title: string;
  start: number;
  chapter?: MediaChapter;
  color?: string;
};
const emptyChapters: MediaChapter[] = [];
const emptySegments: SponsorSegment[] = [];

function ChapterThumbnail({
  chapter,
  videoId,
  active,
  width,
}: {
  chapter: MediaChapter;
  videoId: string;
  active: boolean;
  width: number;
}) {
  const [failed, setFailed] = useState(false);
  const s = useThemeStyles(makeStyles);
  return (
    <View style={[s.thumbnail, { width }]}>
      <Image
        accessible={false}
        source={{ uri: (!failed && chapter.previewUrl) || `https://i.ytimg.com/vi/${videoId}/hqdefault.jpg` }}
        resizeMode="cover"
        style={StyleSheet.absoluteFill}
        onError={() => setFailed(true)}
      />
      {active && (
        <View style={s.thumbnailShade}>
          <View style={s.thumbnailPlay}>
            <Play size={22} color="white" />
          </View>
        </View>
      )}
    </View>
  );
}

/** Thumbnail chapters, with SponsorBlock segments as the no-chapters fallback. */
export function SegmentsMenu({
  visible,
  videoId,
  chapters = emptyChapters,
  segments = emptySegments,
  selected,
  maxHeight,
  formatTime,
  categoryLabel,
  categoryColor,
  onJump,
  onShare,
  onClose,
}: {
  visible: boolean;
  videoId: string;
  chapters?: MediaChapter[];
  segments?: SponsorSegment[];
  selected: number | null;
  maxHeight?: number;
  formatTime: (seconds: number) => string;
  categoryLabel: (category: string) => string;
  categoryColor: (category: string) => string;
  onJump: (position: number, title: string) => void;
  onShare: (position: number) => void;
  onClose: () => void;
}) {
  const screen = useWindowDimensions();
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  const vibrate = useHaptic();
  const [preview, setPreview] = useState<MediaChapter | null>(null);
  const list = useRef<FlatList<Row> | null>(null);
  const positioned = useRef(false);
  const retry = useRef<ReturnType<typeof setTimeout> | null>(null);
  const attempts = useRef(0);
  const thumbnailWidth = Math.min(156, (screen.width - 48) * 0.42);
  const rows = useMemo<Row[]>(
    () =>
      chapters.length
        ? chapters.map((chapter) => ({
            title: chapter.title || 'Chapter',
            start: chapter.start,
            chapter,
          }))
        : segments.map((segment) => ({
            title: categoryLabel(segment.category),
            start: segment.start,
            color: categoryColor(segment.category),
          })),
    [chapters, segments, categoryLabel, categoryColor],
  );
  useEffect(() => {
    setPreview(null);
    positioned.current = false;
    attempts.current = 0;
    return () => {
      if (retry.current) clearTimeout(retry.current);
    };
  }, [visible, videoId]);
  useEffect(() => {
    if (!visible || selected === null || positioned.current || preview) return;
    positioned.current = true;
    list.current?.scrollToIndex({ index: selected, animated: false, viewPosition: 0.3 });
  }, [visible, videoId, selected, preview]);
  const jump = (row: { start: number; title: string }) => {
    vibrate();
    onJump(row.start, row.title || 'Chapter');
  };
  const closePreview = () => {
    positioned.current = false;
    attempts.current = 0;
    setPreview(null);
  };
  return (
    <PlayerSheet
      visible={visible}
      title={preview ? 'Chapter preview' : chapters.length ? 'Chapters' : 'Segments'}
      maxHeight={preview ? undefined : maxHeight}
      backgroundColor={colors.background}
      presentation={preview ? 'dialog' : 'sheet'}
      scrollable={!!preview}
      onClose={preview ? closePreview : onClose}
      footer={
        preview ? (
          <View style={s.actions}>
            <Pressable accessibilityRole="button" onPress={closePreview} style={s.button}>
              <Text style={s.action}>Close</Text>
            </Pressable>
            <Pressable accessibilityRole="button" onPress={() => jump(preview)} style={s.button}>
              <Text style={s.action}>Play chapter</Text>
            </Pressable>
          </View>
        ) : undefined
      }
    >
      {preview ? (
        <View style={s.preview}>
          <ChapterThumbnail key={`${videoId}:${preview.start}`} chapter={preview} videoId={videoId} active={false} width={Math.min(280, screen.width - 96)} />
          <Text style={s.previewTitle}>{preview.title}</Text>
          <Text style={s.previewTime}>{formatTime(preview.start)}</Text>
        </View>
      ) : (
        <FlatList
          ref={list}
          data={rows}
          extraData={selected}
          style={[s.list, { maxHeight: screen.height * 0.65 }]}
          contentContainerStyle={s.listContent}
          initialNumToRender={12}
          keyExtractor={(row, index) => `${videoId}:${index}:${row.start}`}
          onScrollToIndexFailed={({ index, averageItemLength }) => {
            list.current?.scrollToOffset({ offset: Math.max(0, (index - 1) * averageItemLength), animated: false });
            if (attempts.current++ < 3) {
              retry.current = setTimeout(() => {
                list.current?.scrollToIndex({ index, animated: false, viewPosition: 0.3 });
              }, 100);
            }
          }}
          ListEmptyComponent={<Text style={s.empty}>No chapters or segments available</Text>}
          renderItem={({ item, index }) => {
            const active = !!item.chapter && selected === index;
            return (
              <Pressable
                accessibilityRole="button"
                accessibilityLabel={`${item.title}, ${formatTime(item.start)}`}
                accessibilityState={{ selected: active }}
                accessibilityHint={item.chapter ? 'Hold to preview chapter' : undefined}
                accessibilityActions={item.chapter ? [{ name: 'activate' }, { name: 'longpress', label: 'Preview chapter' }] : undefined}
                onAccessibilityAction={({ nativeEvent }) => {
                  if (nativeEvent.actionName === 'longpress' && item.chapter) setPreview(item.chapter);
                  else if (nativeEvent.actionName === 'activate') jump(item);
                }}
                onPress={() => jump(item)}
                onLongPress={item.chapter ? () => { vibrate(); setPreview(item.chapter!); } : undefined}
                style={({ pressed }) => [s.row, active && s.activeRow, pressed && s.pressed]}
              >
                {item.chapter ? (
                  <ChapterThumbnail chapter={item.chapter} videoId={videoId} active={active} width={thumbnailWidth} />
                ) : (
                  <View style={[s.dot, { backgroundColor: item.color }]} />
                )}
                <View style={s.grow}>
                  <Text numberOfLines={3} style={s.title}>{item.title}</Text>
                  <View style={s.details}>
                    <View style={s.timestamp}>
                      <Text style={s.time}>{formatTime(item.start)}</Text>
                    </View>
                    {active && (
                      <Pressable
                        accessibilityRole="button"
                        accessibilityLabel={`Share chapter: ${item.title}`}
                        onPress={() => { vibrate(); onShare(item.start); }}
                        style={s.share}
                      >
                        <Share2 size={20} color={colors.ink} />
                      </Pressable>
                    )}
                  </View>
                </View>
              </Pressable>
            );
          }}
        />
      )}
    </PlayerSheet>
  );
}
const makeStyles = (colors: ThemeColors) =>
  StyleSheet.create({
    list: { borderTopWidth: StyleSheet.hairlineWidth, borderTopColor: colors.line },
    listContent: { paddingTop: 8, paddingBottom: 12 },
    row: { flexDirection: 'row', alignItems: 'flex-start', paddingHorizontal: 16, paddingVertical: 10, gap: 16, minHeight: 64 },
    activeRow: { backgroundColor: colors.raised },
    grow: { flex: 1, minWidth: 0 },
    title: { color: colors.ink, fontSize: 16, lineHeight: 22, fontWeight: '600' },
    details: { flexDirection: 'row', alignItems: 'center', gap: 8, marginTop: 6, minHeight: 28 },
    timestamp: { alignSelf: 'flex-start', paddingHorizontal: 4, paddingVertical: 2, borderRadius: 3, backgroundColor: `${colors.action}26` },
    time: { color: colors.action, fontSize: 14, fontWeight: '600', fontVariant: ['tabular-nums'] },
    share: { width: 44, height: 44, alignItems: 'center', justifyContent: 'center', marginVertical: -8 },
    thumbnail: { aspectRatio: 16 / 9, borderRadius: 8, overflow: 'hidden', backgroundColor: colors.raised },
    thumbnailShade: { position: 'absolute', top: 0, bottom: 0, left: 0, right: 0, alignItems: 'center', justifyContent: 'center', backgroundColor: '#0003' },
    thumbnailPlay: { width: 36, height: 36, borderRadius: 18, alignItems: 'center', justifyContent: 'center', backgroundColor: '#0009' },
    dot: { width: 12, height: 12, borderRadius: 6, marginTop: 5 },
    pressed: { backgroundColor: colors.line },
    empty: { color: colors.muted, fontSize: 14, padding: 24 },
    preview: { paddingHorizontal: 16, paddingTop: 8 },
    previewTitle: { color: colors.ink, fontSize: 18, fontWeight: '600', marginTop: 12 },
    previewTime: { color: colors.action, fontSize: 14, marginTop: 8, fontVariant: ['tabular-nums'] },
    actions: { flexDirection: 'row', justifyContent: 'flex-end', paddingHorizontal: 16, paddingBottom: 12 },
    button: { minWidth: 64, minHeight: 48, alignItems: 'center', justifyContent: 'center', paddingHorizontal: 12 },
    action: { color: colors.action, fontSize: 14, fontWeight: '500' },
  });
