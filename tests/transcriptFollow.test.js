import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import React from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
const data = { videoId: 'abcdefghijk', trackId: '.en', title: 'Long transcript', language: 'English', tracks: [],
  segments: Array.from({ length: 1500 }, (_, index) => ({ start: index * 2, duration: 2, text: `Line ${index + 1}` })) };

async function harness(t, position) {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const windows = [], scrolls = [];
  // Model the public list contract: only the initial window mounts, and an
  // unmeasured distant index fails. Time passing does not render that index.
  class VirtualList extends React.Component {
    constructor(props) {
      super(props);
      this.first = props.initialScrollIndex ?? 0;
      this.measured = -1;
      windows.push(this);
    }
    scrollToIndex(params) {
      scrolls.push(params);
      if (params.index > this.measured) this.props.onScrollToIndexFailed({ index: params.index,
        averageItemLength: 80, highestMeasuredFrameIndex: this.measured });
    }
    render() {
      return React.createElement('FlatList', {}, this.props.data.slice(this.first, this.first + 10).map((item, offset) =>
        React.createElement(React.Fragment, { key: this.props.keyExtractor(item) }, this.props.renderItem({ item, index: this.first + offset }))));
    }
  }
  const compiled = await build({ entryPoints: ['src/components/TranscriptPanel.tsx'], bundle: true, write: false, platform: 'node', format: 'cjs', jsx: 'automatic', packages: 'external',
    plugins: [{ name: 'transcript-viewport', setup(builder) {
      builder.onResolve({ filter: /\/(theme|core\/transcripts|core\/vocabulary|IconButton|TranscriptWords|WordTranslationSheet|CaptionSelection)$/ }, args => ({ path: args.path, external: true }));
    } }],
  });
  const colors = new Proxy({}, { get: () => '#123456' });
  const native = { ...Object.fromEntries(['View', 'Text', 'Pressable', 'ScrollView', 'TextInput', 'Switch', 'ActivityIndicator', 'Modal'].map(name => [name, name])),
    StyleSheet: { create: value => value, absoluteFill: {}, hairlineWidth: 1 }, useWindowDimensions: () => ({ fontScale: 1 }), FlatList: VirtualList };
  const module = { exports: {} };
  Function('require', 'module', 'exports', compiled.outputFiles[0].text)(name => {
    if (name === 'react-native') return native;
    if (name === 'lucide-react-native') return new Proxy({}, { get: () => 'Icon' });
    if (name === '@react-native-clipboard/clipboard') return {};
    if (name.endsWith('/theme')) return { useThemeColors: () => colors, useThemeStyles: factory => factory(colors) };
    if (name.endsWith('/core/transcripts')) return { readTranscripts: async source => ({ total: source.segments.length,
      rows: source.segments.map((segment, index) => ({ start: segment.start, lines: [{ track: 0, index }] })),
      trackChoices: { sourceTracks: [], captionTracks: [], translations: [] } }) };
    if (name.endsWith('/core/vocabulary')) return {};
    if (name.endsWith('/IconButton')) return { IconButton: 'IconButton' };
    if (name.endsWith('/TranscriptWords')) return { TranscriptWords: ({ segmentIndex }) => React.createElement('Caption', { index: segmentIndex }) };
    if (name.endsWith('/WordTranslationSheet')) return { WordTranslationSheet: () => null };
    if (name.endsWith('/CaptionSelection')) return { CaptionSelection: () => null };
    return require(name);
  }, module, module.exports);
  let props = { data, position, loading: false, error: null, cached: false, pauseAfterLine: false, onClose() {}, onSeek() {} }, root;
  const element = () => React.createElement(module.exports.TranscriptPanel, props);
  await act(() => { root = create(element()); });
  await act(async () => { t.mock.timers.tick(0); });
  t.after(() => act(() => root.unmount()));
  return { windows, scrolls,
    list: () => windows.at(-1),
    captions: () => root.root.findAllByType('Caption').map(node => node.props.index),
    render: changes => act(() => { props = { ...props, ...changes }; root.update(element()); }),
    layout: () => act(() => windows.at(-1).props.onLayout()),
    measure: index => act(() => {
      const list = windows.at(-1);
      list.measured = Math.max(list.measured, index);
      list.props.renderItem({ item: list.props.data[index], index }).props.onLayout();
    }),
    drag: () => act(() => windows.at(-1).props.onScrollBeginDrag()),
    follow: () => act(() => root.root.findAllByType('Pressable').find(node => node.props.accessibilityLabel === 'Follow playback').props.onPress()),
    tick: ms => act(() => t.mock.timers.tick(ms)),
  };
}

