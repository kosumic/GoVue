import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createElement, forwardRef, useImperativeHandle } from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
const preview = { label: 'Shared connection', kind: 'openai', baseUrl: 'https://example.com/v1', protocol: 'chat', oauth: false, model: 'first', modelCount: 2 };

async function harness(options = {}) {
  const calls = [], imports = [], imported = [], files = [], shares = [], appListeners = new Set();
  let permissionRequests = 0;
  const permission = { granted: !options.denied, canAskAgain: !options.denied };
  const requestPermission = async () => { permissionRequests++; return permission; };
  const getPermission = async () => permission;
  const ai = {
    providers: [{ id: 'source', label: preview.label, model: 'first', models: [{ id: 'first' }, { id: 'second' }] }], error: '',
    async mutate(command) {
      imports.push(command);
      return { providers: [{ id: 'new-connection', label: preview.label }] };
    },
  };
  const core = { async aiRequest(command) {
    calls.push(command);
    if (command.op === 'ai_provider_qr') return { provider: preview, payload: 'govue:provider:1:configuration', modules: Array.from({ length: 21 }, () => '1'.repeat(21)) };
    if (command.payload === 'bad') throw Error('This QR code does not contain a supported GoVue provider.');
    return options.previewGate || preview;
  } };
  const native = {
    ActivityIndicator: 'ActivityIndicator', Modal: 'Modal', Pressable: 'Pressable', ScrollView: 'ScrollView', Switch: 'Switch', Text: 'Text', View: 'View',
    Linking: { async openSettings() {} }, useWindowDimensions: () => ({ width: 390, height: 844 }),
    StyleSheet: { create: value => value, absoluteFill: {} },
    AppState: { currentState: 'active', addEventListener(_event, callback) { appListeners.add(callback); return { remove: () => appListeners.delete(callback) }; } },
  };
  const result = await build({ entryPoints: ['src/components/ProviderQr.tsx'], bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external', jsx: 'automatic',
    plugins: [{ name: 'qr-boundary', setup(builder) {
      builder.onResolve({ filter: /modules\/govue-core$/ }, () => ({ path: 'mock-core', external: true }));
    } }],
  });
  const Svg = forwardRef(function Svg(props, ref) {
    useImperativeHandle(ref, () => ({ toDataURL(callback) { callback('native-png-base64'); } }), []);
    return createElement('Svg', props);
  });
  const module = { exports: {} };
  Function('require', 'module', 'exports', result.outputFiles[0].text)(name => {
    if (name === 'mock-core') return core;
    if (name === 'react-native') return native;
    if (name === 'react-native-safe-area-context') return { SafeAreaView: 'SafeAreaView' };
    if (name === 'expo-camera') return { CameraView: 'CameraView', useCameraPermissions: () => [permission, requestPermission, getPermission], scanFromURLAsync: async () => [{ data: 'bad' }, { data: 'good' }] };
    if (name === 'expo-image-picker') return { launchImageLibraryAsync: async () => ({ canceled: false, assets: [{ uri: 'file:///picked-qr.png' }] }) };
    if (name === 'expo-file-system/legacy') return { cacheDirectory: 'file:///cache/', EncodingType: { Base64: 'base64' }, async writeAsStringAsync(path, data, options) { files.push({ path, data, options }); } };
    if (name === 'expo-sharing') return { isAvailableAsync: async () => true, async shareAsync(path, options) { shares.push({ path, options }); } };
    if (name === 'lucide-react-native') return Object.fromEntries(['Flashlight', 'Image', 'ScanLine', 'Share2', 'X'].map(key => [key, key]));
    if (name === 'react-native-svg') return Object.assign(Svg, Object.fromEntries(['Defs', 'G', 'LinearGradient', 'Path', 'Rect', 'Stop', 'Text'].map(key => [key, `Svg${key}`])));
    return require(name);
  }, module, module.exports);
  let root;
  await act(async () => { root = create(createElement(module.exports.ProviderQr, { ai, providerId: options.sharing ? 'source' : undefined, onClose() {}, onImported: p => imported.push(p) })); });
  const button = title => root.root.findAllByType('Pressable').find(node => node.findAllByType('Text').some(text => text.props.children === title));
  return { root, calls, imports, imported, files, shares, permissionRequests: () => permissionRequests,
    button, press: title => act(async () => { const target = button(title); assert.ok(target, `Missing button: ${title}`); await target.props.onPress(); }),
    state: value => act(async () => { for (const listener of appListeners) listener(value); }),
    unmount: () => act(() => root.unmount()),
  };
}

