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

async function harness() {
  const restores = [], extractions = [], selections = [], replies = [];
  const compiled = await build({ entryPoints: ['src/player/usePlayerTranscript.ts'], bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external',
    plugins: [{ name: 'transcript-edge', setup(builder) {
      builder.onResolve({ filter: /\/core\/transcripts$/ }, () => ({ path: 'test-transcripts', external: true }));
    } }],
  });
  const module = { exports: {} };
  Function('require', 'module', 'exports', compiled.outputFiles[0].text)(name => name === 'test-transcripts' ? {
    restoreTranscript: async (videoId, trackId) => { restores.push({ videoId, trackId }); return !trackId || trackId === '.en' ? transcript(videoId) : null; },
    selectTranscriptTracks: async (...args) => { selections.push(args); return await replies.shift(); },
    extractTranscript: (videoId, trackId, signal) => {
      const work = deferred(); extractions.push({ videoId, trackId, signal, ...work }); return work.promise;
    },
  } : require(name), module, module.exports);
  let reader, root;
  function Reader({ videoId = first }) { reader = module.exports.usePlayerTranscript(videoId); return null; }
  await act(() => { root = create(createElement(Reader)); });
  await act(() => reader.toggle());
  return { reader: () => reader, restores, extractions, selections, replies,
    render: videoId => act(() => root.update(createElement(Reader, { videoId }))),
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
  assert.deepEqual(env.restores.at(-1), { videoId: first, trackId: '.ja' });
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
  assert.equal(env.extractions.length, 0);
  await act(() => env.reader().toggle());
  env.replies.push(['.en', '.ja']);
  await act(() => env.reader().selectTrack('.ja'));
  await env.render(second);
  assert.equal(env.reader().open, false);
  assert.equal(env.reader().data, null);
  assert.equal(env.extractions[0].signal.aborted, true);
  await act(() => env.extractions[0].resolve(transcript(first, '.ja')));
  await act(() => env.reader().toggle());
  assert.equal(env.reader().data.videoId, second);
  assert.equal(env.reader().comparison, null);
});
