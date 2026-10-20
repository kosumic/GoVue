import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { createElement } from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
const first = 'abcdefghijk', second = '0123456789a';
const deferred = () => { let resolve; const promise = new Promise(done => { resolve = done; }); return { promise, resolve }; };

async function harness(state = 'active') {
  const preferences = deferred(), extractions = [], appListeners = new Set();
  const commands = [], approvals = [], plans = [];
  const defaults = { backgroundPlayback: true, adBlock: true, autoPip: true, sponsorBlock: false,
    categories: [], playbackSpeed: 1, useOriginalTitle: false, rememberSpeed: false, rememberResize: false };
  const app = { currentState: state, addEventListener: (_event, callback) => {
    appListeners.add(callback); return { remove: () => appListeners.delete(callback) };
  } };
  const player = { currentTime: 0, playing: false, status: 'idle', timeUpdateEventInterval: 0.25,
    addListener: () => ({ remove() {} }), pause() {}, play() {}, replaceAsync: async () => {} };
  let setup = false;
  const cast = { stateRef: { current: {} }, state: {}, stop: async () => {}, control: async () => false };
  const media = {
    defaultMediaSettings: defaults,
    mediaSettings: changes => changes ? Promise.resolve({ ...defaults, ...changes }) : preferences.promise,
    playbackSpeeds: async () => [1], playbackChoices: async () => [],
    extractMedia: (videoId, signal) => { const task = deferred(); extractions.push({ videoId, signal, ...task }); return task.promise; },
  };
  const result = await build({
    entryPoints: ['src/media/useMediaBrowser.ts'], bundle: true, write: false,
    platform: 'node', format: 'cjs', packages: 'external',
    plugins: [{ name: 'media-startup-edges', setup(builder) {
      for (const [filter, path] of [
        [/\/core\/media$/, 'test-media'], [/modules\/govue-core$/, 'test-core'],
        [/\/useCastSession$/, 'test-cast'], [/\/useTranscriptPosition$/, 'test-position'],
        [/\/youtube\/mediaBridge$/, 'test-bridge'], [/\/youtube\/pageScripts$/, 'test-page'],
      ]) builder.onResolve({ filter }, () => ({ path, external: true }));
    } }],
  });
  const module = { exports: {} };
  Function('require', 'module', 'exports', result.outputFiles[0].text)(name => {
    if (name === 'react-native') return { AppState: app, Alert: { alert() {} } };
    if (name === 'expo-video') return { useVideoPlayer: (_source, configure) => {
      if (!setup) { setup = true; configure(player); } return player;
    } };
    if (name === 'test-media') return media;
    if (name === 'test-core') return {
      coreRequest: async command => command.op === 'media_message_origin' ? true : { seek: null },
      stateRequest(command) {
        commands.push(command);
        if (command.op === 'player_session_create') return 1;
        if (command.op === 'player_session_auto') return approvals.shift() ?? false;
        if (command.op === 'player_session_begin') return plans.shift() ?? null;
        return false;
      },
    };
    if (name === 'test-cast') return { useCastSession: () => cast };
    if (name === 'test-position') return { useTranscriptPosition: () => ({ position: 0, handlePosition() {}, reset() {} }) };
    if (name === 'test-bridge') return { mediaNativeScript: () => '', mediaSettingsScript: () => '', mediaSponsorsScript: () => '' };
    if (name === 'test-page') return { seekScript: () => '' };
    return require(name);
  }, module, module.exports);
  const webView = { current: { injectJavaScript() {} } };
  let api, root;
  function Host({ id = first }) { api = module.exports.useMediaBrowser(webView, id, 'fixture'); return null; }
  await act(async () => { root = create(createElement(Host)); });
  return {
    extractions, commands, api: () => api,
    approve: (videoId = first) => { approvals.push(true); plans.push({ token: plans.length + 1, videoId, settings: {}, height: 0, fps: 0 }); },
    preferences: overrides => act(async () => { preferences.resolve({ ...defaults, ...overrides }); }),
    render: id => act(async () => root.update(createElement(Host, { id }))),
    ready: () => act(async () => { await api.handleMessage('', 'https://m.youtube.com', { type: 'mediaReady', videoId: first }); }),
    appState: value => act(async () => { app.currentState = value; for (const listener of [...appListeners]) listener(value); }),
    unmount: () => act(async () => root.unmount()),
  };
}

test('startup waits for preferences and obeys the native startup decision', async t => {
  const env = await harness(); t.after(env.unmount);
  assert.equal(env.extractions.length, 0);
  await env.preferences();
  assert.equal(env.extractions.length, 0, 'a denied native decision must stay denied');
  const command = env.commands.findLast(c => c.op === 'player_session_auto');
  assert.equal(command.ready, true);
  assert.equal(command.owner, first);
  assert.equal(command.foreground, true);
  env.approve(); await env.ready();
  assert.deepEqual(env.extractions.map(job => job.videoId), [first]);
  await env.ready();
  assert.equal(env.extractions.length, 1);
});

test('navigation cancels extraction and late results cannot replace the new selection', async t => {
  const env = await harness(); t.after(env.unmount);
  await env.preferences(); env.approve(); await env.ready();
  const old = env.extractions[0];
  await env.render(second);
  assert.equal(old.signal.aborted, true);
  assert.equal(env.commands.findLast(c => c.op === 'player_session_select').videoId, second);
  await act(async () => old.resolve({ videoId: first }));
  assert.equal(env.api().catalog, null);
});

test('foreground and manual web playback are reported to the Rust session', async t => {
  const env = await harness('background'); t.after(env.unmount);
  await env.preferences({ adBlock: false, backgroundPlayback: false, autoPip: false });
  const command = env.commands.findLast(c => c.op === 'player_session_auto');
  assert.equal(command.foreground, false);
  assert.equal(command.settings.adBlock, false);
  await env.appState('active');
  assert.equal(env.commands.findLast(c => c.op === 'player_session_auto').foreground, true);
  await act(() => env.api().closePlayer());
  assert.equal(env.commands.findLast(c => c.op === 'player_session_close').manual, true);
  await env.ready();
  assert.equal(env.extractions.length, 0);
});
