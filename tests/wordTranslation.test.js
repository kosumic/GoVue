import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { createElement } from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
const source = { videoId: 'abcdefghijk', trackId: '.en', segments: [{ text: 'Hello world.' }] };
const selection = { source, segmentIndex: 0, tokenIndex: 0, word: 'Hello' };
const plan = (targetLangCode, targetLanguage, input = 'Hello') => ({ input, sourceLangCode: 'en', targetLangCode, targetLanguage, sameLanguage: false });

async function harness() {
  const plans = [], native = [], saves = [];
  const deferred = (list, args) => new Promise((resolve, reject) => list.push({ args, resolve, reject }));
  const bundle = await build({ entryPoints: ['src/components/WordTranslationSheet.tsx'], bundle: true, write: false, platform: 'node', format: 'cjs', jsx: 'automatic', packages: 'external',
    plugins: [{ name: 'translation-platform', setup(builder) {
      builder.onResolve({ filter: /\/core\/vocabulary$/ }, () => ({ path: 'test-core', external: true }));
      builder.onResolve({ filter: /\/theme$/ }, () => ({ path: 'test-theme', external: true }));
      builder.onResolve({ filter: /\/IconButton$/ }, () => ({ path: 'test-button', external: true }));
      builder.onResolve({ filter: /\/player\/Haptics$/ }, () => ({ path: 'test-haptics', external: true }));
      builder.onResolve({ filter: /\/ai\/use(GameAudio|WordAssistance|ExampleSpeech)$/ }, () => ({ path: 'test-word-tools', external: true }));
      builder.onResolve({ filter: /\/ai\/useAi$/ }, () => ({ path: 'test-ai', external: true }));
      builder.onResolve({ filter: /\/WordAiProviderPicker$/ }, () => ({ path: 'test-ai-picker', external: true }));
      builder.onResolve({ filter: /^test-native$/ }, () => ({ path: 'test-native', external: true }));
      builder.onResolve({ filter: /^expo-translate-text$/ }, () => ({ path: 'native', namespace: 'test-native' }));
      builder.onLoad({ filter: /.*/, namespace: 'test-native' }, () => ({ contents: 'export const onTranslateTask = params => require("test-native").translate(params);', loader: 'js' }));
    } }],
  });
  const module = { exports: {} };
  Function('require', 'module', 'exports', bundle.outputFiles[0].text)(name => {
    if (name === 'test-core') return { planWordTranslation: (...args) => deferred(plans, args), saveWord: (...args) => deferred(saves, args), wordAudioTiming: async () => null };
    if (name === 'test-word-tools') return { useWordAudio: () => ({ phase: 'idle', error: '', play() {}, stop() {} }), useWordAssistance: () => ({ result: null, error: '', busy: false, generate() {} }), useExampleSpeech: () => ({ active: null, error: '', speak() {}, stop() {} }) };
    if (name === 'test-ai') return { useAi: () => ({ ready: true, providers: [], groups: [], activeId: null }) };
    if (name === 'test-ai-picker') return { WordAiProviderPicker: 'WordAiProviderPicker' };
    if (name === 'test-native') return { translate: (...args) => deferred(native, args) };
    if (name === 'test-theme') return { useThemeColors: () => ({}), useThemeStyles: factory => factory({}) };
    if (name === 'test-button') return { IconButton: 'IconButton' };
    if (name === 'test-haptics') return { useHaptic: () => () => {} };
    if (name === '@react-native-clipboard/clipboard') return { setString() {} };
    if (name === 'lucide-react-native') return Object.fromEntries(['X', 'RotateCw', 'BookmarkPlus', 'BookOpen', 'Check', 'Copy', 'Sparkles', 'Square', 'Volume2'].map(name => [name, name]));
    if (name === 'react-native-safe-area-context') return { useSafeAreaInsets: () => ({ top: 0, right: 0, bottom: 0, left: 0 }) };
    if (name === 'react-native') return { ...Object.fromEntries(['Text', 'View', 'Modal', 'Pressable', 'ScrollView', 'ActivityIndicator'].map(name => [name, name])), useWindowDimensions: () => ({ width: 400, height: 800, scale: 1, fontScale: 1 }), StyleSheet: { create: value => value, absoluteFill: {}, hairlineWidth: 1 } };
    return require(name);
  }, module, module.exports);
  let props = { selection, onClose() {} }, root;
  const element = () => createElement(module.exports.WordTranslationSheet, props);
  await act(() => { root = create(element()); });
  return { root, plans, native, saves,
    render: changes => act(() => { props = { ...props, ...changes }; root.update(element()); }),
    reply: (item, value) => act(async () => item.resolve(value)),
    fail: (item, error) => act(async () => item.reject(error)),
    press: label => act(() => root.root.findAll(node => node.props.accessibilityLabel === label || node.props.label === label)[0].props.onPress()),
    text: () => root.root.findAllByType('Text').map(node => node.children.filter(child => typeof child === 'string').join('')).join('\n'),
    unmount: () => act(() => root.unmount()) };
}

