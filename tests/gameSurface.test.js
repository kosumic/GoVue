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
    builder.onResolve({ filter: /^(react-native|react-native-safe-area-context|react-native-webview|lucide-react-native)$|\/(theme|useGameAudio)$/ }, args => ({
      path: `mock:${args.path.split('/').at(-1)}`, external: true,
    }));
  } }],
});

async function surface(t) {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const actions = [], injections = [];
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
    useGameAudio: { useGameAudio: () => ({ phase: 'idle', stop: noop, play: async () => {} }) },
  };
  const module = { exports: {} };
  Function('require', 'module', 'exports', bundled.outputFiles[0].text)(name => name.startsWith('mock:') ? mocks[name.slice(5)] : require(name), module, module.exports);
  const game = {
    id: 'game', definition: { title: 'Greeting', html: '<button>Choose</button>', css: '', script: '' },
    cues: [], state: { selected: [] }, revision: 0,
  };
  let root;
  await act(async () => { root = create(h(module.exports.LearningGameCard, {
    game, chatId: 'chat', messageId: 'reply', active: true, enabled: true, onActivate: noop, onDeactivate: noop,
    onAction: async (...args) => { actions.push(args); return { ...game, state: { selected: ['good'] }, revision: 1 }; },
  })); });
  t.after(() => act(async () => root.unmount()));
  const view = () => root.root.findByType('WebView');
  const session = JSON.parse(view().props.source.html.match(/const session=("[^"\n]+");/)[1]);
  const post = (url, request, hostSession = session) => act(async () => view().props.onMessage({ nativeEvent: {
    url, data: JSON.stringify({ session: hostSession, request: { v: 1, ...request } }),
  } }));
  return { root, session, actions, injections, post, labels: () => root.root.findAllByType('Text').map(node => node.children.join('')) };
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
