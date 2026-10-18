import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import React, { createElement } from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
const source = { videoId: 'abcdefghijk', trackId: '.en', segments: [{ start: 1, duration: 2, text: 'Hello, world' }] };
const timed = [
  { text: 'Hello', word: true, start: 1, end: 1.7, bold: true },
  { text: ',', word: false }, { text: ' ', word: false },
  { text: 'world', word: true, start: 1.7, end: 3 },
];

async function harness() {
  const pending = [], seeks = [], saves = [], translations = [], alerts = [];
  const bundle = await build({ entryPoints: ['src/components/TranscriptWords.tsx'], bundle: true, write: false, platform: 'node', format: 'cjs', jsx: 'automatic', packages: 'external',
    plugins: [{ name: 'caption-core', setup(builder) {
      builder.onResolve({ filter: /\/core\/vocabulary$/ }, () => ({ path: 'caption-core', external: true }));
    } }],
  });
  const module = { exports: {} };
  Function('require', 'module', 'exports', bundle.outputFiles[0].text)(name => {
    if (name === 'caption-core') return { captionWords: (...args) => new Promise(resolve => pending.push({ args, resolve })) };
    if (name === 'react-native') return { Text: 'Text', Alert: { alert: (...args) => alerts.push(args) } };
    return require(name);
  }, module, module.exports);
  const highlight = { backgroundColor: 'white', color: 'black' };
  let props = { source, segmentIndex: 0, position: 1.5, selected: true, style: {}, activeWordStyle: highlight,
    onSeek: () => seeks.push('line'), onSeekWord: seconds => seeks.push(seconds), onSave: (...args) => saves.push(args), onTranslate: (index, word) => translations.push([index, word]) };
  let root;
  const element = () => createElement(module.exports.TranscriptWords, props);
  await act(() => { root = create(element()); });
  return { root, pending, seeks, saves, translations, alerts, highlight,
    render: changes => act(() => { props = { ...props, ...changes }; root.update(element()); }),
    reply: (index, tokens = timed, text = props.source.segments[props.segmentIndex].text) => act(async () => { pending[index].resolve({ text, tokens }); }),
    unmount: () => act(() => root.unmount()) };
}
const event = () => ({ stopPropagation() {}, nativeEvent: { pageX: 40, pageY: 80 } });
const word = (env, text) => env.root.root.findAllByType('Text').find(node => node.children[0] === text);

test('word taps seek precisely, holds translate, and saving remains an explicit action', async t => {
  const env = await harness(); t.after(env.unmount);
  await env.reply(0);
  assert.ok(word(env, 'Hello').props.style.includes(env.highlight));
  assert.ok(!word(env, 'world').props.style.includes(env.highlight));
  await env.render({ position: 1.7 });
  assert.ok(!word(env, 'Hello').props.style.includes(env.highlight));
  assert.ok(word(env, 'world').props.style.includes(env.highlight));
  await act(() => word(env, 'world').props.onPress(event()));
  assert.deepEqual(env.seeks, [1.7]);
  await act(() => word(env, 'world').props.onLongPress(event()));
  assert.deepEqual(env.translations, [[3, 'world']]);
  assert.deepEqual(env.saves, []);
  await act(() => env.root.root.findAllByType('Text')[0].props.onAccessibilityAction({ nativeEvent: { actionName: 'save-word-3' } }));
  assert.deepEqual(env.saves, [[3, 'world']]);
  assert.deepEqual(env.seeks, [1.7], 'holding a word does not seek');
  await env.render({ position: 3 });
  assert.ok(!word(env, 'world').props.style.includes(env.highlight));
  await env.render({ position: 2, selected: false });
  assert.ok(!word(env, 'world').props.style.includes(env.highlight));
});

test('untimed captions retain line seeking and timed words expose accessible play actions', async t => {
  const env = await harness(); t.after(env.unmount);
  await env.reply(0);
  const line = env.root.root.findAllByType('Text')[0];
  const action = line.props.accessibilityActions.find(action => action.name === 'play-word-3');
  assert.equal(action.label, 'Play from world');
  const translateAction = line.props.accessibilityActions.find(action => action.name === 'translate-word-3');
  assert.equal(translateAction.label, 'Translate world');
  await act(() => line.props.onAccessibilityAction({ nativeEvent: { actionName: translateAction.name } }));
  assert.deepEqual(env.translations, [[3, 'world']]);
  await act(() => line.props.onAccessibilityAction({ nativeEvent: { actionName: action.name } }));
  assert.deepEqual(env.seeks, [1.7]);
  const untimed = { ...source, trackId: '.manual' };
  await env.render({ source: untimed });
  await env.reply(1, timed.map(({ start, end, ...token }) => token));
  await act(() => word(env, 'world').props.onPress(event()));
  assert.deepEqual(env.seeks, [1.7, 'line']);
  assert.ok(!env.root.root.findAllByType('Text')[0].props.accessibilityActions.some(action => action.name.startsWith('play-word')));
});

test('late results from a previous video are ignored and ruby is accessible without changing saved tokens', async t => {
  const env = await harness(); t.after(env.unmount);
  const next = { videoId: '0123456789a', trackId: '.ja', segments: [{ start: 5, duration: 2, text: '漢字' }] };
  await env.render({ source: next, position: 5.2 });
  await env.reply(0, timed, 'Hello, world');
  assert.equal(env.root.root.findAllByType('Text').length, 1);
  await env.reply(1, [{ text: '漢', word: true, start: 5, end: 7 }, { text: '字', word: true, start: 5, end: 7, reading: 'かんじ' }]);
  assert.equal(env.root.root.findAllByType('Text')[0].props.accessibilityLabel, '漢字 (かんじ)');
  await act(() => word(env, '字').props.onLongPress(event()));
  assert.deepEqual(env.translations, [[1, '字']]);
});

test('speaker dividers announce possible changes and preserve word seeking and save indices', async t => {
  const env = await harness(); t.after(env.unmount);
  const dividers = () => env.root.root.findAllByType('Text').filter(node => typeof node.children[0] === 'string' && node.children[0].includes('───'));
  await env.reply(0, timed.map(token => token.word ? { ...token, speakerChange: true } : token));
  assert.deepEqual(dividers().map(node => node.children[0]), ['───\n', '\n───\n']);
  assert.ok(dividers().every(node => node.props.accessible === false));
  const line = env.root.root.findAllByType('Text')[0];
  assert.equal(line.props.accessibilityLabel.trim(), 'Possible speaker change. Hello,  Possible speaker change. world');
  assert.ok(!line.props.accessibilityLabel.includes('───'));
  await act(() => word(env, 'world').props.onPress(event()));
  await act(() => word(env, 'world').props.onLongPress(event()));
  assert.deepEqual(env.seeks, [1.7]);
  assert.deepEqual(env.translations, [[3, 'world']]);
  await act(() => line.props.onAccessibilityAction({ nativeEvent: { actionName: 'save-word-3' } }));
  assert.deepEqual(env.saves, [[3, 'world']]);
  await env.render({ source: { ...source, trackId: '.unflagged' } });
  await env.reply(1, timed.map(token => ({ ...token, speakerChange: false })));
  assert.equal(dividers().length, 0);
  assert.equal(env.root.root.findAllByType('Text')[0].props.accessibilityLabel, 'Hello, world');
});