test('every learning-language result appears independently and native options never contain undefined', async t => {
  const env = await harness(); t.after(env.unmount);
  await env.reply(env.plans[0], [plan('es', 'Spanish'), plan('de', 'German')]);
  assert.match(env.text(), /Spanish/); assert.match(env.text(), /German/);
  assert.equal(env.native.length, 1, 'native translation sessions run sequentially');
  const params = env.native[0].args[0];
  for (const key of ['input', 'sourceLangCode', 'targetLangCode', 'preferredStrategy', 'requiresWifi', 'requireCharging']) assert.notEqual(params[key], undefined, `${key} must cross the Kotlin boundary with a value`);
  await env.reply(env.native[0], { translatedTexts: 'Hola' });
  assert.match(env.text(), /Hola/);
  assert.equal(env.native[1].args[0].targetLangCode, 'de');
  await env.reply(env.native[1], { translatedTexts: 'Hallo' });
  assert.match(env.text(), /Hola/); assert.match(env.text(), /Hallo/);
  assert.equal(env.saves.length, 0);
  await env.press('Save word');
  assert.deepEqual(env.saves[0].args, [source, 0, 0, 'Hello', undefined]);
  await env.reply(env.saves[0], { saved: true });
  assert.match(env.text(), /Word saved/);
});

test('changing words discards late results and skips remaining old-language requests', async t => {
  const env = await harness(); t.after(env.unmount);
  await env.reply(env.plans[0], [plan('es', 'Spanish'), plan('de', 'German')]);
  const next = { ...selection, tokenIndex: 2, word: 'world' };
  await env.render({ selection: next });
  await env.reply(env.plans[1], [plan('es', 'Spanish', 'world')]);
  assert.equal(env.native.length, 1);
  await env.reply(env.native[0], { translatedTexts: 'Stale greeting' });
  assert.doesNotMatch(env.text(), /Stale greeting/);
  assert.equal(env.native.length, 2, 'the old German translation was cancelled before entering native');
  assert.equal(env.native[1].args[0].input, 'world');
  await env.reply(env.native[1], { translatedTexts: 'mundo' });
  assert.match(env.text(), /mundo/);
  await env.press('Save word');
  await env.render({ selection });
  await env.reply(env.saves[0], { saved: true });
  assert.doesNotMatch(env.text(), /Word saved/);
});

test('one unavailable language does not hide other translations, and retry recovers', async t => {
  const env = await harness(); t.after(env.unmount);
  await env.reply(env.plans[0], [plan('es', 'Spanish'), plan('de', 'German')]);
  await env.fail(env.native[0], Object.assign(Error('download failed'), { code: 'MODEL_DOWNLOAD_FAILED' }));
  assert.match(env.text(), /Check your connection/);
  await env.reply(env.native[1], { translatedTexts: 'Hallo' });
  assert.match(env.text(), /Hallo/);
  await env.press('Retry word translation');
  await env.reply(env.plans[1], [plan('es', 'Spanish')]);
  await env.reply(env.native[2], { translatedTexts: 'Hola' });
  assert.match(env.text(), /Hola/);
  assert.doesNotMatch(env.text(), /Check your connection/);
});

test('dismissed planning never invokes native translation and same-language fallback needs no model', async t => {
  const env = await harness();
  await env.unmount();
  await env.reply(env.plans[0], [plan('es', 'Spanish')]);
  assert.equal(env.native.length, 0);
  const same = await harness(); t.after(same.unmount);
  await same.reply(same.plans[0], [{ ...plan('en', 'English'), sameLanguage: true }]);
  assert.equal(same.native.length, 0);
  assert.match(same.text(), /Already in English/);
});

test('dragging keeps the card nonmodal, drops old translations, and saves the updated phrase', async t => {
  const env = await harness(); t.after(env.unmount);
  assert.equal(env.root.root.findAllByType('Modal').length, 0, 'the source handles remain in the active window');
  assert.ok(env.root.root.findAllByType('View').some(node => node.props.pointerEvents === 'box-none'));
  await env.reply(env.plans[0], [plan('es', 'Spanish'), plan('de', 'German')]);
  await env.render({ pending: true });
  assert.equal(env.root.root.findByProps({ accessibilityLabel: 'Save word' }).props.disabled, true);
  const next = { ...selection, word: 'Hello world', endTokenIndex: 2 };
  await env.render({ selection: next });
  assert.equal(env.plans.length, 1, 'pending handle movement does not translate an outdated range');
  await env.reply(env.native[0], { translatedTexts: 'Stale greeting' });
  assert.doesNotMatch(env.text(), /Stale greeting/);
  assert.equal(env.native.length, 1, 'remaining translations for the old word were cancelled');
  await env.render({ pending: false });
  assert.deepEqual(env.plans[1].args[0], next);
  await env.reply(env.plans[1], [plan('es', 'Spanish', 'Hello world')]);
  assert.equal(env.native[1].args[0].input, 'Hello world');
  await env.reply(env.native[1], { translatedTexts: 'Hola mundo' });
  assert.match(env.text(), /Hola mundo/);
  await env.press('Save phrase');
  assert.deepEqual(env.saves[0].args, [source, 0, 0, 'Hello world', 2]);
  await env.reply(env.saves[0], { saved: true });
  assert.match(env.text(), /Phrase saved/);
});
