import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import React, { createElement } from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
const first = 'abcdefghijk', second = '0123456789a';
const transcript = (videoId, trackId = '.en') => ({ videoId, trackId, title: 'Example', language: trackId,
  tracks: ['.en', '.ja'].map(id => ({ id, language: id, label: id, automatic: false })),
  segments: [{ start: 1, duration: 2, text: trackId }] });
const deferred = () => {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
};

async function harness({ open = true, restore = (videoId, trackId) => !trackId || trackId === '.en' ? transcript(videoId) : null } = {}) {
  const restores = [], extractions = [], selections = [], replies = [];
  const compiled = await build({ entryPoints: ['src/player/usePlayerTranscript.ts'], bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external',
    plugins: [{ name: 'transcript-edge', setup(builder) {
      builder.onResolve({ filter: /\/core\/transcripts$/ }, () => ({ path: 'test-transcripts', external: true }));
    } }],
  });
  const module = { exports: {} };
  Function('require', 'module', 'exports', compiled.outputFiles[0].text)(name => name === 'test-transcripts' ? {
    restoreTranscript: async (videoId, trackId, audioKey) => { restores.push({ videoId, trackId, audioKey }); return await restore(videoId, trackId, audioKey); },
    selectTranscriptTracks: async (...args) => { selections.push(args); return await replies.shift(); },
    extractTranscript: (videoId, trackId, signal, fresh, audioKey) => {
      const work = deferred(); extractions.push({ videoId, trackId, signal, fresh, audioKey, ...work }); return work.promise;
    },
  } : require(name), module, module.exports);
  let reader, root;
  function Reader({ videoId = first, audioKey = '' }) { reader = module.exports.usePlayerTranscript(videoId, undefined, audioKey); return null; }
  await act(async () => { root = create(createElement(Reader)); });
  if (open) await act(() => reader.toggle());
  return { reader: () => reader, restores, extractions, selections, replies,
    render: (videoId, audioKey = '') => act(() => root.update(createElement(Reader, { videoId, audioKey }))),
    unmount: () => act(() => root.unmount()) };
}

test('adding a second language keeps the first usable and removing it cancels pending extraction', async t => {
  const env = await harness(); t.after(env.unmount);
  const primary = env.reader().data;
  env.replies.push(['.en', '.ja']);
  await act(() => env.reader().selectTrack('.ja'));
  assert.equal(env.reader().data, primary);
  assert.equal(env.reader().loading, false);
  assert.equal(env.reader().comparisonLoading, true);
  assert.deepEqual(env.restores.at(-1), { videoId: first, trackId: '.ja', audioKey: '' });
  assert.deepEqual(env.selections[0], [primary, ['.en'], '.ja']);
  env.replies.push(['.en']);
  await act(() => env.reader().selectTrack('.ja'));
  assert.equal(env.extractions[0].signal.aborted, true);
  await act(() => env.extractions[0].resolve(transcript(first, '.ja')));
  assert.equal(env.reader().data, primary);
  assert.equal(env.reader().comparison, null);
  assert.deepEqual(env.reader().trackIds, ['.en']);
});

test('a failed comparison preserves the primary transcript and removing it clears the error', async t => {
  const env = await harness(); t.after(env.unmount);
  const primary = env.reader().data;
  env.replies.push(['.en', '.ja']);
  await act(() => env.reader().selectTrack('.ja'));
  await act(() => env.extractions[0].reject(Error('Comparison unavailable')));
  assert.equal(env.reader().data, primary);
  assert.equal(env.reader().error, null);
  assert.equal(env.reader().comparisonError, 'Comparison unavailable');
  assert.equal(env.reader().comparisonLoading, false);
  env.replies.push(['.en']);
  await act(() => env.reader().selectTrack('.ja'));
  assert.equal(env.reader().comparisonError, null);
});