test('scans pause in background, handle one event, and import only after confirmation', async t => {
  let finishPreview;
  const previewGate = new Promise(resolve => { finishPreview = resolve; });
  const env = await harness({ previewGate });
  t.after(env.unmount);
  const scan = env.root.root.findByType('CameraView').props.onBarcodeScanned;
  await env.state('background');
  assert.equal(env.root.root.findAllByType('CameraView').length, 0);
  await act(async () => { scan({ data: 'good' }); });
  assert.equal(env.calls.length, 0);
  await env.state('active');
  await act(async () => { scan({ data: 'good' }); scan({ data: 'good' }); });
  assert.equal(env.calls.length, 1, 'repeated camera frames cannot queue duplicate previews');
  assert.equal(env.imports.length, 0);
  await act(async () => { finishPreview(preview); });
  assert.equal(env.root.root.findAllByType('CameraView').length, 0);
  assert.ok(env.button('Import provider'));
  assert.equal(env.imports.length, 0, 'a scan must never silently import');
  await env.press('Import provider');
  assert.deepEqual(env.imports, [{ op: 'ai_import_provider_qr', payload: 'good' }]);
  assert.equal(env.imported[0].id, 'new-connection');
});

test('invalid scans can retry; a QR image works when camera access is denied', async t => {
  const env = await harness();
  t.after(env.unmount);
  await act(async () => { env.root.root.findByType('CameraView').props.onBarcodeScanned({ data: 'bad' }); });
  assert.equal(env.imports.length, 0);
  assert.ok(env.root.root.findByProps({ accessibilityRole: 'alert' }));
  await env.press('Scan again');
  assert.equal(env.root.root.findAllByType('CameraView').length, 1);

  const denied = await harness({ denied: true });
  t.after(denied.unmount);
  assert.equal(denied.root.root.findAllByType('CameraView').length, 0);
  assert.ok(denied.button('Open Settings'));
  await act(async () => { denied.root.root.findByProps({ accessibilityLabel: 'Choose provider QR image' }).props.onPress(); });
  assert.deepEqual(denied.calls.map(c => c.payload), ['bad', 'good'], 'an unrelated QR cannot hide a valid code in the same image');
  assert.equal(denied.imports.length, 0);
  await denied.press('Import provider');
  assert.equal(denied.imported.length, 1);
});

test('sharing exports a rendered PNG and can limit a large library without camera access', async t => {
  const env = await harness({ sharing: true });
  t.after(env.unmount);
  assert.equal(env.permissionRequests(), 0, 'showing a QR must not ask for camera permission');
  assert.equal(env.calls[0].op, 'ai_provider_qr');
  await act(async () => { env.root.root.findByType('Switch').props.onValueChange(true); });
  assert.equal(env.calls.at(-1).selectedModelOnly, true);
  await env.press('Share QR image');
  assert.equal(env.files.length, 1);
  assert.equal(env.files[0].data, 'native-png-base64');
  assert.equal(env.files[0].options.encoding, 'base64');
  assert.equal(env.shares[0].path, env.files[0].path);
  assert.equal(env.shares[0].options.mimeType, 'image/png');
  assert.equal(env.imports.length, 0);
});

test('Android prebuild preserves scanner visibility and reflection targets without replacing existing settings', async t => {
  const { compileModsAsync, AndroidConfig } = require('@expo/config-plugins');
  const withProviderQr = require('../plugins/withProviderQr.cjs');
  const root = await mkdtemp(join(tmpdir(), 'govue-qr-manifest-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const folder = join(root, 'android/app/src/main');
  const path = join(folder, 'AndroidManifest.xml');
  const proguard = join(root, 'android/app/proguard-rules.pro');
  await mkdir(folder, { recursive: true });
  await writeFile(proguard, '# existing application rules\n');
  await writeFile(path, '<manifest xmlns:android="http://schemas.android.com/apk/res/android"><queries><intent><action android:name="android.intent.action.VIEW"/><data android:scheme="https"/></intent></queries><queries><package android:name="existing.package"/></queries><application/></manifest>');
  for (let i = 0; i < 2; i++) {
    await compileModsAsync(withProviderQr({ name: 'GoVue', slug: 'govue', android: { package: 'io.github.kaminarios.govue' } }), { projectRoot: root, platforms: ['android'] });
  }
  const { manifest } = await AndroidConfig.Manifest.readAndroidManifestAsync(path);
  const packages = manifest.queries.flatMap(query => query.package || []).map(pkg => pkg.$['android:name']);
  assert.deepEqual(packages.sort(), ['com.android.vending', 'existing.package']);
  assert.equal(manifest.queries[0].intent[0].data[0].$['android:scheme'], 'https');
  const rules = await readFile(proguard, 'utf8');
  assert.ok(rules.startsWith('# existing application rules'));
  assert.equal(rules.split('-keep class com.google.mlkit.vision.barcode.BarcodeScanning').length, 2);
  assert.equal(rules.split('-keep class com.google.mlkit.** implements com.google.firebase.components.ComponentRegistrar { public <init>(); }').length, 2);
});