test('opening late mounts the playing line before any rows are measured', async t => {
  const env = await harness(t, 2400.2);
  assert.ok(env.captions().includes(1200), 'the playing line is rendered immediately');
  assert.ok(!env.captions().includes(0), 'rendering earlier rows is unnecessary');
  await env.layout();
  await env.tick(10_000);
  assert.equal(env.windows.length, 1, 'waiting for native measurements does not cause remount or timer loops');
  await env.measure(1200);
  await env.tick(0);
  assert.deepEqual(env.scrolls.at(-1), { index: 1200, viewPosition: 0, animated: false });
});

test('a distant seek renders its target, and dragging cancels alignment until following resumes', async t => {
  const env = await harness(t, 0.2);
  await env.measure(0);
  await env.tick(0);
  await env.render({ position: 2900.2 });
  assert.ok(env.captions().includes(1450), 'the destination renders without depending on cached offsets or timer retries');
  await env.measure(1450);
  await env.drag();
  const count = env.scrolls.length;
  await env.tick(10_000);
  await env.render({ position: 2980.2 });
  assert.equal(env.scrolls.length, count, 'the old row layout and clock updates cannot override a drag');
  assert.ok(!env.captions().includes(1490));
  await env.follow();
  assert.ok(env.captions().includes(1490), 'following resumes at the latest playing line');
});

test('a distant seek reveals its caption even when cached cell measurements claim the index is known', async t => {
  const env = await harness(t, 0.2);
  await env.measure(1499);
  await env.tick(0);
  await env.render({ position: 2900.2 });
  assert.ok(env.captions().includes(1450), 'a successful scrollToIndex call with stale metrics cannot leave the destination unmounted');
});

async function visibilityHarness(t, frames = [{ y: 20, height: 180 }, { y: 200, height: 80 }, { y: 280, height: 80 }]) {
  const offsets = [], indices = [], pending = [];
  let offset = 0, deferred = false, list, root, stopped = 0;
  const measure = index => ({ measureInWindow(callback) {
    const read = () => {
      const frame = frames[index];
      if (frame.clipped) callback(0, 0, 0, 0);
      else callback(0, 100 + frame.y - offset, 300, frame.height);
    };
    if (deferred) pending.push(read); else read();
  } });
  class NativeList extends React.Component {
    constructor(props) { super(props); list = this; offset = 0; }
    getNativeScrollRef() { return { measureInWindow: callback => callback(0, 100, 300, 300) }; }
    scrollToOffset(value) {
      offsets.push(value);
      offset = value.offset;
      this.props.onScroll({ nativeEvent: { contentOffset: { y: offset } } });
    }
    scrollToIndex(value) { indices.push(value); }
    render() {
      return React.createElement('FlatList', {}, this.props.data.map((item, index) =>
        React.createElement(React.Fragment, { key: index }, this.props.renderItem({ item, index }))));
    }
  }
  const bundle = await build({ entryPoints: ['src/components/TranscriptList.tsx'], bundle: true, write: false,
    platform: 'node', format: 'cjs', jsx: 'automatic', packages: 'external' });
  const module = { exports: {} };
  Function('require', 'module', 'exports', bundle.outputFiles[0].text)(name =>
    name === 'react-native' ? { FlatList: NativeList, View: 'View' } : require(name), module, module.exports);
  let props = { rows: frames.map((_, index) => ({ start: index, lines: [{ track: 0, index }] })),
    activeIndex: 0, following: true, followRevision: 0, enabled: true, contentContainerStyle: {}, empty: null,
    onStopFollowing: () => stopped++,
    renderRow: (row, active, onFollow) => React.createElement('Caption', { index: row.lines[0].index, active, onFollow }) };
  const element = () => React.createElement(module.exports.TranscriptList, props);
  await act(() => { root = create(element(), { createNodeMock: element =>
    element.type === 'View' ? measure(element.props.children.props.index) : null }); });
  await act(() => list.props.onLayout());
  offsets.length = 0;
  indices.length = 0;
  t.after(() => act(() => root.unmount()));
  return { offsets, indices,
    offset: () => offset,
    render: changes => act(() => { props = { ...props, ...changes }; root.update(element()); }),
    remount: async () => {
      await act(() => { props = { ...props, rows: [...props.rows] }; root.update(element()); });
      await act(() => list.props.onLayout());
    },
    layout: () => act(() => list.props.onLayout()),
    line: (index, y, height = 24) => act(() => {
      root.root.findAllByType('Caption').find(node => node.props.index === index).props.onFollow({ node: measure(index), y, height });
    }),
    defer: () => { deferred = true; },
    flush: () => act(() => { deferred = false; pending.splice(0).forEach(read => read()); }),
    drag: () => act(() => list.props.onScrollBeginDrag()),
    stopped: () => stopped,
  };
}

