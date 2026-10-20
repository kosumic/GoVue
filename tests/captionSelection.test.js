import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { createElement } from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
const initial = { source: { videoId: 'abcdefghijk', trackId: '.en', segments: [{ text: 'Hello world' }] }, segmentIndex: 0, tokenIndex: 0, word: 'Hello' };
const hello = { word: 'Hello', tokenIndex: 0, endTokenIndex: 0, selectionStart: 0, selectionEnd: 5 };
const phrase = { word: 'Hello world', tokenIndex: 0, endTokenIndex: 2, selectionStart: 0, selectionEnd: 11 };
const world = { word: 'world', tokenIndex: 2, endTokenIndex: 2, selectionStart: 6, selectionEnd: 11 };

async function harness(t) {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const requests = [], changes = [], pending = [];
  const bundle = await build({ entryPoints: ['src/components/CaptionSelection.tsx'], bundle: true, write: false,
    platform: 'node', format: 'cjs', jsx: 'automatic', packages: 'external',
    plugins: [{ name: 'selection-boundaries', setup(builder) {
      builder.onResolve({ filter: /\/core\/vocabulary$/ }, () => ({ path: 'test-core', external: true }));
      builder.onResolve({ filter: /\/govue-core\/CaptionSelection$/ }, () => ({ path: 'test-native', external: true }));
      builder.onResolve({ filter: /\/theme$/ }, () => ({ path: 'test-theme', external: true }));
    } }],
  });
  const module = { exports: {} };
  Function('require', 'module', 'exports', bundle.outputFiles[0].text)(name => {
    if (name === 'test-core') return { captionSelection: (...args) => new Promise(resolve => requests.push({ args, resolve })) };
    if (name === 'test-native') return { NativeCaptionSelection: 'NativeCaptionSelection' };
    if (name === 'test-theme') return { useThemeColors: () => ({}) };
    if (name === 'react-native') return { View: 'View', Text: 'Text', ActivityIndicator: 'ActivityIndicator', useWindowDimensions: () => ({ fontScale: 1 }) };
    return require(name);
  }, module, module.exports);
  let root;
  await act(() => { root = create(createElement(module.exports.CaptionSelection, { initial, initialHeight: 24,
    onChange: selection => changes.push(selection), onPending: value => pending.push(value) })); });
  const unmount = () => act(() => { if (root) { root.unmount(); root = null; } });
  t.after(unmount);
  return { requests, changes, pending, unmount,
    reply: (index, value) => act(async () => requests[index].resolve(value)),
    tick: ms => act(async () => { t.mock.timers.tick(ms); }),
    move: (selectionStart, selectionEnd) => act(() => root.root.findByType('NativeCaptionSelection').props.onSelectionChange({ nativeEvent: { selectionStart, selectionEnd } })),
    text: () => root.root.findAllByType('Text').map(node => node.children.join('')).join('\n'),
  };
}

test('a continuous drag updates before movement stops and discards out-of-order range replies', async t => {
  const env = await harness(t);
  await env.reply(0, hello);
  await env.move(0, 8); await env.tick(30);
  await env.move(0, 10); await env.tick(30);
  assert.equal(env.requests.length, 2, 'continuous movement does not postpone the range lookup');
  assert.deepEqual(env.requests[1].args[1], { selectionStart: 0, selectionEnd: 10 });
  await env.reply(1, phrase);
  assert.equal(env.changes.at(-1).word, 'Hello world');
  await env.move(6, 11); await env.tick(60);
  await env.move(7, 11); await env.tick(60);
  await env.reply(3, world);
  await env.reply(2, phrase);
  assert.equal(env.changes.at(-1).word, 'world');
  assert.equal(env.pending.at(-1), false);
});

test('collapsed and dismissed selections cannot apply an older range', async t => {
  const env = await harness(t);
  await env.reply(0, hello);
  await env.move(0, 11); await env.tick(60);
  await env.move(3, 3);
  await env.reply(1, phrase);
  assert.equal(env.changes.at(-1).word, 'Hello');
  assert.equal(env.pending.at(-1), true);
  assert.match(env.text(), /Select at least one word/);
  await env.move(6, 11); await env.tick(60);
  await env.reply(2, world);
  assert.equal(env.changes.at(-1).word, 'world');
  assert.equal(env.pending.at(-1), false);
  assert.equal(env.text(), '');
  await env.move(0, 11); await env.tick(60);
  const count = env.changes.length;
  await env.unmount();
  await env.reply(3, phrase);
  assert.equal(env.changes.length, count);
});
