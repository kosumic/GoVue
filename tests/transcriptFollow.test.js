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
  assert.deepEqual(env.scrolls.at(-1), { index: 1200, viewPosition: 0.3, animated: false });
});

test('a distant seek renders its target, and dragging cancels alignment until following resumes', async t => {
  const env = await harness(t, 0.2);
  await env.measure(0);
  await env.tick(0);
  await env.render({ position: 2900.2 });
  assert.ok(env.captions().includes(1450), 'a failed jump renders the destination without waiting for retries');
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