test('following wrapped lines leaves a fully visible multi-line caption still', async t => {
  const env = await visibilityHarness(t);
  for (const y of [0, 24, 48, 72, 96, 120, 144, 156]) await env.line(0, y);
  assert.deepEqual(env.offsets, [], 'advancing the spoken line does not reposition visible text');
  assert.deepEqual(env.indices, []);
});

test('a new timestamped caption moves to the top even when it was already visible', async t => {
  const env = await visibilityHarness(t);
  await env.render({ activeIndex: 1 });
  assert.deepEqual(env.offsets, [{ offset: 200, animated: false }]);
  assert.equal(env.offset(), 200, 'the previous caption ends at the top edge and is hidden');
  for (const y of [0, 24, 48]) await env.line(1, y);
  await env.layout();
  assert.equal(env.offsets.length, 1, 'wrapped lines and layout events do not realign the caption');
  assert.deepEqual(env.indices, []);
});

test('caption changes align the new caption at the top in either direction', async t => {
  const env = await visibilityHarness(t);
  await env.render({ activeIndex: 2 });
  assert.deepEqual(env.offsets, [{ offset: 280, animated: false }]);
  await env.render({ activeIndex: 0 });
  assert.deepEqual(env.offsets.at(-1), { offset: 20, animated: false });
  assert.deepEqual(env.indices, []);
});

test('a long caption pages its spoken line to the top and leaves subsequent visible lines still', async t => {
  const env = await visibilityHarness(t, [{ y: 20, height: 1200 }]);
  assert.equal(env.offset(), 20, 'the long caption starts at the top');
  for (const y of [0, 120, 240, 264]) await env.line(0, y);
  assert.deepEqual(env.offsets, []);
  await env.line(0, 288);
  assert.deepEqual(env.offsets, [{ offset: 308, animated: false }], 'all visual lines before the spoken line are hidden');
  for (const y of [312, 336, 360, 384, 408, 432, 456, 480, 504, 528, 552]) await env.line(0, y);
  await env.layout();
  assert.equal(env.offsets.length, 1, 'following does not stick to the bottom and scroll on every wrapped line');
  await env.line(0, 576);
  await env.line(0, 576);
  await env.line(0, 0);
  assert.deepEqual(env.offsets, [308, 596, 20].map(offset => ({ offset, animated: false })),
    'the next page and a backward seek also place the spoken line at the top');
  assert.deepEqual(env.indices, []);
});

test('a spoken line received during caption alignment is followed after the caption reaches the top', async t => {
  const env = await visibilityHarness(t, [{ y: 20, height: 80 }, { y: 100, height: 1200 }]);
  env.defer();
  await env.render({ activeIndex: 1 });
  await env.line(1, 500);
  await env.flush();
  assert.deepEqual(env.offsets, [100, 600].map(offset => ({ offset, animated: false })));
  assert.equal(env.offset(), 600, 'a seek into a long caption finishes with its spoken line at the top');
});

test('a remounted pane starts with its own native scroll offset', async t => {
  const env = await visibilityHarness(t, [{ y: 8, height: 1200 }]);
  await env.line(0, 264);
  await env.line(0, 288);
  assert.deepEqual(env.offsets, [{ offset: 296, animated: false }]);
  await env.remount();
  assert.equal(env.offset(), 8, 'the new pane aligns using its own native offset');
  await env.line(0, 0);
  assert.equal(env.offsets.length, 2, 'the previous pane offset cannot pull a newly mounted caption away from the top');
});

test('a mounted destination with a clipped native view retains the index-jump fallback', async t => {
  const env = await visibilityHarness(t, [{ y: 20, height: 80 }, { y: 1000, height: 80, clipped: true }]);
  await env.render({ activeIndex: 1 });
  assert.deepEqual(env.indices, [{ index: 1, viewPosition: 0, animated: false }]);
  assert.deepEqual(env.offsets, []);
});

test('stale line measurements and a manual drag cannot scroll the pane', async t => {
  const env = await visibilityHarness(t);
  env.defer();
  await env.line(0, 500);
  await env.line(0, 48);
  await env.flush();
  assert.deepEqual(env.offsets, [], 'the latest visible line invalidates the earlier hidden line measurement');
  env.defer();
  await env.line(0, 500);
  await env.drag();
  await env.flush();
  assert.equal(env.stopped(), 1);
  assert.deepEqual(env.offsets, [], 'pending native measurement callbacks cannot override a drag');
});
