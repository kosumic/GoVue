import { useEffect, useRef, useState } from 'react';
import {
  Alert,
  FlatList,
  Image,
  PanResponder,
  Pressable,
  StyleSheet,
  Text,
  useWindowDimensions,
  View,
} from 'react-native';
import {
  Download,
  LoopPause,
  LoopRepeat,
  LoopShuffle,
  LoopNext,
  Trash2,
  X,
} from './icons';
import { IconButton } from '../components/IconButton';
import { YouTubeSwitch as Switch } from '../components/YouTubeSwitch';
import { useThemeColors, useThemeStyles, type ThemeColors } from '../theme';
import type { QueueItem, QueueState } from './usePlayerSession';
import { useHaptic } from './Haptics';

type Drag = { id: string; from: number; pointer: number; captured: boolean };
export function QueueSheet({
  queue,
  currentId,
  onPlay,
  onClose,
  onEnabled,
  onLoop,
  onClear,
  onRemove,
  onMove,
  onDownloadAll,
  onDownloadItem,
  onError,
}: {
  queue: QueueState;
  currentId?: string;
  onPlay: (item: QueueItem) => void;
  onClose: () => void;
  onEnabled: (enabled: boolean) => Promise<unknown>;
  onLoop: () => Promise<unknown>;
  onClear: () => Promise<unknown>;
  onRemove: (id: string) => Promise<unknown>;
  onMove: (id: string, offset: number) => Promise<QueueState>;
  onDownloadAll: () => void;
  onDownloadItem: (item: QueueItem) => void;
  onError: (error: unknown) => void;
}) {
  const screen = useWindowDimensions();
  const bodyHeight = Math.max(
    200,
    Math.min(520, Math.floor(screen.height * 0.75)),
  );
  const colors = useThemeColors();
  const s = useThemeStyles(makeStyles);
  const vibrate = useHaptic();
  const [items, setItems] = useState(queue.queue);
  const [dragging, setDragging] = useState<string | null>(null);
  const list = useRef<FlatList<QueueItem> | null>(null);
  const viewport = useRef<View | null>(null);
  const bounds = useRef({ top: 0, height: 1, content: 1, scroll: 0 });
  const rowHeights = useRef(new Map<string, number>());
  const drag = useRef<Drag | null>(null);
  const committing = useRef(false);
  const lastDrag = useRef(0);
  const current = useRef({ items, queue, onMove, onError });
  current.current = { items, queue, onMove, onError };
  const work = (action: () => Promise<unknown>) => {
    void action().catch(onError);
  };
  const update = (next: QueueItem[]) => {
    current.current.items = next;
    setItems(next);
  };
  useEffect(() => {
    if (!drag.current && !committing.current) update(queue.queue);
  }, [queue.queue]);
  const moveAtPointer = () => {
    const active = drag.current;
    if (!active) return;
    const values = current.current.items;
    const from = values.findIndex((item) => item.videoId === active.id);
    let y = active.pointer - bounds.current.top + bounds.current.scroll;
    let to = values.length - 1;
    for (let i = 0; i < values.length; i++) {
      const height = rowHeights.current.get(values[i].videoId) || 80;
      if (y < height) {
        to = i;
        break;
      }
      y -= height;
    }
    if (from < 0 || from === to) return;
    const next = [...values];
    next.splice(to, 0, next.splice(from, 1)[0]);
    update(next);
  };
  const finish = () => {
    const active = drag.current;
    if (!active) return;
    drag.current = null;
    lastDrag.current = Date.now();
    setDragging(null);
    const to = current.current.items.findIndex(
      (item) => item.videoId === active.id,
    );
    if (to < 0 || to === active.from) {
      update(current.current.queue.queue);
      return;
    }
    committing.current = true;
    void current.current
      .onMove(active.id, to - active.from)
      .then((result) => update(result.queue))
      .catch((error) => {
        update(current.current.queue.queue);
        current.current.onError(error);
      })
      .finally(() => {
        committing.current = false;
      });
  };
  const handlers = useRef({ moveAtPointer, finish });
  handlers.current = { moveAtPointer, finish };
  const responder = useRef(
    PanResponder.create({
      onMoveShouldSetPanResponderCapture: () => !!drag.current,
      onPanResponderGrant: () => {
        if (drag.current) drag.current.captured = true;
      },
      onPanResponderMove: (event) => {
        if (drag.current) {
          drag.current.pointer = event.nativeEvent.pageY;
          handlers.current.moveAtPointer();
        }
      },
      onPanResponderRelease: () => handlers.current.finish(),
      onPanResponderTerminate: () => handlers.current.finish(),
      onPanResponderTerminationRequest: () => false,
    }),
  ).current;
  useEffect(() => {
    if (!dragging) return;
    let frame = 0,
      previous = 0;
    const tick = (time: number) => {
      if (!drag.current) return;
      const b = bounds.current;
      const y = drag.current.pointer - b.top;
      const dt = previous ? Math.min(0.1, (time - previous) / 1000) : 0;
      previous = time;
      const velocity =
        y < 40
          ? -Math.min(1, (40 - y) / 40)
          : y > b.height - 40
            ? Math.min(1, (y - b.height + 40) / 40)
            : 0;
      const next = Math.max(
        0,
        Math.min(
          Math.max(0, b.content - b.height),
          b.scroll + velocity * 900 * dt,
        ),
      );
      if (next !== b.scroll) {
        b.scroll = next;
        list.current?.scrollToOffset({ offset: next, animated: false });
        handlers.current.moveAtPointer();
      }
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [dragging]);
  const initialScroll = useRef(false);
  const loopIcon =
    queue.loopMode === 'repeat'
      ? LoopRepeat
      : queue.loopMode === 'shuffle'
        ? LoopShuffle
        : queue.loopMode === 'pause'
          ? LoopPause
          : LoopNext;
  const loopLabel = {
    next: 'Play next',
    repeat: 'Loop one',
    pause: 'Pause at end',
    shuffle: 'Shuffle',
  }[queue.loopMode];
  return (
    <View style={{ flexShrink: 1, paddingBottom: 24 }}>
      <View style={s.toolbar}>
        <IconButton
          compact
          icon={loopIcon}
          label={loopLabel}
          onPress={() => work(onLoop)}
        />
        <IconButton
          compact
          icon={Trash2}
          label="Clear queue"
          disabled={!items.length}
          onPress={() =>
            Alert.alert('Clear queue?', 'Remove all videos from the queue?', [
              { text: 'Cancel', style: 'cancel' },
              {
                text: 'OK',
                style: 'destructive',
                onPress: () => work(onClear),
              },
            ])
          }
        />
        <IconButton
          compact
          icon={Download}
          label="Download queue"
          disabled={!items.length}
          onPress={onDownloadAll}
        />
        <View style={s.grow} />
        <Text style={s.autoplay}>Autoplay</Text>
        <Switch
          accessibilityLabel="Queue autoplay"
          value={queue.enabled}
          onValueChange={(enabled) => work(() => onEnabled(enabled))}
        />
      </View>
      {!items.length ? (
        <View style={s.empty}>
          <Text style={s.author}>Queue is empty</Text>
        </View>
      ) : (
        <View
          ref={viewport}
          onLayout={(event) => {
            bounds.current.height = event.nativeEvent.layout.height;
            viewport.current?.measureInWindow((_x, y) => {
              bounds.current.top = y;
            });
          }}
          style={{
            height: Math.min(
              items.length * 96,
              Math.max(48, bodyHeight - 48 - 56 - 24),
            ),
          }}
          {...responder.panHandlers}
        >
          <FlatList
            ref={list}
            data={items}
            keyExtractor={(item) => item.videoId}
            scrollEnabled={!dragging}
            initialNumToRender={12}
            maxToRenderPerBatch={12}
            windowSize={5}
            scrollEventThrottle={16}
            onScroll={(event) => {
              bounds.current.scroll = event.nativeEvent.contentOffset.y;
            }}
            onContentSizeChange={(_w, height) => {
              bounds.current.content = height;
              if (!initialScroll.current) {
                initialScroll.current = true;
                const index = current.current.items.findIndex(
                  (item) => item.videoId === currentId,
                );
                if (index > 0)
                  list.current?.scrollToOffset({
                    offset: Math.max(0, index * 80 - 200),
                    animated: false,
                  });
              }
            }}
            renderItem={({ item, index }) => (
              <View
                onLayout={(event) =>
                  rowHeights.current.set(
                    item.videoId,
                    event.nativeEvent.layout.height,
                  )
                }
                style={[
                  s.row,
                  item.videoId === currentId && s.playing,
                  dragging === item.videoId && s.dragging,
                ]}
              >
                <Pressable
                  style={s.item}
                  accessibilityRole="button"
                  accessibilityLabel={`Play ${item.title}`}
                  accessibilityHint="Hold and drag to reorder"
                  accessibilityActions={[
                    { name: 'increment', label: 'Move down' },
                    { name: 'decrement', label: 'Move up' },
                  ]}
                  onAccessibilityAction={(event) =>
                    work(() =>
                      onMove(
                        item.videoId,
                        event.nativeEvent.actionName === 'increment' ? 1 : -1,
                      ),
                    )
                  }
                  onPress={() => {
                    if (drag.current || Date.now() - lastDrag.current < 600)
                      return;
                    if (item.videoId === currentId) onClose();
                    else onPlay(item);
                  }}
                  delayLongPress={400}
                  onLongPress={(event) => {
                    if (committing.current) return;
                    vibrate();
                    drag.current = {
                      id: item.videoId,
                      from: index,
                      pointer: event.nativeEvent.pageY,
                      captured: false,
                    };
                    setDragging(item.videoId);
                  }}
                  onPressOut={() => {
                    const active = drag.current;
                    if (active)
                      setTimeout(() => {
                        if (drag.current === active && !active.captured)
                          handlers.current.finish();
                      }, 0);
                  }}
                >
                  <Image
                    source={{
                      uri: `https://i.ytimg.com/vi/${item.videoId}/hqdefault.jpg`,
                    }}
                    style={{
                      width: Math.max(72, Math.min(112, screen.width * 0.24)),
                      aspectRatio: 16 / 9,
                      borderRadius: 6,
                    }}
                  />
                  <View style={s.grow}>
                    {item.videoId === currentId && (
                      <Text style={s.now}>Now playing</Text>
                    )}
                    <Text numberOfLines={2} style={s.title}>
                      {item.title || item.videoId}
                    </Text>
                    {!!item.author && (
                      <Text numberOfLines={1} style={s.author}>
                        {item.author}
                      </Text>
                    )}
                  </View>
                </Pressable>
                <IconButton
                  compact
                  icon={Download}
                  label={`Download ${item.title}`}
                  color={`${colors.muted}b3`}
                  onPress={() => onDownloadItem(item)}
                />
                <IconButton
                  compact
                  icon={X}
                  label={`Remove ${item.title}`}
                  color={`${colors.muted}b3`}
                  onPress={() => work(() => onRemove(item.videoId))}
                />
              </View>
            )}
          />
        </View>
      )}
    </View>
  );
}
const makeStyles = (colors: ThemeColors) =>
  StyleSheet.create({
    toolbar: {
      flexDirection: 'row',
      alignItems: 'center',
      paddingHorizontal: 12,
      paddingVertical: 4,
    },
    grow: { flex: 1 },
    autoplay: { color: colors.muted, fontSize: 14, marginRight: 8 },
    empty: { height: 180, alignItems: 'center', justifyContent: 'center' },
    row: {
      flexDirection: 'row',
      minHeight: 80,
      alignItems: 'center',
      paddingLeft: 12,
      paddingRight: 4,
      paddingVertical: 8,
    },
    playing: { backgroundColor: colors.raised },
    dragging: { backgroundColor: colors.accentSoft },
    item: { flex: 1, flexDirection: 'row', alignItems: 'center', gap: 10 },
    title: { color: colors.ink, fontSize: 14, lineHeight: 20 },
    author: { color: colors.muted, fontSize: 12, lineHeight: 18 },
    now: { color: '#ff0000', fontSize: 12, lineHeight: 16 },
  });
