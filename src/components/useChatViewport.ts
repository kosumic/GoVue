import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { createElement } from 'react';
import { View, type CellRendererProps, type FlatList, type NativeScrollEvent, type NativeSyntheticEvent } from 'react-native';
import type { ChatMessage } from '../ai/useChats';

export type ChatViewportState = {
  offset: number;
  followEnd: boolean;
  anchor?: { messageId: string; offset: number };
  expanded: ReadonlySet<string>;
};
export type ChatViewportStore = Map<string, ChatViewportState>;
const FOLLOW_THRESHOLD = 72;
const POSITION_EPSILON = 2;
const SCROLL_IDLE_MS = 200;
type Expansion = { expanded: ReadonlySet<string>; setExpanded: (id: string, value: boolean) => void };
export const ChatExpansionContext = createContext<Expansion | null>(null);

export function useChatExpansion(id: string): [boolean, (value: boolean) => void] {
  const context = useContext(ChatExpansionContext);
  const [local, setLocal] = useState(false);
  return [context ? context.expanded.has(id) : local, value => context ? context.setExpanded(id, value) : setLocal(value)];
}

/** One mounted conversation owns this hook; the small checkpoints survive its list. */
export function useChatViewport(chatId: string, messages: ChatMessage[], store: ChatViewportStore, searching = false) {
  const list = useRef<FlatList<ChatMessage>>(null);
  const saved = useRef(store.get(chatId));
  const followEnd = useRef(saved.current?.followEnd ?? true);
  const geometry = useRef({ offset: 0, height: 0, viewport: 0 });
  const layouts = useRef(new Map<string, { y: number; height: number }>());
  const firstVisible = useRef<string | null>(null);
  const currentMessages = useRef(messages);
  currentMessages.current = messages;
  const readyRef = useRef(false);
  const [ready, setReady] = useState(false);
  const [away, setAway] = useState(false);
  const [scrolling, setScrolling] = useState(false);
  const [expanded, setExpandedState] = useState<ReadonlySet<string>>(() => saved.current?.expanded ?? new Set());
  const expandedRef = useRef(expanded);
  expandedRef.current = expanded;
  const interaction = useRef<'idle' | 'drag' | 'awaiting' | 'momentum'>('idle');
  const lastUserOffset = useRef(0);
  const frame = useRef<number | null>(null);
  const idleTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const alignRef = useRef<() => void>(() => {});
  const pendingReveal = useRef<{ id: string; attempts: number } | null>(null);
  const revealTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const save = useCallback(() => {
    if (!readyRef.current) return;
    const { offset } = geometry.current;
    const first = currentMessages.current.find(message => message.id === firstVisible.current) ?? currentMessages.current.find(message => {
      const layout = layouts.current.get(message.id);
      return layout && layout.y + layout.height > offset;
    });
    const layout = first && layouts.current.get(first.id);
    store.set(chatId, { offset, followEnd: followEnd.current, expanded: expandedRef.current,
      anchor: first && layout ? { messageId: first.id, offset: offset - layout.y } : undefined });
  }, [chatId, store]);
  const scheduleAlign = useCallback(() => {
    if (frame.current !== null) return;
    frame.current = requestAnimationFrame(() => { frame.current = null; alignRef.current(); });
  }, []);
  const updateAway = () => {
    const g = geometry.current;
    setAway(!followEnd.current && g.height - g.viewport - g.offset >= FOLLOW_THRESHOLD);
  };
  alignRef.current = () => {
    const g = geometry.current;
    if (g.viewport <= 0 || (messages.length && g.height <= 0)) return;
    const maximum = Math.max(0, g.height - g.viewport);
    const reveal = pendingReveal.current;
    if (reveal) {
      const index = currentMessages.current.findIndex(message => message.id === reveal.id);
      const layout = layouts.current.get(reveal.id);
      if (index < 0) pendingReveal.current = null;
      else if (layout) {
        const target = Math.max(0, Math.min(maximum, layout.y - 8));
        list.current?.scrollToOffset({ offset: target, animated: false });
        if (Math.abs(g.offset - target) <= POSITION_EPSILON) {
          pendingReveal.current = null;
          readyRef.current = true; setReady(true); updateAway(); save();
        }
        return;
      } else if (reveal.attempts < 12) {
        reveal.attempts++;
        list.current?.scrollToIndex({ index, animated: false, viewPosition: 0, viewOffset: 8 });
        return;
      } else pendingReveal.current = null;
    }
    if (readyRef.current) {
      if (followEnd.current && interaction.current === 'idle') list.current?.scrollToOffset({ offset: maximum, animated: false });
      updateAway();
      return;
    }
    const checkpoint = saved.current;
    const anchor = checkpoint?.anchor;
    const anchorExists = anchor && messages.some(message => message.id === anchor.messageId);
    const anchorLayout = anchorExists ? layouts.current.get(anchor.messageId) : undefined;
    const restoring = checkpoint && !checkpoint.followEnd;
    const target = Math.max(0, Math.min(maximum, restoring
      ? anchorLayout && anchor ? anchorLayout.y + anchor.offset : checkpoint.offset
      : maximum));
    const measured = !messages.length || (restoring
      ? !anchorExists || !!anchorLayout
      : layouts.current.has(messages.at(-1)!.id));
    if (measured && Math.abs(g.offset - target) <= POSITION_EPSILON) {
      readyRef.current = true; setReady(true); updateAway(); save();
    } else {
      list.current?.scrollToOffset({ offset: target, animated: false });
    }
  };

  const CellRendererComponent = useMemo(() => function ChatCell({ item, children, onLayout, onFocusCapture, style }: CellRendererProps<ChatMessage>) {
    return createElement(View, { ...{ onFocusCapture }, collapsable: false, style, onLayout: event => {
      layouts.current.set(item.id, event.nativeEvent.layout);
      onLayout?.(event);
      scheduleAlign();
    } }, children);
  }, [scheduleAlign]);

  const setExpanded = useCallback((id: string, value: boolean) => {
    setExpandedState(current => {
      if (current.has(id) === value) return current;
      const next = new Set(current);
      if (value) next.add(id); else next.delete(id);
      return next;
    });
  }, []);
  useLayoutEffect(save, [expanded, save]);
  useLayoutEffect(() => () => {
    save();
    if (frame.current !== null) cancelAnimationFrame(frame.current);
    if (idleTimer.current !== null) clearTimeout(idleTimer.current);
    if (revealTimer.current !== null) clearTimeout(revealTimer.current);
  }, [save]);
  useEffect(scheduleAlign, [messages.length, scheduleAlign]);
  useLayoutEffect(() => {
    if (searching) { followEnd.current = false; updateAway(); save(); }
    else pendingReveal.current = null;
  }, [searching, save]);

  const revealMessage = useCallback((id: string) => {
    followEnd.current = false; interaction.current = 'idle';
    pendingReveal.current = { id, attempts: 0 };
    scheduleAlign();
  }, [scheduleAlign]);

  function onScroll({ nativeEvent: e }: NativeSyntheticEvent<NativeScrollEvent>) {
    const maximum = Math.max(0, e.contentSize.height - e.layoutMeasurement.height);
    const offset = Math.max(0, Math.min(maximum, e.contentOffset.y));
    geometry.current = { offset, height: e.contentSize.height, viewport: e.layoutMeasurement.height };
    if (readyRef.current && (interaction.current === 'drag' || interaction.current === 'momentum')) {
      const delta = offset - lastUserOffset.current;
      if (delta < -1) followEnd.current = false;
      else if (delta > 1 && maximum - offset < FOLLOW_THRESHOLD) followEnd.current = true;
      lastUserOffset.current = offset;
    }
    updateAway();
    if (!readyRef.current || pendingReveal.current) scheduleAlign();
    else save();
  }
  function beginScroll() {
    if (idleTimer.current !== null) clearTimeout(idleTimer.current);
    idleTimer.current = null;
    setScrolling(true);
  }
  function endScroll() {
    if (idleTimer.current !== null) clearTimeout(idleTimer.current);
    idleTimer.current = setTimeout(() => {
      idleTimer.current = null; interaction.current = 'idle'; setScrolling(false); save();
    }, SCROLL_IDLE_MS);
  }
  function jumpToLatest(animated = false) {
    pendingReveal.current = null;
    followEnd.current = true; interaction.current = 'idle'; setAway(false);
    if (!readyRef.current) { saved.current = undefined; scheduleAlign(); }
    list.current?.scrollToEnd({ animated });
    save();
  }
  return { list, ready, showJump: ready && away && !scrolling, jumpToLatest, revealMessage,
    expansion: useMemo(() => ({ expanded, setExpanded }), [expanded, setExpanded]),
    listProps: {
      CellRendererComponent,
      onViewableItemsChanged: useCallback(({ viewableItems }: { viewableItems: { item: ChatMessage; isViewable: boolean }[] }) => {
        firstVisible.current = viewableItems.find(item => item.isViewable)?.item.id ?? null;
        save();
      }, [save]),
      onScroll,
      onLayout: (event: { nativeEvent: { layout: { height: number } } }) => { geometry.current.viewport = event.nativeEvent.layout.height; scheduleAlign(); },
      onContentSizeChange: (_width: number, height: number) => { geometry.current.height = height; scheduleAlign(); },
      onScrollToIndexFailed: ({ averageItemLength, index }: { averageItemLength: number; index: number }) => {
        if (!pendingReveal.current) return;
        list.current?.scrollToOffset({ offset: averageItemLength * index, animated: false });
        if (revealTimer.current !== null) clearTimeout(revealTimer.current);
        revealTimer.current = setTimeout(() => { revealTimer.current = null; scheduleAlign(); }, 100);
      },
      onScrollBeginDrag: (event: NativeSyntheticEvent<NativeScrollEvent>) => { pendingReveal.current = null; beginScroll(); interaction.current = 'drag'; lastUserOffset.current = event.nativeEvent.contentOffset.y; },
      onScrollEndDrag: (event: NativeSyntheticEvent<NativeScrollEvent>) => { onScroll(event); interaction.current = 'awaiting'; endScroll(); },
      onMomentumScrollBegin: () => { if (interaction.current === 'awaiting') { beginScroll(); interaction.current = 'momentum'; lastUserOffset.current = geometry.current.offset; } },
      onMomentumScrollEnd: (event: NativeSyntheticEvent<NativeScrollEvent>) => { onScroll(event); endScroll(); },
      scrollEventThrottle: 16,
    },
  };
}
