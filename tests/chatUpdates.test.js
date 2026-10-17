import { test } from 'node:test';
import assert from 'node:assert/strict';
import { applyChatUpdate } from '../src/ai/chatUpdates.ts';

test('stream deltas preserve untouched blocks and games, append Unicode, and replace corrected text', () => {
  const block = { id: 'old', kind: 'text', content: 'Hola' };
  const game = { id: 'game', revision: 0 };
  const initial = applyChatUpdate(null, { kind: 'snapshot', revision: 4, state: { status: 'running', text: 'Hola 🌎', reasoning: 'Plan', blocks: [block], games: [game] } });
  const added = { id: 'new', kind: 'text', content: 'Buen día' };
  const next = applyChatUpdate(initial, { kind: 'delta', baseRevision: 4, revision: 5, fields: {}, strings: { text: { append: '. Buen día' } }, arrays: { blocks: { length: 2, upserts: [{ index: 1, value: added }] } } });
  assert.equal(next.state.text, 'Hola 🌎. Buen día');
  assert.equal(next.state.blocks[0], block);
  assert.equal(next.state.blocks[1], added);
  assert.equal(next.state.games, initial.state.games);
  const final = applyChatUpdate(next, { kind: 'delta', baseRevision: 5, revision: 6, fields: { status: 'completed' }, strings: { text: { replace: 'Correction' }, reasoning: { append: ' complete' } }, arrays: { blocks: { length: 1, upserts: [] } } });
  assert.equal(final.state.text, 'Correction');
  assert.equal(final.state.reasoning, 'Plan complete');
  assert.equal(final.state.status, 'completed');
  assert.deepEqual(final.state.blocks, [block]);
  assert.equal(final.state.games[0], game);
});

test('duplicates and stale events preserve identity; missing revisions require a snapshot', () => {
  const current = applyChatUpdate(null, { kind: 'snapshot', revision: 4, state: { status: 'running', text: 'Current' } });
  for (const update of [
    { kind: 'unchanged', revision: 4 },
    { kind: 'snapshot', revision: 3, state: { text: 'Old' } },
    { kind: 'delta', revision: 4, baseRevision: 3 },
  ]) assert.equal(applyChatUpdate(current, update), current);
  for (const update of [
    { kind: 'unchanged', revision: 5 },
    { kind: 'delta', baseRevision: 3, revision: 5 },
    { kind: 'delta', baseRevision: 4, revision: 6 },
  ]) assert.equal(applyChatUpdate(current, update), null);
  assert.equal(applyChatUpdate(null, { kind: 'unchanged', revision: 1 }), null);
  const recovered = applyChatUpdate(current, { kind: 'snapshot', revision: 8, state: { status: 'completed', text: 'Recovered' } });
  assert.equal(recovered.state.text, 'Recovered');
});
