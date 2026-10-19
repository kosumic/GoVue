import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import React, { createElement, forwardRef, useImperativeHandle } from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
const first = 'abcdefghijk', second = '0123456789a', third = 'lmnopqrstuv';
const url = id => `https://m.youtube.com/watch?v=${id}`;

test('accepted navigation selects native playback before page callbacks and ignores the previous URL', async t => {
  let navigate, selected, state = { tabs: [{ id: 1, url: url(first), kind: 'watch' }], activeId: 1,
    canGoBack: false, target: null, transient: 'none', revision: 1 };
  const writes = [], loads = [];
  const media = { settings: {}, player: {}, cast: { state: {} }, configurePage() {}, pageTerminated() {} };
  const session = { transcript: { open: false, close() {} }, queue: { queue: [] }, setMini() {} };
  const BrowserTab = forwardRef((props, ref) => {
    useImperativeHandle(ref, () => ({ view: { current: {} }, navigate: target => loads.push(target), inject() {} }), []);
    return createElement('BrowserTab', props);
  });
  const component = new Proxy({}, { get: (_target, name) => props => createElement(name, props) });
  const native = new Proxy({ Platform: { OS: 'android' }, AppState: { currentState: 'active' },
    Animated: { View: 'AnimatedView' }, StyleSheet: { create: value => value, absoluteFill: {}, hairlineWidth: 1 },
    useColorScheme: () => 'dark', BackHandler: { addEventListener: () => ({ remove() {} }) },
    Alert: { alert: message => assert.fail(message) },
  }, { get: (target, key) => target[key] ?? key });
  const edges = {
    'react-native': native,
    'react-native-safe-area-context': { SafeAreaProvider: ({ children }) => children, SafeAreaView: 'SafeAreaView',
      useSafeAreaInsets: () => ({ top: 0, bottom: 0, left: 0, right: 0 }) },
    'expo-status-bar': { StatusBar: 'StatusBar' }, 'react-native-webview': { default: 'WebView' },
    'expo-screen-orientation': { unlockAsync: async () => {}, lockAsync: async () => {}, OrientationLock: {} },
    'expo-navigation-bar': { NavigationBar: { setStyle() {}, setHidden() {} } },
    './modules/govue-core': { coreRequest: async command => {
      writes.push(command);
      if (command.op === 'browser_open') state = { ...state, revision: state.revision + 1,
        tabs: [{ id: 1, url: command.url, kind: 'watch' }], target: { id: 1, url: command.url, load: true } };
      return state;
    } },
    './src/browser/BrowserTab': { BrowserTab },
    './src/browser/useBrowserIdentity': { useBrowserIdentity: () => ({ ready: true }) },
    './src/browser/useBrowserLaunches': { useBrowserLaunches: callback => { navigate = callback; } },
    './src/media/useMediaBrowser': { useMediaBrowser: (_view, id) => { selected = id; return media; } },
    './src/media/downloads': { useDownloads: () => ({ entries: [] }) },
    './src/media/useTranscriptPosition': { useTranscriptPosition: () => ({ position: null }) },
    './src/player/usePlayerSession': { usePlayerSession: () => session },
    './src/player/useMiniPlayer': { useMiniPlayer: () => ({ style: {}, motionStyle: {} }) },
    './src/player/Haptics': { HapticStrength: React.createContext(0) },
    './src/theme': { colors: {}, lightColors: {}, ThemeContext: React.createContext({}) },
  };
  const result = await build({ entryPoints: ['App.tsx'], bundle: true, write: false, platform: 'node',
    format: 'cjs', jsx: 'automatic', packages: 'external', plugins: [{ name: 'platform-edges', setup(builder) {
      builder.onResolve({ filter: /^\.\/(src|modules)\// }, args =>
        /\/(address|playerLayout)$/.test(args.path) ? undefined : { path: args.path, external: true });
    } }] });
  const module = { exports: {} };
  Function('require', 'module', 'exports', result.outputFiles[0].text)(name =>
    edges[name] ?? (name.startsWith('./src/') ? component : require(name)), module, module.exports);
  let root;
  await act(() => { root = create(createElement(module.exports.default)); });
  t.after(() => act(() => root.unmount()));
  const page = () => root.root.findByType('BrowserTab').props;
  await act(() => page().onUrl(1, url(first), false));
  assert.equal(selected, first);
  await act(() => navigate(url(second)));
  assert.deepEqual(loads, [url(second)]);
  assert.equal(selected, second, 'accepted targets must not wait for WebView navigation or page readiness');
  const before = writes.length;
  await act(() => page().onUrl(1, url(first), false));
  assert.equal(selected, second, 'a late callback for the previous video must not cancel the new extraction');
  assert.equal(writes.length, before, 'the previous URL must not overwrite accepted Rust state');
  await act(() => page().onUrl(1, url(second), false));
  await act(() => page().onUrl(1, url(third), true));
  assert.equal(selected, third, 'later in-page navigation must still select its new video');
});
