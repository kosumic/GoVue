import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import React, { createElement as h, forwardRef, useImperativeHandle } from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
const noop = () => {};
const colors = new Proxy({}, { get: () => '#777777' });
const bundled = await build({ entryPoints: ['src/components/LearningGameCard.tsx'], bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external',
  plugins: [{ name: 'game-surface', setup(builder) {
    builder.onResolve({ filter: /^(react-native|react-native-safe-area-context|react-native-webview|lucide-react-native|@react-native-clipboard\/clipboard)$|\/(theme|useGameAudio|govue-core|Haptics)$/ }, args => ({
      path: `mock:${args.path.split('/').at(-1)}`, external: true,
    }));
  } }],
});

async function surface(t, { active = true, exportError = '', audio = { phase: 'idle', stop: noop, play: async (_, started) => started?.() }, onGameAction } = {}) {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const actions = [], injections = [], exports = [], clipboard = [], haptics = [];
  const exported = JSON.stringify({ format: 'lingoplay-game', version: 1, definition: { title: 'Greeting' }, source: null, cues: [] });
  const mocks = {
    'react-native': {
      View: 'View', Text: 'Text', Pressable: 'Pressable', ActivityIndicator: 'ActivityIndicator',
      StyleSheet: { create: value => value },
      Modal: ({ visible, children }) => visible ? h('Modal', null, children) : null,
      AppState: { currentState: 'active', addEventListener: () => ({ remove: noop }) },
    },
    'react-native-safe-area-context': { SafeAreaView: 'SafeAreaView' },
    'react-native-webview': forwardRef(function WebView(props, ref) {
      useImperativeHandle(ref, () => ({ injectJavaScript: script => injections.push(script) }), []);
      return h('WebView', props);
    }),
    'lucide-react-native': new Proxy({}, { get: (_, name) => props => h(`Icon${String(name)}`, props) }),
    theme: { useThemeColors: () => colors, useThemeStyles: factory => factory(colors) },
    useGameAudio: { useGameAudio: () => audio },
    Haptics: { useHaptic: () => () => haptics.push('copy') },
    clipboard: { setString: text => clipboard.push(text) },
    'govue-core': { aiRequest: async command => {
      exports.push(command);
      if (exportError) throw Error(exportError);
      return { text: exported };
    } },
  };
  const module = { exports: {} };
  Function('require', 'module', 'exports', bundled.outputFiles[0].text)(name => name.startsWith('mock:') ? mocks[name.slice(5)] : require(name), module, module.exports);
  const game = {
    id: 'game', definition: { title: 'Greeting', html: '<button>Choose</button>', css: '', script: '' },
    cues: ['cue-0', 'cue-1'].map(id => ({ id, text: id, start: 0, duration: 1 })), state: { selected: [] }, revision: 0,
  };
  let root;
  await act(async () => { root = create(h(module.exports.LearningGameCard, {
    game, chatId: 'chat', messageId: 'reply', active, enabled: true, onActivate: noop, onDeactivate: noop,
    onAction: async (...args) => { actions.push(args); return onGameAction ? onGameAction(...args) : { ...game, state: { selected: ['good'] }, revision: 1 }; },
  })); });
  t.after(() => act(async () => root.unmount()));
  const view = () => root.root.findByType('WebView');
  const session = active ? JSON.parse(view().props.source.html.match(/const session=("[^"\n]+");/)[1]) : null;
  const post = (url, request, hostSession = session) => act(async () => view().props.onMessage({ nativeEvent: {
    url, data: JSON.stringify({ session: hostSession, request: { v: 1, ...request } }),
  } }));
  return { root, session, actions, injections, post, exports, clipboard, exported, haptics,
    button: label => root.root.findByProps({ accessibilityLabel: label }),
    labels: () => root.root.findAllByType('Text').map(node => node.children.join('')) };
}

for (const origin of ['null', 'about:blank']) {
  test(`game surface accepts the ${origin} bridge handshake and completes a correlated action`, async t => {
    const env = await surface(t);
    assert.ok(env.labels().includes('Opening game…'));
    await env.post(origin, { id: 'ready', method: 'ready' });
    assert.equal(env.labels().includes('Opening game…'), false);
    await act(async () => t.mock.timers.tick(10001));
    assert.equal(env.root.root.findAllByType('WebView').length, 1, 'a valid handshake prevents the load timeout');
    await env.post(origin, { id: '1', method: 'action', action: 'choose', input: { id: 'good' } });
    assert.deepEqual(env.actions, [['reply', 'game', 'choose', { id: 'good' }, `${env.session}-1`]]);
    const reply = JSON.parse(env.injections.at(-1).match(/__lingoGameReply\?\.\((.*)\); true;/)[1]);
    assert.deepEqual(reply, { id: '1', state: { selected: ['good'] } });
  });
}

test('opaque-origin messages still require the private host session and a completed handshake', async t => {
  const env = await surface(t);
  const ready = { id: 'ready', method: 'ready' };
  const action = { id: '1', method: 'action', action: 'choose', input: { id: 'good' } };
  await env.post('https://untrusted.example', ready);
  await env.post('null', ready, 'another-host-session');
  await env.post('null', action);
  assert.ok(env.labels().includes('Opening game…'));
  assert.equal(env.actions.length, 0);
  assert.equal(env.injections.length, 0);
  await env.post('null', ready);
  await env.post('null', action, 'another-host-session');
  await env.post('https://untrusted.example', action);
  assert.equal(env.actions.length, 0, 'even after readiness, unrelated sources and sessions cannot dispatch');
});

for (const active of [false, true]) {
  test(`copy exports a reusable game from ${active ? 'an open' : 'a closed'} card`, async t => {
    const env = await surface(t, { active });
    assert.equal(env.root.root.findAllByType('WebView').length, active ? 1 : 0);
    await act(async () => env.button('Copy game').props.onPress());
    assert.deepEqual(env.exports, [{ op: 'ai_game_export', chatId: 'chat', messageId: 'reply', gameId: 'game' }]);
    assert.deepEqual(env.clipboard, [env.exported]);
    assert.deepEqual(env.haptics, ['copy']);
    assert.ok(env.button('Game copied'));
    await act(async () => t.mock.timers.tick(2000));
    assert.ok(env.button('Copy game'));
  });
}

test('failed game export leaves the clipboard untouched and offers retry', async t => {
  const env = await surface(t, { active: false, exportError: 'This game is no longer available.' });
  await act(async () => env.button('Copy game').props.onPress());
  assert.deepEqual(env.clipboard, []);
  assert.deepEqual(env.haptics, []);
  assert.ok(env.labels().includes('This game is no longer available.'));
  assert.equal(env.button('Copy game').props.disabled, false);
});

const deferred = () => { let resolve, reject; const promise = new Promise((done, fail) => { resolve = done; reject = fail; }); return { promise, resolve, reject }; };
const replies = env => env.injections.map(script => JSON.parse(script.match(/__lingoGameReply\?\.\((.*)\); true;/)[1]));

test('audio start unlocks the card request while the clip is still playing', async t => {
  const playback = deferred();
  const played = [];
  const env = await surface(t, { audio: { phase: 'idle', stop: noop, play: (cueId, started) => {
    played.push(cueId); started(); return playback.promise;
  } } });
  await env.post('null', { id: 'ready', method: 'ready' });
  await env.post('null', { id: '1', method: 'audio', cueId: 'cue-0' });
  assert.deepEqual(played, ['cue-0']);
  assert.deepEqual(replies(env).filter(reply => reply.id === '1'), [{ id: '1' }]);
  await act(async () => { playback.resolve(); await playback.promise; });
  assert.deepEqual(replies(env).filter(reply => reply.id === '1'), [{ id: '1' }], 'playback completion does not reply a second time');
});

test('the newest audio tap stops playback immediately and ignores an older pending write', async t => {
  const pending = deferred();
  const played = [], stopped = [];
  const env = await surface(t, {
    audio: { phase: 'idle', stop: () => stopped.push('stop'), play: async (cueId, started) => { played.push(cueId); started(); } },
    onGameAction: (...args) => args[3].cueId === 'cue-0' ? pending.promise : { state: {} },
  });
  await env.post('null', { id: 'ready', method: 'ready' });
  await env.post('null', { id: '1', method: 'audio', cueId: 'cue-0' });
  assert.equal(stopped.length, 1, 'the old clip stops before the action write finishes');
  await env.post('null', { id: '2', method: 'audio', cueId: 'cue-1' });
  assert.equal(stopped.length, 2);
  assert.deepEqual(played, ['cue-1']);
  await act(async () => { pending.resolve({ state: {} }); await pending.promise; });
  assert.deepEqual(played, ['cue-1'], 'a late response cannot restart the earlier clip');
  assert.deepEqual(replies(env).filter(reply => reply.id === '1'), [{ id: '1' }]);
});

test('playback failures after the start acknowledgment remain visible without replying twice', async t => {
  const playback = deferred();
  const env = await surface(t, { audio: { phase: 'idle', stop: noop, play: (_, started) => { started(); return playback.promise; } } });
  await env.post('null', { id: 'ready', method: 'ready' });
  await env.post('null', { id: '1', method: 'audio', cueId: 'cue-0' });
  await act(async () => { playback.reject(Error('Caption audio could not be played.')); await playback.promise.catch(noop); });
  assert.ok(env.labels().includes('Caption audio could not be played.'));
  assert.deepEqual(replies(env).filter(reply => reply.id === '1'), [{ id: '1' }]);
});
