import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import React, { createElement } from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
const videoId = 'abcdefghijk';
const transcript = { videoId, trackId: 'en' };

async function harness(props) {
  const calls = [], boundaries = [], positions = [];
  let renders = 0;
  const publishPosition = position => positions.push(position);
  const result = await build({
    entryPoints: ['src/media/useTranscriptPosition.ts'], bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external',
    plugins: [{ name: 'core-edge', setup(builder) {
      builder.onResolve({ filter: /modules\/govue-core$/ }, () => ({ path: 'boundary-core', external: true }));
      builder.onResolve({ filter: /\/core\/transcripts$/ }, () => ({ path: 'test-core', external: true }));
    } }],
  });
  const module = { exports: {} };
  Function('require', 'module', 'exports', result.outputFiles[0].text)(name => name === 'test-core' ? {
    advanceTranscriptPlayback: (...args) => new Promise(resolve => calls.push({ args, resolve })),
  } : name === 'boundary-core' ? { browserRequest: () => boundaries.shift() ?? null } : require(name), module, module.exports);
  const listeners = new Map();
  const player = { currentTime: 0.9, playing: true, playbackRate: 1, timeUpdateEventInterval: 0.25, pauses: 0,
    pause() { this.pauses++; this.playing = false; },
    addListener(name, callback) { listeners.set(name, callback); return { remove: () => listeners.delete(name) }; },
  };
  const injections = [];
  const webView = { current: { injectJavaScript: code => injections.push(code) } };
  let playback;
  function Reader({ enabled = true, native = true, publish = false }) {
    renders++;
    playback = module.exports.useTranscriptPosition(webView, player, videoId, enabled, native ? videoId : null, transcript, true, publish ? publishPosition : undefined);
    return null;
  }
  let root;
  await act(() => { root = create(createElement(Reader, props)); });
  return { calls, boundaries, player, listeners, injections, positions, renders: () => renders, playback: () => playback,
    render: props => act(() => root.update(createElement(Reader, props))), unmount: () => act(() => root.unmount()) };
}

test('native line pauses follow Rust decisions and discard pending results after closing the reader', async t => {
  const env = await harness();
  t.after(env.unmount);
  assert.equal(env.player.timeUpdateEventInterval, 0.1);
  await act(() => env.calls[0].resolve({ pause: false, cursor: { line: 0 } }));
  env.player.currentTime = 1.05;
  await act(() => { env.listeners.get('timeUpdate')(); });
  assert.deepEqual(env.calls[1].args[1], { line: 0 });
  await act(() => env.calls[1].resolve({ pause: true, cursor: { line: 1 } }));
  assert.equal(env.player.pauses, 1);
  env.player.playing = true;
  await act(() => { env.listeners.get('playingChange')(); });
  await env.render({ enabled: false });
  await act(() => env.calls[2].resolve({ pause: true, cursor: {} }));
  assert.equal(env.player.pauses, 1);
  assert.equal(env.player.timeUpdateEventInterval, 0.25);
  assert.equal(env.listeners.size, 0);
});

test('a transcript tap invalidates pending pause decisions before seeking', async t => {
  const env = await harness();
  t.after(env.unmount);
  await act(() => env.playback().reset());
  await act(() => env.calls[0].resolve({ pause: true, cursor: { stale: true } }));
  assert.equal(env.player.pauses, 0);
  env.player.currentTime = 20;
  await act(() => { env.listeners.get('timeUpdate')(); });
  assert.equal(env.calls[1].args[1], null, 'the seek starts a fresh Rust cursor');
  await act(() => env.calls[1].resolve({ pause: false, cursor: {} }));
});

test('web pause samples use Rust validation before applying pause decisions', async t => {
  const env = await harness();
  t.after(env.unmount);
  await act(() => env.calls[0].resolve({ pause: false, cursor: {} }));
  await env.render({ native: false });
  const requestId = JSON.parse(env.injections.at(-1).match(/position\((".*?"),/)[1]);
  const sample = { requestId, videoId, seconds: 1.05, playing: true, seeking: false, rate: 1 };
  for (const [message, origin] of [
    [sample, 'https://evil.test'],
    [sample, 'https://m.youtube.com/watch?v=0123456789a'],
    [{ ...sample, requestId: 'stale' }, 'https://m.youtube.com'],
    [{ ...sample, rate: Infinity }, 'https://m.youtube.com'],
  ]) await act(() => env.playback().handlePosition(message, origin));
  assert.equal(env.calls.length, 1);
  env.boundaries.push({ position: sample.seconds, playing: true, seeking: false, rate: 1 });
  await act(() => env.playback().handlePosition(sample, 'https://m.youtube.com'));
  await act(() => env.calls[1].resolve({ pause: true, cursor: {} }));
  assert.match(env.injections.at(-1), /\.pause\(/);
  await env.render({ enabled: false, native: false });
  assert.match(env.injections.at(-1), /\.cancelPosition\(/);
});

test('the reader publishes fresh native time without rerendering its owner or increasing player-control events', async t => {
  t.mock.timers.enable({ apis: ['setInterval'] });
  const env = await harness({ publish: true }); t.after(env.unmount);
  assert.equal(env.player.timeUpdateEventInterval, 0.25);
  assert.equal(env.positions.at(-1), 0.9);
  await act(() => env.calls[0].resolve({ pause: false, cursor: {} }));
  const renders = env.renders();
  env.player.currentTime = 1.3;
  await act(() => t.mock.timers.tick(50));
  assert.equal(env.positions.at(-1), 1.3);
  assert.equal(env.calls[1].args[2].position, 1.3, 'line pauses receive actual media time');
  assert.equal(env.renders(), renders, 'the app owning the player does not render on a clock tick');
  await act(() => env.playback().reset());
  assert.equal(env.positions.at(-1), null);
  await env.render({ enabled: false, publish: true });
  const samples = env.positions.length;
  await act(() => t.mock.timers.tick(500));
  assert.equal(env.positions.length, samples);
  await act(() => env.calls[1].resolve({ pause: true, cursor: {} }));
  assert.equal(env.player.pauses, 0);
});
