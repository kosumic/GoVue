import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import React from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
globalThis.requestAnimationFrame = callback => setImmediate(callback);
globalThis.cancelAnimationFrame = clearImmediate;
const require = createRequire(import.meta.url);
const bundle = await build({ entryPoints: ['src/components/useChatViewport.ts'], bundle: true, write: false, platform: 'node', format: 'cjs', packages: 'external',
  plugins: [{ name: 'viewport', setup(builder) { builder.onResolve({ filter: /^react-native$/ }, () => ({ path: 'native', external: true })); } }],
});
const module = { exports: {} };
Function('require', 'module', 'exports', bundle.outputFiles[0].text)(name => name === 'native' ? { View: 'View' } : require(name), module, module.exports);
const messages = [{ id: 'one' }, { id: 'two' }, { id: 'three' }];
const scroll = (y, height = 1200) => ({ nativeEvent: { contentSize: { height }, layoutMeasurement: { height: 400 }, contentOffset: { y } } });

async function harness(t, store = new Map(), rows = [{ y: 0, height: 400 }, { y: 400, height: 400 }, { y: 800, height: 400 }], unmeasured = new Set()) {
  let viewport, root;
  const positions = [];
  const indices = [];
  const list = { scrollToOffset: value => positions.push(value.offset), scrollToEnd: () => positions.push('end'), scrollToIndex: value => indices.push(value.index) };
  function Probe() { viewport = module.exports.useChatViewport('chat', messages, store); viewport.list.current = list; return null; }
  await act(async () => { root = create(React.createElement(Probe)); });
  t.after(() => act(async () => root.unmount()));
  const invoke = (name, ...args) => act(async () => { viewport.listProps[name](...args); });
  await act(async () => {
    messages.forEach((item, i) => { if (!unmeasured.has(item.id)) viewport.listProps.CellRendererComponent({ item }).props.onLayout({ nativeEvent: { layout: rows[i] } }); });
    viewport.listProps.onLayout({ nativeEvent: { layout: { height: 400 } } });
    viewport.listProps.onContentSizeChange(400, rows.at(-1).y + rows.at(-1).height);
  });
  return { value: () => viewport, positions, indices, invoke, store };
}

test('programmatic scroll and layout growth preserve follow mode; drag and user momentum control it', async t => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const env = await harness(t);
  assert.equal(env.value().ready, false, 'wait until the requested position is confirmed');
  assert.equal(env.positions.at(-1), 800);
  await env.invoke('onScroll', scroll(800));
  assert.equal(env.value().ready, true);
  await env.invoke('onScroll', scroll(600));
  assert.equal(env.store.get('chat').followEnd, true, 'automatic movement does not change intent');
  await env.invoke('onContentSizeChange', 400, 1400);
  assert.equal(env.positions.at(-1), 1000);
  await env.invoke('onScroll', scroll(1000, 1400));
  await env.invoke('onScrollBeginDrag', scroll(1000, 1400));
  await env.invoke('onScroll', scroll(700, 1400));
  assert.equal(env.store.get('chat').followEnd, false);
  assert.equal(env.value().showJump, false, 'the jump button waits for scrolling to settle');
  await env.invoke('onScrollEndDrag', scroll(700, 1400));
  await env.invoke('onMomentumScrollBegin');
  await env.invoke('onScroll', scroll(500, 1400));
  await env.invoke('onMomentumScrollEnd', scroll(500, 1400));
  await act(async () => t.mock.timers.tick(200));
  assert.equal(env.value().showJump, true);
  const beforeGrowth = env.positions.length;
  await env.invoke('onContentSizeChange', 400, 1600);
  assert.equal(env.positions.length, beforeGrowth, 'content growth does not pull a reader to the end');
  await act(async () => env.value().jumpToLatest());
  assert.equal(env.positions.at(-1), 'end');
  assert.equal(env.value().showJump, false);
  assert.equal(env.store.get('chat').followEnd, true);
});

test('restoration uses the message anchor after layout changes and retains expansion', async t => {
  const store = new Map([['chat', { offset: 450, followEnd: false, anchor: { messageId: 'two', offset: 50 }, expanded: new Set(['reply:usage']) }]]);
  const env = await harness(t, store, [{ y: 0, height: 500 }, { y: 500, height: 400 }, { y: 900, height: 400 }]);
  assert.equal(env.positions.at(-1), 550, 'restoration follows the content rather than the old pixel offset');
  assert.equal(env.value().ready, false);
  assert.equal(env.value().expansion.expanded.has('reply:usage'), true);
  await env.invoke('onScroll', scroll(550, 1300));
  await env.invoke('onViewableItemsChanged', { viewableItems: [{ item: messages[1], isViewable: true }] });
  assert.equal(env.value().ready, true);
  assert.deepEqual(store.get('chat').anchor, { messageId: 'two', offset: 50 });
  await act(async () => env.value().expansion.setExpanded('reply:reasoning', true));
  assert.equal(store.get('chat').expanded.has('reply:reasoning'), true);
  await env.invoke('onScroll', scroll(900, 1300));
  assert.equal(store.get('chat').followEnd, false, 'automatic bottom positioning cannot re-enable follow mode');
  await env.invoke('onScrollBeginDrag', scroll(700, 1300));
  await env.invoke('onScroll', scroll(880, 1300));
  assert.equal(store.get('chat').followEnd, true, 'scrolling down near the end opts back in');
});

test('revealing a search result suspends following until Latest is requested', async t => {
  const env = await harness(t);
  await env.invoke('onScroll', scroll(800));
  await act(() => env.value().revealMessage('one'));
  assert.equal(env.positions.at(-1), 0);
  await env.invoke('onScroll', scroll(0));
  assert.equal(env.store.get('chat').followEnd, false);
  const count = env.positions.length;
  await env.invoke('onContentSizeChange', 400, 1600);
  assert.equal(env.positions.length, count, 'stream growth leaves the selected message in place');
  await act(() => env.value().jumpToLatest());
  assert.equal(env.positions.at(-1), 'end');
  assert.equal(env.store.get('chat').followEnd, true);
});

test('search can reveal an unmeasured virtualized message and refine its actual position', async t => {
  const env = await harness(t, new Map(), undefined, new Set(['two']));
  await env.invoke('onScroll', scroll(800));
  await act(() => env.value().revealMessage('two'));
  assert.equal(env.indices.at(-1), 1);
  await env.invoke('onScrollToIndexFailed', { averageItemLength: 400, index: 1 });
  assert.equal(env.positions.at(-1), 400);
  await act(() => env.value().listProps.CellRendererComponent({ item: messages[1] }).props.onLayout({ nativeEvent: { layout: { y: 450, height: 400 } } }));
  assert.equal(env.positions.at(-1), 442);
  await env.invoke('onScroll', scroll(442));
  assert.equal(env.store.get('chat').followEnd, false);
  const count = env.positions.length;
  await env.invoke('onContentSizeChange', 400, 1600);
  assert.equal(env.positions.length, count);
});
