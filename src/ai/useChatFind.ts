import { useEffect, useRef, useState } from 'react';
import { aiRequest } from '../../modules/govue-core';
import type { ChatMessage } from './useChats';

export type SearchExcerpt = { before: string; matched: string; after: string; leading: boolean; trailing: boolean };
export type ChatSearchHit = SearchExcerpt & { messageId: string | null; role: 'user' | 'assistant' | null; offset: number };
type Results = { query: string; matches: ChatSearchHit[]; selected: number | null; truncated: boolean };
const empty: Results = { query: '', matches: [], selected: null, truncated: false };

/** Only schedules native search and presents its result; Rust owns matching and navigation. */
export function useChatFind(chatId: string | undefined, messages: ChatMessage[], enabled: boolean, initialQuery: string) {
  const [query, setQuery] = useState(initialQuery);
  const [results, setResults] = useState<Results>(empty);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [navigation, setNavigation] = useState(0);
  const current = useRef({ chatId, query, enabled });
  current.current = { chatId, query, enabled };
  const previous = useRef(empty);
  const sequence = useRef(0);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const navigating = useRef<number | null>(null);

  useEffect(() => {
    setQuery(initialQuery); setResults(empty); previous.current = empty; setError(''); setBusy(false);
    return () => {
      ++sequence.current;
      navigating.current = null;
      if (timer.current !== null) clearTimeout(timer.current);
      timer.current = null;
    };
  }, [chatId, enabled, initialQuery]);

  async function search(direction?: 'next' | 'previous') {
    const owner = current.current;
    if (!owner.enabled || !owner.chatId) return;
    const version = ++sequence.current;
    if (direction) navigating.current = version;
    const old = previous.current;
    const selected = old.selected === null ? undefined : old.matches[old.selected];
    setBusy(true); setError('');
    try {
      const result = await aiRequest<Results>({ op: 'ai_chat_find', chatId: owner.chatId, query: owner.query,
        previous: selected ? { query: old.query, messageId: selected.messageId, offset: selected.offset } : null, ...(direction ? { direction } : {}) });
      if (version !== sequence.current || !current.current.enabled || current.current.chatId !== owner.chatId || current.current.query !== owner.query) return;
      previous.current = result; setResults(result);
      if (direction) setNavigation(value => value + 1);
    } catch (reason) {
      if (version === sequence.current && current.current.chatId === owner.chatId && current.current.query === owner.query) setError(reason instanceof Error ? reason.message : 'Could not search this chat.');
    } finally {
      if (navigating.current === version) navigating.current = null;
      if (version === sequence.current) setBusy(false);
    }
  }

  useEffect(() => {
    if (!enabled || !chatId) return;
    if (!query.trim()) {
      ++sequence.current; previous.current = empty; setResults(empty); setError(''); setBusy(false);
      return;
    }
    // Coalesce streaming updates without postponing the timer on every token.
    if (timer.current !== null) return;
    const run = () => {
      timer.current = null;
      if (navigating.current !== null) { timer.current = setTimeout(run, 150); return; }
      void search();
    };
    timer.current = setTimeout(run, 150);
  }, [chatId, enabled, messages, query]);

  const ready = enabled && !error && results.query === query.trim();
  const hit = ready && results.selected !== null ? results.matches[results.selected] : undefined;
  return { query, setQuery, results, ready, busy, error, hit, navigation,
    navigate: (direction: 'next' | 'previous') => {
      if (!ready || busy || !hit) return;
      if (timer.current !== null) clearTimeout(timer.current);
      timer.current = null;
      void search(direction);
    } };
}

export type ChatFindController = ReturnType<typeof useChatFind>;
