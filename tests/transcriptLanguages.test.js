import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import React from 'react';
import { act, create } from 'react-test-renderer';
import { build } from 'esbuild';

globalThis.IS_REACT_ACT_ENVIRONMENT = true;
const require = createRequire(import.meta.url);
const source = { id: '.en', label: 'English', language: 'en', automatic: false };
const supplied = { id: '.es', label: 'Spanish', language: 'es', automatic: false };
const automatic = { id: 'a.fr', label: 'French', language: 'fr', automatic: true };
const translated = { id: 'translate:ja:.en', label: 'Japanese · Translated from English', language: 'ja', automatic: false, translatedFrom: '.en' };
const data = { videoId: 'abcdefghijk', trackId: '.en', title: 'Example', language: 'English',
  tracks: [source, supplied, automatic, translated], segments: [{ start: 0, duration: 1, text: 'Hello' }] };

test('caption menus stay separate, keep selected chips removable, and enforce the comparison limit', async t => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const selections = [];
  const compiled = await build({ entryPoints: ['src/components/TranscriptPanel.tsx'], bundle: true, write: false, platform: 'node', format: 'cjs', jsx: 'automatic', packages: 'external',
    plugins: [{ name: 'transcript-presentation', setup(builder) {
      builder.onResolve({ filter: /\/(theme|core\/transcripts|core\/vocabulary|IconButton|TranscriptWords|WordTranslationSheet)$/ }, args => ({ path: args.path, external: true }));
    } }],
  });
  const colors = new Proxy({}, { get: () => '#123456' });
  const native = {
    View: 'View', Text: 'Text', Pressable: 'Pressable', ScrollView: 'ScrollView', TextInput: 'TextInput', Switch: 'Switch', ActivityIndicator: 'ActivityIndicator',
    StyleSheet: { create: styles => styles, absoluteFill: {}, hairlineWidth: 1 },
    useWindowDimensions: () => ({ fontScale: 1 }),
    Modal: ({ visible, children }) => visible ? React.createElement('Modal', {}, children) : null,
    FlatList: props => React.createElement('FlatList', {}, props.data.map((item, index) =>
      React.createElement(React.Fragment, { key: props.keyExtractor(item, index) }, props.renderItem({ item, index })))),
  };
  const module = { exports: {} };
  Function('require', 'module', 'exports', compiled.outputFiles[0].text)(name => {
    if (name === 'react-native') return native;
    if (name === 'lucide-react-native') return new Proxy({}, { get: () => 'Icon' });
    if (name === '@react-native-clipboard/clipboard') return {};
    if (name.endsWith('/theme')) return { useThemeColors: () => colors, useThemeStyles: factory => factory(colors) };
    if (name.endsWith('/core/transcripts')) return { readTranscripts: async () => ({ rows: [], total: 1,
      trackChoices: { sourceTracks: [source], captionTracks: [supplied], translations: [automatic, translated] } }) };
    if (name.endsWith('/core/vocabulary')) return {};
    if (name.endsWith('/IconButton')) return { IconButton: ({ label, onPress }) => React.createElement('Pressable', { accessibilityLabel: label, onPress }) };
    if (name.endsWith('/TranscriptWords')) return { TranscriptWords: () => null };
    if (name.endsWith('/WordTranslationSheet')) return { WordTranslationSheet: () => null };
    return require(name);
  }, module, module.exports);
  let props = { data, trackIds: ['.en'], loading: false, error: null, cached: false, position: null, pauseAfterLine: false,
    onClose() {}, onSelectTrack: id => selections.push(id) };
  let root;
  const element = () => React.createElement(module.exports.TranscriptPanel, props);
  await act(() => { root = create(element()); });
  t.after(() => act(() => root.unmount()));
  await act(async () => { t.mock.timers.tick(0); });
  const button = label => root.root.findAllByType('Pressable').find(node => node.props.accessibilityLabel === label);
  const checkboxes = node => node.findAllByType('Pressable').filter(node => node.props.accessibilityRole === 'checkbox');
  const labels = node => checkboxes(node).map(node => node.findByType('Text').children.join(''));
  const render = changes => act(() => { props = { ...props, ...changes }; root.update(element()); });

  assert.deepEqual(labels(root.root), ['1 · English']);
  await act(() => button('Caption translations').props.onPress());
  assert.deepEqual(labels(root.root.findByType('Modal')), ['Spanish']);
  await act(() => checkboxes(root.root.findByType('Modal'))[0].props.onPress());
  assert.deepEqual(selections, ['.es']);
  assert.equal(root.root.findAllByType('Modal').length, 0);

  await render({ trackIds: ['.en', '.es'] });
  assert.deepEqual(labels(root.root), ['1 · English', '2 · Spanish']);
  await act(() => button('Machine translations').props.onPress());
  assert.deepEqual(labels(root.root.findByType('Modal')), ['French · Auto', 'Japanese · Translated from English']);
  assert.ok(checkboxes(root.root.findByType('Modal')).every(node => node.props.disabled));
  await act(() => button('Close machine translations').props.onPress());
  await act(() => button('Spanish').props.onPress());
  assert.deepEqual(selections, ['.es', '.es']);

  await render({ trackIds: ['.en'] });
  await act(() => button('Machine translations').props.onPress());
  const machine = checkboxes(root.root.findByType('Modal'))[1];
  assert.equal(machine.props.disabled, false);
  await act(() => machine.props.onPress());
  assert.deepEqual(selections, ['.es', '.es', 'translate:ja:.en']);
  assert.equal(root.root.findAllByType('Modal').length, 0);
});
