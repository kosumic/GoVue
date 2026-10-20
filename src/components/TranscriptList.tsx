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
  const focus = useRef<{ rows: TranscriptRow[]; index: number; value: TranscriptFocus } | null>(null);
  const measurement = useRef(0);
  const followLine = useCallback((index: number, value: TranscriptFocus) => {
    focus.current = { rows, index, value };
    const revision = ++measurement.current;
    const current = target.current;
    if (!current || current.index !== index) return;
    const viewport = list.current?.getNativeScrollRef();
    if (!viewport || !('measureInWindow' in viewport)) return;
    viewport.measureInWindow((_x, top, _width, height) => {
      value.node.measureInWindow((_x, y) => {
        if (target.current !== current || revision !== measurement.current || height <= 0) return;
        const anchor = Math.min(48, Math.max(0, (height - value.height) * 0.2));
        const offset = Math.max(0, scrollOffset.current + y + value.y - top - anchor);
        if (Math.abs(offset - scrollOffset.current) < 1) return;
        scrollOffset.current = offset;
        list.current?.scrollToOffset({ offset, animated: false });
      });
    });
  }, [rows]);
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
      list.current?.scrollToIndex({ index: current.index, viewPosition: comparison ? 0 : 0.3, animated: false });
    }
  }, [comparison, followLine, rows]);
  useEffect(() => {
    target.current = following && activeIndex >= 0 && enabled ? { index: activeIndex, recovering: false } : null;
    scrollToCurrent();
    return () => {
      target.current = null;
      if (retry.current !== null) clearTimeout(retry.current);
    };
  }, [activeIndex, rows, following, followRevision, enabled, scrollToCurrent]);
  if (window?.rows !== rows) return null;
  return <FlatList key={window.version} ref={list} data={rows} initialScrollIndex={window.index}
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
    renderItem={({ item, index }) => <View onLayout={() => {
      if (target.current?.index !== index) return;
      if (retry.current !== null) clearTimeout(retry.current);
      // Align after FlatList has recorded this cell's native measurements.
      retry.current = setTimeout(scrollToCurrent, 0);
    }}>{renderRow(item, index === activeIndex, value => followLine(index, value))}</View>} />;
}
