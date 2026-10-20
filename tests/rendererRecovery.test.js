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
  const externalLinks = [], policyCalls = [];
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
    expo: { useEvent: (_target, _event, initial) => initial, requireNativeModule: () => ({
      browserUrlPolicy: url => {
        policyCalls.push(url);
        return new Map([
          ['https://example.com/', 'internal'],
          ['http://example.com/', 'external'],
          ['https://example.com:8443/', 'external'],
        ]).get(url) ?? (url.startsWith('https://m.youtube.com/') ? 'internal' : 'blocked');
      },
    }) },
    'expo-video': { VideoView: 'VideoView' },
    'expo-web-browser': { openBrowserAsync: async url => { externalLinks.push(url); } },
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
  return { ...module.exports, media, externalLinks, policyCalls };
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

for (const os of ['android', 'ios']) test(`${os} watch tab recovers its current document and rejects dead-renderer events`, async t => {
  const { BrowserTab } = await harness('src/browser/BrowserTab.tsx', os);
  const ref = createRef();
  const messages = [], pages = [], deaths = [];
  const props = { tab: { id: 2, kind: 'watch', url: 'https://m.youtube.com/' }, active: true,
    width: 400, height: 800, settings: {},
    onMessage: (...args) => messages.push(args), onUrl: (...args) => pages.push(args), onDead: id => deaths.push(id) };
  let root;
  await act(() => { root = create(createElement(BrowserTab, { ...props, ref })); });
  t.after(async () => { await act(() => root.unmount()); });
  const initial = root.root.findByType('WebView').props;
  await act(() => initial.onNavigationStateChange({ url, canGoBack: true }));
  assert.equal(root.root.findByType('WebView').props.source, initial.source, 'SPA navigation keeps the initial source stable');
  await act(() => root.update(createElement(BrowserTab, { ...props, ref, active: false })));
  assert.equal(root.root.findByType('WebView').props.handle, initial.handle, 'parking the tab preserves its native WebView');
  await act(() => root.update(createElement(BrowserTab, { ...props, ref })));
  const old = root.root.findByType('WebView').props;
  await act(() => os === 'android' ? old.onRenderProcessGone() : old.onContentProcessDidTerminate());
  assert.deepEqual(deaths, [2]);
  if (os === 'android') assert.equal(root.root.findAllByType('WebView').length, 0);
  const before = { pages: pages.length, messages: messages.length };
  await act(() => {
    old.onMessage({ nativeEvent: { url, data: '{}' } });
    old.onNavigationStateChange({ url: 'https://m.youtube.com/watch?v=stalevideo1', canGoBack: false });
    old.onLoadEnd();
  });
  assert.equal(pages.length, before.pages);
  assert.equal(messages.length, before.messages);
  await act(() => root.root.findAllByType('Pressable').find(node => node.props.accessibilityLabel === 'Reload page').props.onPress());
  const fresh = root.root.findByType('WebView').props;
  if (os === 'android') {
    assert.notEqual(fresh.handle, old.handle);
    assert.equal(fresh.source.uri, url, 'recovery opens the last validated document');
    await act(() => old.onMessage({ nativeEvent: { url, data: '{}' } }));
    assert.equal(messages.length, before.messages);
  } else {
    assert.equal(fresh.handle, old.handle);
    assert.equal(fresh.handle.reloads, 1);
  }
  await act(() => fresh.onMessage({ nativeEvent: { url, data: '{}' } }));
  assert.equal(messages.length, before.messages + 1, 'the recovered document delivers messages');
});

test('WebView navigation uses native Rust policy and hands unsupported web destinations to the browser', async t => {
  const { BrowserTab, externalLinks, policyCalls } = await harness('src/browser/BrowserTab.tsx');
  const ref = createRef(), pages = [];
  let root;
  await act(() => { root = create(createElement(BrowserTab, { ref,
    tab: { id: 1, url: 'https://m.youtube.com/', kind: 'home' }, active: true,
    width: 400, height: 800, settings: {}, onMessage() {}, onDead() {},
    onUrl: (...args) => pages.push(args),
  })); });
  t.after(() => act(() => root.unmount()));
  const web = root.root.findByType('WebView').props;
  assert.deepEqual(web.originWhitelist, ['*'], 'all decisions reach the shared policy');
  assert.equal(web.onShouldStartLoadWithRequest({ url: 'https://example.com/' }), true);
  assert.equal(web.onShouldStartLoadWithRequest({ url: 'http://example.com/' }), false);
  assert.equal(web.onShouldStartLoadWithRequest({ url: 'https://example.com:8443/' }), false);
  for (const url of ['javascript:alert(1)', 'file:///etc/passwd', 'https://user:pass@example.com/'])
    assert.equal(web.onShouldStartLoadWithRequest({ url }), false);
  assert.equal(web.onShouldStartLoadWithRequest({ url: 'about:blank' }), true);
  assert.equal(web.onShouldStartLoadWithRequest({ url: 'http://example.com/', isTopFrame: false }), false);
  assert.deepEqual(externalLinks, ['http://example.com/', 'https://example.com:8443/']);
  await act(() => {
    web.onNavigationStateChange({ url: 'http://example.com/', canGoBack: true });
    ref.current.navigate('https://example.com:8443/');
  });
  assert.deepEqual(pages, [], 'rejected URLs cannot update or replace the current document');
  assert.equal(root.root.findByType('WebView').props.source, web.source);
  assert.ok(policyCalls.includes('https://example.com:8443/'));
});
