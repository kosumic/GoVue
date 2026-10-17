import type { ChatMessage } from './useChats';

export type ChatProgress = {
  status: 'running' | 'completed' | 'failed' | 'cancelled';
  text: string;
} & Pick<ChatMessage, 'games' | 'blocks' | 'error' | 'reasoning' | 'reasoningEffort' | 'usage' | 'usageDetails' | 'model' | 'providerLabel'>;
type ArrayDelta<T> = { length: number; upserts: { index: number; value: T }[] };
type StringDelta = { append: string } | { replace: string };
export type ChatUpdate =
  | { kind: 'snapshot'; revision: number; state: ChatProgress }
  | { kind: 'unchanged'; revision: number }
  | { kind: 'delta'; revision: number; baseRevision: number;
      fields: Partial<Omit<ChatProgress, 'text' | 'reasoning' | 'blocks' | 'games'>>;
      strings: { text?: StringDelta; reasoning?: StringDelta };
      arrays: { blocks?: ArrayDelta<NonNullable<ChatMessage['blocks']>[number]>; games?: ArrayDelta<NonNullable<ChatMessage['games']>[number]> } };
export type ChatStream = { revision: number; state: ChatProgress };

function stringDelta(previous: string | undefined, delta: StringDelta | undefined) {
  return !delta ? previous : 'replace' in delta ? delta.replace : (previous ?? '') + delta.append;
}
function arrayDelta<T>(previous: T[] | undefined, delta: ArrayDelta<T> | undefined): T[] | undefined {
  if (!delta) return previous;
  const next = previous?.slice(0, delta.length) ?? [];
  for (const { index, value } of delta.upserts) next[index] = value;
  return next;
}

/** Mechanical application of Rust's projection. A gap requires a fresh snapshot. */
export function applyChatUpdate(current: ChatStream | null, update: ChatUpdate): ChatStream | null {
  if (current && update.revision <= current.revision) return current;
  if (update.kind === 'snapshot') return { revision: update.revision, state: update.state };
  if (!current || update.kind === 'unchanged' || update.baseRevision !== current.revision || update.revision !== current.revision + 1) return null;
  const previous = current.state;
  return { revision: update.revision, state: { ...previous, ...update.fields,
    text: stringDelta(previous.text, update.strings.text) ?? '',
    reasoning: stringDelta(previous.reasoning, update.strings.reasoning),
    blocks: arrayDelta(previous.blocks, update.arrays.blocks),
    games: arrayDelta(previous.games, update.arrays.games),
  } };
}
