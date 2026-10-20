import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { createElement } from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
const first = 'abcdefghijk', second = '0123456789a';
const transcript = (videoId, trackId = '.en') => ({ videoId, trackId, title: 'Example', language: trackId,
  tracks: ['.en', '.ja'].map(id => ({ id, language: id, label: id, automatic: false })),
  segments: [{ start: 1, duration: 2, text: trackId }] });
const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; };

// The fixture supplies authoritative native snapshots. Loading/cache policy is
// exercised in core/tests/transcript_sessions.rs rather than recreated here.
async function harness() {
  const commands = [], selections = [], replies = [], listeners = new Set();
  let snapshot = null;
  const compiled = await build({ entryPoints: ['src/player/usePlayerTranscript.ts'], bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external',
    plugins: [{ name: 'transcript-edge', setup(builder) {
      builder.onResolve({ filter: /\/core\/transcripts$/ }, () => ({ path: 'test-transcripts', external: true }));
      builder.onResolve({ filter: /modules\/govue-core$/ }, () => ({ path: 'test-native', external: true }));
    } }],
  });
  const module = { exports: {} };
  Function('require', 'module', 'exports', compiled.outputFiles[0].text)(name => {
    if (name === 'test-transcripts') return {
      selectTranscriptTracks: async (...args) => { selections.push(args); return await replies.shift(); },
    };
    if (name === 'test-native') return {
      observeCoreUpdates: (_channel, listener) => { listeners.add(listener); return { remove: () => listeners.delete(listener) }; },
      coreRequest: async command => {
        commands.push(command);
        if (command.op === 'transcript_session_load') {
          snapshot = { version: command.version, documents: [], loading: true, error: null };
          return snapshot;
        }
        if (command.op === 'transcript_session_snapshot') return snapshot;
        return null;
      },
    };
    return require(name);
  }, module, module.exports);
  let reader, root;
  function Reader({ videoId = first, audioKey = '' }) { reader = module.exports.usePlayerTranscript(videoId, undefined, audioKey); return null; }
  await act(async () => { root = create(createElement(Reader)); });
  return {
    reader: () => reader, commands, selections, replies,
    load: () => commands.findLast(c => c.op === 'transcript_session_load'),
    publish: (documents, values = {}) => act(async () => {
      snapshot = { ...snapshot, documents: documents.map(data => ({ data, cached: true })), loading: false, error: null, ...values };
      for (const listener of listeners) listener();
    }),
    render: (videoId, audioKey = '') => act(async () => root.update(createElement(Reader, { videoId, audioKey }))),
    unmount: () => act(async () => root.unmount()),
  };
}

test('adding and removing a comparison renders the native snapshot without replacing the primary', async t => {
  const env = await harness(); t.after(env.unmount);
  const primary = transcript(first); await env.publish([primary]);
  env.replies.push(['.en', '.ja']); await act(() => env.reader().selectTrack('.ja'));
  assert.deepEqual(env.load().trackIds, ['.en', '.ja']);
  await env.publish([primary], { loading: true });
  assert.deepEqual(env.reader().data, primary);
  assert.equal(env.reader().loading, false);
  assert.equal(env.reader().comparisonLoading, true);
  const oldVersion = env.load().version;
  env.replies.push(['.en']); await act(() => env.reader().selectTrack('.ja'));
  await env.publish([primary]);
  await env.publish([primary, transcript(first, '.ja')], { version: oldVersion });
  assert.equal(env.reader().comparison, null, 'a snapshot from the removed selection is stale');
});

test('a failed comparison preserves the primary and retry requests only the selected tracks', async t => {
  const env = await harness(); t.after(env.unmount);
  const primary = transcript(first); await env.publish([primary]);
  env.replies.push(['.en', '.ja']); await act(() => env.reader().selectTrack('.ja'));
  await env.publish([primary], { error: 'Comparison unavailable' });
  assert.deepEqual(env.reader().data, primary);
  assert.equal(env.reader().error, null);
  assert.equal(env.reader().comparisonError, 'Comparison unavailable');
  await act(() => env.reader().extract());
  assert.equal(env.load().fresh, false);
  assert.deepEqual(env.load().trackIds, ['.en', '.ja']);
  await env.publish([primary, transcript(first, '.ja')]);
  assert.equal(env.reader().comparison.trackId, '.ja');
  assert.equal(env.reader().comparisonError, null);
});

test('refreshing a successful transcript explicitly bypasses cached captions', async t => {
  const env = await harness(); t.after(env.unmount);
  await env.publish([transcript(first)]);
  await act(() => env.reader().extract());
  assert.equal(env.load().fresh, true);
  assert.deepEqual(env.load().trackIds, ['.en']);
});

test('closing the reader rejects late language selection while native preloading continues', async t => {
  const env = await harness(); t.after(env.unmount);
  const primary = transcript(first); await env.publish([primary]);
  await act(() => env.reader().toggle());
  const pending = deferred(); env.replies.push(pending.promise);
  await act(() => env.reader().selectTrack('.ja'));
  const loads = env.commands.filter(c => c.op === 'transcript_session_load').length;
  await act(() => env.reader().close());
  await act(() => pending.resolve(['.en', '.ja']));
  assert.equal(env.reader().open, false);
  assert.equal(env.commands.filter(c => c.op === 'transcript_session_load').length, loads);
  await env.publish([primary]);
  assert.deepEqual(env.reader().data, primary);
  await act(() => env.reader().toggle());
  assert.equal(env.reader().open, true);
});

test('navigation releases the old reader generation and ignores its late snapshot', async t => {
  const env = await harness(); t.after(env.unmount);
  const previous = env.load();
  await env.render(second);
  assert.ok(env.commands.some(c => c.op === 'transcript_session_release' && c.version === previous.version));
  assert.equal(env.load().videoId, second);
  await env.publish([transcript(first)], { version: previous.version });
  assert.equal(env.reader().data, null);
  await env.publish([transcript(second)], { version: env.load().version });
  assert.equal(env.reader().data.videoId, second);
  assert.equal(env.reader().open, false);
  await env.render(null);
  assert.equal(env.reader().data, null);
  assert.equal(env.reader().loading, false);
});

test('changing audio selects default captions for the new native audio session', async t => {
  const env = await harness(); t.after(env.unmount);
  await env.publish([transcript(first)]);
  await act(() => env.reader().toggle());
  await env.render(first, 'ja.1');
  assert.equal(env.reader().open, true);
  assert.equal(env.reader().data, null);
  assert.equal(env.load().audioKey, 'ja.1');
  assert.deepEqual(env.load().trackIds, ['default']);
  await env.publish([transcript(first, '.ja')]);
  assert.equal(env.reader().data.trackId, '.ja');
});

test('a failed hidden preload stays closed and opening it retries without forcing fresh data', async t => {
  const env = await harness(); t.after(env.unmount);
  await env.publish([], { error: 'Temporary extraction failure' });
  assert.equal(env.reader().open, false);
  assert.equal(env.reader().error, 'Temporary extraction failure');
  await act(() => env.reader().toggle());
  assert.equal(env.reader().open, true);
  assert.equal(env.reader().error, null);
  assert.equal(env.load().fresh, false);
});
