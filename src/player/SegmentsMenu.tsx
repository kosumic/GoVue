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
import { coreRequest } from '../../modules/govue-core';
import type { MediaChapter, SponsorSegment } from '../core/media';
import { useThemeStyles, type ThemeColors } from '../theme';
import { MarqueeTitle } from './MarqueeTitle';
import { PlayerSheet, type MenuAnchor } from './PlayerSheet';
import { useHaptic } from './Haptics';

type Row = {
  title: string;
  start: number;
  chapter?: MediaChapter;
  color?: string;
};
const emptyChapters: MediaChapter[] = [];
const emptySegments: SponsorSegment[] = [];

/** LiTube's chapter-first popup and long-press preview, sharing one modal. */
export function SegmentsMenu({
  visible,
  videoId,
  chapters = emptyChapters,
  segments = emptySegments,
  position,
  anchor,
  formatTime,
  categoryLabel,
  categoryColor,
  onJump,
  onClose,
}: {
  visible: boolean;
  videoId: string;
  chapters?: MediaChapter[];
  segments?: SponsorSegment[];
  position: number;
  anchor?: MenuAnchor;
  formatTime: (seconds: number) => string;
  categoryLabel: (category: string) => string;
  categoryColor: (category: string) => string;
  onJump: (position: number, title: string) => void;
  onClose: () => void;
}) {
  const screen = useWindowDimensions();
  const s = useThemeStyles(makeStyles);
  const vibrate = useHaptic();
  const [preview, setPreview] = useState<MediaChapter | null>(null);
  const [selected, setSelected] = useState<number | null>(null);
  const [imageFailed, setImageFailed] = useState(false);
  const rows = useMemo<Row[]>(
    () =>
      chapters.length
        ? chapters.map((chapter) => ({
            title: chapter.title || 'Segments',
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
  const labels = rows.length
    ? rows.map((row) => `${row.title}  ${formatTime(row.start)}`)
    : ['No segments'];
  const measurementKey = JSON.stringify([labels, screen.fontScale]);
  const measureOwner = useRef(measurementKey);
  measureOwner.current = measurementKey;
  const [measurement, setMeasurement] = useState({ key: '', width: 0 });
  const samples =
    labels.length <= 48
      ? labels
      : [...labels].sort((a, b) => b.length - a.length).slice(0, 24);
  const popupWidth = Math.min(
    screen.width * 0.8,
    measurement.key === measurementKey
      ? measurement.width + 40
      : screen.width * 0.8,
  );
  useEffect(() => {
    setPreview(null);
    setSelected(null);
  }, [visible, videoId]);
  useEffect(() => setImageFailed(false), [preview]);
  useEffect(() => {
    if (!visible || !chapters.length) return;
    let active = true;
    void coreRequest<number | null>({
      op: 'player_chapter',
      starts: chapters.map((chapter) => chapter.start),
      position,
    })
      .then((index) => {
        if (active) setSelected(index);
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [visible, videoId, chapters, position]);
  const jump = (row: { start: number; title: string }) => {
    vibrate();
    onJump(row.start, row.title || 'Segments');
  };
  const closePreview = () => setPreview(null);
  return (
    <PlayerSheet
      visible={visible}
      title="Segments"
      anchor={preview ? undefined : anchor}
      popupWidth={popupWidth}
      presentation={preview ? 'dialog' : 'sheet'}
      showHeading={false}
      scrollable={!!preview}
      onClose={preview ? closePreview : onClose}
      footer={
        preview ? (
          <View>
            <Text style={[s.previewTime, s.fixedTime]}>
              {formatTime(preview.start)}
            </Text>
            <View style={s.actions}>
              <Pressable
                accessibilityRole="button"
                onPress={closePreview}
                style={s.button}
              >
                <Text style={s.action}>Close</Text>
              </Pressable>
              <Pressable
                accessibilityRole="button"
                onPress={() => jump(preview)}
                style={s.button}
              >
                <Text style={s.action}>Jump</Text>
              </Pressable>
            </View>
          </View>
        ) : undefined
      }
    >
      {preview ? (
        <View style={s.preview}>
          <Image
            accessibilityLabel="Thumbnail"
            source={{
              uri:
                (!imageFailed && preview.previewUrl) ||
                `https://i.ytimg.com/vi/${videoId}/hqdefault.jpg`,
            }}
            style={s.image}
            resizeMode="cover"
            onError={() => setImageFailed(true)}
          />
          <Text style={s.previewTitle}>{preview.title || 'Segments'}</Text>
        </View>
      ) : (
        <View>
          <View
            pointerEvents="none"
            accessibilityElementsHidden
            importantForAccessibility="no-hide-descendants"
            style={s.measure}
          >
            {samples.map((label, i) => (
              <Text
                key={`${i}:${label}`}
                style={s.title}
                onTextLayout={({ nativeEvent }) => {
                  const width = nativeEvent.lines[0]?.width;
                  if (
                    measureOwner.current !== measurementKey ||
                    width === undefined ||
                    !Number.isFinite(width)
                  )
                    return;
                  setMeasurement((old) =>
                    old.key === measurementKey && old.width >= width
                      ? old
                      : {
                          key: measurementKey,
                          width: Math.max(
                            width,
                            old.key === measurementKey ? old.width : 0,
                          ),
                        },
                  );
                }}
              >
                {label}
              </Text>
            ))}
          </View>
          <FlatList
            data={rows}
            extraData={selected}
            style={{ maxHeight: screen.height * 0.7 }}
            initialNumToRender={16}
            keyExtractor={(row, index) => `${index}:${row.start}`}
            ItemSeparatorComponent={() => <View style={s.divider} />}
            ListEmptyComponent={<Text style={s.empty}>No segments</Text>}
            renderItem={({ item, index }) => (
              <Pressable
                accessibilityRole="button"
                accessibilityLabel={`${item.title}, ${formatTime(item.start)}`}
                accessibilityState={{
                  selected: !!item.chapter && selected === index,
                }}
                accessibilityHint={
                  item.chapter ? 'Hold to preview chapter' : undefined
                }
                accessibilityActions={
                  item.chapter
                    ? [
                        { name: 'activate' },
                        { name: 'longpress', label: 'Preview chapter' },
                      ]
                    : undefined
                }
                onAccessibilityAction={({ nativeEvent }) => {
                  if (nativeEvent.actionName === 'longpress' && item.chapter)
                    setPreview(item.chapter);
                  else if (nativeEvent.actionName === 'activate') jump(item);
                }}
                onPress={() => jump(item)}
                onLongPress={
                  item.chapter
                    ? () => {
                        vibrate();
                        setPreview(item.chapter!);
                      }
                    : undefined
                }
                style={({ pressed }) => [s.row, pressed && s.pressed]}
              >
                {item.color && (
                  <View style={[s.dot, { backgroundColor: item.color }]} />
                )}
                <View style={s.grow}>
                  <MarqueeTitle
                    style={[
                      s.title,
                      !!item.chapter && selected === index && s.selected,
                    ]}
                  >
                    {item.title}
                  </MarqueeTitle>
                </View>
                <Text style={s.time}>{formatTime(item.start)}</Text>
              </Pressable>
            )}
          />
        </View>
      )}
    </PlayerSheet>
  );
}
const makeStyles = (colors: ThemeColors) =>
  StyleSheet.create({
    row: {
      flexDirection: 'row',
      alignItems: 'center',
      paddingHorizontal: 16,
      paddingVertical: 10,
      gap: 12,
    },
    grow: { flex: 1 },
    title: { color: colors.ink, fontSize: 14, lineHeight: 20 },
    selected: { color: colors.action },
    time: { color: colors.muted, fontSize: 12 },
    dot: { width: 12, height: 12, borderRadius: 6 },
    divider: { height: StyleSheet.hairlineWidth, backgroundColor: colors.line },
    pressed: { backgroundColor: colors.accentSoft },
    empty: {
      color: colors.muted,
      fontSize: 14,
      paddingHorizontal: 16,
      paddingVertical: 10,
    },
    measure: {
      position: 'absolute',
      left: 0,
      top: 0,
      width: 10000,
      opacity: 0,
    },
    preview: { paddingHorizontal: 16, paddingTop: 24 },
    image: {
      width: '100%',
      height: 160,
      borderRadius: 8,
      backgroundColor: colors.line,
    },
    previewTitle: {
      color: colors.ink,
      fontSize: 18,
      fontWeight: '500',
      marginTop: 12,
    },
    previewTime: { color: colors.muted, fontSize: 14, marginTop: 4 },
    fixedTime: { paddingHorizontal: 24 },
    actions: {
      flexDirection: 'row',
      justifyContent: 'flex-end',
      paddingHorizontal: 16,
      paddingBottom: 12,
    },
    button: {
      minWidth: 64,
      minHeight: 48,
      alignItems: 'center',
      justifyContent: 'center',
      paddingHorizontal: 12,
    },
    action: { color: colors.action, fontSize: 14, fontWeight: '500' },
  });
