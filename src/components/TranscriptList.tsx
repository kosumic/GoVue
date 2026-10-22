import { type ReactNode, useCallback, useEffect, useRef, useState } from 'react';
import { FlatList, View, type StyleProp, type ViewStyle } from 'react-native';
import type { TranscriptRow } from '../core/transcripts';

export type TranscriptFocus = { node: { measureInWindow: (callback: (x: number, y: number, width: number, height: number) => void) => void }; y: number; height: number };

// Each language keeps its own render window and measurements. A long caption
// can scroll within its pane without moving the other language out of view.
export function TranscriptList({ rows, activeIndex, following, followRevision, enabled, extraData, style, contentContainerStyle, empty, onStopFollowing, renderRow }: {
  rows: TranscriptRow[];
  activeIndex: number;
  following: boolean;
  followRevision: number;
  enabled: boolean;
  extraData: unknown;
  style?: StyleProp<ViewStyle>;
  contentContainerStyle: StyleProp<ViewStyle>;
  empty: ReactNode;
  onStopFollowing: () => void;
  renderRow: (row: TranscriptRow, active: boolean, follow: (focus: TranscriptFocus) => void) => ReactNode;
}) {
  const [window, setWindow] = useState<{ rows: TranscriptRow[]; index: number; version: number } | null>(null);
  const list = useRef<FlatList<TranscriptRow>>(null);
  const target = useRef<{ index: number; recovering: boolean; aligned: boolean } | null>(null);
  const retry = useRef<ReturnType<typeof setTimeout> | null>(null);
  const scrollOffset = useRef(0);
  const rowNodes = useRef(new Map<number, View>());
  const focus = useRef<{ rows: TranscriptRow[]; index: number; value: TranscriptFocus } | null>(null);
  const measurement = useRef(0);
  const mountList = useCallback((node: FlatList<TranscriptRow> | null) => {
    list.current = node;
    scrollOffset.current = 0;
    focus.current = null;
    if (target.current) target.current.aligned = false;
    ++measurement.current;
  }, []);
  const reveal = useCallback((index: number, node: TranscriptFocus['node'], line?: TranscriptFocus, onAligned?: () => void) => {
    const current = target.current;
    if (!current || current.index !== index || (line && !current.aligned)) return;
    const revision = ++measurement.current;
    const viewport = list.current?.getNativeScrollRef();
    if (!viewport || !('measureInWindow' in viewport)) return;
    viewport.measureInWindow((_x, top, _width, height) => {
      node.measureInWindow((_x, y, _width, nodeHeight) => {
        if (target.current !== current || revision !== measurement.current || height <= 0) return;
        // Android can keep a row mounted while clipping its native view.
        if (nodeHeight <= 0) {
          list.current?.scrollToIndex({ index, viewPosition: 0, animated: false });
          return;
        }
        const padding = Math.min(12, height * 0.05);
        const visibleBottom = top + height - padding;
        const lineTop = y + (line?.y ?? 0);
        // New captions start at the top. Within a long caption, wait until
        // the spoken line leaves view, then hide all preceding visual lines.
        if (line && lineTop >= top && lineTop + line.height <= visibleBottom) return;
        if (!line) current.aligned = true;
        const offset = Math.max(0, scrollOffset.current + lineTop - top);
        if (Math.abs(offset - scrollOffset.current) >= 1) {
          scrollOffset.current = offset;
          list.current?.scrollToOffset({ offset, animated: false });
        }
        onAligned?.();
      });
    });
  }, []);
  const followLine = useCallback((index: number, value: TranscriptFocus) => {
    focus.current = { rows, index, value };
    reveal(index, value.node, value);
  }, [reveal, rows]);
  useEffect(() => {
    setWindow(current => current?.rows === rows ? current : {
      rows, index: following && activeIndex >= 0 ? activeIndex : 0, version: (current?.version ?? 0) + 1,
    });
  }, [rows, following, activeIndex]);
  const scrollToCurrent = useCallback(() => {
    if (retry.current !== null) clearTimeout(retry.current);
    retry.current = null;
    const current = target.current;
    if (current && current.index < (list.current?.props.data?.length ?? 0)) {
      const followVisual = () => {
        const visual = focus.current;
        if (visual?.rows === rows && visual.index === current.index) followLine(current.index, visual.value);
      };
      if (current.aligned) {
        followVisual();
        return;
      }
      current.recovering = false;
      const node = rowNodes.current.get(current.index);
      if (node) reveal(current.index, node, undefined, followVisual);
      else list.current?.scrollToIndex({ index: current.index, viewPosition: 0, animated: false });
    }
  }, [followLine, reveal, rows]);
  useEffect(() => {
    target.current = following && activeIndex >= 0 && enabled ? { index: activeIndex, recovering: false, aligned: false } : null;
    if (target.current && list.current && !rowNodes.current.has(activeIndex)) {
      // Cached cell offsets can be stale after a tall caption changes the
      // render window. Mount an offscreen seek destination with fresh metrics.
      target.current.recovering = true;
      setWindow(previous => ({ rows, index: activeIndex, version: (previous?.version ?? 0) + 1 }));
    } else scrollToCurrent();
    return () => {
      target.current = null;
      if (retry.current !== null) clearTimeout(retry.current);
    };
  }, [activeIndex, rows, following, followRevision, enabled, scrollToCurrent]);
  if (window?.rows !== rows) return null;
  return <FlatList key={window.version} ref={mountList} data={rows} initialScrollIndex={window.index}
    style={style} extraData={extraData} keyExtractor={item => item.lines.map(line => `${line.track}:${line.index}`).join('/')}
    contentContainerStyle={contentContainerStyle}
    onScrollBeginDrag={() => {
      target.current = null;
      if (retry.current !== null) clearTimeout(retry.current);
      onStopFollowing();
    }}
    onLayout={scrollToCurrent}
    scrollEventThrottle={16}
    onScroll={event => {
      scrollOffset.current = event.nativeEvent.contentOffset.y;
      if (target.current) scrollToCurrent();
    }}
    onContentSizeChange={() => { if (target.current?.recovering) scrollToCurrent(); }}
    onScrollToIndexFailed={({ index }) => {
      const current = target.current;
      if (!current || current.index !== index) return;
      current.recovering = true;
      current.aligned = false;
      // Mount an unmeasured destination directly rather than guessing its offset.
      setWindow(previous => previous?.rows === rows && previous.index === index ? previous : {
        rows, index, version: (previous?.version ?? 0) + 1,
      });
    }}
    keyboardShouldPersistTaps="handled" ListEmptyComponent={<>{empty}</>}
    renderItem={({ item, index }) => <View collapsable={false} ref={node => {
      if (node) rowNodes.current.set(index, node);
      else rowNodes.current.delete(index);
    }} onLayout={() => {
      if (target.current?.index !== index) return;
      if (retry.current !== null) clearTimeout(retry.current);
      // Align after FlatList has recorded this cell's native measurements.
      retry.current = setTimeout(scrollToCurrent, 0);
    }}>{renderRow(item, index === activeIndex, value => followLine(index, value))}</View>} />;
}
