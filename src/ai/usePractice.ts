import { useEffect, useRef, useState } from 'react';
import { aiRequest } from '../../modules/govue-core';
import type { GameAction, GameSource, LearningGame } from './games';

export type PracticeSession = {
  revision: number; started: boolean; game: LearningGame | null;
  remaining: number; total: number; forgot: number; known: number; canUndo: boolean;
};
type SavedGame = { id: string; title: string; source: GameSource | null; verdict: 'forgot' | 'know' | null };
type Library = { games: SavedGame[]; session: PracticeSession };

export function usePractice() {
  const [library, setLibrary] = useState<Library | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const current = useRef<Library | null>(null);
  const mounted = useRef(true);
  const version = useRef(0);
  const writes = useRef(Promise.resolve());
  const pending = useRef(0);
  function update(value: Library) { current.current = value; setLibrary(value); }
  async function load() {
    const request = ++version.current;
    setError('');
    try {
      const value = await aiRequest<Library>({ op: 'ai_practice_list' });
      if (mounted.current && version.current === request) update(value);
    } catch (reason) { if (mounted.current && version.current === request) setError(reason instanceof Error ? reason.message : 'Could not open Practice.'); }
  }
  useEffect(() => {
    mounted.current = true;
    void load();
    return () => { mounted.current = false; ++version.current; };
  }, []);
  function enqueue<T>(work: () => Promise<T>): Promise<T> {
    const request = version.current;
    ++pending.current; setBusy(true); setError('');
    const result = writes.current.catch(() => {}).then(async () => {
      if (!mounted.current || request !== version.current) throw Error('Practice is no longer open.');
      return work();
    });
    writes.current = result.then(() => {}, () => {});
    return result.catch(reason => {
      if (mounted.current && request === version.current) setError(reason instanceof Error ? reason.message : 'Could not save practice progress.');
      throw reason;
    }).finally(() => { --pending.current; if (mounted.current) setBusy(pending.current > 0); });
  }
  function updateSession(session: PracticeSession) {
    if (!mounted.current || !current.current) return;
    update({ ...current.current, session });
  }
  async function start(filter: 'all' | 'forgot' = 'all', gameId?: string) {
    return enqueue(async () => {
      const session = await aiRequest<PracticeSession>({ op: 'ai_practice_start', filter, ...(gameId ? { gameId } : {}) });
      updateSession(session);
    });
  }
  async function review(verdict: 'forgot' | 'know') {
    const id = current.current?.session.game?.id;
    const eventId = `review-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
    return enqueue(async () => {
      const saved = current.current;
      if (!saved || !id || saved.session.game?.id !== id) throw Error('This card is no longer current.');
      const session = await aiRequest<PracticeSession>({ op: 'ai_practice_review', gameId: id, verdict, eventId, revision: saved.session.revision });
      if (mounted.current) update({ ...saved, games: saved.games.map(game => game.id === id ? { ...game, verdict } : game), session });
    });
  }
  async function undo() {
    const eventId = `undo-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
    return enqueue(async () => {
      const session = await aiRequest<PracticeSession>({ op: 'ai_practice_undo', revision: current.current?.session.revision, eventId });
      updateSession(session);
      // The restored rating belongs to Rust; refresh the library after undo.
      const value = await aiRequest<Library>({ op: 'ai_practice_list' });
      if (mounted.current) update(value);
    });
  }
  function updateGame(game: LearningGame) {
    const saved = current.current;
    if (mounted.current && saved?.session.game?.id === game.id) updateSession({ ...saved.session, game });
  }
  const gameAction: GameAction = (_messageId, gameId, action, input, eventId) => enqueue(async () => {
    const game = current.current?.session.game;
    if (!game || game.id !== gameId) throw Error('This activity is no longer open.');
    const result = await aiRequest<{ game: LearningGame }>({ op: 'ai_practice_action', gameId, action, input, eventId, revision: game.revision });
    updateGame(result.game);
    return result.game;
  });
  async function reset() {
    return enqueue(async () => {
      const game = current.current?.session.game;
      if (!game) return;
      const result = await aiRequest<{ game: LearningGame }>({ op: 'ai_practice_reset', gameId: game.id, revision: game.revision });
      updateGame(result.game);
    });
  }
  return { library, error, busy, load, start, review, undo, reset, gameAction };
}
