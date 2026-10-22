import { type ReactNode, useCallback, useEffect, useRef, useState } from 'react';
import { FlatList, View, type StyleProp, type ViewStyle } from 'react-native';
import type { TranscriptRow } from '../core/transcripts';

export type TranscriptFocus = { node: { measureInWindow: (callback: (x: number, y: number, width: number, height: number) => void) => void }; y: number; height: number };

// Each language keeps its own render window and measurements. A long caption
// can scroll within its pane without moving the other language out of view.
export function TranscriptList({ rows, activeIndex, following, followRevision, enabled, extraData, style, contentContainerStyle, empty, onStopFollowing, renderRow, comparison = false }: {
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
  comparison?: boolean;
}) {
  const [window, setWindow] = useState<{ rows: TranscriptRow[]; index: number; version: number } | null>(null);
  const list = useRef<FlatList<TranscriptRow>>(null);
  const target = useRef<{ index: number; recovering: boolean } | null>(null);
  const retry = useRef<ReturnType<typeof setTimeout> | null>(null);
  const scrollOffset = useRef(0);
  const rowNodes = useRef(new Map<number, View>());
  const focus = useRef<{ rows: TranscriptRow[]; index: number; value: TranscriptFocus } | null>(null);
  const measurement = useRef(0);
  const mountList = useCallback((node: FlatList<TranscriptRow> | null) => {
    list.current = node;
    scrollOffset.current = 0;
    ++measurement.current;
  }, []);
  const reveal = useCallback((index: number, node: TranscriptFocus['node'], line?: TranscriptFocus) => {
    const revision = ++measurement.current;
    const current = target.current;
    if (!current || current.index !== index) return;
    const viewport = list.current?.getNativeScrollRef();
    if (!viewport || !('measureInWindow' in viewport)) return;
    viewport.measureInWindow((_x, top, _width, height) => {
      node.measureInWindow((_x, y, _width, nodeHeight) => {
        if (target.current !== current || revision !== measurement.current || height <= 0) return;
        // Android can keep a row mounted while clipping its native view.
        if (nodeHeight <= 0) {
          list.current?.scrollToIndex({ index, viewPosition: comparison ? 0 : 0.3, animated: false });
          return;
        }
        const padding = Math.min(12, height * 0.05);
        const visibleTop = top + padding;
        const visibleBottom = top + height - padding;
        // A visible tall caption must wait for its spoken line's text layout.
        if (!line && nodeHeight > visibleBottom - visibleTop && y < visibleBottom && y + nodeHeight > visibleTop) return;
        // For an offscreen tall caption, reveal its beginning until text is ready.
        const lineHeight = line?.height ?? Math.min(nodeHeight, visibleBottom - visibleTop);
        if (lineHeight <= 0) return;
        const lineTop = y + (line?.y ?? 0);
        const lineBottom = lineTop + lineHeight;
        if (lineTop >= visibleTop && lineBottom <= visibleBottom) return;
        const delta = lineTop < visibleTop ? lineTop - visibleTop : lineBottom - visibleBottom;
        const offset = Math.max(0, scrollOffset.current + delta);
        if (Math.abs(offset - scrollOffset.current) < 1) return;
        scrollOffset.current = offset;
        list.current?.scrollToOffset({ offset, animated: false });
      });
    });
  }, [comparison]);
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
      const visual = focus.current;
      if (visual?.rows === rows && visual.index === current.index) {
        current.recovering = false;
        followLine(current.index, visual.value);
        return;
      }
      current.recovering = false;
      const node = rowNodes.current.get(current.index);
      if (node) {
        reveal(current.index, node);
        return;
      }
      list.current?.scrollToIndex({ index: current.index, viewPosition: comparison ? 0 : 0.3, animated: false });
    }
  }, [comparison, followLine, reveal, rows]);
  useEffect(() => {
    target.current = following && activeIndex >= 0 && enabled ? { index: activeIndex, recovering: false } : null;
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
    onScroll={event => { scrollOffset.current = event.nativeEvent.contentOffset.y; }}
    onContentSizeChange={() => { if (target.current?.recovering) scrollToCurrent(); }}
    onScrollToIndexFailed={({ index }) => {
      const current = target.current;
      if (!current || current.index !== index) return;
      current.recovering = true;
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
