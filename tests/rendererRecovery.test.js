import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import React, { createElement, createRef, forwardRef, useImperativeHandle, useRef } from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';
import { JSDOM } from 'jsdom';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
const videoId = 'abcdefghijk';
const url = `https://m.youtube.com/watch?v=${videoId}&t=12s`;
const transcript = { videoId, title: 'Saved captions', trackId: 'en', language: 'en', tracks: [], segments: [{ start: 12, duration: 2, text: 'Still available' }] };

// Run the real React components and hooks; replace only native/platform edges.
async function harness(entry, os = 'android') {
  const WebView = forwardRef((props, ref) => {
    const handle = useRef(null);
    if (!handle.current) handle.current = {
      injections: [], reloads: 0,
      injectJavaScript(code) { this.injections.push(code); this.execute?.(code); },
      reload() { this.reloads++; },
    };
    useImperativeHandle(ref, () => handle.current);
    return createElement('WebView', { ...props, handle: handle.current, attach: ref });
  });
  const media = {
    player: { status: 'readyToPlay' }, playback: { videoId, title: 'Playing video' },
    settings: {}, speeds: [], qualities: [], configureCalls: 0, terminated: 0,
    configurePage() { this.configureCalls++; },
    pageTerminated() { this.terminated++; },
  };
  const native = new Proxy({
    Platform: { OS: os }, StyleSheet: { create: value => value, hairlineWidth: 1 },
    Keyboard: { dismiss() {} }, BackHandler: { addEventListener: () => ({ remove() {} }) },
  }, { get: (target, key) => target[key] ?? key });
  const edges = {
    'react-native': native,
    'react-native-webview': WebView,
    'react-native-safe-area-context': { SafeAreaProvider: ({ children }) => children, SafeAreaView: 'SafeAreaView', useSafeAreaInsets: () => ({ top: 0 }) },
    'expo-status-bar': { StatusBar: () => null },
    expo: { useEvent: (_target, _event, initial) => initial, requireNativeModule: () => ({}) },
    'expo-video': { VideoView: 'VideoView' },
    'lucide-react-native': new Proxy({}, { get: (_target, key) => key }),
    'test-components': new Proxy({}, { get: (_target, key) => props => createElement(key, props) }),
    'test-media': { useMediaBrowser: () => media },
    'test-transcripts': { restoreTranscript: async () => transcript },
    'test-downloads': { useDownloads: () => ({}) },
    'test-position': { useTranscriptPosition: () => ({ position: 12 }) },
    'test-decoder': { PLAYER_DECODER: '' },
  };
  const result = await build({
    entryPoints: [entry], bundle: true, write: false, platform: 'node', format: 'cjs', jsx: 'automatic', packages: 'external',
    plugins: [{ name: 'native-test-edges', setup(builder) {
      builder.onResolve({ filter: /\/components\// }, () => ({ path: 'test-components', external: true }));
      for (const [path, name] of [['/useMediaBrowser', 'media'], ['/core/transcripts', 'transcripts'], ['/downloads', 'downloads'], ['/useTranscriptPosition', 'position'], ['/generated/playerDecoder', 'decoder']]) {
        builder.onResolve({ filter: new RegExp(`${path}(?:\\.[jt]s)?$`) }, () => ({ path: `test-${name}`, external: true }));
      }
    } }],
  });
  const module = { exports: {} };
  Function('require', 'module', 'exports', result.outputFiles[0].text)(name => name in edges ? edges[name] : require(name), module, module.exports);
  return { ...module.exports, media };
}

function bootstrap(props) {
  const dom = new JSDOM(props.source.html, { runScripts: 'outside-only' });
  dom.window.ReactNativeWebView = { postMessage: data => props.onMessage({ nativeEvent: { data, url: 'about:blank' } }) };
  props.handle.execute = code => dom.window.eval(code);
  dom.window.eval(dom.window.document.querySelector('script').textContent);
  return dom;
}

test('Android runtime rejects interrupted jobs, waits for its replacement, and ignores old renderer events', async t => {
  const { PlayerRuntime } = await harness('src/media/PlayerRuntime.tsx');
  const ref = createRef();
  let root;
  await act(() => { root = create(createElement(PlayerRuntime, { ref })); });
  t.after(async () => { await act(() => root.unmount()); });
  const old = root.root.findByType('WebView').props;
  let interrupted;
  await act(() => { interrupted = ref.current.evaluate('return {n:"old"}'); });
  assert.equal(old.handle.injections.length, 0, 'work waits for the bootstrap handshake');
  const rejected = assert.rejects(interrupted, /runtime stopped/);
  await act(() => old.onRenderProcessGone());
  await rejected;
  assert.equal(root.root.findAllByType('WebView').length, 0, 'dead native view is disposed immediately');

  let result;
  await act(() => { result = ref.current.evaluate('return {n:"new",sig:"signature",token:"discarded"}'); });
  const fresh = root.root.findByType('WebView').props;
  assert.notEqual(fresh.handle, old.handle);
  let stale;
  await act(() => {
    stale = bootstrap(old);
    old.onRenderProcessGone();
    old.attach(null);
  });
  assert.equal(fresh.handle.injections.length, 0, 'stale ready messages cannot start replacement jobs');
  let dom;
  await act(() => { dom = bootstrap(fresh); });
  assert.deepEqual(await result, { n: 'new', sig: 'signature' });
  assert.equal(fresh.handle.injections.length, 1, 'interrupted jobs are not replayed');
  stale.window.close();
  dom.window.close();
});

test('iOS runtime reloads its existing view and waits for a fresh bootstrap', async t => {
  const { PlayerRuntime } = await harness('src/media/PlayerRuntime.tsx', 'ios');
  const ref = createRef();
  let root;
  await act(() => { root = create(createElement(PlayerRuntime, { ref })); });
  t.after(async () => { await act(() => root.unmount()); });
  const old = root.root.findByType('WebView').props;
  await act(() => old.onContentProcessDidTerminate());
  let result;
  await act(() => { result = ref.current.evaluate('return {sig:"recovered"}'); });
  const fresh = root.root.findByType('WebView').props;
  assert.equal(fresh.handle, old.handle);
  assert.notEqual(fresh.source, old.source, 'a fresh document is loaded into the same native view');
  let stale;
  await act(() => { stale = bootstrap(old); });
  assert.equal(fresh.handle.injections.length, 0);
  let dom;
  await act(() => { dom = bootstrap(fresh); });
  assert.deepEqual(await result, { sig: 'recovered' });
  stale.window.close();
  dom.window.close();
});

for (const os of ['android', 'ios']) test(`${os} browser recovers the current page while retaining its transcript and native player`, async t => {
  const { default: App, media } = await harness('App.tsx', os);
  let root;
  await act(() => { root = create(createElement(App)); });
  t.after(async () => { await act(() => root.unmount()); });
  await act(() => root.root.findByType('StartPage').props.onNavigate('https://example.com/'));
  const initial = root.root.findAllByType('WebView').find(node => node.props.source.uri).props;
  await act(() => {
    initial.onNavigationStateChange({ url, canGoBack: true, canGoForward: false });
    initial.onMessage({ nativeEvent: { url, data: JSON.stringify({ type: 'page', url, videoId, userAgent: 'test browser' }) } });
  });
  const extract = root.root.findAllByType('Pressable').find(node => node.props.accessibilityLabel === 'Extract YouTube transcript');
  await act(() => extract.props.onPress());
  const reader = root.root.findByType('TranscriptPanel');
  assert.equal(reader.props.data, transcript);
  const player = root.root.findByType('VideoView');
  await act(() => reader.props.onExtract()); // Start an extraction that must be cancelled.
  const old = root.root.findAllByType('WebView').find(node => node.props.source.uri).props;
  await act(() => os === 'android' ? old.onRenderProcessGone() : old.onContentProcessDidTerminate());
  assert.equal(media.terminated, 1);
  assert.equal(root.root.findByType('TranscriptPanel').props.data, transcript);
  assert.equal(root.root.findByType('TranscriptPanel').props.loading, false);
  assert.equal(root.root.findByType('VideoView'), player);
  if (os === 'android') assert.equal(root.root.findAllByType('WebView').filter(node => node.props.source.uri).length, 0);

  const reload = root.root.findAllByType('IconButton').find(node => node.props.label === 'Reload page');
  await act(() => reload.props.onPress());
  const fresh = root.root.findAllByType('WebView').find(node => node.props.source.uri).props;
  if (os === 'android') {
    assert.notEqual(fresh.handle, old.handle);
    assert.equal(fresh.source.uri, url, 'recovery uses the current URL, including the video ID, rather than initial source');
    await act(() => {
      old.onNavigationStateChange({ url: 'https://example.org/', canGoBack: false, canGoForward: false });
      old.onLoadEnd();
      old.onRenderProcessGone();
    });
    assert.equal(media.configureCalls, 0);
    assert.equal(media.terminated, 1);
  } else {
    assert.equal(fresh.handle, old.handle);
    assert.equal(fresh.handle.reloads, 1);
  }
  assert.equal(root.root.findByType('TranscriptPanel').props.data, transcript);
  assert.equal(root.root.findByType('VideoView'), player);
});