test('closing or navigating prevents late selection and extraction from reopening an old reader', async t => {
  const env = await harness(); t.after(env.unmount);
  const pending = deferred();
  env.replies.push(pending.promise);
  await act(() => env.reader().selectTrack('.ja'));
  await act(() => env.reader().close());
  await act(() => pending.resolve(['.en', '.ja']));
  assert.equal(env.reader().open, false);
  assert.equal(env.reader().data.videoId, first);
  assert.equal(env.extractions.length, 0);
  await act(() => env.reader().toggle());
  env.replies.push(['.en', '.ja']);
  await act(() => env.reader().selectTrack('.ja'));
  await env.render(second);
  assert.equal(env.reader().open, false);
  assert.equal(env.reader().data.videoId, second);
  assert.equal(env.extractions[0].signal.aborted, true);
  await act(() => env.extractions[0].resolve(transcript(first, '.ja')));
  await act(() => env.reader().toggle());
  assert.equal(env.reader().data.videoId, second);
  assert.equal(env.reader().comparison, null);
});

test('opening a video preloads captions and reader visibility reuses the same pending and completed work', async t => {
  const env = await harness({ open: false, restore: () => null }); t.after(env.unmount);
  assert.equal(env.reader().open, false);
  assert.equal(env.reader().loading, true);
  assert.equal(env.extractions.length, 1);
  const pending = env.extractions[0];
  assert.equal(pending.videoId, first);
  assert.equal(pending.trackId, 'default');
  await act(() => env.reader().toggle());
  assert.equal(env.reader().open, true);
  await act(() => env.reader().close());
  assert.equal(pending.signal.aborted, false);
  const data = transcript(first);
  await act(async () => { pending.resolve(data); });
  assert.equal(env.reader().open, false);
  assert.equal(env.reader().data, data);
  await act(() => env.reader().toggle());
  assert.equal(env.reader().open, true);
  assert.equal(env.reader().data, data);
  assert.equal(env.reader().loading, false);
  assert.equal(env.restores.length, 1);
  assert.equal(env.extractions.length, 1);
});

test('navigation cancels a hidden preload and discards its late result while preloading the new video', async t => {
  const env = await harness({ open: false, restore: () => null }); t.after(env.unmount);
  const previous = env.extractions[0];
  await env.render(second);
  assert.equal(previous.signal.aborted, true);
  assert.equal(env.extractions.length, 2);
  assert.equal(env.extractions[1].videoId, second);
  await act(async () => { previous.resolve(transcript(first)); });
  assert.equal(env.reader().data, null);
  assert.equal(env.reader().open, false);
  await act(async () => { env.extractions[1].resolve(transcript(second)); });
  assert.equal(env.reader().data.videoId, second);
  assert.equal(env.reader().open, false);
  await env.render(null);
  assert.equal(env.reader().data, null);
  assert.equal(env.reader().loading, false);
  assert.equal(env.extractions.length, 2);
});

test('changing audio reloads the default captions without retaining the previous audio transcript', async t => {
  const env = await harness({ restore: (videoId, trackId, audioKey) => transcript(videoId, audioKey === 'ja.1' ? '.ja' : '.en') });
  t.after(env.unmount);
  assert.equal(env.reader().data.trackId, '.en');
  await env.render(first, 'ja.1');
  assert.equal(env.reader().open, true);
  assert.equal(env.reader().data.trackId, '.ja');
  assert.deepEqual(env.restores.at(-1), { videoId: first, trackId: undefined, audioKey: 'ja.1' });
  assert.equal(env.extractions.length, 0);
});

test('a failed background preload stays closed and opening the reader retries it', async t => {
  const env = await harness({ open: false, restore: () => null }); t.after(env.unmount);
  await act(async () => { env.extractions[0].reject(Error('Temporary extraction failure')); });
  assert.equal(env.reader().open, false);
  assert.equal(env.reader().error, 'Temporary extraction failure');
  await act(() => env.reader().toggle());
  assert.equal(env.reader().open, true);
  assert.equal(env.reader().error, null);
  assert.equal(env.extractions.length, 2);
  assert.equal(env.extractions[1].fresh, false);
  await act(async () => { env.extractions[1].resolve(transcript(first)); });
  assert.equal(env.reader().data.videoId, first);
  assert.equal(env.reader().loading, false);
});
