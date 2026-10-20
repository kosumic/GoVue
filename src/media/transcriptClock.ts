// A UI subscription to the current media time. Captions retain their source times.
export type TranscriptClock = {
  getSnapshot: () => number | null;
  subscribe: (listener: () => void) => () => void;
  update: (position: number | null) => void;
};

export const idleTranscriptClock = () => () => {};

export function createTranscriptClock(): TranscriptClock {
  let position: number | null = null;
  const listeners = new Set<() => void>();
  return {
    getSnapshot: () => position,
    subscribe(listener) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    update(next) {
      if (Object.is(position, next)) return;
      position = next;
      for (const listener of listeners) listener();
    },
  };
}
